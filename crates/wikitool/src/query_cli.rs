use anyhow::{Result, bail};
use clap::Args;
use wikitool_core::index::{
    LocalContextBundle, LocalSearchHit, build_local_context, query_search_local,
};
use wikitool_core::sync::{
    ExternalSearchHit, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE,
    search_external_wiki_with_config,
};

use crate::cli_support::{
    normalize_path, normalize_title_query, print_string_list, resolve_runtime_paths,
    resolve_runtime_with_config,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct ContextArgs {
    title: String,
}

#[derive(Debug, Args)]
pub(crate) struct SearchArgs {
    query: String,
}

#[derive(Debug, Args)]
pub(crate) struct SearchExternalArgs {
    query: String,
}

pub(crate) fn run_context(runtime: &RuntimeOptions, args: ContextArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let title = normalize_title_query(&args.title);
    if title.is_empty() {
        bail!("context requires a non-empty title");
    }

    println!("context");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("title: {title}");
    match build_local_context(&paths, &title)? {
        Some(bundle) => {
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

    println!("search");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("query: {query}");
    match query_search_local(&paths, &query, 20)? {
        Some(results) => {
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

pub(crate) fn run_search_external(
    runtime: &RuntimeOptions,
    args: SearchExternalArgs,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let query = normalize_title_query(&args.query);
    if query.is_empty() {
        bail!("search-external requires a non-empty query");
    }

    let namespaces = [NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
    let hits = search_external_wiki_with_config(&query, &namespaces, 20, &config)?;

    println!("search-external");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("query: {query}");
    print_external_search_hits("search_external", &hits);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn print_search_hits(prefix: &str, hits: &[LocalSearchHit]) {
    println!("{prefix}.count: {}", hits.len());
    if hits.is_empty() {
        println!("{prefix}.hits: <none>");
        return;
    }
    for hit in hits {
        println!(
            "{prefix}.hit: {} (namespace={}, redirect={})",
            hit.title,
            hit.namespace,
            if hit.is_redirect { "yes" } else { "no" }
        );
    }
}

fn print_external_search_hits(prefix: &str, hits: &[ExternalSearchHit]) {
    println!("{prefix}.count: {}", hits.len());
    if hits.is_empty() {
        println!("{prefix}.hits: <none>");
        return;
    }
    for hit in hits {
        println!(
            "{prefix}.hit: {} (namespace={}, page_id={})",
            hit.title, hit.namespace, hit.page_id
        );
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
