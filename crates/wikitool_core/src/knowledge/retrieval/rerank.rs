use super::*;

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
    audience: RetrievalAudience,
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
            score += match audience {
                RetrievalAudience::Authoring => {
                    if chunk.source_namespace == Namespace::Main.as_str() {
                        48
                    } else {
                        -120
                    }
                }
                RetrievalAudience::General => {
                    if chunk.source_namespace == Namespace::Main.as_str() {
                        18
                    } else if chunk.source_namespace == Namespace::Category.as_str() {
                        -28
                    } else {
                        0
                    }
                }
            };
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
            if audience == RetrievalAudience::Authoring {
                score +=
                    section_authoring_bias(chunk.section_heading.as_deref(), &chunk.chunk_text);
            }
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
    let audience = plan.audience;
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
        audience,
    )?);
    let candidates = candidates
        .into_iter()
        .filter_map(|chunk| sanitize_chunk_for_audience(chunk, audience))
        .collect::<Vec<_>>();
    let reranked = rerank_retrieved_chunks(candidates, query, query_terms, &signals, audience);
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
        } else if audience == RetrievalAudience::Authoring {
            format!("{retrieval_mode}+seed-pages+authoring-curated")
        } else {
            format!("{retrieval_mode}+seed-pages")
        },
        max_pages: capped_max_pages,
        source_page_count,
        chunks,
        token_estimate_total,
    })
}
