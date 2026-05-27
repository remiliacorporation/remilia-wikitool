use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{PullOptions, PullReport, pull_from_remote_with_config};

use crate::cli_support::{normalize_path, print_scan_stats, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::PullArgs;
use super::shared::pull_namespaces_from_args;

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
    report: &'a PullReport,
}

pub(crate) fn run_pull(runtime: &RuntimeOptions, args: PullArgs) -> Result<()> {
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

    if args.format.is_json() {
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
