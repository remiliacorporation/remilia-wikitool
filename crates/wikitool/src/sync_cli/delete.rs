use anyhow::Result;
use wikitool_core::delete::{DeleteOptions as LocalDeleteOptions, DeleteReport, delete_local_page};
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{RemoteDeleteStatus, delete_remote_page_with_config};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::DeleteArgs;

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
