use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::remote::{observe_remote_page, reconcile_delete_mutation_with_api};
use crate::storage::{
    EditMutationIntent, advance_verified_edit_mutation, bind_edit_response,
    claim_existing_delete_staging, complete_edit_mutation_without_state_change,
    load_delete_mutation, load_edit_mutation, mark_edit_mutation_unresolved, normalized_title_key,
    open_existing_sync_connection, prepare_delete_local_effects, verify_bound_delete_backup,
};
use crate::support::unix_timestamp;
use crate::support::{compute_sha256, normalize_wiki_content};
use crate::{
    DeleteReconciliationStatus, EditConstraint, EditReceipt, RemoteMutationInspection,
    RemoteMutationListReport, RemoteMutationOperation, RemoteMutationReconciliationReport,
    RemoteMutationReconciliationStatus, RemotePage, SyncProjectPaths, SyncTargetIdentity,
    WikiWriteApi,
};

pub fn list_remote_mutations(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    unresolved_only: bool,
) -> Result<RemoteMutationListReport> {
    let connection = open_existing_sync_connection(paths, target)?;
    let mut mutations = load_edit_inspections(&connection, unresolved_only)?;
    mutations.extend(load_delete_inspections(&connection, unresolved_only)?);
    mutations.sort_by_key(|item| (item.mutation_id, operation_sort_key(item.operation)));
    Ok(RemoteMutationListReport {
        target_api_url: target.api_url().to_string(),
        unresolved_only,
        mutations,
    })
}

pub fn show_remote_mutation(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    operation: RemoteMutationOperation,
    mutation_id: i64,
) -> Result<RemoteMutationInspection> {
    if mutation_id <= 0 {
        bail!("remote mutation ID must be positive");
    }
    let connection = open_existing_sync_connection(paths, target)?;
    let inspection = match operation {
        RemoteMutationOperation::Edit => load_edit_inspection(&connection, mutation_id)?,
        RemoteMutationOperation::Delete => load_delete_inspection(&connection, mutation_id)?,
    };
    if inspection.target_api_url != target.api_url() {
        bail!(
            "remote {:?} mutation {} belongs to target {}, not {}",
            operation,
            mutation_id,
            inspection.target_api_url,
            target.api_url()
        );
    }
    Ok(inspection)
}

pub fn reconcile_remote_mutation_with_api<A: WikiWriteApi>(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    api: &mut A,
    operation: RemoteMutationOperation,
    mutation_id: i64,
) -> Result<RemoteMutationReconciliationReport> {
    target.ensure_matches_api(api.target_api_url())?;
    match operation {
        RemoteMutationOperation::Edit => {
            reconcile_edit_mutation_with_api(paths, target, api, mutation_id)
        }
        RemoteMutationOperation::Delete => {
            let report = reconcile_delete_mutation_with_api(paths, target, api, mutation_id)?;
            let status = match report.status {
                DeleteReconciliationStatus::StateAdvanced => {
                    RemoteMutationReconciliationStatus::StateAdvanced
                }
                DeleteReconciliationStatus::NotApplied => {
                    RemoteMutationReconciliationStatus::NotApplied
                }
                DeleteReconciliationStatus::AppliedThenRecreated => {
                    RemoteMutationReconciliationStatus::AppliedThenRecreated
                }
                DeleteReconciliationStatus::RemotePresentAfterMissing => {
                    RemoteMutationReconciliationStatus::RemotePresentAfterMissing
                }
                DeleteReconciliationStatus::StillAmbiguous => {
                    RemoteMutationReconciliationStatus::StillAmbiguous
                }
                DeleteReconciliationStatus::RemotePresent => {
                    RemoteMutationReconciliationStatus::ReconciliationRequired
                }
            };
            Ok(RemoteMutationReconciliationReport {
                mutation_id: report.mutation_id,
                operation,
                target_api_url: report.target_api_url,
                title: report.title,
                status,
                response_revision_id: None,
                deletion_log_id: report.deletion_log_id,
                detail: report.detail,
                request_count: report.request_count,
            })
        }
    }
}

pub fn close_remote_mutation(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    operation: RemoteMutationOperation,
    mutation_id: i64,
    actor: &str,
    reason: &str,
) -> Result<crate::RemoteMutationClosureReport> {
    if mutation_id <= 0 {
        bail!("remote mutation ID must be positive");
    }
    let actor = actor.trim();
    let reason = reason.trim();
    if actor.is_empty() {
        bail!("operator closure requires a non-empty actor");
    }
    if reason.is_empty() {
        bail!("operator closure requires a non-empty reason");
    }
    let connection = open_existing_sync_connection(paths, target)?;
    let (title, stored_target, previous_phase, mut local_effect_status) = match operation {
        RemoteMutationOperation::Edit => connection
            .query_row(
                "SELECT title, target_api_url, phase, NULL
                 FROM sync_edit_mutations WHERE mutation_id = ?1",
                [mutation_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .with_context(|| format!("edit mutation {mutation_id} does not exist"))?,
        RemoteMutationOperation::Delete => connection
            .query_row(
                "SELECT title, target_api_url, phase, local_effect_status
                 FROM sync_delete_mutations WHERE mutation_id = ?1",
                [mutation_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .with_context(|| format!("delete mutation {mutation_id} does not exist"))?,
    };
    if stored_target != target.api_url() {
        bail!(
            "remote {:?} mutation {mutation_id} belongs to target {stored_target}, not {}",
            operation,
            target.api_url()
        );
    }
    if matches!(previous_phase.as_str(), "state_advanced" | "resolved") {
        bail!(
            "remote {:?} mutation {mutation_id} is already terminal in phase {previous_phase}",
            operation
        );
    }
    if operation == RemoteMutationOperation::Delete {
        let intent = load_delete_mutation(&connection, mutation_id)?;
        local_effect_status = Some(
            claim_existing_delete_staging(paths, &connection, target, &intent)?.local_effect_status,
        );
    }
    if matches!(
        local_effect_status.as_deref(),
        Some("source_staging" | "source_staged")
    ) {
        bail!(
            "delete mutation {mutation_id} owns an in-progress or staged local source; repair its bound backup/staging issue and reconcile it before operator closure"
        );
    }
    if operation == RemoteMutationOperation::Delete {
        let mut intent = load_delete_mutation(&connection, mutation_id)?;
        if intent.relative_path.is_some() && intent.backup_enabled {
            if intent.local_effect_status == "pending" {
                prepare_delete_local_effects(paths, &connection, &intent).with_context(|| {
                    format!(
                        "delete mutation {mutation_id} cannot be operator-closed until its exact bound backup is prepared"
                    )
                })?;
                intent = load_delete_mutation(&connection, mutation_id)?;
            }
            if intent.local_effect_status != "backup_ready" {
                bail!(
                    "delete mutation {mutation_id} cannot be operator-closed with backup-enabled local effect in status {:?}",
                    intent.local_effect_status
                );
            }
            verify_bound_delete_backup(paths, &intent).with_context(|| {
                format!(
                    "delete mutation {mutation_id} cannot be operator-closed because its exact bound backup is not recoverable"
                )
            })?;
        }
    }

    let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)
        .context("failed to begin operator mutation closure")?;
    crate::store::bind_sync_target_for_state_write(paths, &transaction, target)?;
    let now_u64 = unix_timestamp()?;
    let now = i64::try_from(now_u64).context("timestamp does not fit into i64")?;
    let table = match operation {
        RemoteMutationOperation::Edit => "sync_edit_mutations",
        RemoteMutationOperation::Delete => "sync_delete_mutations",
    };
    let local_effect_guard = match operation {
        RemoteMutationOperation::Edit => "",
        RemoteMutationOperation::Delete => {
            " AND local_effect_status NOT IN ('source_staging', 'source_staged')"
        }
    };
    let changed = transaction
        .execute(
            &format!(
                "UPDATE {table}
                 SET phase = 'resolved', terminal_outcome = 'operator_closed_unresolved',
                     updated_at_unix = ?1
                 WHERE mutation_id = ?2
                   AND phase NOT IN ('state_advanced', 'resolved'){local_effect_guard}"
            ),
            params![now, mutation_id],
        )
        .context("failed to terminalize operator-closed mutation")?;
    if changed != 1 {
        bail!("remote mutation changed while operator closure was being recorded");
    }
    transaction
        .execute(
            "INSERT INTO sync_mutation_closures (
                operation, mutation_id, target_api_url, title, previous_phase,
                actor, reason, closed_at_unix
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                operation_label(operation),
                mutation_id,
                target.api_url(),
                title,
                previous_phase,
                actor,
                reason,
                now
            ],
        )
        .context("failed to insert operator closure receipt")?;
    let closure_id = transaction.last_insert_rowid();
    transaction
        .execute(
            "INSERT INTO sync_invalidated_titles (
                title_key, title, target_api_url, closure_id, invalidated_at_unix
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                normalized_title_key(&title),
                title,
                target.api_url(),
                closure_id,
                now
            ],
        )
        .context("failed to invalidate sync baseline after operator closure")?;
    transaction
        .commit()
        .context("failed to commit operator mutation closure")?;
    Ok(crate::RemoteMutationClosureReport {
        closure_id,
        mutation_id,
        operation,
        target_api_url: target.api_url().to_string(),
        title: title.clone(),
        previous_phase,
        terminal_outcome: "operator_closed_unresolved".to_string(),
        actor: actor.to_string(),
        reason: reason.to_string(),
        closed_at_unix: now_u64,
        sync_baseline_invalidated: true,
        next_action: format!(
            "run `wikitool pull --full --all`; if {title:?} is present but local content differs, preserve those edits and explicitly use `--overwrite-local`. Only a present-page refresh or authoritative global absence clears the write block"
        ),
    })
}

fn reconcile_edit_mutation_with_api<A: WikiWriteApi>(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
    api: &mut A,
    mutation_id: i64,
) -> Result<RemoteMutationReconciliationReport> {
    let connection = open_existing_sync_connection(paths, target)?;
    let intent = load_edit_mutation(&connection, mutation_id)?;
    require_edit_target(&intent, target)?;
    if intent.phase == "state_advanced" || intent.phase == "resolved" {
        return terminal_edit_report(&intent, target, api.request_count());
    }
    if intent.request_started_at_unix.is_none() {
        let detail = "the durable edit intent was never marked request-started; no MediaWiki edit request was issued";
        complete_edit_mutation_without_state_change(
            paths,
            &connection,
            target,
            &intent,
            "not_applied",
            detail,
        )?;
        return Ok(edit_report(
            &intent,
            target,
            RemoteMutationReconciliationStatus::NotApplied,
            None,
            detail,
            api.request_count(),
        ));
    }

    if let Some(response_revision_id) = intent.response_new_revision_id {
        let page = api
            .get_revision_by_id(response_revision_id)?
            .with_context(|| {
                format!("MediaWiki did not return response-bound revision {response_revision_id}")
            })?;
        verify_edit_revision(&intent, &page, response_revision_id)?;
        let current = observe_remote_page(api, &intent.title)?;
        if current
            .as_ref()
            .is_none_or(|current| current.revision_id != response_revision_id)
        {
            let detail = match current {
                Some(current) => format!(
                    "response-bound revision {response_revision_id} proves this edit applied, but current revision {} is newer; prior sync authority was retained",
                    current.revision_id
                ),
                None => format!(
                    "response-bound revision {response_revision_id} proves this edit applied, but the page is now absent; prior sync authority was retained"
                ),
            };
            complete_edit_mutation_without_state_change(
                paths,
                &connection,
                target,
                &intent,
                "applied_then_changed",
                &detail,
            )?;
            return Ok(edit_report(
                &intent,
                target,
                RemoteMutationReconciliationStatus::AppliedThenChanged,
                Some(response_revision_id),
                &detail,
                api.request_count(),
            ));
        }
        advance_verified_edit_mutation(paths, &connection, target, &intent, &page)?;
        return Ok(edit_report(
            &intent,
            target,
            RemoteMutationReconciliationStatus::StateAdvanced,
            Some(response_revision_id),
            "response-bound exact revision content was verified and sync state advanced atomically",
            api.request_count(),
        ));
    }

    let lineage = api.get_revision_lineage(&intent.title)?;
    let hidden_lineage = lineage.iter().any(|entry| entry.comment_hidden);
    let marker = intent.summary_marker.as_deref().and_then(|summary_marker| {
        lineage.iter().find(|entry| {
            !entry.comment_hidden
                && entry
                    .comment
                    .as_deref()
                    .is_some_and(|comment| comment.contains(summary_marker))
                && normalized_title_key(&entry.title) == normalized_title_key(&intent.title)
        })
    });
    let current = observe_remote_page(api, &intent.title)?;

    if let Some(marker) = marker {
        let marker_content = marker.content.as_deref().with_context(|| {
            format!(
                "revision {} carries the edit marker but its content is unavailable",
                marker.revision_id
            )
        })?;
        let marker_hash = compute_sha256(&normalize_wiki_content(marker_content));
        if marker_hash != intent.intended_normalized_sha256 {
            let detail = format!(
                "revision {} carries the mutation marker but content hash {marker_hash} differs from intended hash {}",
                marker.revision_id, intent.intended_normalized_sha256
            );
            mark_edit_mutation_unresolved(
                &connection,
                &intent,
                "reconciliation_required",
                &detail,
            )?;
            return Ok(edit_report(
                &intent,
                target,
                RemoteMutationReconciliationStatus::ReconciliationRequired,
                Some(marker.revision_id),
                &detail,
                api.request_count(),
            ));
        }

        if current
            .as_ref()
            .is_some_and(|page| page.revision_id != marker.revision_id)
            || current.is_none()
        {
            let detail = match current {
                Some(ref page) => format!(
                    "revision {} proves this edit applied, but current revision {} is newer; prior sync authority was retained",
                    marker.revision_id, page.revision_id
                ),
                None => format!(
                    "revision {} proves this edit applied, but the page is now absent; prior sync authority was retained",
                    marker.revision_id
                ),
            };
            complete_edit_mutation_without_state_change(
                paths,
                &connection,
                target,
                &intent,
                "applied_then_changed",
                &detail,
            )?;
            return Ok(edit_report(
                &intent,
                target,
                RemoteMutationReconciliationStatus::AppliedThenChanged,
                Some(marker.revision_id),
                &detail,
                api.request_count(),
            ));
        }

        let old_revision_id = match intent.constraint {
            EditConstraint::CreateOnly => 0,
            EditConstraint::ExistingRevision { revision_id } => revision_id,
        };
        let receipt = EditReceipt {
            title: marker.title.clone(),
            page_id: marker.page_id,
            old_revision_id,
            new_revision_id: marker.revision_id,
            new_timestamp: marker.timestamp.clone(),
        };
        bind_edit_response(&connection, &intent, &receipt)?;
        let page = RemotePage {
            title: marker.title.clone(),
            namespace: intent.namespace_id.with_context(|| {
                format!(
                    "edit mutation {} predates durable numeric namespace identity for {:?}",
                    intent.mutation_id, intent.namespace
                )
            })?,
            page_id: marker.page_id,
            revision_id: marker.revision_id,
            timestamp: marker.timestamp.clone(),
            content: marker_content.to_string(),
        };
        verify_edit_revision(&intent, &page, marker.revision_id)?;
        advance_verified_edit_mutation(paths, &connection, target, &intent, &page)?;
        return Ok(edit_report(
            &intent,
            target,
            RemoteMutationReconciliationStatus::StateAdvanced,
            Some(marker.revision_id),
            "complete revision lineage and exact content prove the marked edit; sync state advanced atomically",
            api.request_count(),
        ));
    }

    match (intent.constraint, current) {
        (EditConstraint::ExistingRevision { revision_id }, Some(current))
            if current.revision_id == revision_id && !hidden_lineage =>
        {
            let detail = format!(
                "complete visible revision lineage contains no mutation marker and exact expected revision {revision_id} remains current"
            );
            complete_edit_mutation_without_state_change(
                paths,
                &connection,
                target,
                &intent,
                "not_applied",
                &detail,
            )?;
            Ok(edit_report(
                &intent,
                target,
                RemoteMutationReconciliationStatus::NotApplied,
                None,
                &detail,
                api.request_count(),
            ))
        }
        (EditConstraint::CreateOnly, None) | (_, None) => {
            let detail = if hidden_lineage {
                "the page is absent and hidden revision comments prevent complete marker proof"
            } else {
                "the page is absent with no visible mutation marker; an applied-then-deleted edit cannot be excluded"
            };
            mark_edit_mutation_unresolved(&connection, &intent, "outcome_ambiguous", detail)?;
            Ok(edit_report(
                &intent,
                target,
                RemoteMutationReconciliationStatus::StillAmbiguous,
                None,
                detail,
                api.request_count(),
            ))
        }
        (_, Some(current)) => {
            let detail = format!(
                "no visible mutation marker was found and current revision {} does not prove the request was not applied",
                current.revision_id
            );
            mark_edit_mutation_unresolved(
                &connection,
                &intent,
                "reconciliation_required",
                &detail,
            )?;
            Ok(edit_report(
                &intent,
                target,
                RemoteMutationReconciliationStatus::ReconciliationRequired,
                Some(current.revision_id),
                &detail,
                api.request_count(),
            ))
        }
    }
}

fn load_edit_inspections(
    connection: &Connection,
    unresolved_only: bool,
) -> Result<Vec<RemoteMutationInspection>> {
    let where_clause = if unresolved_only {
        " WHERE phase NOT IN ('state_advanced', 'resolved')"
    } else {
        ""
    };
    let sql = format!(
        "SELECT mutation_id, target_api_url, title, phase,
                request_started_at_unix, terminal_outcome,
                expected_revision_id, response_new_revision_id, detail, relative_path
         FROM sync_edit_mutations{where_clause}"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], decode_edit_inspection)?;
    let mut inspections = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode edit mutation list")?;
    attach_closure_receipts(connection, &mut inspections)?;
    Ok(inspections)
}

fn load_delete_inspections(
    connection: &Connection,
    unresolved_only: bool,
) -> Result<Vec<RemoteMutationInspection>> {
    let where_clause = if unresolved_only {
        " WHERE phase NOT IN ('state_advanced', 'resolved')"
    } else {
        ""
    };
    let sql = format!(
        "SELECT mutation_id, target_api_url, title, phase,
                request_started_at_unix, terminal_outcome,
                expected_revision_id, response_log_id, detail, relative_path,
                backup_enabled, backup_directory, backup_path,
                local_content_sha256, local_effect_status
         FROM sync_delete_mutations{where_clause}"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], decode_delete_inspection)?;
    let mut inspections = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to decode delete mutation list")?;
    attach_closure_receipts(connection, &mut inspections)?;
    Ok(inspections)
}

fn load_edit_inspection(
    connection: &Connection,
    mutation_id: i64,
) -> Result<RemoteMutationInspection> {
    let mut inspection = connection
        .query_row(
            "SELECT mutation_id, target_api_url, title, phase,
                    request_started_at_unix, terminal_outcome,
                    expected_revision_id, response_new_revision_id, detail, relative_path
             FROM sync_edit_mutations WHERE mutation_id = ?1",
            [mutation_id],
            decode_edit_inspection,
        )
        .optional()?
        .with_context(|| format!("edit mutation {mutation_id} does not exist"))?;
    inspection.closure = load_closure_receipt(
        connection,
        RemoteMutationOperation::Edit,
        inspection.mutation_id,
    )?;
    Ok(inspection)
}

fn load_delete_inspection(
    connection: &Connection,
    mutation_id: i64,
) -> Result<RemoteMutationInspection> {
    let mut inspection = connection
        .query_row(
            "SELECT mutation_id, target_api_url, title, phase,
                    request_started_at_unix, terminal_outcome,
                    expected_revision_id, response_log_id, detail, relative_path,
                    backup_enabled, backup_directory, backup_path,
                    local_content_sha256, local_effect_status
             FROM sync_delete_mutations WHERE mutation_id = ?1",
            [mutation_id],
            decode_delete_inspection,
        )
        .optional()?
        .with_context(|| format!("delete mutation {mutation_id} does not exist"))?;
    inspection.closure = load_closure_receipt(
        connection,
        RemoteMutationOperation::Delete,
        inspection.mutation_id,
    )?;
    Ok(inspection)
}

fn decode_edit_inspection(row: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteMutationInspection> {
    Ok(RemoteMutationInspection {
        mutation_id: row.get(0)?,
        operation: RemoteMutationOperation::Edit,
        target_api_url: row.get(1)?,
        title: row.get(2)?,
        relative_path: Some(row.get(9)?),
        phase: row.get(3)?,
        request_started: row.get::<_, Option<i64>>(4)?.is_some(),
        terminal_outcome: row.get(5)?,
        expected_revision_id: row.get(6)?,
        response_revision_id: row.get(7)?,
        deletion_log_id: None,
        local_effect_policy: None,
        local_effect_status: None,
        detail: row.get(8)?,
        closure: None,
    })
}

fn decode_delete_inspection(row: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteMutationInspection> {
    Ok(RemoteMutationInspection {
        mutation_id: row.get(0)?,
        operation: RemoteMutationOperation::Delete,
        target_api_url: row.get(1)?,
        title: row.get(2)?,
        relative_path: row.get(9)?,
        phase: row.get(3)?,
        request_started: row.get::<_, Option<i64>>(4)?.is_some(),
        terminal_outcome: row.get(5)?,
        expected_revision_id: row.get(6)?,
        response_revision_id: None,
        deletion_log_id: row.get(7)?,
        local_effect_policy: Some(crate::RemoteDeleteLocalEffectPolicy {
            backup_enabled: row.get::<_, i64>(10)? != 0,
            backup_directory: row.get(11)?,
            backup_path: row.get(12)?,
            local_content_sha256: row.get(13)?,
        }),
        local_effect_status: row.get(14)?,
        detail: row.get(8)?,
        closure: None,
    })
}

fn attach_closure_receipts(
    connection: &Connection,
    inspections: &mut [RemoteMutationInspection],
) -> Result<()> {
    for inspection in inspections {
        inspection.closure =
            load_closure_receipt(connection, inspection.operation, inspection.mutation_id)?;
    }
    Ok(())
}

fn load_closure_receipt(
    connection: &Connection,
    operation: RemoteMutationOperation,
    mutation_id: i64,
) -> Result<Option<crate::RemoteMutationClosureReceipt>> {
    connection
        .query_row(
            "SELECT closure_id, target_api_url, title, previous_phase,
                    actor, reason, closed_at_unix
             FROM sync_mutation_closures
             WHERE operation = ?1 AND mutation_id = ?2",
            params![operation_label(operation), mutation_id],
            |row| {
                let closed_at = row.get::<_, i64>(6)?;
                Ok(crate::RemoteMutationClosureReceipt {
                    closure_id: row.get(0)?,
                    mutation_id,
                    operation,
                    target_api_url: row.get(1)?,
                    title: row.get(2)?,
                    previous_phase: row.get(3)?,
                    actor: row.get(4)?,
                    reason: row.get(5)?,
                    closed_at_unix: u64::try_from(closed_at)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(6, closed_at))?,
                })
            },
        )
        .optional()
        .context("failed to load operator mutation closure receipt")
}

fn verify_edit_revision(
    intent: &EditMutationIntent,
    page: &RemotePage,
    revision_id: i64,
) -> Result<()> {
    if page.revision_id != revision_id {
        bail!(
            "exact revision query returned revision {} instead of {revision_id}",
            page.revision_id
        );
    }
    if normalized_title_key(&page.title) != normalized_title_key(&intent.title) {
        bail!(
            "exact revision title {:?} does not match intended title {:?}",
            page.title,
            intent.title
        );
    }
    let returned_hash = compute_sha256(&normalize_wiki_content(&page.content));
    if returned_hash != intent.intended_normalized_sha256 {
        bail!(
            "exact revision content hash {returned_hash} does not match intended normalized content hash {}",
            intent.intended_normalized_sha256
        );
    }
    Ok(())
}

fn require_edit_target(intent: &EditMutationIntent, target: &SyncTargetIdentity) -> Result<()> {
    if intent.target_api_url != target.api_url() {
        bail!(
            "edit mutation {} belongs to target {}, not {}",
            intent.mutation_id,
            intent.target_api_url,
            target.api_url()
        );
    }
    Ok(())
}

fn terminal_edit_report(
    intent: &EditMutationIntent,
    target: &SyncTargetIdentity,
    request_count: usize,
) -> Result<RemoteMutationReconciliationReport> {
    let (status, detail) = match (intent.phase.as_str(), intent.terminal_outcome.as_deref()) {
        ("state_advanced", _) => (
            RemoteMutationReconciliationStatus::StateAdvanced,
            "edit mutation already advanced sync state".to_string(),
        ),
        ("resolved", Some("not_applied")) => (
            RemoteMutationReconciliationStatus::NotApplied,
            "edit mutation was already resolved as not applied".to_string(),
        ),
        ("resolved", Some("applied_then_changed")) => (
            RemoteMutationReconciliationStatus::AppliedThenChanged,
            "edit mutation was already resolved as applied then changed".to_string(),
        ),
        _ => bail!(
            "edit mutation {} has unsupported terminal phase/outcome {}/{:?}",
            intent.mutation_id,
            intent.phase,
            intent.terminal_outcome
        ),
    };
    Ok(RemoteMutationReconciliationReport {
        mutation_id: intent.mutation_id,
        operation: RemoteMutationOperation::Edit,
        target_api_url: target.api_url().to_string(),
        title: intent.title.clone(),
        status,
        response_revision_id: intent.response_new_revision_id,
        deletion_log_id: None,
        detail,
        request_count,
    })
}

fn edit_report(
    intent: &EditMutationIntent,
    target: &SyncTargetIdentity,
    status: RemoteMutationReconciliationStatus,
    response_revision_id: Option<i64>,
    detail: &str,
    request_count: usize,
) -> RemoteMutationReconciliationReport {
    RemoteMutationReconciliationReport {
        mutation_id: intent.mutation_id,
        operation: RemoteMutationOperation::Edit,
        target_api_url: target.api_url().to_string(),
        title: intent.title.clone(),
        status,
        response_revision_id,
        deletion_log_id: None,
        detail: detail.to_string(),
        request_count,
    }
}

fn operation_sort_key(operation: RemoteMutationOperation) -> u8 {
    match operation {
        RemoteMutationOperation::Edit => 0,
        RemoteMutationOperation::Delete => 1,
    }
}

fn operation_label(operation: RemoteMutationOperation) -> &'static str {
    match operation {
        RemoteMutationOperation::Edit => "edit",
        RemoteMutationOperation::Delete => "delete",
    }
}
