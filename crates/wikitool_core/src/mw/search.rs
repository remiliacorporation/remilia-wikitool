use anyhow::{Context, Result};
use serde::Deserialize;

use super::client::{ExternalSearchHit, MediaWikiClient};

#[derive(Debug, Deserialize, Default)]
struct QueryResponse {
    #[serde(default)]
    query: QueryPayload,
}

#[derive(Debug, Deserialize, Default)]
struct QueryPayload {
    #[serde(default)]
    search: Vec<SearchQueryItem>,
}

#[derive(Debug, Deserialize)]
struct SearchQueryItem {
    title: String,
    ns: i32,
    pageid: i64,
    wordcount: Option<i64>,
    snippet: Option<String>,
    timestamp: Option<String>,
}

pub(crate) fn search_pages(
    client: &mut MediaWikiClient,
    query: &str,
    namespaces: &[i32],
    limit: usize,
) -> Result<Vec<ExternalSearchHit>> {
    let namespace_filter = namespaces
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("|");
    let params = vec![
        ("action", "query".to_string()),
        ("list", "search".to_string()),
        ("srsearch", query.to_string()),
        ("srnamespace", namespace_filter),
        ("srlimit", limit.to_string()),
    ];

    let response = client.request_json_get(&params)?;
    let parsed: QueryResponse =
        serde_json::from_value(response).context("failed to decode search API response")?;

    Ok(parsed
        .query
        .search
        .into_iter()
        .map(|item| ExternalSearchHit {
            title: item.title,
            namespace: item.ns,
            page_id: item.pageid,
            word_count: item.wordcount.and_then(|value| u64::try_from(value).ok()),
            snippet: item.snippet.unwrap_or_default(),
            timestamp: item.timestamp,
        })
        .collect())
}
