use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde::Serialize;

use crate::filesystem::ScannedFile;
use crate::knowledge::content_index::RebuildReport;

#[derive(Debug, Clone)]
pub struct PullOptions {
    pub namespaces: Vec<i32>,
    pub category: Option<String>,
    pub full: bool,
    pub overwrite_local: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullPageResult {
    pub title: String,
    pub action: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullReport {
    pub success: bool,
    pub requested_pages: usize,
    pub pulled: usize,
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
    pub pages: Vec<PullPageResult>,
    pub request_count: usize,
    pub reindex: Option<RebuildReport>,
}

#[derive(Debug, Clone)]
pub struct PushOptions {
    pub summary: String,
    pub dry_run: bool,
    pub force: bool,
    pub delete: bool,
    pub include_templates: bool,
    pub categories_only: bool,
    pub selection: SyncSelection,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushPageResult {
    pub title: String,
    pub action: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushReport {
    pub success: bool,
    pub dry_run: bool,
    pub pushed: usize,
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub unchanged: usize,
    pub conflicts: Vec<String>,
    pub errors: Vec<String>,
    pub pages: Vec<PushPageResult>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteDeleteStatus {
    Deleted,
    AlreadyMissing,
    SkippedMissingCredentials,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteDeleteReport {
    pub status: RemoteDeleteStatus,
    pub title: String,
    pub detail: Option<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffChangeType {
    NewLocal,
    ModifiedLocal,
    DeletedLocal,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct SyncSelection {
    pub titles: Vec<String>,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffBaselineStatus {
    Available,
    MissingSnapshot,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffChange {
    pub title: String,
    pub change_type: DiffChangeType,
    pub relative_path: String,
    pub local_hash: Option<String>,
    pub synced_hash: Option<String>,
    pub synced_wiki_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_status: Option<DiffBaselineStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unified_diff: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffReport {
    pub new_local: usize,
    pub modified_local: usize,
    pub deleted_local: usize,
    pub conflict_count: usize,
    pub changes: Vec<DiffChange>,
}

#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    pub include_templates: bool,
    pub categories_only: bool,
    pub include_content: bool,
    pub selection: SyncSelection,
}

#[derive(Debug, Clone)]
pub struct SyncPlanOptions {
    pub include_templates: bool,
    pub categories_only: bool,
    pub include_deletes: bool,
    pub include_remote_conflicts: bool,
    pub selection: SyncSelection,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncPlanChange {
    pub title: String,
    pub change_type: DiffChangeType,
    pub relative_path: String,
    pub local_hash: Option<String>,
    pub synced_hash: Option<String>,
    pub synced_wiki_timestamp: Option<String>,
    pub remote_conflict: bool,
    pub remote_wiki_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncPlanReport {
    pub new_local: usize,
    pub modified_local: usize,
    pub deleted_local: usize,
    pub conflict_count: usize,
    pub changes: Vec<SyncPlanChange>,
    pub request_count: usize,
}

#[derive(Debug, Clone)]
pub(super) struct SyncLedgerEntry {
    pub(super) title: String,
    pub(super) namespace: i32,
    pub(super) relative_path: String,
    pub(super) content_hash: String,
    pub(super) wiki_modified_at: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct SyncSnapshotEntry {
    pub(super) title: String,
    pub(super) relative_path: String,
    pub(super) content_text: String,
}

#[derive(Debug, Clone)]
pub(super) struct PlannedSyncChangeInternal {
    pub(super) title: String,
    pub(super) change_type: DiffChangeType,
    pub(super) relative_path: String,
    pub(super) local_hash: Option<String>,
    pub(super) synced_hash: Option<String>,
    pub(super) synced_wiki_timestamp: Option<String>,
    pub(super) remote_conflict: bool,
    pub(super) remote_wiki_timestamp: Option<String>,
}

#[derive(Debug)]
pub(super) struct SyncPlanningContext {
    pub(super) connection: Connection,
    pub(super) local_map: BTreeMap<String, ScannedFile>,
    pub(super) ledger: BTreeMap<String, SyncLedgerEntry>,
    pub(super) changes: Vec<PlannedSyncChangeInternal>,
    pub(super) request_count: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ResolvedSyncSelection {
    pub(super) title_keys: BTreeSet<String>,
    pub(super) exact_paths: BTreeSet<String>,
    pub(super) path_prefixes: Vec<String>,
}
