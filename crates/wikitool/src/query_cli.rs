use anyhow::{Result, bail};
use clap::ValueEnum;
use wikitool_core::config::WikiConfig;
use wikitool_core::sync::{
    ExternalSearchReport, MediaWikiSearchWhat, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE,
    NS_TEMPLATE, search_external_wiki_report_with_config,
};

use crate::cli_support::normalize_title_query;

const REMOTE_WIKI_SEARCH_NAMESPACES: [i32; 5] =
    [NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];

#[derive(Debug, Clone, Copy)]
pub(crate) struct RemoteWikiSearchRequest<'a> {
    pub(crate) command_name: &'a str,
    pub(crate) query: &'a str,
    pub(crate) limit: usize,
    pub(crate) what: RemoteSearchScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum RemoteSearchScope {
    Text,
    Title,
    #[value(name = "nearmatch")]
    Nearmatch,
}

impl RemoteSearchScope {
    fn as_search_what(self) -> MediaWikiSearchWhat {
        match self {
            Self::Text => MediaWikiSearchWhat::Text,
            Self::Title => MediaWikiSearchWhat::Title,
            Self::Nearmatch => MediaWikiSearchWhat::NearMatch,
        }
    }
}

impl std::fmt::Display for RemoteSearchScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Text => "text",
            Self::Title => "title",
            Self::Nearmatch => "nearmatch",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedRemoteWikiSearchRequest {
    query: String,
    limit: usize,
    what: MediaWikiSearchWhat,
}

pub(crate) fn remote_wiki_search_report(
    config: &WikiConfig,
    request: RemoteWikiSearchRequest<'_>,
) -> Result<ExternalSearchReport> {
    let parsed = parse_remote_wiki_search_request(request)?;
    search_external_wiki_report_with_config(
        &parsed.query,
        &REMOTE_WIKI_SEARCH_NAMESPACES,
        parsed.limit,
        parsed.what,
        config,
    )
}

fn parse_remote_wiki_search_request(
    request: RemoteWikiSearchRequest<'_>,
) -> Result<ParsedRemoteWikiSearchRequest> {
    if request.limit == 0 {
        bail!("{} requires --limit >= 1", request.command_name);
    }
    let query = normalize_title_query(request.query);
    if query.is_empty() {
        bail!("{} requires a non-empty query", request.command_name);
    }

    Ok(ParsedRemoteWikiSearchRequest {
        query,
        limit: request.limit,
        what: request.what.as_search_what(),
    })
}

pub(crate) fn print_external_search_report(prefix: &str, report: &ExternalSearchReport) {
    println!("{prefix}.count: {}", report.hits.len());
    println!("{prefix}.what: {}", report.what.as_api_param());
    println!(
        "{prefix}.namespaces: {}",
        if report.namespaces.is_empty() {
            "<all>".to_string()
        } else {
            report
                .namespaces
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        "{prefix}.total_hits: {}",
        report
            .total_hits
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );
    println!(
        "{prefix}.suggestion: {}",
        report.suggestion.as_deref().unwrap_or("<none>")
    );
    println!(
        "{prefix}.rewritten_query: {}",
        report.rewritten_query.as_deref().unwrap_or("<none>")
    );
    if report.hits.is_empty() {
        println!("{prefix}.hits: <none>");
        return;
    }
    for hit in &report.hits {
        println!(
            "{prefix}.hit: {} (namespace={}, page_id={})",
            hit.title, hit.namespace, hit.page_id
        );
        if let Some(value) = hit.byte_size {
            println!("{prefix}.hit.byte_size: {value}");
        }
        println!(
            "{prefix}.hit.word_count: {}",
            hit.word_count
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<none>".to_string())
        );
        println!(
            "{prefix}.hit.timestamp: {}",
            hit.timestamp.as_deref().unwrap_or("<none>")
        );
        println!(
            "{prefix}.hit.snippet: {}",
            if hit.snippet.trim().is_empty() {
                "<none>"
            } else {
                &hit.snippet
            }
        );
        if let Some(value) = hit.title_snippet.as_deref() {
            println!("{prefix}.hit.title_snippet: {value}");
        }
        if let Some(value) = hit.redirect_title.as_deref() {
            println!("{prefix}.hit.redirect_title: {value}");
        }
        if let Some(value) = hit.redirect_snippet.as_deref() {
            println!("{prefix}.hit.redirect_snippet: {value}");
        }
        if let Some(value) = hit.section_title.as_deref() {
            println!("{prefix}.hit.section_title: {value}");
        }
        if let Some(value) = hit.section_snippet.as_deref() {
            println!("{prefix}.hit.section_snippet: {value}");
        }
        if let Some(value) = hit.category_snippet.as_deref() {
            println!("{prefix}.hit.category_snippet: {value}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_remote_wiki_search_request_normalizes_query_and_scope() {
        let parsed = parse_remote_wiki_search_request(RemoteWikiSearchRequest {
            command_name: "research wiki-search",
            query: " Alpha_Beta ",
            limit: 3,
            what: RemoteSearchScope::Nearmatch,
        })
        .expect("parse remote wiki search request");

        assert_eq!(
            parsed,
            ParsedRemoteWikiSearchRequest {
                query: "Alpha Beta".to_string(),
                limit: 3,
                what: MediaWikiSearchWhat::NearMatch,
            }
        );
    }

    #[test]
    fn parse_remote_wiki_search_request_rejects_zero_limit() {
        let error = parse_remote_wiki_search_request(RemoteWikiSearchRequest {
            command_name: "research wiki-search",
            query: "Alpha",
            limit: 0,
            what: RemoteSearchScope::Text,
        })
        .expect_err("zero limit should fail");

        assert_eq!(
            error.to_string(),
            "research wiki-search requires --limit >= 1"
        );
    }

    #[test]
    fn parse_remote_wiki_search_request_rejects_blank_query() {
        let error = parse_remote_wiki_search_request(RemoteWikiSearchRequest {
            command_name: "research wiki-search",
            query: "   ",
            limit: 1,
            what: RemoteSearchScope::Text,
        })
        .expect_err("blank query should fail");

        assert_eq!(
            error.to_string(),
            "research wiki-search requires a non-empty query"
        );
    }
}
