use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use wikitool_core::filesystem::validate_scoped_path;
use wikitool_core::index::{
    AuthoringKnowledgePack, AuthoringKnowledgePackOptions, AuthoringSuggestion, MediaUsageSummary,
    ModuleUsageSummary, ReferenceUsageSummary, StubTemplateHint, TemplateParameterUsage,
    TemplateReference, TemplateUsageSummary, build_authoring_knowledge_pack,
};
use wikitool_core::runtime::ResolvedPaths;

use crate::cli_support::{
    collapse_whitespace, format_flag, normalize_option, normalize_path, prompt_yes_no,
    resolve_default_true_flag, resolve_runtime_paths,
};
use crate::dev_cli::{self, InstallGitHooksArgs};
use crate::docs_cli::{self, DocsGenerateReferenceArgs};
use crate::quality_cli;
use crate::sync_cli::{self, InitArgs, PullArgs, StatusArgs};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct WorkflowArgs {
    #[command(subcommand)]
    command: WorkflowSubcommand,
}

#[derive(Debug, Subcommand)]
enum WorkflowSubcommand {
    Bootstrap(WorkflowBootstrapArgs),
    #[command(name = "full-refresh")]
    FullRefresh(WorkflowFullRefreshArgs),
    Ask(WorkflowAskArgs),
    /// Generate a token-budgeted knowledge pack for article authoring
    #[command(name = "authoring-pack")]
    AuthoringPack(WorkflowAuthoringPackArgs),
}

#[derive(Debug, Args)]
struct WorkflowBootstrapArgs {
    #[arg(long, help = "Create templates/ during initialization (default: true)")]
    templates: bool,
    #[arg(long, help = "Do not create templates/ during initialization")]
    no_templates: bool,
    #[arg(long, help = "Pull content after initialization (default: true)")]
    pull: bool,
    #[arg(long, help = "Skip content pull after initialization")]
    no_pull: bool,
    #[arg(long, help = "Skip docs reference generation")]
    skip_reference: bool,
    #[arg(long, help = "Skip commit-msg hook installation")]
    skip_git_hooks: bool,
}

#[derive(Debug, Args)]
struct WorkflowFullRefreshArgs {
    #[arg(long, help = "Assume yes; do not prompt for confirmation")]
    yes: bool,
    #[arg(long, help = "Create templates/ during initialization (default: true)")]
    templates: bool,
    #[arg(long, help = "Do not create templates/ during initialization")]
    no_templates: bool,
    #[arg(long, help = "Skip docs reference generation")]
    skip_reference: bool,
}

#[derive(Debug, Args)]
struct WorkflowAuthoringPackArgs {
    #[arg(
        value_name = "TOPIC",
        help = "Primary article topic/title for retrieval"
    )]
    topic: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional stub wikitext file used for link/template hint extraction"
    )]
    stub_path: Option<PathBuf>,
    #[arg(
        long,
        default_value_t = 18,
        value_name = "N",
        help = "Maximum related pages in the pack"
    )]
    related_limit: usize,
    #[arg(
        long,
        default_value_t = 10,
        value_name = "N",
        help = "Maximum retrieved context chunks"
    )]
    chunk_limit: usize,
    #[arg(
        long,
        default_value_t = 1200,
        value_name = "TOKENS",
        help = "Token budget across retrieved chunks"
    )]
    token_budget: usize,
    #[arg(
        long,
        default_value_t = 8,
        value_name = "N",
        help = "Maximum distinct source pages in chunk retrieval"
    )]
    max_pages: usize,
    #[arg(
        long,
        default_value_t = 18,
        value_name = "N",
        help = "Maximum internal link suggestions"
    )]
    link_limit: usize,
    #[arg(
        long,
        default_value_t = 8,
        value_name = "N",
        help = "Maximum category suggestions"
    )]
    category_limit: usize,
    #[arg(
        long,
        default_value_t = 16,
        value_name = "N",
        help = "Maximum template summaries"
    )]
    template_limit: usize,
    #[arg(
        long,
        default_value = "json",
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: String,
    #[arg(long, help = "Enable lexical chunk de-duplication and diversification")]
    diversify: bool,
    #[arg(
        long,
        help = "Disable lexical chunk de-duplication and diversification"
    )]
    no_diversify: bool,
}

#[derive(Debug, Args)]
struct WorkflowAskArgs {
    #[arg(value_name = "PROMPT", help = "Natural-language authoring request")]
    prompt: String,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional stub wikitext file used for link/template hint extraction"
    )]
    stub_path: Option<PathBuf>,
    #[arg(
        long,
        default_value = "json",
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: String,
}

pub(crate) fn run_workflow(runtime: &RuntimeOptions, args: WorkflowArgs) -> Result<()> {
    match args.command {
        WorkflowSubcommand::Bootstrap(options) => run_workflow_bootstrap(runtime, options),
        WorkflowSubcommand::FullRefresh(options) => run_workflow_full_refresh(runtime, options),
        WorkflowSubcommand::Ask(options) => run_workflow_ask(runtime, options),
        WorkflowSubcommand::AuthoringPack(options) => run_workflow_authoring_pack(runtime, options),
    }
}

fn run_workflow_bootstrap(runtime: &RuntimeOptions, args: WorkflowBootstrapArgs) -> Result<()> {
    let include_templates = resolve_default_true_flag(
        args.templates,
        args.no_templates,
        "workflow bootstrap templates",
    )?;
    let should_pull =
        resolve_default_true_flag(args.pull, args.no_pull, "workflow bootstrap pull")?;

    sync_cli::run_init(
        runtime,
        InitArgs {
            templates: include_templates,
            force: false,
            no_config: false,
            no_parser_config: false,
        },
    )?;

    let paths = resolve_runtime_paths(runtime)?;

    if !args.skip_reference {
        docs_cli::run_docs_generate_reference(DocsGenerateReferenceArgs {
            output: Some(paths.project_root.join("docs/wikitool/reference.md")),
        })?;
    }

    if !args.skip_git_hooks {
        dev_cli::run_dev_install_git_hooks(InstallGitHooksArgs {
            repo_root: Some(paths.project_root.clone()),
            source: None,
            allow_missing_git: true,
        })?;
    }

    if should_pull {
        sync_cli::run_pull(
            runtime,
            PullArgs {
                full: true,
                overwrite_local: false,
                category: None,
                templates: false,
                categories: false,
                all: true,
            },
        )?;
    } else {
        println!("workflow bootstrap: pull skipped (--no-pull)");
    }

    Ok(())
}

fn run_workflow_full_refresh(
    runtime: &RuntimeOptions,
    args: WorkflowFullRefreshArgs,
) -> Result<()> {
    let include_templates = resolve_default_true_flag(
        args.templates,
        args.no_templates,
        "workflow full-refresh templates",
    )?;
    if !args.yes
        && !prompt_yes_no(
            "This will reset .wikitool/data/wikitool.db and re-download content/templates. Continue? (y/N) ",
        )?
    {
        println!("Aborted.");
        return Ok(());
    }

    let paths = resolve_runtime_paths(runtime)?;
    if paths.db_path.exists() {
        fs::remove_file(&paths.db_path)
            .with_context(|| format!("failed to delete {}", normalize_path(&paths.db_path)))?;
        println!("Removed {}", normalize_path(&paths.db_path));
    }

    sync_cli::run_init(
        runtime,
        InitArgs {
            templates: include_templates,
            force: false,
            no_config: false,
            no_parser_config: false,
        },
    )?;

    if !args.skip_reference {
        docs_cli::run_docs_generate_reference(DocsGenerateReferenceArgs {
            output: Some(paths.project_root.join("docs/wikitool/reference.md")),
        })?;
    }

    sync_cli::run_pull(
        runtime,
        PullArgs {
            full: true,
            overwrite_local: false,
            category: None,
            templates: false,
            categories: false,
            all: true,
        },
    )?;
    quality_cli::run_validate(runtime)?;
    sync_cli::run_status(
        runtime,
        StatusArgs {
            modified: false,
            conflicts: false,
            templates: true,
        },
    )?;
    Ok(())
}

fn run_workflow_authoring_pack(
    runtime: &RuntimeOptions,
    args: WorkflowAuthoringPackArgs,
) -> Result<()> {
    if args.related_limit == 0 {
        bail!("workflow authoring-pack requires --related-limit >= 1");
    }
    if args.chunk_limit == 0 {
        bail!("workflow authoring-pack requires --chunk-limit >= 1");
    }
    if args.token_budget == 0 {
        bail!("workflow authoring-pack requires --token-budget >= 1");
    }
    if args.max_pages == 0 {
        bail!("workflow authoring-pack requires --max-pages >= 1");
    }
    if args.link_limit == 0 {
        bail!("workflow authoring-pack requires --link-limit >= 1");
    }
    if args.category_limit == 0 {
        bail!("workflow authoring-pack requires --category-limit >= 1");
    }
    if args.template_limit == 0 {
        bail!("workflow authoring-pack requires --template-limit >= 1");
    }
    if args.diversify && args.no_diversify {
        bail!("cannot use --diversify and --no-diversify together");
    }

    let format = args.format.trim().to_ascii_lowercase();
    if format != "text" && format != "json" {
        bail!("unsupported format: {} (expected text|json)", args.format);
    }
    let use_diversify = !args.no_diversify;

    let paths = resolve_runtime_paths(runtime)?;
    let topic = normalize_option(args.topic.as_deref())
        .or_else(|| derive_topic_from_stub_path(args.stub_path.as_deref()));
    let stub_content = load_workflow_stub_content(&paths, args.stub_path.as_deref())?;

    let options = AuthoringKnowledgePackOptions {
        related_page_limit: args.related_limit,
        chunk_limit: args.chunk_limit,
        token_budget: args.token_budget,
        max_pages: args.max_pages,
        link_limit: args.link_limit,
        category_limit: args.category_limit,
        template_limit: args.template_limit,
        diversify: use_diversify,
    };

    let pack = build_authoring_knowledge_pack(
        &paths,
        topic.as_deref(),
        stub_content.as_deref(),
        &options,
    )?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&pack)?);
        return Ok(());
    }

    println!("workflow authoring-pack");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "topic: {}",
        topic.as_deref().unwrap_or("<derived-from-stub>")
    );
    if let Some(path) = args.stub_path.as_deref() {
        println!("stub_path: {}", normalize_path(path));
    } else {
        println!("stub_path: <none>");
    }
    println!("related_limit: {}", args.related_limit);
    println!("chunk_limit: {}", args.chunk_limit);
    println!("token_budget: {}", args.token_budget);
    println!("max_pages: {}", args.max_pages);
    println!("diversify: {use_diversify}");

    match pack {
        AuthoringKnowledgePack::IndexMissing => {
            println!("index.storage: <not built> (run `wikitool index rebuild`)");
        }
        AuthoringKnowledgePack::QueryMissing => {
            bail!(
                "workflow authoring-pack requires a topic or a stub with at least one resolvable wikilink"
            );
        }
        AuthoringKnowledgePack::Found(boxed_report) => {
            let report = *boxed_report;
            println!("pack.query: {}", report.query);
            println!(
                "pack.query_terms: {}",
                if report.query_terms.is_empty() {
                    "<none>".to_string()
                } else {
                    report.query_terms.join(" | ")
                }
            );
            println!("pack.token_budget: {}", report.pack_token_budget);
            println!(
                "pack.token_estimate_total: {}",
                report.pack_token_estimate_total
            );
            println!(
                "inventory.pages.total: {}",
                report.inventory.indexed_pages_total
            );
            println!(
                "inventory.semantic_profiles.total: {}",
                report.inventory.semantic_profiles_total
            );
            println!("inventory.pages.main: {}", report.inventory.main_pages);
            println!(
                "inventory.pages.templates: {}",
                report.inventory.template_pages
            );
            println!(
                "inventory.links.total: {}",
                report.inventory.indexed_links_total
            );
            println!(
                "inventory.templates.invocation_rows: {}",
                report.inventory.template_invocation_rows
            );
            println!(
                "inventory.templates.distinct: {}",
                report.inventory.distinct_templates_invoked
            );
            println!(
                "inventory.modules.invocation_rows: {}",
                report.inventory.module_invocation_rows_total
            );
            println!(
                "inventory.modules.distinct: {}",
                report.inventory.distinct_modules_invoked
            );
            println!(
                "inventory.references.total: {}",
                report.inventory.reference_rows_total
            );
            println!(
                "inventory.references.authority_rows: {}",
                report.inventory.reference_authority_rows_total
            );
            println!(
                "inventory.references.identifier_rows: {}",
                report.inventory.reference_identifier_rows_total
            );
            println!(
                "inventory.references.distinct_profiles: {}",
                report.inventory.distinct_reference_profiles
            );
            println!(
                "inventory.media.total: {}",
                report.inventory.media_rows_total
            );
            println!(
                "inventory.media.distinct_files: {}",
                report.inventory.distinct_media_files
            );
            println!(
                "inventory.templates.implementation_rows: {}",
                report.inventory.template_implementation_rows_total
            );
            println!("related_pages.count: {}", report.related_pages.len());
            for page in &report.related_pages {
                println!(
                    "related_page: {} (namespace={} redirect={} source={} retrieval_weight={} summary={})",
                    page.title,
                    page.namespace,
                    format_flag(page.is_redirect),
                    page.source,
                    page.retrieval_weight,
                    if page.summary.is_empty() {
                        "<none>"
                    } else {
                        &page.summary
                    }
                );
            }
            println!("suggested_links.count: {}", report.suggested_links.len());
            for link in &report.suggested_links {
                print_authoring_suggestion("suggested_link", link);
            }
            println!(
                "suggested_categories.count: {}",
                report.suggested_categories.len()
            );
            for category in &report.suggested_categories {
                print_authoring_suggestion("suggested_category", category);
            }
            println!(
                "suggested_templates.count: {}",
                report.suggested_templates.len()
            );
            for template in &report.suggested_templates {
                print_template_summary("suggested_template", template);
            }
            println!(
                "suggested_references.count: {}",
                report.suggested_references.len()
            );
            for reference in &report.suggested_references {
                print_reference_summary("suggested_reference", reference);
            }
            println!("suggested_media.count: {}", report.suggested_media.len());
            for media in &report.suggested_media {
                print_media_summary("suggested_media", media);
            }
            println!(
                "template_baseline.count: {}",
                report.template_baseline.len()
            );
            for template in &report.template_baseline {
                print_template_summary("template_baseline", template);
            }
            println!(
                "template_references.count: {}",
                report.template_references.len()
            );
            for template_reference in &report.template_references {
                print_template_reference("template_reference", template_reference);
            }
            println!("module_patterns.count: {}", report.module_patterns.len());
            for module in &report.module_patterns {
                print_module_summary("module_pattern", module);
            }
            println!(
                "docs_context.count: {}",
                report
                    .docs_context
                    .as_ref()
                    .map(|context| {
                        context.pages.len()
                            + context.sections.len()
                            + context.symbols.len()
                            + context.examples.len()
                    })
                    .unwrap_or(0)
            );
            if let Some(docs_context) = &report.docs_context {
                print_docs_context("docs_context", docs_context);
            }
            println!(
                "stub.existing_links.count: {}",
                report.stub_existing_links.len()
            );
            for link in &report.stub_existing_links {
                println!("stub.existing_link: {link}");
            }
            println!(
                "stub.missing_links.count: {}",
                report.stub_missing_links.len()
            );
            for link in &report.stub_missing_links {
                println!("stub.missing_link: {link}");
            }
            println!(
                "stub.detected_templates.count: {}",
                report.stub_detected_templates.len()
            );
            for template in &report.stub_detected_templates {
                print_stub_template_hint(template);
            }
            println!("chunks.retrieval_mode: {}", report.retrieval_mode);
            println!("chunks.count: {}", report.chunks.len());
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

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_workflow_ask(runtime: &RuntimeOptions, args: WorkflowAskArgs) -> Result<()> {
    let topic = derive_topic_from_prompt(&args.prompt)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| collapse_whitespace(&args.prompt));
    if topic.is_empty() {
        bail!("workflow ask requires a non-empty prompt");
    }

    run_workflow_authoring_pack(
        runtime,
        WorkflowAuthoringPackArgs {
            topic: Some(topic),
            stub_path: args.stub_path,
            related_limit: 18,
            chunk_limit: 10,
            token_budget: 1200,
            max_pages: 8,
            link_limit: 18,
            category_limit: 8,
            template_limit: 16,
            format: args.format,
            diversify: true,
            no_diversify: false,
        },
    )
}

fn print_authoring_suggestion(label: &str, suggestion: &AuthoringSuggestion) {
    println!(
        "{label}: {} (support={} evidence={})",
        suggestion.title,
        suggestion.support_count,
        if suggestion.evidence_titles.is_empty() {
            "<none>".to_string()
        } else {
            suggestion.evidence_titles.join(", ")
        }
    );
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

fn print_stub_template_hint(template: &StubTemplateHint) {
    println!(
        "stub.detected_template: {} (keys={})",
        template.template_title,
        if template.parameter_keys.is_empty() {
            "<none>".to_string()
        } else {
            template.parameter_keys.join(", ")
        }
    );
}

fn print_reference_summary(label: &str, reference: &ReferenceUsageSummary) {
    println!(
        "{label}: {} (family={} type={} origin={} source_family={} usage={} pages={} templates={} links={} domains={} authorities={} identifiers={} identifier_entries={} signals={})",
        reference.citation_profile,
        reference.citation_family,
        reference.source_type,
        reference.source_origin,
        reference.source_family,
        reference.usage_count,
        reference.distinct_page_count,
        if reference.common_templates.is_empty() {
            "<none>".to_string()
        } else {
            reference.common_templates.join(", ")
        },
        if reference.common_links.is_empty() {
            "<none>".to_string()
        } else {
            reference.common_links.join(", ")
        },
        if reference.common_domains.is_empty() {
            "<none>".to_string()
        } else {
            reference.common_domains.join(", ")
        },
        if reference.common_authorities.is_empty() {
            "<none>".to_string()
        } else {
            reference.common_authorities.join(", ")
        },
        if reference.common_identifier_keys.is_empty() {
            "<none>".to_string()
        } else {
            reference.common_identifier_keys.join(", ")
        },
        if reference.common_identifier_entries.is_empty() {
            "<none>".to_string()
        } else {
            reference.common_identifier_entries.join(", ")
        },
        if reference.common_retrieval_signals.is_empty() {
            "<none>".to_string()
        } else {
            reference.common_retrieval_signals.join(", ")
        }
    );
    if !reference.example_pages.is_empty() {
        println!(
            "{label}.example_pages: {}",
            reference.example_pages.join(", ")
        );
    }
    for example in &reference.example_references {
        println!(
            "{label}.example: profile={} source={} section={} name={} group={} family={} template={} type={} origin={} source_family={} authority_kind={} authority={} title={} container={} author={} domain={} date={} url={} summary={} templates={} links={} identifiers={} identifier_entries={} urls={} signals={} tokens={} text={}",
            reference.citation_profile,
            example.source_title,
            example.section_heading.as_deref().unwrap_or("<lead>"),
            example.reference_name.as_deref().unwrap_or("<none>"),
            example.reference_group.as_deref().unwrap_or("<none>"),
            example.citation_family,
            example
                .primary_template_title
                .as_deref()
                .unwrap_or("<none>"),
            example.source_type,
            example.source_origin,
            example.source_family,
            example.authority_kind,
            if example.source_authority.is_empty() {
                "<none>"
            } else {
                &example.source_authority
            },
            if example.reference_title.is_empty() {
                "<none>"
            } else {
                &example.reference_title
            },
            if example.source_container.is_empty() {
                "<none>"
            } else {
                &example.source_container
            },
            if example.source_author.is_empty() {
                "<none>"
            } else {
                &example.source_author
            },
            if example.source_domain.is_empty() {
                "<none>"
            } else {
                &example.source_domain
            },
            if example.source_date.is_empty() {
                "<none>"
            } else {
                &example.source_date
            },
            if example.canonical_url.is_empty() {
                "<none>"
            } else {
                &example.canonical_url
            },
            example.summary_text,
            if example.template_titles.is_empty() {
                "<none>".to_string()
            } else {
                example.template_titles.join(", ")
            },
            if example.link_titles.is_empty() {
                "<none>".to_string()
            } else {
                example.link_titles.join(", ")
            },
            if example.identifier_keys.is_empty() {
                "<none>".to_string()
            } else {
                example.identifier_keys.join(", ")
            },
            if example.identifier_entries.is_empty() {
                "<none>".to_string()
            } else {
                example.identifier_entries.join(", ")
            },
            if example.source_urls.is_empty() {
                "<none>".to_string()
            } else {
                example.source_urls.join(", ")
            },
            if example.retrieval_signals.is_empty() {
                "<none>".to_string()
            } else {
                example.retrieval_signals.join(", ")
            },
            example.token_estimate,
            example.reference_wikitext
        );
    }
}

fn print_template_reference(label: &str, reference: &TemplateReference) {
    println!(
        "{label}: {} (pages={} sections={} chunks={})",
        reference.template.template_title,
        reference.implementation_pages.len(),
        reference.implementation_sections.len(),
        reference.implementation_chunks.len()
    );
    for page in &reference.implementation_pages {
        println!(
            "{label}.page: template={} role={} page={} summary={}",
            reference.template.template_title,
            page.role,
            page.page_title,
            if page.summary_text.is_empty() {
                "<none>"
            } else {
                &page.summary_text
            }
        );
    }
}

fn print_module_summary(label: &str, module: &ModuleUsageSummary) {
    println!(
        "{label}: {} (usage={} pages={})",
        module.module_title, module.usage_count, module.distinct_page_count
    );
    for function in &module.function_stats {
        println!(
            "{label}.function: module={} name={} usage={} keys={}",
            module.module_title,
            function.function_name,
            function.usage_count,
            if function.example_parameter_keys.is_empty() {
                "<none>".to_string()
            } else {
                function.example_parameter_keys.join(", ")
            }
        );
    }
    for example in &module.example_invocations {
        println!(
            "{label}.example: module={} source={} function={} keys={} tokens={} text={}",
            module.module_title,
            example.source_title,
            example.function_name,
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

fn print_docs_context(label: &str, context: &wikitool_core::index::AuthoringDocsContext) {
    println!("{label}.profile: {}", context.profile);
    println!("{label}.queries: {}", context.queries.join(" | "));
    println!(
        "{label}.token_estimate_total: {}",
        context.token_estimate_total
    );
    println!("{label}.pages.count: {}", context.pages.len());
    for page in &context.pages {
        println!(
            "{label}.page: [{}] {} page={} weight={}",
            page.tier, page.title, page.page_title, page.retrieval_weight
        );
    }
    println!("{label}.sections.count: {}", context.sections.len());
    for section in &context.sections {
        println!(
            "{label}.section: page={} heading={} weight={}",
            section.page_title,
            section.section_heading.as_deref().unwrap_or("<lead>"),
            section.retrieval_weight
        );
    }
    println!("{label}.symbols.count: {}", context.symbols.len());
    for symbol in &context.symbols {
        println!(
            "{label}.symbol: [{}] {} page={} weight={}",
            symbol.symbol_kind, symbol.symbol_name, symbol.page_title, symbol.retrieval_weight
        );
    }
    println!("{label}.examples.count: {}", context.examples.len());
    for example in &context.examples {
        println!(
            "{label}.example: [{}] page={} lang={} weight={}",
            example.example_kind,
            example.page_title,
            if example.language_hint.is_empty() {
                "<none>"
            } else {
                &example.language_hint
            },
            example.retrieval_weight
        );
    }
}

fn print_media_summary(label: &str, media: &MediaUsageSummary) {
    println!(
        "{label}: {} (kind={} usage={} pages={})",
        media.file_title, media.media_kind, media.usage_count, media.distinct_page_count
    );
    if !media.example_pages.is_empty() {
        println!("{label}.example_pages: {}", media.example_pages.join(", "));
    }
    for example in &media.example_usages {
        println!(
            "{label}.example: file={} source={} section={} tokens={} caption={} options={}",
            media.file_title,
            example.source_title,
            example.section_heading.as_deref().unwrap_or("<lead>"),
            example.token_estimate,
            if example.caption_text.is_empty() {
                "<none>"
            } else {
                &example.caption_text
            },
            if example.options.is_empty() {
                "<none>".to_string()
            } else {
                example.options.join(", ")
            }
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

fn load_workflow_stub_content(
    paths: &ResolvedPaths,
    stub_path: Option<&Path>,
) -> Result<Option<String>> {
    let Some(path) = stub_path else {
        return Ok(None);
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    validate_scoped_path(paths, &absolute)?;
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read {}", normalize_path(&absolute)))?;
    Ok(Some(content))
}

fn derive_topic_from_stub_path(path: Option<&Path>) -> Option<String> {
    let path = path?;
    let stem = path.file_stem()?.to_string_lossy();
    let normalized = collapse_whitespace(&stem.replace('_', " "));
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn derive_topic_from_prompt(prompt: &str) -> Option<String> {
    let normalized = collapse_whitespace(prompt);
    if normalized.is_empty() {
        return None;
    }

    for prefix in [
        "please write an article on ",
        "please write an article about ",
        "write an article on ",
        "write an article about ",
        "write article on ",
        "write article about ",
        "write a wiki article on ",
        "write a wiki article about ",
        "draft an article on ",
        "draft an article about ",
        "draft a page on ",
        "draft a page about ",
        "create an article on ",
        "create an article about ",
        "create a page on ",
        "create a page about ",
        "write a page on ",
        "write a page about ",
        "article on ",
        "article about ",
    ] {
        if let Some(remainder) = strip_case_insensitive_prefix(&normalized, prefix) {
            let topic = trim_prompt_topic(remainder);
            if !topic.is_empty() {
                return Some(topic);
            }
        }
    }

    Some(trim_prompt_topic(&normalized))
}

fn trim_prompt_topic(value: &str) -> String {
    value
        .trim()
        .trim_matches(['.', '!', '?', ':', ';'])
        .trim()
        .trim_matches(['"', '\'', '`'])
        .trim()
        .to_string()
}

fn strip_case_insensitive_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .and_then(|_| value.get(prefix.len()..))
}

#[cfg(test)]
mod tests {
    use super::{derive_topic_from_prompt, derive_topic_from_stub_path};
    use std::path::Path;

    #[test]
    fn derive_topic_from_stub_path_normalizes_filename() {
        assert_eq!(
            derive_topic_from_stub_path(Some(Path::new("drafts/Remilia_Corporation.md"))),
            Some("Remilia Corporation".to_string())
        );
    }

    #[test]
    fn derive_topic_from_stub_path_rejects_blank_stem() {
        assert_eq!(
            derive_topic_from_stub_path(Some(Path::new("drafts/___.md"))),
            None
        );
    }

    #[test]
    fn derive_topic_from_prompt_extracts_article_subject() {
        assert_eq!(
            derive_topic_from_prompt("write an article on Remilia Corporation"),
            Some("Remilia Corporation".to_string())
        );
        assert_eq!(
            derive_topic_from_prompt("Draft an article about Milady Maker."),
            Some("Milady Maker".to_string())
        );
        assert_eq!(
            derive_topic_from_prompt("Please write an article on \"Milady\""),
            Some("Milady".to_string())
        );
    }

    #[test]
    fn derive_topic_from_prompt_falls_back_to_raw_prompt() {
        assert_eq!(
            derive_topic_from_prompt("Remilia Corporation"),
            Some("Remilia Corporation".to_string())
        );
    }
}
