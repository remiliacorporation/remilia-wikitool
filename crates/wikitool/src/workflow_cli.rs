use std::fs;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use wikitool_core::profile::sync_wiki_profile_with_config;

use crate::docs_cli::{self, DocsGenerateReferenceArgs};
use crate::knowledge_cli::{self, KnowledgeWarmArgs};
use crate::quality_cli;
use crate::sync_cli::{self, InitArgs, PullArgs, StatusArgs};
use crate::{
    RuntimeOptions, cli_support::OutputFormat, cli_support::normalize_path,
    cli_support::prompt_yes_no, cli_support::resolve_default_true_flag,
    cli_support::resolve_runtime_paths, cli_support::resolve_runtime_with_config,
};

#[derive(Debug, Args)]
pub(crate) struct WorkflowArgs {
    #[command(subcommand)]
    command: WorkflowSubcommand,
}

#[derive(Debug, Subcommand)]
enum WorkflowSubcommand {
    #[command(
        name = "session-refresh",
        about = "Refresh runtime content and agent authoring context"
    )]
    SessionRefresh(WorkflowSessionRefreshArgs),
    #[command(
        name = "full-refresh",
        about = "Rebuild local runtime from scratch and re-warm knowledge"
    )]
    FullRefresh(WorkflowFullRefreshArgs),
}

#[derive(Debug, Args)]
struct WorkflowSessionRefreshArgs {
    #[arg(long, help = "Create templates/ during initialization (default: true)")]
    templates: bool,
    #[arg(long, help = "Do not create templates/ during initialization")]
    no_templates: bool,
    #[arg(
        long,
        help = "Perform a full pull instead of an incremental session pull"
    )]
    full: bool,
    #[arg(long, help = "Pull content after initialization (default: true)")]
    pull: bool,
    #[arg(long, help = "Skip content pull during session refresh")]
    no_pull: bool,
    #[arg(
        long,
        default_value = wikitool_core::knowledge::status::DEFAULT_DOCS_PROFILE,
        value_name = "PROFILE",
        help = "Docs profile to hydrate during knowledge warmup"
    )]
    docs_profile: String,
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
    #[arg(
        long,
        default_value = wikitool_core::knowledge::status::DEFAULT_DOCS_PROFILE,
        value_name = "PROFILE",
        help = "Docs profile to hydrate during knowledge warmup"
    )]
    docs_profile: String,
}

pub(crate) fn run_workflow(runtime: &RuntimeOptions, args: WorkflowArgs) -> Result<()> {
    match args.command {
        WorkflowSubcommand::SessionRefresh(options) => {
            run_workflow_session_refresh(runtime, options)
        }
        WorkflowSubcommand::FullRefresh(options) => run_workflow_full_refresh(runtime, options),
    }
}

fn run_workflow_session_refresh(
    runtime: &RuntimeOptions,
    args: WorkflowSessionRefreshArgs,
) -> Result<()> {
    let include_templates = resolve_default_true_flag(
        args.templates,
        args.no_templates,
        "workflow session-refresh templates",
    )?;
    let should_pull =
        resolve_default_true_flag(args.pull, args.no_pull, "workflow session-refresh pull")?;

    sync_cli::run_init(
        runtime,
        InitArgs {
            templates: include_templates,
            force: false,
            no_config: false,
            no_parser_config: false,
        },
    )?;

    if should_pull {
        sync_cli::run_pull(
            runtime,
            PullArgs {
                full: args.full,
                overwrite_local: false,
                category: None,
                templates: false,
                categories: false,
                all: true,
                format: OutputFormat::Text,
            },
        )?;
    } else {
        println!("workflow session-refresh: pull skipped (--no-pull)");
    }

    knowledge_cli::run_knowledge_warm(
        runtime,
        KnowledgeWarmArgs {
            docs_profile: args.docs_profile,
            format: OutputFormat::Text,
        },
    )?;
    sync_profile_for_workflow(runtime)?;

    Ok(())
}

fn sync_profile_for_workflow(runtime: &RuntimeOptions) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let snapshot = sync_wiki_profile_with_config(&paths, &config)?;
    println!("workflow session-refresh: wiki profile synced");
    println!(
        "workflow session-refresh.profile.refreshed_at: {}",
        snapshot.overlay.refreshed_at
    );
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
            format: OutputFormat::Text,
        },
    )?;

    knowledge_cli::run_knowledge_warm(
        runtime,
        KnowledgeWarmArgs {
            docs_profile: args.docs_profile,
            format: OutputFormat::Text,
        },
    )?;

    if let Err(error) = quality_cli::run_validate(runtime, quality_cli::ValidateArgs::default()) {
        if error.to_string().starts_with("validation detected ") {
            println!("workflow full-refresh: validate reported content issues; continuing");
        } else {
            return Err(error);
        }
    }
    sync_cli::run_status(
        runtime,
        StatusArgs {
            modified: false,
            conflicts: false,
            templates: true,
            categories: false,
            titles: Vec::new(),
            paths: Vec::new(),
            titles_file: None,
            format: OutputFormat::Text,
        },
    )?;
    Ok(())
}
