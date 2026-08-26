use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

use crate::config::WikiConfig;
use crate::knowledge::status::{DEFAULT_DOCS_PROFILE, KNOWLEDGE_GENERATION};
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{compute_hash, normalize_path, unix_timestamp};

use super::rules::{
    AuthoringRules, CategoryRules, CitationRules, ExtensionContractRule, GoldenSetRules, LintRules,
    ProfileOverlay, ProfileSourceDocument, RemiliaRules, WikiProfileSnapshot,
};
use super::template_catalog::load_template_catalog;
use super::wiki_capabilities::{
    load_wiki_capabilities_with_config, sync_wiki_capabilities_with_config,
};

const PROFILE_OVERLAY_ARTIFACT_KIND: &str = "profile_overlay";
const PROFILE_OVERLAY_SCHEMA_VERSION: &str = "profile_overlay_v4";
const PROFILE_POLICY_SCHEMA_VERSION: &str = "profile_policy_v2";
const REMILIA_PROFILE_ID: &str = "remilia";
const MEDIAWIKI_GENERIC_PROFILE_ID: &str = "mediawiki-generic";

const PROFILE_POLICY_PATH: &str = "tools/wikitool/ai-pack/writing_context/profile.toml";
const ARTICLE_STRUCTURE_PATH: &str = "tools/wikitool/ai-pack/writing_context/article_structure.md";
const STYLE_RULES_PATH: &str = "tools/wikitool/ai-pack/writing_context/style_rules.md";
const WRITING_GUIDE_PATH: &str = "tools/wikitool/ai-pack/writing_context/writing_guide.md";
const EXTENSIONS_PATH: &str = "tools/wikitool/ai-pack/writing_context/extensions.md";

#[derive(Debug, Deserialize)]
struct ProfilePolicyDocument {
    schema_version: String,
    profile_id: String,
    base_profile_id: String,
    docs_profile: String,
    authoring: AuthoringRules,
    citations: CitationRules,
    remilia: RemiliaRules,
    categories: CategoryRules,
    lint: LintRules,
    golden_set: GoldenSetRules,
    extension_contracts: Vec<ExtensionContractRule>,
}
pub fn build_remilia_profile_overlay(paths: &ResolvedPaths) -> Result<ProfileOverlay> {
    let (policy_source, policy_document) = load_source_document(paths, PROFILE_POLICY_PATH)?;
    let policy: ProfilePolicyDocument = toml::from_str(&policy_source)
        .with_context(|| format!("failed to parse typed profile policy {PROFILE_POLICY_PATH}"))?;
    validate_profile_policy(&policy)?;
    let (_, article_structure_source) = load_source_document(paths, ARTICLE_STRUCTURE_PATH)?;
    let (_, style_rules_source) = load_source_document(paths, STYLE_RULES_PATH)?;
    let (_, writing_guide_source) = load_source_document(paths, WRITING_GUIDE_PATH)?;
    let (_, extensions_source) = load_source_document(paths, EXTENSIONS_PATH)?;

    Ok(ProfileOverlay {
        schema_version: PROFILE_OVERLAY_SCHEMA_VERSION.to_string(),
        profile_id: policy.profile_id,
        base_profile_id: policy.base_profile_id,
        docs_profile: policy.docs_profile,
        source_documents: vec![
            policy_document,
            article_structure_source,
            style_rules_source,
            writing_guide_source,
            extensions_source,
        ],
        authoring: policy.authoring,
        citations: policy.citations,
        remilia: policy.remilia,
        categories: policy.categories,
        lint: policy.lint,
        extension_contracts: policy.extension_contracts,
        golden_set: policy.golden_set,
        refreshed_at: unix_timestamp()?.to_string(),
    })
}

fn validate_profile_policy(policy: &ProfilePolicyDocument) -> Result<()> {
    if policy.schema_version != PROFILE_POLICY_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported profile policy schema: expected {PROFILE_POLICY_SCHEMA_VERSION}, got {}",
            policy.schema_version
        );
    }
    if policy.profile_id != REMILIA_PROFILE_ID {
        anyhow::bail!(
            "profile policy id mismatch: expected {REMILIA_PROFILE_ID}, got {}",
            policy.profile_id
        );
    }
    if policy.base_profile_id != MEDIAWIKI_GENERIC_PROFILE_ID {
        anyhow::bail!(
            "profile policy base mismatch: expected {MEDIAWIKI_GENERIC_PROFILE_ID}, got {}",
            policy.base_profile_id
        );
    }
    if policy.docs_profile != DEFAULT_DOCS_PROFILE {
        anyhow::bail!(
            "profile policy docs profile mismatch: expected {DEFAULT_DOCS_PROFILE}, got {}",
            policy.docs_profile
        );
    }
    Ok(())
}

pub fn sync_remilia_profile_overlay(paths: &ResolvedPaths) -> Result<ProfileOverlay> {
    let overlay = build_remilia_profile_overlay(paths)?;
    store_profile_overlay(paths, &overlay)?;
    Ok(overlay)
}

pub fn load_profile_overlay(
    paths: &ResolvedPaths,
    profile_id: &str,
) -> Result<Option<ProfileOverlay>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let overlay_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM runtime_artifacts
             WHERE artifact_key = ?1",
            params![profile_overlay_artifact_key(profile_id)],
            |row| row.get(0),
        )
        .optional()
        .with_context(|| format!("failed to load profile overlay for {profile_id}"))?;

    overlay_json
        .map(|value| decode_current_profile_overlay(&value))
        .transpose()
        .map(Option::flatten)
}

pub fn load_latest_profile_overlay(paths: &ResolvedPaths) -> Result<Option<ProfileOverlay>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let overlay_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM runtime_artifacts
             WHERE artifact_kind = ?1
             ORDER BY built_at_unix DESC
             LIMIT 1",
            params![PROFILE_OVERLAY_ARTIFACT_KIND],
            |row| row.get(0),
        )
        .optional()
        .context("failed to load latest profile overlay")?;

    overlay_json
        .map(|value| decode_current_profile_overlay(&value))
        .transpose()
        .map(Option::flatten)
}

fn decode_current_profile_overlay(encoded: &str) -> Result<Option<ProfileOverlay>> {
    let value: serde_json::Value =
        serde_json::from_str(encoded).context("failed to decode profile overlay envelope")?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("profile overlay envelope has no schema_version"))?;
    if schema_version != PROFILE_OVERLAY_SCHEMA_VERSION {
        return Ok(None);
    }
    serde_json::from_value(value)
        .map(Some)
        .context("failed to decode current profile overlay")
}

pub fn load_or_build_remilia_profile_overlay(paths: &ResolvedPaths) -> Result<ProfileOverlay> {
    if let Some(overlay) = load_profile_overlay(paths, REMILIA_PROFILE_ID)?
        && overlay.schema_version == PROFILE_OVERLAY_SCHEMA_VERSION
    {
        return Ok(overlay);
    }
    build_remilia_profile_overlay(paths)
}

pub fn load_wiki_profile_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<WikiProfileSnapshot> {
    let overlay = load_or_build_remilia_profile_overlay(paths)?;
    build_wiki_profile_snapshot(paths, config, overlay, false)
}

pub fn sync_wiki_profile_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<WikiProfileSnapshot> {
    let overlay = sync_remilia_profile_overlay(paths)?;
    build_wiki_profile_snapshot(paths, config, overlay, true)
}

fn build_wiki_profile_snapshot(
    paths: &ResolvedPaths,
    config: &WikiConfig,
    overlay: ProfileOverlay,
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
        load_template_catalog(paths, &overlay.profile_id)?.map(|catalog| catalog.summary());

    Ok(WikiProfileSnapshot {
        base_profile_id: overlay.base_profile_id.clone(),
        overlay,
        capabilities,
        template_catalog,
    })
}

fn store_profile_overlay(paths: &ResolvedPaths, overlay: &ProfileOverlay) -> Result<()> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let metadata_json =
        serde_json::to_string_pretty(overlay).context("failed to serialize profile overlay")?;
    let row_count = overlay
        .profile_template_titles()
        .len()
        .saturating_add(overlay.lint.synthetic_phrase_prompts.len())
        .saturating_add(overlay.citations.unreliable_sources.len());
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
                profile_overlay_artifact_key(&overlay.profile_id),
                PROFILE_OVERLAY_ARTIFACT_KIND,
                Some(overlay.profile_id.as_str()),
                KNOWLEDGE_GENERATION,
                i64::try_from(built_at_unix).context("artifact timestamp does not fit into i64")?,
                i64::try_from(row_count).context("artifact row count does not fit into i64")?,
                metadata_json,
            ],
        )
        .with_context(|| format!("failed to store profile overlay for {}", overlay.profile_id))?;

    Ok(())
}

fn profile_overlay_artifact_key(profile_id: &str) -> String {
    format!("profile_overlay:{}", profile_id.trim().to_ascii_lowercase())
}

fn load_source_document(
    paths: &ResolvedPaths,
    relative_path: &str,
) -> Result<(String, ProfileSourceDocument)> {
    let path = resolve_source_document_path(paths, relative_path)?;
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok((
        content.clone(),
        ProfileSourceDocument {
            relative_path: normalize_path(path),
            content_hash: compute_hash(&content),
        },
    ))
}

fn resolve_source_document_path(
    paths: &ResolvedPaths,
    relative_path: &str,
) -> Result<std::path::PathBuf> {
    let relative = Path::new(relative_path);
    let file_name = relative
        .file_name()
        .context("profile source document path is missing a file name")?;
    let mut candidates = Vec::new();
    candidates.push(paths.project_root.join(relative));
    candidates.push(paths.project_root.join("writing_context").join(file_name));

    if let Ok(executable) = env::current_exe() {
        for ancestor in executable.ancestors().take(8) {
            candidates.push(ancestor.join(relative));
            candidates.push(ancestor.join("ai-pack/writing_context").join(file_name));
            candidates.push(ancestor.join("writing_context").join(file_name));
        }
    }

    candidates.sort();
    candidates.dedup();
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    let fallback = paths.project_root.join(relative);
    Err(anyhow::anyhow!("failed to read {}", fallback.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE_POLICY: &str = include_str!("../../../../ai-pack/writing_context/profile.toml");

    #[test]
    fn typed_profile_policy_is_complete_and_valid() {
        let policy: ProfilePolicyDocument = toml::from_str(PROFILE_POLICY).expect("profile policy");
        validate_profile_policy(&policy).expect("valid profile policy");
        assert_eq!(policy.profile_id, "remilia");
        assert!(policy.authoring.require_short_description);
        assert!(policy.categories.preferred_categories.is_empty());
        assert!(
            policy
                .lint
                .discouraged_relationship_headings
                .iter()
                .any(|heading| heading == "Relation to Remilia")
        );
        assert!(
            policy
                .lint
                .discouraged_lead_relationship_terms
                .iter()
                .any(|term| term == "Charlotte Fang")
        );
        assert!(
            policy
                .citations
                .preferred_templates
                .iter()
                .any(|rule| rule.template_title == "Template:Cite web")
        );
        assert!(
            policy
                .extension_contracts
                .iter()
                .any(|contract| contract.name == "tabber")
        );
    }

    #[test]
    fn stale_overlay_schema_is_invalidated_before_payload_decode() {
        let stale = r#"{"schema_version":"profile_overlay_v3"}"#;
        assert_eq!(
            decode_current_profile_overlay(stale).expect("stale overlay is readable"),
            None
        );
    }

    #[test]
    fn malformed_current_overlay_fails_loudly() {
        let malformed = r#"{"schema_version":"profile_overlay_v4"}"#;
        assert!(decode_current_profile_overlay(malformed).is_err());
    }
}
