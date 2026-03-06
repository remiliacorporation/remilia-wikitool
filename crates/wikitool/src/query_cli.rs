use anyhow::{Result, bail};
use clap::Args;
use wikitool_core::filesystem::{ScanOptions, scan_files};
use wikitool_core::index::{
    LocalContextBundle, LocalSearchHit, build_local_context, load_stored_index_stats,
    query_search_local,
};
use wikitool_core::sync::{
    ExternalSearchHit, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE,
    search_external_wiki_with_config,
};

use crate::cli_support::{
    normalize_path, normalize_title_query, print_string_list, resolve_runtime_paths,
    resolve_runtime_with_config,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions, index_cli};

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
    if let Some(bundle) = build_local_context(&paths, &title)? {
        println!("context.backend: indexed");
        print_context_bundle("context", &bundle);
    } else {
        let has_index = load_stored_index_stats(&paths)?.is_some();
        if let Some(bundle) = index_cli::build_context_from_scan(&paths, &title)? {
            println!("context.backend: fallback-filesystem");
            if !has_index {
                println!(
                    "index.storage: <not built> (run `wikitool index rebuild` for richer context)"
                );
            }
            print_context_bundle("context", &bundle);
        } else if has_index {
            bail!("page not found in local index: {title}");
        } else {
            bail!(
                "local index is not built and page was not found by filesystem scan: {title}\nRun `wikitool index rebuild` after `wikitool pull`."
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
            println!("search.backend: fallback-filesystem");
            println!("index.storage: <not built> (run `wikitool index rebuild` for faster search)");
            let mut results = scan_files(&paths, &ScanOptions::default())?
                .into_iter()
                .filter(|file| {
                    file.title
                        .to_ascii_lowercase()
                        .contains(&query.to_ascii_lowercase())
                })
                .map(|file| LocalSearchHit {
                    title: file.title,
                    namespace: file.namespace,
                    is_redirect: file.is_redirect,
                })
                .collect::<Vec<_>>();
            results.sort_by(|left, right| left.title.cmp(&right.title));
            results.truncate(20);
            print_search_hits("search", &results);
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
                invocation.parameter_keys.join(",")
            }
        );
    }
    println!("{prefix}.references.count: {}", bundle.references.len());
    for reference in &bundle.references {
        println!(
            "{prefix}.reference: section={} name={} group={} profile={} family={} template={} type={} origin={} quality={} title={} container={} author={} domain={} summary={} templates={} links={} identifiers={} flags={} tokens={}",
            reference.section_heading.as_deref().unwrap_or("<lead>"),
            reference.reference_name.as_deref().unwrap_or("<none>"),
            reference.reference_group.as_deref().unwrap_or("<none>"),
            reference.citation_profile,
            reference.citation_family,
            reference
                .primary_template_title
                .as_deref()
                .unwrap_or("<none>"),
            reference.source_type,
            reference.source_origin,
            reference.quality_score,
            if reference.reference_title.is_empty() {
                "<none>"
            } else {
                &reference.reference_title
            },
            if reference.source_container.is_empty() {
                "<none>"
            } else {
                &reference.source_container
            },
            if reference.source_author.is_empty() {
                "<none>"
            } else {
                &reference.source_author
            },
            if reference.source_domain.is_empty() {
                "<none>"
            } else {
                &reference.source_domain
            },
            reference.summary_text,
            if reference.template_titles.is_empty() {
                "<none>".to_string()
            } else {
                reference.template_titles.join(",")
            },
            if reference.link_titles.is_empty() {
                "<none>".to_string()
            } else {
                reference.link_titles.join(",")
            },
            if reference.identifier_keys.is_empty() {
                "<none>".to_string()
            } else {
                reference.identifier_keys.join(",")
            },
            if reference.quality_flags.is_empty() {
                "<none>".to_string()
            } else {
                reference.quality_flags.join(",")
            },
            reference.token_estimate
        );
    }
    println!("{prefix}.media.count: {}", bundle.media.len());
    for media in &bundle.media {
        println!(
            "{prefix}.media_usage: section={} file={} kind={} tokens={} caption={} options={}",
            media.section_heading.as_deref().unwrap_or("<lead>"),
            media.file_title,
            media.media_kind,
            media.token_estimate,
            if media.caption_text.is_empty() {
                "<none>"
            } else {
                &media.caption_text
            },
            if media.options.is_empty() {
                "<none>".to_string()
            } else {
                media.options.join(", ")
            }
        );
    }
}
