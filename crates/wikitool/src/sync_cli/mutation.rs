use anyhow::Result;
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{
    RemoteMutationReconciliationStatus, close_remote_mutation_with_config,
    list_remote_mutations_with_config, reconcile_remote_mutation_with_config,
    show_remote_mutation_with_config,
};

use crate::RuntimeOptions;
use crate::cli_support::{normalize_path, resolve_runtime_with_config};

use super::{MutationArgs, MutationCommand};

pub(crate) fn run_mutation(runtime: &RuntimeOptions, args: MutationArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    match args.command {
        MutationCommand::List { all, format } => {
            let report = list_remote_mutations_with_config(&paths, !all, &config)?;
            if format.is_json() {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("remote mutations");
                println!("project_root: {}", normalize_path(&paths.project_root));
                println!("target_api_url: {}", report.target_api_url);
                println!("unresolved_only: {}", report.unresolved_only);
                println!("count: {}", report.mutations.len());
                for mutation in report.mutations {
                    println!(
                        "- {:?} {} | {} | {} | {}",
                        mutation.operation,
                        mutation.mutation_id,
                        mutation.phase,
                        mutation.title,
                        mutation.detail.as_deref().unwrap_or("")
                    );
                }
            }
        }
        MutationCommand::Show {
            operation,
            mutation_id,
            format,
        } => {
            let receipt =
                show_remote_mutation_with_config(&paths, operation.into(), mutation_id, &config)?;
            if format.is_json() {
                println!("{}", serde_json::to_string_pretty(&receipt)?);
            } else {
                println!("remote mutation");
                println!("target_api_url: {}", receipt.target_api_url);
                println!("operation: {:?}", receipt.operation);
                println!("mutation_id: {}", receipt.mutation_id);
                println!("title: {}", receipt.title);
                println!("phase: {}", receipt.phase);
                println!("request_started: {}", receipt.request_started);
                println!(
                    "terminal_outcome: {}",
                    receipt.terminal_outcome.as_deref().unwrap_or("<none>")
                );
                println!(
                    "local_effect_status: {}",
                    receipt.local_effect_status.as_deref().unwrap_or("<none>")
                );
                println!("detail: {}", receipt.detail.as_deref().unwrap_or("<none>"));
                if let Some(closure) = receipt.closure {
                    println!("closure_id: {}", closure.closure_id);
                    println!("closure_previous_phase: {}", closure.previous_phase);
                    println!("closure_actor: {}", closure.actor);
                    println!("closure_reason: {}", closure.reason);
                    println!("closure_closed_at_unix: {}", closure.closed_at_unix);
                }
            }
        }
        MutationCommand::Reconcile {
            operation,
            mutation_id,
            format,
        } => {
            let report = reconcile_remote_mutation_with_config(
                &paths,
                operation.into(),
                mutation_id,
                &config,
            )?;
            let terminal = !matches!(
                report.status,
                RemoteMutationReconciliationStatus::StillAmbiguous
                    | RemoteMutationReconciliationStatus::ReconciliationRequired
            );
            if format.is_json() {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("remote mutation reconciliation");
                println!("target_api_url: {}", report.target_api_url);
                println!("operation: {:?}", report.operation);
                println!("mutation_id: {}", report.mutation_id);
                println!("title: {}", report.title);
                println!("status: {:?}", report.status);
                println!("detail: {}", report.detail);
                println!("request_count: {}", report.request_count);
            }
            if !terminal {
                anyhow::bail!(
                    "remote mutation {} remains {:?}; inspect the emitted receipt before retrying reconciliation",
                    report.mutation_id,
                    report.status
                );
            }
        }
        MutationCommand::Close {
            operation,
            mutation_id,
            actor,
            reason,
            confirm,
            format,
        } => {
            if !confirm {
                anyhow::bail!(
                    "mutation close requires --confirm; it preserves the uncertain receipt and blocks this title until a fresh target-bound pull"
                );
            }
            let report = close_remote_mutation_with_config(
                &paths,
                operation.into(),
                mutation_id,
                &actor,
                &reason,
                &config,
            )?;
            if format.is_json() {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("remote mutation operator closure");
                println!("target_api_url: {}", report.target_api_url);
                println!("operation: {:?}", report.operation);
                println!("mutation_id: {}", report.mutation_id);
                println!("closure_id: {}", report.closure_id);
                println!("title: {}", report.title);
                println!("previous_phase: {}", report.previous_phase);
                println!("terminal_outcome: {}", report.terminal_outcome);
                println!("actor: {}", report.actor);
                println!("reason: {}", report.reason);
                println!("next_action: {}", report.next_action);
            }
        }
    }
    Ok(())
}
