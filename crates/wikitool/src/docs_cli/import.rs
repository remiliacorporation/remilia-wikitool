use super::*;

#[derive(Debug, Args)]
pub(super) struct DocsImportArgs {
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
pub(super) struct DocsImportTechnicalArgs {
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
pub(super) struct DocsImportProfileArgs {
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

pub(super) fn run_docs_import(runtime: &RuntimeOptions, args: DocsImportArgs) -> Result<()> {
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

pub(super) fn run_docs_import_technical(
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

pub(super) fn run_docs_import_profile(
    runtime: &RuntimeOptions,
    args: DocsImportProfileArgs,
) -> Result<()> {
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
