use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::delete::{plan_local_delete_effect, remove_deleted_page_from_index};
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{
    RemoteDeleteError, RemoteDeleteReport, RemoteDeleteStatus, apply_remote_delete_with_config,
    plan_remote_delete_with_config,
};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::DeleteArgs;

#[derive(Serialize)]
struct DeleteApplyOutput<'a> {
    success: bool,
    mode: &'static str,
    project_root: String,
    target_api_url: &'a str,
    title: &'a str,
    reason: &'a str,
    plan_id: &'a str,
    local_effect: &'a wikitool_core::delete::LocalDeleteEffectPlan,
    remote: &'a RemoteDeleteReport,
    deleted_index_rows: usize,
    catalog_cleanup_status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    catalog_cleanup_warning: Option<&'a str>,
}

#[derive(Serialize)]
struct DeleteErrorOutput<'a> {
    success: bool,
    status: &'static str,
    target_api_url: Option<&'a str>,
    mutation_id: Option<i64>,
    title: Option<&'a str>,
    detail: String,
    next_action: Option<DeleteNextAction>,
}

#[derive(Serialize)]
struct DeleteNextAction {
    command: &'static str,
    operation: &'static str,
    mutation_id: i64,
}

pub(crate) fn run_delete(runtime: &RuntimeOptions, args: DeleteArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;
    if args.title.trim().is_empty() {
        bail!("delete requires a non-empty title");
    }
    if args.reason.trim().is_empty() {
        bail!("delete requires a non-empty reason");
    }

    let local = plan_local_delete_effect(
        &paths,
        &args.title,
        args.no_backup,
        args.backup_dir.as_deref(),
    )?;
    let Some(plan_id) = args.apply.as_deref() else {
        let plan = plan_remote_delete_with_config(
            &paths,
            &args.title,
            &args.reason,
            local.policy.clone(),
            &config,
        )?;
        if args.format.is_json() {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "success": true,
                    "mode": "plan",
                    "project_root": normalize_path(&paths.project_root),
                    "plan": plan,
                    "local_effect": local,
                    "next_action": {
                        "command": "wikitool delete",
                        "apply_plan_id": plan.plan_id,
                    }
                }))?
            );
        } else {
            println!("delete plan");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("target_api_url: {}", plan.target_api_url);
            println!("title: {}", plan.canonical_title);
            println!("observed_state: {:?}", plan.observed_state);
            println!(
                "observed_revision_id: {}",
                plan.observed_revision_id
                    .map(|value| value.to_string())
                    .as_deref()
                    .unwrap_or("<none>")
            );
            println!("plan_id: {}", plan.plan_id);
            println!(
                "local_relative_path: {}",
                local.relative_path.as_deref().unwrap_or("<remote-only>")
            );
            println!(
                "backup_path: {}",
                local.policy.backup_path.as_deref().unwrap_or("<none>")
            );
            println!("apply: rerun with --apply {}", plan.plan_id);
        }
        return Ok(());
    };

    let remote = match apply_remote_delete_with_config(
        &paths,
        &args.title,
        &args.reason,
        local.policy.clone(),
        plan_id,
        &config,
    ) {
        Ok(report) => report,
        Err(error) => {
            if args.format.is_json() {
                print_structured_delete_error(&error)?;
            }
            return Err(error);
        }
    };
    let applied_success = matches!(
        remote.status,
        RemoteDeleteStatus::Deleted | RemoteDeleteStatus::AlreadyMissing
    );
    let (deleted_index_rows, catalog_cleanup_status, catalog_cleanup_warning) = if applied_success {
        match remove_deleted_page_from_index(&paths, local.relative_path.as_deref()) {
            Ok(rows) => (rows, "complete", None),
            Err(error) => (
                0,
                "warning",
                Some(format!(
                    "durable remote and sync authority completed, but derived catalog cleanup failed: {error:#}; rebuild the local catalog"
                )),
            ),
        }
    } else {
        (0, "not_attempted", None)
    };

    if args.format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&DeleteApplyOutput {
                success: applied_success,
                mode: "apply",
                project_root: normalize_path(&paths.project_root),
                target_api_url: &remote.target_api_url,
                title: &remote.title,
                reason: &args.reason,
                plan_id,
                local_effect: &local,
                remote: &remote,
                deleted_index_rows,
                catalog_cleanup_status,
                catalog_cleanup_warning: catalog_cleanup_warning.as_deref(),
            })?
        );
        if !applied_success {
            bail!(
                "delete apply ended as {:?}; local sync authority was retained",
                remote.status
            );
        }
        return Ok(());
    }

    println!("delete apply");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("target_api_url: {}", remote.target_api_url);
    println!("title: {}", remote.title);
    println!("status: {:?}", remote.status);
    println!(
        "mutation_id: {}",
        remote
            .mutation_id
            .map(|value| value.to_string())
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "deletion_log_id: {}",
        remote
            .deletion_log_id
            .map(|value| value.to_string())
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "backup_path: {}",
        local.policy.backup_path.as_deref().unwrap_or("<none>")
    );
    println!("deleted_index_rows: {deleted_index_rows}");
    println!("catalog_cleanup_status: {catalog_cleanup_status}");
    if let Some(warning) = &catalog_cleanup_warning {
        println!("catalog_cleanup_warning: {warning}");
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if !applied_success {
        bail!(
            "delete apply ended as {:?}; local sync authority was retained",
            remote.status
        );
    }
    Ok(())
}

fn print_structured_delete_error(error: &anyhow::Error) -> Result<()> {
    let Some(remote_error) = error.downcast_ref::<RemoteDeleteError>() else {
        println!(
            "{}",
            serde_json::to_string_pretty(&DeleteErrorOutput {
                success: false,
                status: "error",
                target_api_url: None,
                mutation_id: None,
                title: None,
                detail: format!("{error:#}"),
                next_action: None,
            })?
        );
        return Ok(());
    };
    let (status, target_api_url, mutation_id, title, next_action) = match remote_error {
        RemoteDeleteError::OutcomeAmbiguous {
            mutation_id,
            target_api_url,
            title,
            ..
        } => (
            "outcome_ambiguous",
            Some(target_api_url.as_str()),
            Some(*mutation_id),
            Some(title.as_str()),
            Some(DeleteNextAction {
                command: "wikitool mutation reconcile",
                operation: "delete",
                mutation_id: *mutation_id,
            }),
        ),
        RemoteDeleteError::ReconciliationRequired {
            mutation_id,
            target_api_url,
            title,
            ..
        } => (
            "reconciliation_required",
            Some(target_api_url.as_str()),
            Some(*mutation_id),
            Some(title.as_str()),
            Some(DeleteNextAction {
                command: "wikitool mutation reconcile",
                operation: "delete",
                mutation_id: *mutation_id,
            }),
        ),
        RemoteDeleteError::MissingCredentials => ("missing_credentials", None, None, None, None),
        RemoteDeleteError::MissingApplyPlanId => ("missing_plan_id", None, None, None, None),
        RemoteDeleteError::PlanMismatch { .. } => ("plan_mismatch", None, None, None, None),
        RemoteDeleteError::NotApplied {
            mutation_id,
            target_api_url,
            title,
            ..
        } => (
            "not_applied",
            Some(target_api_url.as_str()),
            Some(*mutation_id),
            Some(title.as_str()),
            Some(DeleteNextAction {
                command: "wikitool mutation show",
                operation: "delete",
                mutation_id: *mutation_id,
            }),
        ),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&DeleteErrorOutput {
            success: false,
            status,
            target_api_url,
            mutation_id,
            title,
            detail: remote_error.to_string(),
            next_action,
        })?
    );
    Ok(())
}
