use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::phase1::ResolvedPaths;
use crate::phase2::{Namespace, ScanOptions, ScanStats, ScannedFile, scan_files};

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

pub fn query_backlinks(paths: &ResolvedPaths, title: &str) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    let normalized = normalize_query_title(title);
    if normalized.is_empty() {
        return Ok(Some(Vec::new()));
    }

    let mut statement = connection
        .prepare(
            "SELECT DISTINCT source_title
             FROM indexed_links
             WHERE target_title = ?1
             ORDER BY source_title ASC",
        )
        .context("failed to prepare backlinks query")?;
    let rows = statement
        .query_map([normalized], |row| row.get::<_, String>(0))
        .context("failed to run backlinks query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode backlinks row")?);
    }
    Ok(Some(out))
}

pub fn query_orphans(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };

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
    Ok(Some(out))
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
        extract_wikilinks, load_stored_index_stats, query_backlinks, query_empty_categories,
        query_orphans, rebuild_index,
    };
    use crate::phase1::{ResolvedPaths, ValueSource};
    use crate::phase2::{Namespace, ScanOptions};

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
    fn load_stored_index_stats_returns_none_when_db_is_missing() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        let stored = load_stored_index_stats(&paths).expect("load stats");
        assert!(stored.is_none());
    }
}
