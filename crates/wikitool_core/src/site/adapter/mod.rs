use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::{WikiConfig, load_config};
use crate::runtime::ResolvedPaths;
use crate::support::{compute_sha256, normalize_path, unix_timestamp};

use self::model::{
    AdapterSourceDocument, AuthoringRules, CategoryRules, CitationRules, ExtensionContractRule,
    LintRules, SiteAdapter, TemplateRules,
};

pub mod model;

const RESOLVED_SITE_ADAPTER_SCHEMA_VERSION: &str = "resolved_site_adapter_v1";
const SITE_ADAPTER_SCHEMA_VERSION: &str = "site_adapter_v2";
#[cfg(test)]
const MEDIAWIKI_GENERIC_ADAPTER_ID: &str = "mediawiki-generic";
const EMBEDDED_GENERIC_ADAPTER_PATH: &str = "<embedded:mediawiki-generic>";
const EMBEDDED_GENERIC_ADAPTER: &str =
    include_str!("../../../../../site_adapters/generic/site-adapter.toml");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicationPolicyIdentity {
    pub adapter_id: String,
    pub policy_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOwnedAdapterPath {
    pub absolute: PathBuf,
    pub project_relative: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SiteAdapterDocument {
    schema_version: String,
    adapter_id: String,
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

pub fn load_site_adapter(paths: &ResolvedPaths) -> Result<SiteAdapter> {
    let config = load_config(&paths.config_path)?;
    load_site_adapter_with_config(paths, &config)
}

pub fn publication_policy_identity(paths: &ResolvedPaths) -> Result<PublicationPolicyIdentity> {
    let config = load_config(&paths.config_path)?;
    publication_policy_identity_with_config(paths, &config)
}

pub fn publication_policy_identity_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<PublicationPolicyIdentity> {
    #[derive(Serialize)]
    struct FingerprintPayload<'a> {
        schema_version: &'a str,
        adapter_id: &'a str,
        docs_profile: &'a str,
        source_content_sha256s: Vec<&'a str>,
        authoring: &'a AuthoringRules,
        citations: &'a CitationRules,
        templates: &'a TemplateRules,
        categories: &'a CategoryRules,
        lint: &'a LintRules,
        extension_contracts: &'a [ExtensionContractRule],
    }

    let adapter = load_site_adapter_with_config(paths, config)?;
    let payload = FingerprintPayload {
        schema_version: &adapter.schema_version,
        adapter_id: &adapter.adapter_id,
        docs_profile: &adapter.docs_profile,
        source_content_sha256s: adapter
            .source_documents
            .iter()
            .map(|document| document.content_hash.as_str())
            .collect(),
        authoring: &adapter.authoring,
        citations: &adapter.citations,
        templates: &adapter.templates,
        categories: &adapter.categories,
        lint: &adapter.lint,
        extension_contracts: &adapter.extension_contracts,
    };
    let encoded =
        serde_json::to_string(&payload).context("failed to encode publication-policy identity")?;
    Ok(PublicationPolicyIdentity {
        adapter_id: adapter.adapter_id,
        policy_sha256: crate::support::compute_sha256(&encoded),
    })
}

pub fn load_site_adapter_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<SiteAdapter> {
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

    Ok(SiteAdapter {
        schema_version: RESOLVED_SITE_ADAPTER_SCHEMA_VERSION.to_string(),
        adapter_id: policy.adapter_id,
        docs_profile: policy.docs_profile,
        source_documents,
        authoring: policy.authoring,
        citations: policy.citations,
        templates: policy.templates,
        categories: policy.categories,
        lint: policy.lint,
        extension_contracts: policy.extension_contracts,
        resolved_at: unix_timestamp()?.to_string(),
    })
}

/// Resolve the docs profile for a command invocation.
///
/// An explicit operator selection takes precedence. Otherwise the configured
/// site adapter supplies the default; projects without an adapter use the
/// embedded generic MediaWiki adapter.
pub fn resolve_docs_profile_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
    requested: Option<&str>,
) -> Result<String> {
    if let Some(requested) = requested {
        return Ok(requested.to_string());
    }
    Ok(load_site_adapter_with_config(paths, config)?.docs_profile)
}

fn validate_site_adapter(policy: &SiteAdapterDocument) -> Result<()> {
    if policy.schema_version != SITE_ADAPTER_SCHEMA_VERSION {
        bail!(
            "unsupported site-adapter schema: expected {SITE_ADAPTER_SCHEMA_VERSION}, got {}",
            policy.schema_version
        );
    }
    if policy.adapter_id.trim().is_empty() {
        bail!("site-adapter adapter_id must not be empty");
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

pub fn resolve_project_owned_adapter_path(
    paths: &ResolvedPaths,
    configured_path: &Path,
) -> Result<ProjectOwnedAdapterPath> {
    if configured_path.is_absolute() {
        bail!(
            "adapter.path must be project-relative so the adapter remains snapshot-safe and relocatable"
        );
    }
    let project_root = fs::canonicalize(&paths.project_root).with_context(|| {
        format!(
            "failed to canonicalize project root {}",
            normalize_path(&paths.project_root)
        )
    })?;
    let candidate = paths.project_root.join(configured_path);
    if !candidate.is_file() {
        bail!(
            "configured site adapter does not exist or is not a file: {}",
            normalize_path(&candidate)
        );
    }
    let absolute = fs::canonicalize(&candidate).with_context(|| {
        format!(
            "failed to canonicalize configured site adapter {}",
            normalize_path(&candidate)
        )
    })?;
    if !absolute.starts_with(&project_root) {
        bail!(
            "configured site adapter resolves outside the project root: {}",
            normalize_path(&absolute)
        );
    }
    let project_relative = absolute
        .strip_prefix(&project_root)
        .map(normalize_path)
        .context("configured site adapter could not be made project-relative")?;
    Ok(ProjectOwnedAdapterPath {
        absolute,
        project_relative,
    })
}

fn load_adapter_document(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<(String, AdapterSourceDocument, Option<PathBuf>)> {
    let Some(configured_path) = config.adapter.path.as_deref() else {
        return Ok((
            EMBEDDED_GENERIC_ADAPTER.to_string(),
            AdapterSourceDocument {
                relative_path: EMBEDDED_GENERIC_ADAPTER_PATH.to_string(),
                content_hash: compute_sha256(EMBEDDED_GENERIC_ADAPTER),
            },
            None,
        ));
    };
    let configured_path = configured_path.trim();
    if configured_path.is_empty() {
        bail!("adapter.path must not be empty");
    }
    let resolved = resolve_project_owned_adapter_path(paths, Path::new(configured_path))?;
    let absolute = resolved.absolute;
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read site adapter {}", absolute.display()))?;
    let document = AdapterSourceDocument {
        relative_path: display_source_path(paths, &absolute),
        content_hash: compute_sha256(&content),
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
) -> Result<AdapterSourceDocument> {
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
    Ok(AdapterSourceDocument {
        relative_path: display_source_path(paths, &absolute),
        content_hash: compute_sha256(&content),
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
        let adapter = load_site_adapter_with_config(&paths(temp.path()), &WikiConfig::default())
            .expect("generic adapter");
        assert_eq!(adapter.adapter_id, MEDIAWIKI_GENERIC_ADAPTER_ID);
        assert_eq!(
            adapter.source_documents[0].relative_path,
            EMBEDDED_GENERIC_ADAPTER_PATH
        );
        assert_eq!(adapter.source_documents[0].content_hash.len(), 64);
        assert!(adapter.templates.infobox_preferences.is_empty());
    }

    #[test]
    fn publication_policy_identity_binds_exact_guidance_bytes() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let adapter_dir = temp.path().join("site-adapter");
        fs::create_dir_all(&adapter_dir).expect("adapter dir");
        fs::write(
            adapter_dir.join("site-adapter.toml"),
            EMBEDDED_GENERIC_ADAPTER.replace(
                "guidance_documents = []",
                "guidance_documents = [\"editorial.md\"]",
            ),
        )
        .expect("policy");
        let guidance_path = adapter_dir.join("editorial.md");
        fs::write(&guidance_path, "# First policy\n").expect("guidance");
        let config = WikiConfig {
            adapter: crate::config::AdapterSection {
                path: Some("site-adapter/site-adapter.toml".to_string()),
            },
            ..WikiConfig::default()
        };

        let first = publication_policy_identity_with_config(&paths, &config)
            .expect("first publication identity");
        fs::write(&guidance_path, "# Second policy\n").expect("changed guidance");
        let second = publication_policy_identity_with_config(&paths, &config)
            .expect("second publication identity");

        assert_eq!(first.policy_sha256.len(), 64);
        assert_ne!(first.policy_sha256, second.policy_sha256);
    }

    #[test]
    fn docs_profile_resolution_uses_adapter_default_and_preserves_explicit_choice() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let generic = resolve_docs_profile_with_config(&paths, &WikiConfig::default(), None)
            .expect("generic docs profile");
        assert_eq!(
            generic,
            crate::catalog::status::DEFAULT_DOCS_PROFILE,
            "the embedded generic adapter remains the no-adapter fallback"
        );

        let adapter_dir = temp.path().join("site-adapter");
        fs::create_dir_all(&adapter_dir).expect("adapter dir");
        fs::write(
            adapter_dir.join("site-adapter.toml"),
            EMBEDDED_GENERIC_ADAPTER.replace(
                "docs_profile = \"mw-1.44-authoring\"",
                "docs_profile = \"site-specific-authoring\"",
            ),
        )
        .expect("adapter");
        let config = WikiConfig {
            adapter: crate::config::AdapterSection {
                path: Some("site-adapter/site-adapter.toml".to_string()),
            },
            ..WikiConfig::default()
        };

        assert_eq!(
            resolve_docs_profile_with_config(&paths, &config, None).expect("adapter docs profile"),
            "site-specific-authoring"
        );
        assert_eq!(
            resolve_docs_profile_with_config(&paths, &config, Some("operator-selected"))
                .expect("explicit docs profile"),
            "operator-selected"
        );
    }

    #[test]
    fn explicit_adapter_path_is_required_exactly_and_unknown_fields_fail() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let missing = WikiConfig {
            adapter: crate::config::AdapterSection {
                path: Some("site-adapter/site-adapter.toml".to_string()),
            },
            ..WikiConfig::default()
        };
        let error = load_site_adapter_with_config(&paths, &missing)
            .expect_err("missing configured adapter must fail");
        assert!(error.to_string().contains("configured site adapter"));

        let adapter_dir = temp.path().join("site-adapter");
        fs::create_dir_all(&adapter_dir).expect("adapter dir");
        fs::write(
            adapter_dir.join("site-adapter.toml"),
            EMBEDDED_GENERIC_ADAPTER.replace(
                "schema_version = \"site_adapter_v2\"",
                "schema_version = \"site_adapter_v2\"\nunknown_key = true",
            ),
        )
        .expect("adapter");
        let error =
            load_site_adapter_with_config(&paths, &missing).expect_err("unknown field must fail");
        assert!(
            error
                .to_string()
                .contains("failed to parse typed site-adapter policy")
        );

        fs::write(
            adapter_dir.join("site-adapter.toml"),
            EMBEDDED_GENERIC_ADAPTER.replace(
                "source_review_rules = []",
                "source_review_rules = [{ label = \"Wikipedia\", host = \"https://wikipedia.org\", reason = \"review\" }]",
            ),
        )
        .expect("invalid host adapter");
        let error = load_site_adapter_with_config(&paths, &missing)
            .expect_err("non-host review syntax must fail");
        assert!(error.to_string().contains("normalized lowercase hostname"));
    }

    #[test]
    fn configured_adapter_must_be_project_relative_and_canonically_confined() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let outside_dir = temp.path().join("outside-adapter");
        fs::create_dir_all(&project_root).expect("project root");
        fs::create_dir_all(&outside_dir).expect("outside adapter dir");
        let paths = paths(&project_root);
        let outside_policy = outside_dir.join("site-adapter.toml");
        fs::write(&outside_policy, EMBEDDED_GENERIC_ADAPTER).expect("outside adapter");

        let absolute = WikiConfig {
            adapter: crate::config::AdapterSection {
                path: Some(normalize_path(&outside_policy)),
            },
            ..WikiConfig::default()
        };
        let error = load_site_adapter_with_config(&paths, &absolute)
            .expect_err("absolute adapter config must fail closed");
        assert!(error.to_string().contains("must be project-relative"));

        let escape = WikiConfig {
            adapter: crate::config::AdapterSection {
                path: Some("../outside-adapter/site-adapter.toml".to_string()),
            },
            ..WikiConfig::default()
        };
        let error = load_site_adapter_with_config(&paths, &escape)
            .expect_err("relative project escape must fail closed");
        assert!(
            error
                .to_string()
                .contains("resolves outside the project root")
        );
    }

    #[test]
    fn configured_adapter_rejects_symlink_escape_from_project() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("project root");
        let paths = paths(&project_root);
        let outside_policy = temp.path().join("outside-site-adapter.toml");
        fs::write(&outside_policy, EMBEDDED_GENERIC_ADAPTER).expect("outside adapter");
        let link = project_root.join("linked-site-adapter.toml");
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside_policy, &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_file(&outside_policy, &link);
        if let Err(error) = link_result {
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314)
            {
                return;
            }
            panic!("create adapter symlink: {error}");
        }

        let config = WikiConfig {
            adapter: crate::config::AdapterSection {
                path: Some("linked-site-adapter.toml".to_string()),
            },
            ..WikiConfig::default()
        };
        let error = load_site_adapter_with_config(&paths, &config)
            .expect_err("symlink adapter escape must fail closed");
        assert!(
            error
                .to_string()
                .contains("resolves outside the project root")
        );
    }

    #[test]
    fn adapter_bundle_contains_only_declared_resources() {
        let temp = tempdir().expect("tempdir");
        let policy_path = temp.path().join("site-adapter.toml");
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
        let policy_path = temp.path().join("site-adapter.toml");
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
