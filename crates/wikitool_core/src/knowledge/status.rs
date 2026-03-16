use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{table_exists, unix_timestamp};

pub const DEFAULT_DOCS_PROFILE: &str = "remilia-mw-1.44";
pub const KNOWLEDGE_GENERATION: &str = concat!("knowledge-v", env!("CARGO_PKG_VERSION"));

const CONTENT_INDEX_ARTIFACT_KEY: &str = "content_index";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KnowledgeArtifact {
    pub artifact_key: String,
    pub artifact_kind: String,
    pub profile: Option<String>,
    pub schema_generation: String,
    pub built_at_unix: u64,
    pub row_count: usize,
    pub metadata_json: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeReadinessLevel {
    NotReady,
    ContentReady,
    AuthoringReady,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KnowledgeStatusReport {
    pub docs_profile_requested: String,
    pub readiness: KnowledgeReadinessLevel,
    pub degradations: Vec<String>,
    pub knowledge_generation: String,
    pub db_exists: bool,
    pub content_index_ready: bool,
    pub docs_profile_ready: bool,
    pub index_rows: usize,
    pub docs_profile_corpora: usize,
    pub content_index_artifact: Option<KnowledgeArtifact>,
    pub docs_profile_artifact: Option<KnowledgeArtifact>,
}

pub fn docs_profile_artifact_key(profile: &str) -> String {
    format!("docs_profile:{}", normalize_profile(profile))
}

pub fn record_content_index_artifact(
    connection: &Connection,
    row_count: usize,
    metadata_json: &str,
) -> Result<()> {
    upsert_artifact(
        connection,
        CONTENT_INDEX_ARTIFACT_KEY,
        "content_index",
        None,
        row_count,
        metadata_json,
    )
}

pub fn record_docs_profile_artifact(
    connection: &Connection,
    profile: &str,
    row_count: usize,
    metadata_json: &str,
) -> Result<()> {
    let normalized_profile = normalize_profile(profile);
    upsert_artifact(
        connection,
        &docs_profile_artifact_key(&normalized_profile),
        "docs_profile",
        Some(&normalized_profile),
        row_count,
        metadata_json,
    )
}

pub fn knowledge_status(
    paths: &ResolvedPaths,
    docs_profile: &str,
) -> Result<KnowledgeStatusReport> {
    let requested_profile = normalize_profile(docs_profile);
    if !paths.db_path.exists() {
        return Ok(KnowledgeStatusReport {
            docs_profile_requested: requested_profile,
            readiness: KnowledgeReadinessLevel::NotReady,
            degradations: vec![
                "content_index_missing".to_string(),
                "docs_profile_missing".to_string(),
            ],
            knowledge_generation: KNOWLEDGE_GENERATION.to_string(),
            db_exists: false,
            content_index_ready: false,
            docs_profile_ready: false,
            index_rows: 0,
            docs_profile_corpora: 0,
            content_index_artifact: None,
            docs_profile_artifact: None,
        });
    }

    let connection = open_initialized_database_connection(&paths.db_path)?;
    let index_rows = load_index_rows(&connection)?;
    let docs_profile_corpora = load_docs_profile_corpora(&connection, &requested_profile)?;
    let content_index_artifact = load_artifact(&connection, CONTENT_INDEX_ARTIFACT_KEY)?;
    let docs_profile_artifact =
        load_artifact(&connection, &docs_profile_artifact_key(&requested_profile))?;

    let mut degradations = Vec::new();
    if index_rows == 0 {
        degradations.push("content_index_missing".to_string());
    } else if content_index_artifact.is_none() {
        degradations.push("content_index_manifest_missing".to_string());
    }
    if docs_profile_corpora == 0 {
        degradations.push("docs_profile_missing".to_string());
    } else if docs_profile_artifact.is_none() {
        degradations.push("docs_profile_manifest_missing".to_string());
    }

    let content_index_ready = index_rows > 0 && content_index_artifact.is_some();
    let docs_profile_ready = docs_profile_corpora > 0 && docs_profile_artifact.is_some();
    let readiness = if content_index_ready && docs_profile_ready {
        KnowledgeReadinessLevel::AuthoringReady
    } else if content_index_ready {
        KnowledgeReadinessLevel::ContentReady
    } else {
        KnowledgeReadinessLevel::NotReady
    };

    Ok(KnowledgeStatusReport {
        docs_profile_requested: requested_profile,
        readiness,
        degradations,
        knowledge_generation: KNOWLEDGE_GENERATION.to_string(),
        db_exists: true,
        content_index_ready,
        docs_profile_ready,
        index_rows,
        docs_profile_corpora,
        content_index_artifact,
        docs_profile_artifact,
    })
}

fn normalize_profile(profile: &str) -> String {
    profile.trim().to_ascii_lowercase()
}

fn upsert_artifact(
    connection: &Connection,
    artifact_key: &str,
    artifact_kind: &str,
    profile: Option<&str>,
    row_count: usize,
    metadata_json: &str,
) -> Result<()> {
    let built_at_unix = unix_timestamp()?;
    connection
        .execute(
            "INSERT INTO knowledge_artifacts (
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
                artifact_key,
                artifact_kind,
                profile,
                KNOWLEDGE_GENERATION,
                i64::try_from(built_at_unix).context("artifact timestamp does not fit into i64")?,
                i64::try_from(row_count).context("artifact row count does not fit into i64")?,
                metadata_json,
            ],
        )
        .with_context(|| format!("failed to upsert knowledge artifact {artifact_key}"))?;
    Ok(())
}

fn load_index_rows(connection: &Connection) -> Result<usize> {
    if !table_exists(connection, "indexed_pages")? {
        return Ok(0);
    }
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM indexed_pages", [], |row| row.get(0))
        .context("failed to count indexed_pages rows")?;
    usize::try_from(count).context("indexed_pages row count does not fit into usize")
}

fn load_docs_profile_corpora(connection: &Connection, docs_profile: &str) -> Result<usize> {
    if !table_exists(connection, "docs_corpora")? {
        return Ok(0);
    }
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM docs_corpora
             WHERE corpus_kind = 'profile' AND lower(source_profile) = lower(?1)",
            params![docs_profile],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to count docs corpora for profile {docs_profile}"))?;
    usize::try_from(count).context("docs corpora count does not fit into usize")
}

fn load_artifact(connection: &Connection, artifact_key: &str) -> Result<Option<KnowledgeArtifact>> {
    if !table_exists(connection, "knowledge_artifacts")? {
        return Ok(None);
    }

    connection
        .query_row(
            "SELECT
                artifact_key,
                artifact_kind,
                profile,
                schema_generation,
                built_at_unix,
                row_count,
                metadata_json
             FROM knowledge_artifacts
             WHERE artifact_key = ?1",
            params![artifact_key],
            |row| {
                let built_at_unix: i64 = row.get(4)?;
                let row_count: i64 = row.get(5)?;
                Ok(KnowledgeArtifact {
                    artifact_key: row.get(0)?,
                    artifact_kind: row.get(1)?,
                    profile: row.get(2)?,
                    schema_generation: row.get(3)?,
                    built_at_unix: u64::try_from(built_at_unix).unwrap_or_default(),
                    row_count: usize::try_from(row_count).unwrap_or_default(),
                    metadata_json: row.get(6)?,
                })
            },
        )
        .optional()
        .with_context(|| format!("failed to load knowledge artifact {artifact_key}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::filesystem::ScanOptions;
    use crate::knowledge::content_index::rebuild_index;
    use crate::runtime::{ResolvedPaths, ValueSource};

    fn test_paths() -> (tempfile::TempDir, ResolvedPaths) {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let wiki_content_dir = project_root.join("wiki_content");
        let templates_dir = project_root.join("templates");
        let state_dir = project_root.join(".wikitool");
        let data_dir = state_dir.join("data");
        fs::create_dir_all(wiki_content_dir.join("Main")).expect("create wiki_content");
        fs::create_dir_all(&templates_dir).expect("create templates");
        fs::create_dir_all(&data_dir).expect("create data");

        (
            temp,
            ResolvedPaths {
                project_root: project_root.clone(),
                wiki_content_dir,
                templates_dir,
                state_dir: state_dir.clone(),
                data_dir: data_dir.clone(),
                db_path: data_dir.join("wikitool.db"),
                config_path: state_dir.join("config.toml"),
                parser_config_path: state_dir.join("parser.toml"),
                root_source: ValueSource::Flag,
                data_source: ValueSource::Flag,
                config_source: ValueSource::Flag,
            },
        )
    }

    fn write_main_page(paths: &ResolvedPaths, title: &str, content: &str) {
        let filename = format!("{}.wiki", title.replace(' ', "_"));
        fs::write(paths.wiki_content_dir.join("Main").join(filename), content).expect("write page");
    }

    #[test]
    fn knowledge_status_reports_missing_database() {
        let (_temp, paths) = test_paths();
        let status = knowledge_status(&paths, DEFAULT_DOCS_PROFILE).expect("status");
        assert_eq!(status.readiness, KnowledgeReadinessLevel::NotReady);
        assert!(
            status
                .degradations
                .contains(&"content_index_missing".to_string())
        );
        assert!(
            status
                .degradations
                .contains(&"docs_profile_missing".to_string())
        );
    }

    #[test]
    fn knowledge_status_reports_content_ready_without_docs_profile() {
        let (_temp, paths) = test_paths();
        write_main_page(&paths, "Alpha", "'''Alpha''' article.\n[[Category:People]]");
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let status = knowledge_status(&paths, DEFAULT_DOCS_PROFILE).expect("status");
        assert_eq!(status.readiness, KnowledgeReadinessLevel::ContentReady);
        assert!(status.content_index_ready);
        assert!(!status.docs_profile_ready);
        assert!(
            status
                .degradations
                .contains(&"docs_profile_missing".to_string())
        );
    }

    #[test]
    fn knowledge_status_reports_authoring_ready_with_docs_profile_artifact() {
        let (_temp, paths) = test_paths();
        write_main_page(&paths, "Alpha", "'''Alpha''' article.\n[[Category:People]]");
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let connection = open_initialized_database_connection(&paths.db_path).expect("open db");
        connection
            .execute(
                "INSERT INTO docs_corpora (
                    corpus_id,
                    corpus_kind,
                    label,
                    source_wiki,
                    source_version,
                    source_profile,
                    technical_type,
                    refresh_kind,
                    refresh_spec,
                    pages_count,
                    sections_count,
                    symbols_count,
                    examples_count,
                    fetched_at_unix,
                    expires_at_unix
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    "profile:remilia-mw-1.44",
                    "profile",
                    "Remilia MediaWiki 1.44 authoring reference",
                    "mediawiki.org",
                    "1.44",
                    DEFAULT_DOCS_PROFILE,
                    "profile",
                    "profile",
                    "{}",
                    1i64,
                    1i64,
                    0i64,
                    0i64,
                    1i64,
                    2i64,
                ],
            )
            .expect("insert docs corpus");
        record_docs_profile_artifact(&connection, DEFAULT_DOCS_PROFILE, 1, "{}")
            .expect("record artifact");

        let status = knowledge_status(&paths, DEFAULT_DOCS_PROFILE).expect("status");
        assert_eq!(status.readiness, KnowledgeReadinessLevel::AuthoringReady);
        assert!(status.content_index_ready);
        assert!(status.docs_profile_ready);
        assert!(status.degradations.is_empty());
    }
}
