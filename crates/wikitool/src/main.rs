use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

pub(crate) use wikitool_core::schema::LOCAL_DB_POLICY_MESSAGE;

mod adapter_cli;
mod article_cli;
mod briefs;
mod catalog_cli;
mod catalog_inspect_cli;
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
mod interview_cli;
mod lsp_cli;
mod module_cli;
mod quality_cli;
mod query_cli;
#[cfg(feature = "maintainer")]
mod release;
mod review_cli;
mod source_cli;
mod sync_cli;
mod templates_cli;
mod wiki_cli;

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
    #[command(about = "Inspect and reconcile durable remote mutation receipts")]
    Mutation(sync_cli::MutationArgs),
    #[command(about = "Inspect or reset the local runtime database")]
    Db(db_cli::DbArgs),
    #[command(about = "Manage and query pinned MediaWiki docs corpora")]
    Docs(docs_cli::DocsArgs),
    #[command(about = "Import content from external sources")]
    Import(import_cli::ImportArgs),
    #[command(about = "Build and query the disposable local catalog")]
    Catalog(catalog_cli::CatalogArgs),
    #[command(about = "Inspect the explicit project-owned site adapter")]
    Adapter(adapter_cli::AdapterArgs),
    #[command(about = "Create, validate, show, and audit neutral article interview ledgers")]
    Interview(interview_cli::InterviewArgs),
    #[command(
        about = "Inspect target-wiki evidence and fetch source URLs without mutating the wiki"
    )]
    Source(source_cli::SourceArgs),
    #[command(about = "Sync and inspect live wiki capability metadata")]
    Wiki(wiki_cli::WikiArgs),
    #[command(about = "Build and inspect the local template catalog")]
    Templates(templates_cli::TemplatesArgs),
    #[command(about = "Lint and mechanically remediate article drafts")]
    Article(article_cli::ArticleArgs),
    #[command(about = "Generate parser config and editor integration settings")]
    Lsp(lsp_cli::LspArgs),
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
        Some(Commands::Mutation(args)) => sync_cli::run_mutation(&runtime, args),
        Some(Commands::Db(args)) => db_cli::run_db(&runtime, args),
        Some(Commands::Docs(args)) => docs_cli::run_docs(&runtime, args),
        Some(Commands::Import(args)) => import_cli::run_import(&runtime, args),
        Some(Commands::Catalog(args)) => catalog_cli::run_catalog(&runtime, args),
        Some(Commands::Adapter(args)) => adapter_cli::run_adapter(&runtime, args),
        Some(Commands::Interview(args)) => interview_cli::run_interview(&runtime, args),
        Some(Commands::Source(args)) => source_cli::run_source(&runtime, args),
        Some(Commands::Wiki(args)) => wiki_cli::run_wiki(&runtime, args),
        Some(Commands::Templates(args)) => templates_cli::run_templates(&runtime, args),
        Some(Commands::Article(args)) => article_cli::run_article(&runtime, args),
        Some(Commands::Lsp(args)) => lsp_cli::run_lsp(&runtime, args),
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
    fn source_wiki_search_is_the_canonical_configured_wiki_search() {
        let cli = Cli::try_parse_from(["wikitool", "source", "wiki-search", "Remilia"])
            .expect("parse canonical source wiki-search");

        assert!(matches!(
            cli.command,
            Some(Commands::Source(source_cli::SourceArgs { .. }))
        ));
    }

    #[test]
    fn source_search_is_not_a_wiki_search_alias() {
        let error = Cli::try_parse_from(["wikitool", "source", "search", "Remilia"])
            .expect_err("source search should not parse as wiki-search");

        assert!(error.to_string().contains("unrecognized subcommand"));
    }

    #[test]
    fn brief_view_surfaces_parse_and_reject_card_wording() {
        let valid_cases: &[&[&str]] = &[
            &[
                "wikitool",
                "article",
                "scout",
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
                "catalog",
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
                "wikitool", "catalog", "surface", "show", "--format", "json", "--view", "brief",
            ],
            &[
                "wikitool",
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
                "article",
                "scout",
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
        for command in ["context", "search", "fetch", "seo", "net", "knowledge"] {
            let error = Cli::try_parse_from(["wikitool", command])
                .expect_err("retired top-level command should not parse");

            assert!(
                error.to_string().contains("unrecognized subcommand"),
                "{command} should be retired"
            );
        }
    }

    #[test]
    fn adapter_capability_and_catalog_surfaces_are_explicit() {
        Cli::try_parse_from(["wikitool", "adapter", "inspect", "--format", "json"])
            .expect("adapter inspection should parse");
        Cli::try_parse_from([
            "wikitool",
            "wiki",
            "capabilities",
            "remote",
            "https://wiki.example.org/",
        ])
        .expect("remote capability probe should parse");
        Cli::try_parse_from(["wikitool", "catalog", "surface", "show"])
            .expect("catalog surface should parse");

        for retired in ["profile", "rules", "surface"] {
            let error = Cli::try_parse_from(["wikitool", "wiki", retired])
                .expect_err("aggregate wiki command should be retired");
            assert!(error.to_string().contains("unrecognized subcommand"));
        }
    }

    #[test]
    fn interview_command_family_parses() {
        let cases: &[&[&str]] = &[
            &[
                "wikitool",
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
                "interview",
                "validate",
                ".wikitool/interviews/Radbro_Webring/20260601T172430Z.brief.md",
            ],
            &[
                "wikitool",
                "interview",
                "show",
                ".wikitool/interviews/Radbro_Webring/20260601T172430Z.brief.md",
                "--view",
                "full",
            ],
            &["wikitool", "interview", "audit"],
            &[
                "wikitool",
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
                "interview",
                "open-item",
                "list",
                ".wikitool/interviews/Radbro_Webring/20260601T172430Z.brief.md",
            ],
        ];

        for args in cases {
            Cli::try_parse_from(*args).expect("interview command should parse");
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
                "source",
                "fetch",
                "https://example.org",
                "--format",
                "rendered_html",
            ],
            &[
                "wikitool",
                "source",
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
    fn mediawiki_read_commands_parse() {
        let cargo_count = Cli::try_parse_from([
            "wikitool", "wiki", "cargo", "count", "Traits", "--format", "json",
        ])
        .expect("wiki cargo count should parse");
        assert!(matches!(cargo_count.command, Some(Commands::Wiki(_))));

        let render_check = Cli::try_parse_from([
            "wikitool",
            "wiki",
            "render-check",
            "Redacted Remilio Babies Traits",
            "--scope-class",
            "trait-layered-gallery__item",
            "--expect-scopes",
            "18",
            "--require-interactive-link",
            "--require-href-contains",
            "(Remilio_mouth)",
            "--require-page-image",
            "Remilio_Mouth_Binky_Preview.png",
            "--format",
            "json",
        ])
        .expect("wiki render-check should parse");
        assert!(matches!(render_check.command, Some(Commands::Wiki(_))));

        let contract_render_check = Cli::try_parse_from([
            "wikitool",
            "templates",
            "contract",
            "render-check",
            "template-contract.json",
            "--format",
            "json",
        ])
        .expect("template contract render-check should parse");
        assert!(matches!(
            contract_render_check.command,
            Some(Commands::Templates(_))
        ));
    }
}
