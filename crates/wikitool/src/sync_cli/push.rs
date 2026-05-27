use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{PushOptions, PushReport, SyncSelection, push_to_remote_with_config};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::PushArgs;
use super::shared::load_sync_selection;

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
    report: &'a PushReport,
}

pub(crate) fn run_push(runtime: &RuntimeOptions, args: PushArgs) -> Result<()> {
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

    if args.format.is_json() {
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
