use std::fs;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::dev_cli::{self, InstallGitHooksArgs};
use crate::docs_cli::{self, DocsGenerateReferenceArgs};
use crate::knowledge_cli::{self, KnowledgeWarmArgs};
use crate::quality_cli;
use crate::sync_cli::{self, InitArgs, PullArgs, StatusArgs};
use crate::{
    RuntimeOptions, cli_support::normalize_path, cli_support::prompt_yes_no,
    cli_support::resolve_default_true_flag, cli_support::resolve_runtime_paths,
};

#[derive(Debug, Args)]
pub(crate) struct WorkflowArgs {
    #[command(subcommand)]
    command: WorkflowSubcommand,
}

#[derive(Debug, Subcommand)]
enum WorkflowSubcommand {
    #[command(about = "Initialize runtime, optionally pull content, and warm knowledge")]
    Bootstrap(WorkflowBootstrapArgs),
    #[command(
        name = "full-refresh",
        about = "Rebuild local runtime from scratch and re-warm knowledge"
    )]
    FullRefresh(WorkflowFullRefreshArgs),
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
        WorkflowSubcommand::Bootstrap(options) => run_workflow_bootstrap(runtime, options),
        WorkflowSubcommand::FullRefresh(options) => run_workflow_full_refresh(runtime, options),
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

    knowledge_cli::run_knowledge_warm(
        runtime,
        KnowledgeWarmArgs {
            docs_profile: args.docs_profile,
            format: "text".to_string(),
        },
    )?;

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

    knowledge_cli::run_knowledge_warm(
        runtime,
        KnowledgeWarmArgs {
            docs_profile: args.docs_profile,
            format: "text".to_string(),
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
