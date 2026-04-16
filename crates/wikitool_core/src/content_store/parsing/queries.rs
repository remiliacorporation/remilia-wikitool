use super::*;

pub(crate) fn normalize_query_title(title: &str) -> String {
    let normalized = normalize_spaces(&title.replace('_', " "));
    if normalized.is_empty() {
        return normalized;
    }
    match normalize_title_and_namespace(&normalized) {
        Some((value, _)) => value,
        None => String::new(),
    }
}

pub(crate) fn load_page_record(
    connection: &Connection,
    title: &str,
) -> Result<Option<IndexedPageRecord>> {
    if let Some(record) = load_page_record_exact(connection, title)? {
        return Ok(Some(record));
    }
    let resolved = resolve_alias_title(connection, title, 6)?;
    if resolved.eq_ignore_ascii_case(title) {
        return Ok(None);
    }
    load_page_record_exact(connection, &resolved)
}

pub(crate) fn load_page_record_exact(
    connection: &Connection,
    title: &str,
) -> Result<Option<IndexedPageRecord>> {
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

pub(crate) fn resolve_alias_title(
    connection: &Connection,
    title: &str,
    max_hops: usize,
) -> Result<String> {
    let mut current = normalize_query_title(title);
    if current.is_empty() {
        return Ok(current);
    }
    if !table_exists(connection, "indexed_page_aliases")? {
        return Ok(current);
    }
    let mut seen = BTreeSet::new();
    for _ in 0..max_hops.max(1) {
        let normalized = normalize_query_title(&current);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            break;
        }
        let mut statement = connection
            .prepare(
                "SELECT canonical_title
                 FROM indexed_page_aliases
                 WHERE lower(alias_title) = lower(?1)
                 LIMIT 1",
            )
            .context("failed to prepare alias resolution query")?;
        let mut rows = statement
            .query([normalized.as_str()])
            .context("failed to run alias resolution query")?;
        let Some(row) = rows.next().context("failed to read alias resolution row")? else {
            return Ok(normalized);
        };
        let canonical: String = row
            .get(0)
            .context("failed to decode alias canonical title")?;
        if canonical.eq_ignore_ascii_case(&normalized) {
            return Ok(normalized);
        }
        current = canonical;
    }
    Ok(current)
}

pub(crate) fn load_outgoing_link_rows(
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

pub(crate) fn query_backlinks_for_connection(
    connection: &Connection,
    title: &str,
) -> Result<Vec<String>> {
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

pub(crate) fn query_orphans_for_connection(connection: &Connection) -> Result<Vec<String>> {
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

pub(crate) fn query_broken_links_for_connection(
    connection: &Connection,
) -> Result<Vec<BrokenLinkIssue>> {
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

pub(crate) fn query_double_redirects_for_connection(
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

pub(crate) fn query_uncategorized_pages_for_connection(
    connection: &Connection,
) -> Result<Vec<String>> {
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

pub(crate) fn count_words(content: &str) -> usize {
    content
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .count()
}

pub(crate) fn make_content_preview(content: &str, max_chars: usize) -> String {
    let normalized = normalize_spaces(content);
    if normalized.len() <= max_chars {
        return normalized;
    }
    let output = normalized.chars().take(max_chars).collect::<String>();
    format!("{output}...")
}

pub(crate) fn summarize_files(files: &[ScannedFile]) -> ScanStats {
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

pub(crate) fn load_scanned_file_content(
    paths: &ResolvedPaths,
    file: &ScannedFile,
) -> Result<String> {
    let absolute = absolute_path_from_relative(paths, &file.relative_path);
    fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))
}

pub(crate) fn absolute_path_from_relative(paths: &ResolvedPaths, relative: &str) -> PathBuf {
    let mut out = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            out.push(segment);
        }
    }
    out
}

pub(crate) fn open_indexed_connection(paths: &ResolvedPaths) -> Result<Option<Connection>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }
    let connection = open_initialized_database_connection(&paths.db_path)?;
    if !has_populated_local_index(&connection)? {
        return Ok(None);
    }
    Ok(Some(connection))
}

pub(crate) fn has_populated_local_index(connection: &Connection) -> Result<bool> {
    if !table_exists(connection, "indexed_pages")? || !table_exists(connection, "indexed_links")? {
        return Ok(false);
    }
    Ok(count_query(connection, "SELECT COUNT(*) FROM indexed_pages")? > 0)
}

pub(crate) fn count_query(connection: &Connection, sql: &str) -> Result<usize> {
    let count: i64 = connection
        .query_row(sql, [], |row| row.get(0))
        .with_context(|| format!("failed query: {sql}"))?;
    usize::try_from(count).context("count does not fit into usize")
}

pub(crate) fn namespace_counts(connection: &Connection) -> Result<BTreeMap<String, usize>> {
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

pub(crate) fn fts_table_exists(connection: &Connection, table_name: &str) -> bool {
    table_exists(connection, table_name).unwrap_or(false)
}

pub(crate) fn rebuild_fts_index(connection: &Connection) -> Result<()> {
    if fts_table_exists(connection, "indexed_pages_fts") {
        connection
            .execute_batch("INSERT INTO indexed_pages_fts(indexed_pages_fts) VALUES('rebuild')")
            .context("failed to rebuild indexed_pages_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_chunks_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_chunks_fts(indexed_page_chunks_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_chunks_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_sections_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_sections_fts(indexed_page_sections_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_sections_fts")?;
    }
    if fts_table_exists(connection, "indexed_template_examples_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_template_examples_fts(indexed_template_examples_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_template_examples_fts")?;
    }
    if fts_table_exists(connection, "indexed_module_invocations_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_module_invocations_fts(indexed_module_invocations_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_module_invocations_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_references_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_references_fts(indexed_page_references_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_references_fts")?;
    }
    if fts_table_exists(connection, "indexed_reference_authorities_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_reference_authorities_fts(indexed_reference_authorities_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_reference_authorities_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_media_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_media_fts(indexed_page_media_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_media_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_semantics_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_semantics_fts(indexed_page_semantics_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_semantics_fts")?;
    }
    Ok(())
}
