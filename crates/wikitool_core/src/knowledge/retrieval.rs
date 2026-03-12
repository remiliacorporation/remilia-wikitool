use super::prelude::*;
use crate::knowledge::model::{AuthoringPageCandidate, StubTemplateHint};
use crate::knowledge::authoring::push_authoring_query_term;
use crate::knowledge::references::{LocalMediaUsage, LocalReferenceUsage};
use crate::knowledge::templates::{
    load_template_invocation_rows_for_template, normalize_template_lookup_title,
};
pub use super::model::{
    LocalChunkAcrossPagesResult, LocalChunkAcrossRetrieval, LocalChunkRetrieval,
    LocalChunkRetrievalResult, LocalContextBundle, LocalContextChunk, LocalContextHeading,
    LocalSearchHit, LocalSectionSummary, LocalTemplateInvocation, RetrievedChunk,
};

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
    if fts_table_exists(&connection, "indexed_pages_fts")
        && let Ok(hits) = query_search_fts(&connection, &normalized, limit)
        && !hits.is_empty()
    {
        return Ok(Some(hits));
    }

    // Fallback to LIKE-based search
    query_search_like(&connection, &normalized, limit).map(Some)
}

pub(crate) fn query_search_fts(
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

pub(crate) fn query_search_like(
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
    let link_rows = load_outgoing_link_rows(&connection, &page.relative_path)?;
    let backlinks = query_backlinks_for_connection(&connection, &page.title)?;
    let mut content = None;

    let section_rows =
        if let Some(rows) = load_section_records_for_bundle(&connection, &page.relative_path)? {
            rows
        } else {
            let loaded = load_page_content(paths, &page.relative_path)?;
            let rows = extract_section_records(&loaded);
            content = Some(loaded);
            rows
        };
    let sections = section_rows
        .iter()
        .filter_map(|section| {
            let heading = section.section_heading.as_ref()?.clone();
            Some(LocalContextHeading {
                level: section.section_level,
                heading,
            })
        })
        .take(AUTHORING_SECTION_LIMIT)
        .collect::<Vec<_>>();
    let section_summaries = section_rows
        .iter()
        .take(AUTHORING_SECTION_LIMIT)
        .map(|section| LocalSectionSummary {
            section_heading: section.section_heading.clone(),
            section_level: section.section_level,
            summary_text: section.summary_text.clone(),
            token_estimate: section.token_estimate,
        })
        .collect::<Vec<_>>();
    let word_count = section_rows
        .iter()
        .map(|section| count_words(&section.section_text))
        .sum::<usize>();
    let content_preview = section_rows
        .iter()
        .find_map(|section| {
            let summary = normalize_spaces(&section.summary_text);
            if summary.is_empty() {
                None
            } else {
                Some(summary)
            }
        })
        .unwrap_or_else(|| {
            let loaded = content.get_or_insert(String::new());
            make_content_preview(loaded, 280)
        });
    let context_chunks =
        match load_context_chunks_for_bundle(&connection, &page.relative_path, content.as_deref())?
        {
            Some(chunks) => chunks,
            None => {
                let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
                fallback_context_chunks_from_content(loaded)
            }
        };
    let context_tokens_estimate = context_chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();
    let template_invocations =
        match load_template_invocations_for_bundle(&connection, &page.relative_path)? {
            Some(invocations) => invocations,
            None => {
                let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
                summarize_template_invocations(
                    extract_template_invocations(loaded),
                    TEMPLATE_INVOCATION_LIMIT,
                )
            }
        };
    let references = match load_references_for_bundle(&connection, &page.relative_path)? {
        Some(references) => references,
        None => {
            let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
            extract_reference_records(loaded)
        }
    };
    let media = match load_media_for_bundle(&connection, &page.relative_path)? {
        Some(media) => media,
        None => {
            let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
            extract_media_records(loaded)
        }
    };

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
    for invocation in &template_invocations {
        template_set.insert(invocation.template_title.clone());
    }

    Ok(Some(LocalContextBundle {
        title: page.title,
        namespace: page.namespace,
        is_redirect: page.is_redirect,
        redirect_target: page.redirect_target,
        relative_path: page.relative_path,
        bytes: page.bytes,
        word_count,
        content_preview,
        sections,
        section_summaries,
        context_chunks,
        context_tokens_estimate,
        outgoing_links: outgoing_set.into_iter().collect(),
        backlinks,
        categories: category_set.into_iter().collect(),
        templates: template_set.into_iter().collect(),
        modules: module_set.into_iter().collect(),
        template_invocations,
        references,
        media,
    }))
}

pub fn retrieve_local_context_chunks(
    paths: &ResolvedPaths,
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
) -> Result<LocalChunkRetrieval> {
    retrieve_local_context_chunks_with_options(paths, title, query, limit, token_budget, true)
}

pub fn retrieve_local_context_chunks_with_options(
    paths: &ResolvedPaths,
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
    diversify: bool,
) -> Result<LocalChunkRetrieval> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(LocalChunkRetrieval::IndexMissing),
    };
    let normalized_title = normalize_query_title(title);
    if normalized_title.is_empty() {
        return Ok(LocalChunkRetrieval::TitleMissing {
            title: title.to_string(),
        });
    }
    let page = match load_page_record(&connection, &normalized_title)? {
        Some(page) => page,
        None => {
            return Ok(LocalChunkRetrieval::TitleMissing {
                title: normalized_title,
            });
        }
    };
    let normalized_query = query
        .map(|value| normalize_spaces(&value.replace('_', " ")))
        .filter(|value| !value.is_empty());
    let max_chunks = limit.max(1);
    let max_tokens = token_budget.max(1);
    let candidate_limit = candidate_limit(max_chunks, CHUNK_CANDIDATE_MULTIPLIER_SINGLE);
    let (chunks, retrieval_mode) = load_chunks_for_query(
        paths,
        &connection,
        &page.relative_path,
        normalized_query.as_deref(),
        candidate_limit,
    )?;
    let chunk_candidates = chunks
        .into_iter()
        .map(|chunk| RetrievedChunk {
            source_title: page.title.clone(),
            source_namespace: page.namespace.clone(),
            source_relative_path: page.relative_path.clone(),
            section_heading: chunk.section_heading,
            token_estimate: chunk.token_estimate,
            chunk_text: chunk.chunk_text,
        })
        .collect::<Vec<_>>();
    let selected = select_retrieved_chunks(
        chunk_candidates,
        max_chunks,
        max_tokens,
        diversify,
        Some(1),
        false,
    );
    let chunks = selected
        .into_iter()
        .map(|chunk| LocalContextChunk {
            section_heading: chunk.section_heading,
            token_estimate: chunk.token_estimate,
            chunk_text: chunk.chunk_text,
        })
        .collect::<Vec<_>>();
    let token_estimate_total = chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();

    Ok(LocalChunkRetrieval::Found(LocalChunkRetrievalResult {
        title: page.title,
        namespace: page.namespace,
        relative_path: page.relative_path,
        query: normalized_query,
        retrieval_mode,
        chunks,
        token_estimate_total,
    }))
}

pub fn retrieve_local_context_chunks_across_pages(
    paths: &ResolvedPaths,
    query: &str,
    limit: usize,
    token_budget: usize,
    max_pages: usize,
    diversify: bool,
) -> Result<LocalChunkAcrossRetrieval> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(LocalChunkAcrossRetrieval::IndexMissing),
    };
    let normalized_query = normalize_spaces(&query.replace('_', " "));
    if normalized_query.is_empty() {
        return Ok(LocalChunkAcrossRetrieval::QueryMissing);
    }
    let query_terms = expand_retrieval_query_terms(&normalized_query);
    let semantic_page_hits =
        load_semantic_page_hits(&connection, &query_terms, max_pages.max(limit).max(1))?;
    let authority_page_hits =
        load_reference_authority_page_hits(&connection, &query_terms, max_pages.max(limit).max(1))?;
    let identifier_page_hits = load_reference_identifier_page_hits(
        &connection,
        &query_terms,
        max_pages.max(limit).max(1),
    )?;
    let mut seed_pages = BTreeSet::new();
    let mut related_page_titles = Vec::new();
    for title in semantic_page_hits
        .iter()
        .chain(authority_page_hits.iter())
        .chain(identifier_page_hits.iter())
        .map(|hit| hit.title.clone())
    {
        if seed_pages.insert(title.to_ascii_lowercase()) {
            related_page_titles.push(title);
        }
    }
    let report = retrieve_reranked_chunks_across_pages(
        &connection,
        paths,
        &normalized_query,
        &query_terms,
        ChunkRetrievalPlan {
            limit,
            token_budget,
            max_pages,
            diversify,
        },
        &related_page_titles,
        ChunkRerankSignals {
            semantic_page_weights: build_semantic_page_weight_map(&semantic_page_hits),
            authority_page_weights: build_authority_page_weight_map(&authority_page_hits),
            identifier_page_weights: build_identifier_page_weight_map(&identifier_page_hits),
            ..ChunkRerankSignals::default()
        },
    )?;
    Ok(LocalChunkAcrossRetrieval::Found(report))
}

pub(crate) fn load_context_chunks_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
    content: Option<&str>,
) -> Result<Option<Vec<LocalContextChunk>>> {
    if table_exists(connection, "indexed_page_chunks")? {
        let db_rows = load_indexed_context_chunks_for_connection(
            connection,
            source_relative_path,
            CONTEXT_CHUNK_LIMIT,
            CONTEXT_TOKEN_BUDGET,
        )?;
        if !db_rows.is_empty() {
            return Ok(Some(db_rows));
        }
    }
    Ok(content.map(fallback_context_chunks_from_content))
}

pub(crate) fn load_template_invocations_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<LocalTemplateInvocation>>> {
    if table_exists(connection, "indexed_template_invocations")? {
        let db_rows = load_indexed_template_invocations_for_connection(
            connection,
            source_relative_path,
            TEMPLATE_INVOCATION_LIMIT,
        )?;
        if !db_rows.is_empty() {
            return Ok(Some(db_rows));
        }
    }
    Ok(None)
}

pub(crate) fn load_references_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<LocalReferenceUsage>>> {
    if !table_exists(connection, "indexed_page_references")? {
        return Ok(None);
    }
    let rows = load_indexed_reference_rows_for_connection(
        connection,
        source_relative_path,
        CONTEXT_REFERENCE_LIMIT,
    )?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rows))
    }
}

pub(crate) fn load_media_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<LocalMediaUsage>>> {
    if !table_exists(connection, "indexed_page_media")? {
        return Ok(None);
    }
    let rows = load_indexed_media_rows_for_connection(
        connection,
        source_relative_path,
        CONTEXT_MEDIA_LIMIT,
    )?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rows))
    }
}

pub(crate) fn load_section_records_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<IndexedSectionRecord>>> {
    if !table_exists(connection, "indexed_page_sections")? {
        return Ok(None);
    }
    let rows = load_indexed_section_rows_for_connection(
        connection,
        source_relative_path,
        AUTHORING_SECTION_LIMIT,
    )?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rows))
    }
}

pub(crate) fn fallback_context_chunks_from_content(content: &str) -> Vec<LocalContextChunk> {
    let fallback_rows = chunk_article_context(content);
    apply_context_chunk_budget(
        fallback_rows
            .into_iter()
            .map(|row| LocalContextChunk {
                section_heading: row.section_heading,
                token_estimate: row.token_estimate,
                chunk_text: row.chunk_text,
            })
            .collect(),
        CONTEXT_CHUNK_LIMIT,
        CONTEXT_TOKEN_BUDGET,
    )
}

pub(crate) fn load_indexed_section_rows_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    limit: usize,
) -> Result<Vec<IndexedSectionRecord>> {
    let limit_i64 = i64::try_from(limit).context("section limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT section_heading, section_level, summary_text, section_text, token_estimate
             FROM indexed_page_sections
             WHERE source_relative_path = ?1
             ORDER BY section_index ASC
             LIMIT ?2",
        )
        .context("failed to prepare indexed_page_sections query")?;
    let rows = statement
        .query_map(params![source_relative_path, limit_i64], |row| {
            let section_level_i64: i64 = row.get(1)?;
            let token_estimate_i64: i64 = row.get(4)?;
            Ok(IndexedSectionRecord {
                section_heading: row.get(0)?,
                section_level: u8::try_from(section_level_i64).unwrap_or(1),
                summary_text: row.get(2)?,
                section_text: row.get(3)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run indexed_page_sections query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode indexed_page_sections row")?);
    }
    Ok(out)
}

pub(crate) fn load_page_content(paths: &ResolvedPaths, source_relative_path: &str) -> Result<String> {
    let absolute = absolute_path_from_relative(paths, source_relative_path);
    validate_scoped_path(paths, &absolute)?;
    fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))
}

pub(crate) fn load_chunks_for_query(
    paths: &ResolvedPaths,
    connection: &Connection,
    source_relative_path: &str,
    normalized_query: Option<&str>,
    limit: usize,
) -> Result<(Vec<LocalContextChunk>, String)> {
    if table_exists(connection, "indexed_page_chunks")? {
        if let Some(query) = normalized_query {
            if fts_table_exists(connection, "indexed_page_chunks_fts")
                && let Ok(hits) = query_page_chunks_fts_for_connection(
                    connection,
                    source_relative_path,
                    query,
                    limit,
                )
                && !hits.is_empty()
            {
                return Ok((hits, "fts".to_string()));
            }

            let hits = query_page_chunks_like_for_connection(
                connection,
                source_relative_path,
                query,
                limit,
            )?;
            return Ok((hits, "like".to_string()));
        }

        let hits = load_indexed_context_chunks_for_connection(
            connection,
            source_relative_path,
            limit,
            usize::MAX,
        )?;
        return Ok((hits, "ordered".to_string()));
    }

    let absolute = absolute_path_from_relative(paths, source_relative_path);
    validate_scoped_path(paths, &absolute)?;
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))?;
    let mut chunks = chunk_article_context(&content)
        .into_iter()
        .map(|row| LocalContextChunk {
            section_heading: row.section_heading,
            token_estimate: row.token_estimate,
            chunk_text: row.chunk_text,
        })
        .collect::<Vec<_>>();
    if let Some(query) = normalized_query {
        let lowered = query.to_ascii_lowercase();
        chunks.retain(|chunk| chunk.chunk_text.to_ascii_lowercase().contains(&lowered));
        return Ok((chunks, "scan-like".to_string()));
    }
    Ok((chunks, "scan-ordered".to_string()))
}

pub(crate) fn select_retrieved_chunks(
    candidates: Vec<RetrievedChunk>,
    limit: usize,
    token_budget: usize,
    diversify: bool,
    max_pages: Option<usize>,
    round_robin_pages: bool,
) -> Vec<RetrievedChunk> {
    let capped_limit = limit.max(1);
    let capped_token_budget = token_budget.max(1);
    let max_pages = max_pages.map(|value| value.max(1));

    let mut candidates = candidates
        .into_iter()
        .map(|chunk| {
            let lexical_terms = lexical_terms(&chunk.chunk_text);
            RetrievedChunkCandidate {
                lexical_signature: lexical_signature_from_terms(&lexical_terms),
                lexical_terms,
                chunk,
            }
        })
        .collect::<Vec<_>>();
    if round_robin_pages && max_pages.is_some() {
        candidates = round_robin_by_source(candidates, max_pages.unwrap_or(1));
    }

    let mut out = Vec::new();
    let mut used_tokens = 0usize;
    let mut used_signatures = BTreeSet::<String>::new();
    let mut selected_terms = Vec::<BTreeSet<String>>::new();
    let mut selected_pages = BTreeSet::<String>::new();

    for candidate in candidates {
        if out.len() >= capped_limit {
            break;
        }
        if used_signatures.contains(&candidate.lexical_signature) {
            continue;
        }
        if let Some(max_pages) = max_pages
            && !selected_pages.contains(&candidate.chunk.source_relative_path)
            && selected_pages.len() >= max_pages
        {
            continue;
        }
        if diversify
            && !selected_terms.is_empty()
            && selected_terms.iter().any(|terms| {
                lexical_similarity_terms(terms, &candidate.lexical_terms)
                    >= CHUNK_LEXICAL_SIMILARITY_THRESHOLD
            })
        {
            continue;
        }

        let next_tokens = used_tokens.saturating_add(candidate.chunk.token_estimate);
        if !out.is_empty() && next_tokens > capped_token_budget {
            continue;
        }

        used_tokens = next_tokens;
        used_signatures.insert(candidate.lexical_signature);
        selected_terms.push(candidate.lexical_terms);
        selected_pages.insert(candidate.chunk.source_relative_path.clone());
        out.push(candidate.chunk);
    }

    out
}

pub(crate) fn round_robin_by_source(
    candidates: Vec<RetrievedChunkCandidate>,
    max_pages: usize,
) -> Vec<RetrievedChunkCandidate> {
    let mut source_order = Vec::<String>::new();
    let mut buckets =
        BTreeMap::<String, std::collections::VecDeque<RetrievedChunkCandidate>>::new();
    for candidate in candidates {
        let source = candidate.chunk.source_relative_path.clone();
        if !buckets.contains_key(&source) {
            if source_order.len() >= max_pages {
                continue;
            }
            source_order.push(source.clone());
        }
        buckets.entry(source).or_default().push_back(candidate);
    }

    let mut out = Vec::new();
    loop {
        let mut made_progress = false;
        for source in &source_order {
            if let Some(bucket) = buckets.get_mut(source)
                && let Some(candidate) = bucket.pop_front()
            {
                out.push(candidate);
                made_progress = true;
            }
        }
        if !made_progress {
            break;
        }
    }
    out
}

pub(crate) fn lexical_signature_from_terms(terms: &BTreeSet<String>) -> String {
    terms.iter().cloned().collect::<Vec<_>>().join(" ")
}

pub(crate) fn lexical_terms(value: &str) -> BTreeSet<String> {
    value
        .split_whitespace()
        .map(|token| {
            token
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|token| token.len() >= 3)
        .collect()
}

pub(crate) fn lexical_similarity_terms(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(right).count();
    let union = left.union(right).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f32 / union as f32
}

pub(crate) fn query_chunks_fts_across_pages_for_connection(
    connection: &Connection,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<RetrievedChunk>> {
    let limit_i64 = i64::try_from(limit).context("chunk query limit does not fit into i64")?;
    let fts_query = format!("\"{normalized_query}\" *");
    let mut statement = connection
        .prepare(
            "SELECT c.source_title, c.source_namespace, c.source_relative_path, c.section_heading, c.token_estimate, c.chunk_text
             FROM indexed_page_chunks_fts fts
             JOIN indexed_page_chunks c ON c.rowid = fts.rowid
             WHERE indexed_page_chunks_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .context("failed to prepare cross-page chunk FTS query")?;
    let rows = statement
        .query_map(params![fts_query, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(4)?;
            Ok(RetrievedChunk {
                source_title: row.get(0)?,
                source_namespace: row.get(1)?,
                source_relative_path: row.get(2)?,
                section_heading: row.get(3)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                chunk_text: row.get(5)?,
            })
        })
        .context("failed to run cross-page chunk FTS query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode cross-page chunk FTS row")?);
    }
    Ok(out)
}

pub(crate) fn query_chunks_like_across_pages_for_connection(
    connection: &Connection,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<RetrievedChunk>> {
    let wildcard = format!("%{normalized_query}%");
    let prefix = format!("{normalized_query}%");
    let limit_i64 = i64::try_from(limit).context("chunk query limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT source_title, source_namespace, source_relative_path, section_heading, token_estimate, chunk_text
             FROM indexed_page_chunks
             WHERE lower(chunk_text) LIKE lower(?1)
             ORDER BY
               CASE
                 WHEN lower(chunk_text) LIKE lower(?2) THEN 0
                 ELSE 1
               END,
               source_title ASC,
               chunk_index ASC
             LIMIT ?3",
        )
        .context("failed to prepare cross-page chunk LIKE query")?;
    let rows = statement
        .query_map(params![wildcard, prefix, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(4)?;
            Ok(RetrievedChunk {
                source_title: row.get(0)?,
                source_namespace: row.get(1)?,
                source_relative_path: row.get(2)?,
                section_heading: row.get(3)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                chunk_text: row.get(5)?,
            })
        })
        .context("failed to run cross-page chunk LIKE query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode cross-page chunk LIKE row")?);
    }
    Ok(out)
}

pub(crate) fn query_chunks_scan_across_pages(
    paths: &ResolvedPaths,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<RetrievedChunk>> {
    let lowered_query = normalized_query.to_ascii_lowercase();
    let files = scan_files(paths, &ScanOptions::default())?;
    let mut out = Vec::new();
    for file in files {
        let content = load_scanned_file_content(paths, &file)?;
        for chunk in chunk_article_context(&content) {
            if !chunk
                .chunk_text
                .to_ascii_lowercase()
                .contains(&lowered_query)
            {
                continue;
            }
            out.push(RetrievedChunk {
                source_title: file.title.clone(),
                source_namespace: file.namespace.clone(),
                source_relative_path: file.relative_path.clone(),
                section_heading: chunk.section_heading,
                token_estimate: chunk.token_estimate,
                chunk_text: chunk.chunk_text,
            });
            if out.len() >= limit {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

pub(crate) fn expand_retrieval_query_terms(query: &str) -> Vec<String> {
    let normalized = normalize_spaces(&query.replace('_', " "));
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    push_authoring_query_term(&mut out, &mut seen, &normalized);
    if let Some((_, body)) = normalized.split_once(':') {
        push_authoring_query_term(&mut out, &mut seen, body);
    }
    for token in normalized.split_whitespace() {
        if token.len() >= 4 {
            push_authoring_query_term(&mut out, &mut seen, token);
        }
    }
    out
}

pub(crate) fn collect_chunk_candidates_across_pages(
    connection: &Connection,
    paths: &ResolvedPaths,
    query_terms: &[String],
    candidate_cap: usize,
) -> Result<(Vec<RetrievedChunk>, String)> {
    let mut candidates = Vec::new();
    let mut modes = BTreeSet::new();

    if table_exists(connection, "indexed_page_chunks")? {
        let has_fts = fts_table_exists(connection, "indexed_page_chunks_fts");
        for term in query_terms {
            if has_fts {
                let hits =
                    query_chunks_fts_across_pages_for_connection(connection, term, candidate_cap)?;
                if !hits.is_empty() {
                    modes.insert("fts");
                    candidates.extend(hits);
                    continue;
                }
            }
            let hits =
                query_chunks_like_across_pages_for_connection(connection, term, candidate_cap)?;
            if !hits.is_empty() {
                modes.insert("like");
                candidates.extend(hits);
            }
        }
    } else {
        for term in query_terms {
            let hits = query_chunks_scan_across_pages(paths, term, candidate_cap)?;
            if !hits.is_empty() {
                modes.insert("scan");
                candidates.extend(hits);
            }
        }
    }

    let retrieval_mode = if modes.is_empty() {
        "hybrid-rerank-across".to_string()
    } else {
        format!(
            "hybrid-{}-rerank-across",
            modes.into_iter().collect::<Vec<_>>().join("+")
        )
    };
    Ok((candidates, retrieval_mode))
}

pub(crate) fn build_related_page_weight_map(
    related_pages: &[AuthoringPageCandidate],
    seed_titles: &[String],
) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::<String, usize>::new();
    for page in related_pages {
        out.insert(
            page.title.to_ascii_lowercase(),
            page.retrieval_weight.clamp(1, 240),
        );
    }
    for title in seed_titles {
        out.entry(title.to_ascii_lowercase()).or_insert(160);
    }
    out
}

pub(crate) fn build_template_match_score_map(
    connection: &Connection,
    stub_templates: &[StubTemplateHint],
) -> Result<BTreeMap<String, usize>> {
    if stub_templates.is_empty() || !table_exists(connection, "indexed_template_invocations")? {
        return Ok(BTreeMap::new());
    }

    let mut out = BTreeMap::<String, usize>::new();
    for hint in stub_templates {
        let template_title = normalize_template_lookup_title(&hint.template_title);
        if template_title.is_empty() {
            continue;
        }
        let stub_keys = hint
            .parameter_keys
            .iter()
            .map(|key| normalize_template_parameter_key(key))
            .collect::<BTreeSet<_>>();
        for (source_title, parameter_keys_serialized) in
            load_template_invocation_rows_for_template(connection, &template_title)?
        {
            let page_key = source_title.to_ascii_lowercase();
            let invocation_keys = parse_parameter_key_list(&parameter_keys_serialized)
                .into_iter()
                .map(|key| normalize_template_parameter_key(&key))
                .collect::<BTreeSet<_>>();
            let overlap = if stub_keys.is_empty() {
                0
            } else {
                stub_keys.intersection(&invocation_keys).count()
            };
            let mut score = 72usize;
            if overlap > 0 {
                score = score.saturating_add(overlap.saturating_mul(18));
            }
            if !stub_keys.is_empty() && overlap >= stub_keys.len().min(3) {
                score = score.saturating_add(24);
            }
            let entry = out.entry(page_key).or_insert(0);
            *entry = (*entry).saturating_add(score);
        }
    }
    Ok(out)
}

pub(crate) fn query_page_records_from_reference_authorities_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_reference_authorities")? {
        return Ok(Vec::new());
    }

    let normalized = normalize_spaces(&query.replace('_', " "));
    if normalized.is_empty() {
        return Ok(Vec::new());
    }
    let limit_i64 =
        i64::try_from(limit).context("reference authority query limit does not fit into i64")?;
    if fts_table_exists(connection, "indexed_reference_authorities_fts") {
        let fts_query = format!("\"{}\" *", normalized);
        let mut statement = connection
            .prepare(
                "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
                 FROM indexed_reference_authorities_fts fts
                 JOIN indexed_reference_authorities a ON a.rowid = fts.rowid
                 JOIN indexed_pages p ON p.relative_path = a.source_relative_path
                 WHERE indexed_reference_authorities_fts MATCH ?1
                 GROUP BY p.relative_path
                 ORDER BY COUNT(*) DESC, p.title ASC
                 LIMIT ?2",
            )
            .context("failed to prepare reference authority FTS query")?;
        let rows = statement
            .query_map(params![fts_query, limit_i64], decode_page_record_row)
            .context("failed to run reference authority FTS query")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("failed to decode reference authority FTS row")?);
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    let wildcard = format!("%{normalized}%");
    let prefix = format!("{normalized}%");
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_reference_authorities a
             JOIN indexed_pages p ON p.relative_path = a.source_relative_path
             WHERE lower(a.authority_label) LIKE lower(?1)
                OR lower(a.retrieval_text) LIKE lower(?1)
                OR lower(a.source_family) LIKE lower(?1)
                OR lower(a.source_domain) LIKE lower(?1)
                OR lower(a.source_container) LIKE lower(?1)
                OR lower(a.source_author) LIKE lower(?1)
             GROUP BY p.relative_path
             ORDER BY
               CASE
                 WHEN lower(a.authority_label) = lower(?2) THEN 0
                 WHEN lower(a.authority_label) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               COUNT(*) DESC,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare reference authority LIKE query")?;
    let rows = statement
        .query_map(
            params![wildcard, normalized, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run reference authority LIKE query")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode reference authority LIKE row")?);
    }
    Ok(out)
}

pub(crate) fn load_reference_authority_page_hits(
    connection: &Connection,
    query_terms: &[String],
    limit: usize,
) -> Result<Vec<SemanticPageHit>> {
    if limit == 0 || query_terms.is_empty() {
        return Ok(Vec::new());
    }

    let search_limit = candidate_limit(limit.max(1), 2);
    let mut weights = BTreeMap::<String, usize>::new();
    let mut titles = BTreeMap::<String, String>::new();
    for (query_index, term) in query_terms.iter().enumerate() {
        let base_weight = 200usize
            .saturating_sub(query_index.saturating_mul(16))
            .max(36);
        for (rank, page) in query_page_records_from_reference_authorities_for_connection(
            connection,
            term,
            search_limit,
        )?
        .into_iter()
        .enumerate()
        {
            let key = page.title.to_ascii_lowercase();
            titles.entry(key.clone()).or_insert(page.title);
            let weight = base_weight.saturating_sub(rank.saturating_mul(12)).max(18);
            let entry = weights.entry(key).or_insert(0);
            *entry = entry.saturating_add(weight);
        }
    }

    materialize_page_hits(weights, titles, limit)
}

pub(crate) fn query_page_records_from_reference_identifiers_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_reference_identifiers")? {
        return Ok(Vec::new());
    }

    let normalized_query = normalize_reference_identifier_search_term(query);
    if normalized_query.is_empty() {
        return Ok(Vec::new());
    }
    let limit_i64 =
        i64::try_from(limit).context("reference identifier query limit does not fit into i64")?;
    let wildcard = format!("%{normalized_query}%");
    let prefix = format!("{normalized_query}%");
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_reference_identifiers i
             JOIN indexed_pages p ON p.relative_path = i.source_relative_path
             WHERE lower(i.normalized_value) = lower(?1)
                OR lower(i.normalized_value) LIKE lower(?2)
                OR lower(i.identifier_value) LIKE lower(?2)
             GROUP BY p.relative_path
             ORDER BY
               CASE
                 WHEN lower(i.normalized_value) = lower(?1) THEN 0
                 WHEN lower(i.normalized_value) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               COUNT(*) DESC,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare reference identifier query")?;
    let rows = statement
        .query_map(
            params![normalized_query, wildcard, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run reference identifier query")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode reference identifier row")?);
    }
    Ok(out)
}

pub(crate) fn load_reference_identifier_page_hits(
    connection: &Connection,
    query_terms: &[String],
    limit: usize,
) -> Result<Vec<SemanticPageHit>> {
    if limit == 0 || query_terms.is_empty() {
        return Ok(Vec::new());
    }

    let search_limit = candidate_limit(limit.max(1), 2);
    let mut weights = BTreeMap::<String, usize>::new();
    let mut titles = BTreeMap::<String, String>::new();
    for (query_index, term) in query_terms.iter().enumerate() {
        let base_weight = 240usize
            .saturating_sub(query_index.saturating_mul(20))
            .max(48);
        for (rank, page) in query_page_records_from_reference_identifiers_for_connection(
            connection,
            term,
            search_limit,
        )?
        .into_iter()
        .enumerate()
        {
            let key = page.title.to_ascii_lowercase();
            titles.entry(key.clone()).or_insert(page.title);
            let weight = base_weight.saturating_sub(rank.saturating_mul(16)).max(24);
            let entry = weights.entry(key).or_insert(0);
            *entry = entry.saturating_add(weight);
        }
    }

    materialize_page_hits(weights, titles, limit)
}

pub(crate) fn normalize_reference_identifier_search_term(query: &str) -> String {
    let normalized = normalize_spaces(&query.replace('_', " "));
    if normalized.is_empty() {
        return String::new();
    }
    if let Some((key, value)) = normalized.split_once(':') {
        let key = normalize_template_parameter_key(key);
        if !key.is_empty() {
            let normalized_value = normalize_reference_identifier_value(&key, value);
            if !normalized_value.is_empty() {
                return normalized_value;
            }
        }
    }

    normalize_reference_identifier_token(&normalized, true)
}

pub(crate) fn load_seed_chunks_for_related_pages(
    connection: &Connection,
    related_page_titles: &[String],
    per_page_limit: usize,
) -> Result<Vec<RetrievedChunk>> {
    if related_page_titles.is_empty()
        || per_page_limit == 0
        || !table_exists(connection, "indexed_page_chunks")?
    {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for title in related_page_titles {
        let Some(page) = load_page_record(connection, title)? else {
            continue;
        };
        let rows = load_indexed_context_chunks_for_connection(
            connection,
            &page.relative_path,
            per_page_limit,
            usize::MAX,
        )?;
        for chunk in rows {
            out.push(RetrievedChunk {
                source_title: page.title.clone(),
                source_namespace: page.namespace.clone(),
                source_relative_path: page.relative_path.clone(),
                section_heading: chunk.section_heading,
                token_estimate: chunk.token_estimate,
                chunk_text: chunk.chunk_text,
            });
        }
    }
    Ok(out)
}

pub(crate) fn section_authoring_bias(section_heading: Option<&str>, chunk_text: &str) -> i64 {
    let heading = section_heading.unwrap_or_default().to_ascii_lowercase();
    let text = chunk_text.to_ascii_lowercase();

    let mut score = if heading.is_empty() { 32 } else { 0 };
    for low_signal in [
        "references",
        "notes",
        "external links",
        "further reading",
        "bibliography",
        "gallery",
        "see also",
    ] {
        if heading.contains(low_signal) {
            score -= 120;
        }
    }
    for high_signal in [
        "history",
        "background",
        "overview",
        "biography",
        "profile",
        "works",
        "career",
        "philosophy",
    ] {
        if heading.contains(high_signal) {
            score += 24;
        }
    }
    if text.contains("{{reflist") || text.contains("[[category:") {
        score -= 120;
    }
    score
}

pub(crate) fn rerank_retrieved_chunks(
    candidates: Vec<RetrievedChunk>,
    query: &str,
    query_terms: &[String],
    signals: &ChunkRerankSignals,
) -> Vec<RetrievedChunk> {
    let normalized_query = query.to_ascii_lowercase();
    let mut deduped = BTreeMap::<String, RetrievedChunk>::new();
    for chunk in candidates {
        let key = format!(
            "{}\u{1f}{}\u{1f}{}",
            chunk.source_relative_path,
            chunk.section_heading.as_deref().unwrap_or_default(),
            chunk.chunk_text
        );
        deduped.entry(key).or_insert(chunk);
    }

    let mut scored = deduped
        .into_values()
        .map(|chunk| {
            let mut score = 0i64;
            let title = chunk.source_title.to_ascii_lowercase();
            let section = chunk
                .section_heading
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let text = chunk.chunk_text.to_ascii_lowercase();

            if !normalized_query.is_empty() {
                if title == normalized_query {
                    score += 220;
                } else if title.contains(&normalized_query) {
                    score += 140;
                }
                if section.contains(&normalized_query) {
                    score += 90;
                }
                if text.contains(&normalized_query) {
                    score += 120;
                }
            }

            let mut coverage = 0usize;
            for (index, term) in query_terms.iter().enumerate() {
                let term = term.to_ascii_lowercase();
                if term.is_empty() {
                    continue;
                }
                let weight = 36usize.saturating_sub(index.saturating_mul(4)).max(8);
                let mut matched = false;
                if title == term {
                    score += i64::try_from(weight.saturating_mul(4)).unwrap_or(0);
                    matched = true;
                } else if title.contains(&term) {
                    score += i64::try_from(weight.saturating_mul(2)).unwrap_or(0);
                    matched = true;
                }
                if section.contains(&term) {
                    score += i64::try_from(weight.saturating_add(24)).unwrap_or(0);
                    matched = true;
                }
                if text.contains(&term) {
                    score += i64::try_from(weight.saturating_add(12)).unwrap_or(0);
                    matched = true;
                }
                if matched {
                    coverage = coverage.saturating_add(1);
                }
            }
            score += i64::try_from(coverage.saturating_mul(28)).unwrap_or(0);
            if !query_terms.is_empty() && coverage >= query_terms.len().min(3) {
                score += 60;
            }
            if chunk.source_namespace == Namespace::Main.as_str() {
                score += 18;
            } else {
                score -= 20;
            }
            score += i64::try_from(
                signals
                    .related_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score += i64::try_from(
                signals
                    .template_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score += i64::try_from(
                signals
                    .authority_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score += i64::try_from(
                signals
                    .identifier_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score += i64::try_from(
                signals
                    .semantic_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score +=
                i64::try_from(48usize.saturating_sub(chunk.token_estimate.min(48))).unwrap_or(0);
            score += section_authoring_bias(chunk.section_heading.as_deref(), &chunk.chunk_text);
            (score, chunk)
        })
        .collect::<Vec<_>>();

    scored.sort_by(|(left_score, left_chunk), (right_score, right_chunk)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_chunk.source_title.cmp(&right_chunk.source_title))
            .then_with(|| left_chunk.section_heading.cmp(&right_chunk.section_heading))
            .then_with(|| left_chunk.chunk_text.cmp(&right_chunk.chunk_text))
    });
    scored.into_iter().map(|(_, chunk)| chunk).collect()
}

pub(crate) fn retrieve_reranked_chunks_across_pages(
    connection: &Connection,
    paths: &ResolvedPaths,
    query: &str,
    query_terms: &[String],
    plan: ChunkRetrievalPlan,
    related_page_titles: &[String],
    signals: ChunkRerankSignals,
) -> Result<LocalChunkAcrossPagesResult> {
    let max_chunks = plan.limit.max(1);
    let max_tokens = plan.token_budget.max(1);
    let capped_max_pages = plan.max_pages.max(1);
    let candidate_cap = candidate_limit(
        max_chunks.saturating_mul(query_terms.len().max(1)),
        CHUNK_CANDIDATE_MULTIPLIER_ACROSS,
    );
    let (mut candidates, retrieval_mode) =
        collect_chunk_candidates_across_pages(connection, paths, query_terms, candidate_cap)?;
    candidates.extend(load_seed_chunks_for_related_pages(
        connection,
        related_page_titles,
        AUTHORING_SEED_CHUNKS_PER_PAGE,
    )?);
    let reranked = rerank_retrieved_chunks(candidates, query, query_terms, &signals);
    let chunks = select_retrieved_chunks(
        reranked,
        max_chunks,
        max_tokens,
        plan.diversify,
        Some(capped_max_pages),
        true,
    );
    let source_page_count = chunks
        .iter()
        .map(|chunk| chunk.source_relative_path.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let token_estimate_total = chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();

    let mut retrieval_mode = retrieval_mode;
    if !signals.semantic_page_weights.is_empty() {
        retrieval_mode = format!("{retrieval_mode}+semantic");
    }
    if !signals.authority_page_weights.is_empty() {
        retrieval_mode = format!("{retrieval_mode}+authority");
    }
    if !signals.identifier_page_weights.is_empty() {
        retrieval_mode = format!("{retrieval_mode}+identifier");
    }

    Ok(LocalChunkAcrossPagesResult {
        query: query.to_string(),
        retrieval_mode: if related_page_titles.is_empty() {
            retrieval_mode
        } else {
            format!("{retrieval_mode}+seed-pages")
        },
        max_pages: capped_max_pages,
        source_page_count,
        chunks,
        token_estimate_total,
    })
}



