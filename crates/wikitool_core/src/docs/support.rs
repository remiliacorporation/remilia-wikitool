use std::collections::BTreeSet;

use anyhow::Result;
use rusqlite::Connection;

use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;

use super::TechnicalDocType;
use super::parse::{is_translation_variant, normalize_title};
#[derive(Debug, Clone)]
pub(super) struct OutdatedRefreshRow {
    pub(super) label: String,
    pub(super) refresh_kind: String,
    pub(super) refresh_spec: String,
}

pub(super) fn open_docs_connection(paths: &ResolvedPaths) -> Result<Connection> {
    open_initialized_database_connection(&paths.db_path)
}

pub(super) fn rebuild_docs_fts_indexes(paths: &ResolvedPaths) -> Result<()> {
    let connection = open_docs_connection(paths)?;
    for table_name in [
        "docs_pages_fts",
        "docs_sections_fts",
        "docs_symbols_fts",
        "docs_examples_fts",
    ] {
        connection.execute_batch(&format!(
            "INSERT INTO {table_name}({table_name}) VALUES('rebuild')"
        ))?;
    }
    Ok(())
}

pub(super) fn cleanup_empty_corpora(connection: &Connection) -> Result<()> {
    connection.execute(
        "DELETE FROM docs_corpora
         WHERE NOT EXISTS (
             SELECT 1 FROM docs_pages WHERE docs_pages.corpus_id = docs_corpora.corpus_id
         )",
        [],
    )?;
    Ok(())
}

pub(super) fn count_query(connection: &Connection, sql: &str) -> Result<usize> {
    let value: i64 = connection.query_row(sql, [], |row| row.get(0))?;
    Ok(usize::try_from(value).unwrap_or(0))
}

pub(super) fn extension_corpus_id(extension_name: &str, source_profile: &str) -> String {
    if source_profile.is_empty() {
        return format!("extension:{}", sanitize_id(extension_name));
    }
    format!(
        "extension:{}:{}",
        sanitize_id(source_profile),
        sanitize_id(extension_name)
    )
}

pub(super) fn technical_corpus_id(
    doc_type: TechnicalDocType,
    page_title: Option<&str>,
    source_profile: &str,
) -> String {
    let scope = page_title.unwrap_or(doc_type.main_page());
    if source_profile.is_empty() {
        return format!("technical:{}:{}", doc_type.as_str(), sanitize_id(scope));
    }
    format!(
        "technical:{}:{}:{}",
        sanitize_id(source_profile),
        doc_type.as_str(),
        sanitize_id(scope)
    )
}

pub(super) fn profile_corpus_id(profile: &str) -> String {
    format!("profile:{}", sanitize_id(profile))
}

pub(super) fn normalize_corpus_kind_filter(value: Option<&str>) -> String {
    value.unwrap_or_default().trim().to_ascii_lowercase()
}

pub(super) fn infer_doc_type_from_title(title: &str) -> TechnicalDocType {
    if title.starts_with("Manual:Hooks") {
        return TechnicalDocType::Hooks;
    }
    if title.starts_with("Manual:$wg") {
        return TechnicalDocType::Config;
    }
    if title.starts_with("API:") {
        return TechnicalDocType::Api;
    }
    if title.starts_with("Help:") {
        return TechnicalDocType::Help;
    }
    TechnicalDocType::Manual
}

pub(super) fn normalize_extensions(extensions: &[String]) -> Vec<String> {
    let mut out = extensions
        .iter()
        .map(|value| normalize_extension_name(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalize_extension_list(&mut out);
    out
}

pub(super) fn normalize_extension_list(extensions: &mut Vec<String>) {
    extensions.sort_unstable_by_key(|value| value.to_ascii_lowercase());
    extensions.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
}

pub(super) fn filter_translation_titles(titles: &mut Vec<String>) {
    titles.retain(|title| !is_translation_variant(title));
}

/// Talk-page archives live under documentation prefixes on mediawiki.org
/// (e.g. `Extension:Cargo/Archive January to March 2016`) but are discussion
/// history, not documentation.
pub(super) fn filter_archive_titles(titles: &mut Vec<String>) {
    titles.retain(|title| {
        !title
            .split('/')
            .any(|segment| segment.starts_with("Archive"))
    });
}

pub(super) fn normalize_extension_name(value: &str) -> String {
    normalize_title(value.trim().trim_start_matches("Extension:"))
}

pub(super) fn dedupe_titles_in_order(titles: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    titles.retain(|title| seen.insert(title.to_ascii_lowercase()));
}

pub(super) fn extension_local_path(extension: &str, title: &str) -> String {
    format!(
        "docs/extensions/{}/{}.wiki",
        sanitize_path_segment(extension),
        sanitize_title_for_filename(title),
    )
}

pub(super) fn technical_local_path(doc_type: TechnicalDocType, title: &str) -> String {
    format!(
        "docs/technical/{}/{}.wiki",
        doc_type.as_str(),
        sanitize_title_for_filename(title),
    )
}

pub(super) fn sanitize_path_segment(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "_".to_string()
    } else {
        output
    }
}

pub(super) fn sanitize_title_for_filename(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            output.push('_');
        } else {
            output.push(ch);
        }
    }
    if output.is_empty() {
        "_".to_string()
    } else {
        output
    }
}

pub(super) fn sanitize_id(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            output.push('-');
            previous_dash = true;
        }
    }
    output.trim_matches('-').to_string()
}

pub(super) fn serialize_string_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| normalize_title(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn deserialize_string_list(value: &str) -> Vec<String> {
    value
        .lines()
        .map(normalize_title)
        .filter(|line| !line.is_empty())
        .collect()
}
