pub use super::model::{
    AuthoringDocsContext, AuthoringInventory, AuthoringKnowledgePack,
    AuthoringKnowledgePackOptions, AuthoringKnowledgePackResult, AuthoringPageCandidate,
    AuthoringSuggestion, ModuleFunctionUsage, ModuleInvocationExample, ModuleUsageSummary,
    StubTemplateHint,
};
use super::prelude::*;
use crate::content_store::parsing;
use crate::knowledge::references::{
    MediaUsageSummary, ReferenceUsageSummary, summarize_media_usage_for_sources,
    summarize_reference_usage_for_sources,
};
use crate::knowledge::retrieval::{
    LocalSearchHit, RetrievedChunk, build_related_page_weight_map, build_template_match_score_map,
    load_reference_authority_page_hits, load_reference_identifier_page_hits, query_search_fts,
    query_search_like, retrieve_reranked_chunks_across_pages,
};
use crate::knowledge::templates::{
    TemplateReference, TemplateUsageSummary, build_authoring_module_patterns,
    collect_authoring_template_reference_titles, load_authoring_template_references,
    load_page_summary_for_connection, summarize_template_usage_for_sources,
};

pub fn build_authoring_knowledge_pack(
    paths: &ResolvedPaths,
    topic: Option<&str>,
    stub_content: Option<&str>,
    options: &AuthoringKnowledgePackOptions,
) -> Result<AuthoringKnowledgePack> {
    let connection = match parsing::open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(AuthoringKnowledgePack::IndexMissing),
    };

    let normalized_topic = topic
        .map(|value| parsing::normalize_spaces(&value.replace('_', " ")))
        .unwrap_or_default();
    let (stub_link_titles, stub_template_titles) = analyze_stub_hints(stub_content);

    let topic = if !normalized_topic.is_empty() {
        normalized_topic
    } else if let Some(first_link) = stub_link_titles.first() {
        first_link.clone()
    } else {
        String::new()
    };
    if topic.is_empty() {
        return Ok(AuthoringKnowledgePack::QueryMissing);
    }

    let related_limit = options.related_page_limit.max(1);
    let chunk_limit = options.chunk_limit.max(1);
    let token_budget = options.token_budget.max(1);
    let max_pages = options.max_pages.max(1);
    let link_limit = options.link_limit.max(1);
    let category_limit = options.category_limit.max(1);
    let template_limit = options.template_limit.max(1);

    let query_terms = expand_authoring_query_terms(&topic, &stub_link_titles);
    if query_terms.is_empty() {
        return Ok(AuthoringKnowledgePack::QueryMissing);
    }
    let query = query_terms[0].clone();
    let template_page_weights = build_template_match_score_map(&connection, &stub_template_titles)?;
    let semantic_page_hits = load_semantic_page_hits(&connection, &query_terms, related_limit)?;
    let authority_page_hits =
        load_reference_authority_page_hits(&connection, &query_terms, related_limit)?;
    let identifier_page_hits =
        load_reference_identifier_page_hits(&connection, &query_terms, related_limit)?;
    let semantic_page_weights = build_semantic_page_weight_map(&semantic_page_hits);
    let authority_page_weights = build_authority_page_weight_map(&authority_page_hits);
    let identifier_page_weights = build_identifier_page_weight_map(&identifier_page_hits);

    let related_pages = collect_related_pages_for_authoring(
        &connection,
        AuthoringRelatedPageInputs {
            stub_link_titles: &stub_link_titles,
            query_terms: &query_terms,
            limit: related_limit,
            template_page_scores: &template_page_weights,
            semantic_page_hits: &semantic_page_hits,
            authority_page_hits: &authority_page_hits,
            identifier_page_hits: &identifier_page_hits,
        },
    )?;

    let mut stub_existing_links = Vec::new();
    let mut stub_missing_links = Vec::new();
    for link in stub_link_titles {
        if let Some(page) =
            parsing::load_page_record(&connection, &parsing::normalize_query_title(&link))?
        {
            stub_existing_links.push(page.title);
        } else {
            stub_missing_links.push(link);
        }
    }
    stub_existing_links.sort();
    stub_existing_links.dedup();
    stub_missing_links.sort();
    stub_missing_links.dedup();

    let stub_detected_templates = stub_template_titles;
    let related_page_weights = build_related_page_weight_map(&related_pages, &stub_existing_links);
    let chunk_report = retrieve_reranked_chunks_across_pages(
        &connection,
        paths,
        &query,
        &query_terms,
        ChunkRetrievalPlan {
            limit: chunk_limit,
            token_budget,
            max_pages,
            diversify: options.diversify,
        },
        &related_pages
            .iter()
            .map(|page| page.title.clone())
            .collect::<Vec<_>>(),
        ChunkRerankSignals {
            related_page_weights,
            template_page_weights,
            semantic_page_weights,
            authority_page_weights,
            identifier_page_weights,
        },
    )?;
    let mut retrieval_mode = chunk_report.retrieval_mode;
    let chunks = chunk_report.chunks;
    let token_estimate_total = chunk_report.token_estimate_total;

    let mut source_titles = Vec::new();
    let mut seen_source_titles = BTreeSet::new();
    for page in &related_pages {
        if seen_source_titles.insert(page.title.to_ascii_lowercase()) {
            source_titles.push(page.title.clone());
        }
    }
    for chunk in &chunks {
        if seen_source_titles.insert(chunk.source_title.to_ascii_lowercase()) {
            source_titles.push(chunk.source_title.clone());
        }
    }
    for link in &stub_existing_links {
        if seen_source_titles.insert(link.to_ascii_lowercase()) {
            source_titles.push(link.clone());
        }
    }

    let mut suggested_links =
        query_suggested_main_links_for_sources(&connection, &source_titles, link_limit)?;
    let mut seen_suggested_links = suggested_links
        .iter()
        .map(|suggestion| suggestion.title.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    for page in &related_pages {
        if suggested_links.len() >= link_limit {
            break;
        }
        if page.namespace == Namespace::Main.as_str()
            && !page.is_redirect
            && seen_suggested_links.insert(page.title.to_ascii_lowercase())
        {
            suggested_links.push(AuthoringSuggestion {
                title: page.title.clone(),
                support_count: 1,
                evidence_titles: vec![page.title.clone()],
            });
        }
    }
    for chunk in &chunks {
        if suggested_links.len() >= link_limit {
            break;
        }
        if chunk.source_namespace != Namespace::Main.as_str() {
            continue;
        }
        if seen_suggested_links.insert(chunk.source_title.to_ascii_lowercase()) {
            suggested_links.push(AuthoringSuggestion {
                title: chunk.source_title.clone(),
                support_count: 1,
                evidence_titles: vec![chunk.source_title.clone()],
            });
        }
    }
    suggested_links.truncate(link_limit);

    let suggested_categories =
        query_suggested_categories_for_sources(&connection, &source_titles, category_limit)?;
    let suggested_templates =
        summarize_template_usage_for_sources(&connection, Some(&source_titles), template_limit)?;
    let suggested_references = summarize_reference_usage_for_sources(
        &connection,
        &source_titles,
        AUTHORING_REFERENCE_LIMIT,
    )?;
    let suggested_media =
        summarize_media_usage_for_sources(&connection, &source_titles, AUTHORING_MEDIA_LIMIT)?;
    let template_baseline =
        summarize_template_usage_for_sources(&connection, None, template_limit)?;
    let template_reference_titles = collect_authoring_template_reference_titles(
        &stub_detected_templates,
        &suggested_templates,
        &template_baseline,
        AUTHORING_TEMPLATE_REFERENCE_LIMIT,
    );
    let template_references = load_authoring_template_references(
        &connection,
        &template_reference_titles,
        AUTHORING_TEMPLATE_REFERENCE_LIMIT,
    )?;
    let module_patterns = build_authoring_module_patterns(
        &connection,
        &source_titles,
        &template_references,
        AUTHORING_MODULE_PATTERN_LIMIT,
    )?;
    let docs_context = crate::knowledge::docs_bridge::build_authoring_docs_context(
        paths,
        &topic,
        &query_terms,
        &template_references,
        &module_patterns,
        &options.docs_profile,
    )?;
    if !template_references.is_empty() {
        retrieval_mode.push_str("+template-guides");
    }
    if !module_patterns.is_empty() {
        retrieval_mode.push_str("+module-patterns");
    }
    if docs_context.is_some() {
        retrieval_mode.push_str("+docs-bridge");
    }

    let inventory = load_authoring_inventory(&connection)?;
    let pack_token_estimate_total = estimate_authoring_pack_total(AuthoringPackEstimateInputs {
        related_pages: &related_pages,
        suggested_links: &suggested_links,
        suggested_categories: &suggested_categories,
        suggested_templates: &suggested_templates,
        suggested_references: &suggested_references,
        suggested_media: &suggested_media,
        template_baseline: &template_baseline,
        template_references: &template_references,
        module_patterns: &module_patterns,
        docs_context: docs_context.as_ref(),
        stub_detected_templates: &stub_detected_templates,
        chunks: &chunks,
    });

    Ok(AuthoringKnowledgePack::Found(Box::new(
        AuthoringKnowledgePackResult {
            topic,
            query,
            query_terms,
            inventory,
            pack_token_budget: token_budget,
            pack_token_estimate_total,
            related_pages,
            suggested_links,
            suggested_categories,
            suggested_templates,
            suggested_references,
            suggested_media,
            template_baseline,
            template_references,
            module_patterns,
            docs_context,
            stub_existing_links,
            stub_missing_links,
            stub_detected_templates,
            retrieval_mode,
            chunks,
            token_estimate_total,
        },
    )))
}

fn analyze_stub_hints(stub_content: Option<&str>) -> (Vec<String>, Vec<StubTemplateHint>) {
    let Some(content) = stub_content else {
        return (Vec::new(), Vec::new());
    };

    let mut links = BTreeSet::new();
    for link in parsing::extract_wikilinks(content) {
        let normalized = parsing::normalize_query_title(&link.target_title);
        if !normalized.is_empty() {
            links.insert(normalized);
        }
    }

    let mut templates = BTreeMap::<String, BTreeSet<String>>::new();
    for invocation in parsing::extract_template_invocations(content) {
        let entry = templates.entry(invocation.template_title).or_default();
        for key in invocation.parameter_keys {
            entry.insert(key);
        }
    }

    (
        links.into_iter().collect(),
        templates
            .into_iter()
            .map(|(template_title, parameter_keys)| StubTemplateHint {
                template_title,
                parameter_keys: parameter_keys.into_iter().collect(),
            })
            .collect(),
    )
}

fn query_local_search_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<LocalSearchHit>> {
    let normalized = parsing::normalize_spaces(&query.replace('_', " "));
    if normalized.is_empty() {
        return Ok(Vec::new());
    }
    if parsing::fts_table_exists(connection, "indexed_pages_fts")
        && let Ok(hits) = query_search_fts(connection, &normalized, limit)
        && !hits.is_empty()
    {
        return Ok(hits);
    }
    query_search_like(connection, &normalized, limit)
}

#[derive(Default)]
struct AuthoringPageAccumulator {
    title: String,
    namespace: String,
    is_redirect: bool,
    relative_path: String,
    score: usize,
    sources: BTreeSet<String>,
}

struct AuthoringRelatedPageInputs<'a> {
    stub_link_titles: &'a [String],
    query_terms: &'a [String],
    limit: usize,
    template_page_scores: &'a BTreeMap<String, usize>,
    semantic_page_hits: &'a [SemanticPageHit],
    authority_page_hits: &'a [SemanticPageHit],
    identifier_page_hits: &'a [SemanticPageHit],
}

fn collect_related_pages_for_authoring(
    connection: &Connection,
    inputs: AuthoringRelatedPageInputs<'_>,
) -> Result<Vec<AuthoringPageCandidate>> {
    let mut candidates = BTreeMap::<String, AuthoringPageAccumulator>::new();
    let search_limit = candidate_limit(inputs.limit.max(1), 2);

    for title in inputs.stub_link_titles {
        let normalized = parsing::normalize_query_title(title);
        if normalized.is_empty() {
            continue;
        }
        if let Some(page) = parsing::load_page_record(connection, &normalized)? {
            add_authoring_page_candidate(&mut candidates, page, "stub-link", 400);
        }
    }

    let mut ranked_template_matches = inputs.template_page_scores.iter().collect::<Vec<_>>();
    ranked_template_matches.sort_by(|(left_title, left_score), (right_title, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_title.cmp(right_title))
    });
    for (title, score) in ranked_template_matches.into_iter().take(search_limit) {
        if let Some(page) = parsing::load_page_record(connection, title)? {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "template-match",
                (*score).clamp(32, 260),
            );
        }
    }

    for semantic_hit in inputs.semantic_page_hits {
        if let Some(page) = parsing::load_page_record(connection, &semantic_hit.title)? {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "semantic-profile",
                semantic_hit.retrieval_weight.clamp(24, 260),
            );
        }
    }

    for authority_hit in inputs.authority_page_hits {
        if let Some(page) = parsing::load_page_record(connection, &authority_hit.title)? {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "source-authority",
                authority_hit.retrieval_weight.clamp(20, 240),
            );
        }
    }

    for identifier_hit in inputs.identifier_page_hits {
        if let Some(page) = parsing::load_page_record(connection, &identifier_hit.title)? {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "source-identifier",
                identifier_hit.retrieval_weight.clamp(24, 280),
            );
        }
    }

    for (query_index, term) in inputs.query_terms.iter().enumerate() {
        let title_search_score = 240usize.saturating_sub(query_index.saturating_mul(20));
        for (rank, hit) in query_local_search_for_connection(connection, term, search_limit)?
            .into_iter()
            .enumerate()
        {
            let normalized = parsing::normalize_query_title(&hit.title);
            if normalized.is_empty() {
                continue;
            }
            if let Some(page) = parsing::load_page_record(connection, &normalized)? {
                let score = title_search_score
                    .saturating_sub(rank.saturating_mul(12))
                    .max(24);
                add_authoring_page_candidate(&mut candidates, page, "title-search", score);
            }
        }

        let alias_search_score = 200usize.saturating_sub(query_index.saturating_mul(16));
        for (rank, page) in
            query_page_records_from_aliases_for_connection(connection, term, search_limit)?
                .into_iter()
                .enumerate()
        {
            let score = alias_search_score
                .saturating_sub(rank.saturating_mul(10))
                .max(20);
            add_authoring_page_candidate(&mut candidates, page, "alias-search", score);
        }

        let section_search_score = 160usize.saturating_sub(query_index.saturating_mul(12));
        for (rank, page) in
            query_page_records_from_sections_for_connection(connection, term, search_limit)?
                .into_iter()
                .enumerate()
        {
            let score = section_search_score
                .saturating_sub(rank.saturating_mul(8))
                .max(16);
            add_authoring_page_candidate(&mut candidates, page, "section-search", score);
        }
    }

    let mut ranked = candidates.into_values().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.title.cmp(&right.title))
    });
    ranked.truncate(inputs.limit);

    ranked
        .into_iter()
        .map(|candidate| {
            Ok(AuthoringPageCandidate {
                title: candidate.title,
                namespace: candidate.namespace,
                is_redirect: candidate.is_redirect,
                source: candidate.sources.into_iter().collect::<Vec<_>>().join("+"),
                retrieval_weight: candidate.score,
                summary: load_page_summary_for_connection(connection, &candidate.relative_path)?,
            })
        })
        .collect()
}

fn add_authoring_page_candidate(
    candidates: &mut BTreeMap<String, AuthoringPageAccumulator>,
    page: IndexedPageRecord,
    source: &str,
    score: usize,
) {
    let key = page.title.to_ascii_lowercase();
    let entry = candidates.entry(key).or_default();
    if entry.title.is_empty() {
        entry.title = page.title;
        entry.namespace = page.namespace;
        entry.is_redirect = page.is_redirect;
        entry.relative_path = page.relative_path;
    }
    entry.score = entry.score.saturating_add(score);
    entry.sources.insert(source.to_string());
}

fn expand_authoring_query_terms(topic: &str, stub_link_titles: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    push_authoring_query_term(&mut out, &mut seen, topic);
    if let Some((_, body)) = topic.split_once(':') {
        push_authoring_query_term(&mut out, &mut seen, body);
    }
    for token in parsing::normalize_spaces(&topic.replace('_', " ")).split_whitespace() {
        if token.len() >= 4 {
            push_authoring_query_term(&mut out, &mut seen, token);
        }
    }
    for title in stub_link_titles {
        if out.len() >= AUTHORING_QUERY_EXPANSION_LIMIT {
            break;
        }
        push_authoring_query_term(&mut out, &mut seen, title);
        if let Some((_, body)) = title.split_once(':') {
            push_authoring_query_term(&mut out, &mut seen, body);
        }
    }

    out
}

pub(crate) fn push_authoring_query_term(
    out: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    value: &str,
) {
    if out.len() >= AUTHORING_QUERY_EXPANSION_LIMIT {
        return;
    }
    let normalized = parsing::normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return;
    }
    let key = normalized.to_ascii_lowercase();
    if !seen.insert(key) {
        return;
    }
    out.push(normalized);
}

#[derive(Default)]
struct SuggestionAccumulator {
    evidence_titles: BTreeSet<String>,
}

fn query_suggested_main_links_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<AuthoringSuggestion>> {
    query_suggestions_for_sources(
        connection,
        source_titles,
        limit,
        false,
        Some(Namespace::Main.as_str()),
    )
}

fn query_suggested_categories_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<AuthoringSuggestion>> {
    query_suggestions_for_sources(connection, source_titles, limit, true, None)
}

fn query_suggestions_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
    category_membership: bool,
    target_namespace: Option<&str>,
) -> Result<Vec<AuthoringSuggestion>> {
    if source_titles.is_empty() || limit == 0 || !table_exists(connection, "indexed_links")? {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "SELECT target_title, source_title
         FROM indexed_links
         WHERE source_title IN ({placeholders})
           AND is_category_membership = ?"
    );
    if target_namespace.is_some() {
        sql.push_str(" AND target_namespace = ?");
    }
    sql.push_str(" ORDER BY target_title ASC, source_title ASC");

    let mut values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    values.push(rusqlite::types::Value::from(if category_membership {
        1i64
    } else {
        0i64
    }));
    if let Some(namespace) = target_namespace {
        values.push(rusqlite::types::Value::from(namespace.to_string()));
    }

    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare suggestion query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .context("failed to run suggestion query")?;

    let mut accumulators = BTreeMap::<String, SuggestionAccumulator>::new();
    for row in rows {
        let (target_title, source_title) = row.context("failed to decode suggestion row")?;
        accumulators
            .entry(target_title)
            .or_default()
            .evidence_titles
            .insert(source_title);
    }

    let mut out = accumulators
        .into_iter()
        .map(|(title, accumulator)| AuthoringSuggestion {
            support_count: accumulator.evidence_titles.len(),
            evidence_titles: accumulator
                .evidence_titles
                .into_iter()
                .take(AUTHORING_SUGGESTION_EVIDENCE_LIMIT)
                .collect(),
            title,
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| {
        right
            .support_count
            .cmp(&left.support_count)
            .then_with(|| left.title.cmp(&right.title))
    });
    out.truncate(limit);
    Ok(out)
}

struct AuthoringPackEstimateInputs<'a> {
    related_pages: &'a [AuthoringPageCandidate],
    suggested_links: &'a [AuthoringSuggestion],
    suggested_categories: &'a [AuthoringSuggestion],
    suggested_templates: &'a [TemplateUsageSummary],
    suggested_references: &'a [ReferenceUsageSummary],
    suggested_media: &'a [MediaUsageSummary],
    template_baseline: &'a [TemplateUsageSummary],
    template_references: &'a [TemplateReference],
    module_patterns: &'a [ModuleUsageSummary],
    docs_context: Option<&'a AuthoringDocsContext>,
    stub_detected_templates: &'a [StubTemplateHint],
    chunks: &'a [RetrievedChunk],
}

fn estimate_authoring_pack_total(inputs: AuthoringPackEstimateInputs<'_>) -> usize {
    let page_summary_tokens = inputs
        .related_pages
        .iter()
        .map(|page| parsing::estimate_tokens(&page.summary))
        .sum::<usize>();
    let link_tokens = inputs
        .suggested_links
        .iter()
        .map(|suggestion| parsing::estimate_tokens(&suggestion.title))
        .sum::<usize>();
    let category_tokens = inputs
        .suggested_categories
        .iter()
        .map(|suggestion| parsing::estimate_tokens(&suggestion.title))
        .sum::<usize>();
    let template_tokens = inputs
        .suggested_templates
        .iter()
        .chain(inputs.template_baseline.iter())
        .map(|template| {
            parsing::estimate_tokens(&template.template_title)
                + template
                    .parameter_stats
                    .iter()
                    .map(|stat| {
                        parsing::estimate_tokens(&stat.key)
                            + stat
                                .example_values
                                .iter()
                                .map(|value| parsing::estimate_tokens(value))
                                .sum::<usize>()
                    })
                    .sum::<usize>()
                + template
                    .implementation_titles
                    .iter()
                    .map(|title| parsing::estimate_tokens(title))
                    .sum::<usize>()
                + template
                    .implementation_preview
                    .as_deref()
                    .map(estimate_tokens)
                    .unwrap_or(0)
                + template
                    .example_invocations
                    .iter()
                    .map(|example| example.token_estimate)
                    .sum::<usize>()
        })
        .sum::<usize>();
    let template_reference_tokens = inputs
        .template_references
        .iter()
        .map(|reference| {
            parsing::estimate_tokens(&reference.template.template_title)
                + reference
                    .implementation_pages
                    .iter()
                    .map(|page| {
                        parsing::estimate_tokens(&page.page_title)
                            + parsing::estimate_tokens(&page.summary_text)
                            + page
                                .context_chunks
                                .iter()
                                .map(|chunk| chunk.token_estimate)
                                .sum::<usize>()
                    })
                    .sum::<usize>()
        })
        .sum::<usize>();
    let module_tokens = inputs
        .module_patterns
        .iter()
        .map(|module| {
            parsing::estimate_tokens(&module.module_title)
                + module
                    .function_stats
                    .iter()
                    .map(|function| {
                        parsing::estimate_tokens(&function.function_name)
                            + function
                                .example_parameter_keys
                                .iter()
                                .map(|key| parsing::estimate_tokens(key))
                                .sum::<usize>()
                    })
                    .sum::<usize>()
                + module
                    .example_invocations
                    .iter()
                    .map(|example| example.token_estimate)
                    .sum::<usize>()
        })
        .sum::<usize>();
    let reference_tokens = inputs
        .suggested_references
        .iter()
        .map(|reference| {
            parsing::estimate_tokens(&reference.citation_profile)
                + parsing::estimate_tokens(&reference.citation_family)
                + parsing::estimate_tokens(&reference.source_type)
                + parsing::estimate_tokens(&reference.source_origin)
                + parsing::estimate_tokens(&reference.source_family)
                + reference
                    .common_templates
                    .iter()
                    .map(|template| parsing::estimate_tokens(template))
                    .sum::<usize>()
                + reference
                    .common_links
                    .iter()
                    .map(|title| parsing::estimate_tokens(title))
                    .sum::<usize>()
                + reference
                    .common_domains
                    .iter()
                    .map(|domain| parsing::estimate_tokens(domain))
                    .sum::<usize>()
                + reference
                    .common_authorities
                    .iter()
                    .map(|authority| parsing::estimate_tokens(authority))
                    .sum::<usize>()
                + reference
                    .common_identifier_keys
                    .iter()
                    .map(|identifier| parsing::estimate_tokens(identifier))
                    .sum::<usize>()
                + reference
                    .common_identifier_entries
                    .iter()
                    .map(|identifier| parsing::estimate_tokens(identifier))
                    .sum::<usize>()
                + reference
                    .common_retrieval_signals
                    .iter()
                    .map(|flag| parsing::estimate_tokens(flag))
                    .sum::<usize>()
                + reference
                    .example_references
                    .iter()
                    .map(|example| example.token_estimate)
                    .sum::<usize>()
        })
        .sum::<usize>();
    let media_tokens = inputs
        .suggested_media
        .iter()
        .map(|media| {
            parsing::estimate_tokens(&media.file_title)
                + media
                    .example_usages
                    .iter()
                    .map(|example| example.token_estimate)
                    .sum::<usize>()
        })
        .sum::<usize>();
    let stub_template_tokens = inputs
        .stub_detected_templates
        .iter()
        .map(|template| {
            parsing::estimate_tokens(&template.template_title)
                + template
                    .parameter_keys
                    .iter()
                    .map(|key| parsing::estimate_tokens(key))
                    .sum::<usize>()
        })
        .sum::<usize>();
    let chunk_tokens = inputs
        .chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();
    let docs_tokens = inputs
        .docs_context
        .map(|context| context.token_estimate_total)
        .unwrap_or(0);

    page_summary_tokens
        .saturating_add(link_tokens)
        .saturating_add(category_tokens)
        .saturating_add(template_tokens)
        .saturating_add(template_reference_tokens)
        .saturating_add(module_tokens)
        .saturating_add(reference_tokens)
        .saturating_add(media_tokens)
        .saturating_add(docs_tokens)
        .saturating_add(stub_template_tokens)
        .saturating_add(chunk_tokens)
}

fn load_authoring_inventory(connection: &Connection) -> Result<AuthoringInventory> {
    let indexed_pages_total =
        parsing::count_query(connection, "SELECT COUNT(*) FROM indexed_pages")
            .context("failed to count indexed pages for authoring inventory")?;
    let semantic_profiles_total = if table_exists(connection, "indexed_page_semantics")? {
        parsing::count_query(connection, "SELECT COUNT(*) FROM indexed_page_semantics")
            .context("failed to count semantic profiles for authoring inventory")?
    } else {
        0
    };
    let main_pages = parsing::count_query(
        connection,
        "SELECT COUNT(*) FROM indexed_pages WHERE namespace = 'Main'",
    )
    .context("failed to count main pages for authoring inventory")?;
    let template_pages = parsing::count_query(
        connection,
        "SELECT COUNT(*) FROM indexed_pages WHERE namespace = 'Template'",
    )
    .context("failed to count template pages for authoring inventory")?;
    let indexed_links_total = if table_exists(connection, "indexed_links")? {
        parsing::count_query(connection, "SELECT COUNT(*) FROM indexed_links")
            .context("failed to count indexed links for authoring inventory")?
    } else {
        0
    };

    let (template_invocation_rows, distinct_templates_invoked) =
        if table_exists(connection, "indexed_template_invocations")? {
            (
                parsing::count_query(
                    connection,
                    "SELECT COUNT(*) FROM indexed_template_invocations",
                )
                .context("failed to count template invocation rows for authoring inventory")?,
                parsing::count_query(
                    connection,
                    "SELECT COUNT(DISTINCT template_title) FROM indexed_template_invocations",
                )
                .context("failed to count distinct templates for authoring inventory")?,
            )
        } else {
            (0, 0)
        };
    let (module_invocation_rows_total, distinct_modules_invoked) =
        if table_exists(connection, "indexed_module_invocations")? {
            (
                parsing::count_query(
                    connection,
                    "SELECT COUNT(*) FROM indexed_module_invocations",
                )
                .context("failed to count module invocation rows for authoring inventory")?,
                parsing::count_query(
                    connection,
                    "SELECT COUNT(DISTINCT module_title) FROM indexed_module_invocations",
                )
                .context("failed to count distinct modules for authoring inventory")?,
            )
        } else {
            (0, 0)
        };
    let (reference_rows_total, distinct_reference_profiles) =
        if table_exists(connection, "indexed_page_references")? {
            (
                parsing::count_query(connection, "SELECT COUNT(*) FROM indexed_page_references")
                    .context("failed to count reference rows for authoring inventory")?,
                parsing::count_query(
                    connection,
                    "SELECT COUNT(DISTINCT citation_profile) FROM indexed_page_references",
                )
                .context("failed to count distinct reference profiles for authoring inventory")?,
            )
        } else {
            (0, 0)
        };
    let reference_authority_rows_total =
        if table_exists(connection, "indexed_reference_authorities")? {
            parsing::count_query(
                connection,
                "SELECT COUNT(*) FROM indexed_reference_authorities",
            )
            .context("failed to count reference authority rows for authoring inventory")?
        } else {
            0
        };
    let reference_identifier_rows_total =
        if table_exists(connection, "indexed_reference_identifiers")? {
            parsing::count_query(
                connection,
                "SELECT COUNT(*) FROM indexed_reference_identifiers",
            )
            .context("failed to count reference identifier rows for authoring inventory")?
        } else {
            0
        };
    let (media_rows_total, distinct_media_files) =
        if table_exists(connection, "indexed_page_media")? {
            (
                parsing::count_query(connection, "SELECT COUNT(*) FROM indexed_page_media")
                    .context("failed to count media rows for authoring inventory")?,
                parsing::count_query(
                    connection,
                    "SELECT COUNT(DISTINCT file_title) FROM indexed_page_media",
                )
                .context("failed to count distinct media files for authoring inventory")?,
            )
        } else {
            (0, 0)
        };
    let template_implementation_rows_total =
        if table_exists(connection, "indexed_template_implementation_pages")? {
            parsing::count_query(
                connection,
                "SELECT COUNT(*) FROM indexed_template_implementation_pages",
            )
            .context("failed to count template implementation rows for authoring inventory")?
        } else {
            0
        };

    Ok(AuthoringInventory {
        indexed_pages_total,
        semantic_profiles_total,
        main_pages,
        template_pages,
        indexed_links_total,
        template_invocation_rows,
        distinct_templates_invoked,
        module_invocation_rows_total,
        distinct_modules_invoked,
        reference_rows_total,
        reference_authority_rows_total,
        reference_identifier_rows_total,
        distinct_reference_profiles,
        media_rows_total,
        distinct_media_files,
        template_implementation_rows_total,
    })
}
