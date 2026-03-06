use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

pub(crate) use wikitool_core::runtime::MIGRATIONS_POLICY_MESSAGE;

mod cli_support;
mod contracts_cli;
mod db_cli;
mod dev_cli;
mod docs_cli;
mod export_cli;
mod import_cli;
mod index_cli;
mod inspect_cli;
mod lsp_cli;
mod quality_cli;
mod query_cli;
mod release;
mod sync_cli;
mod workflow_cli;

const LICENSE_AGPL: &str = include_str!("../../../LICENSE");
const LICENSE_SSL: &str = include_str!("../../../LICENSE-SSL");
const LICENSE_VPL: &str = include_str!("../../../LICENSE-VPL");

#[derive(Debug, Parser)]
#[command(name = "wikitool", version, about = "Wiki management CLI")]
pub(crate) struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    project_root: Option<PathBuf>,
    #[arg(long, global = true, value_name = "PATH")]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long, global = true, help = "Print resolved runtime diagnostics")]
    diagnostics: bool,
    #[arg(long, help = "Print license information and exit")]
    license: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeOptions {
    project_root: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    config: Option<PathBuf>,
    diagnostics: bool,
}

impl RuntimeOptions {
    fn from_cli(cli: &Cli) -> Self {
        Self {
            project_root: cli.project_root.clone(),
            data_dir: cli.data_dir.clone(),
            config: cli.config.clone(),
            diagnostics: cli.diagnostics,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    Init(sync_cli::InitArgs),
    Pull(sync_cli::PullArgs),
    Push(sync_cli::PushArgs),
    Diff(sync_cli::DiffArgs),
    Status(sync_cli::StatusArgs),
    Context(query_cli::ContextArgs),
    Search(query_cli::SearchArgs),
    #[command(name = "search-external")]
    SearchExternal(query_cli::SearchExternalArgs),
    Validate,
    Lint(quality_cli::LintArgs),
    Fetch(export_cli::FetchArgs),
    Export(export_cli::ExportArgs),
    Delete(sync_cli::DeleteArgs),
    Db(db_cli::DbArgs),
    Docs(docs_cli::DocsArgs),
    Seo(inspect_cli::SeoArgs),
    Net(inspect_cli::NetArgs),
    Perf(inspect_cli::PerfArgs),
    Import(import_cli::ImportArgs),
    Index(index_cli::IndexArgs),
    #[command(name = "lsp:generate-config")]
    LspGenerateConfig(lsp_cli::LspGenerateConfigArgs),
    #[command(name = "lsp:status")]
    LspStatus,
    #[command(name = "lsp:info")]
    LspInfo,
    Workflow(workflow_cli::WorkflowArgs),
    Release(release::ReleaseArgs),
    Dev(dev_cli::DevArgs),
    #[command(
        name = "contracts",
        about = "Contract bootstrap and differential harness helpers"
    )]
    Contracts(contracts_cli::ContractsArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.license {
        print!("{LICENSE_AGPL}");
        println!("\n{}", "=".repeat(72));
        println!("SUPPLEMENTARY TERMS\n");
        println!("This software is additionally subject to the following terms:\n");
        print!("{LICENSE_SSL}");
        println!();
        print!("{LICENSE_VPL}");
        return Ok(());
    }

    let runtime = RuntimeOptions::from_cli(&cli);

    match cli.command {
        Some(Commands::Init(args)) => sync_cli::run_init(&runtime, args),
        Some(Commands::Pull(args)) => sync_cli::run_pull(&runtime, args),
        Some(Commands::Push(args)) => sync_cli::run_push(&runtime, args),
        Some(Commands::Diff(args)) => sync_cli::run_diff(&runtime, args),
        Some(Commands::Status(args)) => sync_cli::run_status(&runtime, args),
        Some(Commands::Context(args)) => query_cli::run_context(&runtime, args),
        Some(Commands::Search(args)) => query_cli::run_search(&runtime, args),
        Some(Commands::SearchExternal(args)) => query_cli::run_search_external(&runtime, args),
        Some(Commands::Validate) => quality_cli::run_validate(&runtime),
        Some(Commands::Lint(args)) => quality_cli::run_lint(&runtime, args),
        Some(Commands::Fetch(args)) => export_cli::run_fetch(&runtime, args),
        Some(Commands::Export(args)) => export_cli::run_export(&runtime, args),
        Some(Commands::Delete(args)) => sync_cli::run_delete(&runtime, args),
        Some(Commands::Db(args)) => db_cli::run_db(&runtime, args),
        Some(Commands::Docs(args)) => docs_cli::run_docs(&runtime, args),
        Some(Commands::Seo(args)) => inspect_cli::run_seo(&runtime, args),
        Some(Commands::Net(args)) => inspect_cli::run_net(&runtime, args),
        Some(Commands::Perf(args)) => inspect_cli::run_perf(&runtime, args),
        Some(Commands::Import(args)) => import_cli::run_import(&runtime, args),
        Some(Commands::Index(args)) => index_cli::run_index(&runtime, args),
        Some(Commands::LspGenerateConfig(args)) => lsp_cli::run_lsp_generate_config(&runtime, args),
        Some(Commands::LspStatus) => lsp_cli::run_lsp_status(&runtime),
        Some(Commands::LspInfo) => lsp_cli::run_lsp_info(),
        Some(Commands::Workflow(args)) => workflow_cli::run_workflow(&runtime, args),
        Some(Commands::Release(args)) => release::run_release(args),
        Some(Commands::Dev(args)) => dev_cli::run_dev(args),
        Some(Commands::Contracts(args)) => contracts_cli::run_contracts(args),
        None => {
            let mut command = Cli::command();
            command.print_help()?;
            println!();
            Ok(())
        }
    }
}
