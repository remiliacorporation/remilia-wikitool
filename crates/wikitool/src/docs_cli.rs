use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Command, CommandFactory, error::ErrorKind};
use wikitool_core::docs::{
    DocsImportOptions, DocsImportTechnicalOptions, DocsListOptions, DocsRemoveKind,
    TechnicalDocType, TechnicalImportTask, discover_installed_extensions_from_wiki_with_config,
    format_expiration, import_docs_bundle, import_extension_docs, import_technical_docs, list_docs,
    remove_docs, search_docs, update_outdated_docs,
};

use crate::{
    Cli, DocsGenerateReferenceArgs, DocsImportArgs, DocsImportTechnicalArgs, DocsListArgs,
    DocsSubcommand, MIGRATIONS_POLICY_MESSAGE, RuntimeOptions,
    cli_support::{
        collapse_whitespace, format_flag, normalize_path, normalize_title_query,
        resolve_runtime_paths, resolve_runtime_with_config,
    },
};

pub(crate) fn run_docs(runtime: &RuntimeOptions, command: DocsSubcommand) -> Result<()> {
    match command {
        DocsSubcommand::Import(args) => run_docs_import(runtime, args),
        DocsSubcommand::ImportTechnical(args) => run_docs_import_technical(runtime, args),
        DocsSubcommand::GenerateReference(args) => run_docs_generate_reference(args),
        DocsSubcommand::List(args) => run_docs_list(runtime, args),
        DocsSubcommand::Update => run_docs_update(runtime),
        DocsSubcommand::Remove { target } => run_docs_remove(runtime, &target),
        DocsSubcommand::Search { query, tier, limit } => {
            run_docs_search(runtime, &query, tier.as_deref(), limit)
        }
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
        println!("failures.count: {}", report.failures.len());
        for failure in &report.failures {
            println!("failure: {failure}");
        }
        println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
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
        let discovered = discover_installed_extensions_from_wiki_with_config(&config)
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
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
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
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
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
        println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
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
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
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
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
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
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
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
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
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
