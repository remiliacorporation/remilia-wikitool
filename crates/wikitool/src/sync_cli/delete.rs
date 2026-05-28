use anyhow::Result;
use serde::Serialize;
use wikitool_core::delete::{DeleteOptions as LocalDeleteOptions, DeleteReport, delete_local_page};
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{RemoteDeleteStatus, delete_remote_page_with_config};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::DeleteArgs;

#[derive(Serialize)]
struct DeleteCommandJson {
    project_root: String,
    title: String,
    reason: String,
    dry_run: bool,
    backup_enabled: bool,
    backup_dir: Option<String>,
    local: DeleteReport,
    remote: DeleteRemoteJson,
}

#[derive(Serialize)]
struct DeleteRemoteJson {
    status: &'static str,
    request_count: Option<usize>,
    detail: Option<String>,
}

fn remote_status_label(status: &RemoteDeleteStatus) -> &'static str {
    match status {
        RemoteDeleteStatus::Deleted => "deleted",
        RemoteDeleteStatus::AlreadyMissing => "already_missing",
        RemoteDeleteStatus::SkippedMissingCredentials => "skipped_missing_credentials",
    }
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

pub(crate) fn run_delete(runtime: &RuntimeOptions, args: DeleteArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    let backup_dir_display = args.backup_dir.as_ref().map(normalize_path);
    let report = delete_local_page(
        &paths,
        &args.title,
        &LocalDeleteOptions {
            reason: args.reason.clone(),
            no_backup: args.no_backup,
            backup_dir: args.backup_dir.clone(),
            dry_run: args.dry_run,
        },
    )?;

    let remote = if args.dry_run {
        DeleteRemoteJson {
            status: "dry_run",
            request_count: None,
            detail: None,
        }
    } else {
        let outcome = delete_remote_page_with_config(&args.title, &args.reason, &config)?;
        DeleteRemoteJson {
            status: remote_status_label(&outcome.status),
            request_count: Some(outcome.request_count),
            detail: outcome.detail.clone(),
        }
    };

    if args.format.is_json() {
        let output = DeleteCommandJson {
            project_root: normalize_path(&paths.project_root),
            title: args.title.clone(),
            reason: args.reason.clone(),
            dry_run: args.dry_run,
            backup_enabled: !args.no_backup,
            backup_dir: backup_dir_display,
            local: report,
            remote,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("delete");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("title: {}", args.title);
    println!("reason: {}", args.reason);
    println!("dry_run: {}", args.dry_run);
    println!("backup_enabled: {}", !args.no_backup);
    if let Some(backup_dir) = &backup_dir_display {
        println!("backup_dir: {backup_dir}");
    }
    print_delete_report(&report);
    if remote.status == "dry_run" {
        println!("remote_delete: dry_run");
    } else {
        println!("remote_delete: {}", remote.status);
        if let Some(request_count) = remote.request_count {
            println!("remote_delete.request_count: {request_count}");
        }
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
