use std::collections::{BTreeMap, BTreeSet};

use crate::content_store::parsing;
use crate::knowledge::prelude::{
    SemanticPageHit, candidate_limit, query_page_records_from_aliases_for_connection,
    query_page_records_from_sections_for_connection,
};
use crate::knowledge::templates::load_page_summary_for_connection;
use crate::knowledge::{model::*, prelude::*};

use super::topic_assessment::query_local_search_for_connection;

#[derive(Default)]
struct AuthoringPageAccumulator {
    title: String,
    namespace: String,
    is_redirect: bool,
    relative_path: String,
    score: usize,
    sources: BTreeSet<String>,
}

pub(crate) struct AuthoringRelatedPageInputs<'a> {
    pub stub_link_titles: &'a [String],
    pub query_terms: &'a [String],
    pub limit: usize,
    pub template_page_scores: &'a BTreeMap<String, usize>,
    pub semantic_page_hits: &'a [SemanticPageHit],
    pub authority_page_hits: &'a [SemanticPageHit],
    pub identifier_page_hits: &'a [SemanticPageHit],
}

pub(crate) fn authoring_page_allowed(page: &IndexedPageRecord) -> bool {
    page.namespace == Namespace::Main.as_str()
        && !page.is_redirect
        && !title_looks_like_translation_subpage(&page.title)
}

pub(crate) fn load_semantic_page_hits(
    connection: &Connection,
    query_terms: &[String],
    limit: usize,
) -> Result<Vec<SemanticPageHit>> {
    crate::knowledge::prelude::load_semantic_page_hits(connection, query_terms, limit)
}

pub(crate) fn collect_related_pages_for_authoring(
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
        if let Some(page) = parsing::load_page_record(connection, &normalized)?
            && authoring_page_allowed(&page)
        {
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
        if let Some(page) = parsing::load_page_record(connection, title)?
            && authoring_page_allowed(&page)
        {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "template-match",
                (*score).clamp(32, 260),
            );
        }
    }

    for semantic_hit in inputs.semantic_page_hits {
        if let Some(page) = parsing::load_page_record(connection, &semantic_hit.title)?
            && authoring_page_allowed(&page)
        {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "semantic-profile",
                semantic_hit.retrieval_weight.clamp(24, 260),
            );
        }
    }

    for authority_hit in inputs.authority_page_hits {
        if let Some(page) = parsing::load_page_record(connection, &authority_hit.title)?
            && authoring_page_allowed(&page)
        {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "source-authority",
                authority_hit.retrieval_weight.clamp(20, 240),
            );
        }
    }

    for identifier_hit in inputs.identifier_page_hits {
        if let Some(page) = parsing::load_page_record(connection, &identifier_hit.title)?
            && authoring_page_allowed(&page)
        {
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
            if let Some(page) = parsing::load_page_record(connection, &normalized)?
                && authoring_page_allowed(&page)
            {
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
            if authoring_page_allowed(&page) {
                let score = alias_search_score
                    .saturating_sub(rank.saturating_mul(10))
                    .max(20);
                add_authoring_page_candidate(&mut candidates, page, "alias-search", score);
            }
        }

        let section_search_score = 160usize.saturating_sub(query_index.saturating_mul(12));
        for (rank, page) in
            query_page_records_from_sections_for_connection(connection, term, search_limit)?
                .into_iter()
                .enumerate()
        {
            if authoring_page_allowed(&page) {
                let score = section_search_score
                    .saturating_sub(rank.saturating_mul(8))
                    .max(16);
                add_authoring_page_candidate(&mut candidates, page, "section-search", score);
            }
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

fn title_looks_like_translation_subpage(title: &str) -> bool {
    let Some((_, suffix)) = title.rsplit_once('/') else {
        return false;
    };
    suffix.len() == 2 && suffix.chars().all(|ch| ch.is_ascii_lowercase())
}
