use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::{ExternalSearchHit, MediaWikiClient};

const SEARCH_PROPS: &str = concat!(
    "size|wordcount|timestamp|snippet|titlesnippet|",
    "redirecttitle|redirectsnippet|sectiontitle|sectionsnippet|categorysnippet"
);
const SEARCH_INFO: &str = "suggestion|rewrittenquery|totalhits";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MediaWikiSearchWhat {
    #[default]
    Text,
    Title,
    NearMatch,
}

impl MediaWikiSearchWhat {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "title" => Ok(Self::Title),
            "nearmatch" | "near-match" | "near_match" => Ok(Self::NearMatch),
            _ => bail!(
                "unsupported search scope: {} (expected text|title|nearmatch)",
                value
            ),
        }
    }

    pub fn as_api_param(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Title => "title",
            Self::NearMatch => "nearmatch",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaWikiSearchOptions {
    pub namespaces: Vec<i32>,
    pub limit: usize,
    pub what: MediaWikiSearchWhat,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalSearchReport {
    pub query: String,
    pub what: MediaWikiSearchWhat,
    pub namespaces: Vec<i32>,
    pub total_hits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rewritten_query: Option<String>,
    pub hits: Vec<ExternalSearchHit>,
}

#[derive(Debug, Deserialize, Default)]
struct QueryResponse {
    #[serde(default)]
    query: QueryPayload,
}

#[derive(Debug, Deserialize, Default)]
struct QueryPayload {
    #[serde(default)]
    searchinfo: SearchInfo,
    #[serde(default)]
    search: Vec<SearchQueryItem>,
}

#[derive(Debug, Deserialize, Default)]
struct SearchInfo {
    totalhits: Option<i64>,
    suggestion: Option<String>,
    rewrittenquery: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchQueryItem {
    title: String,
    ns: i32,
    pageid: i64,
    size: Option<i64>,
    wordcount: Option<i64>,
    snippet: Option<String>,
    titlesnippet: Option<String>,
    redirecttitle: Option<String>,
    redirectsnippet: Option<String>,
    sectiontitle: Option<String>,
    sectionsnippet: Option<String>,
    categorysnippet: Option<String>,
    timestamp: Option<String>,
}

pub fn search_pages_report(
    client: &mut MediaWikiClient,
    query: &str,
    options: &MediaWikiSearchOptions,
) -> Result<ExternalSearchReport> {
    let namespace_filter = options
        .namespaces
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("|");
    let params = vec![
        ("action", "query".to_string()),
        ("list", "search".to_string()),
        ("srsearch", query.to_string()),
        ("srnamespace", namespace_filter),
        ("srlimit", options.limit.to_string()),
        ("srwhat", options.what.as_api_param().to_string()),
        ("srprop", SEARCH_PROPS.to_string()),
        ("srinfo", SEARCH_INFO.to_string()),
    ];

    let response = client.request_json_get(&params)?;
    decode_search_response(query, options, response)
}

pub(crate) fn search_pages(
    client: &mut MediaWikiClient,
    query: &str,
    namespaces: &[i32],
    limit: usize,
) -> Result<Vec<ExternalSearchHit>> {
    Ok(search_pages_report(
        client,
        query,
        &MediaWikiSearchOptions {
            namespaces: namespaces.to_vec(),
            limit,
            what: MediaWikiSearchWhat::Text,
        },
    )?
    .hits)
}

fn decode_search_response(
    query: &str,
    options: &MediaWikiSearchOptions,
    response: Value,
) -> Result<ExternalSearchReport> {
    let parsed: QueryResponse =
        serde_json::from_value(response).context("failed to decode search API response")?;

    Ok(ExternalSearchReport {
        query: query.to_string(),
        what: options.what,
        namespaces: options.namespaces.clone(),
        total_hits: parse_u64(parsed.query.searchinfo.totalhits),
        suggestion: normalize_optional_string(parsed.query.searchinfo.suggestion),
        rewritten_query: normalize_optional_string(parsed.query.searchinfo.rewrittenquery),
        hits: parsed
            .query
            .search
            .into_iter()
            .map(|item| ExternalSearchHit {
                title: item.title,
                namespace: item.ns,
                page_id: item.pageid,
                word_count: parse_u64(item.wordcount),
                snippet: item.snippet.unwrap_or_default(),
                timestamp: item.timestamp,
                byte_size: parse_u64(item.size),
                title_snippet: normalize_optional_string(item.titlesnippet),
                redirect_title: normalize_optional_string(item.redirecttitle),
                redirect_snippet: normalize_optional_string(item.redirectsnippet),
                section_title: normalize_optional_string(item.sectiontitle),
                section_snippet: normalize_optional_string(item.sectionsnippet),
                category_snippet: normalize_optional_string(item.categorysnippet),
            })
            .collect(),
    })
}

fn parse_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{MediaWikiSearchOptions, MediaWikiSearchWhat, decode_search_response};

    #[test]
    fn parses_search_scope_variants() {
        assert_eq!(
            MediaWikiSearchWhat::parse("text").expect("text should parse"),
            MediaWikiSearchWhat::Text
        );
        assert_eq!(
            MediaWikiSearchWhat::parse("title").expect("title should parse"),
            MediaWikiSearchWhat::Title
        );
        assert_eq!(
            MediaWikiSearchWhat::parse("nearmatch").expect("nearmatch should parse"),
            MediaWikiSearchWhat::NearMatch
        );
        assert_eq!(
            MediaWikiSearchWhat::parse("near-match").expect("near-match should parse"),
            MediaWikiSearchWhat::NearMatch
        );
        assert!(MediaWikiSearchWhat::parse("body").is_err());
    }

    #[test]
    fn decodes_richer_search_metadata() {
        let options = MediaWikiSearchOptions {
            namespaces: vec![0, 14],
            limit: 10,
            what: MediaWikiSearchWhat::Title,
        };

        let report = decode_search_response(
            "milady",
            &options,
            json!({
                "query": {
                    "searchinfo": {
                        "totalhits": 42,
                        "suggestion": "Milady",
                        "rewrittenquery": "Milady Maker"
                    },
                    "search": [
                        {
                            "title": "Milady Maker",
                            "ns": 0,
                            "pageid": 123,
                            "size": 4096,
                            "wordcount": 512,
                            "snippet": "A <span>snippet</span>",
                            "titlesnippet": "<span>Milady</span> Maker",
                            "redirecttitle": "Milady",
                            "redirectsnippet": "Redirect <span>match</span>",
                            "sectiontitle": "History",
                            "sectionsnippet": "Section <span>match</span>",
                            "categorysnippet": "Category:<span>Milady</span>",
                            "timestamp": "2026-03-01T12:00:00Z"
                        }
                    ]
                }
            }),
        )
        .expect("search response should decode");

        assert_eq!(report.query, "milady");
        assert_eq!(report.what, MediaWikiSearchWhat::Title);
        assert_eq!(report.namespaces, vec![0, 14]);
        assert_eq!(report.total_hits, Some(42));
        assert_eq!(report.suggestion.as_deref(), Some("Milady"));
        assert_eq!(report.rewritten_query.as_deref(), Some("Milady Maker"));
        assert_eq!(report.hits.len(), 1);

        let hit = &report.hits[0];
        assert_eq!(hit.title, "Milady Maker");
        assert_eq!(hit.namespace, 0);
        assert_eq!(hit.page_id, 123);
        assert_eq!(hit.byte_size, Some(4096));
        assert_eq!(hit.word_count, Some(512));
        assert_eq!(hit.timestamp.as_deref(), Some("2026-03-01T12:00:00Z"));
        assert_eq!(
            hit.title_snippet.as_deref(),
            Some("<span>Milady</span> Maker")
        );
        assert_eq!(hit.redirect_title.as_deref(), Some("Milady"));
        assert_eq!(
            hit.redirect_snippet.as_deref(),
            Some("Redirect <span>match</span>")
        );
        assert_eq!(hit.section_title.as_deref(), Some("History"));
        assert_eq!(
            hit.section_snippet.as_deref(),
            Some("Section <span>match</span>")
        );
        assert_eq!(
            hit.category_snippet.as_deref(),
            Some("Category:<span>Milady</span>")
        );
        assert_eq!(hit.snippet, "A <span>snippet</span>");
    }
}
