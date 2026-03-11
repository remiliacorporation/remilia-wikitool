use super::*;
use super::model::*;

pub(crate) fn query_page_records_from_sections_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_page_sections")? {
        return Ok(Vec::new());
    }

    let limit_i64 = i64::try_from(limit).context("section query limit does not fit into i64")?;
    if crate::index::parsing::fts_table_exists(connection, "indexed_page_sections_fts") {
        let fts_query = format!("\"{}\" *", crate::index::parsing::normalize_spaces(&query.replace('_', " ")));
        let mut statement = connection
            .prepare(
                "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
                 FROM indexed_page_sections_fts fts
                 JOIN indexed_page_sections s ON s.rowid = fts.rowid
                 JOIN indexed_pages p ON p.relative_path = s.source_relative_path
                 WHERE indexed_page_sections_fts MATCH ?1
                 GROUP BY p.relative_path
                 ORDER BY COUNT(*) DESC, p.title ASC
                 LIMIT ?2",
            )
            .context("failed to prepare section FTS query")?;
        let rows = statement
            .query_map(params![fts_query, limit_i64], decode_page_record_row)
            .context("failed to run section FTS query")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("failed to decode section FTS page row")?);
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    let wildcard = format!("%{query}%");
    let prefix = format!("{query}%");
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_page_sections s
             JOIN indexed_pages p ON p.relative_path = s.source_relative_path
             WHERE lower(s.section_text) LIKE lower(?1)
                OR lower(s.summary_text) LIKE lower(?1)
                OR lower(COALESCE(s.section_heading, '')) LIKE lower(?1)
             GROUP BY p.relative_path
             ORDER BY
               CASE
                 WHEN lower(p.title) = lower(?2) THEN 0
                 WHEN lower(p.title) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               COUNT(*) DESC,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare section LIKE query")?;
    let rows = statement
        .query_map(
            params![wildcard, query, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run section LIKE query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode section LIKE page row")?);
    }
    Ok(out)
}

pub(crate) fn query_page_records_from_semantics_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_page_semantics")? {
        return Ok(Vec::new());
    }

    let limit_i64 = i64::try_from(limit).context("semantic query limit does not fit into i64")?;
    if crate::index::parsing::fts_table_exists(connection, "indexed_page_semantics_fts") {
        let fts_query = format!("\"{}\" *", crate::index::parsing::normalize_spaces(&query.replace('_', " ")));
        let mut statement = connection
            .prepare(
                "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
                 FROM indexed_page_semantics_fts fts
                 JOIN indexed_page_semantics s ON s.rowid = fts.rowid
                 JOIN indexed_pages p ON p.relative_path = s.source_relative_path
                 WHERE indexed_page_semantics_fts MATCH ?1
                 ORDER BY bm25(indexed_page_semantics_fts) ASC, p.title ASC
                 LIMIT ?2",
            )
            .context("failed to prepare semantic FTS query")?;
        let rows = statement
            .query_map(params![fts_query, limit_i64], decode_page_record_row)
            .context("failed to run semantic FTS query")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("failed to decode semantic FTS row")?);
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    let wildcard = format!("%{query}%");
    let prefix = format!("{query}%");
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_page_semantics s
             JOIN indexed_pages p ON p.relative_path = s.source_relative_path
             WHERE lower(s.semantic_text) LIKE lower(?1)
                OR lower(s.summary_text) LIKE lower(?1)
                OR lower(s.source_title) LIKE lower(?1)
             ORDER BY
               CASE
                 WHEN lower(s.source_title) = lower(?2) THEN 0
                 WHEN lower(s.source_title) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare semantic LIKE query")?;
    let rows = statement
        .query_map(
            params![wildcard, query, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run semantic LIKE query")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode semantic LIKE row")?);
    }
    Ok(out)
}

pub(crate) fn load_semantic_page_hits(
    connection: &Connection,
    query_terms: &[String],
    limit: usize,
) -> Result<Vec<SemanticPageHit>> {
    if limit == 0 || query_terms.is_empty() {
        return Ok(Vec::new());
    }

    let search_limit = crate::index::model::candidate_limit(limit.max(1), 2);
    let mut weights = BTreeMap::<String, usize>::new();
    let mut titles = BTreeMap::<String, String>::new();
    for (query_index, term) in query_terms.iter().enumerate() {
        let base_weight = 220usize
            .saturating_sub(query_index.saturating_mul(18))
            .max(40);
        for (rank, page) in
            query_page_records_from_semantics_for_connection(connection, term, search_limit)?
                .into_iter()
                .enumerate()
        {
            let key = page.title.to_ascii_lowercase();
            titles.entry(key.clone()).or_insert(page.title);
            let weight = base_weight.saturating_sub(rank.saturating_mul(14)).max(20);
            let entry = weights.entry(key).or_insert(0);
            *entry = entry.saturating_add(weight);
        }
    }

    materialize_page_hits(weights, titles, limit)
}

pub(crate) fn materialize_page_hits(
    weights: BTreeMap<String, usize>,
    titles: BTreeMap<String, String>,
    limit: usize,
) -> Result<Vec<SemanticPageHit>> {
    let mut hits = weights
        .into_iter()
        .filter_map(|(key, retrieval_weight)| {
            titles.get(&key).map(|title| SemanticPageHit {
                title: title.clone(),
                retrieval_weight,
            })
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.title.cmp(&right.title))
    });
    hits.truncate(limit);
    Ok(hits)
}

pub(crate) fn build_semantic_page_weight_map(semantic_hits: &[SemanticPageHit]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for hit in semantic_hits {
        out.insert(hit.title.to_ascii_lowercase(), hit.retrieval_weight);
    }
    out
}

pub(crate) fn build_authority_page_weight_map(hits: &[SemanticPageHit]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for hit in hits {
        out.insert(hit.title.to_ascii_lowercase(), hit.retrieval_weight);
    }
    out
}

pub(crate) fn build_identifier_page_weight_map(hits: &[SemanticPageHit]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for hit in hits {
        out.insert(hit.title.to_ascii_lowercase(), hit.retrieval_weight);
    }
    out
}

pub(crate) fn query_page_records_from_aliases_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_page_aliases")? {
        return Ok(Vec::new());
    }

    let wildcard = format!("%{query}%");
    let prefix = format!("{query}%");
    let limit_i64 = i64::try_from(limit).context("alias query limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_page_aliases a
             JOIN indexed_pages p ON p.relative_path = a.source_relative_path
             WHERE lower(a.alias_title) LIKE lower(?1)
             GROUP BY p.relative_path
             ORDER BY
               CASE
                 WHEN lower(a.alias_title) = lower(?2) THEN 0
                 WHEN lower(a.alias_title) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare alias page query")?;
    let rows = statement
        .query_map(
            params![wildcard, query, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run alias page query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode alias page row")?);
    }
    Ok(out)
}

pub(crate) fn decode_page_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedPageRecord> {
    let bytes_i64: i64 = row.get(5)?;
    Ok(IndexedPageRecord {
        title: row.get(0)?,
        namespace: row.get(1)?,
        is_redirect: row.get::<_, i64>(2)? == 1,
        redirect_target: row.get(3)?,
        relative_path: row.get(4)?,
        bytes: u64::try_from(bytes_i64).unwrap_or(0),
    })
}

pub(crate) fn load_indexed_context_chunks_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    max_chunks: usize,
    token_budget: usize,
) -> Result<Vec<LocalContextChunk>> {
    let mut statement = connection
        .prepare(
            "SELECT section_heading, token_estimate, chunk_text
             FROM indexed_page_chunks
             WHERE source_relative_path = ?1
             ORDER BY chunk_index ASC",
        )
        .context("failed to prepare indexed_page_chunks query")?;
    let rows = statement
        .query_map([source_relative_path], |row| {
            let token_estimate_i64: i64 = row.get(1)?;
            Ok(IndexedContextChunkRow {
                section_heading: row.get(0)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                chunk_text: row.get(2)?,
            })
        })
        .context("failed to run indexed_page_chunks query")?;

    let mut out = Vec::new();
    for row in rows {
        let row = row.context("failed to decode indexed_page_chunks row")?;
        out.push(LocalContextChunk {
            section_heading: row.section_heading,
            token_estimate: row.token_estimate,
            chunk_text: row.chunk_text,
        });
    }
    Ok(crate::index::parsing::apply_context_chunk_budget(out, max_chunks, token_budget))
}

pub(crate) fn query_page_chunks_fts_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<LocalContextChunk>> {
    let limit_i64 = i64::try_from(limit).context("chunk query limit does not fit into i64")?;
    let fts_query = format!("\"{normalized_query}\" *");
    let mut statement = connection
        .prepare(
            "SELECT c.section_heading, c.token_estimate, c.chunk_text
             FROM indexed_page_chunks_fts fts
             JOIN indexed_page_chunks c ON c.rowid = fts.rowid
             WHERE c.source_relative_path = ?1
               AND indexed_page_chunks_fts MATCH ?2
             ORDER BY rank
             LIMIT ?3",
        )
        .context("failed to prepare chunk FTS query")?;
    let rows = statement
        .query_map(params![source_relative_path, fts_query, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(1)?;
            Ok(LocalContextChunk {
                section_heading: row.get(0)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                chunk_text: row.get(2)?,
            })
        })
        .context("failed to run chunk FTS query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode chunk FTS row")?);
    }
    Ok(out)
}

pub(crate) fn query_page_chunks_like_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<LocalContextChunk>> {
    let wildcard = format!("%{normalized_query}%");
    let prefix = format!("{normalized_query}%");
    let limit_i64 = i64::try_from(limit).context("chunk query limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT section_heading, token_estimate, chunk_text
             FROM indexed_page_chunks
             WHERE source_relative_path = ?1
               AND lower(chunk_text) LIKE lower(?2)
             ORDER BY
               CASE
                 WHEN lower(chunk_text) LIKE lower(?3) THEN 0
                 ELSE 1
               END,
               chunk_index ASC
             LIMIT ?4",
        )
        .context("failed to prepare chunk LIKE query")?;
    let rows = statement
        .query_map(
            params![source_relative_path, wildcard, prefix, limit_i64],
            |row| {
                let token_estimate_i64: i64 = row.get(1)?;
                Ok(LocalContextChunk {
                    section_heading: row.get(0)?,
                    token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                    chunk_text: row.get(2)?,
                })
            },
        )
        .context("failed to run chunk LIKE query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode chunk LIKE row")?);
    }
    Ok(out)
}

pub(crate) fn load_indexed_template_invocations_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    limit: usize,
) -> Result<Vec<LocalTemplateInvocation>> {
    let limit_i64 =
        i64::try_from(limit).context("template invocation limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT template_title, parameter_keys
             FROM indexed_template_invocations
             WHERE source_relative_path = ?1
             ORDER BY template_title ASC, parameter_keys ASC
             LIMIT ?2",
        )
        .context("failed to prepare indexed_template_invocations query")?;
    let rows = statement
        .query_map(params![source_relative_path, limit_i64], |row| {
            let template_title: String = row.get(0)?;
            let parameter_keys: String = row.get(1)?;
            Ok(LocalTemplateInvocation {
                template_title,
                parameter_keys: crate::index::parsing::parse_parameter_key_list(&parameter_keys),
            })
        })
        .context("failed to run indexed_template_invocations query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode indexed_template_invocations row")?);
    }
    Ok(out)
}

pub(crate) fn load_indexed_reference_rows_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    limit: usize,
) -> Result<Vec<LocalReferenceUsage>> {
    let limit_i64 = i64::try_from(limit).context("reference limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT section_heading, reference_name, reference_group, citation_profile, citation_family,
                    primary_template_title, source_type, source_origin, source_family, authority_kind,
                    source_authority, reference_title, source_container, source_author, source_domain,
                    source_date, canonical_url, identifier_keys, identifier_entries, source_urls,
                    retrieval_signals, summary_text, template_titles, link_titles, token_estimate
             FROM indexed_page_references
             WHERE source_relative_path = ?1
             ORDER BY reference_index ASC
             LIMIT ?2",
        )
        .context("failed to prepare indexed_page_references query")?;
    let rows = statement
        .query_map(params![source_relative_path, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(24)?;
            Ok(LocalReferenceUsage {
                section_heading: row.get(0)?,
                reference_name: row.get(1)?,
                reference_group: row.get(2)?,
                citation_profile: row.get(3)?,
                citation_family: row.get(4)?,
                primary_template_title: crate::index::parsing::normalize_non_empty_string(row.get::<_, String>(5)?),
                source_type: row.get(6)?,
                source_origin: row.get(7)?,
                source_family: row.get(8)?,
                authority_kind: row.get(9)?,
                source_authority: row.get(10)?,
                reference_title: row.get(11)?,
                source_container: row.get(12)?,
                source_author: row.get(13)?,
                source_domain: row.get(14)?,
                source_date: row.get(15)?,
                canonical_url: row.get(16)?,
                identifier_keys: crate::index::parsing::parse_string_list(&row.get::<_, String>(17)?),
                identifier_entries: crate::index::parsing::parse_string_list(&row.get::<_, String>(18)?),
                source_urls: crate::index::parsing::parse_string_list(&row.get::<_, String>(19)?),
                retrieval_signals: crate::index::parsing::parse_string_list(&row.get::<_, String>(20)?),
                summary_text: row.get(21)?,
                template_titles: crate::index::parsing::parse_string_list(&row.get::<_, String>(22)?),
                link_titles: crate::index::parsing::parse_string_list(&row.get::<_, String>(23)?),
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run indexed_page_references query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode indexed_page_references row")?);
    }
    Ok(out)
}

pub(crate) fn load_indexed_media_rows_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    limit: usize,
) -> Result<Vec<LocalMediaUsage>> {
    let limit_i64 = i64::try_from(limit).context("media limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT section_heading, file_title, media_kind, caption_text, options_text, token_estimate
             FROM indexed_page_media
             WHERE source_relative_path = ?1
             ORDER BY media_index ASC
             LIMIT ?2",
        )
        .context("failed to prepare indexed_page_media query")?;
    let rows = statement
        .query_map(params![source_relative_path, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(5)?;
            Ok(LocalMediaUsage {
                section_heading: row.get(0)?,
                file_title: row.get(1)?,
                media_kind: row.get(2)?,
                caption_text: row.get(3)?,
                options: crate::index::parsing::parse_string_list(&row.get::<_, String>(4)?),
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run indexed_page_media query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode indexed_page_media row")?);
    }
    Ok(out)
}





