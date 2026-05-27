use super::*;

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
    if is_translation_variant(&normalized_title) {
        bail!(unsupported_translation_message(&normalized_title));
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
            audience: RetrievalAudience::General,
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
    audience: RetrievalAudience,
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
            let chunk = RetrievedChunk {
                source_title: page.title.clone(),
                source_namespace: page.namespace.clone(),
                source_relative_path: page.relative_path.clone(),
                section_heading: chunk.section_heading,
                token_estimate: chunk.token_estimate,
                chunk_text: chunk.chunk_text,
            };
            if chunk_allowed_for_audience(&chunk, audience) {
                out.push(chunk);
            }
        }
    }
    Ok(out)
}
