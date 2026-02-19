use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
use wikitool_core::phase0::{command_surface, generate_fixture_snapshot};
use wikitool_core::phase1::{
    InitOptions, NO_MIGRATIONS_POLICY_MESSAGE, PathOverrides, ResolutionContext,
    embedded_parser_config, ensure_runtime_ready_for_sync, init_layout, inspect_runtime,
    lsp_settings_json, materialize_parser_config, resolve_paths,
};
use wikitool_core::phase2::{ScanOptions, ScanStats, scan_stats};
use wikitool_core::phase3::{
    StoredIndexStats, load_stored_index_stats, query_backlinks, query_empty_categories,
    query_orphans, rebuild_index,
};

#[derive(Debug, Parser)]
#[command(
    name = "wikitool",
    version,
    about = "Rust rewrite CLI for remilia-wikitool (Phase 4 index query slice)"
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
    Pull(PullArgs),
    Push,
    Diff,
    Status(StatusArgs),
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
struct PullArgs {
    #[arg(long, help = "Full refresh (ignore last pull timestamp)")]
    full: bool,
    #[arg(long, help = "Overwrite locally modified files during pull")]
    overwrite_local: bool,
    #[arg(short = 'c', long, value_name = "NAME", help = "Filter by category")]
    category: Option<String>,
    #[arg(long, help = "Pull templates instead of articles")]
    templates: bool,
    #[arg(long, help = "Pull Category: namespace pages")]
    categories: bool,
    #[arg(long, help = "Pull everything (articles, categories, and templates)")]
    all: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[arg(long, help = "Only show modified")]
    modified: bool,
    #[arg(long, help = "Only show conflicts")]
    conflicts: bool,
    #[arg(long, help = "Include templates")]
    templates: bool,
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
        Some(Commands::Pull(args)) => run_pull_preflight(&runtime, args),
        Some(Commands::Push) => run_stub(&runtime, "push"),
        Some(Commands::Diff) => run_stub(&runtime, "diff"),
        Some(Commands::Status(args)) => run_status(&runtime, args),
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
            DbSubcommand::Stats => run_db_stats(&runtime),
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
            IndexSubcommand::Rebuild => run_index_rebuild(&runtime),
            IndexSubcommand::Stats => run_index_stats(&runtime),
            IndexSubcommand::Backlinks { title } => run_index_backlinks(&runtime, &title),
            IndexSubcommand::Orphans => run_index_orphans(&runtime),
            IndexSubcommand::PruneCategories => run_index_prune_categories(&runtime),
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

fn run_pull_preflight(runtime: &RuntimeOptions, args: PullArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    println!("pull preflight");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("full: {}", args.full);
    println!("overwrite_local: {}", args.overwrite_local);
    println!("category: {}", args.category.as_deref().unwrap_or("<none>"));
    println!("templates: {}", args.templates);
    println!("categories: {}", args.categories);
    println!("all: {}", args.all);

    if !status.warnings.is_empty() {
        println!("warnings:");
        for warning in &status.warnings {
            println!("  - {warning}");
        }
    }
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    bail!(
        "`pull` network sync is not implemented yet in the Rust rewrite.\nPolicy: {}",
        NO_MIGRATIONS_POLICY_MESSAGE
    );
}

fn run_status(runtime: &RuntimeOptions, args: StatusArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    let scan = scan_stats(
        &paths,
        &ScanOptions {
            include_content: true,
            include_templates: args.templates,
        },
    )?;

    println!("runtime status");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "project_root_exists: {}",
        format_flag(status.project_root_exists)
    );
    println!(
        "wiki_content_exists: {}",
        format_flag(status.wiki_content_exists)
    );
    println!("templates_exists: {}", format_flag(status.templates_exists));
    println!("state_dir_exists: {}", format_flag(status.state_dir_exists));
    println!("data_dir_exists: {}", format_flag(status.data_dir_exists));
    println!("db_exists: {}", format_flag(status.db_exists));
    println!(
        "db_size_bytes: {}",
        status
            .db_size_bytes
            .map(|size| size.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    );
    println!("config_exists: {}", format_flag(status.config_exists));
    println!(
        "parser_config_exists: {}",
        format_flag(status.parser_config_exists)
    );
    println!("filters.modified: {}", args.modified);
    println!("filters.conflicts: {}", args.conflicts);
    println!("filters.templates: {}", args.templates);
    print_scan_stats("scan", &scan);
    if !status.warnings.is_empty() {
        println!("warnings:");
        for warning in &status.warnings {
            println!("  - {warning}");
        }
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn run_index_rebuild(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    let report = rebuild_index(&paths, &ScanOptions::default())?;

    println!("index rebuild");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("db_path: {}", normalize_path(&paths.db_path));
    println!("inserted_rows: {}", report.inserted_rows);
    println!("inserted_links: {}", report.inserted_links);
    print_scan_stats("scan", &report.scan);
    println!("migrations: disabled");
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn run_index_backlinks(runtime: &RuntimeOptions, title: &str) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let normalized_title = title.trim();

    println!("index backlinks");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("target: {normalized_title}");
    if normalized_title.is_empty() {
        bail!("index backlinks requires a non-empty title");
    }

    match query_backlinks(&paths, normalized_title)? {
        Some(backlinks) => {
            println!("backlinks.count: {}", backlinks.len());
            if backlinks.is_empty() {
                println!("backlinks: <none>");
            } else {
                for source in backlinks {
                    println!("backlinks.source: {source}");
                }
            }
        }
        None => {
            println!("index.storage: <not built> (run `wikitool index rebuild`)");
        }
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_index_orphans(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("index orphans");
    println!("project_root: {}", normalize_path(&paths.project_root));
    match query_orphans(&paths)? {
        Some(orphans) => {
            println!("orphans.count: {}", orphans.len());
            if orphans.is_empty() {
                println!("orphans: <none>");
            } else {
                for title in orphans {
                    println!("orphans.title: {title}");
                }
            }
        }
        None => {
            println!("index.storage: <not built> (run `wikitool index rebuild`)");
        }
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_index_prune_categories(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("index prune-categories");
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
            println!("index.storage: <not built> (run `wikitool index rebuild`)");
        }
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_index_stats(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let scan = scan_stats(&paths, &ScanOptions::default())?;
    let stored = load_stored_index_stats(&paths)?;

    println!("index stats");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "wiki_content_dir: {}",
        normalize_path(&paths.wiki_content_dir)
    );
    println!("templates_dir: {}", normalize_path(&paths.templates_dir));
    print_scan_stats("scan", &scan);
    match stored {
        Some(stored) => print_stored_index_stats("index", &stored),
        None => println!("index.storage: <not built> (run `wikitool index rebuild`)"),
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn run_db_stats(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    let stored = load_stored_index_stats(&paths)?;

    println!("db stats");
    println!("db_path: {}", normalize_path(&paths.db_path));
    println!("data_dir: {}", normalize_path(&paths.data_dir));
    println!("db_exists: {}", format_flag(status.db_exists));
    println!(
        "db_size_bytes: {}",
        status
            .db_size_bytes
            .map(|size| size.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    );
    match stored {
        Some(stored) => print_stored_index_stats("index", &stored),
        None => println!("index.storage: <not built> (run `wikitool index rebuild`)"),
    }
    println!("migrations: disabled");
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn print_scan_stats(prefix: &str, stats: &ScanStats) {
    println!("{prefix}.total_files: {}", stats.total_files);
    println!("{prefix}.content_files: {}", stats.content_files);
    println!("{prefix}.template_files: {}", stats.template_files);
    println!("{prefix}.redirects: {}", stats.redirects);
    if stats.by_namespace.is_empty() {
        println!("{prefix}.by_namespace: <empty>");
    } else {
        for (namespace, count) in &stats.by_namespace {
            println!("{prefix}.namespace.{namespace}: {count}");
        }
    }
}

fn print_stored_index_stats(prefix: &str, stats: &StoredIndexStats) {
    println!("{prefix}.indexed_rows: {}", stats.indexed_rows);
    println!("{prefix}.redirects: {}", stats.redirects);
    if stats.by_namespace.is_empty() {
        println!("{prefix}.by_namespace: <empty>");
    } else {
        for (namespace, count) in &stats.by_namespace {
            println!("{prefix}.namespace.{namespace}: {count}");
        }
    }
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

fn format_flag(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
