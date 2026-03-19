use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::Serialize;
use wikitool_core::config::{WikiConfig, WikiConfigPatch, load_config, patch_wiki_config};
use wikitool_core::delete::{DeleteOptions as LocalDeleteOptions, DeleteReport, delete_local_page};
use wikitool_core::filesystem::{ScanOptions, scan_stats};
use wikitool_core::runtime::{
    InitOptions, ResolvedPaths, ensure_runtime_ready_for_sync, init_layout, inspect_runtime,
};
use wikitool_core::sync::{
    DiffBaselineStatus, DiffChangeType, DiffOptions, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE,
    NS_TEMPLATE, PullOptions, PushOptions, RemoteDeleteStatus, SyncPlanChange, SyncPlanOptions,
    SyncPlanReport, SyncSelection, delete_remote_page_with_config, diff_local_against_sync,
    discover_custom_namespaces, plan_sync_changes_with_config, pull_from_remote_with_config,
    push_to_remote_with_config,
};

use crate::cli_support::{
    format_flag, normalize_path, print_scan_stats, resolve_runtime_paths,
    resolve_runtime_with_config,
};
use crate::query_cli::normalize_output;
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    #[arg(long, help = "Create templates/ during initialization")]
    pub(crate) templates: bool,
    #[arg(long, help = "Overwrite existing config/parser files")]
    pub(crate) force: bool,
    #[arg(long, help = "Skip writing .wikitool/config.toml")]
    pub(crate) no_config: bool,
    #[arg(long, help = "Skip writing parser config")]
    pub(crate) no_parser_config: bool,
}

#[derive(Debug, Args)]
pub(crate) struct PullArgs {
    #[arg(long, help = "Full refresh (ignore last pull timestamp)")]
    pub(crate) full: bool,
    #[arg(long, help = "Overwrite locally modified files during pull")]
    pub(crate) overwrite_local: bool,
    #[arg(short = 'c', long, value_name = "NAME", help = "Filter by category")]
    pub(crate) category: Option<String>,
    #[arg(long, help = "Pull templates instead of articles")]
    pub(crate) templates: bool,
    #[arg(long, help = "Pull Category: namespace pages")]
    pub(crate) categories: bool,
    #[arg(long, help = "Pull everything (articles, categories, and templates)")]
    pub(crate) all: bool,
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub(crate) format: String,
}

#[derive(Debug, Args)]
pub(crate) struct PushArgs {
    #[arg(long, value_name = "TEXT", help = "Edit summary for pushed changes")]
    pub(crate) summary: Option<String>,
    #[arg(long, help = "Preview push actions without writing to the wiki")]
    pub(crate) dry_run: bool,
    #[arg(long, help = "Force push even when remote timestamps diverge")]
    pub(crate) force: bool,
    #[arg(long, help = "Propagate local deletions to remote wiki pages")]
    pub(crate) delete: bool,
    #[arg(long, help = "Include template/module/mediawiki namespaces")]
    pub(crate) templates: bool,
    #[arg(long, help = "Limit push to Category namespace pages")]
    pub(crate) categories: bool,
    #[arg(long = "title", value_name = "TITLE")]
    pub(crate) titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    pub(crate) paths: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    pub(crate) titles_file: Option<PathBuf>,
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub(crate) format: String,
}

#[derive(Debug, Args)]
pub(crate) struct DiffArgs {
    #[arg(long, help = "Include template/module/mediawiki namespaces")]
    pub(crate) templates: bool,
    #[arg(long, help = "Limit diff to Category namespace pages")]
    pub(crate) categories: bool,
    #[arg(long, help = "Show hash-level details for modified entries")]
    pub(crate) verbose: bool,
    #[arg(
        long,
        help = "Render unified textual diffs against the last synced baseline"
    )]
    pub(crate) content: bool,
    #[arg(long = "title", value_name = "TITLE")]
    pub(crate) titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    pub(crate) paths: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    pub(crate) titles_file: Option<PathBuf>,
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub(crate) format: String,
}

#[derive(Debug, Args)]
pub(crate) struct StatusArgs {
    #[arg(long, help = "Only show modified")]
    pub(crate) modified: bool,
    #[arg(long, help = "Only show conflicts")]
    pub(crate) conflicts: bool,
    #[arg(long, help = "Include templates")]
    pub(crate) templates: bool,
    #[arg(long, help = "Limit status to Category namespace pages")]
    pub(crate) categories: bool,
    #[arg(long = "title", value_name = "TITLE")]
    pub(crate) titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    pub(crate) paths: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    pub(crate) titles_file: Option<PathBuf>,
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    pub(crate) format: String,
}

#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    pub(crate) title: String,
    #[arg(long, value_name = "TEXT", help = "Reason for deletion (required)")]
    pub(crate) reason: String,
    #[arg(long, help = "Skip backup (not recommended)")]
    pub(crate) no_backup: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Custom backup directory under .wikitool/"
    )]
    pub(crate) backup_dir: Option<PathBuf>,
    #[arg(long, help = "Preview deletion without making changes")]
    pub(crate) dry_run: bool,
}

#[derive(Debug, Serialize)]
struct PullJsonReport<'a> {
    project_root: String,
    full: bool,
    overwrite_local: bool,
    category: Option<&'a str>,
    templates: bool,
    categories: bool,
    all: bool,
    namespaces: Vec<i32>,
    report: &'a wikitool_core::sync::PullReport,
}

#[derive(Debug, Serialize)]
struct PushJsonReport<'a> {
    project_root: String,
    summary: &'a str,
    dry_run: bool,
    force: bool,
    delete: bool,
    templates: bool,
    categories: bool,
    selection: &'a SyncSelection,
    report: &'a wikitool_core::sync::PushReport,
}

#[derive(Debug, Serialize)]
struct StatusJsonReport {
    project_root: String,
    filters: StatusJsonFilters,
    sync_ledger_ready: bool,
    plan: Option<SyncPlanReport>,
    runtime: RuntimeStatusJson,
    scan: wikitool_core::filesystem::ScanStats,
}

#[derive(Debug, Serialize)]
struct StatusJsonFilters {
    modified: bool,
    conflicts: bool,
    templates: bool,
    categories: bool,
    selection: SyncSelection,
}

#[derive(Debug, Serialize)]
struct RuntimeStatusJson {
    project_root_exists: bool,
    wiki_content_exists: bool,
    templates_exists: bool,
    state_dir_exists: bool,
    data_dir_exists: bool,
    db_exists: bool,
    db_size_bytes: Option<u64>,
    config_exists: bool,
    parser_config_exists: bool,
    warnings: Vec<String>,
}

fn runtime_status_json(status: &wikitool_core::runtime::RuntimeStatus) -> RuntimeStatusJson {
    RuntimeStatusJson {
        project_root_exists: status.project_root_exists,
        wiki_content_exists: status.wiki_content_exists,
        templates_exists: status.templates_exists,
        state_dir_exists: status.state_dir_exists,
        data_dir_exists: status.data_dir_exists,
        db_exists: status.db_exists,
        db_size_bytes: status.db_size_bytes,
        config_exists: status.config_exists,
        parser_config_exists: status.parser_config_exists,
        warnings: status.warnings.clone(),
    }
}

fn load_sync_selection(
    titles: &[String],
    paths: &[String],
    titles_file: Option<&PathBuf>,
) -> Result<SyncSelection> {
    let mut loaded_titles = titles.to_vec();
    if let Some(titles_file) = titles_file {
        let content = fs::read_to_string(titles_file)
            .with_context(|| format!("failed to read {}", normalize_path(titles_file)))?;
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                loaded_titles.push(trimmed.to_string());
            }
        }
    }
    Ok(SyncSelection {
        titles: loaded_titles,
        paths: paths.to_vec(),
    })
}

pub(crate) fn run_init(runtime: &RuntimeOptions, args: InitArgs) -> Result<()> {
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
    let mut wrote_namespace_config = false;
    let mut discovered_namespaces = 0usize;
    let mut created_namespace_dirs = 0usize;
    let mut namespace_discovery_status = "skipped (--no-config)".to_string();
    let mut persisted_api_url = false;
    let mut persisted_wiki_url = false;

    if !args.no_config {
        let config = load_config(&paths.config_path)
            .with_context(|| format!("failed to load {}", normalize_path(&paths.config_path)))?;
        let resolved_api_url = config.api_url_owned();
        let resolved_wiki_url = config.wiki_url();
        let discovered = match discover_custom_namespaces(&config) {
            Ok(ns) => ns,
            Err(_) if config.api_url_owned().is_none() => {
                namespace_discovery_status = "skipped (no API URL configured)".to_string();
                Vec::new()
            }
            Err(err) => {
                namespace_discovery_status = format!("failed: {err:#}");
                Vec::new()
            }
        };
        let mut patch = WikiConfigPatch {
            set_url: None,
            set_api_url: None,
            set_custom_namespaces: Some(discovered.clone()),
        };
        if config.wiki.api_url.is_none() {
            patch.set_api_url = resolved_api_url;
            persisted_api_url = patch.set_api_url.is_some();
        }
        if config.wiki.url.is_none() {
            patch.set_url = resolved_wiki_url;
            persisted_wiki_url = patch.set_url.is_some();
        }
        wrote_namespace_config = patch_wiki_config(&paths.config_path, &patch)
            .with_context(|| format!("failed to update {}", normalize_path(&paths.config_path)))?;

        let refreshed = load_config(&paths.config_path)
            .with_context(|| format!("failed to load {}", normalize_path(&paths.config_path)))?;
        let created = materialize_custom_namespace_dirs(&paths, &refreshed)?;
        created_namespace_dirs = created.len();
        discovered_namespaces = discovered.len();
        if namespace_discovery_status.starts_with("skipped")
            || namespace_discovery_status.starts_with("failed")
        {
            // Keep the status set by the error handler above.
        } else {
            namespace_discovery_status = "ok".to_string();
        }
    }

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
    println!("namespace_discovery: {namespace_discovery_status}");
    println!("discovered_custom_namespaces: {discovered_namespaces}");
    println!("wrote_namespace_config: {wrote_namespace_config}");
    println!("created_namespace_dirs: {created_namespace_dirs}");
    println!("persisted_wiki_api_url: {persisted_api_url}");
    println!("persisted_wiki_url: {persisted_wiki_url}");
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

pub(crate) fn run_pull(runtime: &RuntimeOptions, args: PullArgs) -> Result<()> {
    let format = normalize_output(&args.format)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    let namespaces = pull_namespaces_from_args(&args, &config);
    let report = pull_from_remote_with_config(
        &paths,
        &PullOptions {
            namespaces: namespaces.clone(),
            category: args.category.clone(),
            full: args.full,
            overwrite_local: args.overwrite_local,
        },
        &config,
    )?;

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&PullJsonReport {
                project_root: normalize_path(&paths.project_root),
                full: args.full,
                overwrite_local: args.overwrite_local,
                category: args.category.as_deref(),
                templates: args.templates,
                categories: args.categories,
                all: args.all,
                namespaces,
                report: &report,
            })?
        );
        if report.success {
            return Ok(());
        }
        bail!("pull completed with {} error(s)", report.errors.len());
    }

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
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.success {
        Ok(())
    } else {
        bail!("pull completed with {} error(s)", report.errors.len())
    }
}

pub(crate) fn run_push(runtime: &RuntimeOptions, args: PushArgs) -> Result<()> {
    let format = normalize_output(&args.format)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;
    let selection = load_sync_selection(&args.titles, &args.paths, args.titles_file.as_ref())?;

    let summary = args
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "wikitool rust push".to_string());

    let report = push_to_remote_with_config(
        &paths,
        &PushOptions {
            summary: summary.clone(),
            dry_run: args.dry_run,
            force: args.force,
            delete: args.delete,
            include_templates: args.templates,
            categories_only: args.categories,
            selection: selection.clone(),
        },
        &config,
    )?;

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&PushJsonReport {
                project_root: normalize_path(&paths.project_root),
                summary: &summary,
                dry_run: args.dry_run,
                force: args.force,
                delete: args.delete,
                templates: args.templates,
                categories: args.categories,
                selection: &selection,
                report: &report,
            })?
        );
        if report.success {
            return Ok(());
        }
        if !report.conflicts.is_empty() && !args.force {
            bail!(
                "push blocked by {} conflict(s); rerun with --force after review",
                report.conflicts.len()
            );
        }
        bail!("push completed with {} error(s)", report.errors.len());
    }

    println!("push");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("summary: {summary}");
    println!("dry_run: {}", args.dry_run);
    println!("force: {}", args.force);
    println!("delete: {}", args.delete);
    println!("templates: {}", args.templates);
    println!("categories: {}", args.categories);
    if !selection.titles.is_empty() {
        println!("selection.titles: {}", selection.titles.join(" | "));
    }
    if !selection.paths.is_empty() {
        println!("selection.paths: {}", selection.paths.join(" | "));
    }
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
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
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

pub(crate) fn run_diff(runtime: &RuntimeOptions, args: DiffArgs) -> Result<()> {
    let format = normalize_output(&args.format)?;
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;
    let selection = load_sync_selection(&args.titles, &args.paths, args.titles_file.as_ref())?;

    let report = match diff_local_against_sync(
        &paths,
        &DiffOptions {
            include_templates: args.templates,
            categories_only: args.categories,
            include_content: args.content,
            selection: selection.clone(),
        },
    )? {
        Some(report) => report,
        None => {
            if format == "json" {
                println!(
                    "{}",
                    serde_json::json!({
                        "project_root": normalize_path(&paths.project_root),
                        "sync_ledger_ready": false,
                        "templates": args.templates,
                        "categories": args.categories,
                        "content": args.content,
                        "selection": selection,
                    })
                );
            } else {
                println!("diff");
                println!("project_root: {}", normalize_path(&paths.project_root));
                println!("templates: {}", args.templates);
                println!("categories: {}", args.categories);
                println!("content: {}", args.content);
                println!(
                    "diff.sync_ledger: <not built> (run `wikitool pull --full{}`)",
                    if args.templates { " --templates" } else { "" }
                );
                println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
                if runtime.diagnostics {
                    println!("\n[diagnostics]\n{}", paths.diagnostics());
                }
            }
            return Ok(());
        }
    };

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("diff");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("templates: {}", args.templates);
    println!("categories: {}", args.categories);
    println!("verbose: {}", args.verbose);
    println!("content: {}", args.content);
    if !selection.titles.is_empty() {
        println!("selection.titles: {}", selection.titles.join(" | "));
    }
    if !selection.paths.is_empty() {
        println!("selection.paths: {}", selection.paths.join(" | "));
    }

    println!("diff.new_local: {}", report.new_local);
    println!("diff.modified_local: {}", report.modified_local);
    println!("diff.deleted_local: {}", report.deleted_local);
    println!("diff.conflicts.count: {}", report.conflict_count);
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
                if args.content {
                    println!(
                        "diff.change.baseline_status: {}",
                        format_baseline_status(change.baseline_status.as_ref())
                    );
                }
            }
            if args.content
                && let Some(unified_diff) = &change.unified_diff
            {
                println!("diff.change.content:");
                print!("{unified_diff}");
            }
        }
    }

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

pub(crate) fn run_status(runtime: &RuntimeOptions, args: StatusArgs) -> Result<()> {
    let format = normalize_output(&args.format)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let status = inspect_runtime(&paths)?;
    let selection = load_sync_selection(&args.titles, &args.paths, args.titles_file.as_ref())?;
    let custom_folders: Vec<String> = config
        .wiki
        .custom_namespaces
        .iter()
        .map(|ns| ns.folder().to_string())
        .collect();
    let scan = scan_stats(
        &paths,
        &ScanOptions {
            include_content: true,
            include_templates: args.templates,
            custom_content_folders: custom_folders,
        },
    )?;
    let plan = plan_sync_changes_with_config(
        &paths,
        &SyncPlanOptions {
            include_templates: args.templates,
            categories_only: args.categories,
            include_deletes: true,
            include_remote_conflicts: args.conflicts,
            selection: selection.clone(),
        },
        &config,
    )?;

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&StatusJsonReport {
                project_root: normalize_path(&paths.project_root),
                filters: StatusJsonFilters {
                    modified: args.modified,
                    conflicts: args.conflicts,
                    templates: args.templates,
                    categories: args.categories,
                    selection,
                },
                sync_ledger_ready: plan.is_some(),
                plan,
                runtime: runtime_status_json(&status),
                scan,
            })?
        );
        return Ok(());
    }

    println!("status");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("filters.modified: {}", args.modified);
    println!("filters.conflicts: {}", args.conflicts);
    println!("filters.templates: {}", args.templates);
    println!("filters.categories: {}", args.categories);
    if !selection.titles.is_empty() {
        println!("selection.titles: {}", selection.titles.join(" | "));
    }
    if !selection.paths.is_empty() {
        println!("selection.paths: {}", selection.paths.join(" | "));
    }

    if let Some(plan) = &plan {
        println!("status.new_local: {}", plan.new_local);
        println!("status.modified_local: {}", plan.modified_local);
        println!("status.deleted_local: {}", plan.deleted_local);
        println!("status.total: {}", plan.changes.len());
        println!("status.conflicts.checked: {}", args.conflicts);
        println!("status.conflicts.count: {}", plan.conflict_count);

        let display_changes = status_display_changes(plan, args.modified, args.conflicts);
        if display_changes.is_empty() {
            println!("status.changes: <none>");
        } else {
            for change in display_changes {
                println!(
                    "status.change: type={} title={} path={} conflict={}",
                    format_diff_change_type(&change.change_type),
                    change.title,
                    change.relative_path,
                    change.remote_conflict
                );
            }
        }
    } else {
        println!("status.sync_ledger: <not built>");
    }

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
    print_scan_stats("scan", &scan);
    if !status.warnings.is_empty() {
        println!("warnings:");
        for warning in &status.warnings {
            println!("  - {warning}");
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

pub(crate) fn run_delete(runtime: &RuntimeOptions, args: DeleteArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
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
            reason: args.reason.clone(),
            no_backup: args.no_backup,
            backup_dir: args.backup_dir,
            dry_run: args.dry_run,
        },
    )?;
    print_delete_report(&report);

    if args.dry_run {
        println!("remote_delete: dry_run");
    } else {
        let remote = delete_remote_page_with_config(&args.title, &args.reason, &config)?;
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
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn materialize_custom_namespace_dirs(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<Vec<PathBuf>> {
    let mut created = Vec::new();
    for namespace in &config.wiki.custom_namespaces {
        let folder = namespace.folder().trim();
        if folder.is_empty() {
            continue;
        }
        let namespace_dir = paths.wiki_content_dir.join(folder);
        if !namespace_dir.exists() {
            fs::create_dir_all(&namespace_dir)
                .with_context(|| format!("failed to create {}", normalize_path(&namespace_dir)))?;
            created.push(namespace_dir.clone());
        }
        let redirects = namespace_dir.join("_redirects");
        if !redirects.exists() {
            fs::create_dir_all(&redirects)
                .with_context(|| format!("failed to create {}", normalize_path(&redirects)))?;
            created.push(redirects);
        }
    }
    Ok(created)
}

fn pull_namespaces_from_args(args: &PullArgs, config: &WikiConfig) -> Vec<i32> {
    if args.templates {
        return vec![NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
    }
    if args.categories {
        return vec![NS_CATEGORY];
    }
    if args.all {
        let mut namespaces = vec![NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
        for custom in &config.wiki.custom_namespaces {
            if custom.id >= 0 {
                namespaces.push(custom.id);
            }
        }
        namespaces.sort_unstable();
        namespaces.dedup();
        return namespaces;
    }
    vec![NS_MAIN]
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

fn format_baseline_status(value: Option<&DiffBaselineStatus>) -> &'static str {
    match value {
        Some(DiffBaselineStatus::Available) => "available",
        Some(DiffBaselineStatus::MissingSnapshot) => "missing_snapshot",
        Some(DiffBaselineStatus::NotApplicable) => "not_applicable",
        None => "<none>",
    }
}

fn format_diff_change_type(value: &DiffChangeType) -> &'static str {
    match value {
        DiffChangeType::NewLocal => "new_local",
        DiffChangeType::ModifiedLocal => "modified_local",
        DiffChangeType::DeletedLocal => "deleted_local",
    }
}

fn status_display_changes(
    plan: &SyncPlanReport,
    modified_only: bool,
    conflicts_only: bool,
) -> Vec<&SyncPlanChange> {
    plan.changes
        .iter()
        .filter(|change| {
            if conflicts_only && !change.remote_conflict {
                return false;
            }
            if modified_only && conflicts_only {
                return true;
            }
            if modified_only {
                return true;
            }
            true
        })
        .collect()
}
