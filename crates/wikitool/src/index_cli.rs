use std::fs;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use wikitool_core::filesystem::{ScanOptions, scan_files, scan_stats};
use wikitool_core::index::{
    LocalChunkAcrossRetrieval, LocalChunkRetrieval, LocalContextBundle, load_stored_index_stats,
    query_backlinks, query_empty_categories, query_orphans, rebuild_index,
    retrieve_local_context_chunks_across_pages, retrieve_local_context_chunks_with_options,
};
use wikitool_core::runtime::{ResolvedPaths, ensure_runtime_ready_for_sync, inspect_runtime};

use crate::cli_support::{
    collapse_whitespace, normalize_path, normalize_title_query, print_migration_status,
    print_scan_stats, print_stored_index_stats, resolve_runtime_paths,
};
use crate::{MIGRATIONS_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct IndexArgs {
    #[command(subcommand)]
    command: IndexSubcommand,
}

#[derive(Debug, Subcommand)]
enum IndexSubcommand {
    /// Rebuild the local search index from wiki_content and templates
    Rebuild,
    /// Show index statistics
    Stats,
    /// Retrieve token-budgeted content chunks from indexed pages
    Chunks {
        title: Option<String>,
        #[arg(
            long,
            value_name = "QUERY",
            help = "Optional relevance query applied to chunk retrieval"
        )]
        query: Option<String>,
        #[arg(
            long,
            help = "Retrieve chunks across indexed pages (query required, omit TITLE)"
        )]
        across_pages: bool,
        #[arg(
            long,
            default_value_t = 8,
            value_name = "N",
            help = "Maximum number of chunks to return"
        )]
        limit: usize,
        #[arg(
            long,
            default_value_t = 720,
            value_name = "TOKENS",
            help = "Token budget across returned chunks"
        )]
        token_budget: usize,
        #[arg(
            long,
            default_value_t = 12,
            value_name = "N",
            help = "Maximum distinct source pages in across-pages mode"
        )]
        max_pages: usize,
        #[arg(
            long,
            default_value = "text",
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: String,
        #[arg(long, help = "Enable lexical de-duplication and diversification")]
        diversify: bool,
        #[arg(long, help = "Disable lexical de-duplication and diversification")]
        no_diversify: bool,
    },
    Backlinks {
        title: String,
    },
    Orphans,
    #[command(name = "prune-categories")]
    PruneCategories,
}

pub(crate) fn run_index(runtime: &RuntimeOptions, args: IndexArgs) -> Result<()> {
    match args.command {
        IndexSubcommand::Rebuild => run_index_rebuild(runtime),
        IndexSubcommand::Stats => run_index_stats(runtime),
        IndexSubcommand::Chunks {
            title,
            query,
            across_pages,
            limit,
            token_budget,
            max_pages,
            format,
            diversify,
            no_diversify,
        } => run_index_chunks(
            runtime,
            title.as_deref(),
            query.as_deref(),
            across_pages,
            limit,
            token_budget,
            max_pages,
            &format,
            diversify,
            no_diversify,
        ),
        IndexSubcommand::Backlinks { title } => run_index_backlinks(runtime, &title),
        IndexSubcommand::Orphans => run_index_orphans(runtime),
        IndexSubcommand::PruneCategories => run_index_prune_categories(runtime),
    }
}

pub(crate) fn build_context_from_scan(
    paths: &ResolvedPaths,
    title: &str,
) -> Result<Option<LocalContextBundle>> {
    let normalized = normalize_title_query(title);
    let files = scan_files(paths, &ScanOptions::default())?;
    let file = match files
        .into_iter()
        .find(|item| item.title.eq_ignore_ascii_case(&normalized))
    {
        Some(file) => file,
        None => return Ok(None),
    };

    let mut absolute = paths.project_root.clone();
    for segment in file.relative_path.split('/') {
        if !segment.is_empty() {
            absolute.push(segment);
        }
    }
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read {}", normalize_path(&absolute)))?;
    let content_preview = collapse_whitespace(&content)
        .chars()
        .take(280)
        .collect::<String>();

    Ok(Some(LocalContextBundle {
        title: file.title,
        namespace: file.namespace,
        is_redirect: file.is_redirect,
        redirect_target: file.redirect_target,
        relative_path: file.relative_path,
        bytes: file.bytes,
        word_count: content
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .count(),
        content_preview: if content_preview.is_empty() {
            String::new()
        } else {
            format!("{content_preview}...")
        },
        sections: Vec::new(),
        context_chunks: Vec::new(),
        context_tokens_estimate: 0,
        outgoing_links: Vec::new(),
        backlinks: Vec::new(),
        categories: Vec::new(),
        templates: Vec::new(),
        modules: Vec::new(),
        template_invocations: Vec::new(),
    }))
}

fn run_index_rebuild(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    let report = rebuild_index(&paths, &ScanOptions::default())?;

    println!("index rebuild");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("db_path: {}", normalize_path(&paths.db_path));
    println!("inserted_rows: {}", report.inserted_rows);
    println!("inserted_links: {}", report.inserted_links);
    print_scan_stats("scan", &report.scan);
    print_migration_status(&paths);
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_index_chunks(
    runtime: &RuntimeOptions,
    title: Option<&str>,
    query: Option<&str>,
    across_pages: bool,
    limit: usize,
    token_budget: usize,
    max_pages: usize,
    format: &str,
    diversify: bool,
    no_diversify: bool,
) -> Result<()> {
    if limit == 0 {
        bail!("index chunks requires --limit >= 1");
    }
    if token_budget == 0 {
        bail!("index chunks requires --token-budget >= 1");
    }
    if max_pages == 0 {
        bail!("index chunks requires --max-pages >= 1");
    }

    let format = format.to_ascii_lowercase();
    if format != "text" && format != "json" {
        bail!("unsupported format: {} (expected text|json)", format);
    }
    if diversify && no_diversify {
        bail!("cannot use --diversify and --no-diversify together");
    }
    let use_diversify = !no_diversify;

    let paths = resolve_runtime_paths(runtime)?;

    if across_pages {
        if title.is_some() {
            bail!("omit TITLE when using --across-pages");
        }
        let query = query.unwrap_or_default().trim();
        if query.is_empty() {
            bail!("index chunks --across-pages requires --query");
        }

        let retrieval = retrieve_local_context_chunks_across_pages(
            &paths,
            query,
            limit,
            token_budget,
            max_pages,
            use_diversify,
        )?;
        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&retrieval)?);
        } else {
            println!("index chunks");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("target: <across-pages>");
            println!("query: {query}");
            println!("limit: {limit}");
            println!("token_budget: {token_budget}");
            println!("max_pages: {max_pages}");
            println!("diversify: {use_diversify}");
            match retrieval {
                LocalChunkAcrossRetrieval::IndexMissing => {
                    println!("index.storage: <not built> (run `wikitool index rebuild`)");
                }
                LocalChunkAcrossRetrieval::QueryMissing => {
                    bail!("query is required for across-pages chunk retrieval");
                }
                LocalChunkAcrossRetrieval::Found(report) => {
                    println!("chunks.retrieval_mode: {}", report.retrieval_mode);
                    println!("chunks.count: {}", report.chunks.len());
                    println!("chunks.source_page_count: {}", report.source_page_count);
                    println!(
                        "chunks.tokens_estimate_total: {}",
                        report.token_estimate_total
                    );
                    for chunk in &report.chunks {
                        println!(
                            "chunk: source={} section={} tokens={} text={}",
                            chunk.source_title,
                            chunk.section_heading.as_deref().unwrap_or("<lead>"),
                            chunk.token_estimate,
                            chunk.chunk_text
                        );
                    }
                }
            }
        }
    } else {
        let title = title.unwrap_or_default().trim();
        if title.is_empty() {
            bail!("index chunks requires a non-empty TITLE unless --across-pages is set");
        }

        let retrieval = retrieve_local_context_chunks_with_options(
            &paths,
            title,
            query,
            limit,
            token_budget,
            use_diversify,
        )?;
        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&retrieval)?);
        } else {
            println!("index chunks");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("target: {title}");
            println!("query: {}", query.unwrap_or("<none>"));
            println!("limit: {limit}");
            println!("token_budget: {token_budget}");
            println!("diversify: {use_diversify}");
            match retrieval {
                LocalChunkRetrieval::IndexMissing => {
                    println!("index.storage: <not built> (run `wikitool index rebuild`)");
                }
                LocalChunkRetrieval::TitleMissing { title } => {
                    bail!("page not found in local index: {title}");
                }
                LocalChunkRetrieval::Found(report) => {
                    println!("chunks.title: {}", report.title);
                    println!("chunks.namespace: {}", report.namespace);
                    println!("chunks.relative_path: {}", report.relative_path);
                    println!("chunks.retrieval_mode: {}", report.retrieval_mode);
                    println!("chunks.count: {}", report.chunks.len());
                    println!(
                        "chunks.tokens_estimate_total: {}",
                        report.token_estimate_total
                    );
                    for chunk in &report.chunks {
                        println!(
                            "chunk: section={} tokens={} text={}",
                            chunk.section_heading.as_deref().unwrap_or("<lead>"),
                            chunk.token_estimate,
                            chunk.chunk_text
                        );
                    }
                }
            }
        }
    }
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_index_backlinks(runtime: &RuntimeOptions, title: &str) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let normalized_title = title.trim();

    println!("index backlinks");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("target: {normalized_title}");
    if normalized_title.is_empty() {
        bail!("index backlinks requires a non-empty title");
    }

    match query_backlinks(&paths, normalized_title)? {
        Some(backlinks) => {
            println!("backlinks.count: {}", backlinks.len());
            if backlinks.is_empty() {
                println!("backlinks: <none>");
            } else {
                for source in backlinks {
                    println!("backlinks.source: {source}");
                }
            }
        }
        None => {
            println!("index.storage: <not built> (run `wikitool index rebuild`)");
        }
    }
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_index_orphans(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("index orphans");
    println!("project_root: {}", normalize_path(&paths.project_root));
    match query_orphans(&paths)? {
        Some(orphans) => {
            println!("orphans.count: {}", orphans.len());
            if orphans.is_empty() {
                println!("orphans: <none>");
            } else {
                for title in orphans {
                    println!("orphans.title: {title}");
                }
            }
        }
        None => {
            println!("index.storage: <not built> (run `wikitool index rebuild`)");
        }
    }
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_index_prune_categories(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("index prune-categories");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("mode: report-only");
    match query_empty_categories(&paths)? {
        Some(categories) => {
            println!("empty_categories.count: {}", categories.len());
            if categories.is_empty() {
                println!("empty_categories: <none>");
            } else {
                for title in categories {
                    println!("empty_categories.title: {title}");
                }
            }
        }
        None => {
            println!("index.storage: <not built> (run `wikitool index rebuild`)");
        }
    }
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_index_stats(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let scan = scan_stats(&paths, &ScanOptions::default())?;
    let stored = load_stored_index_stats(&paths)?;

    println!("index stats");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "wiki_content_dir: {}",
        normalize_path(&paths.wiki_content_dir)
    );
    println!("templates_dir: {}", normalize_path(&paths.templates_dir));
    print_scan_stats("scan", &scan);
    match stored {
        Some(stored) => print_stored_index_stats("index", &stored),
        None => println!("index.storage: <not built> (run `wikitool index rebuild`)"),
    }
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}
