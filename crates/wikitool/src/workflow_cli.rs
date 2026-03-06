use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use wikitool_core::filesystem::validate_scoped_path;
use wikitool_core::index::{
    AuthoringKnowledgePack, AuthoringKnowledgePackOptions, build_authoring_knowledge_pack,
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
use crate::{MIGRATIONS_POLICY_MESSAGE, RuntimeOptions};

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

pub(crate) fn run_workflow(runtime: &RuntimeOptions, args: WorkflowArgs) -> Result<()> {
    match args.command {
        WorkflowSubcommand::Bootstrap(options) => run_workflow_bootstrap(runtime, options),
        WorkflowSubcommand::FullRefresh(options) => run_workflow_full_refresh(runtime, options),
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
                "inventory.pages.total: {}",
                report.inventory.indexed_pages_total
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
            println!("related_pages.count: {}", report.related_pages.len());
            for page in &report.related_pages {
                println!(
                    "related_page: {} (namespace={} redirect={} source={})",
                    page.title,
                    page.namespace,
                    format_flag(page.is_redirect),
                    page.source
                );
            }
            println!("suggested_links.count: {}", report.suggested_links.len());
            for link in &report.suggested_links {
                println!("suggested_link: {link}");
            }
            println!(
                "suggested_categories.count: {}",
                report.suggested_categories.len()
            );
            for category in &report.suggested_categories {
                println!("suggested_category: {category}");
            }
            println!(
                "suggested_templates.count: {}",
                report.suggested_templates.len()
            );
            for template in &report.suggested_templates {
                println!(
                    "suggested_template: {} (usage={} keys={})",
                    template.template_title,
                    template.usage_count,
                    if template.parameter_keys.is_empty() {
                        "<none>".to_string()
                    } else {
                        template.parameter_keys.join(", ")
                    }
                );
            }
            println!(
                "template_baseline.count: {}",
                report.template_baseline.len()
            );
            for template in &report.template_baseline {
                println!(
                    "template_baseline: {} (usage={} keys={})",
                    template.template_title,
                    template.usage_count,
                    if template.parameter_keys.is_empty() {
                        "<none>".to_string()
                    } else {
                        template.parameter_keys.join(", ")
                    }
                );
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
                println!("stub.detected_template: {template}");
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

    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::derive_topic_from_stub_path;
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
}
