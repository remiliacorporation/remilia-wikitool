use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

pub(crate) use wikitool_core::schema::LOCAL_DB_POLICY_MESSAGE;

mod article_cli;
mod cli_support;
mod db_cli;
#[cfg(feature = "maintainer-surface")]
mod dev_cli;
mod docs_cli;
mod export_cli;
#[cfg(test)]
mod guidance_contracts;
mod import_cli;
mod inspect_cli;
mod knowledge_cli;
mod knowledge_inspect_cli;
mod lsp_cli;
mod module_cli;
mod quality_cli;
mod query_cli;
#[cfg(feature = "maintainer-surface")]
mod release;
mod research_cli;
mod review_cli;
mod sync_cli;
mod templates_cli;
mod wiki_cli;
#[cfg(feature = "maintainer-surface")]
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
    #[command(about = "Initialize a new wikitool project")]
    Init(sync_cli::InitArgs),
    #[command(about = "Pull wiki content and templates to local files")]
    Pull(sync_cli::PullArgs),
    #[command(about = "Push local changes to the live wiki")]
    Push(sync_cli::PushArgs),
    #[command(about = "Show local changes not yet pushed to the wiki")]
    Diff(sync_cli::DiffArgs),
    #[command(about = "Show sync status and local project state")]
    Status(sync_cli::StatusArgs),
    #[command(about = "Show indexed local page context for one title")]
    Context(query_cli::ContextArgs),
    #[command(about = "Search indexed local page titles")]
    Search(query_cli::SearchArgs),
    #[command(about = "Run structural and link integrity checks")]
    Validate(quality_cli::ValidateArgs),
    #[command(about = "Run the structured pre-push review gate")]
    Review(review_cli::ReviewArgs),
    #[command(about = "Run Lua module linting and related checks")]
    Module(module_cli::ModuleArgs),
    #[command(about = "Fetch a remote URL as wikitext or rendered HTML")]
    Fetch(export_cli::FetchArgs),
    #[command(about = "Export a remote wiki page tree to local files")]
    Export(export_cli::ExportArgs),
    #[command(about = "Delete a page from the live wiki")]
    Delete(sync_cli::DeleteArgs),
    #[command(about = "Inspect or reset the local runtime database")]
    Db(db_cli::DbArgs),
    #[command(about = "Manage and query pinned MediaWiki docs corpora")]
    Docs(docs_cli::DocsArgs),
    #[command(about = "Inspect SEO metadata for wiki pages")]
    Seo(inspect_cli::SeoArgs),
    #[command(about = "Inspect link network and page relationships")]
    Net(inspect_cli::NetArgs),
    #[command(about = "Import content from external sources")]
    Import(import_cli::ImportArgs),
    #[command(about = "Build and query the local knowledge layer")]
    Knowledge(knowledge_cli::KnowledgeArgs),
    #[command(about = "Search and fetch subject evidence without mutating the wiki")]
    Research(research_cli::ResearchArgs),
    #[command(about = "Sync and inspect live wiki capability metadata")]
    Wiki(wiki_cli::WikiArgs),
    #[command(about = "Build and inspect the local template catalog")]
    Templates(templates_cli::TemplatesArgs),
    #[command(about = "Lint and mechanically remediate article drafts")]
    Article(article_cli::ArticleArgs),
    #[command(about = "Generate parser config and editor integration settings")]
    Lsp(lsp_cli::LspArgs),
    #[cfg(feature = "maintainer-surface")]
    #[command(about = "Run maintainer runtime refresh workflows", hide = true)]
    Workflow(workflow_cli::WorkflowArgs),
    #[cfg(feature = "maintainer-surface")]
    #[command(about = "Build AI companion packs and release bundles", hide = true)]
    Release(release::ReleaseArgs),
    #[cfg(feature = "maintainer-surface")]
    #[command(about = "Install local development helpers", hide = true)]
    Dev(dev_cli::DevArgs),
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
        Some(Commands::Validate(args)) => quality_cli::run_validate(&runtime, args),
        Some(Commands::Review(args)) => review_cli::run_review(&runtime, args),
        Some(Commands::Module(args)) => module_cli::run_module(&runtime, args),
        Some(Commands::Fetch(args)) => export_cli::run_fetch(&runtime, args),
        Some(Commands::Export(args)) => export_cli::run_export(&runtime, args),
        Some(Commands::Delete(args)) => sync_cli::run_delete(&runtime, args),
        Some(Commands::Db(args)) => db_cli::run_db(&runtime, args),
        Some(Commands::Docs(args)) => docs_cli::run_docs(&runtime, args),
        Some(Commands::Seo(args)) => inspect_cli::run_seo(&runtime, args),
        Some(Commands::Net(args)) => inspect_cli::run_net(&runtime, args),
        Some(Commands::Import(args)) => import_cli::run_import(&runtime, args),
        Some(Commands::Knowledge(args)) => knowledge_cli::run_knowledge(&runtime, args),
        Some(Commands::Research(args)) => research_cli::run_research(&runtime, args),
        Some(Commands::Wiki(args)) => wiki_cli::run_wiki(&runtime, args),
        Some(Commands::Templates(args)) => templates_cli::run_templates(&runtime, args),
        Some(Commands::Article(args)) => article_cli::run_article(&runtime, args),
        Some(Commands::Lsp(args)) => lsp_cli::run_lsp(&runtime, args),
        #[cfg(feature = "maintainer-surface")]
        Some(Commands::Workflow(args)) => workflow_cli::run_workflow(&runtime, args),
        #[cfg(feature = "maintainer-surface")]
        Some(Commands::Release(args)) => release::run_release(args),
        #[cfg(feature = "maintainer-surface")]
        Some(Commands::Dev(args)) => dev_cli::run_dev(args),
        None => {
            let mut command = Cli::command();
            command.print_help()?;
            println!();
            Ok(())
        }
    }
}
