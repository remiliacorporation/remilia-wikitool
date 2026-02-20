use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::filesystem::{Namespace, ScanOptions, ScanStats, ScannedFile, scan_files};
use crate::runtime::ResolvedPaths;

const INDEX_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS indexed_pages (
    relative_path TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    namespace TEXT NOT NULL,
    is_redirect INTEGER NOT NULL,
    redirect_target TEXT,
    content_hash TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    indexed_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_indexed_pages_title ON indexed_pages(title);
CREATE INDEX IF NOT EXISTS idx_indexed_pages_namespace ON indexed_pages(namespace);

CREATE TABLE IF NOT EXISTS indexed_links (
    source_relative_path TEXT NOT NULL,
    source_title TEXT NOT NULL,
    target_title TEXT NOT NULL,
    target_namespace TEXT NOT NULL,
    is_category_membership INTEGER NOT NULL,
    PRIMARY KEY (source_relative_path, target_title, is_category_membership),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_links_target ON indexed_links(target_title);
CREATE INDEX IF NOT EXISTS idx_indexed_links_source ON indexed_links(source_title);
CREATE INDEX IF NOT EXISTS idx_indexed_links_category_membership ON indexed_links(is_category_membership, target_title);
"#;

#[derive(Debug, Clone, Serialize)]
pub struct RebuildReport {
    pub db_path: String,
    pub inserted_rows: usize,
    pub inserted_links: usize,
    pub scan: ScanStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredIndexStats {
    pub indexed_rows: usize,
    pub redirects: usize,
    pub by_namespace: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalSearchHit {
    pub title: String,
    pub namespace: String,
    pub is_redirect: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalContextHeading {
    pub level: u8,
    pub heading: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalContextBundle {
    pub title: String,
    pub namespace: String,
    pub is_redirect: bool,
    pub redirect_target: Option<String>,
    pub relative_path: String,
    pub bytes: u64,
    pub word_count: usize,
    pub content_preview: String,
    pub sections: Vec<LocalContextHeading>,
    pub outgoing_links: Vec<String>,
    pub backlinks: Vec<String>,
    pub categories: Vec<String>,
    pub templates: Vec<String>,
    pub modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BrokenLinkIssue {
    pub source_title: String,
    pub target_title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoubleRedirectIssue {
    pub title: String,
    pub first_target: String,
    pub final_target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub broken_links: Vec<BrokenLinkIssue>,
    pub double_redirects: Vec<DoubleRedirectIssue>,
    pub uncategorized_pages: Vec<String>,
    pub orphan_pages: Vec<String>,
}

pub fn rebuild_index(paths: &ResolvedPaths, options: &ScanOptions) -> Result<RebuildReport> {
    let files = scan_files(paths, options)?;
    let scan = summarize_files(&files);
    ensure_db_parent(paths)?;
    let mut connection = open_connection(&paths.db_path)?;
    initialize_schema(&connection)?;
    let indexed_at_unix = unix_timestamp()?;

    let transaction = connection
        .transaction()
        .context("failed to start index rebuild transaction")?;
    transaction
        .execute("DELETE FROM indexed_pages", [])
        .context("failed to clear indexed_pages table")?;

    let mut page_statement = transaction
        .prepare(
            "INSERT INTO indexed_pages (
                relative_path,
                title,
                namespace,
                is_redirect,
                redirect_target,
                content_hash,
                bytes,
                indexed_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .context("failed to prepare indexed_pages insert")?;

    let mut link_statement = transaction
        .prepare(
            "INSERT OR IGNORE INTO indexed_links (
                source_relative_path,
                source_title,
                target_title,
                target_namespace,
                is_category_membership
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .context("failed to prepare indexed_links insert")?;

    let mut inserted_rows = 0usize;
    let mut inserted_links = 0usize;
    for file in &files {
        page_statement
            .execute(params![
                file.relative_path,
                file.title,
                file.namespace,
                if file.is_redirect { 1i64 } else { 0i64 },
                file.redirect_target,
                file.content_hash,
                i64::try_from(file.bytes).context("bytes value does not fit into i64")?,
                i64::try_from(indexed_at_unix).context("timestamp does not fit into i64")?,
            ])
            .with_context(|| format!("failed to insert {}", file.relative_path))?;
        inserted_rows += 1;

        let content = load_scanned_file_content(paths, file)?;
        for link in extract_wikilinks(&content) {
            let affected = link_statement
                .execute(params![
                    file.relative_path,
                    file.title,
                    link.target_title,
                    link.target_namespace,
                    if link.is_category_membership {
                        1i64
                    } else {
                        0i64
                    }
                ])
                .with_context(|| format!("failed to insert links for {}", file.relative_path))?;
            inserted_links += affected;
        }
    }
    drop(link_statement);
    drop(page_statement);

    transaction
        .commit()
        .context("failed to commit index rebuild transaction")?;

    // Rebuild FTS5 index if the virtual table exists (created by migration v003)
    rebuild_fts_index(&connection);

    Ok(RebuildReport {
        db_path: normalize_path(&paths.db_path),
        inserted_rows,
        inserted_links,
        scan,
    })
}

pub fn load_stored_index_stats(paths: &ResolvedPaths) -> Result<Option<StoredIndexStats>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }

    let connection = open_connection(&paths.db_path)?;
    if !table_exists(&connection, "indexed_pages")? {
        return Ok(None);
    }

    let indexed_rows = count_query(&connection, "SELECT COUNT(*) FROM indexed_pages")
        .context("failed to count indexed rows")?;
    let redirects = count_query(
        &connection,
        "SELECT COUNT(*) FROM indexed_pages WHERE is_redirect = 1",
    )
    .context("failed to count redirects")?;
    let by_namespace = namespace_counts(&connection)?;

    Ok(Some(StoredIndexStats {
        indexed_rows,
        redirects,
        by_namespace,
    }))
}

pub fn query_search_local(
    paths: &ResolvedPaths,
    query: &str,
    limit: usize,
) -> Result<Option<Vec<LocalSearchHit>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    let normalized = normalize_spaces(&query.replace('_', " "));
    if normalized.is_empty() {
        return Ok(Some(Vec::new()));
    }

    // Try FTS5 first if the virtual table exists
    if fts_table_exists(&connection, "indexed_pages_fts") {
        if let Ok(hits) = query_search_fts(&connection, &normalized, limit) {
            return Ok(Some(hits));
        }
    }

    // Fallback to LIKE-based search
    query_search_like(&connection, &normalized, limit).map(Some)
}

fn query_search_fts(
    connection: &Connection,
    normalized: &str,
    limit: usize,
) -> Result<Vec<LocalSearchHit>> {
    let limit_i64 = i64::try_from(limit).context("search limit does not fit into i64")?;
    // FTS5 match expression: quote the term for phrase matching, add * for prefix
    let fts_query = format!("\"{normalized}\" *");
    let mut statement = connection
        .prepare(
            "SELECT ip.title, ip.namespace, ip.is_redirect
             FROM indexed_pages_fts fts
             JOIN indexed_pages ip ON ip.rowid = fts.rowid
             WHERE indexed_pages_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .context("failed to prepare FTS search query")?;
    let rows = statement
        .query_map(params![fts_query, limit_i64], |row| {
            let title: String = row.get(0)?;
            let namespace: String = row.get(1)?;
            let is_redirect: i64 = row.get(2)?;
            Ok(LocalSearchHit {
                title,
                namespace,
                is_redirect: is_redirect == 1,
            })
        })
        .context("failed to run FTS search query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode FTS search row")?);
    }
    Ok(out)
}

fn query_search_like(
    connection: &Connection,
    normalized: &str,
    limit: usize,
) -> Result<Vec<LocalSearchHit>> {
    let wildcard = format!("%{normalized}%");
    let prefix = format!("{normalized}%");
    let limit_i64 = i64::try_from(limit).context("search limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT title, namespace, is_redirect
             FROM indexed_pages
             WHERE lower(title) LIKE lower(?1)
             ORDER BY
               CASE
                 WHEN lower(title) = lower(?2) THEN 0
                 WHEN lower(title) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               title ASC
             LIMIT ?4",
        )
        .context("failed to prepare local search query")?;
    let rows = statement
        .query_map(params![wildcard, normalized, prefix, limit_i64], |row| {
            let title: String = row.get(0)?;
            let namespace: String = row.get(1)?;
            let is_redirect: i64 = row.get(2)?;
            Ok(LocalSearchHit {
                title,
                namespace,
                is_redirect: is_redirect == 1,
            })
        })
        .context("failed to run local search query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode local search row")?);
    }
    Ok(out)
}

pub fn build_local_context(
    paths: &ResolvedPaths,
    title: &str,
) -> Result<Option<LocalContextBundle>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    let normalized = normalize_query_title(title);
    if normalized.is_empty() {
        return Ok(None);
    }

    let page = match load_page_record(&connection, &normalized)? {
        Some(page) => page,
        None => return Ok(None),
    };
    let absolute = absolute_path_from_relative(paths, &page.relative_path);
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))?;

    let link_rows = load_outgoing_link_rows(&connection, &page.relative_path)?;
    let backlinks = query_backlinks_for_connection(&connection, &page.title)?;

    let mut outgoing_set = BTreeSet::new();
    let mut category_set = BTreeSet::new();
    let mut template_set = BTreeSet::new();
    let mut module_set = BTreeSet::new();
    for link in &link_rows {
        outgoing_set.insert(link.target_title.clone());
        if link.is_category_membership {
            category_set.insert(link.target_title.clone());
        }
        if link.target_namespace == Namespace::Template.as_str() {
            template_set.insert(link.target_title.clone());
        }
        if link.target_namespace == Namespace::Module.as_str() {
            module_set.insert(link.target_title.clone());
        }
    }

    Ok(Some(LocalContextBundle {
        title: page.title,
        namespace: page.namespace,
        is_redirect: page.is_redirect,
        redirect_target: page.redirect_target,
        relative_path: page.relative_path,
        bytes: page.bytes,
        word_count: count_words(&content),
        content_preview: make_content_preview(&content, 280),
        sections: parse_section_headings(&content, 24),
        outgoing_links: outgoing_set.into_iter().collect(),
        backlinks,
        categories: category_set.into_iter().collect(),
        templates: template_set.into_iter().collect(),
        modules: module_set.into_iter().collect(),
    }))
}

pub fn run_validation_checks(paths: &ResolvedPaths) -> Result<Option<ValidationReport>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };

    Ok(Some(ValidationReport {
        broken_links: query_broken_links_for_connection(&connection)?,
        double_redirects: query_double_redirects_for_connection(&connection)?,
        uncategorized_pages: query_uncategorized_pages_for_connection(&connection)?,
        orphan_pages: query_orphans_for_connection(&connection)?,
    }))
}

pub fn query_backlinks(paths: &ResolvedPaths, title: &str) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    let normalized = normalize_query_title(title);
    if normalized.is_empty() {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(query_backlinks_for_connection(
        &connection,
        &normalized,
    )?))
}

pub fn query_orphans(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    Ok(Some(query_orphans_for_connection(&connection)?))
}

pub fn query_empty_categories(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };

    let mut statement = connection
        .prepare(
            "SELECT p.title
             FROM indexed_pages p
             WHERE p.namespace = 'Category'
               AND NOT EXISTS (
                   SELECT 1
                   FROM indexed_links l
                   WHERE l.target_title = p.title
                     AND l.is_category_membership = 1
               )
             ORDER BY p.title ASC",
        )
        .context("failed to prepare empty category query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .context("failed to run empty category query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode empty category row")?);
    }
    Ok(Some(out))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLink {
    target_title: String,
    target_namespace: String,
    is_category_membership: bool,
}

#[derive(Debug, Clone)]
struct IndexedPageRecord {
    title: String,
    namespace: String,
    is_redirect: bool,
    redirect_target: Option<String>,
    relative_path: String,
    bytes: u64,
}

#[derive(Debug, Clone)]
struct IndexedLinkRow {
    target_title: String,
    target_namespace: String,
    is_category_membership: bool,
}

fn extract_wikilinks(content: &str) -> Vec<ParsedLink> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
            let start = cursor + 2;
            let mut end = start;
            while end + 1 < bytes.len() {
                if bytes[end] == b']' && bytes[end + 1] == b']' {
                    break;
                }
                end += 1;
            }
            if end + 1 >= bytes.len() {
                break;
            }

            let inner = &content[start..end];
            if let Some(link) = parse_wikilink(inner) {
                out.push(link);
            }
            cursor = end + 2;
            continue;
        }
        cursor += 1;
    }

    out
}

fn parse_wikilink(inner: &str) -> Option<ParsedLink> {
    let target_part = inner.split('|').next().unwrap_or("").trim();
    if target_part.is_empty() {
        return None;
    }

    let mut target = target_part;
    let mut leading_colon = false;
    while let Some(stripped) = target.strip_prefix(':') {
        leading_colon = true;
        target = stripped.trim_start();
    }
    if target.is_empty() {
        return None;
    }

    if let Some((without_fragment, _)) = target.split_once('#') {
        target = without_fragment.trim_end();
    }
    if target.is_empty() {
        return None;
    }

    if target.starts_with("http://") || target.starts_with("https://") || target.starts_with("//") {
        return None;
    }

    let target = normalize_spaces(&target.replace('_', " "));
    if target.is_empty() {
        return None;
    }

    let (title, namespace) = normalize_title_and_namespace(&target)?;
    let is_category_membership = namespace == Namespace::Category.as_str() && !leading_colon;

    Some(ParsedLink {
        target_title: title,
        target_namespace: namespace.to_string(),
        is_category_membership,
    })
}

fn normalize_title_and_namespace(value: &str) -> Option<(String, &'static str)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((prefix, rest)) = trimmed.split_once(':')
        && let Some(namespace) = canonical_namespace(prefix)
    {
        let body = normalize_spaces(rest);
        if body.is_empty() {
            return None;
        }
        return Some((format!("{namespace}:{body}"), namespace));
    }

    Some((trimmed.to_string(), Namespace::Main.as_str()))
}

fn canonical_namespace(prefix: &str) -> Option<&'static str> {
    let trimmed = prefix.trim();
    if trimmed.eq_ignore_ascii_case("Category") {
        return Some(Namespace::Category.as_str());
    }
    if trimmed.eq_ignore_ascii_case("File") {
        return Some(Namespace::File.as_str());
    }
    if trimmed.eq_ignore_ascii_case("User") {
        return Some(Namespace::User.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Goldenlight") {
        return Some(Namespace::Goldenlight.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Template") {
        return Some(Namespace::Template.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Module") {
        return Some(Namespace::Module.as_str());
    }
    if trimmed.eq_ignore_ascii_case("MediaWiki") {
        return Some(Namespace::MediaWiki.as_str());
    }
    None
}

fn normalize_query_title(title: &str) -> String {
    let normalized = normalize_spaces(&title.replace('_', " "));
    if normalized.is_empty() {
        return normalized;
    }
    match normalize_title_and_namespace(&normalized) {
        Some((value, _)) => value,
        None => String::new(),
    }
}

fn normalize_spaces(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;

    for ch in value.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
                previous_was_space = true;
            }
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }

    output.trim().to_string()
}

fn load_page_record(connection: &Connection, title: &str) -> Result<Option<IndexedPageRecord>> {
    let mut statement = connection
        .prepare(
            "SELECT
                title,
                namespace,
                is_redirect,
                redirect_target,
                relative_path,
                bytes
             FROM indexed_pages
             WHERE lower(title) = lower(?1)
             LIMIT 1",
        )
        .context("failed to prepare page record lookup")?;

    let mut rows = statement
        .query([title])
        .context("failed to run page record lookup")?;
    let row = match rows.next().context("failed to read page record row")? {
        Some(row) => row,
        None => return Ok(None),
    };

    let bytes_i64: i64 = row.get(5).context("failed to decode page bytes")?;
    let bytes = u64::try_from(bytes_i64).context("page bytes are negative")?;
    Ok(Some(IndexedPageRecord {
        title: row.get(0).context("failed to decode page title")?,
        namespace: row.get(1).context("failed to decode page namespace")?,
        is_redirect: row
            .get::<_, i64>(2)
            .context("failed to decode redirect flag")?
            == 1,
        redirect_target: row.get(3).context("failed to decode redirect target")?,
        relative_path: row.get(4).context("failed to decode relative path")?,
        bytes,
    }))
}

fn load_outgoing_link_rows(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Vec<IndexedLinkRow>> {
    let mut statement = connection
        .prepare(
            "SELECT target_title, target_namespace, is_category_membership
             FROM indexed_links
             WHERE source_relative_path = ?1
             ORDER BY target_title ASC",
        )
        .context("failed to prepare outgoing links query")?;
    let rows = statement
        .query_map([source_relative_path], |row| {
            let target_title: String = row.get(0)?;
            let target_namespace: String = row.get(1)?;
            let is_category_membership: i64 = row.get(2)?;
            Ok(IndexedLinkRow {
                target_title,
                target_namespace,
                is_category_membership: is_category_membership == 1,
            })
        })
        .context("failed to run outgoing links query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode outgoing link row")?);
    }
    Ok(out)
}

fn query_backlinks_for_connection(connection: &Connection, title: &str) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT source_title
             FROM indexed_links
             WHERE target_title = ?1
             ORDER BY source_title ASC",
        )
        .context("failed to prepare backlinks query")?;
    let rows = statement
        .query_map([title], |row| row.get::<_, String>(0))
        .context("failed to run backlinks query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode backlinks row")?);
    }
    Ok(out)
}

fn query_orphans_for_connection(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT p.title
             FROM indexed_pages p
             WHERE p.namespace = 'Main'
               AND p.is_redirect = 0
               AND NOT EXISTS (
                   SELECT 1
                   FROM indexed_links l
                   JOIN indexed_pages src ON src.relative_path = l.source_relative_path
                   WHERE l.target_title = p.title
                     AND src.namespace = 'Main'
                     AND src.is_redirect = 0
                     AND src.title <> p.title
               )
             ORDER BY p.title ASC",
        )
        .context("failed to prepare orphan query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .context("failed to run orphan query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode orphan row")?);
    }
    Ok(out)
}

fn query_broken_links_for_connection(connection: &Connection) -> Result<Vec<BrokenLinkIssue>> {
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT l.source_title, l.target_title
             FROM indexed_links l
             LEFT JOIN indexed_pages p ON p.title = l.target_title
             WHERE l.target_namespace = 'Main'
               AND p.title IS NULL
             ORDER BY l.source_title ASC, l.target_title ASC",
        )
        .context("failed to prepare broken-links query")?;
    let rows = statement
        .query_map([], |row| {
            Ok(BrokenLinkIssue {
                source_title: row.get(0)?,
                target_title: row.get(1)?,
            })
        })
        .context("failed to run broken-links query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode broken-link row")?);
    }
    Ok(out)
}

fn query_double_redirects_for_connection(
    connection: &Connection,
) -> Result<Vec<DoubleRedirectIssue>> {
    let mut statement = connection
        .prepare(
            "SELECT
                p.title,
                p.redirect_target,
                p2.redirect_target
             FROM indexed_pages p
             JOIN indexed_pages p2 ON p.redirect_target = p2.title
             WHERE p.is_redirect = 1
               AND p2.is_redirect = 1
             ORDER BY p.title ASC",
        )
        .context("failed to prepare double-redirect query")?;
    let rows = statement
        .query_map([], |row| {
            let first_target: String = row.get(1)?;
            let final_target: String = row.get(2)?;
            Ok(DoubleRedirectIssue {
                title: row.get(0)?,
                first_target,
                final_target,
            })
        })
        .context("failed to run double-redirect query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode double-redirect row")?);
    }
    Ok(out)
}

fn query_uncategorized_pages_for_connection(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT p.title
             FROM indexed_pages p
             WHERE p.namespace = 'Main'
               AND p.is_redirect = 0
               AND NOT EXISTS (
                   SELECT 1
                   FROM indexed_links l
                   WHERE l.source_relative_path = p.relative_path
                     AND l.is_category_membership = 1
               )
             ORDER BY p.title ASC",
        )
        .context("failed to prepare uncategorized query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .context("failed to run uncategorized query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode uncategorized row")?);
    }
    Ok(out)
}

fn count_words(content: &str) -> usize {
    content
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .count()
}

fn make_content_preview(content: &str, max_chars: usize) -> String {
    let normalized = normalize_spaces(content);
    if normalized.len() <= max_chars {
        return normalized;
    }
    let output = normalized.chars().take(max_chars).collect::<String>();
    format!("{output}...")
}

fn parse_section_headings(content: &str, max_sections: usize) -> Vec<LocalContextHeading> {
    let mut out = Vec::new();
    if max_sections == 0 {
        return out;
    }

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.len() < 4 || !trimmed.starts_with('=') || !trimmed.ends_with('=') {
            continue;
        }

        let leading = trimmed.chars().take_while(|ch| *ch == '=').count();
        let trailing = trimmed.chars().rev().take_while(|ch| *ch == '=').count();
        if leading != trailing || !(2..=6).contains(&leading) {
            continue;
        }
        if leading * 2 >= trimmed.len() {
            continue;
        }

        let heading = trimmed[leading..trimmed.len() - trailing].trim();
        if heading.is_empty() {
            continue;
        }
        out.push(LocalContextHeading {
            level: u8::try_from(leading).unwrap_or(6),
            heading: heading.to_string(),
        });
        if out.len() >= max_sections {
            break;
        }
    }

    out
}

fn summarize_files(files: &[ScannedFile]) -> ScanStats {
    let mut by_namespace = BTreeMap::new();
    let mut content_files = 0usize;
    let mut template_files = 0usize;
    let mut redirects = 0usize;

    for file in files {
        *by_namespace.entry(file.namespace.clone()).or_insert(0) += 1;
        match file.namespace.as_str() {
            value
                if value == Namespace::Template.as_str()
                    || value == Namespace::Module.as_str()
                    || value == Namespace::MediaWiki.as_str() =>
            {
                template_files += 1;
            }
            _ => {
                content_files += 1;
            }
        }
        if file.is_redirect {
            redirects += 1;
        }
    }

    ScanStats {
        total_files: files.len(),
        content_files,
        template_files,
        redirects,
        by_namespace,
    }
}

fn load_scanned_file_content(paths: &ResolvedPaths, file: &ScannedFile) -> Result<String> {
    let absolute = absolute_path_from_relative(paths, &file.relative_path);
    fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))
}

fn absolute_path_from_relative(paths: &ResolvedPaths, relative: &str) -> PathBuf {
    let mut out = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            out.push(segment);
        }
    }
    out
}

fn open_connection(db_path: &Path) -> Result<Connection> {
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

fn open_indexed_connection(paths: &ResolvedPaths) -> Result<Option<Connection>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }
    let connection = open_connection(&paths.db_path)?;
    if !table_exists(&connection, "indexed_pages")? || !table_exists(&connection, "indexed_links")?
    {
        return Ok(None);
    }
    Ok(Some(connection))
}

fn initialize_schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(INDEX_SCHEMA_SQL)
        .context("failed to initialize index schema")
}

fn ensure_db_parent(paths: &ResolvedPaths) -> Result<()> {
    let parent = paths
        .db_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("db path has no parent: {}", paths.db_path.display()))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create database parent directory {}",
            parent.display()
        )
    })
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool> {
    let exists: i64 = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to check sqlite_master for table {table_name}"))?;
    Ok(exists == 1)
}

fn count_query(connection: &Connection, sql: &str) -> Result<usize> {
    let count: i64 = connection
        .query_row(sql, [], |row| row.get(0))
        .with_context(|| format!("failed query: {sql}"))?;
    usize::try_from(count).context("count does not fit into usize")
}

fn namespace_counts(connection: &Connection) -> Result<BTreeMap<String, usize>> {
    let mut statement = connection
        .prepare(
            "SELECT namespace, COUNT(*) AS count
             FROM indexed_pages
             GROUP BY namespace
             ORDER BY namespace ASC",
        )
        .context("failed to prepare namespace aggregation query")?;

    let rows = statement
        .query_map([], |row| {
            let namespace: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((namespace, count))
        })
        .context("failed to run namespace aggregation query")?;

    let mut out = BTreeMap::new();
    for row in rows {
        let (namespace, count) = row.context("failed to read namespace aggregation row")?;
        let count = usize::try_from(count).context("namespace count does not fit into usize")?;
        out.insert(namespace, count);
    }
    Ok(out)
}

fn fts_table_exists(connection: &Connection, table_name: &str) -> bool {
    table_exists(connection, table_name).unwrap_or(false)
}

fn rebuild_fts_index(connection: &Connection) {
    if fts_table_exists(connection, "indexed_pages_fts") {
        let _ = connection.execute_batch(
            "INSERT INTO indexed_pages_fts(indexed_pages_fts) VALUES('rebuild')",
        );
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn unix_timestamp() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")
        .map(|duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use super::{
        BrokenLinkIssue, build_local_context, extract_wikilinks, load_stored_index_stats,
        query_backlinks, query_empty_categories, query_orphans, query_search_local, rebuild_index,
        run_validation_checks,
    };
    use crate::filesystem::{Namespace, ScanOptions};
    use crate::runtime::{ResolvedPaths, ValueSource};

    fn write_file(path: &Path, content: &str) {
        let parent = path.parent().expect("parent");
        fs::create_dir_all(parent).expect("create parent");
        fs::write(path, content).expect("write file");
    }

    fn paths(project_root: &Path) -> ResolvedPaths {
        ResolvedPaths {
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool").join("data"),
            db_path: project_root
                .join(".wikitool")
                .join("data")
                .join("wikitool.db"),
            config_path: project_root.join(".wikitool").join("config.toml"),
            parser_config_path: project_root.join(".wikitool").join("remilia-parser.json"),
            project_root: project_root.to_path_buf(),
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn extract_wikilinks_parses_titles_and_category_membership() {
        let content = "[[Alpha|label]] [[Category:People]] [[:Category:People]] [[Module:Navbar/configuration]] [[Alpha#History]] [[https://example.com]]";
        let links = extract_wikilinks(content);

        assert_eq!(links.len(), 5);
        assert_eq!(links[0].target_title, "Alpha");
        assert!(!links[0].is_category_membership);
        assert_eq!(links[1].target_title, "Category:People");
        assert!(links[1].is_category_membership);
        assert_eq!(links[2].target_title, "Category:People");
        assert!(!links[2].is_category_membership);
        assert_eq!(links[3].target_title, "Module:Navbar/configuration");
        assert_eq!(links[4].target_title, "Alpha");
    }

    #[test]
    fn rebuild_index_persists_scan_rows() {
        let temp = tempdir().expect("tempdir");
        let project_root: PathBuf = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Alpha article",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("Foo.wiki"),
            "#REDIRECT [[Category:Bar]]",
        );
        write_file(
            &paths
                .templates_dir
                .join("navbox")
                .join("Module_Navbar")
                .join("configuration.lua"),
            "return {}",
        );

        let report = rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
        assert!(paths.db_path.exists());
        assert_eq!(report.inserted_rows, 3);
        assert_eq!(report.scan.total_files, 3);
        assert_eq!(report.scan.redirects, 1);

        let stored = load_stored_index_stats(&paths)
            .expect("load stats")
            .expect("stats must exist");
        assert_eq!(stored.indexed_rows, 3);
        assert_eq!(stored.redirects, 1);
        assert_eq!(
            stored.by_namespace,
            BTreeMap::from([
                (Namespace::Category.as_str().to_string(), 1usize),
                (Namespace::Main.as_str().to_string(), 1usize),
                (Namespace::Module.as_str().to_string(), 1usize),
            ])
        );
    }

    #[test]
    fn query_backlinks_orphans_and_empty_categories() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "[[Beta]] [[Category:People]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "No links here",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "[[Beta]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("People.wiki"),
            "People category",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("Empty.wiki"),
            "Empty category",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let backlinks = query_backlinks(&paths, "Beta")
            .expect("backlinks query")
            .expect("backlinks should exist");
        assert_eq!(backlinks, vec!["Alpha".to_string(), "Gamma".to_string()]);

        let orphans = query_orphans(&paths)
            .expect("orphans query")
            .expect("orphans should exist");
        assert_eq!(orphans, vec!["Alpha".to_string(), "Gamma".to_string()]);

        let empty_categories = query_empty_categories(&paths)
            .expect("empty category query")
            .expect("empty categories should exist");
        assert_eq!(empty_categories, vec!["Category:Empty".to_string()]);
    }

    #[test]
    fn query_search_and_context_bundle() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Lead paragraph\n== History ==\n[[Beta]] [[Template:Infobox person]] [[Module:Navbar]] [[Category:People]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "No links here",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "[[Beta]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("People.wiki"),
            "People category",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let search = query_search_local(&paths, "be", 20)
            .expect("search query")
            .expect("search should be available");
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].title, "Beta");

        let context = build_local_context(&paths, "Alpha")
            .expect("context query")
            .expect("alpha context exists");
        assert_eq!(context.title, "Alpha");
        assert_eq!(context.namespace, "Main");
        assert_eq!(context.sections.len(), 1);
        assert_eq!(context.sections[0].heading, "History");
        assert_eq!(context.categories, vec!["Category:People".to_string()]);
        assert_eq!(
            context.templates,
            vec!["Template:Infobox person".to_string()]
        );
        assert_eq!(context.modules, vec!["Module:Navbar".to_string()]);
        assert_eq!(context.backlinks.len(), 0);

        let beta_context = build_local_context(&paths, "Beta")
            .expect("beta context query")
            .expect("beta context exists");
        assert_eq!(
            beta_context.backlinks,
            vec!["Alpha".to_string(), "Gamma".to_string()]
        );
    }

    #[test]
    fn validation_checks_report_expected_issues() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "[[Beta]] [[MissingTarget]] [[Category:People]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "#REDIRECT [[Gamma]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "#REDIRECT [[Delta]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("NoCategory.wiki"),
            "Standalone page",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("People.wiki"),
            "People category",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
        let report = run_validation_checks(&paths)
            .expect("validate query")
            .expect("validation should be available");

        assert_eq!(report.broken_links.len(), 2);
        assert!(report.broken_links.contains(&BrokenLinkIssue {
            source_title: "Alpha".to_string(),
            target_title: "MissingTarget".to_string(),
        }));
        assert!(report.broken_links.contains(&BrokenLinkIssue {
            source_title: "Gamma".to_string(),
            target_title: "Delta".to_string(),
        }));
        assert_eq!(report.double_redirects.len(), 1);
        assert_eq!(report.double_redirects[0].title, "Beta");
        assert!(
            report
                .uncategorized_pages
                .contains(&"NoCategory".to_string())
        );
        assert!(report.orphan_pages.contains(&"Alpha".to_string()));
    }

    #[test]
    fn load_stored_index_stats_returns_none_when_db_is_missing() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        let stored = load_stored_index_stats(&paths).expect("load stats");
        assert!(stored.is_none());
    }
}
