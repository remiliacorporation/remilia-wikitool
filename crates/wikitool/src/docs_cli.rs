use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Command, CommandFactory, Subcommand, error::ErrorKind};
use wikitool_core::docs::{
    DocsContextOptions, DocsImportOptions, DocsImportProfileOptions, DocsImportTechnicalOptions,
    DocsListOptions, DocsRemoveKind, DocsSearchOptions, DocsSymbolLookupOptions, TechnicalDocType,
    TechnicalImportTask, build_docs_context, format_expiration, import_docs_bundle,
    import_docs_profile_with_config, import_extension_docs, import_technical_docs, list_docs,
    lookup_docs_symbols, remove_docs, search_docs, update_outdated_docs_with_config,
};

use crate::{
    Cli, LOCAL_DB_POLICY_MESSAGE, RuntimeOptions,
    cli_support::{
        collapse_whitespace, format_flag, normalize_path, normalize_title_query,
        resolve_runtime_paths, resolve_runtime_with_config,
    },
};

#[derive(Debug, Args)]
pub(crate) struct DocsArgs {
    #[command(subcommand)]
    command: DocsSubcommand,
}

#[derive(Debug, Subcommand)]
enum DocsSubcommand {
    Import(DocsImportArgs),
    #[command(name = "import-technical")]
    ImportTechnical(DocsImportTechnicalArgs),
    #[command(name = "import-profile")]
    ImportProfile(DocsImportProfileArgs),
    #[command(name = "generate-reference")]
    GenerateReference(DocsGenerateReferenceArgs),
    List(DocsListArgs),
    Update,
    Remove {
        target: String,
    },
    Search(DocsSearchArgs),
    Context(DocsContextArgs),
    Symbols(DocsSymbolsArgs),
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
    #[arg(long = "help-docs", help = "Import Help: docs")]
    help_docs: bool,
    #[arg(
        short = 'l',
        long,
        default_value_t = 100,
        help = "Limit subpage imports per task"
    )]
    limit: usize,
}

#[derive(Debug, Args)]
struct DocsImportProfileArgs {
    #[arg(value_name = "PROFILE", default_value = "remilia-mw-1.44")]
    profile: String,
    #[arg(long, help = "Discover installed extensions from the configured wiki")]
    installed: bool,
    #[arg(
        long = "no-extension-subpages",
        help = "Skip extension subpages for profile extension docs"
    )]
    no_extension_subpages: bool,
    #[arg(
        long = "extension",
        value_name = "EXTENSION",
        help = "Add extra extension docs to the profile import"
    )]
    extensions: Vec<String>,
    #[arg(
        short = 'l',
        long,
        default_value_t = 100,
        help = "Limit subpage imports per profile seed"
    )]
    limit: usize,
}

#[derive(Debug, Args)]
struct DocsListArgs {
    #[arg(long, help = "Show only outdated docs")]
    outdated: bool,
    #[arg(long, value_name = "TYPE", help = "Filter technical docs by type")]
    r#type: Option<String>,
    #[arg(long, value_name = "KIND", help = "Filter corpora by kind")]
    kind: Option<String>,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Filter corpora by source profile"
    )]
    profile: Option<String>,
}

#[derive(Debug, Args)]
struct DocsSearchArgs {
    query: String,
    #[arg(
        long,
        value_name = "TIER",
        help = "Search tier: page|section|symbol|example|extension|technical|profile"
    )]
    tier: Option<String>,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Restrict search to a docs profile"
    )]
    profile: Option<String>,
    #[arg(long, default_value = "text", help = "Output format: text|json")]
    format: String,
    #[arg(short = 'l', long, default_value_t = 20, help = "Limit result count")]
    limit: usize,
}

#[derive(Debug, Args)]
struct DocsContextArgs {
    query: String,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Restrict context retrieval to a docs profile"
    )]
    profile: Option<String>,
    #[arg(long, default_value = "json", help = "Output format: text|json")]
    format: String,
    #[arg(short = 'l', long, default_value_t = 6, help = "Limit hits per tier")]
    limit: usize,
    #[arg(
        long,
        default_value_t = 1600,
        help = "Approximate token budget for returned context"
    )]
    token_budget: usize,
}

#[derive(Debug, Args)]
struct DocsSymbolsArgs {
    query: String,
    #[arg(long, value_name = "KIND", help = "Symbol kind filter")]
    kind: Option<String>,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Restrict symbol lookup to a docs profile"
    )]
    profile: Option<String>,
    #[arg(long, default_value = "text", help = "Output format: text|json")]
    format: String,
    #[arg(short = 'l', long, default_value_t = 20, help = "Limit result count")]
    limit: usize,
}

#[derive(Debug, Args)]
pub(crate) struct DocsGenerateReferenceArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Output markdown path (default: docs/wikitool/reference.md in current directory)"
    )]
    pub(crate) output: Option<PathBuf>,
}

pub(crate) fn run_docs(runtime: &RuntimeOptions, args: DocsArgs) -> Result<()> {
    match args.command {
        DocsSubcommand::Import(args) => run_docs_import(runtime, args),
        DocsSubcommand::ImportTechnical(args) => run_docs_import_technical(runtime, args),
        DocsSubcommand::ImportProfile(args) => run_docs_import_profile(runtime, args),
        DocsSubcommand::GenerateReference(args) => run_docs_generate_reference(args),
        DocsSubcommand::List(args) => run_docs_list(runtime, args),
        DocsSubcommand::Update => run_docs_update(runtime),
        DocsSubcommand::Remove { target } => run_docs_remove(runtime, &target),
        DocsSubcommand::Search(args) => run_docs_search(runtime, args),
        DocsSubcommand::Context(args) => run_docs_context(runtime, args),
        DocsSubcommand::Symbols(args) => run_docs_symbols(runtime, args),
    }
}

fn run_docs_import(runtime: &RuntimeOptions, args: DocsImportArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;

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
        println!("imported_sections: {}", report.imported_sections);
        println!("imported_symbols: {}", report.imported_symbols);
        println!("imported_examples: {}", report.imported_examples);
        println!("failures.count: {}", report.failures.len());
        for failure in &report.failures {
            println!("failure: {failure}");
        }
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
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
        extensions.extend(
            wikitool_core::docs::discover_installed_extensions_from_wiki_with_config(&config)
                .context("failed to discover installed extensions from live wiki API")?,
        );
    }

    let normalized = normalize_title_list(extensions);
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
    println!("imported_sections: {}", report.imported_sections);
    println!("imported_symbols: {}", report.imported_symbols);
    println!("imported_examples: {}", report.imported_examples);
    println!("request_count: {}", report.request_count);
    println!("failures.count: {}", report.failures.len());
    for failure in &report.failures {
        println!("failure: {failure}");
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
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
    if args.help_docs {
        tasks.push(TechnicalImportTask {
            doc_type: TechnicalDocType::Help,
            page_title: None,
            include_subpages: true,
        });
    }
    if tasks.is_empty() {
        bail!("no technical documentation specified");
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
    println!("requested_tasks: {}", report.requested_tasks);
    println!("imported_corpora: {}", report.imported_corpora);
    println!("imported_pages: {}", report.imported_pages);
    println!("imported_sections: {}", report.imported_sections);
    println!("imported_symbols: {}", report.imported_symbols);
    println!("imported_examples: {}", report.imported_examples);
    println!("request_count: {}", report.request_count);
    for (doc_type, count) in &report.imported_by_type {
        println!("imported_by_type.{doc_type}: {count}");
    }
    println!("failures.count: {}", report.failures.len());
    for failure in &report.failures {
        println!("failure: {failure}");
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.imported_pages == 0 {
        bail!("docs import-technical completed with no imported pages")
    }
    Ok(())
}

fn run_docs_import_profile(runtime: &RuntimeOptions, args: DocsImportProfileArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let report = import_docs_profile_with_config(
        &paths,
        &DocsImportProfileOptions {
            profile: normalize_title_query(&args.profile),
            include_installed_extensions: args.installed,
            include_extension_subpages: !args.no_extension_subpages,
            extra_extensions: normalize_title_list(args.extensions),
            limit: args.limit.max(1),
        },
        &config,
    )?;

    println!("docs import-profile");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("profile: {}", report.profile);
    println!("imported_corpora: {}", report.imported_corpora);
    println!("imported_extensions: {}", report.imported_extensions);
    println!("imported_pages: {}", report.imported_pages);
    println!("imported_sections: {}", report.imported_sections);
    println!("imported_symbols: {}", report.imported_symbols);
    println!("imported_examples: {}", report.imported_examples);
    println!("request_count: {}", report.request_count);
    println!("failures.count: {}", report.failures.len());
    for failure in &report.failures {
        println!("failure: {failure}");
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.imported_pages == 0 {
        bail!("docs import-profile completed with no imported pages")
    }
    Ok(())
}

fn run_docs_list(runtime: &RuntimeOptions, args: DocsListArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let listing = list_docs(
        &paths,
        &DocsListOptions {
            technical_type: args.r#type.clone(),
            corpus_kind: args.kind.clone(),
            profile: args.profile.clone(),
        },
    )?;

    println!("docs list");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("stats.corpora_count: {}", listing.stats.corpora_count);
    println!("stats.pages_count: {}", listing.stats.pages_count);
    println!("stats.sections_count: {}", listing.stats.sections_count);
    println!("stats.symbols_count: {}", listing.stats.symbols_count);
    println!("stats.examples_count: {}", listing.stats.examples_count);
    for (kind, count) in &listing.stats.corpora_by_kind {
        println!("stats.corpora_by_kind.{kind}: {count}");
    }
    for (doc_type, count) in &listing.stats.technical_by_type {
        println!("stats.technical_by_type.{doc_type}: {count}");
    }

    if args.outdated {
        println!("outdated.corpora.count: {}", listing.outdated.corpora.len());
        for corpus in &listing.outdated.corpora {
            println!(
                "outdated.corpus: [{}] {} ({})",
                corpus.corpus_kind,
                corpus.label,
                format_expiration(listing.now_unix, corpus.expires_at_unix)
            );
        }
        return Ok(());
    }

    println!("corpora.count: {}", listing.corpora.len());
    for corpus in &listing.corpora {
        println!(
            "corpus: [{}] {} profile={} version={} pages={} sections={} symbols={} examples={} status={}",
            corpus.corpus_kind,
            corpus.label,
            if corpus.source_profile.is_empty() {
                "<none>"
            } else {
                &corpus.source_profile
            },
            if corpus.source_version.is_empty() {
                "<none>"
            } else {
                &corpus.source_version
            },
            corpus.pages_count,
            corpus.sections_count,
            corpus.symbols_count,
            corpus.examples_count,
            format_expiration(listing.now_unix, corpus.expires_at_unix)
        );
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_docs_update(runtime: &RuntimeOptions) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let report = update_outdated_docs_with_config(&paths, &config)?;

    println!("docs update");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("updated_corpora: {}", report.updated_corpora);
    println!("updated_pages: {}", report.updated_pages);
    println!("updated_sections: {}", report.updated_sections);
    println!("updated_symbols: {}", report.updated_symbols);
    println!("updated_examples: {}", report.updated_examples);
    println!("request_count: {}", report.request_count);
    println!("failures.count: {}", report.failures.len());
    for failure in &report.failures {
        println!("failure: {failure}");
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
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
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if matches!(report.kind, DocsRemoveKind::NotFound) {
        bail!("documentation target not found: {target}");
    }
    Ok(())
}

fn run_docs_search(runtime: &RuntimeOptions, args: DocsSearchArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let format = normalize_output_format(&args.format)?;
    let hits = search_docs(
        &paths,
        &args.query,
        &DocsSearchOptions {
            tier: args.tier.clone(),
            profile: args.profile.clone(),
            limit: args.limit.max(1),
        },
    )?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }

    println!("docs search");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("query: {}", collapse_whitespace(&args.query));
    println!("tier: {}", args.tier.as_deref().unwrap_or("<all>"));
    println!("profile: {}", args.profile.as_deref().unwrap_or("<all>"));
    println!("limit: {}", args.limit.max(1));
    println!("hits.count: {}", hits.len());
    for hit in &hits {
        println!(
            "hit: [{}] {} page={} weight={}",
            hit.tier, hit.title, hit.page_title, hit.retrieval_weight
        );
        println!("hit.snippet: {}", hit.snippet);
    }
    Ok(())
}

fn run_docs_context(runtime: &RuntimeOptions, args: DocsContextArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let format = normalize_output_format(&args.format)?;
    let report = build_docs_context(
        &paths,
        &args.query,
        &DocsContextOptions {
            profile: args.profile.clone(),
            limit: args.limit.max(1),
            token_budget: args.token_budget.max(1),
        },
    )?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("docs context");
    println!("query: {}", report.query);
    println!("profile: {}", report.profile.as_deref().unwrap_or("<all>"));
    println!("token_estimate: {}", report.token_estimate);
    println!("pages.count: {}", report.pages.len());
    for page in &report.pages {
        println!("page: {} weight={}", page.title, page.retrieval_weight);
        println!("page.snippet: {}", page.snippet);
    }
    println!("sections.count: {}", report.sections.len());
    for section in &report.sections {
        println!(
            "section: page={} heading={} weight={}",
            section.page_title,
            section.section_heading.as_deref().unwrap_or("<lead>"),
            section.retrieval_weight
        );
        println!("section.summary: {}", section.summary_text);
    }
    println!("symbols.count: {}", report.symbols.len());
    for symbol in &report.symbols {
        println!(
            "symbol: [{}] {} page={} weight={}",
            symbol.symbol_kind, symbol.symbol_name, symbol.page_title, symbol.retrieval_weight
        );
        println!("symbol.summary: {}", symbol.summary_text);
    }
    println!("examples.count: {}", report.examples.len());
    for example in &report.examples {
        println!(
            "example: [{}] page={} lang={} weight={}",
            example.example_kind,
            example.page_title,
            example.language_hint,
            example.retrieval_weight
        );
        println!("example.summary: {}", example.summary_text);
    }
    Ok(())
}

fn run_docs_symbols(runtime: &RuntimeOptions, args: DocsSymbolsArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let format = normalize_output_format(&args.format)?;
    let hits = lookup_docs_symbols(
        &paths,
        &args.query,
        &DocsSymbolLookupOptions {
            kind: args.kind.clone(),
            profile: args.profile.clone(),
            limit: args.limit.max(1),
        },
    )?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }

    println!("docs symbols");
    println!("query: {}", collapse_whitespace(&args.query));
    println!("kind: {}", args.kind.as_deref().unwrap_or("<all>"));
    println!("profile: {}", args.profile.as_deref().unwrap_or("<all>"));
    println!("hits.count: {}", hits.len());
    for hit in &hits {
        println!(
            "symbol: [{}] {} page={} weight={}",
            hit.symbol_kind, hit.symbol_name, hit.page_title, hit.retrieval_weight
        );
        println!("symbol.summary: {}", hit.summary_text);
        if !hit.signature_text.is_empty() {
            println!("symbol.signature: {}", hit.signature_text);
        }
    }
    Ok(())
}

pub(crate) fn run_docs_generate_reference(args: DocsGenerateReferenceArgs) -> Result<()> {
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

fn normalize_output_format(value: &str) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized != "text" && normalized != "json" {
        bail!("unsupported docs format: {value} (expected text|json)");
    }
    Ok(normalized)
}

fn normalize_title_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = values
        .into_iter()
        .map(|value| normalize_title_query(&value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort_unstable_by_key(|value| value.to_ascii_lowercase());
    normalized.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    normalized
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
    if title.starts_with("Help:") {
        return TechnicalDocType::Help;
    }
    TechnicalDocType::Manual
}
