use super::*;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};
use std::io::Write;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SyncStateFaultPoint {
    AfterLedgerUpsert,
    AfterLedgerDelete,
    AfterSourceRenameBeforeStagedMarker,
    AfterRetainedDeleteLocalRecovery,
    BeforeStagedSourceUnlink,
    AfterStagedSourceUnlink,
}

#[cfg(test)]
thread_local! {
    static NEXT_SYNC_STATE_FAULT: Cell<Option<SyncStateFaultPoint>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(super) fn inject_sync_state_fault_once(point: SyncStateFaultPoint) {
    NEXT_SYNC_STATE_FAULT.with(|next| next.set(Some(point)));
}

#[cfg(test)]
fn fail_if_injected(point: SyncStateFaultPoint) -> Result<()> {
    let injected = NEXT_SYNC_STATE_FAULT.with(|next| {
        if next.get() == Some(point) {
            next.set(None);
            true
        } else {
            false
        }
    });
    if injected {
        bail!("injected sync-state transaction failure at {point:?}");
    }
    Ok(())
}

fn remove_sync_ledger_entry(connection: &Connection, title: &str) -> Result<()> {
    connection
        .execute("DELETE FROM sync_ledger_pages WHERE title = ?1", [title])
        .with_context(|| format!("failed to delete sync ledger row for {title}"))?;
    Ok(())
}

pub(super) fn remove_sync_page_state(connection: &Connection, title: &str) -> Result<()> {
    initialize_sync_schema(connection)?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .with_context(|| format!("failed to begin sync-state deletion for {title}"))?;
    remove_sync_ledger_entry(&transaction, title)?;
    #[cfg(test)]
    fail_if_injected(SyncStateFaultPoint::AfterLedgerDelete)?;
    remove_sync_snapshot(&transaction, title)?;
    transaction
        .commit()
        .with_context(|| format!("failed to commit sync-state deletion for {title}"))?;
    Ok(())
}

pub(super) fn load_sync_snapshot_map(
    connection: &Connection,
) -> Result<BTreeMap<String, SyncSnapshotEntry>> {
    if !table_exists(connection, "sync_snapshots")? {
        return Ok(BTreeMap::new());
    }
    let mut statement = connection
        .prepare(
            "SELECT title, relative_path, content_text
             FROM sync_snapshots",
        )
        .context("failed to prepare sync snapshot query")?;
    let rows = statement
        .query_map([], |row| {
            Ok(SyncSnapshotEntry {
                title: row.get(0)?,
                relative_path: row.get(1)?,
                content_text: row.get(2)?,
            })
        })
        .context("failed to run sync snapshot query")?;

    let mut out = BTreeMap::new();
    for row in rows {
        let row = row.context("failed to decode sync snapshot row")?;
        out.insert(normalized_title_key(&row.title), row);
    }
    Ok(out)
}

pub(super) fn backfill_sync_snapshots_from_local(
    connection: &Connection,
    paths: &SyncProjectPaths,
    local_map: &BTreeMap<String, ScannedFile>,
    ledger: &BTreeMap<String, SyncLedgerEntry>,
) -> Result<()> {
    let snapshots = load_sync_snapshot_map(connection)?;
    for (key, file) in local_map {
        let Some(entry) = ledger.get(key) else {
            continue;
        };
        if file.content_hash != entry.content_hash || snapshots.contains_key(key) {
            continue;
        }
        let absolute = absolute_path_from_relative(paths, &file.relative_path);
        let content = fs::read_to_string(&absolute)
            .with_context(|| format!("failed to read {}", absolute.display()))?;
        upsert_sync_snapshot(connection, &file.title, &file.relative_path, &content)?;
    }
    Ok(())
}

fn upsert_sync_snapshot(
    connection: &Connection,
    title: &str,
    relative_path: &str,
    content_text: &str,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO sync_snapshots (
                title, relative_path, content_text
            ) VALUES (?1, ?2, ?3)
            ON CONFLICT(title) DO UPDATE SET
                relative_path = excluded.relative_path,
                content_text = excluded.content_text",
            params![title, relative_path, content_text],
        )
        .with_context(|| format!("failed to upsert sync snapshot for {title}"))?;
    Ok(())
}

fn remove_sync_snapshot(connection: &Connection, title: &str) -> Result<()> {
    connection
        .execute("DELETE FROM sync_snapshots WHERE title = ?1", [title])
        .with_context(|| format!("failed to delete sync snapshot for {title}"))?;
    Ok(())
}

pub(super) fn load_sync_ledger_map(
    connection: &Connection,
    include_templates: bool,
) -> Result<BTreeMap<String, SyncLedgerEntry>> {
    if !table_exists(connection, "sync_ledger_pages")? {
        return Ok(BTreeMap::new());
    }

    let mut statement = connection
        .prepare(
            "SELECT title, namespace, relative_path, content_hash, wiki_modified_at
             FROM sync_ledger_pages",
        )
        .context("failed to prepare sync ledger query")?;
    let rows = statement
        .query_map([], |row| {
            Ok(SyncLedgerEntry {
                title: row.get(0)?,
                namespace: row.get(1)?,
                relative_path: row.get(2)?,
                content_hash: row.get(3)?,
                wiki_modified_at: row.get(4)?,
            })
        })
        .context("failed to run sync ledger query")?;

    let mut out = BTreeMap::new();
    for row in rows {
        let row = row.context("failed to decode sync ledger row")?;
        if !include_templates && is_template_namespace_id(row.namespace) {
            continue;
        }
        out.insert(normalized_title_key(&row.title), row);
    }
    Ok(out)
}

pub(super) fn load_delete_cleanup_identity(
    connection: &Connection,
    title: &str,
) -> Result<Option<RemoteDeleteCleanupIdentity>> {
    let mut statement = connection
        .prepare(
            "SELECT title, relative_path, content_hash, revision_id
             FROM sync_ledger_pages",
        )
        .context("failed to prepare delete cleanup-identity query")?;
    let rows = statement
        .query_map([], |row| {
            Ok(RemoteDeleteCleanupIdentity {
                title: row.get(0)?,
                relative_path: row.get(1)?,
                content_hash: row.get(2)?,
                revision_id: row.get(3)?,
            })
        })
        .context("failed to inspect delete cleanup identity")?;
    let requested = normalized_title_key(title);
    let mut matched = Vec::new();
    for row in rows {
        let identity = row.context("failed to decode delete cleanup identity")?;
        if normalized_title_key(&identity.title) == requested {
            matched.push(identity);
        }
    }
    match matched.len() {
        0 => Ok(None),
        1 => Ok(matched.pop()),
        count => anyhow::bail!(
            "durable sync authority contains {count} rows for MediaWiki title identity {requested:?}"
        ),
    }
}

fn upsert_sync_ledger(
    connection: &Connection,
    page: &RemotePage,
    relative_path: &str,
    content_hash: &str,
    is_redirect: bool,
    redirect_target: Option<&str>,
) -> Result<()> {
    let now = unix_timestamp()?;
    connection
        .execute(
            "INSERT INTO sync_ledger_pages (
                title, namespace, relative_path, content_hash, wiki_modified_at, revision_id,
                page_id, is_redirect, redirect_target, last_synced_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(title) DO UPDATE SET
                namespace = excluded.namespace,
                relative_path = excluded.relative_path,
                content_hash = excluded.content_hash,
                wiki_modified_at = excluded.wiki_modified_at,
                revision_id = excluded.revision_id,
                page_id = excluded.page_id,
                is_redirect = excluded.is_redirect,
                redirect_target = excluded.redirect_target,
                last_synced_at_unix = excluded.last_synced_at_unix",
            params![
                page.title,
                page.namespace,
                relative_path,
                content_hash,
                page.timestamp,
                page.revision_id,
                page.page_id,
                if is_redirect { 1i64 } else { 0i64 },
                redirect_target,
                i64::try_from(now).context("timestamp does not fit into i64")?
            ],
        )
        .with_context(|| format!("failed to upsert sync ledger row for {}", page.title))?;
    Ok(())
}

pub(super) struct SyncPageState<'a> {
    pub page: &'a RemotePage,
    pub relative_path: &'a str,
    pub content_hash: &'a str,
    pub is_redirect: bool,
    pub redirect_target: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(super) struct EditMutationIntent {
    pub mutation_id: i64,
    pub target_api_url: String,
    pub title: String,
    pub namespace: String,
    pub namespace_id: Option<i32>,
    pub relative_path: String,
    pub intended_normalized_sha256: String,
    pub summary: String,
    pub summary_marker: Option<String>,
    pub constraint: EditConstraint,
    pub phase: String,
    pub request_started_at_unix: Option<i64>,
    pub terminal_outcome: Option<String>,
    pub response_new_revision_id: Option<i64>,
}

impl EditMutationIntent {
    pub fn request_summary(&self) -> String {
        match &self.summary_marker {
            Some(marker) => format!("[{marker}] {}", self.summary),
            None => self.summary.clone(),
        }
    }
}

pub(super) struct NewEditMutation<'a> {
    pub title: &'a str,
    pub namespace: &'a str,
    pub namespace_id: i32,
    pub relative_path: &'a str,
    pub content: &'a str,
    pub summary: &'a str,
    pub constraint: EditConstraint,
}

#[derive(Debug, Clone)]
pub(super) struct DeleteMutationIntent {
    pub mutation_id: i64,
    pub target_api_url: String,
    pub title: String,
    pub relative_path: Option<String>,
    pub expected_revision_id: i64,
    pub reason: String,
    pub reason_marker: String,
    pub phase: String,
    pub response_kind: Option<String>,
    pub response_log_id: Option<i64>,
    pub request_started_at_unix: Option<i64>,
    pub terminal_outcome: Option<String>,
    pub backup_enabled: bool,
    pub backup_directory: Option<String>,
    pub backup_path: Option<String>,
    pub local_content_sha256: Option<String>,
    pub local_effect_status: String,
}

impl DeleteMutationIntent {
    pub fn request_reason(&self) -> String {
        format!("[{}] {}", self.reason_marker, self.reason)
    }
}

pub(super) struct NewDeleteMutation<'a> {
    pub title: &'a str,
    pub relative_path: Option<&'a str>,
    pub expected_revision_id: i64,
    pub reason: &'a str,
    pub local_effect_policy: &'a RemoteDeleteLocalEffectPolicy,
}

pub(super) fn unresolved_remote_mutation(
    connection: &Connection,
    title: &str,
) -> Result<Option<(String, i64, String)>> {
    let mut statement = connection
        .prepare(
            "SELECT 'edit', mutation_id, title, phase
             FROM sync_edit_mutations
             WHERE phase NOT IN ('state_advanced', 'resolved')
             UNION ALL
             SELECT 'delete', mutation_id, title, phase
             FROM sync_delete_mutations
             WHERE phase NOT IN ('state_advanced', 'resolved')",
        )
        .context("failed to prepare unresolved remote-mutation query")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .context("failed to inspect unresolved remote mutations")?;
    let requested_key = normalized_title_key(title);
    for row in rows {
        let (operation, mutation_id, stored_title, phase) =
            row.context("failed to decode unresolved remote mutation")?;
        if normalized_title_key(&stored_title) == requested_key {
            return Ok(Some((operation, mutation_id, phase)));
        }
    }
    Ok(None)
}

pub(super) fn ensure_no_unresolved_remote_mutation(
    connection: &Connection,
    title: &str,
) -> Result<()> {
    if let Some((operation, mutation_id, phase)) = unresolved_remote_mutation(connection, title)? {
        anyhow::bail!(
            "unresolved remote {operation} mutation {mutation_id} for {title} is in phase {phase}; reconcile it before issuing another write"
        );
    }
    if let Some((closure_id, stored_title)) = invalidated_sync_title(connection, title)? {
        anyhow::bail!(
            "sync baseline for {stored_title} was invalidated by operator closure {closure_id}; run a target-bound pull that refreshes this exact title before issuing another write"
        );
    }
    Ok(())
}

pub(super) fn invalidated_sync_title(
    connection: &Connection,
    title: &str,
) -> Result<Option<(i64, String)>> {
    connection
        .query_row(
            "SELECT closure_id, title FROM sync_invalidated_titles WHERE title_key = ?1",
            [normalized_title_key(title)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .context("failed to inspect invalidated sync title")
}

pub(super) fn clear_sync_title_invalidation(connection: &Connection, title: &str) -> Result<()> {
    connection
        .execute(
            "DELETE FROM sync_invalidated_titles WHERE title_key = ?1",
            [normalized_title_key(title)],
        )
        .with_context(|| format!("failed to clear sync-baseline invalidation for {title}"))?;
    Ok(())
}

pub(super) fn begin_edit_mutation(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    mutation: NewEditMutation<'_>,
) -> Result<EditMutationIntent> {
    initialize_sync_schema(connection)?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .with_context(|| format!("failed to begin durable edit intent for {}", mutation.title))?;
    store::bind_sync_target_for_state_write(paths, &transaction, target)?;

    ensure_no_unresolved_remote_mutation(&transaction, mutation.title)?;

    let (constraint_kind, expected_revision_id) = match mutation.constraint {
        EditConstraint::CreateOnly => ("create_only", None),
        EditConstraint::ExistingRevision { revision_id } => {
            ("existing_revision", Some(revision_id))
        }
    };
    let intended_content_sha256 = compute_sha256(mutation.content);
    let intended_normalized_sha256 = compute_sha256(&normalize_wiki_content(mutation.content));
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    transaction
        .execute(
            "INSERT INTO sync_edit_mutations (
                target_api_url, title, namespace, namespace_id, relative_path,
                intended_content_sha256, intended_normalized_sha256, summary, summary_marker,
                constraint_kind, expected_revision_id, phase,
                created_at_unix, updated_at_unix
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                'wikitool-edit:' || lower(hex(randomblob(16))),
                ?9, ?10, 'intent_persisted', ?11, ?11
             )",
            params![
                target.api_url(),
                mutation.title,
                mutation.namespace,
                mutation.namespace_id,
                mutation.relative_path,
                intended_content_sha256,
                intended_normalized_sha256,
                mutation.summary,
                constraint_kind,
                expected_revision_id,
                now,
            ],
        )
        .with_context(|| format!("failed to persist edit intent for {}", mutation.title))?;
    let mutation_id = transaction.last_insert_rowid();
    let summary_marker = transaction
        .query_row(
            "SELECT summary_marker FROM sync_edit_mutations WHERE mutation_id = ?1",
            [mutation_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .context("failed to read persisted edit mutation marker")?;
    transaction
        .commit()
        .with_context(|| format!("failed to commit edit intent for {}", mutation.title))?;

    Ok(EditMutationIntent {
        mutation_id,
        target_api_url: target.api_url().to_string(),
        title: mutation.title.to_string(),
        namespace: mutation.namespace.to_string(),
        namespace_id: Some(mutation.namespace_id),
        relative_path: mutation.relative_path.to_string(),
        intended_normalized_sha256,
        summary: mutation.summary.to_string(),
        summary_marker,
        constraint: mutation.constraint,
        phase: "intent_persisted".to_string(),
        request_started_at_unix: None,
        terminal_outcome: None,
        response_new_revision_id: None,
    })
}

pub(super) fn load_edit_mutation(
    connection: &Connection,
    mutation_id: i64,
) -> Result<EditMutationIntent> {
    connection
        .query_row(
            "SELECT mutation_id, target_api_url, title, namespace, relative_path,
                    intended_content_sha256, intended_normalized_sha256,
                    summary, summary_marker, constraint_kind, expected_revision_id,
                    phase, request_started_at_unix, terminal_outcome,
                    response_title, response_page_id,
                    response_old_revision_id, response_new_revision_id,
                    response_new_timestamp, namespace_id
             FROM sync_edit_mutations
             WHERE mutation_id = ?1",
            [mutation_id],
            |row| {
                let constraint_kind = row.get::<_, String>(9)?;
                let expected_revision_id = row.get::<_, Option<i64>>(10)?;
                let constraint = match (constraint_kind.as_str(), expected_revision_id) {
                    ("create_only", None) => EditConstraint::CreateOnly,
                    ("existing_revision", Some(revision_id)) => {
                        EditConstraint::ExistingRevision { revision_id }
                    }
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                Ok(EditMutationIntent {
                    mutation_id: row.get(0)?,
                    target_api_url: row.get(1)?,
                    title: row.get(2)?,
                    namespace: row.get(3)?,
                    namespace_id: row.get(19)?,
                    relative_path: row.get(4)?,
                    intended_normalized_sha256: row.get(6)?,
                    summary: row.get(7)?,
                    summary_marker: row.get(8)?,
                    constraint,
                    phase: row.get(11)?,
                    request_started_at_unix: row.get(12)?,
                    terminal_outcome: row.get(13)?,
                    response_new_revision_id: row.get(17)?,
                })
            },
        )
        .with_context(|| format!("edit mutation {mutation_id} does not exist"))
}

pub(super) fn bind_edit_response(
    connection: &Connection,
    intent: &EditMutationIntent,
    response: &EditReceipt,
) -> Result<()> {
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .with_context(|| format!("failed to begin edit-response binding for {}", intent.title))?;
    let changed = transaction
        .execute(
            "UPDATE sync_edit_mutations
             SET phase = 'response_bound',
                 response_title = ?1,
                 response_page_id = ?2,
                 response_old_revision_id = ?3,
                 response_new_revision_id = ?4,
                 response_new_timestamp = ?5,
                 detail = NULL,
                 updated_at_unix = ?6
             WHERE mutation_id = ?7
               AND phase IN ('intent_persisted', 'outcome_ambiguous', 'reconciliation_required')",
            params![
                response.title,
                response.page_id,
                response.old_revision_id,
                response.new_revision_id,
                response.new_timestamp,
                now,
                intent.mutation_id,
            ],
        )
        .with_context(|| format!("failed to bind edit response for {}", intent.title))?;
    if changed != 1 {
        anyhow::bail!(
            "edit mutation {} for {} cannot bind a response from phase {}",
            intent.mutation_id,
            intent.title,
            intent.phase
        );
    }
    transaction
        .commit()
        .with_context(|| format!("failed to commit edit response for {}", intent.title))
}

pub(super) fn mark_edit_request_started(
    connection: &Connection,
    intent: &EditMutationIntent,
) -> Result<()> {
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = connection
        .execute(
            "UPDATE sync_edit_mutations
             SET request_started_at_unix = ?1, updated_at_unix = ?1
             WHERE mutation_id = ?2 AND phase = 'intent_persisted'
               AND request_started_at_unix IS NULL",
            params![now, intent.mutation_id],
        )
        .with_context(|| format!("failed to mark edit request {} started", intent.mutation_id))?;
    if changed != 1 {
        anyhow::bail!(
            "edit mutation {} for {} was not ready to start its request",
            intent.mutation_id,
            intent.title
        );
    }
    Ok(())
}

pub(super) fn mark_edit_mutation_unresolved(
    connection: &Connection,
    intent: &EditMutationIntent,
    phase: &'static str,
    detail: &str,
) -> Result<()> {
    if !matches!(phase, "outcome_ambiguous" | "reconciliation_required") {
        anyhow::bail!("invalid unresolved edit-mutation phase {phase}");
    }
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = connection
        .execute(
            "UPDATE sync_edit_mutations
             SET phase = ?1, detail = ?2, updated_at_unix = ?3
             WHERE mutation_id = ?4 AND phase NOT IN ('state_advanced', 'resolved')",
            params![phase, detail, now, intent.mutation_id],
        )
        .with_context(|| {
            format!(
                "failed to mark edit mutation {} unresolved",
                intent.mutation_id
            )
        })?;
    if changed != 1 {
        anyhow::bail!(
            "edit mutation {} for {} is no longer unresolved",
            intent.mutation_id,
            intent.title
        );
    }
    Ok(())
}

pub(super) fn begin_delete_mutation(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    mutation: NewDeleteMutation<'_>,
) -> Result<DeleteMutationIntent> {
    initialize_sync_schema(connection)?;
    if mutation.expected_revision_id < 0 {
        anyhow::bail!(
            "delete intent for {} requires a non-negative observed revision ID",
            mutation.title
        );
    }
    if mutation.reason.trim().is_empty() {
        anyhow::bail!("delete intent for {} requires a reason", mutation.title);
    }
    if mutation.local_effect_policy.backup_enabled
        && (mutation.relative_path.is_none()
            || mutation.local_effect_policy.local_content_sha256.is_none()
            || mutation.local_effect_policy.backup_directory.is_none()
            || mutation.local_effect_policy.backup_path.is_none())
    {
        anyhow::bail!(
            "delete intent for {} has an incomplete local backup policy",
            mutation.title
        );
    }
    let local_effect_status = if mutation.relative_path.is_some()
        && mutation.local_effect_policy.local_content_sha256.is_some()
    {
        "pending"
    } else {
        "not_applicable"
    };
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .with_context(|| {
            format!(
                "failed to begin durable delete intent for {}",
                mutation.title
            )
        })?;
    store::bind_sync_target_for_state_write(paths, &transaction, target)?;
    ensure_no_unresolved_remote_mutation(&transaction, mutation.title)?;
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    transaction
        .execute(
            "INSERT INTO sync_delete_mutations (
                target_api_url, title, relative_path, expected_revision_id, reason, reason_marker,
                phase, backup_enabled, backup_directory, backup_path,
                local_content_sha256, local_effect_status,
                created_at_unix, updated_at_unix
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                'wikitool-delete:' || lower(hex(randomblob(16))),
                'intent_persisted', ?6, ?7, ?8, ?9, ?10, ?11, ?11
             )",
            params![
                target.api_url(),
                mutation.title,
                mutation.relative_path,
                mutation.expected_revision_id,
                mutation.reason.trim(),
                if mutation.local_effect_policy.backup_enabled {
                    1i64
                } else {
                    0i64
                },
                mutation.local_effect_policy.backup_directory,
                mutation.local_effect_policy.backup_path,
                mutation.local_effect_policy.local_content_sha256,
                local_effect_status,
                now,
            ],
        )
        .with_context(|| format!("failed to persist delete intent for {}", mutation.title))?;
    let mutation_id = transaction.last_insert_rowid();
    let reason_marker = transaction
        .query_row(
            "SELECT reason_marker FROM sync_delete_mutations WHERE mutation_id = ?1",
            [mutation_id],
            |row| row.get::<_, String>(0),
        )
        .context("failed to read persisted delete mutation marker")?;
    transaction
        .commit()
        .with_context(|| format!("failed to commit delete intent for {}", mutation.title))?;
    Ok(DeleteMutationIntent {
        mutation_id,
        target_api_url: target.api_url().to_string(),
        title: mutation.title.to_string(),
        relative_path: mutation.relative_path.map(ToString::to_string),
        expected_revision_id: mutation.expected_revision_id,
        reason: mutation.reason.trim().to_string(),
        reason_marker,
        phase: "intent_persisted".to_string(),
        response_kind: None,
        response_log_id: None,
        request_started_at_unix: None,
        terminal_outcome: None,
        backup_enabled: mutation.local_effect_policy.backup_enabled,
        backup_directory: mutation.local_effect_policy.backup_directory.clone(),
        backup_path: mutation.local_effect_policy.backup_path.clone(),
        local_content_sha256: mutation.local_effect_policy.local_content_sha256.clone(),
        local_effect_status: local_effect_status.to_string(),
    })
}

pub(super) fn mark_delete_request_started(
    connection: &Connection,
    intent: &DeleteMutationIntent,
) -> Result<()> {
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = connection
        .execute(
            "UPDATE sync_delete_mutations
             SET request_started_at_unix = ?1, updated_at_unix = ?1
             WHERE mutation_id = ?2 AND phase = 'intent_persisted'
               AND request_started_at_unix IS NULL",
            params![now, intent.mutation_id],
        )
        .with_context(|| {
            format!(
                "failed to mark delete request {} started",
                intent.mutation_id
            )
        })?;
    if changed != 1 {
        anyhow::bail!(
            "delete mutation {} for {} was not ready to start its request",
            intent.mutation_id,
            intent.title
        );
    }
    Ok(())
}

pub(super) fn complete_edit_mutation_without_state_change(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    intent: &EditMutationIntent,
    terminal_outcome: &'static str,
    detail: &str,
) -> Result<()> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .context("failed to begin terminal edit reconciliation")?;
    store::bind_sync_target_for_state_write(paths, &transaction, target)?;
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = transaction
        .execute(
            "UPDATE sync_edit_mutations
             SET phase = 'resolved', terminal_outcome = ?1, detail = ?2,
                 updated_at_unix = ?3
             WHERE mutation_id = ?4 AND phase NOT IN ('state_advanced', 'resolved')",
            params![terminal_outcome, detail, now, intent.mutation_id],
        )
        .context("failed to complete edit reconciliation")?;
    if changed != 1 {
        anyhow::bail!("edit mutation {} is already terminal", intent.mutation_id);
    }
    transaction
        .commit()
        .context("failed to commit terminal edit reconciliation")
}

pub(super) fn complete_delete_mutation_without_state_change(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    intent: &DeleteMutationIntent,
    terminal_outcome: &'static str,
    detail: &str,
) -> Result<()> {
    let current = claim_existing_delete_staging(paths, connection, target, intent)?;
    let local_recovery_detail = matches!(
        current.local_effect_status.as_str(),
        "source_staging" | "source_staged"
    )
    .then(|| recover_staged_delete_for_retained_state(paths, &current))
    .transpose()?;
    let durable_detail = local_recovery_detail
        .as_deref()
        .map_or_else(|| detail.to_string(), |local| format!("{detail}; {local}"));
    let next_local_effect_status = if local_recovery_detail.is_some() {
        "complete"
    } else {
        current.local_effect_status.as_str()
    };
    #[cfg(test)]
    if local_recovery_detail.is_some() {
        fail_if_injected(SyncStateFaultPoint::AfterRetainedDeleteLocalRecovery)?;
    }
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .context("failed to begin terminal delete reconciliation")?;
    store::bind_sync_target_for_state_write(paths, &transaction, target)?;
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = transaction
        .execute(
            "UPDATE sync_delete_mutations
             SET phase = 'resolved', terminal_outcome = ?1, detail = ?2,
                 local_effect_status = ?3, updated_at_unix = ?4
             WHERE mutation_id = ?5
               AND phase NOT IN ('state_advanced', 'resolved')
               AND local_effect_status = ?6",
            params![
                terminal_outcome,
                durable_detail,
                next_local_effect_status,
                now,
                current.mutation_id,
                current.local_effect_status,
            ],
        )
        .context("failed to complete delete reconciliation")?;
    if changed != 1 {
        let latest = load_delete_mutation(&transaction, current.mutation_id)?;
        anyhow::bail!(
            "delete mutation {} changed while terminal local recovery was being recorded: phase {:?}, local-effect status {:?}",
            current.mutation_id,
            latest.phase,
            latest.local_effect_status
        );
    }
    transaction
        .commit()
        .context("failed to commit terminal delete reconciliation")
}

pub(super) fn load_delete_mutation(
    connection: &Connection,
    mutation_id: i64,
) -> Result<DeleteMutationIntent> {
    connection
        .query_row(
            "SELECT mutation_id, target_api_url, title, relative_path, expected_revision_id, reason, reason_marker,
                    phase, response_kind, response_title, response_log_id,
                    response_log_timestamp, request_started_at_unix,
                    terminal_outcome, backup_enabled, backup_directory, backup_path,
                    local_content_sha256, local_effect_status
             FROM sync_delete_mutations
             WHERE mutation_id = ?1",
            [mutation_id],
            |row| {
                Ok(DeleteMutationIntent {
                    mutation_id: row.get(0)?,
                    target_api_url: row.get(1)?,
                    title: row.get(2)?,
                    relative_path: row.get(3)?,
                    expected_revision_id: row.get(4)?,
                    reason: row.get(5)?,
                    reason_marker: row.get(6)?,
                    phase: row.get(7)?,
                    response_kind: row.get(8)?,
                    response_log_id: row.get(10)?,
                    request_started_at_unix: row.get(12)?,
                    terminal_outcome: row.get(13)?,
                    backup_enabled: row.get::<_, i64>(14)? != 0,
                    backup_directory: row.get(15)?,
                    backup_path: row.get(16)?,
                    local_content_sha256: row.get(17)?,
                    local_effect_status: row.get(18)?,
                })
            },
        )
        .with_context(|| format!("delete mutation {mutation_id} does not exist"))
}

pub(super) fn bind_delete_response(
    connection: &Connection,
    intent: &DeleteMutationIntent,
    response_kind: &'static str,
    response_title: &str,
    response_log_id: Option<i64>,
    response_log_timestamp: Option<&str>,
) -> Result<()> {
    if !matches!(response_kind, "deleted" | "already_missing") {
        anyhow::bail!("invalid delete response kind {response_kind}");
    }
    if response_kind == "deleted" && response_log_id.is_none() {
        anyhow::bail!("deleted response requires a MediaWiki log ID");
    }
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = connection
        .execute(
            "UPDATE sync_delete_mutations
             SET phase = 'response_bound', response_kind = ?1, response_title = ?2,
                 response_log_id = ?3, response_log_timestamp = ?4,
                 detail = NULL, updated_at_unix = ?5
             WHERE mutation_id = ?6
               AND phase IN ('intent_persisted', 'outcome_ambiguous', 'reconciliation_required')",
            params![
                response_kind,
                response_title,
                response_log_id,
                response_log_timestamp,
                now,
                intent.mutation_id,
            ],
        )
        .with_context(|| format!("failed to bind delete response for {}", intent.title))?;
    if changed != 1 {
        anyhow::bail!(
            "delete mutation {} for {} cannot bind a response from phase {}",
            intent.mutation_id,
            intent.title,
            intent.phase
        );
    }
    Ok(())
}

pub(super) fn mark_delete_mutation_unresolved(
    connection: &Connection,
    intent: &DeleteMutationIntent,
    phase: &'static str,
    detail: &str,
) -> Result<()> {
    if !matches!(phase, "outcome_ambiguous" | "reconciliation_required") {
        anyhow::bail!("invalid unresolved delete-mutation phase {phase}");
    }
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = connection
        .execute(
            "UPDATE sync_delete_mutations
             SET phase = ?1, detail = ?2, updated_at_unix = ?3
             WHERE mutation_id = ?4 AND phase NOT IN ('state_advanced', 'resolved')",
            params![phase, detail, now, intent.mutation_id],
        )
        .with_context(|| {
            format!(
                "failed to mark delete mutation {} unresolved",
                intent.mutation_id
            )
        })?;
    if changed != 1 {
        anyhow::bail!(
            "delete mutation {} for {} is no longer unresolved",
            intent.mutation_id,
            intent.title
        );
    }
    Ok(())
}

pub(super) fn prepare_delete_local_effects(
    paths: &SyncProjectPaths,
    connection: &Connection,
    intent: &DeleteMutationIntent,
) -> Result<()> {
    if matches!(
        intent.local_effect_status.as_str(),
        "not_applicable" | "backup_ready" | "source_staging" | "source_staged" | "complete"
    ) {
        return Ok(());
    }
    if intent.local_effect_status != "pending" {
        bail!(
            "delete mutation {} has unknown local-effect status {:?}",
            intent.mutation_id,
            intent.local_effect_status
        );
    }
    let relative_path = intent
        .relative_path
        .as_deref()
        .context("pending delete local effect has no relative path")?;
    let expected_hash = intent
        .local_content_sha256
        .as_deref()
        .context("pending delete local effect has no exact content hash")?;
    let source = validated_delete_source_path(paths, relative_path)?;
    let content = fs::read_to_string(&source)
        .with_context(|| format!("failed to read planned delete source {}", source.display()))?;
    require_exact_local_delete_hash(&source, &content, expected_hash)?;

    if intent.backup_enabled {
        let backup_directory = intent
            .backup_directory
            .as_deref()
            .context("enabled delete backup has no directory")?;
        let backup_path = intent
            .backup_path
            .as_deref()
            .context("enabled delete backup has no path")?;
        let backup = PathBuf::from(backup_path);
        let directory = PathBuf::from(backup_directory);
        validate_sync_state_path(paths, &directory)?;
        validate_sync_state_path(paths, &backup)?;
        if backup.parent() != Some(directory.as_path()) {
            bail!(
                "delete backup path {} is not an immediate child of bound directory {}",
                backup.display(),
                directory.display()
            );
        }
        fs::create_dir_all(&directory).with_context(|| {
            format!(
                "failed to create delete backup directory {}",
                directory.display()
            )
        })?;
        publish_delete_backup_noclobber(&directory, &backup, &content, expected_hash)?;
    }

    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = connection
        .execute(
            "UPDATE sync_delete_mutations
             SET local_effect_status = 'backup_ready', updated_at_unix = ?1
             WHERE mutation_id = ?2 AND local_effect_status = 'pending'",
            params![now, intent.mutation_id],
        )
        .context("failed to persist prepared delete local effects")?;
    if changed != 1 {
        bail!(
            "delete mutation {} local effects changed while preparing backup",
            intent.mutation_id
        );
    }
    Ok(())
}

pub(super) fn publish_delete_backup_noclobber(
    directory: &Path,
    backup: &Path,
    content: &str,
    expected_hash: &str,
) -> Result<()> {
    let mut temporary = tempfile::Builder::new()
        .prefix(".wikitool-delete-backup-")
        .suffix(".tmp")
        .tempfile_in(directory)
        .with_context(|| format!("failed to stage delete backup in {}", directory.display()))?;
    temporary
        .write_all(content.as_bytes())
        .with_context(|| format!("failed to stage delete backup {}", backup.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("failed to flush staged delete backup {}", backup.display()))?;

    match temporary.persist_noclobber(backup) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            drop(error);
            let existing = fs::read_to_string(backup).with_context(|| {
                format!(
                    "failed to verify existing delete backup {}",
                    backup.display()
                )
            })?;
            require_exact_local_delete_hash(backup, &existing, expected_hash)
        }
        Err(error) => Err(error.error)
            .with_context(|| format!("failed to publish delete backup {}", backup.display())),
    }
}

fn publish_delete_source_noclobber(
    paths: &SyncProjectPaths,
    relative_path: &str,
    content: &str,
    expected_hash: &str,
) -> Result<bool> {
    let source = validated_delete_source_path(paths, relative_path)?;
    if source.exists() {
        let existing = fs::read_to_string(&source)
            .with_context(|| format!("failed to inspect retained source {}", source.display()))?;
        return Ok(compute_sha256(&existing) == expected_hash);
    }
    let directory = source
        .parent()
        .context("planned delete source has no parent directory")?;
    fs::create_dir_all(directory)
        .with_context(|| format!("failed to create source directory {}", directory.display()))?;
    validate_local_source_path(paths, directory)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".wikitool-delete-restore-")
        .suffix(".tmp")
        .tempfile_in(directory)
        .with_context(|| {
            format!(
                "failed to stage no-clobber source restoration in {}",
                directory.display()
            )
        })?;
    temporary
        .write_all(content.as_bytes())
        .with_context(|| format!("failed to stage source restoration {}", source.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("failed to flush source restoration {}", source.display()))?;

    match temporary.persist_noclobber(&source) {
        Ok(_) => Ok(true),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            drop(error);
            let source = validated_delete_source_path(paths, relative_path)?;
            let existing = fs::read_to_string(&source).with_context(|| {
                format!(
                    "failed to inspect concurrently published source {}",
                    source.display()
                )
            })?;
            Ok(compute_sha256(&existing) == expected_hash)
        }
        Err(error) => Err(error.error).with_context(|| {
            format!(
                "failed to publish no-clobber source restoration {}",
                source.display()
            )
        }),
    }
}

fn recover_staged_delete_for_retained_state(
    paths: &SyncProjectPaths,
    intent: &DeleteMutationIntent,
) -> Result<String> {
    let relative_path = intent
        .relative_path
        .as_deref()
        .context("staged delete local effect has no relative path")?;
    let expected_hash = intent
        .local_content_sha256
        .as_deref()
        .context("staged delete local effect has no exact content hash")?;
    let source = validated_delete_source_path(paths, relative_path)?;
    let staging = delete_staging_path(paths, intent, expected_hash)?;
    validate_sync_state_path(paths, &staging)?;
    let state_dir = paths
        .sync_store_path
        .parent()
        .context("sync store path has no parent")?;
    let short_hash = expected_hash.get(..16).unwrap_or(expected_hash);
    let recovery_directory = state_dir.join("delete-recovery");
    let recovery =
        recovery_directory.join(format!("mutation-{}-{short_hash}.wiki", intent.mutation_id));
    validate_sync_state_path(paths, &recovery_directory)?;
    validate_sync_state_path(paths, &recovery)?;

    if staging.is_file() {
        let staged_content = fs::read_to_string(&staging).with_context(|| {
            format!(
                "failed to inspect retained delete staging {}",
                staging.display()
            )
        })?;
        require_exact_local_delete_hash(&staging, &staged_content, expected_hash)?;
        fs::create_dir_all(&recovery_directory).with_context(|| {
            format!(
                "failed to create delete recovery directory {}",
                recovery_directory.display()
            )
        })?;
        publish_delete_backup_noclobber(
            &recovery_directory,
            &recovery,
            &staged_content,
            expected_hash,
        )?;
        let source_restored =
            publish_delete_source_noclobber(paths, relative_path, &staged_content, expected_hash)?;
        let staged_content = fs::read_to_string(&staging).with_context(|| {
            format!(
                "failed to re-verify retained delete staging before cleanup: {}",
                staging.display()
            )
        })?;
        require_exact_local_delete_hash(&staging, &staged_content, expected_hash).with_context(
            || {
                format!(
                    "delete staging changed during retained-state recovery; bytes were preserved at {}",
                    staging.display()
                )
            },
        )?;
        fs::remove_file(&staging).with_context(|| {
            format!(
                "failed to remove recovered delete staging {}",
                staging.display()
            )
        })?;
        let source_disposition = if source_restored {
            format!("exact source bytes are present at {}", source.display())
        } else {
            format!(
                "a concurrently present source at {} was preserved unchanged",
                source.display()
            )
        };
        return Ok(format!(
            "exact staged bytes were retained in no-clobber recovery copy {}; {source_disposition}",
            recovery.display()
        ));
    }

    if recovery.is_file() {
        let recovery_content = fs::read_to_string(&recovery).with_context(|| {
            format!(
                "failed to inspect existing delete recovery copy {}",
                recovery.display()
            )
        })?;
        require_exact_local_delete_hash(&recovery, &recovery_content, expected_hash)?;
        let source_restored = publish_delete_source_noclobber(
            paths,
            relative_path,
            &recovery_content,
            expected_hash,
        )?;
        let source_disposition = if source_restored {
            format!("exact source bytes are present at {}", source.display())
        } else {
            format!(
                "a concurrently present source at {} was preserved unchanged",
                source.display()
            )
        };
        return Ok(format!(
            "exact staged bytes remain in no-clobber recovery copy {}; {source_disposition}",
            recovery.display()
        ));
    }

    if source.is_file() {
        let source_content = fs::read_to_string(&source)
            .with_context(|| format!("failed to inspect retained source {}", source.display()))?;
        let source_detail = if compute_sha256(&source_content) == expected_hash {
            "the exact planned source is present"
        } else {
            "a different concurrent source is present and was preserved unchanged"
        };
        return Ok(format!(
            "delete staging is absent; {source_detail} at {}",
            source.display()
        ));
    }

    if intent.backup_enabled {
        verify_bound_delete_backup(paths, intent)?;
        let backup = PathBuf::from(
            intent
                .backup_path
                .as_deref()
                .context("enabled delete backup has no path")?,
        );
        let backup_content = fs::read_to_string(&backup)
            .with_context(|| format!("failed to read exact delete backup {}", backup.display()))?;
        let source_restored =
            publish_delete_source_noclobber(paths, relative_path, &backup_content, expected_hash)?;
        let source_disposition = if source_restored {
            format!("exact source bytes were restored at {}", source.display())
        } else {
            format!(
                "a concurrently present source at {} was preserved unchanged",
                source.display()
            )
        };
        return Ok(format!(
            "delete staging was absent, but exact bound backup {} was retained; {source_disposition}",
            backup.display()
        ));
    }

    Ok(format!(
        "delete staging and source are both absent under the explicit no-backup policy; no local recovery is claimed for SHA-256 {expected_hash}"
    ))
}

struct StagedDeleteLocalSource {
    path: PathBuf,
    allow_missing: bool,
    preserved_source: bool,
}

fn transition_delete_local_effect_status(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    mutation_id: i64,
    expected: &str,
    next: &str,
) -> Result<DeleteMutationIntent> {
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .context("failed to begin durable local-delete status transition")?;
    store::bind_sync_target_for_state_write(paths, &transaction, target)?;
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = transaction
        .execute(
            "UPDATE sync_delete_mutations
             SET local_effect_status = ?1, updated_at_unix = ?2
             WHERE mutation_id = ?3
               AND local_effect_status = ?4
               AND phase NOT IN ('state_advanced', 'resolved')",
            params![next, now, mutation_id, expected],
        )
        .with_context(|| {
            format!(
                "failed to transition delete mutation {mutation_id} local effects from {expected} to {next}"
            )
        })?;
    if changed != 1 {
        bail!(
            "delete mutation {mutation_id} did not retain local-effect status {expected:?} before transition to {next:?}"
        );
    }
    transaction
        .commit()
        .context("failed to commit durable local-delete status transition")?;
    load_delete_mutation(connection, mutation_id)
}

pub(super) fn claim_existing_delete_staging(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    intent: &DeleteMutationIntent,
) -> Result<DeleteMutationIntent> {
    let current = load_delete_mutation(connection, intent.mutation_id)?;
    if !matches!(
        current.local_effect_status.as_str(),
        "pending" | "backup_ready"
    ) {
        return Ok(current);
    }
    let Some(relative_path) = current.relative_path.as_deref() else {
        return Ok(current);
    };
    let expected_hash = current
        .local_content_sha256
        .as_deref()
        .context("tracked delete local effect has no exact content hash")?;
    let staging = delete_staging_path(paths, &current, expected_hash)?;
    validate_sync_state_path(paths, &staging)?;
    if !staging.is_file() {
        return Ok(current);
    }
    let content = fs::read_to_string(&staging).with_context(|| {
        format!(
            "failed to inspect pre-marker delete staging for {relative_path}: {}",
            staging.display()
        )
    })?;
    require_exact_local_delete_hash(&staging, &content, expected_hash).with_context(|| {
        format!(
            "delete mutation {} has an untrusted pre-marker staging artifact; terminalization is forbidden",
            current.mutation_id
        )
    })?;
    transition_delete_local_effect_status(
        paths,
        connection,
        target,
        current.mutation_id,
        &current.local_effect_status,
        "source_staging",
    )
}

fn stage_delete_local_source(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    intent: &DeleteMutationIntent,
) -> Result<Option<StagedDeleteLocalSource>> {
    let mut current = load_delete_mutation(connection, intent.mutation_id)?;
    if current.local_effect_status == "not_applicable" {
        return Ok(None);
    }
    if current.local_effect_status == "pending" {
        prepare_delete_local_effects(paths, connection, &current)?;
        current = load_delete_mutation(connection, intent.mutation_id)?;
    }
    if !matches!(
        current.local_effect_status.as_str(),
        "backup_ready" | "source_staging" | "source_staged"
    ) {
        bail!(
            "delete mutation {} cannot stage local effects from status {:?}",
            current.mutation_id,
            current.local_effect_status
        );
    }
    verify_bound_delete_backup(paths, &current)?;
    let relative_path = current
        .relative_path
        .clone()
        .context("delete local effect has no relative path")?;
    let expected_hash = current
        .local_content_sha256
        .clone()
        .context("delete local effect has no exact content hash")?;
    let source = validated_delete_source_path(paths, &relative_path)?;
    let staging = delete_staging_path(paths, &current, &expected_hash)?;
    validate_sync_state_path(paths, &staging)?;

    if current.local_effect_status == "backup_ready" {
        current = transition_delete_local_effect_status(
            paths,
            connection,
            target,
            current.mutation_id,
            "backup_ready",
            "source_staging",
        )?;
    }

    let source_exists = source.is_file();
    let staging_exists = staging.is_file();

    if current.local_effect_status == "source_staged" {
        if staging_exists {
            let content = fs::read_to_string(&staging).with_context(|| {
                format!("failed to read staged delete source {}", staging.display())
            })?;
            require_exact_local_delete_hash(&staging, &content, &expected_hash)?;
        }
        return Ok(Some(StagedDeleteLocalSource {
            path: staging,
            allow_missing: !staging_exists,
            preserved_source: source_exists,
        }));
    }

    debug_assert_eq!(current.local_effect_status, "source_staging");
    match (source_exists, staging_exists) {
        (true, false) => {
            let content = fs::read_to_string(&source)
                .with_context(|| format!("failed to read delete source {}", source.display()))?;
            require_exact_local_delete_hash(&source, &content, &expected_hash)?;
            ensure_parent_dir(&staging)?;
            fs::rename(&source, &staging).with_context(|| {
                format!(
                    "failed to atomically stage delete source {} at {}",
                    source.display(),
                    staging.display()
                )
            })?;
            #[cfg(test)]
            fail_if_injected(SyncStateFaultPoint::AfterSourceRenameBeforeStagedMarker)?;
            let staged_content = fs::read_to_string(&staging).with_context(|| {
                format!(
                    "failed to verify newly staged delete source {}",
                    staging.display()
                )
            })?;
            if let Err(hash_error) =
                require_exact_local_delete_hash(&staging, &staged_content, &expected_hash)
            {
                return Err(hash_error).with_context(|| {
                    format!(
                        "delete source changed while it was being staged; the mismatched bytes were preserved at {} and were not restored because the source path may have been recreated concurrently",
                        staging.display()
                    )
                });
            }
        }
        (false, true) => {
            let content = fs::read_to_string(&staging).with_context(|| {
                format!("failed to read staged delete source {}", staging.display())
            })?;
            require_exact_local_delete_hash(&staging, &content, &expected_hash)?;
        }
        (true, true) => {
            let content = fs::read_to_string(&staging).with_context(|| {
                format!("failed to read staged delete source {}", staging.display())
            })?;
            require_exact_local_delete_hash(&staging, &content, &expected_hash).with_context(
                || {
                    format!(
                        "delete source {} was recreated while recovery staging {} was owned; both paths were preserved",
                        source.display(),
                        staging.display()
                    )
                },
            )?;
        }
        (false, false) => bail!(
            "delete mutation {} owns a source-staging transition, but source {} and recovery staging {} are both missing; no terminal local-effect claim can be made",
            current.mutation_id,
            source.display(),
            staging.display()
        ),
    }

    transition_delete_local_effect_status(
        paths,
        connection,
        target,
        current.mutation_id,
        "source_staging",
        "source_staged",
    )?;
    Ok(Some(StagedDeleteLocalSource {
        path: staging,
        allow_missing: false,
        preserved_source: source_exists,
    }))
}

pub(super) fn verify_bound_delete_backup(
    paths: &SyncProjectPaths,
    intent: &DeleteMutationIntent,
) -> Result<()> {
    if !intent.backup_enabled {
        return Ok(());
    }
    let directory = PathBuf::from(
        intent
            .backup_directory
            .as_deref()
            .context("enabled delete backup has no directory")?,
    );
    let backup = PathBuf::from(
        intent
            .backup_path
            .as_deref()
            .context("enabled delete backup has no path")?,
    );
    validate_sync_state_path(paths, &directory)?;
    validate_sync_state_path(paths, &backup)?;
    if backup.parent() != Some(directory.as_path()) {
        bail!(
            "delete backup path {} is not an immediate child of bound directory {}",
            backup.display(),
            directory.display()
        );
    }
    let expected_hash = intent
        .local_content_sha256
        .as_deref()
        .context("enabled delete backup has no exact content hash")?;
    let content = fs::read_to_string(&backup)
        .with_context(|| format!("bound delete backup is unavailable: {}", backup.display()))?;
    require_exact_local_delete_hash(&backup, &content, expected_hash)
}

fn delete_staging_path(
    paths: &SyncProjectPaths,
    intent: &DeleteMutationIntent,
    content_sha256: &str,
) -> Result<PathBuf> {
    let state_dir = paths
        .sync_store_path
        .parent()
        .context("sync store path has no parent")?;
    let short_hash = content_sha256.get(..16).unwrap_or(content_sha256);
    Ok(state_dir
        .join("delete-staging")
        .join(format!("mutation-{}-{short_hash}.wiki", intent.mutation_id)))
}

pub(super) fn validated_delete_source_path(
    paths: &SyncProjectPaths,
    relative_path: &str,
) -> Result<PathBuf> {
    let candidate = Path::new(relative_path);
    if candidate.is_absolute()
        || relative_path.contains('\\')
        || candidate.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        bail!("invalid delete cleanup relative path {relative_path:?}");
    }
    let absolute = absolute_path_from_relative(paths, relative_path);
    validate_local_source_path(paths, &absolute)?;
    Ok(absolute)
}

fn require_exact_local_delete_hash(path: &Path, content: &str, expected: &str) -> Result<()> {
    let actual = compute_sha256(content);
    if actual != expected {
        bail!(
            "planned delete source {} changed: expected SHA-256 {expected}, found {actual}",
            path.display()
        );
    }
    Ok(())
}

pub(super) fn advance_verified_delete_mutation(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    intent: &DeleteMutationIntent,
) -> Result<()> {
    initialize_sync_schema(connection)?;
    let staged_source = stage_delete_local_source(paths, connection, target, intent)?;
    let preserved_source = staged_source
        .as_ref()
        .is_some_and(|staged| staged.preserved_source);
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .with_context(|| {
            format!(
                "failed to begin verified delete-state advancement for {}",
                intent.title
            )
        })?;
    store::bind_sync_target_for_state_write(paths, &transaction, target)?;
    remove_sync_ledger_entry(&transaction, &intent.title)?;
    #[cfg(test)]
    fail_if_injected(SyncStateFaultPoint::AfterLedgerDelete)?;
    remove_sync_snapshot(&transaction, &intent.title)?;
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let (next_phase, terminal_outcome, local_effect_status, detail) = if staged_source.is_some() {
        let detail = if preserved_source {
            "remote absence and sync-state removal are verified; the staged original still requires unlink completion, and a concurrently present local source was preserved"
        } else {
            "remote absence and sync-state removal are verified; staged local source still requires unlink completion"
        };
        (
            "reconciliation_required",
            None,
            "source_staged",
            Some(detail),
        )
    } else {
        (
            "state_advanced",
            Some("state_advanced"),
            "not_applicable",
            None,
        )
    };
    let changed = transaction
        .execute(
            "UPDATE sync_delete_mutations
             SET phase = ?1, terminal_outcome = ?2, local_effect_status = ?3,
                 detail = ?4, updated_at_unix = ?5
             WHERE mutation_id = ?6
               AND phase IN ('response_bound', 'reconciliation_required')",
            params![
                next_phase,
                terminal_outcome,
                local_effect_status,
                detail,
                now,
                intent.mutation_id
            ],
        )
        .with_context(|| {
            format!(
                "failed to advance delete mutation {} after verification",
                intent.mutation_id
            )
        })?;
    if changed != 1 {
        anyhow::bail!(
            "delete mutation {} for {} was not response-bound during state advancement",
            intent.mutation_id,
            intent.title
        );
    }
    transaction.commit().with_context(|| {
        format!(
            "failed to atomically commit verified delete and sync state for {}",
            intent.title
        )
    })?;
    if let Some(staged_source) = staged_source {
        let StagedDeleteLocalSource {
            path: staging,
            allow_missing,
            ..
        } = staged_source;
        validate_sync_state_path(paths, &staging)?;
        #[cfg(test)]
        fail_if_injected(SyncStateFaultPoint::BeforeStagedSourceUnlink)?;
        match fs::read_to_string(&staging) {
            Ok(content) => {
                let expected_hash = intent.local_content_sha256.as_deref().context(
                    "staged delete source has no bound exact content hash before unlink",
                )?;
                require_exact_local_delete_hash(&staging, &content, expected_hash).with_context(
                    || {
                        format!(
                            "refusing to unlink changed delete staging for mutation {}; the staged bytes were preserved",
                            intent.mutation_id
                        )
                    },
                )?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => {
                // Crash recovery after the unlink but before the terminal DB
                // update has no staging file left to re-verify.
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to re-verify delete staging before unlink: {}",
                        staging.display()
                    )
                });
            }
        }
        match fs::remove_file(&staging) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => bail!(
                "sync state advanced, but recovery staging cleanup at {} failed: {error}; reconcile mutation {} to finish cleanup",
                staging.display(),
                intent.mutation_id
            ),
        }
        #[cfg(test)]
        fail_if_injected(SyncStateFaultPoint::AfterStagedSourceUnlink)?;
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
            .context("failed to begin terminal local-delete cleanup update")?;
        store::bind_sync_target_for_state_write(paths, &transaction, target)?;
        let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
        let terminal_detail = preserved_source.then_some(
            "local delete cleanup completed; a concurrently present local source was preserved",
        );
        let changed = transaction
            .execute(
                "UPDATE sync_delete_mutations
                 SET phase = 'state_advanced', terminal_outcome = 'state_advanced',
                     local_effect_status = 'complete', detail = ?1,
                     updated_at_unix = ?2
                 WHERE mutation_id = ?3
                   AND phase = 'reconciliation_required'
                   AND local_effect_status = 'source_staged'",
                params![terminal_detail, now, intent.mutation_id],
            )
            .context("failed to mark local delete cleanup complete")?;
        if changed != 1 {
            bail!(
                "delete mutation {} did not retain its staged-cleanup state",
                intent.mutation_id
            );
        }
        transaction
            .commit()
            .context("failed to commit terminal local-delete cleanup update")?;
    }
    Ok(())
}

pub(super) fn advance_verified_edit_mutation(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    intent: &EditMutationIntent,
    page: &RemotePage,
) -> Result<()> {
    initialize_sync_schema(connection)?;
    let (is_redirect, redirect_target) = parse_redirect(&page.content);
    let content_hash = compute_wiki_sync_hash(&page.content);
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .with_context(|| {
            format!(
                "failed to begin verified edit-state advancement for {}",
                intent.title
            )
        })?;
    store::bind_sync_target_for_state_write(paths, &transaction, target)?;
    upsert_sync_ledger(
        &transaction,
        page,
        &intent.relative_path,
        &content_hash,
        is_redirect,
        redirect_target.as_deref(),
    )?;
    #[cfg(test)]
    fail_if_injected(SyncStateFaultPoint::AfterLedgerUpsert)?;
    upsert_sync_snapshot(
        &transaction,
        &page.title,
        &intent.relative_path,
        &page.content,
    )?;
    let now = i64::try_from(unix_timestamp()?).context("timestamp does not fit into i64")?;
    let changed = transaction
        .execute(
            "UPDATE sync_edit_mutations
             SET phase = 'state_advanced', terminal_outcome = 'state_advanced',
                 detail = NULL, updated_at_unix = ?1
             WHERE mutation_id = ?2 AND phase = 'response_bound'",
            params![now, intent.mutation_id],
        )
        .with_context(|| {
            format!(
                "failed to advance edit mutation {} after verification",
                intent.mutation_id
            )
        })?;
    if changed != 1 {
        anyhow::bail!(
            "edit mutation {} for {} was not response-bound during state advancement",
            intent.mutation_id,
            intent.title
        );
    }
    transaction.commit().with_context(|| {
        format!(
            "failed to atomically commit verified edit and sync state for {}",
            intent.title
        )
    })
}

pub(super) fn upsert_sync_page_state(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
    state: SyncPageState<'_>,
) -> Result<()> {
    initialize_sync_schema(connection)?;
    let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
        .with_context(|| format!("failed to begin sync-state update for {}", state.page.title))?;
    store::bind_sync_target_for_state_write(paths, &transaction, target)?;
    upsert_sync_ledger(
        &transaction,
        state.page,
        state.relative_path,
        state.content_hash,
        state.is_redirect,
        state.redirect_target,
    )?;
    #[cfg(test)]
    fail_if_injected(SyncStateFaultPoint::AfterLedgerUpsert)?;
    upsert_sync_snapshot(
        &transaction,
        &state.page.title,
        state.relative_path,
        &state.page.content,
    )?;
    clear_sync_title_invalidation(&transaction, &state.page.title)?;
    transaction.commit().with_context(|| {
        format!(
            "failed to commit sync-state update for {}",
            state.page.title
        )
    })?;
    Ok(())
}

pub(super) fn get_sync_config(connection: &Connection, key: &str) -> Result<Option<String>> {
    if !table_exists(connection, "sync_config")? {
        return Ok(None);
    }
    let mut statement = connection
        .prepare("SELECT value FROM sync_config WHERE key = ?1 LIMIT 1")
        .context("failed to prepare sync config query")?;
    let mut rows = statement
        .query([key])
        .with_context(|| format!("failed to read sync config key {key}"))?;
    let row = match rows.next().context("failed to decode sync config row")? {
        Some(row) => row,
        None => return Ok(None),
    };
    let value = row.get(0).context("failed to decode sync config value")?;
    Ok(Some(value))
}

pub(super) fn set_sync_config(connection: &Connection, key: &str, value: &str) -> Result<()> {
    initialize_sync_schema(connection)?;
    connection
        .execute(
            "INSERT INTO sync_config (key, value) VALUES (?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .with_context(|| format!("failed to set sync config key {key}"))?;
    Ok(())
}

pub(super) fn open_sync_connection(paths: &SyncProjectPaths) -> Result<Connection> {
    store::open_or_create_sync_store(paths)
}

pub(super) fn open_existing_sync_connection(
    paths: &SyncProjectPaths,
    target: &store::SyncTargetIdentity,
) -> Result<Connection> {
    store::require_sync_store(paths, target)
}

pub(super) fn initialize_sync_schema(connection: &Connection) -> Result<()> {
    store::validate_sync_store_connection(connection)
}

pub(super) fn verify_sync_target_for_pull(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &store::SyncTargetIdentity,
) -> Result<()> {
    store::verify_sync_target_for_pull(paths, connection, target)
}

pub(super) fn mark_global_baseline_established(
    paths: &SyncProjectPaths,
    connection: &mut Connection,
    target: &store::SyncTargetIdentity,
) -> Result<()> {
    store::mark_global_baseline_established(paths, connection, target)
}

pub(super) fn global_baseline_is_established(
    paths: &SyncProjectPaths,
    connection: &Connection,
) -> Result<bool> {
    store::global_baseline_is_established(paths, connection)
}

pub(super) fn ensure_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))
}

pub(super) fn absolute_path_from_relative(paths: &SyncProjectPaths, relative: &str) -> PathBuf {
    let mut output = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            output.push(segment);
        }
    }
    output
}

pub(super) fn normalized_title_key(title: &str) -> String {
    mediawiki_title_identity(title)
}

/// MediaWiki title identity for local authority matching: normalize
/// underscore/space spelling, canonicalize known namespace prefixes, and
/// normalize only the first title character while preserving later case.
pub fn mediawiki_title_identity(title: &str) -> String {
    let normalized = normalize_title_for_storage(title);
    let Some((prefix, rest)) = normalized.split_once(':') else {
        return normalize_title_initial(&normalized);
    };
    let Some(namespace) = canonical_namespace_prefix(prefix) else {
        return normalize_title_initial(&normalized);
    };
    format!("{namespace}:{}", normalize_title_initial(rest))
}

pub(super) fn normalize_title_for_storage(title: &str) -> String {
    title.replace('_', " ").trim().to_string()
}

fn canonical_namespace_prefix(prefix: &str) -> Option<&'static str> {
    match prefix.trim().to_ascii_lowercase().as_str() {
        "category" => Some("Category"),
        "file" => Some("File"),
        "user" => Some("User"),
        "template" => Some("Template"),
        "module" => Some("Module"),
        "mediawiki" => Some("MediaWiki"),
        _ => None,
    }
}

fn normalize_title_initial(title: &str) -> String {
    let mut chars = title.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    let mut out = String::new();
    out.extend(first.to_uppercase());
    out.push_str(chars.as_str());
    out
}
