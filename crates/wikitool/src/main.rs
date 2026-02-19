use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
use wikitool_core::contracts::{command_surface, generate_fixture_snapshot};
use wikitool_core::delete::{DeleteOptions as LocalDeleteOptions, DeleteReport, delete_local_page};
use wikitool_core::docs::{
    DocsImportOptions, DocsImportTechnicalOptions, DocsListOptions, DocsRemoveKind,
    TechnicalDocType, TechnicalImportTask, discover_installed_extensions_from_wiki,
    format_expiration, import_docs_bundle, import_extension_docs, import_technical_docs, list_docs,
    remove_docs, search_docs, update_outdated_docs,
};
use wikitool_core::filesystem::{ScanOptions, ScanStats, scan_files, scan_stats};
use wikitool_core::index::{
    LocalContextBundle, LocalSearchHit, StoredIndexStats, build_local_context,
    load_stored_index_stats, query_backlinks, query_empty_categories, query_orphans,
    query_search_local, rebuild_index, run_validation_checks,
};
use wikitool_core::runtime::{
    InitOptions, NO_MIGRATIONS_POLICY_MESSAGE, PathOverrides, ResolutionContext,
    embedded_parser_config, ensure_runtime_ready_for_sync, init_layout, inspect_runtime,
    lsp_settings_json, materialize_parser_config, resolve_paths,
};
use wikitool_core::sync::{
    DiffChangeType, DiffOptions, ExternalSearchHit, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE,
    NS_TEMPLATE, PullOptions, PushOptions, diff_local_against_sync, pull_from_remote,
    push_to_remote, search_external_wiki,
};

#[derive(Debug, Parser)]
#[command(
    name = "wikitool",
    version,
    about = "Rust rewrite CLI for remilia-wikitool"
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
    Push(PushArgs),
    Diff(DiffArgs),
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
    #[command(
        name = "contracts",
        about = "Contract bootstrap and differential harness helpers"
    )]
    Contracts(ContractsArgs),
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
struct PushArgs {
    #[arg(long, value_name = "TEXT", help = "Edit summary for pushed changes")]
    summary: Option<String>,
    #[arg(long, help = "Preview push actions without writing to the wiki")]
    dry_run: bool,
    #[arg(long, help = "Force push even when remote timestamps diverge")]
    force: bool,
    #[arg(long, help = "Propagate local deletions to remote wiki pages")]
    delete: bool,
    #[arg(long, help = "Include template/module/mediawiki namespaces")]
    templates: bool,
    #[arg(long, help = "Limit push to Category namespace pages")]
    categories: bool,
}

#[derive(Debug, Args)]
struct DiffArgs {
    #[arg(long, help = "Include template/module/mediawiki namespaces")]
    templates: bool,
    #[arg(long, help = "Show hash-level details for modified entries")]
    verbose: bool,
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
    #[arg(long, value_name = "TEXT", help = "Reason for deletion (required)")]
    reason: String,
    #[arg(long, help = "Skip backup (not recommended)")]
    no_backup: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Custom backup directory under .wikitool/"
    )]
    backup_dir: Option<PathBuf>,
    #[arg(long, help = "Preview deletion without making changes")]
    dry_run: bool,
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
    Import(DocsImportArgs),
    #[command(name = "import-technical")]
    ImportTechnical(DocsImportTechnicalArgs),
    List(DocsListArgs),
    Update,
    Remove {
        target: String,
    },
    Search {
        query: String,
        #[arg(long, value_name = "TIER", help = "Search tier (extension, technical)")]
        tier: Option<String>,
        #[arg(short = 'l', long, default_value_t = 20, help = "Limit result count")]
        limit: usize,
    },
}

#[derive(Debug, Args)]
struct DocsImportArgs {
    #[arg(value_name = "EXTENSION")]
    extensions: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Import docs from precomposed bundle JSON"
    )]
    bundle: Option<PathBuf>,
    #[arg(
        long = "installed",
        help = "Discover installed extensions from live wiki API"
    )]
    installed: bool,
    #[arg(long = "no-subpages", help = "Skip extension subpages")]
    no_subpages: bool,
}

#[derive(Debug, Args)]
struct DocsImportTechnicalArgs {
    #[arg(value_name = "PAGE")]
    pages: Vec<String>,
    #[arg(long, help = "Include subpages for selected pages/types")]
    subpages: bool,
    #[arg(long, help = "Import all hook documentation")]
    hooks: bool,
    #[arg(long, help = "Import configuration variable docs")]
    config: bool,
    #[arg(long, help = "Import API documentation")]
    api: bool,
    #[arg(
        short = 'l',
        long,
        default_value_t = 100,
        help = "Limit subpage imports per task"
    )]
    limit: usize,
}

#[derive(Debug, Args)]
struct DocsListArgs {
    #[arg(long, help = "Show only outdated docs")]
    outdated: bool,
    #[arg(long, value_name = "TYPE", help = "Filter technical docs by type")]
    r#type: Option<String>,
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
struct ContractsArgs {
    #[command(subcommand)]
    command: ContractsCommand,
}

#[derive(Debug, Subcommand)]
enum ContractsCommand {
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
        Some(Commands::Contracts(contracts)) => run_contracts(contracts),
        Some(Commands::Pull(args)) => run_pull(&runtime, args),
        Some(Commands::Push(args)) => run_push(&runtime, args),
        Some(Commands::Diff(args)) => run_diff(&runtime, args),
        Some(Commands::Status(args)) => run_status(&runtime, args),
        Some(Commands::Context(ContextArgs { title })) => run_context(&runtime, &title),
        Some(Commands::Search(SearchArgs { query })) => run_search(&runtime, &query),
        Some(Commands::SearchExternal(SearchExternalArgs { query })) => {
            run_search_external(&runtime, &query)
        }
        Some(Commands::Validate) => run_validate(&runtime),
        Some(Commands::Lint(LintArgs { title })) => run_stub(
            &runtime,
            &match title {
                Some(title) => format!("lint {title}"),
                None => "lint".to_string(),
            },
        ),
        Some(Commands::Fetch(FetchArgs { url })) => run_stub(&runtime, &format!("fetch {url}")),
        Some(Commands::Export(ExportArgs { url })) => run_stub(&runtime, &format!("export {url}")),
        Some(Commands::Delete(args)) => run_delete(&runtime, args),
        Some(Commands::Db(DbArgs { command })) => match command {
            DbSubcommand::Stats => run_db_stats(&runtime),
            DbSubcommand::Sync => run_db_sync(&runtime),
            DbSubcommand::Migrate => unreachable!(),
        },
        Some(Commands::Docs(DocsArgs { command })) => match command {
            DocsSubcommand::Import(args) => run_docs_import(&runtime, args),
            DocsSubcommand::ImportTechnical(args) => run_docs_import_technical(&runtime, args),
            DocsSubcommand::List(args) => run_docs_list(&runtime, args),
            DocsSubcommand::Update => run_docs_update(&runtime),
            DocsSubcommand::Remove { target } => run_docs_remove(&runtime, &target),
            DocsSubcommand::Search { query, tier, limit } => {
                run_docs_search(&runtime, &query, tier.as_deref(), limit)
            }
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

fn run_pull(runtime: &RuntimeOptions, args: PullArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    let namespaces = pull_namespaces_from_args(&args);
    let report = pull_from_remote(
        &paths,
        &PullOptions {
            namespaces: namespaces.clone(),
            category: args.category.clone(),
            full: args.full,
            overwrite_local: args.overwrite_local,
        },
    )?;

    println!("pull");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("full: {}", args.full);
    println!("overwrite_local: {}", args.overwrite_local);
    println!("category: {}", args.category.as_deref().unwrap_or("<none>"));
    println!("templates: {}", args.templates);
    println!("categories: {}", args.categories);
    println!("all: {}", args.all);
    println!(
        "namespaces: {}",
        namespaces
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    println!("pull.request_count: {}", report.request_count);
    println!("pull.requested_pages: {}", report.requested_pages);
    println!("pull.pulled: {}", report.pulled);
    println!("pull.created: {}", report.created);
    println!("pull.updated: {}", report.updated);
    println!("pull.skipped: {}", report.skipped);
    println!("pull.errors.count: {}", report.errors.len());
    for page in &report.pages {
        println!(
            "pull.page: title={} action={} detail={}",
            page.title,
            page.action,
            page.detail.as_deref().unwrap_or("<none>")
        );
    }
    if !report.errors.is_empty() {
        for error in &report.errors {
            println!("pull.error: {error}");
        }
    }
    if let Some(reindex) = &report.reindex {
        println!("pull.reindex.inserted_rows: {}", reindex.inserted_rows);
        println!("pull.reindex.inserted_links: {}", reindex.inserted_links);
        print_scan_stats("pull.reindex.scan", &reindex.scan);
    } else {
        println!("pull.reindex: skipped (no local writes)");
    }

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

    if report.success {
        Ok(())
    } else {
        bail!("pull completed with {} error(s)", report.errors.len())
    }
}

fn run_push(runtime: &RuntimeOptions, args: PushArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    let summary = args
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "wikitool rust push".to_string());

    let report = push_to_remote(
        &paths,
        &PushOptions {
            summary: summary.clone(),
            dry_run: args.dry_run,
            force: args.force,
            delete: args.delete,
            include_templates: args.templates,
            categories_only: args.categories,
        },
    )?;

    println!("push");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("summary: {summary}");
    println!("dry_run: {}", args.dry_run);
    println!("force: {}", args.force);
    println!("delete: {}", args.delete);
    println!("templates: {}", args.templates);
    println!("categories: {}", args.categories);
    println!("push.request_count: {}", report.request_count);
    println!("push.pushed: {}", report.pushed);
    println!("push.created: {}", report.created);
    println!("push.updated: {}", report.updated);
    println!("push.deleted: {}", report.deleted);
    println!("push.unchanged: {}", report.unchanged);
    println!("push.conflicts.count: {}", report.conflicts.len());
    println!("push.errors.count: {}", report.errors.len());
    if report.pages.is_empty() {
        println!("push.pages: <none>");
    } else {
        for page in &report.pages {
            println!(
                "push.page: title={} action={} detail={}",
                page.title,
                page.action,
                page.detail.as_deref().unwrap_or("<none>")
            );
        }
    }
    for title in &report.conflicts {
        println!("push.conflict: {title}");
    }
    for error in &report.errors {
        println!("push.error: {error}");
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.success {
        Ok(())
    } else if !report.conflicts.is_empty() && !args.force {
        bail!(
            "push blocked by {} conflict(s); rerun with --force after review",
            report.conflicts.len()
        )
    } else {
        bail!("push completed with {} error(s)", report.errors.len())
    }
}

fn run_diff(runtime: &RuntimeOptions, args: DiffArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    println!("diff");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("templates: {}", args.templates);
    println!("verbose: {}", args.verbose);

    let report = match diff_local_against_sync(
        &paths,
        &DiffOptions {
            include_templates: args.templates,
        },
    )? {
        Some(report) => report,
        None => {
            println!(
                "diff.sync_ledger: <not built> (run `wikitool pull --full{}`)",
                if args.templates { " --templates" } else { "" }
            );
            println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
            if runtime.diagnostics {
                println!("\n[diagnostics]\n{}", paths.diagnostics());
            }
            return Ok(());
        }
    };

    println!("diff.new_local: {}", report.new_local);
    println!("diff.modified_local: {}", report.modified_local);
    println!("diff.deleted_local: {}", report.deleted_local);
    println!("diff.total: {}", report.changes.len());

    if report.changes.is_empty() {
        println!("diff.changes: <none>");
    } else {
        for change in &report.changes {
            println!(
                "diff.change: type={} title={} path={}",
                format_diff_change_type(&change.change_type),
                change.title,
                change.relative_path
            );
            if args.verbose {
                println!(
                    "diff.change.hashes: local={} synced={}",
                    change.local_hash.as_deref().unwrap_or("<none>"),
                    change.synced_hash.as_deref().unwrap_or("<none>")
                );
                println!(
                    "diff.change.synced_wiki_timestamp: {}",
                    change.synced_wiki_timestamp.as_deref().unwrap_or("<none>")
                );
            }
        }
    }

    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_search_external(runtime: &RuntimeOptions, query: &str) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let query = normalize_title_query(query);
    if query.is_empty() {
        bail!("search-external requires a non-empty query");
    }

    let namespaces = [NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
    let hits = search_external_wiki(&query, &namespaces, 20)?;

    println!("search-external");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("query: {query}");
    print_external_search_hits("search_external", &hits);
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn pull_namespaces_from_args(args: &PullArgs) -> Vec<i32> {
    if args.templates {
        return vec![NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
    }
    if args.categories {
        return vec![NS_CATEGORY];
    }
    if args.all {
        return vec![NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
    }
    vec![NS_MAIN]
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

fn run_search(runtime: &RuntimeOptions, query: &str) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let query = normalize_title_query(query);
    if query.is_empty() {
        bail!("search requires a non-empty query");
    }

    println!("search");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("query: {query}");
    match query_search_local(&paths, &query, 20)? {
        Some(results) => {
            println!("search.backend: indexed");
            print_search_hits("search", &results);
        }
        None => {
            println!("search.backend: fallback-filesystem");
            println!("index.storage: <not built> (run `wikitool index rebuild` for faster search)");
            let mut results = scan_files(&paths, &ScanOptions::default())?
                .into_iter()
                .filter(|file| {
                    file.title
                        .to_ascii_lowercase()
                        .contains(&query.to_ascii_lowercase())
                })
                .map(|file| LocalSearchHit {
                    title: file.title,
                    namespace: file.namespace,
                    is_redirect: file.is_redirect,
                })
                .collect::<Vec<_>>();
            results.sort_by(|left, right| left.title.cmp(&right.title));
            results.truncate(20);
            print_search_hits("search", &results);
        }
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn run_context(runtime: &RuntimeOptions, title: &str) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let title = normalize_title_query(title);
    if title.is_empty() {
        bail!("context requires a non-empty title");
    }

    println!("context");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("title: {title}");
    if let Some(bundle) = build_local_context(&paths, &title)? {
        println!("context.backend: indexed");
        print_context_bundle("context", &bundle);
    } else {
        let has_index = load_stored_index_stats(&paths)?.is_some();
        if let Some(bundle) = build_context_from_scan(&paths, &title)? {
            println!("context.backend: fallback-filesystem");
            if !has_index {
                println!(
                    "index.storage: <not built> (run `wikitool index rebuild` for richer context)"
                );
            }
            print_context_bundle("context", &bundle);
        } else if has_index {
            bail!("page not found in local index: {title}");
        } else {
            bail!(
                "local index is not built and page was not found by filesystem scan: {title}\nRun `wikitool index rebuild` after `wikitool pull`."
            );
        }
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn run_validate(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("validate");
    println!("project_root: {}", normalize_path(&paths.project_root));
    let report = match run_validation_checks(&paths)? {
        Some(report) => report,
        None => {
            println!("index.storage: <not built> (run `wikitool index rebuild`)");
            println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
            if runtime.diagnostics {
                println!("\n[diagnostics]\n{}", paths.diagnostics());
            }
            bail!("validate requires a built local index");
        }
    };

    print_validation_issues(&report);
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    let issue_count = report.broken_links.len()
        + report.double_redirects.len()
        + report.uncategorized_pages.len()
        + report.orphan_pages.len();
    if issue_count == 0 {
        println!("validate.status: clean");
        Ok(())
    } else {
        println!("validate.status: failed");
        bail!("validation detected {issue_count} issue(s)")
    }
}

fn run_delete(runtime: &RuntimeOptions, args: DeleteArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    println!("delete");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("title: {}", args.title);
    println!("reason: {}", args.reason);
    println!("dry_run: {}", args.dry_run);
    println!("backup_enabled: {}", !args.no_backup);
    if let Some(backup_dir) = &args.backup_dir {
        println!("backup_dir: {}", normalize_path(backup_dir));
    }

    let report = delete_local_page(
        &paths,
        &args.title,
        &LocalDeleteOptions {
            reason: args.reason,
            no_backup: args.no_backup,
            backup_dir: args.backup_dir,
            dry_run: args.dry_run,
        },
    )?;
    print_delete_report(&report);

    println!("remote_delete: pending_not_implemented");
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

fn build_context_from_scan(
    paths: &wikitool_core::runtime::ResolvedPaths,
    title: &str,
) -> Result<Option<LocalContextBundle>> {
    let normalized = normalize_title_query(title);
    let files = scan_files(paths, &ScanOptions::default())?;
    let file = match files
        .into_iter()
        .find(|item| item.title.eq_ignore_ascii_case(&normalized))
    {
        Some(file) => file,
        None => return Ok(None),
    };

    let mut absolute = paths.project_root.clone();
    for segment in file.relative_path.split('/') {
        if !segment.is_empty() {
            absolute.push(segment);
        }
    }
    let content = std::fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read {}", normalize_path(&absolute)))?;
    let content_preview = collapse_whitespace(&content)
        .chars()
        .take(280)
        .collect::<String>();

    Ok(Some(LocalContextBundle {
        title: file.title,
        namespace: file.namespace,
        is_redirect: file.is_redirect,
        redirect_target: file.redirect_target,
        relative_path: file.relative_path,
        bytes: file.bytes,
        word_count: content
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .count(),
        content_preview: if content_preview.is_empty() {
            String::new()
        } else {
            format!("{content_preview}...")
        },
        sections: Vec::new(),
        outgoing_links: Vec::new(),
        backlinks: Vec::new(),
        categories: Vec::new(),
        templates: Vec::new(),
        modules: Vec::new(),
    }))
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

fn run_db_sync(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    let report = rebuild_index(&paths, &ScanOptions::default())?;

    println!("db sync");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("db_path: {}", normalize_path(&paths.db_path));
    println!("synced_rows: {}", report.inserted_rows);
    println!("synced_links: {}", report.inserted_links);
    print_scan_stats("scan", &report.scan);
    println!("migrations: disabled");
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn run_docs_import(runtime: &RuntimeOptions, args: DocsImportArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    if let Some(bundle_path) = args.bundle.as_deref() {
        if args.installed || !args.extensions.is_empty() || args.no_subpages {
            bail!(
                "`docs import --bundle` cannot be combined with extensions, --installed, or --no-subpages"
            );
        }
        let report = import_docs_bundle(&paths, bundle_path)?;

        println!("docs import");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("source: {}", report.source);
        println!("bundle_path: {}", normalize_path(bundle_path));
        println!("bundle.schema_version: {}", report.schema_version);
        println!("imported_extensions: {}", report.imported_extensions);
        println!(
            "imported_technical_types: {}",
            report.imported_technical_types
        );
        println!("imported_pages: {}", report.imported_pages);
        println!("failures.count: {}", report.failures.len());
        for failure in &report.failures {
            println!("failure: {failure}");
        }
        println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }

        if report.imported_pages == 0 {
            bail!("docs import bundle completed with no imported pages")
        }
        return Ok(());
    }

    let mut extensions = args.extensions;
    if args.installed {
        let discovered = discover_installed_extensions_from_wiki()
            .context("failed to discover installed extensions from live wiki API")?;
        extensions.extend(discovered);
    }

    let mut normalized = extensions
        .into_iter()
        .map(|value| normalize_title_query(&value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort_unstable_by_key(|value| value.to_ascii_lowercase());
    normalized.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    if normalized.is_empty() {
        bail!(
            "no extensions specified. Use `docs import <Extension>` or `docs import --installed`"
        );
    }

    let report = import_extension_docs(
        &paths,
        &DocsImportOptions {
            extensions: normalized.clone(),
            include_subpages: !args.no_subpages,
        },
    )?;

    println!("docs import");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("extensions.requested: {}", normalized.len());
    println!(
        "extensions.installed_discovery: {}",
        format_flag(args.installed)
    );
    println!("subpages: {}", format_flag(!args.no_subpages));
    println!("imported_extensions: {}", report.imported_extensions);
    println!("imported_pages: {}", report.imported_pages);
    println!("request_count: {}", report.request_count);
    println!("failures.count: {}", report.failures.len());
    for failure in &report.failures {
        println!("failure: {failure}");
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.imported_extensions == 0 {
        bail!("docs import completed with no imported extensions")
    }
    Ok(())
}

fn run_docs_import_technical(
    runtime: &RuntimeOptions,
    args: DocsImportTechnicalArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    let mut tasks = Vec::new();
    for page in args.pages {
        let normalized = normalize_title_query(&page);
        if normalized.is_empty() {
            continue;
        }
        tasks.push(TechnicalImportTask {
            doc_type: infer_doc_type_from_title(&normalized),
            page_title: Some(normalized),
            include_subpages: args.subpages,
        });
    }
    if args.hooks {
        tasks.push(TechnicalImportTask {
            doc_type: TechnicalDocType::Hooks,
            page_title: None,
            include_subpages: true,
        });
    }
    if args.config {
        tasks.push(TechnicalImportTask {
            doc_type: TechnicalDocType::Config,
            page_title: None,
            include_subpages: true,
        });
    }
    if args.api {
        tasks.push(TechnicalImportTask {
            doc_type: TechnicalDocType::Api,
            page_title: None,
            include_subpages: true,
        });
    }
    if tasks.is_empty() {
        bail!(
            "no technical documentation specified. Use `docs import-technical <Page> [--subpages]` or flags: --hooks --config --api"
        );
    }

    let report = import_technical_docs(
        &paths,
        &DocsImportTechnicalOptions {
            tasks,
            limit: args.limit.max(1),
        },
    )?;

    println!("docs import-technical");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("tasks.requested: {}", report.requested_tasks);
    println!("limit: {}", args.limit.max(1));
    println!("imported_pages: {}", report.imported_pages);
    println!("request_count: {}", report.request_count);
    if report.imported_by_type.is_empty() {
        println!("imported_by_type: <none>");
    } else {
        for (doc_type, count) in &report.imported_by_type {
            println!("imported_by_type.{doc_type}: {count}");
        }
    }
    println!("failures.count: {}", report.failures.len());
    for failure in &report.failures {
        println!("failure: {failure}");
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.imported_pages == 0 {
        bail!("docs import-technical completed with no imported pages")
    }
    Ok(())
}

fn run_docs_list(runtime: &RuntimeOptions, args: DocsListArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let listing = list_docs(
        &paths,
        &DocsListOptions {
            technical_type: args.r#type.clone(),
        },
    )?;

    println!("docs list");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("stats.extension_count: {}", listing.stats.extension_count);
    println!(
        "stats.extension_pages_count: {}",
        listing.stats.extension_pages_count
    );
    println!("stats.technical_count: {}", listing.stats.technical_count);
    if listing.stats.technical_by_type.is_empty() {
        println!("stats.technical_by_type: <none>");
    } else {
        for (doc_type, count) in &listing.stats.technical_by_type {
            println!("stats.technical_by_type.{doc_type}: {count}");
        }
    }

    if args.outdated {
        println!(
            "outdated.extensions.count: {}",
            listing.outdated.extensions.len()
        );
        for extension in &listing.outdated.extensions {
            println!(
                "outdated.extension: {} ({})",
                extension.extension_name,
                format_expiration(listing.now_unix, extension.expires_at_unix)
            );
        }
        println!(
            "outdated.technical.count: {}",
            listing.outdated.technical.len()
        );
        for doc in &listing.outdated.technical {
            println!(
                "outdated.technical: [{}] {} ({})",
                doc.doc_type,
                doc.page_title,
                format_expiration(listing.now_unix, doc.expires_at_unix)
            );
        }
        println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
        return Ok(());
    }

    println!("extensions.count: {}", listing.extensions.len());
    for extension in &listing.extensions {
        println!(
            "extension: {} version={} pages={} status={}",
            extension.extension_name,
            extension.version.as_deref().unwrap_or("<none>"),
            extension.pages_count,
            format_expiration(listing.now_unix, extension.expires_at_unix)
        );
    }

    println!("technical.count: {}", listing.technical.len());
    for doc in &listing.technical {
        println!(
            "technical: [{}] {} status={}",
            doc.doc_type,
            doc.page_title,
            format_expiration(listing.now_unix, doc.expires_at_unix)
        );
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_docs_update(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let report = update_outdated_docs(&paths)?;

    println!("docs update");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("updated_extensions: {}", report.updated_extensions);
    println!(
        "updated_technical_types: {}",
        report.updated_technical_types
    );
    println!("updated_pages: {}", report.updated_pages);
    println!("request_count: {}", report.request_count);
    println!("failures.count: {}", report.failures.len());
    for failure in &report.failures {
        println!("failure: {failure}");
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_docs_remove(runtime: &RuntimeOptions, target: &str) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let report = remove_docs(&paths, target)?;

    println!("docs remove");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("target: {}", report.target);
    println!("kind: {:?}", report.kind);
    println!("removed_rows: {}", report.removed_rows);
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if matches!(report.kind, DocsRemoveKind::NotFound) {
        bail!("documentation target not found: {target}");
    }
    Ok(())
}

fn run_docs_search(
    runtime: &RuntimeOptions,
    query: &str,
    tier: Option<&str>,
    limit: usize,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let hits = search_docs(&paths, query, tier, limit)?;

    println!("docs search");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("query: {}", collapse_whitespace(query));
    println!("tier: {}", tier.unwrap_or("<all>"));
    println!("limit: {limit}");
    println!("hits.count: {}", hits.len());
    if hits.is_empty() {
        println!("hits: <none>");
    } else {
        for hit in &hits {
            println!("hit: [{}] {}", hit.tier, hit.title);
            println!("hit.snippet: {}", hit.snippet);
        }
    }
    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn infer_doc_type_from_title(title: &str) -> TechnicalDocType {
    if title.starts_with("Manual:Hooks") {
        return TechnicalDocType::Hooks;
    }
    if title.starts_with("Manual:$wg") {
        return TechnicalDocType::Config;
    }
    if title.starts_with("API:") {
        return TechnicalDocType::Api;
    }
    TechnicalDocType::Manual
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

fn print_search_hits(prefix: &str, hits: &[LocalSearchHit]) {
    println!("{prefix}.count: {}", hits.len());
    if hits.is_empty() {
        println!("{prefix}.hits: <none>");
        return;
    }
    for hit in hits {
        println!(
            "{prefix}.hit: {} (namespace={}, redirect={})",
            hit.title,
            hit.namespace,
            if hit.is_redirect { "yes" } else { "no" }
        );
    }
}

fn print_external_search_hits(prefix: &str, hits: &[ExternalSearchHit]) {
    println!("{prefix}.count: {}", hits.len());
    if hits.is_empty() {
        println!("{prefix}.hits: <none>");
        return;
    }
    for hit in hits {
        println!(
            "{prefix}.hit: {} (namespace={}, page_id={})",
            hit.title, hit.namespace, hit.page_id
        );
        println!(
            "{prefix}.hit.word_count: {}",
            hit.word_count
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<none>".to_string())
        );
        println!(
            "{prefix}.hit.timestamp: {}",
            hit.timestamp.as_deref().unwrap_or("<none>")
        );
        println!(
            "{prefix}.hit.snippet: {}",
            if hit.snippet.trim().is_empty() {
                "<none>"
            } else {
                &hit.snippet
            }
        );
    }
}

fn print_context_bundle(prefix: &str, bundle: &LocalContextBundle) {
    println!("{prefix}.title: {}", bundle.title);
    println!("{prefix}.namespace: {}", bundle.namespace);
    println!("{prefix}.relative_path: {}", bundle.relative_path);
    println!("{prefix}.bytes: {}", bundle.bytes);
    println!("{prefix}.word_count: {}", bundle.word_count);
    println!(
        "{prefix}.is_redirect: {}",
        if bundle.is_redirect { "yes" } else { "no" }
    );
    println!(
        "{prefix}.redirect_target: {}",
        bundle.redirect_target.as_deref().unwrap_or("<none>")
    );
    println!(
        "{prefix}.content_preview: {}",
        if bundle.content_preview.is_empty() {
            "<empty>"
        } else {
            &bundle.content_preview
        }
    );
    println!("{prefix}.sections.count: {}", bundle.sections.len());
    for section in &bundle.sections {
        println!(
            "{prefix}.section: level={} heading={}",
            section.level, section.heading
        );
    }
    print_string_list(&format!("{prefix}.outgoing_links"), &bundle.outgoing_links);
    print_string_list(&format!("{prefix}.backlinks"), &bundle.backlinks);
    print_string_list(&format!("{prefix}.categories"), &bundle.categories);
    print_string_list(&format!("{prefix}.templates"), &bundle.templates);
    print_string_list(&format!("{prefix}.modules"), &bundle.modules);
}

fn print_validation_issues(report: &wikitool_core::index::ValidationReport) {
    println!("validate.broken_links.count: {}", report.broken_links.len());
    if report.broken_links.is_empty() {
        println!("validate.broken_links: <none>");
    } else {
        for issue in &report.broken_links {
            println!(
                "validate.broken_links.issue: source={} target={}",
                issue.source_title, issue.target_title
            );
        }
    }

    println!(
        "validate.double_redirects.count: {}",
        report.double_redirects.len()
    );
    if report.double_redirects.is_empty() {
        println!("validate.double_redirects: <none>");
    } else {
        for issue in &report.double_redirects {
            println!(
                "validate.double_redirects.issue: title={} first_target={} final_target={}",
                issue.title, issue.first_target, issue.final_target
            );
        }
    }

    print_string_list("validate.uncategorized_pages", &report.uncategorized_pages);
    print_string_list("validate.orphan_pages", &report.orphan_pages);
}

fn print_delete_report(report: &DeleteReport) {
    println!("delete.result.title: {}", report.title);
    println!("delete.result.reason: {}", report.reason);
    println!("delete.result.relative_path: {}", report.relative_path);
    println!("delete.result.dry_run: {}", report.dry_run);
    println!(
        "delete.result.deleted_local_file: {}",
        report.deleted_local_file
    );
    println!(
        "delete.result.deleted_index_rows: {}",
        report.deleted_index_rows
    );
    println!(
        "delete.result.backup_path: {}",
        report.backup_path.as_deref().unwrap_or("<none>")
    );
}

fn print_string_list(prefix: &str, values: &[String]) {
    println!("{prefix}.count: {}", values.len());
    if values.is_empty() {
        println!("{prefix}: <none>");
        return;
    }
    for value in values {
        println!("{prefix}.item: {value}");
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

fn run_contracts(args: ContractsArgs) -> Result<()> {
    match args.command {
        ContractsCommand::Snapshot(snapshot) => {
            let report = generate_fixture_snapshot(
                &snapshot.project_root,
                &snapshot.content_dir,
                &snapshot.templates_dir,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        ContractsCommand::CommandSurface => {
            println!("{}", serde_json::to_string_pretty(&command_surface())?);
        }
    }
    Ok(())
}

fn resolve_runtime_paths(
    runtime: &RuntimeOptions,
) -> Result<wikitool_core::runtime::ResolvedPaths> {
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

fn normalize_title_query(value: &str) -> String {
    value.replace('_', " ").trim().to_string()
}

fn collapse_whitespace(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
                previous_was_space = true;
            }
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }
    output.trim().to_string()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn format_flag(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn format_diff_change_type(value: &DiffChangeType) -> &'static str {
    match value {
        DiffChangeType::NewLocal => "new_local",
        DiffChangeType::ModifiedLocal => "modified_local",
        DiffChangeType::DeletedLocal => "deleted_local",
    }
}
