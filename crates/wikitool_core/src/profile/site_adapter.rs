use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

use crate::config::{WikiConfig, load_config};
use crate::knowledge::status::KNOWLEDGE_GENERATION;
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{compute_hash, normalize_path, unix_timestamp};

use super::rules::{
    AuthoringRules, CategoryRules, CitationRules, ExtensionContractRule, LintRules,
    ProfileSourceDocument, SiteProfile, TemplateRules, WikiProfileSnapshot,
};
use super::template_catalog::load_template_catalog;
use super::wiki_capabilities::{
    load_wiki_capabilities_with_config, sync_wiki_capabilities_with_config,
};

const SITE_PROFILE_ARTIFACT_KIND: &str = "site_profile";
const SITE_PROFILE_SCHEMA_VERSION: &str = "site_profile_v1";
const SITE_ADAPTER_SCHEMA_VERSION: &str = "site_adapter_v1";
const MEDIAWIKI_GENERIC_PROFILE_ID: &str = "mediawiki-generic";
const EMBEDDED_GENERIC_ADAPTER_PATH: &str = "<embedded:mediawiki-generic>";
const EMBEDDED_GENERIC_ADAPTER: &str = include_str!("../../../../config/generic-site-adapter.toml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SiteAdapterDocument {
    schema_version: String,
    profile_id: String,
    base_profile_id: String,
    docs_profile: String,
    #[serde(default)]
    guidance_documents: Vec<String>,
    authoring: AuthoringRules,
    citations: CitationRules,
    templates: TemplateRules,
    categories: CategoryRules,
    lint: LintRules,
    #[serde(default)]
    extension_contracts: Vec<ExtensionContractRule>,
}

pub fn build_site_profile(paths: &ResolvedPaths) -> Result<SiteProfile> {
    let config = load_config(&paths.config_path)?;
    build_site_profile_with_config(paths, &config)
}

pub fn build_site_profile_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<SiteProfile> {
    let (policy_source, policy_document, adapter_dir) = load_adapter_document(paths, config)?;
    let policy = parse_site_adapter(&policy_source)?;

    let mut source_documents = vec![policy_document];
    if !policy.guidance_documents.is_empty() && adapter_dir.is_none() {
        bail!("embedded generic adapter cannot name external guidance documents");
    }
    if let Some(adapter_dir) = adapter_dir {
        for relative_path in &policy.guidance_documents {
            source_documents.push(load_guidance_document(paths, &adapter_dir, relative_path)?);
        }
    }

    Ok(SiteProfile {
        schema_version: SITE_PROFILE_SCHEMA_VERSION.to_string(),
        profile_id: policy.profile_id,
        base_profile_id: policy.base_profile_id,
        docs_profile: policy.docs_profile,
        source_documents,
        authoring: policy.authoring,
        citations: policy.citations,
        templates: policy.templates,
        categories: policy.categories,
        lint: policy.lint,
        extension_contracts: policy.extension_contracts,
        refreshed_at: unix_timestamp()?.to_string(),
    })
}

fn validate_site_adapter(policy: &SiteAdapterDocument) -> Result<()> {
    if policy.schema_version != SITE_ADAPTER_SCHEMA_VERSION {
        bail!(
            "unsupported site-adapter schema: expected {SITE_ADAPTER_SCHEMA_VERSION}, got {}",
            policy.schema_version
        );
    }
    if policy.profile_id.trim().is_empty() {
        bail!("site-adapter profile_id must not be empty");
    }
    if policy.base_profile_id != MEDIAWIKI_GENERIC_PROFILE_ID {
        bail!(
            "site-adapter base mismatch: expected {MEDIAWIKI_GENERIC_PROFILE_ID}, got {}",
            policy.base_profile_id
        );
    }
    if policy.docs_profile.trim().is_empty() {
        bail!("site-adapter docs_profile must not be empty");
    }
    if policy.authoring.require_article_quality_banner {
        if policy.authoring.article_quality_template.is_none() {
            bail!(
                "site-adapter requires an article quality banner but names no article_quality_template"
            );
        }
        if policy.authoring.article_quality_default_state.is_none() {
            bail!(
                "site-adapter requires an article quality banner but names no article_quality_default_state"
            );
        }
    }
    for rule in &policy.citations.source_review_rules {
        if rule.label.trim().is_empty()
            || rule.host.trim().is_empty()
            || rule.reason.trim().is_empty()
        {
            bail!("site-adapter source review rules require label, host, and reason");
        }
        let host = rule.host.trim();
        if host != rule.host
            || host != host.to_ascii_lowercase()
            || host.starts_with('.')
            || host.ends_with('.')
            || host.contains(['/', ':', '@', '*'])
            || host.chars().any(char::is_whitespace)
            || reqwest::Url::parse(&format!("https://{host}/"))
                .ok()
                .and_then(|url| url.host_str().map(ToString::to_string))
                .as_deref()
                != Some(host)
        {
            bail!(
                "site-adapter source review host must be a normalized lowercase hostname: {}",
                rule.host
            );
        }
    }
    Ok(())
}

fn parse_site_adapter(source: &str) -> Result<SiteAdapterDocument> {
    let policy: SiteAdapterDocument =
        toml::from_str(source).context("failed to parse typed site-adapter policy")?;
    validate_site_adapter(&policy)?;
    Ok(policy)
}

/// Validate a standalone site-adapter bundle and return the exact files declared by it.
///
/// The returned list always starts with `policy_path`; subsequent entries are the guidance
/// documents named by `guidance_documents`. Undeclared neighboring files are intentionally not
/// part of the bundle.
pub fn site_adapter_resource_paths(policy_path: &Path) -> Result<Vec<PathBuf>> {
    if !policy_path.is_file() {
        bail!(
            "site-adapter policy does not exist or is not a file: {}",
            normalize_path(policy_path)
        );
    }
    let adapter_dir = policy_path
        .parent()
        .context("site-adapter policy has no parent directory")?;
    let canonical_adapter_dir = fs::canonicalize(adapter_dir).with_context(|| {
        format!(
            "failed to canonicalize site-adapter directory {}",
            normalize_path(adapter_dir)
        )
    })?;
    let canonical_policy = fs::canonicalize(policy_path).with_context(|| {
        format!(
            "failed to canonicalize site-adapter policy {}",
            normalize_path(policy_path)
        )
    })?;
    if !canonical_policy.starts_with(&canonical_adapter_dir) {
        bail!("site-adapter policy resolves outside its adapter directory");
    }

    let source = fs::read_to_string(policy_path)
        .with_context(|| format!("failed to read site adapter {}", policy_path.display()))?;
    let policy = parse_site_adapter(&source)?;
    let mut resources = vec![policy_path.to_path_buf()];
    let mut declared_paths = BTreeSet::new();
    declared_paths.insert(canonical_policy);
    for relative_path in &policy.guidance_documents {
        let path = resolve_guidance_path(adapter_dir, relative_path)?;
        let canonical_path = fs::canonicalize(&path).with_context(|| {
            format!(
                "failed to canonicalize adapter guidance {}",
                normalize_path(&path)
            )
        })?;
        if !canonical_path.starts_with(&canonical_adapter_dir) {
            bail!(
                "site-adapter guidance document resolves outside the adapter directory: {relative_path}"
            );
        }
        if !declared_paths.insert(canonical_path) {
            bail!("site-adapter resource is declared more than once: {relative_path}");
        }
        fs::read_to_string(&path)
            .with_context(|| format!("failed to read adapter guidance {}", path.display()))?;
        resources.push(path);
    }
    Ok(resources)
}

fn load_adapter_document(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<(String, ProfileSourceDocument, Option<PathBuf>)> {
    let Some(configured_path) = config.adapter.path.as_deref() else {
        return Ok((
            EMBEDDED_GENERIC_ADAPTER.to_string(),
            ProfileSourceDocument {
                relative_path: EMBEDDED_GENERIC_ADAPTER_PATH.to_string(),
                content_hash: compute_hash(EMBEDDED_GENERIC_ADAPTER),
            },
            None,
        ));
    };
    let configured_path = configured_path.trim();
    if configured_path.is_empty() {
        bail!("adapter.path must not be empty");
    }
    let path = Path::new(configured_path);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    if !absolute.is_file() {
        bail!(
            "configured site adapter does not exist or is not a file: {}",
            normalize_path(&absolute)
        );
    }
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read site adapter {}", absolute.display()))?;
    let document = ProfileSourceDocument {
        relative_path: display_source_path(paths, &absolute),
        content_hash: compute_hash(&content),
    };
    let adapter_dir = absolute
        .parent()
        .map(Path::to_path_buf)
        .context("configured site adapter has no parent directory")?;
    Ok((content, document, Some(adapter_dir)))
}

fn load_guidance_document(
    paths: &ResolvedPaths,
    adapter_dir: &Path,
    relative_path: &str,
) -> Result<ProfileSourceDocument> {
    let absolute = resolve_guidance_path(adapter_dir, relative_path)?;
    let canonical_adapter_dir = fs::canonicalize(adapter_dir).with_context(|| {
        format!(
            "failed to canonicalize site-adapter directory {}",
            normalize_path(adapter_dir)
        )
    })?;
    let canonical_absolute = fs::canonicalize(&absolute).with_context(|| {
        format!(
            "failed to canonicalize adapter guidance {}",
            normalize_path(&absolute)
        )
    })?;
    if !canonical_absolute.starts_with(&canonical_adapter_dir) {
        bail!(
            "site-adapter guidance document resolves outside the adapter directory: {relative_path}"
        );
    }
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read adapter guidance {}", absolute.display()))?;
    Ok(ProfileSourceDocument {
        relative_path: display_source_path(paths, &absolute),
        content_hash: compute_hash(&content),
    })
}

fn resolve_guidance_path(adapter_dir: &Path, relative_path: &str) -> Result<PathBuf> {
    let relative = Path::new(relative_path);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!(
            "site-adapter guidance path must stay relative to the adapter directory: {relative_path}"
        );
    }
    let absolute = adapter_dir.join(relative);
    if !absolute.is_file() {
        bail!(
            "site-adapter guidance document does not exist or is not a file: {}",
            normalize_path(&absolute)
        );
    }
    Ok(absolute)
}

fn display_source_path(paths: &ResolvedPaths, path: &Path) -> String {
    path.strip_prefix(&paths.project_root)
        .map(normalize_path)
        .unwrap_or_else(|_| normalize_path(path))
}

pub fn sync_site_profile(paths: &ResolvedPaths) -> Result<SiteProfile> {
    let config = load_config(&paths.config_path)?;
    sync_site_profile_with_config(paths, &config)
}

pub fn sync_site_profile_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<SiteProfile> {
    let profile = build_site_profile_with_config(paths, config)?;
    store_site_profile(paths, &profile)?;
    Ok(profile)
}

pub fn load_site_profile_artifact(
    paths: &ResolvedPaths,
    profile_id: &str,
) -> Result<Option<SiteProfile>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let profile_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM runtime_artifacts
             WHERE artifact_key = ?1",
            params![site_profile_artifact_key(profile_id)],
            |row| row.get(0),
        )
        .optional()
        .with_context(|| format!("failed to load site profile for {profile_id}"))?;

    profile_json
        .map(|value| decode_current_site_profile(&value))
        .transpose()
        .map(Option::flatten)
}

pub fn load_latest_site_profile(paths: &ResolvedPaths) -> Result<Option<SiteProfile>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let profile_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM runtime_artifacts
             WHERE artifact_kind = ?1
             ORDER BY built_at_unix DESC
             LIMIT 1",
            params![SITE_PROFILE_ARTIFACT_KIND],
            |row| row.get(0),
        )
        .optional()
        .context("failed to load latest site profile")?;

    profile_json
        .map(|value| decode_current_site_profile(&value))
        .transpose()
        .map(Option::flatten)
}

fn decode_current_site_profile(encoded: &str) -> Result<Option<SiteProfile>> {
    let value: serde_json::Value =
        serde_json::from_str(encoded).context("failed to decode site profile envelope")?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("site profile envelope has no schema_version"))?;
    if schema_version != SITE_PROFILE_SCHEMA_VERSION {
        return Ok(None);
    }
    serde_json::from_value(value)
        .map(Some)
        .context("failed to decode current site profile")
}

pub fn load_or_build_site_profile(paths: &ResolvedPaths) -> Result<SiteProfile> {
    // The adapter is small and authoritative. Read it on every command so an
    // edited policy cannot be shadowed by a stale runtime artifact.
    build_site_profile(paths)
}

pub fn load_wiki_profile_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<WikiProfileSnapshot> {
    let profile = build_site_profile_with_config(paths, config)?;
    build_wiki_profile_snapshot(paths, config, profile, false)
}

pub fn sync_wiki_profile_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<WikiProfileSnapshot> {
    let profile = sync_site_profile_with_config(paths, config)?;
    build_wiki_profile_snapshot(paths, config, profile, true)
}

fn build_wiki_profile_snapshot(
    paths: &ResolvedPaths,
    config: &WikiConfig,
    adapter: SiteProfile,
    sync_capabilities: bool,
) -> Result<WikiProfileSnapshot> {
    let capabilities = if sync_capabilities {
        if config.api_url_owned().is_some() {
            Some(sync_wiki_capabilities_with_config(paths, config)?)
        } else {
            load_wiki_capabilities_with_config(paths, config)?
        }
    } else {
        load_wiki_capabilities_with_config(paths, config)?
    };
    let template_catalog =
        load_template_catalog(paths, &adapter.profile_id)?.map(|catalog| catalog.summary());

    Ok(WikiProfileSnapshot {
        base_profile_id: adapter.base_profile_id.clone(),
        adapter,
        capabilities,
        template_catalog,
    })
}

fn store_site_profile(paths: &ResolvedPaths, profile: &SiteProfile) -> Result<()> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let metadata_json =
        serde_json::to_string_pretty(profile).context("failed to serialize site profile")?;
    let row_count = profile
        .profile_template_titles()
        .len()
        .saturating_add(profile.citations.source_review_rules.len())
        .saturating_add(profile.extension_contracts.len());
    let built_at_unix = unix_timestamp()?;

    connection
        .execute(
            "INSERT INTO runtime_artifacts (
                artifact_key,
                artifact_kind,
                profile,
                schema_generation,
                built_at_unix,
                row_count,
                metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(artifact_key) DO UPDATE SET
                artifact_kind = excluded.artifact_kind,
                profile = excluded.profile,
                schema_generation = excluded.schema_generation,
                built_at_unix = excluded.built_at_unix,
                row_count = excluded.row_count,
                metadata_json = excluded.metadata_json",
            params![
                site_profile_artifact_key(&profile.profile_id),
                SITE_PROFILE_ARTIFACT_KIND,
                Some(profile.profile_id.as_str()),
                KNOWLEDGE_GENERATION,
                i64::try_from(built_at_unix).context("artifact timestamp does not fit into i64")?,
                i64::try_from(row_count).context("artifact row count does not fit into i64")?,
                metadata_json,
            ],
        )
        .with_context(|| format!("failed to store site profile for {}", profile.profile_id))?;

    Ok(())
}

fn site_profile_artifact_key(profile_id: &str) -> String {
    format!("site_profile:{}", profile_id.trim().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::ValueSource;
    use tempfile::tempdir;

    fn paths(root: &Path) -> ResolvedPaths {
        let state_dir = root.join(".wikitool");
        fs::create_dir_all(&state_dir).expect("state");
        ResolvedPaths {
            project_root: root.to_path_buf(),
            wiki_content_dir: root.join("wiki_content"),
            templates_dir: root.join("templates"),
            state_dir: state_dir.clone(),
            data_dir: state_dir.join("data"),
            db_path: state_dir.join("data/wikitool.db"),
            config_path: state_dir.join("config.toml"),
            parser_config_path: state_dir.join("parser-config.json"),
            root_source: ValueSource::Default,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn missing_adapter_uses_embedded_generic_policy() {
        let temp = tempdir().expect("tempdir");
        let profile = build_site_profile_with_config(&paths(temp.path()), &WikiConfig::default())
            .expect("generic profile");
        assert_eq!(profile.profile_id, MEDIAWIKI_GENERIC_PROFILE_ID);
        assert_eq!(
            profile.source_documents[0].relative_path,
            EMBEDDED_GENERIC_ADAPTER_PATH
        );
        assert!(profile.templates.infobox_preferences.is_empty());
    }

    #[test]
    fn explicit_adapter_path_is_required_exactly_and_unknown_fields_fail() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let missing = WikiConfig {
            adapter: crate::config::AdapterSection {
                path: Some("site-adapter/profile.toml".to_string()),
            },
            ..WikiConfig::default()
        };
        let error = build_site_profile_with_config(&paths, &missing)
            .expect_err("missing configured adapter must fail");
        assert!(error.to_string().contains("configured site adapter"));

        let adapter_dir = temp.path().join("site-adapter");
        fs::create_dir_all(&adapter_dir).expect("adapter dir");
        fs::write(
            adapter_dir.join("profile.toml"),
            EMBEDDED_GENERIC_ADAPTER.replace(
                "schema_version = \"site_adapter_v1\"",
                "schema_version = \"site_adapter_v1\"\nunknown_key = true",
            ),
        )
        .expect("adapter");
        let error =
            build_site_profile_with_config(&paths, &missing).expect_err("unknown field must fail");
        assert!(
            error
                .to_string()
                .contains("failed to parse typed site-adapter policy")
        );

        fs::write(
            adapter_dir.join("profile.toml"),
            EMBEDDED_GENERIC_ADAPTER.replace(
                "source_review_rules = []",
                "source_review_rules = [{ label = \"Wikipedia\", host = \"https://wikipedia.org\", reason = \"review\" }]",
            ),
        )
        .expect("invalid host adapter");
        let error = build_site_profile_with_config(&paths, &missing)
            .expect_err("non-host review syntax must fail");
        assert!(error.to_string().contains("normalized lowercase hostname"));
    }

    #[test]
    fn adapter_bundle_contains_only_declared_resources() {
        let temp = tempdir().expect("tempdir");
        let policy_path = temp.path().join("profile.toml");
        fs::write(
            &policy_path,
            EMBEDDED_GENERIC_ADAPTER.replace(
                "guidance_documents = []",
                "guidance_documents = [\"editorial.md\"]",
            ),
        )
        .expect("policy");
        let editorial_path = temp.path().join("editorial.md");
        fs::write(&editorial_path, "# Editorial\n").expect("guidance");
        fs::write(temp.path().join("private-notes.md"), "must not ship\n")
            .expect("undeclared file");

        let resources = site_adapter_resource_paths(&policy_path).expect("valid bundle");

        assert_eq!(resources, vec![policy_path, editorial_path]);
    }

    #[test]
    fn adapter_bundle_rejects_duplicate_and_traversing_resources() {
        let temp = tempdir().expect("tempdir");
        let policy_path = temp.path().join("profile.toml");
        fs::write(temp.path().join("editorial.md"), "# Editorial\n").expect("guidance");
        fs::write(
            &policy_path,
            EMBEDDED_GENERIC_ADAPTER.replace(
                "guidance_documents = []",
                "guidance_documents = [\"editorial.md\", \"editorial.md\"]",
            ),
        )
        .expect("duplicate policy");
        let error = site_adapter_resource_paths(&policy_path)
            .expect_err("duplicate resource must fail closed");
        assert!(error.to_string().contains("declared more than once"));

        fs::write(
            &policy_path,
            EMBEDDED_GENERIC_ADAPTER.replace(
                "guidance_documents = []",
                "guidance_documents = [\"../outside.md\"]",
            ),
        )
        .expect("traversing policy");
        let error = site_adapter_resource_paths(&policy_path)
            .expect_err("traversing resource must fail closed");
        assert!(error.to_string().contains("must stay relative"));
    }
}
