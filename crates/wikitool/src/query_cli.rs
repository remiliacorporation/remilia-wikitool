use anyhow::{Result, bail};
use clap::{Args, ValueEnum};
use wikitool_core::config::WikiConfig;
use wikitool_core::knowledge::retrieval::{
    LocalContextBundle, LocalSearchHit, build_local_context, query_search_local,
};
use wikitool_core::sync::{
    ExternalSearchReport, MediaWikiSearchWhat, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE,
    NS_TEMPLATE, search_external_wiki_report_with_config,
};

use crate::cli_support::{
    OutputFormat, normalize_path, normalize_title_query, print_string_list, resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

const REMOTE_WIKI_SEARCH_NAMESPACES: [i32; 5] =
    [NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];

#[derive(Debug, Args)]
pub(crate) struct ContextArgs {
    title: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct SearchArgs {
    query: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

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
    #[value(name = "nearmatch", alias = "near-match")]
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

pub(crate) fn run_context(runtime: &RuntimeOptions, args: ContextArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let title = normalize_title_query(&args.title);
    if title.is_empty() {
        bail!("context requires a non-empty title");
    }

    match build_local_context(&paths, &title)? {
        Some(bundle) => {
            if args.format.is_json() {
                println!("{}", serde_json::to_string_pretty(&bundle)?);
                return Ok(());
            }
            println!("context");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("title: {title}");
            println!("context.backend: indexed");
            print_context_bundle("context", &bundle);
        }
        None => {
            bail!(
                "local knowledge index is not ready or page was not found: {title}\nRun `wikitool knowledge build` first."
            );
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

pub(crate) fn run_search(runtime: &RuntimeOptions, args: SearchArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let query = normalize_title_query(&args.query);
    if query.is_empty() {
        bail!("search requires a non-empty query");
    }

    match query_search_local(&paths, &query, 20)? {
        Some(results) => {
            if args.format.is_json() {
                println!("{}", serde_json::to_string_pretty(&results)?);
                return Ok(());
            }
            println!("search");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("query: {query}");
            println!("search.backend: indexed");
            print_search_hits("search", &results);
        }
        None => {
            bail!("local knowledge index is not ready.\nRun `wikitool knowledge build` first.");
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
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

fn print_search_hits(prefix: &str, hits: &[LocalSearchHit]) {
    println!("{prefix}.count: {}", hits.len());
    if hits.is_empty() {
        println!("{prefix}.hits: <none>");
        return;
    }
    for hit in hits {
        let translation_languages = if hit.translation_languages.is_empty() {
            "<none>".to_string()
        } else {
            hit.translation_languages.join(", ")
        };
        println!(
            "{prefix}.hit: {} (namespace={}, redirect={}, translation_languages={}, matched_translation_language={})",
            hit.title,
            hit.namespace,
            if hit.is_redirect { "yes" } else { "no" },
            translation_languages,
            hit.matched_translation_language
                .as_deref()
                .unwrap_or("<none>")
        );
    }
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

fn print_context_bundle(prefix: &str, bundle: &LocalContextBundle) {
    println!("{prefix}.title: {}", bundle.title);
    println!("{prefix}.namespace: {}", bundle.namespace);
    println!("{prefix}.relative_path: {}", bundle.relative_path);
    println!("{prefix}.bytes: {}", bundle.bytes);
    println!("{prefix}.word_count: {}", bundle.word_count);
    println!(
        "{prefix}.is_redirect: {}",
        if bundle.is_redirect { "yes" } else { "no" }
    );
    println!(
        "{prefix}.redirect_target: {}",
        bundle.redirect_target.as_deref().unwrap_or("<none>")
    );
    println!(
        "{prefix}.content_preview: {}",
        if bundle.content_preview.is_empty() {
            "<empty>"
        } else {
            &bundle.content_preview
        }
    );
    println!("{prefix}.sections.count: {}", bundle.sections.len());
    for section in &bundle.sections {
        println!(
            "{prefix}.section: level={} heading={}",
            section.level, section.heading
        );
    }
    println!(
        "{prefix}.section_summaries.count: {}",
        bundle.section_summaries.len()
    );
    for section in &bundle.section_summaries {
        println!(
            "{prefix}.section_summary: level={} heading={} tokens={} summary={}",
            section.section_level,
            section.section_heading.as_deref().unwrap_or("<lead>"),
            section.token_estimate,
            section.summary_text
        );
    }
    println!(
        "{prefix}.context_chunks.count: {}",
        bundle.context_chunks.len()
    );
    println!(
        "{prefix}.context_chunks.tokens_estimate_total: {}",
        bundle.context_tokens_estimate
    );
    for chunk in &bundle.context_chunks {
        println!(
            "{prefix}.context_chunk: section={} tokens={} text={}",
            chunk.section_heading.as_deref().unwrap_or("<lead>"),
            chunk.token_estimate,
            chunk.chunk_text
        );
    }
    print_string_list(&format!("{prefix}.outgoing_links"), &bundle.outgoing_links);
    print_string_list(&format!("{prefix}.backlinks"), &bundle.backlinks);
    print_string_list(&format!("{prefix}.categories"), &bundle.categories);
    print_string_list(&format!("{prefix}.templates"), &bundle.templates);
    print_string_list(&format!("{prefix}.modules"), &bundle.modules);
    println!(
        "{prefix}.template_invocations.count: {}",
        bundle.template_invocations.len()
    );
    for invocation in &bundle.template_invocations {
        println!(
            "{prefix}.template_invocation: title={} keys={}",
            invocation.template_title,
            if invocation.parameter_keys.is_empty() {
                "<none>".to_string()
            } else {
                invocation.parameter_keys.join(", ")
            }
        );
    }
    println!("{prefix}.references.count: {}", bundle.references.len());
    println!("{prefix}.media.count: {}", bundle.media.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_remote_wiki_search_request_normalizes_query_and_scope() {
        let parsed = parse_remote_wiki_search_request(RemoteWikiSearchRequest {
            command_name: "research search",
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
            command_name: "research search",
            query: "Alpha",
            limit: 0,
            what: RemoteSearchScope::Text,
        })
        .expect_err("zero limit should fail");

        assert_eq!(error.to_string(), "research search requires --limit >= 1");
    }

    #[test]
    fn parse_remote_wiki_search_request_rejects_blank_query() {
        let error = parse_remote_wiki_search_request(RemoteWikiSearchRequest {
            command_name: "research search",
            query: "   ",
            limit: 1,
            what: RemoteSearchScope::Text,
        })
        .expect_err("blank query should fail");

        assert_eq!(
            error.to_string(),
            "research search requires a non-empty query"
        );
    }
}
