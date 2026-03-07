use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

use crate::runtime::ResolvedPaths;
use crate::support::ensure_db_parent;

pub const LOCAL_DB_POLICY_MESSAGE: &str = "The local wikitool DB is disposable. Delete `.wikitool/data/wikitool.db` and rerun the relevant sync/import command if you need a clean rebuild.";

const DB_SCHEMA_SQL: &str = include_str!("schema.sql");

const REQUIRED_REFERENCE_COLUMNS: &[&str] = &[
    "citation_profile",
    "citation_family",
    "primary_template_title",
    "source_type",
    "source_origin",
    "source_family",
    "authority_kind",
    "source_authority",
    "reference_title",
    "source_container",
    "source_author",
    "source_domain",
    "source_date",
    "canonical_url",
    "identifier_keys",
    "identifier_entries",
    "source_urls",
    "retrieval_signals",
];

const REQUIRED_REFERENCE_AUTHORITY_COLUMNS: &[&str] = &[
    "source_title",
    "source_namespace",
    "section_heading",
    "citation_profile",
    "citation_family",
    "source_type",
    "source_origin",
    "source_family",
    "authority_kind",
    "authority_key",
    "authority_label",
    "primary_template_title",
    "source_domain",
    "source_container",
    "source_author",
    "identifier_keys",
    "summary_text",
    "retrieval_text",
];

const REQUIRED_REFERENCE_IDENTIFIER_COLUMNS: &[&str] = &[
    "source_title",
    "source_namespace",
    "section_heading",
    "citation_profile",
    "citation_family",
    "source_type",
    "source_origin",
    "source_family",
    "authority_key",
    "authority_label",
    "identifier_key",
    "identifier_value",
    "normalized_value",
    "summary_text",
];

const REQUIRED_ALIAS_COLUMNS: &[&str] = &[
    "alias_title",
    "canonical_title",
    "canonical_namespace",
    "source_relative_path",
];

const REQUIRED_SECTION_COLUMNS: &[&str] = &[
    "section_index",
    "source_title",
    "source_namespace",
    "section_heading",
    "section_level",
    "summary_text",
    "section_text",
    "token_estimate",
];

const REQUIRED_TEMPLATE_EXAMPLE_COLUMNS: &[&str] = &[
    "template_title",
    "source_relative_path",
    "source_title",
    "invocation_index",
    "example_wikitext",
    "parameter_keys",
    "token_estimate",
];

const REQUIRED_MEDIA_COLUMNS: &[&str] = &[
    "media_index",
    "source_title",
    "source_namespace",
    "section_heading",
    "file_title",
    "media_kind",
    "caption_text",
    "options_text",
    "token_estimate",
];

const REQUIRED_SEMANTIC_COLUMNS: &[&str] = &[
    "source_title",
    "source_namespace",
    "summary_text",
    "section_headings",
    "category_titles",
    "template_titles",
    "template_parameter_keys",
    "link_titles",
    "reference_titles",
    "reference_containers",
    "reference_domains",
    "reference_source_families",
    "reference_authorities",
    "reference_identifiers",
    "media_titles",
    "media_captions",
    "template_implementation_titles",
    "semantic_text",
    "token_estimate",
];

const REQUIRED_TEMPLATE_IMPLEMENTATION_COLUMNS: &[&str] = &[
    "template_title",
    "implementation_page_title",
    "implementation_namespace",
    "source_relative_path",
    "role",
];

const REQUIRED_DOCS_CORPORA_COLUMNS: &[&str] = &[
    "corpus_kind",
    "label",
    "source_wiki",
    "source_version",
    "source_profile",
    "technical_type",
    "refresh_kind",
    "refresh_spec",
    "pages_count",
    "sections_count",
    "symbols_count",
    "examples_count",
    "fetched_at_unix",
    "expires_at_unix",
];

const REQUIRED_DOCS_PAGE_COLUMNS: &[&str] = &[
    "page_title",
    "normalized_title_key",
    "page_namespace",
    "doc_type",
    "title_aliases",
    "local_path",
    "raw_content",
    "normalized_content",
    "content_hash",
    "summary_text",
    "semantic_text",
    "fetched_at_unix",
    "token_estimate",
];

const REQUIRED_DOCS_SECTION_COLUMNS: &[&str] = &[
    "page_title",
    "section_index",
    "section_level",
    "section_heading",
    "summary_text",
    "section_text",
    "semantic_text",
    "token_estimate",
];

const REQUIRED_DOCS_SYMBOL_COLUMNS: &[&str] = &[
    "page_title",
    "symbol_index",
    "symbol_kind",
    "symbol_name",
    "normalized_symbol_key",
    "aliases",
    "section_heading",
    "signature_text",
    "summary_text",
    "detail_text",
    "retrieval_text",
    "token_estimate",
];

const REQUIRED_DOCS_EXAMPLE_COLUMNS: &[&str] = &[
    "page_title",
    "example_index",
    "example_kind",
    "section_heading",
    "language_hint",
    "summary_text",
    "example_text",
    "retrieval_text",
    "token_estimate",
];

const REQUIRED_DOCS_LINK_COLUMNS: &[&str] = &[
    "page_title",
    "link_index",
    "target_title",
    "relation_kind",
    "display_text",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseSchemaState {
    Missing,
    Ready,
    Incompatible { reason: String },
}

pub fn ensure_database_schema(paths: &ResolvedPaths) -> Result<()> {
    let connection = open_database_connection(&paths.db_path)?;
    ensure_database_schema_connection(&connection)
        .with_context(|| schema_reset_hint(&paths.db_path))
}

pub fn schema_state(paths: &ResolvedPaths) -> Result<DatabaseSchemaState> {
    if !paths.db_path.exists() {
        return Ok(DatabaseSchemaState::Missing);
    }

    let connection = open_database_connection(&paths.db_path)?;
    match ensure_database_schema_connection(&connection) {
        Ok(()) => Ok(DatabaseSchemaState::Ready),
        Err(error) => Ok(DatabaseSchemaState::Incompatible {
            reason: error.to_string(),
        }),
    }
}

pub fn open_initialized_database_connection(db_path: &Path) -> Result<Connection> {
    let connection = open_database_connection(db_path)?;
    ensure_database_schema_connection(&connection).with_context(|| schema_reset_hint(db_path))?;
    Ok(connection)
}

pub fn open_database_connection(db_path: &Path) -> Result<Connection> {
    ensure_db_parent(db_path)?;
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .context("failed to set sqlite busy timeout")?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .context("failed to enable foreign_keys pragma")?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .context("failed to enable WAL journal mode")?;
    Ok(connection)
}

pub fn ensure_database_schema_connection(connection: &Connection) -> Result<()> {
    validate_existing_schema_compatibility(connection)?;
    connection
        .execute_batch(DB_SCHEMA_SQL)
        .context("failed to initialize database schema")?;
    validate_disposable_schema(connection)
}

fn validate_disposable_schema(connection: &Connection) -> Result<()> {
    require_columns(connection, "indexed_page_aliases", REQUIRED_ALIAS_COLUMNS)?;
    require_columns(
        connection,
        "indexed_page_sections",
        REQUIRED_SECTION_COLUMNS,
    )?;
    require_columns(
        connection,
        "indexed_template_examples",
        REQUIRED_TEMPLATE_EXAMPLE_COLUMNS,
    )?;
    require_columns(
        connection,
        "indexed_page_references",
        REQUIRED_REFERENCE_COLUMNS,
    )?;
    require_columns(
        connection,
        "indexed_reference_authorities",
        REQUIRED_REFERENCE_AUTHORITY_COLUMNS,
    )?;
    require_columns(
        connection,
        "indexed_reference_identifiers",
        REQUIRED_REFERENCE_IDENTIFIER_COLUMNS,
    )?;
    require_columns(connection, "indexed_page_media", REQUIRED_MEDIA_COLUMNS)?;
    require_columns(
        connection,
        "indexed_page_semantics",
        REQUIRED_SEMANTIC_COLUMNS,
    )?;
    require_columns(
        connection,
        "indexed_template_implementation_pages",
        REQUIRED_TEMPLATE_IMPLEMENTATION_COLUMNS,
    )?;
    require_columns(connection, "docs_corpora", REQUIRED_DOCS_CORPORA_COLUMNS)?;
    require_columns(connection, "docs_pages", REQUIRED_DOCS_PAGE_COLUMNS)?;
    require_columns(connection, "docs_sections", REQUIRED_DOCS_SECTION_COLUMNS)?;
    require_columns(connection, "docs_symbols", REQUIRED_DOCS_SYMBOL_COLUMNS)?;
    require_columns(connection, "docs_examples", REQUIRED_DOCS_EXAMPLE_COLUMNS)?;
    require_columns(connection, "docs_links", REQUIRED_DOCS_LINK_COLUMNS)
}

fn validate_existing_schema_compatibility(connection: &Connection) -> Result<()> {
    require_columns_if_table_exists(connection, "indexed_page_aliases", REQUIRED_ALIAS_COLUMNS)?;
    require_columns_if_table_exists(
        connection,
        "indexed_page_sections",
        REQUIRED_SECTION_COLUMNS,
    )?;
    require_columns_if_table_exists(
        connection,
        "indexed_template_examples",
        REQUIRED_TEMPLATE_EXAMPLE_COLUMNS,
    )?;
    require_columns_if_table_exists(
        connection,
        "indexed_page_references",
        REQUIRED_REFERENCE_COLUMNS,
    )?;
    require_columns_if_table_exists(
        connection,
        "indexed_reference_authorities",
        REQUIRED_REFERENCE_AUTHORITY_COLUMNS,
    )?;
    require_columns_if_table_exists(
        connection,
        "indexed_reference_identifiers",
        REQUIRED_REFERENCE_IDENTIFIER_COLUMNS,
    )?;
    require_columns_if_table_exists(connection, "indexed_page_media", REQUIRED_MEDIA_COLUMNS)?;
    require_columns_if_table_exists(
        connection,
        "indexed_page_semantics",
        REQUIRED_SEMANTIC_COLUMNS,
    )?;
    require_columns_if_table_exists(
        connection,
        "indexed_template_implementation_pages",
        REQUIRED_TEMPLATE_IMPLEMENTATION_COLUMNS,
    )?;
    require_columns_if_table_exists(connection, "docs_corpora", REQUIRED_DOCS_CORPORA_COLUMNS)?;
    require_columns_if_table_exists(connection, "docs_pages", REQUIRED_DOCS_PAGE_COLUMNS)?;
    require_columns_if_table_exists(connection, "docs_sections", REQUIRED_DOCS_SECTION_COLUMNS)?;
    require_columns_if_table_exists(connection, "docs_symbols", REQUIRED_DOCS_SYMBOL_COLUMNS)?;
    require_columns_if_table_exists(connection, "docs_examples", REQUIRED_DOCS_EXAMPLE_COLUMNS)?;
    require_columns_if_table_exists(connection, "docs_links", REQUIRED_DOCS_LINK_COLUMNS)
}

fn require_columns_if_table_exists(
    connection: &Connection,
    table_name: &str,
    required_columns: &[&str],
) -> Result<()> {
    if table_exists(connection, table_name)? {
        require_columns(connection, table_name, required_columns)?;
    }
    Ok(())
}

fn require_columns(
    connection: &Connection,
    table_name: &str,
    required_columns: &[&str],
) -> Result<()> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .with_context(|| format!("failed to inspect schema for {table_name}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .with_context(|| format!("failed to read table info for {table_name}"))?;

    let mut present = std::collections::BTreeSet::new();
    for row in rows {
        present.insert(row.with_context(|| format!("failed to decode column for {table_name}"))?);
    }
    if present.is_empty() {
        bail!("database schema is missing required table `{table_name}`");
    }

    let missing = required_columns
        .iter()
        .filter(|column| !present.contains(**column))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "existing table `{table_name}` uses an older disposable schema; missing columns: {}",
            missing.join(", ")
        );
    }

    Ok(())
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool> {
    let exists: i64 = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM sqlite_master
                WHERE type IN ('table', 'view') AND name = ?1
            )",
            [table_name],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to inspect sqlite_master for {table_name}"))?;
    Ok(exists != 0)
}

fn schema_reset_hint(db_path: &Path) -> String {
    format!(
        "delete {} and rerun the relevant sync/import command to recreate the disposable local database",
        db_path.display()
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::runtime::{ResolvedPaths, ValueSource};

    fn test_paths() -> (tempfile::TempDir, ResolvedPaths) {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(project_root.join("wiki_content")).expect("create wiki_content");
        fs::create_dir_all(project_root.join(".wikitool/data")).expect("create data dir");
        let paths = ResolvedPaths {
            db_path: project_root.join(".wikitool/data/wikitool.db"),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool/data"),
            config_path: project_root.join(".wikitool/config.toml"),
            parser_config_path: project_root
                .join(".wikitool")
                .join(crate::runtime::PARSER_CONFIG_FILENAME),
            project_root,
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        };
        (temp, paths)
    }

    #[test]
    fn schema_bootstrap_is_idempotent() {
        let (_temp, paths) = test_paths();
        ensure_database_schema(&paths).expect("first ensure");
        ensure_database_schema(&paths).expect("second ensure");

        let connection = open_database_connection(&paths.db_path).expect("open db");
        let exists: i64 = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'indexed_page_references')",
                [],
                |row| row.get(0),
            )
            .expect("query sqlite_master");
        assert_eq!(exists, 1);
    }

    #[test]
    fn schema_state_reports_missing_database() {
        let (_temp, paths) = test_paths();
        assert_eq!(
            schema_state(&paths).expect("schema state"),
            DatabaseSchemaState::Missing
        );
    }

    #[test]
    fn schema_validation_rejects_old_reference_table_shape() {
        let (_temp, paths) = test_paths();
        let connection = open_database_connection(&paths.db_path).expect("open db");
        connection
            .execute_batch(
                "CREATE TABLE indexed_page_references (
                    source_relative_path TEXT NOT NULL,
                    reference_index INTEGER NOT NULL,
                    source_title TEXT NOT NULL,
                    source_namespace TEXT NOT NULL,
                    section_heading TEXT,
                    reference_name TEXT,
                    reference_group TEXT,
                    summary_text TEXT NOT NULL,
                    reference_wikitext TEXT NOT NULL,
                    template_titles TEXT NOT NULL,
                    link_titles TEXT NOT NULL,
                    token_estimate INTEGER NOT NULL,
                    PRIMARY KEY (source_relative_path, reference_index)
                );",
            )
            .expect("seed old reference table");

        let error = ensure_database_schema(&paths).expect_err("must reject old schema");
        let message = error.to_string();
        let chain = error.chain().map(ToString::to_string).collect::<Vec<_>>();

        assert!(message.contains("delete"));
        assert!(
            chain
                .iter()
                .any(|entry| entry.contains("older disposable schema"))
        );
        assert!(chain.iter().any(|entry| entry.contains("citation_profile")));
    }
}
