use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::Serialize;
use similar::TextDiff;

use crate::support::{
    compute_sha256, compute_wiki_sync_hash, normalize_path, normalize_wiki_content, parse_redirect,
    table_exists, unix_timestamp,
};
pub use mediawiki_protocol::{
    DeleteLogEntry, DeleteOutcome, DeleteReceipt, EditConstraint, EditReceipt, NS_CATEGORY,
    NS_MAIN, NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE, PageTimestampInfo, RemotePage,
    RevisionLineageEntry, WikiReadApi, WikiWriteApi,
};
pub use workspace::{
    Namespace, NamespaceMapper, ScanOptions, ScanStats, ScannedFile, SyncNamespace,
    SyncProjectPaths, case_safe_title_relative_path, content_path_to_title, namespace_from_title,
    normalize_separators, relative_path_to_title, scan_files, scan_stats, template_path_to_title,
    title_to_relative_path, validate_local_source_path, validate_scoped_path,
    validate_sync_state_path,
};

mod diff;
mod model;
mod mutations;
mod namespaces;
mod planning;
mod pull;
mod push;
mod remote;
mod storage;
mod store;
mod support;
mod timestamps;
mod workspace;

pub use diff::diff_local_against_sync;
pub use model::*;
pub use mutations::{
    close_remote_mutation, list_remote_mutations, reconcile_remote_mutation_with_api,
    show_remote_mutation,
};
pub use planning::{collect_changed_article_paths, plan_sync_changes, plan_sync_changes_with_api};
pub use pull::pull_from_remote_with_api;
pub use push::push_to_remote_with_api_and_preflight;
pub use remote::{
    RemoteDeleteError, apply_remote_delete_with_api, plan_remote_delete_with_api,
    reconcile_delete_mutation_with_api,
};
pub use storage::mediawiki_title_identity;
pub use store::{
    SyncStateError, SyncStoreMigrationReport, SyncStoreMigrationStatus, SyncTargetIdentity,
    preserve_legacy_sync_state, verify_established_sync_target,
};

/// Generic, serializable evidence returned by a publication preflight. Sync
/// carries this receipt to its plan/write reports but never interprets it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PublicationProvenance {
    pub content_sha256: String,
    pub accepted_at_unix: u64,
    pub prose_origin: String,
    pub editor_identity_assurance: String,
    pub warning_decision: String,
    pub decision_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changeset_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_authority: Option<PublicationAuthorityBinding>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PublicationAuthorityBinding {
    pub target_api_url: String,
    pub site_adapter_id: String,
    pub publication_policy_sha256: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PublicationCandidate<'a> {
    pub title: &'a str,
    pub namespace: &'a str,
    pub is_redirect: bool,
    pub relative_path: &'a str,
    pub absolute_path: &'a Path,
}

#[derive(Debug, Clone)]
pub struct PreparedPublication {
    pub content: String,
    pub provenance: Option<PublicationProvenance>,
}

/// Policy injection point for local content that is about to be planned or
/// written. The implementation owns its workspace and authority state; sync
/// supplies only the exact candidate selected by the current plan.
pub trait PublicationPreflight {
    fn prepare(&self, candidate: PublicationCandidate<'_>) -> Result<PreparedPublication>;
}

use model::{
    PlannedSyncChangeInternal, ResolvedSyncSelection, SyncLedgerEntry, SyncPlanningContext,
    SyncSnapshotEntry,
};
use namespaces::{is_template_namespace_id, namespace_name_to_id};
use planning::{collect_sync_planning_context, count_changes, hydrate_remote_conflicts};
use storage::{
    SyncPageState, absolute_path_from_relative, backfill_sync_snapshots_from_local,
    clear_sync_title_invalidation, ensure_parent_dir, get_sync_config,
    global_baseline_is_established, initialize_sync_schema, load_sync_ledger_map,
    load_sync_snapshot_map, mark_global_baseline_established, normalize_title_for_storage,
    normalized_title_key, open_existing_sync_connection, open_sync_connection,
    remove_sync_page_state, set_sync_config, unresolved_remote_mutation, upsert_sync_page_state,
    verify_sync_target_for_pull,
};
use timestamps::timestamps_match_with_tolerance;

#[cfg(test)]
mod tests;
