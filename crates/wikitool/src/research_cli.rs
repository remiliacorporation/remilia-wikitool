use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::research::{
    ExternalFetchFormat, ExternalFetchOptions, ExternalFetchResult, fetch_page_by_url,
};
use wikitool_core::sync::{
    NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE, search_external_wiki_with_config,
};

use crate::cli_support::{normalize_path, normalize_title_query, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct ResearchArgs {
    #[command(subcommand)]
    command: ResearchSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ResearchSubcommand {
    #[command(about = "Search the remote wiki API for subject evidence")]
    Search(ResearchSearchArgs),
    #[command(about = "Fetch readable reference material from a URL")]
    Fetch(ResearchFetchArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ResearchSearchArgs {
    query: String,
    #[arg(long, default_value_t = 20, value_name = "N")]
    limit: usize,
    #[arg(long, default_value = "json", value_name = "FORMAT")]
    format: String,
}

#[derive(Debug, Args)]
pub(crate) struct ResearchFetchArgs {
    url: String,
    #[arg(long, default_value = "html", value_name = "FORMAT")]
    format: String,
    #[arg(long, default_value = "json", value_name = "FORMAT")]
    output: String,
}

#[derive(Debug, Serialize)]
struct ResearchSearchOutput {
    schema_version: String,
    query: String,
    count: usize,
    hits: Vec<wikitool_core::sync::ExternalSearchHit>,
}

#[derive(Debug, Serialize)]
struct ResearchFetchOutput {
    schema_version: String,
    result: ExternalFetchResult,
}

pub(crate) fn run_research(runtime: &RuntimeOptions, args: ResearchArgs) -> Result<()> {
    match args.command {
        ResearchSubcommand::Search(args) => run_research_search(runtime, args),
        ResearchSubcommand::Fetch(args) => run_research_fetch(runtime, args),
    }
}

fn run_research_search(runtime: &RuntimeOptions, args: ResearchSearchArgs) -> Result<()> {
    if args.limit == 0 {
        bail!("research search requires --limit >= 1");
    }
    let format = normalize_output(&args.format)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let query = normalize_title_query(&args.query);
    if query.is_empty() {
        bail!("research search requires a non-empty query");
    }

    let namespaces = [NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
    let hits = search_external_wiki_with_config(&query, &namespaces, args.limit, &config)?;

    if format == "json" {
        let output = ResearchSearchOutput {
            schema_version: "research_search_v1".to_string(),
            query,
            count: hits.len(),
            hits,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("research search");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("query: {query}");
    println!("count: {}", hits.len());
    for hit in hits {
        println!(
            "hit: {} (namespace={}, page_id={})",
            hit.title, hit.namespace, hit.page_id
        );
        println!("hit.snippet: {}", hit.snippet);
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_research_fetch(runtime: &RuntimeOptions, args: ResearchFetchArgs) -> Result<()> {
    let output_format = normalize_output(&args.output)?;
    let fetch_format = ExternalFetchFormat::parse(&args.format)?;
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    let result = fetch_page_by_url(
        &args.url,
        &ExternalFetchOptions {
            format: fetch_format,
            max_bytes: 1_000_000,
        },
    )?
    .ok_or_else(|| anyhow::anyhow!("page not found: {}", args.url))?;

    if output_format == "json" {
        let output = ResearchFetchOutput {
            schema_version: "research_document_v1".to_string(),
            result,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("research fetch");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("source_url: {}", args.url);
    println!("resolved_url: {}", result.url);
    println!("title: {}", result.title);
    println!("content_format: {}", result.content_format);
    println!("content_length: {}", result.content.len());
    println!("content:");
    println!("{}", result.content);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn normalize_output(value: &str) -> Result<&'static str> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "json" => Ok("json"),
        "text" => Ok("text"),
        _ => bail!("unsupported output format: {} (expected text|json)", value),
    }
}
