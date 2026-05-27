use super::*;

#[derive(Debug, Clone)]
pub(crate) struct RawLocalSearchHit {
    title: String,
    namespace: String,
    is_redirect: bool,
    is_translation_variant: bool,
    translation_base_title: Option<String>,
    translation_language: Option<String>,
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

    let raw_limit = candidate_limit(limit.max(1), 4);
    if fts_table_exists(&connection, "indexed_pages_fts")
        && let Ok(hits) = query_search_fts(&connection, &normalized, raw_limit)
        && !hits.is_empty()
    {
        return collapse_search_hits(&connection, hits, limit).map(Some);
    }

    query_search_like(&connection, &normalized, raw_limit)
        .and_then(|hits| collapse_search_hits(&connection, hits, limit))
        .map(Some)
}

pub(crate) fn query_search_fts(
    connection: &Connection,
    normalized: &str,
    limit: usize,
) -> Result<Vec<RawLocalSearchHit>> {
    let limit_i64 = i64::try_from(limit).context("search limit does not fit into i64")?;
    let fts_query = format!("\"{normalized}\" *");
    let mut statement = connection
        .prepare(
            "SELECT ip.title, ip.namespace, ip.is_redirect,
                    ip.is_translation_variant, ip.translation_base_title, ip.translation_language
             FROM indexed_pages_fts fts
             JOIN indexed_pages ip ON ip.rowid = fts.rowid
             WHERE indexed_pages_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .context("failed to prepare FTS search query")?;
    let rows = statement
        .query_map(params![fts_query, limit_i64], decode_raw_search_hit)
        .context("failed to run FTS search query")?;
    load_raw_search_rows(rows)
}

pub(crate) fn query_search_like(
    connection: &Connection,
    normalized: &str,
    limit: usize,
) -> Result<Vec<RawLocalSearchHit>> {
    let wildcard = format!("%{normalized}%");
    let prefix = format!("{normalized}%");
    let limit_i64 = i64::try_from(limit).context("search limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT title, namespace, is_redirect,
                    is_translation_variant, translation_base_title, translation_language
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
        .query_map(
            params![wildcard, normalized, prefix, limit_i64],
            decode_raw_search_hit,
        )
        .context("failed to run local search query")?;
    load_raw_search_rows(rows)
}

fn decode_raw_search_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawLocalSearchHit> {
    Ok(RawLocalSearchHit {
        title: row.get(0)?,
        namespace: row.get(1)?,
        is_redirect: row.get::<_, i64>(2)? == 1,
        is_translation_variant: row.get::<_, i64>(3)? == 1,
        translation_base_title: row.get(4)?,
        translation_language: row.get(5)?,
    })
}

fn load_raw_search_rows<F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<RawLocalSearchHit>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<RawLocalSearchHit>,
{
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode local search row")?);
    }
    Ok(out)
}

pub(crate) fn collapse_search_hits(
    connection: &Connection,
    raw_hits: Vec<RawLocalSearchHit>,
    limit: usize,
) -> Result<Vec<LocalSearchHit>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut translation_languages_cache = BTreeMap::<String, Vec<String>>::new();

    for hit in raw_hits {
        if out.len() >= limit {
            break;
        }

        if hit.is_translation_variant {
            let Some(base_title) = hit.translation_base_title.clone() else {
                continue;
            };
            let Some(base_page) = load_page_record(connection, &base_title)? else {
                continue;
            };
            let key = format!(
                "{}|{}",
                base_page.namespace.to_ascii_lowercase(),
                base_page.title.to_ascii_lowercase()
            );
            let translation_languages = translation_languages_cache
                .entry(key.clone())
                .or_insert_with(|| {
                    load_translation_languages_for_base(connection, &base_page.title)
                        .unwrap_or_default()
                })
                .clone();
            if seen.insert(key) {
                out.push(LocalSearchHit {
                    title: base_page.title,
                    namespace: base_page.namespace,
                    is_redirect: base_page.is_redirect,
                    translation_languages,
                    matched_translation_language: hit.translation_language.clone(),
                });
            } else if let Some(existing) = out
                .iter_mut()
                .find(|existing| existing.title.eq_ignore_ascii_case(&base_page.title))
                && existing.matched_translation_language.is_none()
            {
                existing.matched_translation_language = hit.translation_language.clone();
            }
            continue;
        }

        let key = format!(
            "{}|{}",
            hit.namespace.to_ascii_lowercase(),
            hit.title.to_ascii_lowercase()
        );
        if !seen.insert(key.clone()) {
            continue;
        }
        let translation_languages = translation_languages_cache
            .entry(key)
            .or_insert_with(|| {
                load_translation_languages_for_base(connection, &hit.title).unwrap_or_default()
            })
            .clone();
        out.push(LocalSearchHit {
            title: hit.title,
            namespace: hit.namespace,
            is_redirect: hit.is_redirect,
            translation_languages,
            matched_translation_language: None,
        });
    }

    Ok(out)
}

fn load_translation_languages_for_base(
    connection: &Connection,
    base_title: &str,
) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT translation_language
             FROM indexed_pages
             WHERE translation_base_title = ?1
               AND is_translation_variant = 1
             ORDER BY translation_language ASC",
        )
        .context("failed to prepare translation language lookup")?;
    let rows = statement
        .query_map([base_title], |row| row.get::<_, String>(0))
        .context("failed to run translation language lookup")?;
    let mut out = Vec::new();
    for row in rows {
        let language = row.context("failed to decode translation language row")?;
        if !language.is_empty() {
            out.push(language);
        }
    }
    Ok(out)
}

pub(super) fn unsupported_translation_message(title: &str) -> String {
    format!(
        "translation surfaces are not supported yet for `{title}`; translation subpages are discovery-only and cannot be used for editing context"
    )
}
