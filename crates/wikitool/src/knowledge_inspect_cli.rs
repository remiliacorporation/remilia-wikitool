use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use wikitool_core::filesystem::{ScanOptions, scan_stats};
use wikitool_core::knowledge::content_index::load_stored_index_stats;
use wikitool_core::knowledge::inspect::{query_backlinks, query_empty_categories, query_orphans};
use wikitool_core::knowledge::retrieval::{
    LocalChunkAcrossRetrieval, LocalChunkRetrieval, retrieve_local_context_chunks_across_pages,
    retrieve_local_context_chunks_with_options,
};
use wikitool_core::knowledge::templates::{
    ActiveTemplateCatalogLookup, TemplateParameterUsage, TemplateReferenceLookup,
    TemplateUsageSummary, query_active_template_catalog, query_template_reference,
};

use crate::cli_support::{
    normalize_path, normalize_title_query, print_scan_stats, print_stored_index_stats,
    resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct KnowledgeInspectArgs {
    #[command(subcommand)]
    command: KnowledgeInspectSubcommand,
}

#[derive(Debug, Subcommand)]
enum KnowledgeInspectSubcommand {
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
    /// Show indexed pages that link to a title
    Backlinks { title: String },
    /// Inspect active template usage and implementation references
    Templates {
        #[arg(value_name = "TEMPLATE", help = "Optional specific template title")]
        template: Option<String>,
        #[arg(
            long,
            default_value_t = 40,
            value_name = "N",
            help = "Maximum templates to return in catalog mode"
        )]
        limit: usize,
        #[arg(long, help = "Return the full active template catalog")]
        all: bool,
        #[arg(
            long,
            default_value = "text",
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: String,
    },
    /// Show indexed pages with no backlinks
    Orphans,
    #[command(name = "empty-categories")]
    /// Show categories with no indexed members
    EmptyCategories,
}

pub(crate) fn run_knowledge_inspect(
    runtime: &RuntimeOptions,
    args: KnowledgeInspectArgs,
) -> Result<()> {
    match args.command {
        KnowledgeInspectSubcommand::Stats => run_inspect_stats(runtime),
        KnowledgeInspectSubcommand::Chunks {
            title,
            query,
            across_pages,
            limit,
            token_budget,
            max_pages,
            format,
            diversify,
            no_diversify,
        } => run_inspect_chunks(
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
        KnowledgeInspectSubcommand::Backlinks { title } => run_inspect_backlinks(runtime, &title),
        KnowledgeInspectSubcommand::Templates {
            template,
            limit,
            all,
            format,
        } => run_inspect_templates(runtime, template.as_deref(), limit, all, &format),
        KnowledgeInspectSubcommand::Orphans => run_inspect_orphans(runtime),
        KnowledgeInspectSubcommand::EmptyCategories => run_inspect_empty_categories(runtime),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_inspect_chunks(
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
        bail!("knowledge inspect chunks requires --limit >= 1");
    }
    if token_budget == 0 {
        bail!("knowledge inspect chunks requires --token-budget >= 1");
    }
    if max_pages == 0 {
        bail!("knowledge inspect chunks requires --max-pages >= 1");
    }
    if diversify && no_diversify {
        bail!("cannot use --diversify and --no-diversify together");
    }

    let format = normalize_format(format)?;
    let use_diversify = !no_diversify;
    let paths = resolve_runtime_paths(runtime)?;

    if across_pages {
        if title.is_some() {
            bail!("omit TITLE when using --across-pages");
        }
        let query = query.unwrap_or_default().trim();
        if query.is_empty() {
            bail!("knowledge inspect chunks --across-pages requires --query");
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
            return Ok(());
        }

        println!("knowledge inspect chunks");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("target: <across-pages>");
        println!("query: {query}");
        println!("limit: {limit}");
        println!("token_budget: {token_budget}");
        println!("max_pages: {max_pages}");
        println!("diversify: {use_diversify}");
        match retrieval {
            LocalChunkAcrossRetrieval::IndexMissing => {
                println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
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
    } else {
        let title = title.unwrap_or_default().trim();
        if title.is_empty() {
            bail!(
                "knowledge inspect chunks requires a non-empty TITLE unless --across-pages is set"
            );
        }
        let retrieval = retrieve_local_context_chunks_with_options(
            &paths,
            title,
            query.map(str::trim).filter(|value| !value.is_empty()),
            limit,
            token_budget,
            use_diversify,
        )?;
        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&retrieval)?);
            return Ok(());
        }

        println!("knowledge inspect chunks");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("target: {}", normalize_title_query(title));
        println!("query: {}", query.unwrap_or("<none>").trim());
        println!("limit: {limit}");
        println!("token_budget: {token_budget}");
        match retrieval {
            LocalChunkRetrieval::IndexMissing => {
                println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
            }
            LocalChunkRetrieval::TitleMissing { title } => {
                bail!("page not found in local index: {title}");
            }
            LocalChunkRetrieval::Found(report) => {
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

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_inspect_backlinks(runtime: &RuntimeOptions, title: &str) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let normalized = normalize_title_query(title);
    if normalized.is_empty() {
        bail!("knowledge inspect backlinks requires a non-empty TITLE");
    }

    println!("knowledge inspect backlinks");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("title: {normalized}");
    match query_backlinks(&paths, &normalized)? {
        Some(backlinks) => {
            println!("backlinks.count: {}", backlinks.len());
            if backlinks.is_empty() {
                println!("backlinks: <none>");
            } else {
                for link in backlinks {
                    println!("backlink: {link}");
                }
            }
        }
        None => {
            println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_inspect_templates(
    runtime: &RuntimeOptions,
    template: Option<&str>,
    limit: usize,
    all: bool,
    format: &str,
) -> Result<()> {
    if limit == 0 {
        bail!("knowledge inspect templates requires --limit >= 1");
    }
    if template.is_some() && all {
        bail!(
            "cannot use `knowledge inspect templates TEMPLATE --all`; omit TEMPLATE in catalog mode"
        );
    }
    let format = normalize_format(format)?;
    let paths = resolve_runtime_paths(runtime)?;

    if let Some(template_title) = template {
        let lookup = query_template_reference(&paths, template_title)?;
        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&lookup)?);
            return Ok(());
        }

        println!("knowledge inspect templates");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("template: {}", normalize_title_query(template_title));
        match lookup {
            TemplateReferenceLookup::IndexMissing => {
                println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
            }
            TemplateReferenceLookup::TemplateMissing { template_title } => {
                println!("template.reference: <missing>");
                println!("template.title: {template_title}");
            }
            TemplateReferenceLookup::Found(reference) => {
                let reference = *reference;
                print_template_summary("template", &reference.template);
                println!(
                    "template.implementation_pages.count: {}",
                    reference.implementation_pages.len()
                );
                for page in &reference.implementation_pages {
                    println!(
                        "template.implementation_page: role={} page={} summary={}",
                        page.role,
                        page.page_title,
                        if page.summary_text.is_empty() {
                            "<none>"
                        } else {
                            &page.summary_text
                        }
                    );
                }
                println!(
                    "template.implementation_chunks.count: {}",
                    reference.implementation_chunks.len()
                );
            }
        }
    } else {
        let lookup = query_active_template_catalog(&paths, if all { limit.max(1) } else { limit })?;
        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&lookup)?);
            return Ok(());
        }

        println!("knowledge inspect templates");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("limit: {limit}");
        println!("all: {}", if all { "yes" } else { "no" });
        match lookup {
            ActiveTemplateCatalogLookup::IndexMissing => {
                println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
            }
            ActiveTemplateCatalogLookup::Found(catalog) => {
                println!(
                    "templates.active_template_count: {}",
                    catalog.active_template_count
                );
                println!("templates.count: {}", catalog.templates.len());
                for template in &catalog.templates {
                    print_template_summary("template", template);
                }
            }
        }
    }

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_inspect_orphans(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("knowledge inspect orphans");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("mode: report-only");
    match query_orphans(&paths)? {
        Some(orphans) => {
            println!("orphans.count: {}", orphans.len());
            if orphans.is_empty() {
                println!("orphans: <none>");
            } else {
                for title in orphans {
                    println!("orphan.title: {title}");
                }
            }
        }
        None => {
            println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_inspect_empty_categories(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("knowledge inspect empty-categories");
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
            println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_inspect_stats(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let scan = scan_stats(&paths, &ScanOptions::default())?;
    let stored = load_stored_index_stats(&paths)?;

    println!("knowledge inspect stats");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "wiki_content_dir: {}",
        normalize_path(&paths.wiki_content_dir)
    );
    println!("templates_dir: {}", normalize_path(&paths.templates_dir));
    print_scan_stats("scan", &scan);
    match stored {
        Some(stored) => print_stored_index_stats("content_index", &stored),
        None => println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)"),
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn print_template_summary(label: &str, template: &TemplateUsageSummary) {
    println!(
        "{label}: {} (usage={} pages={} aliases={} keys={} implementations={} preview={})",
        template.template_title,
        template.usage_count,
        template.distinct_page_count,
        if template.aliases.is_empty() {
            "<none>".to_string()
        } else {
            template.aliases.join(", ")
        },
        format_parameter_stats(&template.parameter_stats),
        if template.implementation_titles.is_empty() {
            "<none>".to_string()
        } else {
            template.implementation_titles.join(", ")
        },
        template
            .implementation_preview
            .as_deref()
            .unwrap_or("<none>")
    );
    if !template.example_pages.is_empty() {
        println!(
            "{label}.example_pages: {}",
            template.example_pages.join(", ")
        );
    }
    for example in &template.example_invocations {
        println!(
            "{label}.example: template={} source={} keys={} tokens={} text={}",
            template.template_title,
            example.source_title,
            if example.parameter_keys.is_empty() {
                "<none>".to_string()
            } else {
                example.parameter_keys.join(", ")
            },
            example.token_estimate,
            example.invocation_text
        );
    }
}

fn format_parameter_stats(stats: &[TemplateParameterUsage]) -> String {
    if stats.is_empty() {
        return "<none>".to_string();
    }
    stats
        .iter()
        .map(|stat| {
            if stat.example_values.is_empty() {
                format!("{}:{}", stat.key, stat.usage_count)
            } else {
                format!(
                    "{}:{}[{}]",
                    stat.key,
                    stat.usage_count,
                    stat.example_values.join(" | ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn normalize_format(value: &str) -> Result<String> {
    let format = value.trim().to_ascii_lowercase();
    if format != "text" && format != "json" {
        bail!("unsupported format: {} (expected text|json)", value);
    }
    Ok(format)
}
