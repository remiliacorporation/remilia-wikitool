use crate::content_store::parsing;
use crate::knowledge::retrieval::{collapse_search_hits, query_search_fts, query_search_like};
use crate::knowledge::{model::*, prelude::*};

const AUTHORING_TOPIC_SIGNAL_LIMIT: usize = 8;

pub(crate) fn build_topic_assessment(
    connection: &Connection,
    topic: &str,
) -> Result<AuthoringTopicAssessment> {
    let normalized_topic = parsing::normalize_query_title(topic);
    let exact_page =
        parsing::load_page_record(connection, &normalized_topic)?.map(|page| LocalSearchHit {
            title: page.title,
            namespace: page.namespace,
            is_redirect: page.is_redirect,
            translation_languages: Vec::new(),
            matched_translation_language: None,
        });
    let mut local_title_hits = query_local_search_for_connection(
        connection,
        &normalized_topic,
        AUTHORING_TOPIC_SIGNAL_LIMIT,
    )?;
    let local_title_hit_count = local_title_hits.len();
    if let Some(exact_page) = &exact_page {
        local_title_hits
            .retain(|hit| hit.title != exact_page.title || hit.namespace != exact_page.namespace);
    }
    local_title_hits.truncate(AUTHORING_TOPIC_SIGNAL_LIMIT);

    let mut backlinks = parsing::query_backlinks_for_connection(connection, &normalized_topic)?;
    let backlink_count = backlinks.len();
    backlinks.truncate(AUTHORING_TOPIC_SIGNAL_LIMIT);

    Ok(AuthoringTopicAssessment {
        title_exists_locally: exact_page.is_some(),
        should_create_new_article: exact_page.is_none(),
        exact_page,
        local_title_hit_count,
        local_title_hits,
        backlink_count,
        backlinks,
    })
}

pub(crate) fn query_local_search_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<LocalSearchHit>> {
    let normalized = parsing::normalize_spaces(&query.replace('_', " "));
    if normalized.is_empty() {
        return Ok(Vec::new());
    }
    if parsing::fts_table_exists(connection, "indexed_pages_fts")
        && let Ok(hits) =
            query_search_fts(connection, &normalized, candidate_limit(limit.max(1), 4))
        && !hits.is_empty()
    {
        return collapse_search_hits(connection, hits, limit);
    }
    query_search_like(connection, &normalized, candidate_limit(limit.max(1), 4))
        .and_then(|hits| collapse_search_hits(connection, hits, limit))
}
