use super::*;

pub(super) fn load_docs_stats(connection: &Connection) -> Result<DocsStats> {
    let corpora_count = count_query(connection, "SELECT COUNT(*) FROM docs_corpora")?;
    let pages_count = count_query(connection, "SELECT COUNT(*) FROM docs_pages")?;
    let sections_count = count_query(connection, "SELECT COUNT(*) FROM docs_sections")?;
    let symbols_count = count_query(connection, "SELECT COUNT(*) FROM docs_symbols")?;
    let examples_count = count_query(connection, "SELECT COUNT(*) FROM docs_examples")?;

    let mut corpora_by_kind = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT corpus_kind, COUNT(*) FROM docs_corpora GROUP BY corpus_kind ORDER BY corpus_kind ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (kind, count) = row?;
        corpora_by_kind.insert(kind, usize::try_from(count).unwrap_or(0));
    }

    let mut technical_by_type = BTreeMap::new();
    let mut typed_statement = connection.prepare(
        "SELECT technical_type, COUNT(*) FROM docs_corpora
         WHERE technical_type != ''
         GROUP BY technical_type
         ORDER BY technical_type ASC",
    )?;
    let typed_rows = typed_statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in typed_rows {
        let (doc_type, count) = row?;
        technical_by_type.insert(doc_type, usize::try_from(count).unwrap_or(0));
    }

    Ok(DocsStats {
        corpora_count,
        pages_count,
        sections_count,
        symbols_count,
        examples_count,
        corpora_by_kind,
        technical_by_type,
    })
}

pub(super) fn load_docs_corpora(
    connection: &Connection,
    corpus_kind: Option<&str>,
    technical_type: Option<&str>,
    profile: Option<&str>,
    now_unix: u64,
) -> Result<Vec<DocsCorpusSummary>> {
    let mut out = Vec::new();
    let corpus_kind = corpus_kind.unwrap_or_default().to_string();
    let technical_type = technical_type.unwrap_or_default().to_string();
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT corpus_id, corpus_kind, label, source_wiki, source_version, source_profile,
                technical_type, pages_count, sections_count, symbols_count, examples_count,
                fetched_at_unix, expires_at_unix
         FROM docs_corpora
         WHERE (?1 = '' OR lower(corpus_kind) = lower(?1))
           AND (?2 = '' OR lower(technical_type) = lower(?2))
           AND (?3 = '' OR lower(source_profile) = lower(?3))
         ORDER BY corpus_kind ASC, label ASC",
    )?;
    let rows = statement.query_map(params![corpus_kind, technical_type, profile], |row| {
        let pages_count: i64 = row.get(7)?;
        let sections_count: i64 = row.get(8)?;
        let symbols_count: i64 = row.get(9)?;
        let examples_count: i64 = row.get(10)?;
        let fetched_at_unix: i64 = row.get(11)?;
        let expires_at_unix: i64 = row.get(12)?;
        Ok(DocsCorpusSummary {
            corpus_id: row.get(0)?,
            corpus_kind: row.get(1)?,
            label: row.get(2)?,
            source_wiki: row.get(3)?,
            source_version: row.get(4)?,
            source_profile: row.get(5)?,
            technical_type: row.get(6)?,
            pages_count: usize::try_from(pages_count).unwrap_or(0),
            sections_count: usize::try_from(sections_count).unwrap_or(0),
            symbols_count: usize::try_from(symbols_count).unwrap_or(0),
            examples_count: usize::try_from(examples_count).unwrap_or(0),
            fetched_at_unix: u64::try_from(fetched_at_unix).unwrap_or(0),
            expires_at_unix: u64::try_from(expires_at_unix).unwrap_or(0),
            expired: u64::try_from(expires_at_unix).unwrap_or(0) <= now_unix,
        })
    })?;
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub(super) fn load_outdated_docs(
    connection: &Connection,
    now_unix: u64,
) -> Result<DocsOutdatedReport> {
    let now_i64 = i64::try_from(now_unix)?;
    let mut statement = connection.prepare(
        "SELECT corpus_id, corpus_kind, label, source_profile, expires_at_unix
         FROM docs_corpora
         WHERE refresh_kind != 'static' AND expires_at_unix <= ?1
         ORDER BY corpus_kind ASC, label ASC",
    )?;
    let rows = statement.query_map(params![now_i64], |row| {
        let expires_at_unix: i64 = row.get(4)?;
        Ok(DocsOutdatedCorpus {
            corpus_id: row.get(0)?,
            corpus_kind: row.get(1)?,
            label: row.get(2)?,
            source_profile: row.get(3)?,
            expires_at_unix: u64::try_from(expires_at_unix).unwrap_or(0),
        })
    })?;
    let mut corpora = Vec::new();
    for row in rows {
        corpora.push(row?);
    }
    Ok(DocsOutdatedReport { corpora })
}

pub(super) fn load_outdated_refresh_rows(paths: &ResolvedPaths) -> Result<Vec<OutdatedRefreshRow>> {
    let connection = open_docs_connection(paths)?;
    let now_unix = unix_timestamp()?;
    let now_i64 = i64::try_from(now_unix)?;
    let mut statement = connection.prepare(
        "SELECT label, refresh_kind, refresh_spec, source_profile, corpus_kind
         FROM docs_corpora
         WHERE refresh_kind != 'static' AND expires_at_unix <= ?1
         ORDER BY CASE refresh_kind WHEN 'profile' THEN 0 ELSE 1 END, label ASC",
    )?;
    let rows = statement.query_map(params![now_i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    let mut profile_refreshes = BTreeSet::new();
    for row in rows {
        let (label, refresh_kind, refresh_spec, source_profile, corpus_kind) = row?;
        if refresh_kind == "profile" {
            profile_refreshes.insert(source_profile.clone());
        }
        out.push((
            label,
            refresh_kind,
            refresh_spec,
            source_profile,
            corpus_kind,
        ));
    }

    Ok(out
        .into_iter()
        .filter(|(_, refresh_kind, _, source_profile, corpus_kind)| {
            !(refresh_kind == "extension"
                && corpus_kind == "extension"
                && !source_profile.is_empty()
                && profile_refreshes.contains(source_profile))
        })
        .map(
            |(label, refresh_kind, refresh_spec, _, _)| OutdatedRefreshRow {
                label,
                refresh_kind,
                refresh_spec,
            },
        )
        .collect())
}
