use anyhow::Result;
use serde::Serialize;
use wikitool_core::filesystem::{ScanOptions, ScanStats, scan_stats};
use wikitool_core::runtime::inspect_runtime;
use wikitool_core::sync::{
    SyncPlanOptions, SyncPlanReport, SyncSelection, plan_sync_changes_with_config,
};

use crate::cli_support::{
    format_flag, normalize_path, print_scan_stats, resolve_runtime_with_config,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::StatusArgs;
use super::shared::{
    RuntimeStatusJson, format_diff_change_type, load_sync_selection, runtime_status_json,
    status_display_changes,
};

#[derive(Debug, Serialize)]
struct StatusJsonReport {
    project_root: String,
    filters: StatusJsonFilters,
    sync_ledger_ready: bool,
    plan: Option<SyncPlanReport>,
    runtime: RuntimeStatusJson,
    scan: ScanStats,
}

#[derive(Debug, Serialize)]
struct StatusJsonFilters {
    modified: bool,
    conflicts: bool,
    templates: bool,
    categories: bool,
    selection: SyncSelection,
}

pub(crate) fn run_status(runtime: &RuntimeOptions, args: StatusArgs) -> Result<()> {
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

    if args.format.is_json() {
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
