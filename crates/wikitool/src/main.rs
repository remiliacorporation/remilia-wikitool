use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

pub(crate) use wikitool_core::schema::LOCAL_DB_POLICY_MESSAGE;

mod article_cli;
mod briefs;
mod cli_support;
mod config_cli;
mod db_cli;
#[cfg(feature = "maintainer")]
mod dev_cli;
mod docs_cli;
mod export_cli;
#[cfg(test)]
mod guidance_contracts;
mod import_cli;
mod knowledge_cli;
mod knowledge_inspect_cli;
mod lsp_cli;
mod module_cli;
mod ops_cli;
mod quality_cli;
mod query_cli;
#[cfg(feature = "maintainer")]
mod release;
mod research_cli;
mod review_cli;
mod sync_cli;
mod templates_cli;
mod wiki_cli;
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
    #[command(about = "Show resolved configuration and target-wiki sources")]
    Config(config_cli::ConfigArgs),
    #[command(about = "Pull wiki content and templates to local files")]
    Pull(sync_cli::PullArgs),
    #[command(about = "Push local changes to the live wiki")]
    Push(sync_cli::PushArgs),
    #[command(about = "Show local changes not yet pushed to the wiki")]
    Diff(sync_cli::DiffArgs),
    #[command(about = "Show sync status and local project state")]
    Status(sync_cli::StatusArgs),
    #[command(about = "Run structural and link integrity checks")]
    Validate(quality_cli::ValidateArgs),
    #[command(about = "Run the structured pre-push review gate")]
    Review(review_cli::ReviewArgs),
    #[command(about = "Run Lua module linting and related checks")]
    Module(module_cli::ModuleArgs),
    #[command(about = "Export a remote wiki page tree to local files")]
    Export(export_cli::ExportArgs),
    #[command(about = "Delete a page from the live wiki")]
    Delete(sync_cli::DeleteArgs),
    #[command(about = "Purge pages through the MediaWiki API")]
    Purge(ops_cli::PurgeArgs),
    #[command(about = "Upload a local file through the MediaWiki API")]
    Upload(ops_cli::UploadArgs),
    #[command(about = "Move (rename) a page through the MediaWiki API")]
    Move(ops_cli::MoveArgs),
    #[command(about = "Inspect or reset the local runtime database")]
    Db(db_cli::DbArgs),
    #[command(about = "Manage and query pinned MediaWiki docs corpora")]
    Docs(docs_cli::DocsArgs),
    #[command(about = "Import content from external sources")]
    Import(import_cli::ImportArgs),
    #[command(about = "Build and query the local knowledge layer")]
    Knowledge(knowledge_cli::KnowledgeArgs),
    #[command(
        about = "Inspect target-wiki evidence and fetch source URLs without mutating the wiki"
    )]
    Research(research_cli::ResearchArgs),
    #[command(about = "Sync and inspect live wiki capability metadata")]
    Wiki(wiki_cli::WikiArgs),
    #[command(about = "Build and inspect the local template catalog")]
    Templates(templates_cli::TemplatesArgs),
    #[command(about = "Lint and mechanically remediate article drafts")]
    Article(article_cli::ArticleArgs),
    #[command(about = "Generate parser config and editor integration settings")]
    Lsp(lsp_cli::LspArgs),
    #[command(about = "First-run setup and session/full runtime refresh workflows")]
    Workflow(workflow_cli::WorkflowArgs),
    #[cfg(feature = "maintainer")]
    #[command(about = "Build AI companion packs and release bundles", hide = true)]
    Release(release::ReleaseArgs),
    #[cfg(feature = "maintainer")]
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
        Some(Commands::Config(args)) => config_cli::run_config(&runtime, args),
        Some(Commands::Pull(args)) => sync_cli::run_pull(&runtime, args),
        Some(Commands::Push(args)) => sync_cli::run_push(&runtime, args),
        Some(Commands::Diff(args)) => sync_cli::run_diff(&runtime, args),
        Some(Commands::Status(args)) => sync_cli::run_status(&runtime, args),
        Some(Commands::Validate(args)) => quality_cli::run_validate(&runtime, args),
        Some(Commands::Review(args)) => review_cli::run_review(&runtime, args),
        Some(Commands::Module(args)) => module_cli::run_module(&runtime, args),
        Some(Commands::Export(args)) => export_cli::run_export(&runtime, args),
        Some(Commands::Delete(args)) => sync_cli::run_delete(&runtime, args),
        Some(Commands::Purge(args)) => ops_cli::run_purge(&runtime, args),
        Some(Commands::Upload(args)) => ops_cli::run_upload(&runtime, args),
        Some(Commands::Move(args)) => ops_cli::run_move(&runtime, args),
        Some(Commands::Db(args)) => db_cli::run_db(&runtime, args),
        Some(Commands::Docs(args)) => docs_cli::run_docs(&runtime, args),
        Some(Commands::Import(args)) => import_cli::run_import(&runtime, args),
        Some(Commands::Knowledge(args)) => knowledge_cli::run_knowledge(&runtime, args),
        Some(Commands::Research(args)) => research_cli::run_research(&runtime, args),
        Some(Commands::Wiki(args)) => wiki_cli::run_wiki(&runtime, args),
        Some(Commands::Templates(args)) => templates_cli::run_templates(&runtime, args),
        Some(Commands::Article(args)) => article_cli::run_article(&runtime, args),
        Some(Commands::Lsp(args)) => lsp_cli::run_lsp(&runtime, args),
        Some(Commands::Workflow(args)) => workflow_cli::run_workflow(&runtime, args),
        #[cfg(feature = "maintainer")]
        Some(Commands::Release(args)) => release::run_release(args),
        #[cfg(feature = "maintainer")]
        Some(Commands::Dev(args)) => dev_cli::run_dev(args),
        None => {
            if runtime.diagnostics {
                let paths = cli_support::resolve_runtime_paths(&runtime)?;
                println!("wikitool diagnostics");
                println!("{}", paths.diagnostics());
                return Ok(());
            }
            let mut command = Cli::command();
            command.print_help()?;
            println!();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn research_wiki_search_is_the_canonical_configured_wiki_search() {
        let cli = Cli::try_parse_from(["wikitool", "research", "wiki-search", "Remilia"])
            .expect("parse canonical research wiki-search");

        assert!(matches!(
            cli.command,
            Some(Commands::Research(research_cli::ResearchArgs { .. }))
        ));
    }

    #[test]
    fn research_search_is_not_a_wiki_search_alias() {
        let error = Cli::try_parse_from(["wikitool", "research", "search", "Remilia"])
            .expect_err("research search should not parse as wiki-search");

        assert!(error.to_string().contains("unrecognized subcommand"));
    }

    #[test]
    fn brief_view_surfaces_parse_and_reject_card_wording() {
        let valid_cases: &[&[&str]] = &[
            &[
                "wikitool",
                "knowledge",
                "article-start",
                "Remilia",
                "--format",
                "json",
                "--view",
                "brief",
                "--brief-path",
                ".wikitool/interviews/Remilia/20260601T172430Z.brief.md",
            ],
            &[
                "wikitool",
                "knowledge",
                "inspect",
                "chunks",
                "--across-pages",
                "--query",
                "Remilia",
                "--format",
                "json",
                "--view",
                "brief",
            ],
            &[
                "wikitool",
                "templates",
                "show",
                "Template:Cite web",
                "--format",
                "json",
                "--view",
                "brief",
            ],
            &[
                "wikitool", "wiki", "surface", "show", "--format", "json", "--view", "brief",
            ],
            &[
                "wikitool",
                "knowledge",
                "interview",
                "show",
                ".wikitool/interviews/Radbro_Webring/20260601T172430Z.brief.md",
                "--format",
                "json",
                "--view",
                "brief",
            ],
            &[
                "wikitool",
                "knowledge",
                "interview",
                "audit",
                "--format",
                "json",
                "--view",
                "brief",
            ],
            &[
                "wikitool",
                "review",
                "--format",
                "json",
                "--view",
                "brief",
                "--summary",
                "Review",
                "--brief-path",
                ".wikitool/interviews/Remilia/20260601T172430Z.brief.md",
            ],
        ];
        for args in valid_cases {
            Cli::try_parse_from(*args).expect("brief view should parse");
        }

        let rejected_cases: &[&[&str]] = &[
            &[
                "wikitool",
                "knowledge",
                "article-start",
                "Remilia",
                "--view",
                "agent-card",
            ],
            &[
                "wikitool",
                "templates",
                "show",
                "Template:Cite web",
                "--view",
                "function-card",
            ],
            &["wikitool", "review", "--view", "function-context"],
        ];
        for args in rejected_cases {
            Cli::try_parse_from(*args).expect_err("rejected card wording should not parse");
        }
    }

    #[test]
    fn retired_top_level_primitive_commands_are_not_invocable() {
        for command in ["context", "search", "fetch", "seo", "net"] {
            let error = Cli::try_parse_from(["wikitool", command])
                .expect_err("retired top-level command should not parse");

            assert!(
                error.to_string().contains("unrecognized subcommand"),
                "{command} should be retired"
            );
        }
    }

    #[test]
    fn knowledge_interview_command_family_parses() {
        let cases: &[&[&str]] = &[
            &[
                "wikitool",
                "knowledge",
                "interview",
                "init",
                "Radbro Webring",
                "--intent",
                "new",
                "--timestamp",
                "20260601T172430Z",
                "--format",
                "json",
            ],
            &[
                "wikitool",
                "knowledge",
                "interview",
                "validate",
                ".wikitool/interviews/Radbro_Webring/20260601T172430Z.brief.md",
            ],
            &[
                "wikitool",
                "knowledge",
                "interview",
                "show",
                ".wikitool/interviews/Radbro_Webring/20260601T172430Z.brief.md",
                "--view",
                "full",
            ],
            &["wikitool", "knowledge", "interview", "audit"],
            &[
                "wikitool",
                "knowledge",
                "interview",
                "open-item",
                "add",
                ".wikitool/interviews/Radbro_Webring/20260601T172430Z.brief.md",
                "--kind",
                "rejected-source",
                "--status",
                "open",
                "--text",
                "Mirror did not support the claimed date.",
                "--source-lead",
                "https://example.org/archive",
            ],
            &[
                "wikitool",
                "knowledge",
                "interview",
                "open-item",
                "list",
                ".wikitool/interviews/Radbro_Webring/20260601T172430Z.brief.md",
            ],
        ];

        for args in cases {
            Cli::try_parse_from(*args).expect("knowledge interview command should parse");
        }
    }

    #[test]
    fn retained_compatibility_aliases_are_not_invocable() {
        let cases: &[&[&str]] = &[
            &["wikitool", "db", "status"],
            &["wikitool", "validate", "--no-fail"],
            &["wikitool", "validate", "--category", "broken"],
            &["wikitool", "validate", "--category", "redirects"],
            &["wikitool", "validate", "--category", "double"],
            &["wikitool", "validate", "--category", "uncategorized"],
            &["wikitool", "validate", "--category", "orphans"],
            &[
                "wikitool",
                "research",
                "fetch",
                "https://example.org",
                "--format",
                "rendered_html",
            ],
            &[
                "wikitool",
                "research",
                "wiki-search",
                "Remilia",
                "--what",
                "near-match",
            ],
            &[
                "wikitool",
                "export",
                "https://example.org/wiki/Page",
                "--format",
                "md",
            ],
            &[
                "wikitool",
                "export",
                "https://example.org/wiki/Page",
                "--format",
                "wiki",
            ],
        ];

        for args in cases {
            Cli::try_parse_from(*args).expect_err("compatibility alias should not parse");
        }
    }

    #[test]
    fn mediawiki_operation_commands_parse() {
        let purge = Cli::try_parse_from([
            "wikitool",
            "purge",
            "Main Page",
            "--title",
            "Template:Trait gallery",
            "--forcelinkupdate",
            "--format",
            "json",
        ])
        .expect("purge should parse");
        assert!(matches!(purge.command, Some(Commands::Purge(_))));

        let upload = Cli::try_parse_from([
            "wikitool",
            "upload",
            "image.png",
            "--filename",
            "Example.png",
            "--comment",
            "test",
            "--ignore-warnings",
            "--dry-run",
        ])
        .expect("upload should parse");
        assert!(matches!(upload.command, Some(Commands::Upload(_))));

        let move_page = Cli::try_parse_from([
            "wikitool",
            "move",
            "Old Title",
            "New Title",
            "--reason",
            "test",
            "--no-redirect",
            "--move-talk",
            "--move-subpages",
            "--ignore-warnings",
            "--dry-run",
            "--format",
            "json",
        ])
        .expect("move should parse");
        assert!(matches!(move_page.command, Some(Commands::Move(_))));
    }
}
