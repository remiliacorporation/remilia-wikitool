use std::error::Error;
use std::fmt;
use std::fs;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::storage::{
    DeleteMutationIntent, NewDeleteMutation, advance_verified_delete_mutation,
    begin_delete_mutation, bind_delete_response, complete_delete_mutation_without_state_change,
    ensure_no_unresolved_remote_mutation, load_delete_cleanup_identity, load_delete_mutation,
    mark_delete_mutation_unresolved, mark_delete_request_started, normalized_title_key,
    open_existing_sync_connection, prepare_delete_local_effects, validated_delete_source_path,
};
use crate::support::compute_sha256;
use crate::{
    DeleteOutcome, DeleteReconciliationReport, DeleteReconciliationStatus,
    RemoteDeleteApplyRequest, RemoteDeleteCleanupIdentity, RemoteDeleteLocalEffectPolicy,
    RemoteDeleteObservedState, RemoteDeletePlan, RemoteDeleteReport, RemoteDeleteStatus,
    RemotePage, SyncProjectPaths, SyncTargetIdentity, WikiWriteApi,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteDeleteError {
    MissingCredentials,
    MissingApplyPlanId,
    PlanMismatch {
        expected: String,
        supplied: String,
    },
    NotApplied {
        mutation_id: i64,
        target_api_url: String,
        title: String,
        detail: String,
    },
    OutcomeAmbiguous {
        mutation_id: i64,
        target_api_url: String,
        title: String,
        detail: String,
    },
    ReconciliationRequired {
        mutation_id: i64,
        target_api_url: String,
        title: String,
        detail: String,
    },
}

impl fmt::Display for RemoteDeleteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCredentials => write!(
                formatter,
                "remote delete apply requires MediaWiki credentials"
            ),
            Self::MissingApplyPlanId => write!(
                formatter,
                "remote delete defaults to preview; apply requires the exact preview plan ID"
            ),
            Self::PlanMismatch { expected, supplied } => write!(
                formatter,
                "remote delete plan changed: current plan is {expected}, supplied plan was {supplied}; inspect the new preview before applying"
            ),
            Self::NotApplied {
                mutation_id,
                target_api_url,
                title,
                detail,
            } => write!(
                formatter,
                "remote delete mutation {mutation_id} for {title} on {target_api_url} was not applied: {detail}"
            ),
            Self::OutcomeAmbiguous {
                mutation_id,
                target_api_url,
                title,
                detail,
            } => write!(
                formatter,
                "remote delete mutation {mutation_id} for {title} on {target_api_url} has an ambiguous outcome: {detail}"
            ),
            Self::ReconciliationRequired {
                mutation_id,
                target_api_url,
                title,
                detail,
            } => write!(
                formatter,
                "remote delete mutation {mutation_id} for {title} on {target_api_url} requires reconciliation: {detail}"
            ),
        }
    }
}

impl Error for RemoteDeleteError {}

#[derive(Serialize)]
struct DeletePlanIdentity<'a> {
    schema: &'static str,
    target_api_url: &'a str,
    requested_title: &'a str,
    canonical_title: &'a str,
    observed_state: &'a RemoteDeleteObservedState,
    observed_revision_id: Option<i64>,
    observed_timestamp: Option<&'a str>,
    reason: &'a str,
    local_cleanup: &'a Option<RemoteDeleteCleanupIdentity>,
    local_effect_policy: &'a RemoteDeleteLocalEffectPolicy,
}

/// Observe the exact target-bound remote and local state and return a stable
/// plan. This function never authenticates, writes MediaWiki, or changes local
/// sync authority.
pub fn plan_remote_delete_with_api<A: WikiWriteApi>(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    api: &mut A,
    title: &str,
    reason: &str,
    local_effect_policy: RemoteDeleteLocalEffectPolicy,
) -> Result<RemoteDeletePlan> {
    target.ensure_matches_api(api.target_api_url())?;
    let connection = open_existing_sync_connection(paths, target)?;
    ensure_no_unresolved_remote_mutation(&connection, title)?;
    plan_remote_delete_from_observation(
        paths,
        target,
        api,
        &connection,
        title,
        reason,
        local_effect_policy,
    )
}

/// Apply only the exact plan returned by [`plan_remote_delete_with_api`]. The
/// authentication round trip intentionally happens before the final remote
/// observation so token latency is outside the delete TOCTOU window.
pub fn apply_remote_delete_with_api<A: WikiWriteApi>(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    api: &mut A,
    request: RemoteDeleteApplyRequest<'_>,
) -> Result<RemoteDeleteReport> {
    let supplied_plan = request
        .plan_id
        .filter(|value| !value.trim().is_empty())
        .ok_or(RemoteDeleteError::MissingApplyPlanId)?;
    let (username, password) = request
        .credentials
        .ok_or(RemoteDeleteError::MissingCredentials)?;
    let title = request.title;
    let reason = request.reason;

    target.ensure_matches_api(api.target_api_url())?;
    let connection = open_existing_sync_connection(paths, target)?;
    ensure_no_unresolved_remote_mutation(&connection, title)?;
    api.login(username, password)?;

    let plan = plan_remote_delete_from_observation(
        paths,
        target,
        api,
        &connection,
        title,
        reason,
        request.local_effect_policy,
    )?;
    if plan.plan_id != supplied_plan {
        return Err(RemoteDeleteError::PlanMismatch {
            expected: plan.plan_id,
            supplied: supplied_plan.to_string(),
        }
        .into());
    }

    let observed_revision_id = plan.observed_revision_id.unwrap_or(0);
    let mut intent = begin_delete_mutation(
        paths,
        &connection,
        target,
        NewDeleteMutation {
            title: &plan.canonical_title,
            relative_path: plan
                .local_cleanup
                .as_ref()
                .map(|cleanup| cleanup.relative_path.as_str()),
            expected_revision_id: observed_revision_id,
            reason,
            local_effect_policy: &plan.local_effect_policy,
        },
    )?;
    if let Err(error) = prepare_delete_local_effects(paths, &connection, &intent) {
        return Err(not_applied_error(
            paths,
            &connection,
            target,
            &intent,
            format!("local delete effects could not be prepared before request start: {error:#}"),
        ));
    }
    if intent.local_effect_status == "pending" {
        intent.local_effect_status = "backup_ready".to_string();
    }

    let immediate = match observe_remote_page(api, &plan.canonical_title) {
        Ok(page) => page,
        Err(error) => {
            return Err(not_applied_error(
                paths,
                &connection,
                target,
                &intent,
                format!("final pre-request remote observation failed: {error:#}"),
            ));
        }
    };
    if plan.observed_state == RemoteDeleteObservedState::Missing {
        if let Some(current) = immediate {
            let detail = format!(
                "the page appeared as revision {} after the bound missing-page plan; no delete request was sent and local authority was retained",
                current.revision_id
            );
            if let Err(error) = complete_delete_mutation_without_state_change(
                paths,
                &connection,
                target,
                &intent,
                "remote_present_after_missing",
                &detail,
            ) {
                return Err(reconciliation_required_error(
                    &connection,
                    target,
                    &intent,
                    format!("{detail}; failed to persist terminal outcome: {error:#}"),
                ));
            }
            return Ok(RemoteDeleteReport {
                status: RemoteDeleteStatus::RemotePresentAfterMissing,
                target_api_url: target.api_url().to_string(),
                title: current.title,
                mutation_id: Some(intent.mutation_id),
                detail: Some(detail),
                observed_revision_id: Some(current.revision_id),
                deletion_log_id: None,
                request_count: api.request_count(),
            });
        }
        if let Err(error) = bind_delete_response(
            &connection,
            &intent,
            "already_missing",
            &plan.canonical_title,
            None,
            None,
        ) {
            return Err(not_applied_error(
                paths,
                &connection,
                target,
                &intent,
                format!("verified remote absence could not be bound durably: {error:#}"),
            ));
        }
        if let Err(error) = advance_verified_delete_mutation(paths, &connection, target, &intent) {
            return Err(reconciliation_required_error(
                &connection,
                target,
                &intent,
                format!("verified remote absence could not finish local effects: {error:#}"),
            ));
        }
        return Ok(RemoteDeleteReport {
            status: RemoteDeleteStatus::AlreadyMissing,
            target_api_url: target.api_url().to_string(),
            title: plan.canonical_title,
            mutation_id: Some(intent.mutation_id),
            detail: Some(
                "target-bound apply re-observed the page as absent; no delete request was sent"
                    .to_string(),
            ),
            observed_revision_id: None,
            deletion_log_id: None,
            request_count: api.request_count(),
        });
    }
    let Some(immediate) = immediate else {
        let detail = "the page disappeared after the bound plan and before request start; no delete request was sent";
        return Err(not_applied_error(
            paths,
            &connection,
            target,
            &intent,
            format!("{detail}; preview the new remote state before applying again"),
        ));
    };
    if immediate.revision_id != observed_revision_id {
        let detail = format!(
            "remote revision changed from planned {observed_revision_id} to {} before request start; no delete request was sent",
            immediate.revision_id
        );
        return Err(not_applied_error(
            paths,
            &connection,
            target,
            &intent,
            format!("{detail}; preview the new remote state before applying again"),
        ));
    }
    if let Err(error) = mark_delete_request_started(&connection, &intent) {
        return Err(not_applied_error(
            paths,
            &connection,
            target,
            &intent,
            format!("durable request-start marker failed; no request was sent: {error:#}"),
        ));
    }

    let outcome = match api.delete_page(&plan.canonical_title, &intent.request_reason()) {
        Ok(outcome) => outcome,
        Err(error) => {
            let detail = format!(
                "delete request failed after its durable request-start marker: {error:#}; the request will not be retried"
            );
            let persistence_error =
                persist_unresolved(&connection, &intent, "outcome_ambiguous", &detail).err();
            let detail = append_persistence_error(detail, persistence_error);
            return Err(RemoteDeleteError::OutcomeAmbiguous {
                mutation_id: intent.mutation_id,
                target_api_url: target.api_url().to_string(),
                title: intent.title,
                detail,
            }
            .into());
        }
    };

    let (response_kind, response_title, deletion_log_id) = match outcome {
        DeleteOutcome::Deleted(receipt) => {
            if let Err(error) = require_same_title(&receipt.title, &plan.canonical_title) {
                return Err(reconciliation_required_error(
                    &connection,
                    target,
                    &intent,
                    format!("typed delete receipt failed title-identity validation: {error:#}"),
                ));
            }
            ("deleted", receipt.title, Some(receipt.log_id))
        }
        DeleteOutcome::AlreadyMissing => ("already_missing", plan.canonical_title.clone(), None),
    };
    if let Err(error) = bind_delete_response(
        &connection,
        &intent,
        response_kind,
        &response_title,
        deletion_log_id,
        None,
    ) {
        return Err(reconciliation_required_error(
            &connection,
            target,
            &intent,
            format!("typed delete response could not be bound durably: {error:#}"),
        ));
    }

    let current = match observe_remote_page(api, &response_title) {
        Ok(current) => current,
        Err(error) => {
            let detail = format!(
                "MediaWiki returned a typed {response_kind} response, but the immediate post-delete absence check failed: {error:#}"
            );
            return Err(reconciliation_required_error(
                &connection,
                target,
                &intent,
                detail,
            ));
        }
    };
    if let Some(current) = current {
        if response_kind == "deleted" && current.revision_id == observed_revision_id {
            let Some(log_id) = deletion_log_id else {
                return Err(reconciliation_required_error(
                    &connection,
                    target,
                    &intent,
                    "typed deleted response is missing its deletion log ID".to_string(),
                ));
            };
            let detail = format!(
                "MediaWiki returned deletion log id {}, but the immediate read still exposes expected revision {observed_revision_id}; replica lag prevents proving current absence",
                log_id
            );
            return Err(reconciliation_required_error(
                &connection,
                target,
                &intent,
                detail,
            ));
        }
        let (terminal_outcome, status, detail) = if response_kind == "deleted" {
            (
                "applied_then_recreated",
                RemoteDeleteStatus::AppliedThenRecreated,
                format!(
                    "MediaWiki returned a typed delete receipt, but revision {} is present after the response; local sync authority was retained",
                    current.revision_id
                ),
            )
        } else {
            (
                "remote_present_after_missing",
                RemoteDeleteStatus::RemotePresentAfterMissing,
                format!(
                    "MediaWiki reported missingtitle, then revision {} was observed; this request did not delete the page and local sync authority was retained",
                    current.revision_id
                ),
            )
        };
        if let Err(error) = complete_delete_mutation_without_state_change(
            paths,
            &connection,
            target,
            &intent,
            terminal_outcome,
            &detail,
        ) {
            return Err(reconciliation_required_error(
                &connection,
                target,
                &intent,
                format!("{detail}; failed to persist terminal outcome: {error:#}"),
            ));
        }
        return Ok(RemoteDeleteReport {
            status,
            target_api_url: target.api_url().to_string(),
            title: current.title,
            mutation_id: Some(intent.mutation_id),
            detail: Some(detail),
            observed_revision_id: Some(current.revision_id),
            deletion_log_id,
            request_count: api.request_count(),
        });
    }

    if let Err(error) = advance_verified_delete_mutation(paths, &connection, target, &intent) {
        return Err(reconciliation_required_error(
            &connection,
            target,
            &intent,
            format!("verified delete could not finish local effects and sync state: {error:#}"),
        ));
    }
    Ok(RemoteDeleteReport {
        status: if response_kind == "deleted" {
            RemoteDeleteStatus::Deleted
        } else {
            RemoteDeleteStatus::AlreadyMissing
        },
        target_api_url: target.api_url().to_string(),
        title: response_title,
        mutation_id: Some(intent.mutation_id),
        detail: None,
        observed_revision_id: Some(observed_revision_id),
        deletion_log_id,
        request_count: api.request_count(),
    })
}

/// Reconcile one durable delete mutation without ever replaying the write.
/// A visible marker scan is completed before current-page inspection so an
/// applied-then-recreated delete is not mistaken for a never-applied request.
pub fn reconcile_delete_mutation_with_api<A: WikiWriteApi>(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    api: &mut A,
    mutation_id: i64,
) -> Result<DeleteReconciliationReport> {
    target.ensure_matches_api(api.target_api_url())?;
    let connection = open_existing_sync_connection(paths, target)?;
    let intent = load_delete_mutation(&connection, mutation_id)?;
    require_mutation_target(&intent, target)?;

    if intent.phase == "state_advanced" || intent.phase == "resolved" {
        return terminal_delete_report(&intent, target, api.request_count());
    }
    if intent.response_kind.as_deref() == Some("already_missing") {
        let current = observe_remote_page(api, &intent.title)?;
        if let Some(current) = current {
            let detail = format!(
                "MediaWiki reported missingtitle, then revision {} was observed; this request did not delete the page and sync authority was retained",
                current.revision_id
            );
            complete_delete_mutation_without_state_change(
                paths,
                &connection,
                target,
                &intent,
                "remote_present_after_missing",
                &detail,
            )?;
            return Ok(DeleteReconciliationReport {
                mutation_id,
                target_api_url: target.api_url().to_string(),
                title: current.title,
                status: DeleteReconciliationStatus::RemotePresentAfterMissing,
                deletion_log_id: None,
                detail,
                request_count: api.request_count(),
            });
        }
        if let Err(error) = advance_verified_delete_mutation(paths, &connection, target, &intent) {
            let detail = format!(
                "verified remote absence could not finish local effects during reconciliation: {error:#}"
            );
            persist_unresolved(&connection, &intent, "reconciliation_required", &detail)?;
            bail!(detail);
        }
        return Ok(DeleteReconciliationReport {
            mutation_id,
            target_api_url: target.api_url().to_string(),
            title: intent.title,
            status: DeleteReconciliationStatus::StateAdvanced,
            deletion_log_id: None,
            detail: "verified missingtitle response and current absence; local effects and sync state advanced".to_string(),
            request_count: api.request_count(),
        });
    }
    if intent.request_started_at_unix.is_none() {
        let detail = "the durable intent was never marked request-started; no MediaWiki delete request was issued";
        complete_delete_mutation_without_state_change(
            paths,
            &connection,
            target,
            &intent,
            "not_applied",
            detail,
        )?;
        return Ok(DeleteReconciliationReport {
            mutation_id,
            target_api_url: target.api_url().to_string(),
            title: intent.title,
            status: DeleteReconciliationStatus::NotApplied,
            deletion_log_id: intent.response_log_id,
            detail: detail.to_string(),
            request_count: api.request_count(),
        });
    }

    let logs = api.get_delete_log_entries(&intent.title)?;
    let hidden_lineage = logs.iter().any(|entry| entry.comment_hidden);
    let marker_entry = logs.iter().find(|entry| {
        !entry.comment_hidden
            && entry
                .comment
                .as_deref()
                .is_some_and(|comment| comment.contains(&intent.reason_marker))
            && normalized_title_key(&entry.title) == normalized_title_key(&intent.title)
    });
    let current = observe_remote_page(api, &intent.title)?;

    let applied_log_id = intent
        .response_log_id
        .or_else(|| marker_entry.map(|entry| entry.log_id));
    if let Some(log_id) = applied_log_id {
        if let Some(current) = current {
            if current.revision_id == intent.expected_revision_id {
                let detail = format!(
                    "delete marker/log {log_id} proves a delete response, but expected revision {} is still visible; replica lag prevents terminal classification",
                    intent.expected_revision_id
                );
                persist_unresolved(&connection, &intent, "reconciliation_required", &detail)?;
                return Ok(DeleteReconciliationReport {
                    mutation_id,
                    target_api_url: target.api_url().to_string(),
                    title: current.title,
                    status: DeleteReconciliationStatus::RemotePresent,
                    deletion_log_id: Some(log_id),
                    detail,
                    request_count: api.request_count(),
                });
            }
            let detail = format!(
                "delete marker/log {log_id} proves application, but revision {} is currently present; sync authority was retained",
                current.revision_id
            );
            complete_delete_mutation_without_state_change(
                paths,
                &connection,
                target,
                &intent,
                "applied_then_recreated",
                &detail,
            )?;
            return Ok(DeleteReconciliationReport {
                mutation_id,
                target_api_url: target.api_url().to_string(),
                title: current.title,
                status: DeleteReconciliationStatus::AppliedThenRecreated,
                deletion_log_id: Some(log_id),
                detail,
                request_count: api.request_count(),
            });
        }
        if intent.response_log_id.is_none() {
            let marker = marker_entry.context("delete marker disappeared during reconciliation")?;
            bind_delete_response(
                &connection,
                &intent,
                "deleted",
                &marker.title,
                Some(marker.log_id),
                Some(&marker.timestamp),
            )?;
        }
        if let Err(error) = advance_verified_delete_mutation(paths, &connection, target, &intent) {
            let detail = format!(
                "verified delete lineage could not finish local effects during reconciliation: {error:#}"
            );
            persist_unresolved(&connection, &intent, "reconciliation_required", &detail)?;
            bail!(detail);
        }
        return Ok(DeleteReconciliationReport {
            mutation_id,
            target_api_url: target.api_url().to_string(),
            title: intent.title,
            status: DeleteReconciliationStatus::StateAdvanced,
            deletion_log_id: Some(log_id),
            detail: "delete marker/log and current absence were both verified; ledger and snapshot advanced atomically".to_string(),
            request_count: api.request_count(),
        });
    }

    match current {
        Some(current) if current.revision_id == intent.expected_revision_id && !hidden_lineage => {
            let detail = format!(
                "complete visible delete-log lineage contains no mutation marker and the exact expected revision {} remains current",
                intent.expected_revision_id
            );
            complete_delete_mutation_without_state_change(
                paths,
                &connection,
                target,
                &intent,
                "not_applied",
                &detail,
            )?;
            Ok(DeleteReconciliationReport {
                mutation_id,
                target_api_url: target.api_url().to_string(),
                title: current.title,
                status: DeleteReconciliationStatus::NotApplied,
                deletion_log_id: None,
                detail,
                request_count: api.request_count(),
            })
        }
        Some(current) if current.revision_id == intent.expected_revision_id && hidden_lineage => {
            let detail = format!(
                "no visible delete marker was found and current revision {} still matches the expected revision, but hidden delete-log comments prevent proving whether this mutation applied; operator review is required",
                current.revision_id
            );
            persist_unresolved(&connection, &intent, "reconciliation_required", &detail)?;
            Ok(DeleteReconciliationReport {
                mutation_id,
                target_api_url: target.api_url().to_string(),
                title: current.title,
                status: DeleteReconciliationStatus::RemotePresent,
                deletion_log_id: None,
                detail,
                request_count: api.request_count(),
            })
        }
        Some(current) => {
            let detail = format!(
                "no visible delete marker was found and current revision {} differs from expected revision {}; operator review is required",
                current.revision_id, intent.expected_revision_id
            );
            persist_unresolved(&connection, &intent, "reconciliation_required", &detail)?;
            Ok(DeleteReconciliationReport {
                mutation_id,
                target_api_url: target.api_url().to_string(),
                title: current.title,
                status: DeleteReconciliationStatus::RemotePresent,
                deletion_log_id: None,
                detail,
                request_count: api.request_count(),
            })
        }
        None => {
            let detail = if hidden_lineage {
                "the page is absent, but hidden delete-log comments prevent proving whether this mutation applied"
            } else {
                "the page is absent and complete visible delete-log lineage has no matching marker; application cannot be distinguished from an unrelated deletion"
            }
            .to_string();
            persist_unresolved(&connection, &intent, "outcome_ambiguous", &detail)?;
            Ok(DeleteReconciliationReport {
                mutation_id,
                target_api_url: target.api_url().to_string(),
                title: intent.title,
                status: DeleteReconciliationStatus::StillAmbiguous,
                deletion_log_id: None,
                detail,
                request_count: api.request_count(),
            })
        }
    }
}

fn plan_remote_delete_from_observation<A: WikiWriteApi>(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    api: &mut A,
    connection: &rusqlite::Connection,
    title: &str,
    reason: &str,
    local_effect_policy: RemoteDeleteLocalEffectPolicy,
) -> Result<RemoteDeletePlan> {
    if title.trim().is_empty() {
        bail!("remote delete requires a non-empty title");
    }
    if reason.trim().is_empty() {
        bail!("remote delete requires a non-empty reason");
    }
    ensure_no_unresolved_remote_mutation(connection, title)?;
    let observed = observe_remote_page(api, title)?;
    let local_cleanup = load_delete_cleanup_identity(connection, title)?;
    let canonical_title = observed
        .as_ref()
        .map(|page| page.title.clone())
        .or_else(|| local_cleanup.as_ref().map(|cleanup| cleanup.title.clone()))
        .unwrap_or_else(|| title.replace('_', " ").trim().to_string());
    let effective_policy =
        validate_local_effect_policy(paths, local_cleanup.as_ref(), local_effect_policy)?;
    let observed_state = if observed.is_some() {
        RemoteDeleteObservedState::Present
    } else {
        RemoteDeleteObservedState::Missing
    };
    let observed_revision_id = observed.as_ref().map(|page| page.revision_id);
    let observed_timestamp = observed.as_ref().map(|page| page.timestamp.clone());
    let identity = DeletePlanIdentity {
        schema: "wikitool.remote-delete-plan.v1",
        target_api_url: target.api_url(),
        requested_title: title,
        canonical_title: &canonical_title,
        observed_state: &observed_state,
        observed_revision_id,
        observed_timestamp: observed_timestamp.as_deref(),
        reason: reason.trim(),
        local_cleanup: &local_cleanup,
        local_effect_policy: &effective_policy,
    };
    let canonical = serde_json::to_string(&identity).context("failed to encode delete plan")?;
    Ok(RemoteDeletePlan {
        plan_id: compute_sha256(&canonical),
        target_api_url: target.api_url().to_string(),
        requested_title: title.to_string(),
        canonical_title,
        observed_state,
        observed_revision_id,
        observed_timestamp,
        reason: reason.trim().to_string(),
        local_cleanup,
        local_effect_policy: effective_policy,
        request_count: api.request_count(),
    })
}

fn validate_local_effect_policy(
    paths: &SyncProjectPaths,
    cleanup: Option<&RemoteDeleteCleanupIdentity>,
    mut policy: RemoteDeleteLocalEffectPolicy,
) -> Result<RemoteDeleteLocalEffectPolicy> {
    match policy.backup_enabled {
        true if policy.backup_directory.is_none() || policy.backup_path.is_none() => {
            bail!("enabled local delete backup requires both a resolved directory and path")
        }
        false if policy.backup_directory.is_some() || policy.backup_path.is_some() => {
            bail!("disabled local delete backup must not carry a backup directory or path")
        }
        _ => {}
    }
    let Some(cleanup) = cleanup else {
        if policy.backup_enabled || policy.local_content_sha256.is_some() {
            bail!("remote-only delete plans cannot declare local backup or content effects");
        }
        return Ok(policy);
    };

    let local_path = validated_delete_source_path(paths, &cleanup.relative_path)?;
    let actual_hash = if local_path.is_file() {
        Some(compute_sha256(
            &fs::read_to_string(&local_path)
                .with_context(|| format!("failed to read {}", local_path.display()))?,
        ))
    } else {
        None
    };
    if actual_hash.is_some() && policy.local_content_sha256.is_none() {
        bail!(
            "tracked local delete source {} requires an exact local-effect content binding",
            local_path.display()
        );
    }
    if let Some(supplied) = &policy.local_content_sha256
        && Some(supplied) != actual_hash.as_ref()
    {
        bail!(
            "local delete content changed: supplied SHA-256 {supplied}, current SHA-256 {}",
            actual_hash.as_deref().unwrap_or("<missing>")
        );
    }
    if actual_hash.is_none() && policy.backup_enabled {
        bail!(
            "cannot back up missing local delete source {}",
            local_path.display()
        );
    }
    policy.local_content_sha256 = actual_hash;
    Ok(policy)
}

pub(super) fn observe_remote_page<A: WikiWriteApi>(
    api: &mut A,
    title: &str,
) -> Result<Option<RemotePage>> {
    let mut pages = api.get_page_contents(&[title.to_string()])?;
    if pages.len() > 1 {
        bail!(
            "MediaWiki returned {} pages for single-title observation {title:?}",
            pages.len()
        );
    }
    let page = pages.pop();
    if let Some(page) = &page {
        require_same_title(&page.title, title)?;
    }
    Ok(page)
}

fn require_same_title(observed: &str, expected: &str) -> Result<()> {
    if normalized_title_key(observed) != normalized_title_key(expected) {
        bail!("MediaWiki returned title {observed:?} for exact title observation {expected:?}");
    }
    Ok(())
}

fn require_mutation_target(
    intent: &DeleteMutationIntent,
    target: &SyncTargetIdentity,
) -> Result<()> {
    if intent.target_api_url != target.api_url() {
        bail!(
            "delete mutation {} belongs to target {}, not {}",
            intent.mutation_id,
            intent.target_api_url,
            target.api_url()
        );
    }
    Ok(())
}

fn persist_unresolved(
    connection: &rusqlite::Connection,
    intent: &DeleteMutationIntent,
    phase: &'static str,
    detail: &str,
) -> Result<()> {
    mark_delete_mutation_unresolved(connection, intent, phase, detail).with_context(|| {
        format!(
            "failed to persist {phase} state for delete mutation {}",
            intent.mutation_id
        )
    })
}

fn not_applied_error(
    paths: &SyncProjectPaths,
    connection: &rusqlite::Connection,
    target: &SyncTargetIdentity,
    intent: &DeleteMutationIntent,
    detail: String,
) -> anyhow::Error {
    match complete_delete_mutation_without_state_change(
        paths,
        connection,
        target,
        intent,
        "not_applied",
        &detail,
    ) {
        Ok(()) => RemoteDeleteError::NotApplied {
            mutation_id: intent.mutation_id,
            target_api_url: target.api_url().to_string(),
            title: intent.title.clone(),
            detail,
        }
        .into(),
        Err(error) => RemoteDeleteError::ReconciliationRequired {
            mutation_id: intent.mutation_id,
            target_api_url: target.api_url().to_string(),
            title: intent.title.clone(),
            detail: format!(
                "{detail}; additionally failed to persist terminal not_applied outcome: {error:#}"
            ),
        }
        .into(),
    }
}

fn reconciliation_required_error(
    connection: &rusqlite::Connection,
    target: &SyncTargetIdentity,
    intent: &DeleteMutationIntent,
    detail: String,
) -> anyhow::Error {
    let persistence_error =
        persist_unresolved(connection, intent, "reconciliation_required", &detail).err();
    RemoteDeleteError::ReconciliationRequired {
        mutation_id: intent.mutation_id,
        target_api_url: target.api_url().to_string(),
        title: intent.title.clone(),
        detail: append_persistence_error(detail, persistence_error),
    }
    .into()
}

fn append_persistence_error(detail: String, error: Option<anyhow::Error>) -> String {
    match error {
        Some(error) => {
            format!("{detail}; additionally failed to persist mutation state: {error:#}")
        }
        None => detail,
    }
}

fn terminal_delete_report(
    intent: &DeleteMutationIntent,
    target: &SyncTargetIdentity,
    request_count: usize,
) -> Result<DeleteReconciliationReport> {
    let (status, detail) = match (intent.phase.as_str(), intent.terminal_outcome.as_deref()) {
        ("state_advanced", _) => (
            DeleteReconciliationStatus::StateAdvanced,
            "delete mutation already advanced sync state".to_string(),
        ),
        ("resolved", Some("not_applied")) => (
            DeleteReconciliationStatus::NotApplied,
            "delete mutation was already resolved as not applied".to_string(),
        ),
        ("resolved", Some("applied_then_recreated")) => (
            DeleteReconciliationStatus::AppliedThenRecreated,
            "delete mutation was already resolved as applied then recreated".to_string(),
        ),
        ("resolved", Some("remote_present_after_missing")) => (
            DeleteReconciliationStatus::RemotePresentAfterMissing,
            "delete mutation was already resolved as remote present after missingtitle".to_string(),
        ),
        _ => bail!(
            "delete mutation {} has unsupported terminal phase/outcome {}/{:?}",
            intent.mutation_id,
            intent.phase,
            intent.terminal_outcome
        ),
    };
    Ok(DeleteReconciliationReport {
        mutation_id: intent.mutation_id,
        target_api_url: target.api_url().to_string(),
        title: intent.title.clone(),
        status,
        deletion_log_id: intent.response_log_id,
        detail,
        request_count,
    })
}
