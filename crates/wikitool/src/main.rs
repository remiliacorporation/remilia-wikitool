use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Args, Command, CommandFactory, Parser, Subcommand, error::ErrorKind};
use wikitool_core::contracts::{command_surface, generate_fixture_snapshot};
use wikitool_core::delete::{DeleteOptions as LocalDeleteOptions, DeleteReport, delete_local_page};
use wikitool_core::docs::{
    DocsImportOptions, DocsImportTechnicalOptions, DocsListOptions, DocsRemoveKind,
    TechnicalDocType, TechnicalImportTask, discover_installed_extensions_from_wiki,
    format_expiration, import_docs_bundle, import_extension_docs, import_technical_docs, list_docs,
    remove_docs, search_docs, update_outdated_docs,
};
use wikitool_core::external::{
    ExportFormat, ExternalFetchFormat, ExternalFetchOptions, default_export_path,
    fetch_page_by_url, fetch_pages_by_titles, generate_frontmatter, list_subpages, parse_wiki_url,
    sanitize_filename, wikitext_to_markdown,
};
use wikitool_core::filesystem::{ScanOptions, ScanStats, scan_files, scan_stats};
use wikitool_core::import_cargo::{
    CargoImportOptions, ImportSourceType, ImportUpdateMode, import_to_cargo,
};
use wikitool_core::index::{
    LocalContextBundle, LocalSearchHit, StoredIndexStats, build_local_context,
    load_stored_index_stats, query_backlinks, query_empty_categories, query_orphans,
    query_search_local, rebuild_index, run_validation_checks,
};
use wikitool_core::inspect::{
    LighthouseOutputFormat, LighthouseRunOptions, NetInspectOptions, find_lighthouse_binary,
    lighthouse_version, net_inspect, run_lighthouse, seo_inspect,
};
use wikitool_core::lint::lint_modules;
use wikitool_core::runtime::{
    InitOptions, NO_MIGRATIONS_POLICY_MESSAGE, PathOverrides, ResolutionContext,
    embedded_parser_config, ensure_runtime_ready_for_sync, init_layout, inspect_runtime,
    lsp_settings_json, materialize_parser_config, resolve_paths,
};
use wikitool_core::sync::{
    DiffChangeType, DiffOptions, ExternalSearchHit, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE,
    NS_TEMPLATE, PullOptions, PushOptions, RemoteDeleteStatus, delete_remote_page,
    diff_local_against_sync, pull_from_remote, push_to_remote, search_external_wiki,
};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

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
    Workflow(WorkflowArgs),
    Release(ReleaseArgs),
    Dev(DevArgs),
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
    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: String,
    #[arg(long, help = "Treat warnings as errors")]
    strict: bool,
    #[arg(long, help = "Omit metadata from JSON output")]
    no_meta: bool,
}

#[derive(Debug, Args)]
struct FetchArgs {
    url: String,
    #[arg(
        long,
        default_value = "wikitext",
        value_name = "FORMAT",
        help = "Output format: wikitext|html"
    )]
    format: String,
    #[arg(long, help = "Save output under reference/<source>/ in project root")]
    save: bool,
    #[arg(
        long,
        value_name = "NAME",
        help = "Custom name for saved reference file"
    )]
    name: Option<String>,
}

#[derive(Debug, Args)]
struct ExportArgs {
    url: String,
    #[arg(
        short = 'o',
        long,
        value_name = "PATH",
        help = "Output file or directory path"
    )]
    output: Option<PathBuf>,
    #[arg(
        long,
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format: markdown|wikitext"
    )]
    format: String,
    #[arg(
        long,
        value_name = "LANG",
        help = "Code language hint (reserved for markdown export)"
    )]
    code_language: Option<String>,
    #[arg(long, help = "Skip YAML frontmatter")]
    no_frontmatter: bool,
    #[arg(long, help = "Include subpages for MediaWiki page exports")]
    subpages: bool,
    #[arg(long, help = "With --subpages, combine all pages into one output")]
    combined: bool,
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
    #[command(name = "generate-reference")]
    GenerateReference(DocsGenerateReferenceArgs),
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
struct DocsGenerateReferenceArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Output markdown path (default: docs/wikitool/reference.md in current directory)"
    )]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct SeoArgs {
    #[command(subcommand)]
    command: SeoSubcommand,
}

#[derive(Debug, Subcommand)]
enum SeoSubcommand {
    Inspect {
        target: String,
        #[arg(long, help = "Output JSON for AI consumption")]
        json: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
        #[arg(long, value_name = "URL", help = "Override target URL")]
        url: Option<String>,
    },
}

#[derive(Debug, Args)]
struct NetArgs {
    #[command(subcommand)]
    command: NetSubcommand,
}

#[derive(Debug, Subcommand)]
enum NetSubcommand {
    Inspect {
        target: String,
        #[arg(
            long,
            default_value_t = 25,
            value_name = "N",
            help = "Limit number of resources to probe"
        )]
        limit: usize,
        #[arg(long, help = "Skip HEAD probes (faster, no size/cache info)")]
        no_probe: bool,
        #[arg(long, help = "Output JSON for AI consumption")]
        json: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
        #[arg(long, value_name = "URL", help = "Override target URL")]
        url: Option<String>,
    },
}

#[derive(Debug, Args)]
struct PerfArgs {
    #[command(subcommand)]
    command: PerfSubcommand,
}

#[derive(Debug, Subcommand)]
enum PerfSubcommand {
    Lighthouse {
        target: Option<String>,
        #[arg(
            long,
            default_value = "html",
            value_name = "FORMAT",
            help = "Output format: html|json"
        )]
        output: String,
        #[arg(long, value_name = "PATH", help = "Report output path")]
        out: Option<PathBuf>,
        #[arg(long, value_name = "LIST", help = "Comma-separated categories")]
        categories: Option<String>,
        #[arg(long, value_name = "FLAGS", help = "Pass Chrome flags to Lighthouse")]
        chrome_flags: Option<String>,
        #[arg(long, help = "Print resolved Lighthouse binary + version and exit")]
        show_version: bool,
        #[arg(long, help = "Output JSON summary")]
        json: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
        #[arg(long, value_name = "URL", help = "Override target URL")]
        url: Option<String>,
    },
}

#[derive(Debug, Args)]
struct ImportArgs {
    #[command(subcommand)]
    command: ImportSubcommand,
}

#[derive(Debug, Subcommand)]
enum ImportSubcommand {
    Cargo {
        path: String,
        #[arg(long, value_name = "NAME", help = "Cargo table name")]
        table: String,
        #[arg(long, value_name = "TYPE", help = "Input type: csv|json")]
        r#type: Option<String>,
        #[arg(long, value_name = "NAME", help = "Template wrapper name")]
        template: Option<String>,
        #[arg(long, value_name = "FIELD", help = "Field name to use as page title")]
        title_field: Option<String>,
        #[arg(long, value_name = "PREFIX", help = "Prefix for generated page titles")]
        title_prefix: Option<String>,
        #[arg(long, value_name = "NAME", help = "Category to add to generated pages")]
        category: Option<String>,
        #[arg(
            long,
            default_value = "create",
            value_name = "MODE",
            help = "create|update|upsert"
        )]
        mode: String,
        #[arg(long, help = "Write files (default: dry-run)")]
        write: bool,
        #[arg(
            long,
            default_value = "text",
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: String,
        #[arg(
            long,
            help = "Add SHORTDESC + Article quality header in main namespace"
        )]
        article_header: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
    },
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
struct WorkflowArgs {
    #[command(subcommand)]
    command: WorkflowSubcommand,
}

#[derive(Debug, Subcommand)]
enum WorkflowSubcommand {
    Bootstrap(WorkflowBootstrapArgs),
    #[command(name = "full-refresh")]
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
struct ReleaseArgs {
    #[command(subcommand)]
    command: ReleaseSubcommand,
}

#[derive(Debug, Subcommand)]
enum ReleaseSubcommand {
    #[command(name = "build-ai-pack")]
    BuildAiPack(ReleaseBuildAiPackArgs),
    Package(ReleasePackageArgs),
    #[command(name = "build-matrix")]
    BuildMatrix(ReleaseBuildMatrixArgs),
}

#[derive(Debug, Args)]
struct ReleaseBuildAiPackArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Wikitool repository root (default: current directory)"
    )]
    repo_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Output directory (default: <repo>/dist/ai-pack)"
    )]
    output_dir: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional host project root containing CLAUDE.md + .claude/{rules,skills}"
    )]
    host_project_root: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ReleasePackageArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Wikitool repository root (default: current directory)"
    )]
    repo_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Release binary path (default: <repo>/target/release/wikitool[.exe])"
    )]
    binary_path: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Output directory (default: <repo>/dist/release)"
    )]
    output_dir: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional host project root containing CLAUDE.md + .claude/{rules,skills}"
    )]
    host_project_root: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ReleaseBuildMatrixArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Wikitool repository root (default: current directory)"
    )]
    repo_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "TRIPLE",
        value_delimiter = ',',
        help = "Target triples to build (repeat or use comma-separated list). Defaults to windows/linux/macos x86_64 targets."
    )]
    targets: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Output directory for staged folders and zip artifacts (default: <repo>/dist/release-matrix)"
    )]
    output_dir: Option<PathBuf>,
    #[arg(
        long,
        value_name = "LABEL",
        help = "Version label used in bundle names (default: v<CARGO_PKG_VERSION>)"
    )]
    artifact_version: Option<String>,
    #[arg(
        long,
        help = "Use unversioned bundle names (wikitool-<target>) for CI/ephemeral artifacts"
    )]
    unversioned_names: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Cargo executable path used for target builds (default: cargo)"
    )]
    cargo_bin: Option<PathBuf>,
    #[arg(long, help = "Skip cargo build and package existing target binaries")]
    skip_build: bool,
    #[arg(long, help = "Use cargo --locked for target builds (default: true)")]
    locked: bool,
    #[arg(long, help = "Do not pass --locked to cargo builds")]
    no_locked: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional host project root containing CLAUDE.md + .claude/{rules,skills}"
    )]
    host_project_root: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct DevArgs {
    #[command(subcommand)]
    command: DevSubcommand,
}

#[derive(Debug, Subcommand)]
enum DevSubcommand {
    #[command(name = "install-git-hooks")]
    InstallGitHooks(InstallGitHooksArgs),
}

#[derive(Debug, Args)]
struct InstallGitHooksArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Repository root containing .git/hooks (default: current directory)"
    )]
    repo_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Hook source file (default: scripts/git-hooks/commit-msg under repo root)"
    )]
    source: Option<PathBuf>,
    #[arg(
        long,
        help = "Do not fail when .git/hooks is missing (useful for zip-distributed binaries)"
    )]
    allow_missing_git: bool,
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
        Some(Commands::Workflow(args)) => run_workflow(&runtime, args),
        Some(Commands::Release(args)) => run_release(args),
        Some(Commands::Dev(args)) => run_dev(args),
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
        Some(Commands::Lint(args)) => run_lint(&runtime, args),
        Some(Commands::Fetch(args)) => run_fetch(&runtime, args),
        Some(Commands::Export(args)) => run_export(&runtime, args),
        Some(Commands::Delete(args)) => run_delete(&runtime, args),
        Some(Commands::Db(DbArgs { command })) => match command {
            DbSubcommand::Stats => run_db_stats(&runtime),
            DbSubcommand::Sync => run_db_sync(&runtime),
            DbSubcommand::Migrate => unreachable!(),
        },
        Some(Commands::Docs(DocsArgs { command })) => match command {
            DocsSubcommand::Import(args) => run_docs_import(&runtime, args),
            DocsSubcommand::ImportTechnical(args) => run_docs_import_technical(&runtime, args),
            DocsSubcommand::GenerateReference(args) => run_docs_generate_reference(args),
            DocsSubcommand::List(args) => run_docs_list(&runtime, args),
            DocsSubcommand::Update => run_docs_update(&runtime),
            DocsSubcommand::Remove { target } => run_docs_remove(&runtime, &target),
            DocsSubcommand::Search { query, tier, limit } => {
                run_docs_search(&runtime, &query, tier.as_deref(), limit)
            }
        },
        Some(Commands::Seo(SeoArgs { command })) => match command {
            SeoSubcommand::Inspect {
                target,
                json,
                no_meta: _,
                url,
            } => run_seo_inspect(&runtime, &target, json, url.as_deref()),
        },
        Some(Commands::Net(NetArgs { command })) => match command {
            NetSubcommand::Inspect {
                target,
                limit,
                no_probe,
                json,
                no_meta: _,
                url,
            } => run_net_inspect(
                &runtime,
                &target,
                json,
                url.as_deref(),
                &NetInspectOptions {
                    limit: limit.max(1),
                    probe: !no_probe,
                },
            ),
        },
        Some(Commands::Perf(PerfArgs { command })) => match command {
            PerfSubcommand::Lighthouse {
                target,
                output,
                out,
                categories,
                chrome_flags,
                show_version,
                json,
                no_meta: _,
                url,
            } => run_perf_lighthouse(
                &runtime,
                target,
                output.as_str(),
                out.as_deref(),
                categories.as_deref(),
                chrome_flags.as_deref(),
                show_version,
                json,
                url.as_deref(),
            ),
        },
        Some(Commands::Import(ImportArgs { command })) => match command {
            ImportSubcommand::Cargo {
                path,
                table,
                r#type,
                template,
                title_field,
                title_prefix,
                category,
                mode,
                write,
                format,
                article_header,
                no_meta: _,
            } => run_import_cargo(
                &runtime,
                &path,
                &table,
                r#type.as_deref(),
                template.as_deref(),
                title_field.as_deref(),
                title_prefix.as_deref(),
                category.as_deref(),
                &mode,
                write,
                &format,
                article_header,
            ),
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

fn run_fetch(runtime: &RuntimeOptions, args: FetchArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let format = ExternalFetchFormat::parse(&args.format)?;
    let result = fetch_page_by_url(
        &args.url,
        &ExternalFetchOptions {
            format,
            max_bytes: 1_000_000,
        },
    )?
    .ok_or_else(|| anyhow::anyhow!("page not found: {}", args.url))?;

    println!("fetch");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("source_url: {}", args.url);
    println!("resolved_url: {}", result.url);
    println!("title: {}", result.title);
    println!("source_wiki: {}", result.source_wiki);
    println!("source_domain: {}", result.source_domain);
    println!("content_format: {}", result.content_format);
    println!("content_length: {}", result.content.len());

    if args.save {
        let safe_name = args
            .name
            .as_deref()
            .map(sanitize_filename)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                let fallback = sanitize_filename(&result.title);
                if fallback.is_empty() {
                    "external-page".to_string()
                } else {
                    fallback
                }
            });
        let relative_path = format!("reference/{}/{}.wiki", result.source_wiki, safe_name);
        let absolute_path = paths.project_root.join(relative_path.replace('/', "\\"));
        if let Some(parent) = absolute_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
        }
        std::fs::write(&absolute_path, result.content.as_bytes())
            .with_context(|| format!("failed to write {}", normalize_path(&absolute_path)))?;
        println!("saved: yes");
        println!("saved_path: {}", normalize_path(&absolute_path));
    } else {
        println!("saved: no");
        println!("content:");
        println!("{}", result.content);
    }

    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_export(runtime: &RuntimeOptions, args: ExportArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let export_format = ExportFormat::parse(&args.format)?;
    let fetch_options = ExternalFetchOptions {
        format: ExternalFetchFormat::Wikitext,
        max_bytes: 1_000_000,
    };

    if args.subpages {
        let parsed = parse_wiki_url(&args.url).ok_or_else(|| {
            anyhow::anyhow!("subpages export requires a recognizable MediaWiki URL")
        })?;
        let parent_title = parsed.title.trim_end_matches('/').to_string();
        let mut all_pages = Vec::new();

        if let Some(main_page) =
            fetch_mediawiki_export_page(&parent_title, &parsed, &fetch_options)?
        {
            all_pages.push(main_page);
        }

        let subpage_titles = list_subpages(&parent_title, &parsed, 500)?;
        let subpages = fetch_pages_by_titles(&subpage_titles, &parsed, &fetch_options)?;
        all_pages.extend(subpages);
        if all_pages.is_empty() {
            bail!("no pages found for export target: {}", args.url);
        }

        if args.combined {
            let combined = render_combined_export(
                &all_pages,
                export_format,
                !args.no_frontmatter,
                args.code_language.as_deref(),
                &parsed.domain,
                &args.url,
                &parent_title,
            );
            let output_path = args.output.clone().or_else(|| {
                default_export_path(&paths.project_root, &parent_title, false, export_format)
            });
            write_or_print_export(&combined, output_path.as_deref())?;

            println!("export");
            println!("mode: subpages_combined");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("source_url: {}", args.url);
            println!("pages_exported: {}", all_pages.len());
            println!("format: {}", args.format.to_ascii_lowercase());
            if let Some(path) = output_path {
                println!("output_path: {}", normalize_path(&path));
            } else {
                println!("output_path: <stdout>");
            }
        } else {
            let output_dir = args
                .output
                .clone()
                .or_else(|| {
                    default_export_path(&paths.project_root, &parent_title, true, export_format)
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("output directory is required for subpage export")
                })?;
            std::fs::create_dir_all(&output_dir)
                .with_context(|| format!("failed to create {}", normalize_path(&output_dir)))?;

            for page in &all_pages {
                let rendered = render_export_page(
                    page,
                    export_format,
                    !args.no_frontmatter,
                    args.code_language.as_deref(),
                    &parsed.domain,
                );
                let filename = format!(
                    "{}.{}",
                    sanitize_filename(&page.title),
                    export_format.file_extension()
                );
                let output_file = output_dir.join(filename);
                std::fs::write(&output_file, rendered.as_bytes())
                    .with_context(|| format!("failed to write {}", normalize_path(&output_file)))?;
            }

            let index_content = build_subpage_index(
                &all_pages,
                &parsed.domain,
                &args.url,
                &parent_title,
                export_format,
            );
            let index_path = output_dir.join("_index.md");
            std::fs::write(&index_path, index_content.as_bytes())
                .with_context(|| format!("failed to write {}", normalize_path(&index_path)))?;

            println!("export");
            println!("mode: subpages_separate");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("source_url: {}", args.url);
            println!("pages_exported: {}", all_pages.len());
            println!("format: {}", args.format.to_ascii_lowercase());
            println!("output_dir: {}", normalize_path(&output_dir));
            println!("index_path: {}", normalize_path(&index_path));
        }
    } else {
        let page = fetch_page_by_url(&args.url, &fetch_options)?
            .ok_or_else(|| anyhow::anyhow!("page not found: {}", args.url))?;
        let rendered = render_export_page(
            &page,
            export_format,
            !args.no_frontmatter,
            args.code_language.as_deref(),
            &page.source_domain,
        );
        let output_path = args.output.clone().or_else(|| {
            default_export_path(&paths.project_root, &page.title, false, export_format)
        });
        write_or_print_export(&rendered, output_path.as_deref())?;

        println!("export");
        println!("mode: single");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("source_url: {}", args.url);
        println!("resolved_url: {}", page.url);
        println!("title: {}", page.title);
        println!("format: {}", args.format.to_ascii_lowercase());
        println!("source_domain: {}", page.source_domain);
        println!("content_length: {}", page.content.len());
        if let Some(path) = output_path {
            println!("output_path: {}", normalize_path(&path));
        } else {
            println!("output_path: <stdout>");
        }
    }

    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn fetch_mediawiki_export_page(
    title: &str,
    parsed: &wikitool_core::external::ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<Option<wikitool_core::external::ExternalFetchResult>> {
    wikitool_core::external::fetch_mediawiki_page(title, parsed, options)
}

fn render_export_page(
    page: &wikitool_core::external::ExternalFetchResult,
    export_format: ExportFormat,
    include_frontmatter: bool,
    code_language: Option<&str>,
    domain: &str,
) -> String {
    let converted = match export_format {
        ExportFormat::Wikitext => page.content.clone(),
        ExportFormat::Markdown => wikitext_to_markdown(&page.content, code_language),
    };
    if !include_frontmatter {
        return converted;
    }
    let frontmatter = generate_frontmatter(
        &page.title,
        &page.url,
        domain,
        &page.timestamp,
        &[(
            "format".to_string(),
            export_format.file_extension().to_string(),
        )],
    );
    format!("{frontmatter}\n{converted}")
}

fn render_combined_export(
    pages: &[wikitool_core::external::ExternalFetchResult],
    export_format: ExportFormat,
    include_frontmatter: bool,
    code_language: Option<&str>,
    domain: &str,
    source_url: &str,
    title: &str,
) -> String {
    let mut sections = Vec::new();
    for page in pages {
        let converted = match export_format {
            ExportFormat::Wikitext => page.content.clone(),
            ExportFormat::Markdown => wikitext_to_markdown(&page.content, code_language),
        };
        let heading = match export_format {
            ExportFormat::Markdown => format!("# {}", page.title),
            ExportFormat::Wikitext => format!("== {} ==", page.title),
        };
        sections.push(format!("{heading}\n\n{converted}"));
    }
    let combined = sections.join("\n\n---\n\n");
    if !include_frontmatter {
        return combined;
    }
    let frontmatter = generate_frontmatter(
        title,
        source_url,
        domain,
        &now_timestamp_string(),
        &[("pages".to_string(), pages.len().to_string())],
    );
    format!("{frontmatter}\n{combined}")
}

fn build_subpage_index(
    pages: &[wikitool_core::external::ExternalFetchResult],
    domain: &str,
    source_url: &str,
    title: &str,
    export_format: ExportFormat,
) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("title: \"{} - Index\"", title.replace('"', "\\\"")),
        format!("source: {source_url}"),
        format!("wiki: {domain}"),
        format!("fetched: {}", now_timestamp_string()),
        format!("pages: {}", pages.len()),
        "---".to_string(),
        String::new(),
        format!("# {title}"),
        String::new(),
        "## Pages".to_string(),
        String::new(),
    ];
    for page in pages {
        let filename = format!(
            "{}.{}",
            sanitize_filename(&page.title),
            export_format.file_extension()
        );
        lines.push(format!("- [{}](./{})", page.title, filename));
    }
    lines.join("\n")
}

fn write_or_print_export(content: &str, output_path: Option<&Path>) -> Result<()> {
    if let Some(path) = output_path {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
        }
        std::fs::write(path, content.as_bytes())
            .with_context(|| format!("failed to write {}", normalize_path(path)))?;
    } else {
        println!("{content}");
    }
    Ok(())
}

fn now_timestamp_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
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

    let reason = args.reason.clone();
    let report = delete_local_page(
        &paths,
        &args.title,
        &LocalDeleteOptions {
            reason,
            no_backup: args.no_backup,
            backup_dir: args.backup_dir,
            dry_run: args.dry_run,
        },
    )?;
    print_delete_report(&report);

    if args.dry_run {
        println!("remote_delete: dry_run");
    } else {
        let remote = delete_remote_page(&args.title, &args.reason)?;
        match remote.status {
            RemoteDeleteStatus::Deleted => {
                println!("remote_delete: deleted");
            }
            RemoteDeleteStatus::AlreadyMissing => {
                println!("remote_delete: already_missing");
            }
            RemoteDeleteStatus::SkippedMissingCredentials => {
                println!("remote_delete: skipped_missing_credentials");
            }
        }
        println!("remote_delete.request_count: {}", remote.request_count);
        println!(
            "remote_delete.detail: {}",
            remote.detail.as_deref().unwrap_or("<none>")
        );
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

fn run_lint(runtime: &RuntimeOptions, args: LintArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let report = lint_modules(&paths, args.title.as_deref())?;
    let format = args.format.to_ascii_lowercase();
    if format != "text" && format != "json" {
        bail!(
            "unsupported lint format: {} (expected text|json)",
            args.format
        );
    }

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if !report.selene_available {
        println!("lint");
        println!("selene: missing");
        println!("warning: Selene not found");
        println!("hint: install Selene manually or set SELENE_PATH");
    } else {
        println!("lint");
        println!(
            "selene_path: {}",
            report.selene_path.as_deref().unwrap_or("<none>")
        );
        println!(
            "selene_config: {}",
            report.config_path.as_deref().unwrap_or("<none>")
        );
        println!("inspected_modules: {}", report.inspected_modules);
        println!("errors: {}", report.total_errors);
        println!("warnings: {}", report.total_warnings);
        if report.results.is_empty() {
            println!("issues: <none>");
        } else {
            for result in &report.results {
                println!("module: {}", result.title);
                for issue in &result.errors {
                    println!(
                        "  error: {}:{} {} {}",
                        issue.line, issue.column, issue.code, issue.message
                    );
                }
                for issue in &result.warnings {
                    println!(
                        "  warning: {}:{} {} {}",
                        issue.line, issue.column, issue.code, issue.message
                    );
                }
            }
        }
    }

    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.selene_available
        && (report.total_errors > 0 || (args.strict && report.total_warnings > 0))
    {
        bail!(
            "lint found {} error(s) and {} warning(s)",
            report.total_errors,
            report.total_warnings
        );
    }
    Ok(())
}

fn run_seo_inspect(
    runtime: &RuntimeOptions,
    target: &str,
    json: bool,
    override_url: Option<&str>,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let result = seo_inspect(target, override_url)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("seo inspect");
        println!("url: {}", result.url);
        println!("title: {}", result.title.as_deref().unwrap_or("<missing>"));
        println!(
            "canonical: {}",
            result.canonical.as_deref().unwrap_or("<missing>")
        );
        print_meta_value("description", result.meta.get("description"));
        print_meta_value("og:title", result.meta.get("og:title"));
        print_meta_value("og:description", result.meta.get("og:description"));
        print_meta_value("og:type", result.meta.get("og:type"));
        print_meta_value("og:image", result.meta.get("og:image"));
        print_meta_value("og:url", result.meta.get("og:url"));
        print_meta_value("twitter:card", result.meta.get("twitter:card"));
        print_meta_value("twitter:title", result.meta.get("twitter:title"));
        print_meta_value(
            "twitter:description",
            result.meta.get("twitter:description"),
        );
        print_meta_value("twitter:image", result.meta.get("twitter:image"));
        if result.missing.is_empty() {
            println!("missing: <none>");
        } else {
            println!("missing.count: {}", result.missing.len());
            for item in &result.missing {
                println!("missing.item: {item}");
            }
        }
    }

    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_net_inspect(
    runtime: &RuntimeOptions,
    target: &str,
    json: bool,
    override_url: Option<&str>,
    options: &NetInspectOptions,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let result = net_inspect(target, override_url, options)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("net inspect");
        println!("url: {}", result.url);
        println!("resources.total: {}", result.total_resources);
        println!("resources.inspected: {}", result.inspected);
        println!("known_bytes: {}", result.summary.known_bytes);
        println!("unknown_sizes: {}", result.summary.unknown_count);
        if result.summary.largest.is_empty() {
            println!("largest: <none>");
        } else {
            for entry in &result.summary.largest {
                println!(
                    "largest.resource: size={} type={} url={}",
                    entry
                        .size_bytes
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    entry.resource_type,
                    entry.url
                );
            }
        }
        if result.summary.cache_warnings.is_empty() {
            println!("cache_warnings: <none>");
        } else {
            println!(
                "cache_warnings.count: {}",
                result.summary.cache_warnings.len()
            );
            for warning in &result.summary.cache_warnings {
                println!("cache_warning: {warning}");
            }
        }
    }

    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_perf_lighthouse(
    runtime: &RuntimeOptions,
    target: Option<String>,
    output: &str,
    out: Option<&Path>,
    categories: Option<&str>,
    chrome_flags: Option<&str>,
    show_version: bool,
    json: bool,
    override_url: Option<&str>,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let Some(lighthouse_path) = find_lighthouse_binary(&paths.project_root) else {
        bail!("lighthouse not found on PATH. Install with: npm install -g lighthouse");
    };

    if show_version {
        let info = lighthouse_version(&lighthouse_path)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&info)?);
        } else {
            println!("perf lighthouse");
            println!("path: {}", info.path);
            println!("version: {}", info.version.as_deref().unwrap_or("unknown"));
            println!("code: {}", info.code);
            if !info.stderr.trim().is_empty() {
                println!("stderr: {}", info.stderr.trim());
            }
        }
        println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
        if info.code != 0 {
            bail!("failed to resolve lighthouse version");
        }
        return Ok(());
    }

    let output_format = LighthouseOutputFormat::parse(output)?;
    let report = run_lighthouse(
        &paths.project_root,
        &lighthouse_path,
        &LighthouseRunOptions {
            target,
            target_url_override: override_url.map(ToString::to_string),
            output_format,
            output_path_override: out.map(Path::to_path_buf),
            categories: parse_csv_list(categories),
            chrome_flags: chrome_flags.map(ToString::to_string),
        },
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("perf lighthouse");
        println!("url: {}", report.url);
        println!("format: {}", report.format);
        println!("report_path: {}", report.report_path);
        println!(
            "report_bytes: {}",
            report
                .report_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        );
        if report.categories.is_empty() {
            println!("categories: <default>");
        } else {
            println!("categories: {}", report.categories.join(","));
        }
        if report.ignored_windows_cleanup_failure {
            println!(
                "warning: ignored known Windows chrome-launcher cleanup failure (report generated)"
            );
        }
    }

    println!("policy: {NO_MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_import_cargo(
    runtime: &RuntimeOptions,
    path: &str,
    table: &str,
    source_type: Option<&str>,
    template: Option<&str>,
    title_field: Option<&str>,
    title_prefix: Option<&str>,
    category: Option<&str>,
    mode: &str,
    write: bool,
    format: &str,
    article_header: bool,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let Some(source_type) = ImportSourceType::resolve(path, source_type) else {
        bail!("unable to determine import type (use --type csv|json)");
    };
    let update_mode = parse_import_mode(mode)?;
    let format = format.to_ascii_lowercase();
    if format != "text" && format != "json" {
        bail!("unsupported import format: {format} (expected text|json)");
    }

    let source_path = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(path)
    };
    let result = import_to_cargo(
        &paths,
        &source_path,
        source_type,
        &CargoImportOptions {
            table_name: table.to_string(),
            template_name: normalize_option(template),
            title_field: normalize_option(title_field),
            title_prefix: normalize_option(title_prefix),
            update_mode,
            category_name: normalize_option(category),
            article_header,
            write,
        },
    )?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("import cargo");
        println!("source_path: {}", normalize_path(&source_path));
        println!("source_type: {}", source_type.as_str());
        println!("table: {table}");
        println!("update_mode: {}", mode.to_ascii_lowercase());
        println!("write: {}", format_flag(write));
        println!("created: {}", result.pages_created.len());
        println!("updated: {}", result.pages_updated.len());
        println!("skipped: {}", result.pages_skipped.len());
        println!("errors: {}", result.errors.len());
        for error in result.errors.iter().take(10) {
            println!(
                "error: row={} message={} title={}",
                error.row,
                error.message,
                error.title.as_deref().unwrap_or("<none>")
            );
        }
        for page in result.pages.iter().take(10) {
            println!(
                "page: action={:?} title={} path={}",
                page.action, page.title, page.relative_path
            );
        }
        if !write {
            println!("warning: dry-run only. Use --write to apply changes.");
        }
    }

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

fn run_docs_generate_reference(args: DocsGenerateReferenceArgs) -> Result<()> {
    let output = args
        .output
        .unwrap_or_else(|| PathBuf::from("docs/wikitool/reference.md"));
    let output = if output.is_absolute() {
        output
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(output)
    };

    let markdown = generate_docs_reference_markdown()?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", normalize_path(parent)))?;
    }
    fs::write(&output, markdown)
        .with_context(|| format!("failed to write {}", normalize_path(&output)))?;

    println!("Wrote {}", normalize_path(&output));
    Ok(())
}

fn generate_docs_reference_markdown() -> Result<String> {
    let command = Cli::command();
    let mut command_paths = Vec::new();
    collect_command_paths(&command, &[], &mut command_paths);

    let mut lines = vec![
        "# Wikitool Command Reference".to_string(),
        "".to_string(),
        "This file is generated from Rust CLI help output. Do not edit manually.".to_string(),
        "".to_string(),
        "Regenerate:".to_string(),
        "".to_string(),
        "```bash".to_string(),
        "wikitool docs generate-reference".to_string(),
        "```".to_string(),
        "".to_string(),
    ];

    for path in command_paths {
        let title = if path.is_empty() {
            "Global".to_string()
        } else {
            path.join(" ")
        };
        let help_text = help_text_for_command_path(&path)?;
        lines.push(format!("## {title}"));
        lines.push(String::new());
        lines.push("```text".to_string());
        lines.push(help_text);
        lines.push("```".to_string());
        lines.push(String::new());
    }

    Ok(lines.join("\n"))
}

fn collect_command_paths(command: &Command, prefix: &[String], out: &mut Vec<Vec<String>>) {
    out.push(prefix.to_vec());

    for subcommand in command.get_subcommands() {
        let mut next = prefix.to_vec();
        next.push(subcommand.get_name().to_string());
        collect_command_paths(subcommand, &next, out);
    }
}

fn help_text_for_command_path(path: &[String]) -> Result<String> {
    let mut command = Cli::command();
    command = command.bin_name("wikitool");

    let mut args = Vec::with_capacity(path.len() + 2);
    args.push("wikitool".to_string());
    args.extend(path.iter().cloned());
    args.push("--help".to_string());

    match command.try_get_matches_from(args) {
        Ok(_) => bail!(
            "failed to render help for path {}",
            if path.is_empty() {
                "<global>".to_string()
            } else {
                path.join(" ")
            }
        ),
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                Ok(error.to_string().trim_end().to_string())
            }
            _ => Err(error).with_context(|| {
                format!(
                    "failed to resolve command path {}",
                    if path.is_empty() {
                        "<global>".to_string()
                    } else {
                        path.join(" ")
                    }
                )
            }),
        },
    }
}

fn run_workflow(runtime: &RuntimeOptions, args: WorkflowArgs) -> Result<()> {
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

    run_init(
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
        run_docs_generate_reference(DocsGenerateReferenceArgs {
            output: Some(paths.project_root.join("docs/wikitool/reference.md")),
        })?;
    }

    if !args.skip_git_hooks {
        run_dev_install_git_hooks(InstallGitHooksArgs {
            repo_root: Some(paths.project_root.clone()),
            source: None,
            allow_missing_git: true,
        })?;
    }

    if should_pull {
        run_pull(
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

    run_init(
        runtime,
        InitArgs {
            templates: include_templates,
            force: false,
            no_config: false,
            no_parser_config: false,
        },
    )?;

    if !args.skip_reference {
        run_docs_generate_reference(DocsGenerateReferenceArgs {
            output: Some(paths.project_root.join("docs/wikitool/reference.md")),
        })?;
    }

    run_pull(
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
    run_validate(runtime)?;
    run_status(
        runtime,
        StatusArgs {
            modified: false,
            conflicts: false,
            templates: true,
        },
    )?;
    Ok(())
}

fn run_release(args: ReleaseArgs) -> Result<()> {
    match args.command {
        ReleaseSubcommand::BuildAiPack(options) => run_release_build_ai_pack(options),
        ReleaseSubcommand::Package(options) => run_release_package(options),
        ReleaseSubcommand::BuildMatrix(options) => run_release_build_matrix(options),
    }
}

fn run_release_build_ai_pack(args: ReleaseBuildAiPackArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root)?;
    let output_dir = args
        .output_dir
        .unwrap_or_else(|| repo_root.join("dist/ai-pack"));

    let result = build_ai_pack(&repo_root, &output_dir, args.host_project_root.as_deref())?;

    println!("release build-ai-pack");
    println!("repo_root: {}", normalize_path(&repo_root));
    println!("output_dir: {}", normalize_path(&result.output_dir));
    println!(
        "host_context_included: {}",
        format_flag(result.host_context_included)
    );
    println!(
        "claude_rules_included: {}",
        format_flag(result.claude_rules_included)
    );
    println!(
        "claude_skills_included: {}",
        format_flag(result.claude_skills_included)
    );
    println!(
        "codex_skills_included: {}",
        format_flag(result.codex_skills_included)
    );
    println!(
        "docs_bundle_included: {}",
        format_flag(result.docs_bundle_included)
    );

    Ok(())
}

fn run_release_package(args: ReleasePackageArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root)?;
    let output_dir = args
        .output_dir
        .unwrap_or_else(|| repo_root.join("dist/release"));
    let binary_path = args.binary_path.unwrap_or_else(|| {
        repo_root
            .join("target/release")
            .join(default_release_binary_name())
    });
    if !binary_path.is_file() {
        bail!("missing release binary: {}", normalize_path(&binary_path));
    }

    let ai_pack_dir = repo_root.join("dist/ai-pack");
    let ai_pack_result =
        build_ai_pack(&repo_root, &ai_pack_dir, args.host_project_root.as_deref())?;

    reset_directory(&output_dir)?;
    copy_file(
        &binary_path,
        &output_dir.join(default_release_binary_name()),
    )?;
    copy_dir_contents(&ai_pack_dir, &output_dir)?;

    println!("release package");
    println!("repo_root: {}", normalize_path(&repo_root));
    println!("binary_path: {}", normalize_path(&binary_path));
    println!("output_dir: {}", normalize_path(&output_dir));
    println!(
        "host_context_included: {}",
        format_flag(ai_pack_result.host_context_included)
    );
    println!(
        "claude_rules_included: {}",
        format_flag(ai_pack_result.claude_rules_included)
    );
    println!(
        "claude_skills_included: {}",
        format_flag(ai_pack_result.claude_skills_included)
    );
    println!(
        "codex_skills_included: {}",
        format_flag(ai_pack_result.codex_skills_included)
    );
    println!(
        "docs_bundle_included: {}",
        format_flag(ai_pack_result.docs_bundle_included)
    );
    Ok(())
}

#[derive(Debug)]
struct ReleaseMatrixArtifact {
    target: String,
    binary_path: PathBuf,
    bundle_dir: PathBuf,
    zip_path: PathBuf,
}

fn run_release_build_matrix(args: ReleaseBuildMatrixArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root)?;
    let output_dir = args
        .output_dir
        .unwrap_or_else(|| repo_root.join("dist/release-matrix"));
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", normalize_path(&output_dir)))?;

    let cargo_bin = args.cargo_bin.unwrap_or_else(|| PathBuf::from("cargo"));
    let use_locked = resolve_default_true_flag(
        args.locked,
        args.no_locked,
        "release build-matrix lockfile flag",
    )?;
    let targets = resolve_release_targets(&args.targets);
    let artifact_version =
        resolve_release_artifact_version(args.artifact_version.as_deref(), args.unversioned_names)?;

    let ai_pack_dir = output_dir.join("_ai-pack-staging");
    let ai_pack_result =
        build_ai_pack(&repo_root, &ai_pack_dir, args.host_project_root.as_deref())?;

    let mut artifacts = Vec::new();
    for target in &targets {
        if !args.skip_build {
            run_cargo_release_build_for_target(&repo_root, &cargo_bin, target, use_locked)?;
        }

        let binary_path = release_binary_path_for_target(&repo_root, target);
        if !binary_path.is_file() {
            bail!(
                "missing built binary for target {target}: {}",
                normalize_path(&binary_path)
            );
        }

        let bundle_name = release_bundle_name(target, artifact_version.as_deref());
        let bundle_dir = output_dir.join(&bundle_name);
        reset_directory(&bundle_dir)?;
        copy_file(
            &binary_path,
            &bundle_dir.join(release_binary_name_for_target(target)),
        )?;
        copy_dir_contents(&ai_pack_dir, &bundle_dir)?;

        let zip_path = output_dir.join(format!("{bundle_name}.zip"));
        zip_release_bundle(&bundle_dir, &zip_path, &bundle_name)?;

        artifacts.push(ReleaseMatrixArtifact {
            target: target.clone(),
            binary_path,
            bundle_dir,
            zip_path,
        });
    }

    if ai_pack_dir.exists() {
        fs::remove_dir_all(&ai_pack_dir)
            .with_context(|| format!("failed to remove {}", normalize_path(&ai_pack_dir)))?;
    }

    println!("release build-matrix");
    println!("repo_root: {}", normalize_path(&repo_root));
    println!("output_dir: {}", normalize_path(&output_dir));
    println!(
        "artifact_version: {}",
        artifact_version.as_deref().unwrap_or("<none>")
    );
    println!("target_count: {}", artifacts.len());
    println!(
        "host_context_included: {}",
        format_flag(ai_pack_result.host_context_included)
    );
    println!(
        "claude_rules_included: {}",
        format_flag(ai_pack_result.claude_rules_included)
    );
    println!(
        "claude_skills_included: {}",
        format_flag(ai_pack_result.claude_skills_included)
    );
    println!(
        "codex_skills_included: {}",
        format_flag(ai_pack_result.codex_skills_included)
    );
    println!(
        "docs_bundle_included: {}",
        format_flag(ai_pack_result.docs_bundle_included)
    );
    for artifact in &artifacts {
        println!("artifact.target: {}", artifact.target);
        println!(
            "artifact.binary_path: {}",
            normalize_path(&artifact.binary_path)
        );
        println!(
            "artifact.bundle_dir: {}",
            normalize_path(&artifact.bundle_dir)
        );
        println!("artifact.zip_path: {}", normalize_path(&artifact.zip_path));
    }
    Ok(())
}

fn run_dev(args: DevArgs) -> Result<()> {
    match args.command {
        DevSubcommand::InstallGitHooks(options) => run_dev_install_git_hooks(options),
    }
}

fn run_dev_install_git_hooks(args: InstallGitHooksArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root)?;
    let hooks_dir = repo_root.join(".git/hooks");
    if !hooks_dir.is_dir() {
        if args.allow_missing_git {
            println!(
                "No .git/hooks directory found at {}. Skipping hook install.",
                normalize_path(&hooks_dir)
            );
            return Ok(());
        }
        bail!(
            "no .git/hooks directory found at {}",
            normalize_path(&hooks_dir)
        );
    }

    let source = args
        .source
        .unwrap_or_else(|| repo_root.join("scripts/git-hooks/commit-msg"));
    if !source.is_file() {
        bail!("hook source not found: {}", normalize_path(&source));
    }
    let destination = hooks_dir.join("commit-msg");
    copy_file(&source, &destination)?;
    set_executable_if_unix(&destination)?;

    println!(
        "Installed commit-msg hook: {}",
        normalize_path(&destination)
    );
    Ok(())
}

#[derive(Debug)]
struct AiPackBuildResult {
    output_dir: PathBuf,
    host_context_included: bool,
    claude_rules_included: bool,
    claude_skills_included: bool,
    codex_skills_included: bool,
    docs_bundle_included: bool,
}

fn build_ai_pack(
    repo_root: &Path,
    output_dir: &Path,
    host_project_root: Option<&Path>,
) -> Result<AiPackBuildResult> {
    let ai_pack_root = repo_root.join("ai-pack");
    reset_directory(output_dir)?;

    for file in ["SETUP.md", "README.md"] {
        let src = repo_root.join(file);
        if !src.is_file() {
            bail!("missing required AI pack file: {}", normalize_path(&src));
        }
        copy_file(&src, &output_dir.join(file))?;
    }

    let ai_pack_agents = ai_pack_root.join("AGENTS.md");
    let ai_pack_claude = ai_pack_root.join("CLAUDE.md");
    for file in [&ai_pack_agents, &ai_pack_claude] {
        if !file.is_file() {
            bail!(
                "missing required AI pack source file: {}",
                normalize_path(file)
            );
        }
    }
    ensure_files_identical(
        &ai_pack_agents,
        &ai_pack_claude,
        "ai-pack instruction contract violation",
    )?;

    let claude_rules_source = ai_pack_root.join(".claude/rules");
    if !claude_rules_source.is_dir() {
        bail!(
            "missing required AI pack Claude rules directory: {}",
            normalize_path(&claude_rules_source)
        );
    }
    copy_dir_recursive(&claude_rules_source, &output_dir.join(".claude/rules"))?;
    let mut claude_rules_included = true;

    let claude_skills_source = ai_pack_root.join(".claude/skills");
    if !claude_skills_source.is_dir() {
        bail!(
            "missing required AI pack Claude skills directory: {}",
            normalize_path(&claude_skills_source)
        );
    }
    copy_dir_recursive(&claude_skills_source, &output_dir.join(".claude/skills"))?;
    let mut claude_skills_included = true;

    let mut effective_claude_source = ai_pack_claude.clone();

    let mut host_context_included = false;
    if let Some(host_root) = detect_host_context_root(repo_root, host_project_root)?
        && !paths_equivalent(&host_root, repo_root)?
    {
        copy_file(&ai_pack_claude, &output_dir.join("WIKITOOL_CLAUDE.md"))?;
        effective_claude_source = host_root.join("CLAUDE.md");
        copy_dir_recursive(
            &host_root.join(".claude/rules"),
            &output_dir.join(".claude/rules"),
        )?;
        copy_dir_recursive(
            &host_root.join(".claude/skills"),
            &output_dir.join(".claude/skills"),
        )?;
        claude_rules_included = true;
        claude_skills_included = true;
        host_context_included = true;
    }

    copy_file(&effective_claude_source, &output_dir.join("CLAUDE.md"))?;
    copy_file(&effective_claude_source, &output_dir.join("AGENTS.md"))?;

    let llm_source = ai_pack_root.join("llm_instructions");
    if !llm_source.is_dir() {
        bail!(
            "missing llm_instructions directory: {}",
            normalize_path(&llm_source)
        );
    }
    let llm_dest = output_dir.join("llm_instructions");
    fs::create_dir_all(&llm_dest)
        .with_context(|| format!("failed to create {}", normalize_path(&llm_dest)))?;
    let mut llm_count = 0usize;
    for entry in fs::read_dir(&llm_source)
        .with_context(|| format!("failed to read {}", normalize_path(&llm_source)))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_markdown_file(&path) {
            copy_file(&path, &llm_dest.join(entry.file_name()))?;
            llm_count += 1;
        }
    }
    if llm_count == 0 {
        bail!("no ai-pack/llm_instructions/*.md files found");
    }

    let docs_source = repo_root.join("docs/wikitool");
    if docs_source.is_dir() {
        let docs_dest = output_dir.join("docs/wikitool");
        fs::create_dir_all(&docs_dest)
            .with_context(|| format!("failed to create {}", normalize_path(&docs_dest)))?;
        for entry in fs::read_dir(&docs_source)
            .with_context(|| format!("failed to read {}", normalize_path(&docs_source)))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_markdown_file(&path) {
                copy_file(&path, &docs_dest.join(entry.file_name()))?;
            }
        }
    }

    let codex_skills_source = ai_pack_root.join("codex_skills");
    let mut codex_skills_included = false;
    if codex_skills_source.is_dir() {
        copy_dir_recursive(&codex_skills_source, &output_dir.join("codex_skills"))?;
        codex_skills_included = true;
    }

    let docs_bundle_source = ai_pack_root.join("docs-bundle-v1.json");
    let mut docs_bundle_included = false;
    if docs_bundle_source.is_file() {
        copy_file(
            &docs_bundle_source,
            &output_dir.join("ai/docs-bundle-v1.json"),
        )?;
        docs_bundle_included = true;
    }

    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let manifest = serde_json::json!({
        "schema_version": 1,
        "generated_at_unix": now_unix,
        "host_context_included": host_context_included,
        "claude_rules_included": claude_rules_included,
        "claude_skills_included": claude_skills_included,
        "codex_skills_included": codex_skills_included,
        "docs_bundle_included": docs_bundle_included,
        "agents_mirrors_claude": true,
        "notes": "AI companion pack for wikitool; content is intentionally shipped outside the binary."
    });
    fs::write(
        output_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            normalize_path(&output_dir.join("manifest.json"))
        )
    })?;

    Ok(AiPackBuildResult {
        output_dir: output_dir.to_path_buf(),
        host_context_included,
        claude_rules_included,
        claude_skills_included,
        codex_skills_included,
        docs_bundle_included,
    })
}

fn resolve_repo_root(value: Option<PathBuf>) -> Result<PathBuf> {
    let repo_root = match value {
        Some(path) => path,
        None => std::env::current_dir().context("failed to resolve current directory")?,
    };
    if !repo_root.exists() {
        bail!("path does not exist: {}", normalize_path(&repo_root));
    }
    fs::canonicalize(&repo_root)
        .with_context(|| format!("failed to canonicalize {}", normalize_path(&repo_root)))
}

fn resolve_default_true_flag(enabled: bool, disabled: bool, label: &str) -> Result<bool> {
    if enabled && disabled {
        bail!("invalid options for {label}: enable and disable flags both set");
    }
    if disabled {
        return Ok(false);
    }
    Ok(true)
}

fn prompt_yes_no(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush().context("failed to flush stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read confirmation input")?;
    let normalized = input.trim().to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
}

fn detect_host_context_root(repo_root: &Path, explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    let _ = repo_root;
    let Some(path) = explicit else {
        return Ok(None);
    };

    let root = fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize {}", normalize_path(path)))?;
    if !root.join("CLAUDE.md").is_file()
        || !root.join(".claude/rules").is_dir()
        || !root.join(".claude/skills").is_dir()
    {
        bail!(
            "invalid host project root {}: expected CLAUDE.md and .claude/{{rules,skills}}",
            normalize_path(&root)
        );
    }
    Ok(Some(root))
}

fn ensure_files_identical(left: &Path, right: &Path, label: &str) -> Result<()> {
    let left_bytes =
        fs::read(left).with_context(|| format!("failed to read {}", normalize_path(left)))?;
    let right_bytes =
        fs::read(right).with_context(|| format!("failed to read {}", normalize_path(right)))?;
    if left_bytes != right_bytes {
        bail!(
            "{label}: {} and {} must match",
            normalize_path(left),
            normalize_path(right)
        );
    }
    Ok(())
}

fn reset_directory(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove {}", normalize_path(path)))?;
    }
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", normalize_path(path)))
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_file() {
        bail!("file not found: {}", normalize_path(source));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "failed to copy {} -> {}",
            normalize_path(source),
            normalize_path(destination)
        )
    })?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("directory not found: {}", normalize_path(source));
    }
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", normalize_path(destination)))?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read {}", normalize_path(source)))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to read metadata {}", normalize_path(&source_path)))?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            copy_file(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn copy_dir_contents(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("directory not found: {}", normalize_path(source));
    }
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", normalize_path(destination)))?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read {}", normalize_path(source)))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to read metadata {}", normalize_path(&source_path)))?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            copy_file(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn paths_equivalent(left: &Path, right: &Path) -> Result<bool> {
    let left = fs::canonicalize(left)
        .with_context(|| format!("failed to canonicalize {}", normalize_path(left)))?;
    let right = fs::canonicalize(right)
        .with_context(|| format!("failed to canonicalize {}", normalize_path(right)))?;
    Ok(left == right)
}

fn default_release_binary_name() -> &'static str {
    if cfg!(windows) {
        "wikitool.exe"
    } else {
        "wikitool"
    }
}

const DEFAULT_RELEASE_MATRIX_TARGETS: [&str; 3] = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
];

fn resolve_release_targets(raw_targets: &[String]) -> Vec<String> {
    let mut targets = Vec::new();
    for raw in raw_targets {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !targets.iter().any(|existing| existing == trimmed) {
            targets.push(trimmed.to_string());
        }
    }
    if targets.is_empty() {
        return DEFAULT_RELEASE_MATRIX_TARGETS
            .iter()
            .map(|target| (*target).to_string())
            .collect();
    }
    targets
}

fn resolve_release_artifact_version(
    raw_label: Option<&str>,
    unversioned_names: bool,
) -> Result<Option<String>> {
    if unversioned_names {
        if raw_label.is_some() {
            bail!("cannot combine --artifact-version with --unversioned-names");
        }
        return Ok(None);
    }

    let label = raw_label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));

    if !label
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        bail!("invalid artifact version label `{label}`: allowed characters are [A-Za-z0-9._-]");
    }
    Ok(Some(label))
}

fn release_bundle_name(target: &str, artifact_version: Option<&str>) -> String {
    match artifact_version {
        Some(version) => format!("wikitool-{version}-{target}"),
        None => format!("wikitool-{target}"),
    }
}

fn run_cargo_release_build_for_target(
    repo_root: &Path,
    cargo_bin: &Path,
    target: &str,
    use_locked: bool,
) -> Result<()> {
    let mut command = ProcessCommand::new(cargo_bin);
    command
        .current_dir(repo_root)
        .arg("build")
        .arg("--package")
        .arg("wikitool")
        .arg("--release")
        .arg("--target")
        .arg(target);
    if use_locked {
        command.arg("--locked");
    }
    let status = command.status().with_context(|| {
        format!(
            "failed to execute {} for target {target}",
            normalize_path(cargo_bin)
        )
    })?;
    if !status.success() {
        bail!("cargo build failed for target {target}");
    }
    Ok(())
}

fn release_binary_name_for_target(target: &str) -> &'static str {
    if target.to_ascii_lowercase().contains("windows") {
        "wikitool.exe"
    } else {
        "wikitool"
    }
}

fn release_binary_path_for_target(repo_root: &Path, target: &str) -> PathBuf {
    repo_root
        .join("target")
        .join(target)
        .join("release")
        .join(release_binary_name_for_target(target))
}

fn zip_release_bundle(source_dir: &Path, zip_path: &Path, bundle_name: &str) -> Result<()> {
    if !source_dir.is_dir() {
        bail!("directory not found: {}", normalize_path(source_dir));
    }
    if let Some(parent) = zip_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }

    let zip_file = fs::File::create(zip_path)
        .with_context(|| format!("failed to create {}", normalize_path(zip_path)))?;
    let mut zip_writer = ZipWriter::new(zip_file);
    let dir_options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);
    zip_writer
        .add_directory(format!("{bundle_name}/"), dir_options)
        .with_context(|| format!("failed to create zip root in {}", normalize_path(zip_path)))?;

    for relative_path in collect_relative_file_paths(source_dir)? {
        let source_path = source_dir.join(&relative_path);
        let normalized_relative = normalize_path(&relative_path);
        let entry_name = format!("{bundle_name}/{normalized_relative}");
        let mode = if is_release_binary_entry(&relative_path) {
            0o755
        } else {
            0o644
        };
        let file_options = FileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(mode);
        zip_writer
            .start_file(&entry_name, file_options)
            .with_context(|| {
                format!(
                    "failed to create zip entry {} in {}",
                    entry_name,
                    normalize_path(zip_path)
                )
            })?;
        let mut input = fs::File::open(&source_path)
            .with_context(|| format!("failed to open {}", normalize_path(&source_path)))?;
        io::copy(&mut input, &mut zip_writer).with_context(|| {
            format!(
                "failed to write zip entry {} in {}",
                entry_name,
                normalize_path(zip_path)
            )
        })?;
    }

    zip_writer
        .finish()
        .with_context(|| format!("failed to finalize {}", normalize_path(zip_path)))?;
    Ok(())
}

fn collect_relative_file_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_relative_file_paths_recursive(root, root, &mut files)?;
    files.sort_by_key(|path| normalize_path(path));
    Ok(files)
}

fn collect_relative_file_paths_recursive(
    root: &Path,
    current: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(current)
        .with_context(|| format!("failed to read {}", normalize_path(current)))?
    {
        entries.push(entry?);
    }
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to read metadata {}", normalize_path(&path)))?;
        if metadata.is_dir() {
            collect_relative_file_paths_recursive(root, &path, output)?;
        } else if metadata.is_file() {
            let relative = path.strip_prefix(root).with_context(|| {
                format!(
                    "failed to derive relative path from {} using root {}",
                    normalize_path(&path),
                    normalize_path(root)
                )
            })?;
            output.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn is_release_binary_entry(relative_path: &Path) -> bool {
    relative_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value == "wikitool")
        .unwrap_or(false)
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

#[cfg(unix)]
fn set_executable_if_unix(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata {}", normalize_path(path)))?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set permissions {}", normalize_path(path)))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable_if_unix(_path: &Path) -> Result<()> {
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

fn print_meta_value(label: &str, values: Option<&Vec<String>>) {
    match values {
        Some(values) if !values.is_empty() => {
            println!("meta.{label}: {}", values[0]);
            if values.len() > 1 {
                println!("meta.{label}.extra_count: {}", values.len() - 1);
            }
        }
        _ => println!("meta.{label}: <missing>"),
    }
}

fn parse_csv_list(value: Option<&str>) -> Vec<String> {
    let mut output = Vec::new();
    let Some(raw) = value else {
        return output;
    };
    for part in raw.split(',') {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            output.push(trimmed.to_string());
        }
    }
    output
}

fn parse_import_mode(value: &str) -> Result<ImportUpdateMode> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "create" => Ok(ImportUpdateMode::Create),
        "update" => Ok(ImportUpdateMode::Update),
        "upsert" => Ok(ImportUpdateMode::Upsert),
        _ => bail!("unsupported import mode: {value} (expected create|update|upsert)"),
    }
}

fn normalize_option(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
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
    let mut value = path.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = value.strip_prefix("//?/") {
        value = stripped.to_string();
    }
    value
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
