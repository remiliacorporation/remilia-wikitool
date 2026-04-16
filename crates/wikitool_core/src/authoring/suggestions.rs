use std::collections::{BTreeMap, BTreeSet};

use crate::content_store::parsing;
use crate::knowledge::{model::AuthoringSuggestion, prelude::*};

#[derive(Default)]
struct SuggestionAccumulator {
    evidence_titles: BTreeSet<String>,
}

pub(crate) fn query_suggested_main_links_for_sources(
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

pub(crate) fn query_suggested_categories_for_sources(
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
        if parsing::is_parser_placeholder_title(&target_title) {
            continue;
        }
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
