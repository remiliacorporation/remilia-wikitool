use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
use wikitool_core::phase0::{command_surface, generate_fixture_snapshot};
use wikitool_core::phase1::{
    InitOptions, NO_MIGRATIONS_POLICY_MESSAGE, PathOverrides, ResolutionContext,
    embedded_parser_config, init_layout, lsp_settings_json, materialize_parser_config,
    resolve_paths,
};

#[derive(Debug, Parser)]
#[command(
    name = "wikitool",
    version,
    about = "Rust rewrite CLI for remilia-wikitool (Phase 1 bootstrap)"
)]
struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    project_root: Option<PathBuf>,
    #[arg(long, global = true, value_name = "PATH")]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long, global = true, help = "Print resolved runtime diagnostics")]
    diagnostics: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Clone)]
struct RuntimeOptions {
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
    Init(InitArgs),
    Pull,
    Push,
    Diff,
    Status,
    Context(ContextArgs),
    Search(SearchArgs),
    #[command(name = "search-external")]
    SearchExternal(SearchExternalArgs),
    Validate,
    Lint(LintArgs),
    Fetch(FetchArgs),
    Export(ExportArgs),
    Delete(DeleteArgs),
    Db(DbArgs),
    Docs(DocsArgs),
    Seo(SeoArgs),
    Net(NetArgs),
    Perf(PerfArgs),
    Import(ImportArgs),
    Index(IndexArgs),
    #[command(name = "lsp:generate-config")]
    LspGenerateConfig(LspGenerateConfigArgs),
    #[command(name = "lsp:status")]
    LspStatus,
    #[command(name = "lsp:info")]
    LspInfo,
    #[command(about = "Phase 0 bootstrap and differential harness helpers")]
    Phase0(Phase0Args),
}

#[derive(Debug, Args)]
struct InitArgs {
    #[arg(long, help = "Create templates/ during initialization")]
    templates: bool,
    #[arg(long, help = "Overwrite existing config/parser files")]
    force: bool,
    #[arg(long, help = "Skip writing .wikitool/config.toml")]
    no_config: bool,
    #[arg(long, help = "Skip writing .wikitool/remilia-parser.json")]
    no_parser_config: bool,
}

#[derive(Debug, Args)]
struct ContextArgs {
    title: String,
}

#[derive(Debug, Args)]
struct SearchArgs {
    query: String,
}

#[derive(Debug, Args)]
struct SearchExternalArgs {
    query: String,
}

#[derive(Debug, Args)]
struct LintArgs {
    title: Option<String>,
}

#[derive(Debug, Args)]
struct FetchArgs {
    url: String,
}

#[derive(Debug, Args)]
struct ExportArgs {
    url: String,
}

#[derive(Debug, Args)]
struct DeleteArgs {
    title: String,
}

#[derive(Debug, Args)]
struct DbArgs {
    #[command(subcommand)]
    command: DbSubcommand,
}

#[derive(Debug, Subcommand)]
enum DbSubcommand {
    Stats,
    Sync,
    Migrate,
}

#[derive(Debug, Args)]
struct DocsArgs {
    #[command(subcommand)]
    command: DocsSubcommand,
}

#[derive(Debug, Subcommand)]
enum DocsSubcommand {
    Import,
    #[command(name = "import-technical")]
    ImportTechnical,
    List,
    Update,
    Remove {
        target: String,
    },
    Search {
        query: String,
    },
}

#[derive(Debug, Args)]
struct SeoArgs {
    #[command(subcommand)]
    command: SeoSubcommand,
}

#[derive(Debug, Subcommand)]
enum SeoSubcommand {
    Inspect { target: String },
}

#[derive(Debug, Args)]
struct NetArgs {
    #[command(subcommand)]
    command: NetSubcommand,
}

#[derive(Debug, Subcommand)]
enum NetSubcommand {
    Inspect { target: String },
}

#[derive(Debug, Args)]
struct PerfArgs {
    #[command(subcommand)]
    command: PerfSubcommand,
}

#[derive(Debug, Subcommand)]
enum PerfSubcommand {
    Lighthouse { target: Option<String> },
}

#[derive(Debug, Args)]
struct ImportArgs {
    #[command(subcommand)]
    command: ImportSubcommand,
}

#[derive(Debug, Subcommand)]
enum ImportSubcommand {
    Cargo { path: String },
}

#[derive(Debug, Args)]
struct IndexArgs {
    #[command(subcommand)]
    command: IndexSubcommand,
}

#[derive(Debug, Subcommand)]
enum IndexSubcommand {
    Rebuild,
    Stats,
    Backlinks {
        title: String,
    },
    Orphans,
    #[command(name = "prune-categories")]
    PruneCategories,
}

#[derive(Debug, Args)]
struct LspGenerateConfigArgs {
    #[arg(long, help = "Overwrite parser config if it already exists")]
    force: bool,
}

#[derive(Debug, Args)]
struct Phase0Args {
    #[command(subcommand)]
    command: Phase0Command,
}

#[derive(Debug, Subcommand)]
enum Phase0Command {
    #[command(about = "Generate an offline fixture snapshot used by the differential harness")]
    Snapshot(SnapshotArgs),
    #[command(about = "Print frozen command-surface contract as JSON")]
    CommandSurface,
}

#[derive(Debug, Args)]
struct SnapshotArgs {
    #[arg(long, default_value = ".")]
    project_root: PathBuf,
    #[arg(long, default_value = "wiki_content")]
    content_dir: String,
    #[arg(long, default_value = "custom/templates")]
    templates_dir: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let runtime = RuntimeOptions::from_cli(&cli);

    match cli.command {
        Some(Commands::Init(args)) => run_init(&runtime, args),
        Some(Commands::LspGenerateConfig(args)) => run_lsp_generate_config(&runtime, args),
        Some(Commands::LspStatus) => run_lsp_status(&runtime),
        Some(Commands::LspInfo) => run_lsp_info(),
        Some(Commands::Db(DbArgs {
            command: DbSubcommand::Migrate,
        })) => run_db_migrate_policy_error(&runtime),
        Some(Commands::Phase0(phase0)) => run_phase0(phase0),
        Some(Commands::Pull) => run_stub(&runtime, "pull"),
        Some(Commands::Push) => run_stub(&runtime, "push"),
        Some(Commands::Diff) => run_stub(&runtime, "diff"),
        Some(Commands::Status) => run_stub(&runtime, "status"),
        Some(Commands::Context(ContextArgs { title })) => {
            run_stub(&runtime, &format!("context {title}"))
        }
        Some(Commands::Search(SearchArgs { query })) => {
            run_stub(&runtime, &format!("search {query}"))
        }
        Some(Commands::SearchExternal(SearchExternalArgs { query })) => {
            run_stub(&runtime, &format!("search-external {query}"))
        }
        Some(Commands::Validate) => run_stub(&runtime, "validate"),
        Some(Commands::Lint(LintArgs { title })) => run_stub(
            &runtime,
            &match title {
                Some(title) => format!("lint {title}"),
                None => "lint".to_string(),
            },
        ),
        Some(Commands::Fetch(FetchArgs { url })) => run_stub(&runtime, &format!("fetch {url}")),
        Some(Commands::Export(ExportArgs { url })) => run_stub(&runtime, &format!("export {url}")),
        Some(Commands::Delete(DeleteArgs { title })) => {
            run_stub(&runtime, &format!("delete {title}"))
        }
        Some(Commands::Db(DbArgs { command })) => match command {
            DbSubcommand::Stats => run_stub(&runtime, "db stats"),
            DbSubcommand::Sync => run_stub(&runtime, "db sync"),
            DbSubcommand::Migrate => unreachable!(),
        },
        Some(Commands::Docs(DocsArgs { command })) => match command {
            DocsSubcommand::Import => run_stub(&runtime, "docs import"),
            DocsSubcommand::ImportTechnical => run_stub(&runtime, "docs import-technical"),
            DocsSubcommand::List => run_stub(&runtime, "docs list"),
            DocsSubcommand::Update => run_stub(&runtime, "docs update"),
            DocsSubcommand::Remove { target } => {
                run_stub(&runtime, &format!("docs remove {target}"))
            }
            DocsSubcommand::Search { query } => run_stub(&runtime, &format!("docs search {query}")),
        },
        Some(Commands::Seo(SeoArgs { command })) => match command {
            SeoSubcommand::Inspect { target } => {
                run_stub(&runtime, &format!("seo inspect {target}"))
            }
        },
        Some(Commands::Net(NetArgs { command })) => match command {
            NetSubcommand::Inspect { target } => {
                run_stub(&runtime, &format!("net inspect {target}"))
            }
        },
        Some(Commands::Perf(PerfArgs { command })) => match command {
            PerfSubcommand::Lighthouse { target } => run_stub(
                &runtime,
                &format!(
                    "perf lighthouse {}",
                    target.unwrap_or_else(|| "<default>".to_string())
                ),
            ),
        },
        Some(Commands::Import(ImportArgs { command })) => match command {
            ImportSubcommand::Cargo { path } => run_stub(&runtime, &format!("import cargo {path}")),
        },
        Some(Commands::Index(IndexArgs { command })) => match command {
            IndexSubcommand::Rebuild => run_stub(&runtime, "index rebuild"),
            IndexSubcommand::Stats => run_stub(&runtime, "index stats"),
            IndexSubcommand::Backlinks { title } => {
                run_stub(&runtime, &format!("index backlinks {title}"))
            }
            IndexSubcommand::Orphans => run_stub(&runtime, "index orphans"),
            IndexSubcommand::PruneCategories => run_stub(&runtime, "index prune-categories"),
        },
        None => {
            let mut command = Cli::command();
            command.print_help()?;
            println!();
            Ok(())
        }
    }
}

fn run_init(runtime: &RuntimeOptions, args: InitArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let report = init_layout(
        &paths,
        &InitOptions {
            include_templates: args.templates,
            materialize_config: !args.no_config,
            materialize_parser_config: !args.no_parser_config,
            force: args.force,
        },
    )?;

    println!("Initialized wikitool runtime layout");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("wiki_content: {}", normalize_path(&paths.wiki_content_dir));
    println!("templates: {}", normalize_path(&paths.templates_dir));
    println!("state_dir: {}", normalize_path(&paths.state_dir));
    println!("data_dir: {}", normalize_path(&paths.data_dir));
    println!("db_path: {}", normalize_path(&paths.db_path));
    println!("config_path: {}", normalize_path(&paths.config_path));
    println!(
        "parser_config_path: {}",
        normalize_path(&paths.parser_config_path)
    );
    println!("created_dirs: {}", report.created_dirs.len());
    println!("wrote_config: {}", report.wrote_config);
    println!("wrote_parser_config: {}", report.wrote_parser_config);
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn run_lsp_generate_config(runtime: &RuntimeOptions, args: LspGenerateConfigArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let wrote = materialize_parser_config(&paths, args.force)?;
    if wrote {
        println!(
            "Wrote parser config: {}",
            normalize_path(&paths.parser_config_path)
        );
    } else {
        println!(
            "Parser config already exists: {} (use --force to overwrite)",
            normalize_path(&paths.parser_config_path)
        );
    }
    println!();
    println!("{}", lsp_settings_json(&paths));
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_lsp_status(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    println!(
        "parser config: {} ({})",
        normalize_path(&paths.parser_config_path),
        if paths.parser_config_path.exists() {
            "found"
        } else {
            "missing"
        }
    );
    println!(
        "runtime config: {} ({})",
        normalize_path(&paths.config_path),
        if paths.config_path.exists() {
            "found"
        } else {
            "missing"
        }
    );
    println!(
        "embedded parser baseline bytes: {}",
        embedded_parser_config().len()
    );
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_lsp_info() -> Result<()> {
    println!("wikitext LSP integration");
    println!("  command: wikitool lsp:generate-config");
    println!("  output parser config: <project-root>/.wikitool/remilia-parser.json");
    println!("  policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    Ok(())
}

fn run_db_migrate_policy_error(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    println!("project_root: {}", normalize_path(&paths.project_root));
    bail!("`db migrate` is unavailable. {NO_MIGRATIONS_POLICY_MESSAGE}");
}

fn run_stub(runtime: &RuntimeOptions, command_name: &str) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    if runtime.diagnostics {
        println!("[diagnostics]\n{}", paths.diagnostics());
    }
    bail!(
        "`{command_name}` is not implemented yet in the Rust rewrite (Phase 1 stub).\nResolved runtime root: {}\nPolicy: {}",
        normalize_path(&paths.project_root),
        NO_MIGRATIONS_POLICY_MESSAGE
    );
}

fn run_phase0(args: Phase0Args) -> Result<()> {
    match args.command {
        Phase0Command::Snapshot(snapshot) => {
            let report = generate_fixture_snapshot(
                &snapshot.project_root,
                &snapshot.content_dir,
                &snapshot.templates_dir,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Phase0Command::CommandSurface => {
            println!("{}", serde_json::to_string_pretty(&command_surface())?);
        }
    }
    Ok(())
}

fn resolve_runtime_paths(runtime: &RuntimeOptions) -> Result<wikitool_core::phase1::ResolvedPaths> {
    dotenvy::dotenv().ok();

    let context = ResolutionContext::from_process()?;
    let overrides = PathOverrides {
        project_root: runtime.project_root.clone(),
        data_dir: runtime.data_dir.clone(),
        config: runtime.config.clone(),
    };

    let initial = resolve_paths(&context, &overrides)?;
    let project_env = initial.project_root.join(".env");
    if project_env.exists() {
        let _ = dotenvy::from_path_override(&project_env);
    }

    resolve_paths(&context, &overrides)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
