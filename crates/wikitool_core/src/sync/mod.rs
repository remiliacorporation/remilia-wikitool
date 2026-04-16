use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::Serialize;
use similar::TextDiff;

use crate::filesystem::{
    NamespaceMapper, ScanOptions, ScannedFile, scan_files, validate_scoped_path,
};
use crate::knowledge::content_index::{RebuildReport, rebuild_index};
pub use crate::mw::{
    ExternalSearchHit, ExternalSearchReport, MediaWikiClient, MediaWikiClientConfig,
    MediaWikiSearchOptions, MediaWikiSearchWhat, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE,
    NS_TEMPLATE, PageTimestampInfo, RemotePage, WikiReadApi, WikiWriteApi, search_pages_report,
};
use crate::runtime::ResolvedPaths;
use crate::schema::{ensure_database_schema_connection, open_initialized_database_connection};
use crate::support::{compute_hash, normalize_path, parse_redirect, table_exists, unix_timestamp};

mod namespaces;
mod timestamps;

use namespaces::{is_template_namespace_id, namespace_name_to_id};
use timestamps::timestamps_match_with_tolerance;

#[cfg(test)]
pub(crate) use namespaces::{
    SiteInfoNamespace, namespace_display_name, should_include_discovered_namespace,
};

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
struct SyncLedgerEntry {
    title: String,
    namespace: i32,
    relative_path: String,
    content_hash: String,
    wiki_modified_at: Option<String>,
}

#[derive(Debug, Clone)]
struct SyncSnapshotEntry {
    title: String,
    relative_path: String,
    content_text: String,
}

#[derive(Debug, Clone)]
struct PlannedSyncChangeInternal {
    title: String,
    change_type: DiffChangeType,
    relative_path: String,
    local_hash: Option<String>,
    synced_hash: Option<String>,
    synced_wiki_timestamp: Option<String>,
    remote_conflict: bool,
    remote_wiki_timestamp: Option<String>,
}

#[derive(Debug)]
struct SyncPlanningContext {
    connection: Connection,
    local_map: BTreeMap<String, ScannedFile>,
    ledger: BTreeMap<String, SyncLedgerEntry>,
    changes: Vec<PlannedSyncChangeInternal>,
    request_count: usize,
}

#[derive(Debug, Clone, Default)]
struct ResolvedSyncSelection {
    title_keys: BTreeSet<String>,
    exact_paths: BTreeSet<String>,
    path_prefixes: Vec<String>,
}

pub fn pull_from_remote(paths: &ResolvedPaths, options: &PullOptions) -> Result<PullReport> {
    pull_from_remote_with_config(paths, options, &crate::config::WikiConfig::default())
}

pub fn pull_from_remote_with_config(
    paths: &ResolvedPaths,
    options: &PullOptions,
    config: &crate::config::WikiConfig,
) -> Result<PullReport> {
    let mut client = MediaWikiClient::from_config(config)?;
    pull_from_remote_with_api(paths, options, &mut client)
}

pub fn search_external_wiki(
    query: &str,
    namespaces: &[i32],
    limit: usize,
) -> Result<Vec<ExternalSearchHit>> {
    Ok(search_external_wiki_report(query, namespaces, limit, MediaWikiSearchWhat::Text)?.hits)
}

pub fn search_external_wiki_report(
    query: &str,
    namespaces: &[i32],
    limit: usize,
    what: MediaWikiSearchWhat,
) -> Result<ExternalSearchReport> {
    search_external_wiki_report_with_config(
        query,
        namespaces,
        limit,
        what,
        &crate::config::WikiConfig::default(),
    )
}

pub fn search_external_wiki_with_config(
    query: &str,
    namespaces: &[i32],
    limit: usize,
    config: &crate::config::WikiConfig,
) -> Result<Vec<ExternalSearchHit>> {
    Ok(search_external_wiki_report_with_config(
        query,
        namespaces,
        limit,
        MediaWikiSearchWhat::Text,
        config,
    )?
    .hits)
}

pub fn search_external_wiki_report_with_config(
    query: &str,
    namespaces: &[i32],
    limit: usize,
    what: MediaWikiSearchWhat,
    config: &crate::config::WikiConfig,
) -> Result<ExternalSearchReport> {
    let mut client = MediaWikiClient::from_config(config)?;
    search_pages_report(
        &mut client,
        query,
        &MediaWikiSearchOptions {
            namespaces: namespaces.to_vec(),
            limit,
            what,
        },
    )
}

pub fn push_to_remote(paths: &ResolvedPaths, options: &PushOptions) -> Result<PushReport> {
    push_to_remote_with_config(paths, options, &crate::config::WikiConfig::default())
}

pub fn push_to_remote_with_config(
    paths: &ResolvedPaths,
    options: &PushOptions,
    config: &crate::config::WikiConfig,
) -> Result<PushReport> {
    let mut client = MediaWikiClient::from_config(config)?;
    let credentials = if options.dry_run {
        None
    } else {
        let username = env::var("WIKI_BOT_USER")
            .map_err(|_| anyhow::anyhow!("WIKI_BOT_USER is required for push"))?;
        let password = env::var("WIKI_BOT_PASS")
            .map_err(|_| anyhow::anyhow!("WIKI_BOT_PASS is required for push"))?;
        Some((username, password))
    };
    push_to_remote_with_api(
        paths,
        options,
        &mut client,
        credentials
            .as_ref()
            .map(|(user, pass)| (user.as_str(), pass.as_str())),
    )
}

pub fn delete_remote_page(title: &str, reason: &str) -> Result<RemoteDeleteReport> {
    delete_remote_page_with_config(title, reason, &crate::config::WikiConfig::default())
}

pub fn delete_remote_page_with_config(
    title: &str,
    reason: &str,
    config: &crate::config::WikiConfig,
) -> Result<RemoteDeleteReport> {
    let username = match env::var("WIKI_BOT_USER") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            return Ok(RemoteDeleteReport {
                status: RemoteDeleteStatus::SkippedMissingCredentials,
                title: title.to_string(),
                detail: Some("WIKI_BOT_USER is not set".to_string()),
                request_count: 0,
            });
        }
    };
    let password = match env::var("WIKI_BOT_PASS") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            return Ok(RemoteDeleteReport {
                status: RemoteDeleteStatus::SkippedMissingCredentials,
                title: title.to_string(),
                detail: Some("WIKI_BOT_PASS is not set".to_string()),
                request_count: 0,
            });
        }
    };

    let mut client = MediaWikiClient::from_config(config)?;
    client
        .login(username.trim(), password.trim())
        .context("remote delete login failed")?;

    match client.delete_page(title, reason) {
        Ok(()) => Ok(RemoteDeleteReport {
            status: RemoteDeleteStatus::Deleted,
            title: title.to_string(),
            detail: None,
            request_count: client.request_count(),
        }),
        Err(error) => {
            let message = error.to_string();
            if message.contains("missingtitle") {
                Ok(RemoteDeleteReport {
                    status: RemoteDeleteStatus::AlreadyMissing,
                    title: title.to_string(),
                    detail: Some(message),
                    request_count: client.request_count(),
                })
            } else {
                Err(error).context(format!("remote delete failed for {title}"))
            }
        }
    }
}

pub fn discover_custom_namespaces(
    config: &crate::config::WikiConfig,
) -> Result<Vec<crate::config::CustomNamespace>> {
    if config.api_url_owned().is_none() {
        bail!("wiki API URL is not configured (set [wiki].api_url or WIKI_API_URL)");
    }
    let mut client = MediaWikiClient::from_config(config)?;
    client.discover_custom_namespaces()
}

pub fn plan_sync_changes(
    paths: &ResolvedPaths,
    options: &SyncPlanOptions,
) -> Result<Option<SyncPlanReport>> {
    plan_sync_changes_with_config(paths, options, &crate::config::WikiConfig::default())
}

pub fn plan_sync_changes_with_config(
    paths: &ResolvedPaths,
    options: &SyncPlanOptions,
    config: &crate::config::WikiConfig,
) -> Result<Option<SyncPlanReport>> {
    let Some(mut context) = collect_sync_planning_context(paths, options)? else {
        return Ok(None);
    };
    if options.include_remote_conflicts {
        let mut client = MediaWikiClient::from_config(config)?;
        hydrate_remote_conflicts(&mut context, &mut client)?;
    }
    Ok(Some(build_sync_plan_report(&context)))
}

pub fn collect_changed_article_paths(
    paths: &ResolvedPaths,
    selection: &SyncSelection,
    include_selected_redirects: bool,
) -> Result<Option<Vec<String>>> {
    let Some(context) = collect_sync_planning_context(
        paths,
        &SyncPlanOptions {
            include_templates: false,
            categories_only: false,
            include_deletes: false,
            include_remote_conflicts: false,
            selection: selection.clone(),
        },
    )?
    else {
        return Ok(None);
    };

    let selection_active = !selection.titles.is_empty() || !selection.paths.is_empty();
    let mut out = Vec::new();
    for change in &context.changes {
        let key = normalized_title_key(&change.title);
        let Some(file) = context.local_map.get(&key) else {
            continue;
        };
        if file.namespace != "Main" {
            continue;
        }
        if file.is_redirect && !(include_selected_redirects && selection_active) {
            continue;
        }
        out.push(file.relative_path.clone());
    }
    out.sort();
    out.dedup();
    Ok(Some(out))
}

pub fn diff_local_against_sync(
    paths: &ResolvedPaths,
    options: &DiffOptions,
) -> Result<Option<DiffReport>> {
    let Some(context) = collect_sync_planning_context(
        paths,
        &SyncPlanOptions {
            include_templates: options.include_templates,
            categories_only: options.categories_only,
            include_deletes: true,
            include_remote_conflicts: false,
            selection: options.selection.clone(),
        },
    )?
    else {
        return Ok(None);
    };

    let snapshots = if options.include_content {
        load_sync_snapshot_map(&context.connection)?
    } else {
        BTreeMap::new()
    };

    let changes = context
        .changes
        .iter()
        .map(|change| {
            let (baseline_status, unified_diff) = if options.include_content {
                build_content_diff(paths, &snapshots, change)?
            } else {
                (None, None)
            };
            Ok(DiffChange {
                title: change.title.clone(),
                change_type: change.change_type.clone(),
                relative_path: change.relative_path.clone(),
                local_hash: change.local_hash.clone(),
                synced_hash: change.synced_hash.clone(),
                synced_wiki_timestamp: change.synced_wiki_timestamp.clone(),
                baseline_status,
                unified_diff,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Some(DiffReport {
        new_local: count_changes(&context.changes, DiffChangeType::NewLocal),
        modified_local: count_changes(&context.changes, DiffChangeType::ModifiedLocal),
        deleted_local: count_changes(&context.changes, DiffChangeType::DeletedLocal),
        conflict_count: 0,
        changes,
    }))
}

fn collect_sync_planning_context(
    paths: &ResolvedPaths,
    options: &SyncPlanOptions,
) -> Result<Option<SyncPlanningContext>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }
    let connection = open_sync_connection(paths)?;
    if !table_exists(&connection, "sync_ledger_pages")? {
        return Ok(None);
    }

    let selection = resolve_sync_selection(paths, &options.selection)?;
    let local_files = scan_files(
        paths,
        &ScanOptions {
            include_content: true,
            include_templates: options.include_templates,
            ..ScanOptions::default()
        },
    )?;

    let mut local_map = BTreeMap::new();
    for file in local_files {
        if options.categories_only && namespace_name_to_id(&file.namespace) != Some(NS_CATEGORY) {
            continue;
        }
        if !selection.matches(&file.title, &file.relative_path) {
            continue;
        }
        local_map.insert(normalized_title_key(&file.title), file);
    }

    let ledger = load_sync_ledger_map(&connection, options.include_templates)?
        .into_iter()
        .filter(|(_, entry)| {
            (!options.categories_only || entry.namespace == NS_CATEGORY)
                && selection.matches(&entry.title, &entry.relative_path)
        })
        .collect::<BTreeMap<_, _>>();

    backfill_sync_snapshots_from_local(&connection, paths, &local_map, &ledger)?;

    let mut changes = Vec::new();
    for file in local_map.values() {
        let key = normalized_title_key(&file.title);
        match ledger.get(&key) {
            None => changes.push(PlannedSyncChangeInternal {
                title: file.title.clone(),
                change_type: DiffChangeType::NewLocal,
                relative_path: file.relative_path.clone(),
                local_hash: Some(file.content_hash.clone()),
                synced_hash: None,
                synced_wiki_timestamp: None,
                remote_conflict: false,
                remote_wiki_timestamp: None,
            }),
            Some(entry) if entry.content_hash != file.content_hash => {
                changes.push(PlannedSyncChangeInternal {
                    title: file.title.clone(),
                    change_type: DiffChangeType::ModifiedLocal,
                    relative_path: file.relative_path.clone(),
                    local_hash: Some(file.content_hash.clone()),
                    synced_hash: Some(entry.content_hash.clone()),
                    synced_wiki_timestamp: entry.wiki_modified_at.clone(),
                    remote_conflict: false,
                    remote_wiki_timestamp: None,
                });
            }
            Some(_) => {}
        }
    }

    if options.include_deletes {
        for entry in ledger.values() {
            let key = normalized_title_key(&entry.title);
            if local_map.contains_key(&key) {
                continue;
            }
            changes.push(PlannedSyncChangeInternal {
                title: entry.title.clone(),
                change_type: DiffChangeType::DeletedLocal,
                relative_path: entry.relative_path.clone(),
                local_hash: None,
                synced_hash: Some(entry.content_hash.clone()),
                synced_wiki_timestamp: entry.wiki_modified_at.clone(),
                remote_conflict: false,
                remote_wiki_timestamp: None,
            });
        }
    }

    changes.sort_by(|left, right| {
        change_order(&left.change_type)
            .cmp(&change_order(&right.change_type))
            .then(left.title.cmp(&right.title))
    });

    Ok(Some(SyncPlanningContext {
        connection,
        local_map,
        ledger,
        changes,
        request_count: 0,
    }))
}

fn build_sync_plan_report(context: &SyncPlanningContext) -> SyncPlanReport {
    SyncPlanReport {
        new_local: count_changes(&context.changes, DiffChangeType::NewLocal),
        modified_local: count_changes(&context.changes, DiffChangeType::ModifiedLocal),
        deleted_local: count_changes(&context.changes, DiffChangeType::DeletedLocal),
        conflict_count: context
            .changes
            .iter()
            .filter(|change| change.remote_conflict)
            .count(),
        changes: context
            .changes
            .iter()
            .map(|change| SyncPlanChange {
                title: change.title.clone(),
                change_type: change.change_type.clone(),
                relative_path: change.relative_path.clone(),
                local_hash: change.local_hash.clone(),
                synced_hash: change.synced_hash.clone(),
                synced_wiki_timestamp: change.synced_wiki_timestamp.clone(),
                remote_conflict: change.remote_conflict,
                remote_wiki_timestamp: change.remote_wiki_timestamp.clone(),
            })
            .collect(),
        request_count: context.request_count,
    }
}

fn hydrate_remote_conflicts<A: WikiWriteApi>(
    context: &mut SyncPlanningContext,
    api: &mut A,
) -> Result<()> {
    if context.changes.is_empty() {
        context.request_count = api.request_count();
        return Ok(());
    }

    let titles = context
        .changes
        .iter()
        .map(|change| change.title.clone())
        .collect::<Vec<_>>();
    let remote_timestamps = api
        .get_page_timestamps(&titles)?
        .into_iter()
        .map(|item| (normalized_title_key(&item.title), item))
        .collect::<BTreeMap<_, _>>();

    for change in &mut context.changes {
        change.remote_conflict = push_has_conflict(
            &change.title,
            &change.change_type,
            &context.ledger,
            &remote_timestamps,
        );
        change.remote_wiki_timestamp = remote_timestamps
            .get(&normalized_title_key(&change.title))
            .map(|item| item.timestamp.clone());
    }
    context.request_count = api.request_count();
    Ok(())
}

fn count_changes(changes: &[PlannedSyncChangeInternal], change_type: DiffChangeType) -> usize {
    changes
        .iter()
        .filter(|item| item.change_type == change_type)
        .count()
}

impl ResolvedSyncSelection {
    fn active(&self) -> bool {
        !(self.title_keys.is_empty()
            && self.exact_paths.is_empty()
            && self.path_prefixes.is_empty())
    }

    fn matches(&self, title: &str, relative_path: &str) -> bool {
        if !self.active() {
            return true;
        }
        let normalized_relative = normalize_path(relative_path);
        self.title_keys.contains(&normalized_title_key(title))
            || self.exact_paths.contains(&normalized_relative)
            || self.path_prefixes.iter().any(|prefix| {
                normalized_relative == *prefix
                    || normalized_relative.starts_with(&format!("{prefix}/"))
            })
    }
}

fn resolve_sync_selection(
    paths: &ResolvedPaths,
    selection: &SyncSelection,
) -> Result<ResolvedSyncSelection> {
    let mut resolved = ResolvedSyncSelection::default();
    for title in &selection.titles {
        let normalized = normalize_title_for_storage(title);
        if !normalized.is_empty() {
            resolved.title_keys.insert(normalized.to_ascii_lowercase());
        }
    }
    for path in &selection.paths {
        let Some((relative_path, is_prefix)) = normalize_sync_selection_path(paths, path)? else {
            continue;
        };
        if is_prefix {
            resolved.path_prefixes.push(relative_path);
        } else {
            resolved.exact_paths.insert(relative_path);
        }
    }
    resolved.path_prefixes.sort();
    resolved.path_prefixes.dedup();
    Ok(resolved)
}

fn normalize_sync_selection_path(
    paths: &ResolvedPaths,
    raw: &str,
) -> Result<Option<(String, bool)>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let raw_path = PathBuf::from(trimmed);
    let raw_is_relative = raw_path.is_relative();
    let candidate = if raw_path.is_absolute() {
        raw_path.clone()
    } else {
        paths.project_root.join(&raw_path)
    };
    validate_scoped_path(paths, &candidate)?;

    let normalized_candidate = normalize_path(&candidate);
    let normalized_root = normalize_path(&paths.project_root);
    let relative = normalized_candidate
        .strip_prefix(&format!("{normalized_root}/"))
        .map(ToString::to_string)
        .or_else(|| {
            if raw_is_relative {
                Some(normalize_path(trimmed))
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("selected path is outside the project root: {trimmed}"))?;

    if !relative.starts_with("wiki_content/") && !relative.starts_with("templates/") {
        bail!("selected path must be under wiki_content/ or templates/: {trimmed}");
    }

    let is_prefix = candidate.is_dir() || trimmed.ends_with('/') || trimmed.ends_with('\\');
    let normalized_relative = relative.trim_end_matches('/').to_string();
    Ok(Some((normalized_relative, is_prefix)))
}

fn build_content_diff(
    paths: &ResolvedPaths,
    snapshots: &BTreeMap<String, SyncSnapshotEntry>,
    change: &PlannedSyncChangeInternal,
) -> Result<(Option<DiffBaselineStatus>, Option<String>)> {
    let key = normalized_title_key(&change.title);
    let snapshot = snapshots.get(&key);
    match change.change_type {
        DiffChangeType::NewLocal => {
            let absolute = absolute_path_from_relative(paths, &change.relative_path);
            let local_content = fs::read_to_string(&absolute)
                .with_context(|| format!("failed to read {}", absolute.display()))?;
            Ok((
                Some(DiffBaselineStatus::NotApplicable),
                Some(render_unified_diff(
                    &format!("a/{}", change.relative_path),
                    &format!("b/{}", change.relative_path),
                    "",
                    &local_content,
                )),
            ))
        }
        DiffChangeType::ModifiedLocal => {
            let Some(snapshot) = snapshot else {
                return Ok((Some(DiffBaselineStatus::MissingSnapshot), None));
            };
            let absolute = absolute_path_from_relative(paths, &change.relative_path);
            let local_content = fs::read_to_string(&absolute)
                .with_context(|| format!("failed to read {}", absolute.display()))?;
            Ok((
                Some(DiffBaselineStatus::Available),
                Some(render_unified_diff(
                    &format!("a/{}", snapshot.relative_path),
                    &format!("b/{}", change.relative_path),
                    &snapshot.content_text,
                    &local_content,
                )),
            ))
        }
        DiffChangeType::DeletedLocal => {
            let Some(snapshot) = snapshot else {
                return Ok((Some(DiffBaselineStatus::MissingSnapshot), None));
            };
            Ok((
                Some(DiffBaselineStatus::Available),
                Some(render_unified_diff(
                    &format!("a/{}", snapshot.relative_path),
                    &format!("b/{}", change.relative_path),
                    &snapshot.content_text,
                    "",
                )),
            ))
        }
    }
}

fn render_unified_diff(old_label: &str, new_label: &str, old_text: &str, new_text: &str) -> String {
    TextDiff::from_lines(old_text, new_text)
        .unified_diff()
        .context_radius(3)
        .header(old_label, new_label)
        .to_string()
}

fn push_to_remote_with_api<A: WikiWriteApi>(
    paths: &ResolvedPaths,
    options: &PushOptions,
    api: &mut A,
    credentials: Option<(&str, &str)>,
) -> Result<PushReport> {
    if options.summary.trim().is_empty() {
        bail!("push requires a non-empty summary");
    }

    let Some(mut context) = collect_sync_planning_context(
        paths,
        &SyncPlanOptions {
            include_templates: options.include_templates,
            categories_only: options.categories_only,
            include_deletes: options.delete,
            include_remote_conflicts: true,
            selection: options.selection.clone(),
        },
    )?
    else {
        return Ok(PushReport {
            success: true,
            dry_run: options.dry_run,
            pushed: 0,
            created: 0,
            updated: 0,
            deleted: 0,
            unchanged: 0,
            conflicts: Vec::new(),
            errors: Vec::new(),
            pages: Vec::new(),
            request_count: 0,
        });
    };

    hydrate_remote_conflicts(&mut context, api)?;

    let mut report = PushReport {
        success: true,
        dry_run: options.dry_run,
        pushed: 0,
        created: 0,
        updated: 0,
        deleted: 0,
        unchanged: 0,
        conflicts: Vec::new(),
        errors: Vec::new(),
        pages: Vec::new(),
        request_count: context.request_count,
    };

    if context.changes.is_empty() {
        return Ok(report);
    }

    if options.dry_run {
        for change in &context.changes {
            if change.remote_conflict && !options.force {
                report.conflicts.push(change.title.clone());
                report.pages.push(PushPageResult {
                    title: change.title.clone(),
                    action: "conflict".to_string(),
                    detail: Some("remote page changed since last sync".to_string()),
                });
                continue;
            }

            report.pages.push(PushPageResult {
                title: change.title.clone(),
                action: push_dry_run_action(&change.change_type).to_string(),
                detail: None,
            });
        }
        report.success = report.errors.is_empty() && report.conflicts.is_empty();
        return Ok(report);
    }

    let (username, password) = credentials
        .ok_or_else(|| anyhow::anyhow!("push credentials are required for write mode"))?;
    api.login(username, password)?;

    for change in &context.changes {
        if change.remote_conflict && !options.force {
            report.conflicts.push(change.title.clone());
            report.pages.push(PushPageResult {
                title: change.title.clone(),
                action: "conflict".to_string(),
                detail: Some("remote page changed since last sync".to_string()),
            });
            continue;
        }

        let key = normalized_title_key(&change.title);
        match change.change_type {
            DiffChangeType::NewLocal | DiffChangeType::ModifiedLocal => {
                let file = match context.local_map.get(&key) {
                    Some(file) => file,
                    None => {
                        report
                            .errors
                            .push(format!("{}: local file missing", change.title));
                        report.pages.push(PushPageResult {
                            title: change.title.clone(),
                            action: "error".to_string(),
                            detail: Some("local file missing".to_string()),
                        });
                        continue;
                    }
                };
                let absolute = absolute_path_from_relative(paths, &file.relative_path);
                let content = match fs::read_to_string(&absolute) {
                    Ok(content) => content,
                    Err(error) => {
                        report.errors.push(format!("{}: {error}", change.title));
                        report.pages.push(PushPageResult {
                            title: change.title.clone(),
                            action: "error".to_string(),
                            detail: Some("failed to read local content".to_string()),
                        });
                        continue;
                    }
                };

                match api.edit_page(&file.title, &content, &options.summary) {
                    Ok(remote_page) => {
                        let (is_redirect, redirect_target) = parse_redirect(&remote_page.content);
                        let content_hash = compute_hash(&remote_page.content);
                        if let Err(error) = upsert_sync_ledger(
                            &context.connection,
                            &remote_page,
                            &file.relative_path,
                            &content_hash,
                            is_redirect,
                            redirect_target.as_deref(),
                        ) {
                            report.errors.push(format!("{}: {error}", file.title));
                            report.pages.push(PushPageResult {
                                title: file.title.clone(),
                                action: "error".to_string(),
                                detail: Some("failed to update sync ledger".to_string()),
                            });
                            continue;
                        }
                        if let Err(error) = upsert_sync_snapshot(
                            &context.connection,
                            &remote_page.title,
                            &file.relative_path,
                            &content_hash,
                            &remote_page.content,
                        ) {
                            report.errors.push(format!("{}: {error}", file.title));
                            report.pages.push(PushPageResult {
                                title: file.title.clone(),
                                action: "error".to_string(),
                                detail: Some("failed to update sync snapshot".to_string()),
                            });
                            continue;
                        }

                        report.pushed += 1;
                        match change.change_type {
                            DiffChangeType::NewLocal => {
                                report.created += 1;
                                report.pages.push(PushPageResult {
                                    title: file.title.clone(),
                                    action: "created".to_string(),
                                    detail: None,
                                });
                            }
                            DiffChangeType::ModifiedLocal => {
                                report.updated += 1;
                                report.pages.push(PushPageResult {
                                    title: file.title.clone(),
                                    action: "updated".to_string(),
                                    detail: None,
                                });
                            }
                            DiffChangeType::DeletedLocal => {}
                        }
                    }
                    Err(error) => {
                        report.errors.push(format!("{}: {error}", file.title));
                        report.pages.push(PushPageResult {
                            title: file.title.clone(),
                            action: "error".to_string(),
                            detail: Some("edit failed".to_string()),
                        });
                    }
                }
            }
            DiffChangeType::DeletedLocal => match api.delete_page(
                &change.title,
                &format!("wikitool push delete: {}", options.summary),
            ) {
                Ok(()) => {
                    if let Err(error) = remove_sync_ledger_entry(&context.connection, &change.title)
                    {
                        report.errors.push(format!("{}: {error}", change.title));
                        report.pages.push(PushPageResult {
                            title: change.title.clone(),
                            action: "error".to_string(),
                            detail: Some("failed to update sync ledger".to_string()),
                        });
                        continue;
                    }
                    if let Err(error) = remove_sync_snapshot(&context.connection, &change.title) {
                        report.errors.push(format!("{}: {error}", change.title));
                        report.pages.push(PushPageResult {
                            title: change.title.clone(),
                            action: "error".to_string(),
                            detail: Some("failed to update sync snapshot".to_string()),
                        });
                        continue;
                    }
                    report.pushed += 1;
                    report.deleted += 1;
                    report.pages.push(PushPageResult {
                        title: change.title.clone(),
                        action: "deleted".to_string(),
                        detail: None,
                    });
                }
                Err(error) => {
                    report.errors.push(format!("{}: {error}", change.title));
                    report.pages.push(PushPageResult {
                        title: change.title.clone(),
                        action: "error".to_string(),
                        detail: Some("delete failed".to_string()),
                    });
                }
            },
        }
    }

    report.request_count = api.request_count();
    report.success = report.errors.is_empty() && report.conflicts.is_empty();
    Ok(report)
}

fn pull_from_remote_with_api<A: WikiReadApi>(
    paths: &ResolvedPaths,
    options: &PullOptions,
    api: &mut A,
) -> Result<PullReport> {
    let connection = open_sync_connection(paths)?;
    initialize_sync_schema(&connection)?;

    let mut report = PullReport {
        success: true,
        requested_pages: 0,
        pulled: 0,
        created: 0,
        updated: 0,
        skipped: 0,
        errors: Vec::new(),
        pages: Vec::new(),
        request_count: 0,
        reindex: None,
    };

    let pages_to_pull = resolve_pages_to_pull(&connection, options, api)?;
    report.requested_pages = pages_to_pull.len();
    if pages_to_pull.is_empty() {
        report.request_count = api.request_count();
        return Ok(report);
    }

    let content_rows = api.get_page_contents(&pages_to_pull)?;
    let mut content_by_title = BTreeMap::new();
    for page in content_rows {
        content_by_title.insert(normalized_title_key(&page.title), page);
    }
    let mut ledger_by_title = load_sync_ledger_map(&connection, true)?;

    let mut files_changed = false;
    let mut max_timestamp: Option<String> = None;
    let namespace_mapper = NamespaceMapper::load(paths)?;

    for title in &pages_to_pull {
        let key = normalized_title_key(title);
        let page = match content_by_title.get(&key) {
            Some(page) => page,
            None => {
                let message = format!("{title}: page content missing in API response");
                report.errors.push(message);
                report.pages.push(PullPageResult {
                    title: title.clone(),
                    action: "error".to_string(),
                    detail: Some("missing content".to_string()),
                });
                continue;
            }
        };

        let (is_redirect, redirect_target) = parse_redirect(&page.content);
        let relative_path =
            namespace_mapper.title_to_relative_path(paths, &page.title, is_redirect);
        let absolute_path = absolute_path_from_relative(paths, &relative_path);
        validate_scoped_path(paths, &absolute_path)?;
        ensure_parent_dir(&absolute_path)?;

        let remote_hash = compute_hash(&page.content);
        let ledger_entry = ledger_by_title.get(&key).cloned();
        let stale_synced_path = stale_synced_path_for_removal(
            paths,
            &ledger_entry,
            &relative_path,
            options.overwrite_local,
        )?;

        let local_content = fs::read_to_string(&absolute_path).ok();
        let local_hash = local_content.as_deref().map(compute_hash);

        let local_modified = match (&local_hash, &ledger_entry) {
            (Some(local_hash), Some(entry)) => local_hash != &entry.content_hash,
            (Some(_), None) => true,
            (None, _) => false,
        };

        if let Some(local_hash) = &local_hash
            && local_hash == &remote_hash
        {
            if remove_stale_synced_path(stale_synced_path.as_deref())? {
                files_changed = true;
            }
            upsert_sync_ledger(
                &connection,
                page,
                &relative_path,
                &remote_hash,
                is_redirect,
                redirect_target.as_deref(),
            )?;
            upsert_sync_snapshot(
                &connection,
                &page.title,
                &relative_path,
                &remote_hash,
                &page.content,
            )?;
            ledger_by_title.insert(
                key.clone(),
                SyncLedgerEntry {
                    title: page.title.clone(),
                    namespace: page.namespace,
                    relative_path: relative_path.clone(),
                    content_hash: remote_hash,
                    wiki_modified_at: Some(page.timestamp.clone()),
                },
            );
            note_pull_checkpoint(&mut max_timestamp, &page.timestamp);
            report.skipped += 1;
            report.pulled += 1;
            report.pages.push(PullPageResult {
                title: page.title.clone(),
                action: "skipped".to_string(),
                detail: Some("unchanged".to_string()),
            });
            continue;
        }

        if local_modified && !options.overwrite_local {
            report.skipped += 1;
            report.pages.push(PullPageResult {
                title: page.title.clone(),
                action: "skipped".to_string(),
                detail: Some("local content differs (use --overwrite-local)".to_string()),
            });
            continue;
        }

        let existed_before = absolute_path.exists();
        fs::write(&absolute_path, &page.content)
            .with_context(|| format!("failed to write {}", absolute_path.display()))?;
        files_changed = true;
        remove_stale_synced_path(stale_synced_path.as_deref())?;
        upsert_sync_ledger(
            &connection,
            page,
            &relative_path,
            &remote_hash,
            is_redirect,
            redirect_target.as_deref(),
        )?;
        upsert_sync_snapshot(
            &connection,
            &page.title,
            &relative_path,
            &remote_hash,
            &page.content,
        )?;
        ledger_by_title.insert(
            key.clone(),
            SyncLedgerEntry {
                title: page.title.clone(),
                namespace: page.namespace,
                relative_path: relative_path.clone(),
                content_hash: remote_hash,
                wiki_modified_at: Some(page.timestamp.clone()),
            },
        );
        note_pull_checkpoint(&mut max_timestamp, &page.timestamp);

        report.pulled += 1;
        if existed_before {
            report.updated += 1;
            report.pages.push(PullPageResult {
                title: page.title.clone(),
                action: "updated".to_string(),
                detail: None,
            });
        } else {
            report.created += 1;
            report.pages.push(PullPageResult {
                title: page.title.clone(),
                action: "created".to_string(),
                detail: None,
            });
        }
    }

    if let Some(config_key) = pull_config_key(options)
        && let Some(timestamp) = max_timestamp
    {
        set_sync_config(&connection, &config_key, &timestamp)?;
    }

    if files_changed {
        report.reindex = Some(rebuild_index(paths, &ScanOptions::default())?);
    }

    report.request_count = api.request_count();
    report.success = report.errors.is_empty();
    Ok(report)
}

fn resolve_pages_to_pull<A: WikiReadApi>(
    connection: &Connection,
    options: &PullOptions,
    api: &mut A,
) -> Result<Vec<String>> {
    let mut titles = BTreeSet::new();

    if let Some(category) = &options.category {
        for title in api.get_category_members(category)? {
            let normalized = normalize_title_for_storage(&title);
            if !normalized.is_empty() {
                titles.insert(normalized);
            }
        }
        return Ok(titles.into_iter().collect());
    }

    if options.namespaces.is_empty() {
        bail!("pull requires at least one namespace");
    }

    if !options.full
        && let Some(config_key) = pull_config_key(options)
        && let Some(last_pull) = get_sync_config(connection, &config_key)?
    {
        for title in api.get_recent_changes(&last_pull, &options.namespaces)? {
            let normalized = normalize_title_for_storage(&title);
            if !normalized.is_empty() {
                titles.insert(normalized);
            }
        }
        return Ok(titles.into_iter().collect());
    }

    for namespace in &options.namespaces {
        for title in api.get_all_pages(*namespace)? {
            let normalized = normalize_title_for_storage(&title);
            if !normalized.is_empty() {
                titles.insert(normalized);
            }
        }
    }

    Ok(titles.into_iter().collect())
}

fn note_pull_checkpoint(max_timestamp: &mut Option<String>, timestamp: &str) {
    if max_timestamp
        .as_ref()
        .is_none_or(|current| timestamp > current.as_str())
    {
        *max_timestamp = Some(timestamp.to_string());
    }
}

fn stale_synced_path_for_removal(
    paths: &ResolvedPaths,
    existing: &Option<SyncLedgerEntry>,
    target_relative_path: &str,
    overwrite_local: bool,
) -> Result<Option<PathBuf>> {
    let Some(existing) = existing else {
        return Ok(None);
    };
    if existing.relative_path == target_relative_path {
        return Ok(None);
    }

    let old_absolute = absolute_path_from_relative(paths, &existing.relative_path);
    if !old_absolute.exists() {
        return Ok(None);
    }
    validate_scoped_path(paths, &old_absolute)?;

    let old_content = fs::read_to_string(&old_absolute).with_context(|| {
        format!(
            "failed to read previous synced file {}",
            old_absolute.display()
        )
    })?;
    let old_hash = compute_hash(&old_content);
    let old_modified = old_hash != existing.content_hash;
    if old_modified && !overwrite_local {
        bail!(
            "cannot update path for {} because previous synced path has local modifications: {} (use --overwrite-local)",
            existing.title,
            normalize_path(&old_absolute)
        );
    }

    Ok(Some(old_absolute))
}

fn remove_stale_synced_path(stale_path: Option<&Path>) -> Result<bool> {
    let Some(stale_path) = stale_path else {
        return Ok(false);
    };

    fs::remove_file(stale_path).with_context(|| {
        format!(
            "failed to remove stale synced file {}",
            stale_path.display()
        )
    })?;
    Ok(true)
}

fn pull_config_key(options: &PullOptions) -> Option<String> {
    if options.category.is_some() {
        return None;
    }
    let mut namespaces = options.namespaces.clone();
    namespaces.sort_unstable();
    namespaces.dedup();
    if namespaces.is_empty() {
        return None;
    }
    Some(format!(
        "last_pull_ns_{}",
        namespaces
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("_")
    ))
}

fn push_has_conflict(
    title: &str,
    change_type: &DiffChangeType,
    ledger: &BTreeMap<String, SyncLedgerEntry>,
    remote_timestamps: &BTreeMap<String, PageTimestampInfo>,
) -> bool {
    let key = normalized_title_key(title);
    let remote = remote_timestamps.get(&key);
    match change_type {
        DiffChangeType::NewLocal => remote.is_some(),
        DiffChangeType::ModifiedLocal | DiffChangeType::DeletedLocal => {
            let Some(remote) = remote else {
                return false;
            };
            let Some(stored) = ledger
                .get(&key)
                .and_then(|entry| entry.wiki_modified_at.as_deref())
            else {
                return false;
            };
            !timestamps_match_with_tolerance(stored, &remote.timestamp, 30)
        }
    }
}

fn remove_sync_ledger_entry(connection: &Connection, title: &str) -> Result<()> {
    initialize_sync_schema(connection)?;
    connection
        .execute(
            "DELETE FROM sync_ledger_pages WHERE lower(title) = lower(?1)",
            [title],
        )
        .with_context(|| format!("failed to delete sync ledger row for {title}"))?;
    Ok(())
}

fn push_dry_run_action(change_type: &DiffChangeType) -> &'static str {
    match change_type {
        DiffChangeType::NewLocal => "would_create",
        DiffChangeType::ModifiedLocal => "would_update",
        DiffChangeType::DeletedLocal => "would_delete",
    }
}

fn load_sync_snapshot_map(connection: &Connection) -> Result<BTreeMap<String, SyncSnapshotEntry>> {
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

fn backfill_sync_snapshots_from_local(
    connection: &Connection,
    paths: &ResolvedPaths,
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
        upsert_sync_snapshot(
            connection,
            &file.title,
            &file.relative_path,
            &file.content_hash,
            &content,
        )?;
    }
    Ok(())
}

fn upsert_sync_snapshot(
    connection: &Connection,
    title: &str,
    relative_path: &str,
    content_hash: &str,
    content_text: &str,
) -> Result<()> {
    initialize_sync_schema(connection)?;
    let now = unix_timestamp()?;
    connection
        .execute(
            "INSERT INTO sync_snapshots (
                title, relative_path, content_hash, content_text, synced_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(title) DO UPDATE SET
                relative_path = excluded.relative_path,
                content_hash = excluded.content_hash,
                content_text = excluded.content_text,
                synced_at_unix = excluded.synced_at_unix",
            params![
                title,
                relative_path,
                content_hash,
                content_text,
                i64::try_from(now).context("timestamp does not fit into i64")?
            ],
        )
        .with_context(|| format!("failed to upsert sync snapshot for {title}"))?;
    Ok(())
}

fn remove_sync_snapshot(connection: &Connection, title: &str) -> Result<()> {
    initialize_sync_schema(connection)?;
    connection
        .execute(
            "DELETE FROM sync_snapshots WHERE lower(title) = lower(?1)",
            [title],
        )
        .with_context(|| format!("failed to delete sync snapshot for {title}"))?;
    Ok(())
}

fn change_order(change_type: &DiffChangeType) -> u8 {
    match change_type {
        DiffChangeType::NewLocal => 0,
        DiffChangeType::ModifiedLocal => 1,
        DiffChangeType::DeletedLocal => 2,
    }
}

fn load_sync_ledger_map(
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

fn upsert_sync_ledger(
    connection: &Connection,
    page: &RemotePage,
    relative_path: &str,
    content_hash: &str,
    is_redirect: bool,
    redirect_target: Option<&str>,
) -> Result<()> {
    initialize_sync_schema(connection)?;
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

fn get_sync_config(connection: &Connection, key: &str) -> Result<Option<String>> {
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

fn set_sync_config(connection: &Connection, key: &str, value: &str) -> Result<()> {
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

fn open_sync_connection(paths: &ResolvedPaths) -> Result<Connection> {
    open_initialized_database_connection(&paths.db_path)
}

fn initialize_sync_schema(connection: &Connection) -> Result<()> {
    ensure_database_schema_connection(connection)
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))
}

fn absolute_path_from_relative(paths: &ResolvedPaths, relative: &str) -> PathBuf {
    let mut output = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            output.push(segment);
        }
    }
    output
}

fn normalized_title_key(title: &str) -> String {
    normalize_title_for_storage(title).to_ascii_lowercase()
}

fn normalize_title_for_storage(title: &str) -> String {
    title.replace('_', " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        DiffBaselineStatus, DiffChangeType, DiffOptions, ExternalSearchHit, NS_MAIN,
        PageTimestampInfo, PullOptions, PushOptions, RemotePage, SiteInfoNamespace,
        SyncPlanOptions, SyncSelection, WikiReadApi, WikiWriteApi, collect_changed_article_paths,
        diff_local_against_sync, namespace_display_name, plan_sync_changes,
        pull_from_remote_with_api, push_to_remote_with_api, should_include_discovered_namespace,
    };
    use crate::runtime::{ResolvedPaths, ValueSource};

    #[derive(Default)]
    struct MockApi {
        all_pages_by_namespace: BTreeMap<i32, Vec<String>>,
        recent_changes: Vec<String>,
        category_members: Vec<String>,
        page_contents: BTreeMap<String, RemotePage>,
        page_timestamps: BTreeMap<String, PageTimestampInfo>,
        search_hits: Vec<ExternalSearchHit>,
        edited_pages: Vec<String>,
        deleted_pages: Vec<String>,
        login_required: bool,
        logged_in: bool,
        request_count: usize,
    }

    impl WikiReadApi for MockApi {
        fn get_all_pages(&mut self, namespace: i32) -> anyhow::Result<Vec<String>> {
            self.request_count += 1;
            Ok(self
                .all_pages_by_namespace
                .get(&namespace)
                .cloned()
                .unwrap_or_default())
        }

        fn get_category_members(&mut self, _category: &str) -> anyhow::Result<Vec<String>> {
            self.request_count += 1;
            Ok(self.category_members.clone())
        }

        fn get_recent_changes(
            &mut self,
            _since: &str,
            _namespaces: &[i32],
        ) -> anyhow::Result<Vec<String>> {
            self.request_count += 1;
            Ok(self.recent_changes.clone())
        }

        fn get_page_contents(&mut self, titles: &[String]) -> anyhow::Result<Vec<RemotePage>> {
            self.request_count += 1;
            let mut output = Vec::new();
            for title in titles {
                if let Some(page) = self.page_contents.get(title) {
                    output.push(page.clone());
                }
            }
            Ok(output)
        }

        fn search(
            &mut self,
            _query: &str,
            _namespaces: &[i32],
            _limit: usize,
        ) -> anyhow::Result<Vec<ExternalSearchHit>> {
            self.request_count += 1;
            Ok(self.search_hits.clone())
        }

        fn request_count(&self) -> usize {
            self.request_count
        }
    }

    impl WikiWriteApi for MockApi {
        fn login(&mut self, _username: &str, _password: &str) -> anyhow::Result<()> {
            self.request_count += 1;
            self.logged_in = true;
            Ok(())
        }

        fn get_page_timestamps(
            &mut self,
            titles: &[String],
        ) -> anyhow::Result<Vec<PageTimestampInfo>> {
            self.request_count += 1;
            let mut output = Vec::new();
            for title in titles {
                if let Some(item) = self.page_timestamps.get(title) {
                    output.push(item.clone());
                }
            }
            Ok(output)
        }

        fn edit_page(
            &mut self,
            title: &str,
            content: &str,
            _summary: &str,
        ) -> anyhow::Result<RemotePage> {
            self.request_count += 1;
            if self.login_required && !self.logged_in {
                anyhow::bail!("not logged in");
            }
            self.edited_pages.push(title.to_string());
            let page = RemotePage {
                title: title.to_string(),
                namespace: NS_MAIN,
                page_id: 9000,
                revision_id: 9001,
                timestamp: "2026-02-20T00:00:00Z".to_string(),
                content: content.to_string(),
            };
            self.page_contents.insert(title.to_string(), page.clone());
            self.page_timestamps.insert(
                title.to_string(),
                PageTimestampInfo {
                    title: title.to_string(),
                    timestamp: page.timestamp.clone(),
                    revision_id: page.revision_id,
                },
            );
            Ok(page)
        }

        fn delete_page(&mut self, title: &str, _reason: &str) -> anyhow::Result<()> {
            self.request_count += 1;
            if self.login_required && !self.logged_in {
                anyhow::bail!("not logged in");
            }
            self.deleted_pages.push(title.to_string());
            self.page_timestamps.remove(title);
            self.page_contents.remove(title);
            Ok(())
        }
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(path, content).expect("write file");
    }

    fn paths(project_root: &Path) -> ResolvedPaths {
        ResolvedPaths {
            project_root: project_root.to_path_buf(),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool").join("data"),
            db_path: project_root
                .join(".wikitool")
                .join("data")
                .join("wikitool.db"),
            config_path: project_root.join(".wikitool").join("config.toml"),
            parser_config_path: project_root
                .join(".wikitool")
                .join(crate::runtime::PARSER_CONFIG_FILENAME),
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    fn base_page(title: &str, content: &str) -> RemotePage {
        RemotePage {
            title: title.to_string(),
            namespace: NS_MAIN,
            page_id: 100,
            revision_id: 200,
            timestamp: "2026-02-19T00:00:00Z".to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn namespace_discovery_filters_builtin_and_talk_namespaces() {
        let builtin = SiteInfoNamespace {
            id: 14,
            canonical: Some("Category".to_string()),
            name: Some("Category".to_string()),
            star_name: Some("Category".to_string()),
            content: Some(json!(true)),
        };
        let talk = SiteInfoNamespace {
            id: 3001,
            canonical: Some("Lore talk".to_string()),
            name: Some("Lore talk".to_string()),
            star_name: Some("Lore talk".to_string()),
            content: Some(json!(false)),
        };
        assert!(!should_include_discovered_namespace(&builtin));
        assert!(!should_include_discovered_namespace(&talk));
    }

    #[test]
    fn namespace_discovery_includes_custom_content_namespace() {
        let custom = SiteInfoNamespace {
            id: 3000,
            canonical: Some("Lore".to_string()),
            name: Some("Lore".to_string()),
            star_name: Some("Lore".to_string()),
            content: Some(json!(true)),
        };
        assert!(should_include_discovered_namespace(&custom));
    }

    #[test]
    fn namespace_display_name_prefers_canonical_and_normalizes_underscores() {
        let namespace = SiteInfoNamespace {
            id: 3000,
            canonical: Some("Lore_Namespace".to_string()),
            name: Some("Ignored".to_string()),
            star_name: Some("Ignored".to_string()),
            content: Some(json!(true)),
        };
        assert_eq!(
            namespace_display_name(&namespace).as_deref(),
            Some("Lore Namespace")
        );
    }

    #[test]
    fn pull_writes_files_and_reindexes() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string(), "Beta".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
        api.page_contents
            .insert("Beta".to_string(), base_page("Beta", "[[Alpha]]"));

        let report = pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("pull");

        assert!(report.success);
        assert_eq!(report.created, 2);
        assert_eq!(report.updated, 0);
        assert_eq!(report.skipped, 0);
        assert!(
            paths
                .wiki_content_dir
                .join("Main")
                .join("Alpha.wiki")
                .exists()
        );
        assert!(
            paths
                .wiki_content_dir
                .join("Main")
                .join("Beta.wiki")
                .exists()
        );
        assert!(report.reindex.is_some());
    }

    #[test]
    fn pull_skips_modified_local_when_overwrite_is_disabled() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "local edited",
        );

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "remote version"));

        let report = pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("pull");

        assert_eq!(report.created, 0);
        assert_eq!(report.updated, 0);
        assert_eq!(report.skipped, 1);
        let current = fs::read_to_string(paths.wiki_content_dir.join("Main").join("Alpha.wiki"))
            .expect("read local file");
        assert_eq!(current, "local edited");
    }

    #[test]
    fn incremental_pull_does_not_advance_checkpoint_when_page_is_skipped() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "local edited",
        );

        let connection = super::open_sync_connection(&paths).expect("open sync db");
        super::initialize_sync_schema(&connection).expect("initialize sync schema");
        super::set_sync_config(&connection, "last_pull_ns_0", "2026-02-01T00:00:00Z")
            .expect("seed pull cursor");

        let mut api = MockApi {
            recent_changes: vec!["Alpha".to_string()],
            ..Default::default()
        };
        let mut remote = base_page("Alpha", "remote version");
        remote.timestamp = "2026-02-20T00:00:00Z".to_string();
        api.page_contents.insert("Alpha".to_string(), remote);

        let report = pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: false,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("incremental pull");

        assert_eq!(report.skipped, 1);
        let connection = super::open_sync_connection(&paths).expect("reopen sync db");
        let checkpoint = super::get_sync_config(&connection, "last_pull_ns_0")
            .expect("load pull cursor")
            .expect("pull cursor");
        assert_eq!(checkpoint, "2026-02-01T00:00:00Z");
    }

    #[test]
    fn pull_preserves_old_path_when_redirect_target_has_local_conflict() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
        pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("seed pull");

        let redirect_path = paths
            .wiki_content_dir
            .join("Main")
            .join("_redirects")
            .join("Alpha.wiki");
        write_file(&redirect_path, "conflicting local redirect");

        let mut redirected = base_page("Alpha", "#REDIRECT [[Beta]]");
        redirected.timestamp = "2026-02-20T00:00:00Z".to_string();
        api.page_contents.insert("Alpha".to_string(), redirected);

        let report = pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("pull with redirect conflict");

        assert_eq!(report.skipped, 1);
        assert!(
            paths
                .wiki_content_dir
                .join("Main")
                .join("Alpha.wiki")
                .exists()
        );
        let redirect_content = fs::read_to_string(&redirect_path).expect("read redirect path");
        assert_eq!(redirect_content, "conflicting local redirect");
    }

    #[test]
    fn diff_detects_new_modified_and_deleted_local_pages() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string(), "Beta".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
        api.page_contents
            .insert("Beta".to_string(), base_page("Beta", "beta body"));

        pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("seed pull");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "alpha local edit",
        );
        fs::remove_file(paths.wiki_content_dir.join("Main").join("Beta.wiki"))
            .expect("delete beta");
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "gamma local",
        );

        let diff = diff_local_against_sync(
            &paths,
            &DiffOptions {
                include_templates: false,
                categories_only: false,
                include_content: false,
                selection: SyncSelection::default(),
            },
        )
        .expect("diff")
        .expect("diff report");

        assert_eq!(diff.new_local, 1);
        assert_eq!(diff.modified_local, 1);
        assert_eq!(diff.deleted_local, 1);
        assert!(
            diff.changes
                .iter()
                .any(|item| item.title == "Gamma" && item.change_type == DiffChangeType::NewLocal)
        );
        assert!(
            diff.changes
                .iter()
                .any(|item| item.title == "Alpha"
                    && item.change_type == DiffChangeType::ModifiedLocal)
        );
        assert!(
            diff.changes.iter().any(
                |item| item.title == "Beta" && item.change_type == DiffChangeType::DeletedLocal
            )
        );
    }

    #[test]
    fn push_dry_run_reports_local_changes_without_writes() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

        pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("seed pull");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "alpha local edit",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "gamma local",
        );

        let report = push_to_remote_with_api(
            &paths,
            &PushOptions {
                summary: "test dry run".to_string(),
                dry_run: true,
                force: false,
                delete: false,
                include_templates: false,
                categories_only: false,
                selection: SyncSelection::default(),
            },
            &mut api,
            None,
        )
        .expect("push dry run");

        assert!(report.dry_run);
        assert_eq!(report.created, 0);
        assert_eq!(report.updated, 0);
        assert_eq!(api.edited_pages.len(), 0);
        assert!(
            report
                .pages
                .iter()
                .any(|item| item.title == "Alpha" && item.action == "would_update")
        );
        assert!(
            report
                .pages
                .iter()
                .any(|item| item.title == "Gamma" && item.action == "would_create")
        );
    }

    #[test]
    fn push_detects_remote_conflict_without_force() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi {
            login_required: true,
            ..Default::default()
        };
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

        pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("seed pull");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "alpha local edit",
        );
        api.page_timestamps.insert(
            "Alpha".to_string(),
            PageTimestampInfo {
                title: "Alpha".to_string(),
                timestamp: "2026-02-22T00:00:00Z".to_string(),
                revision_id: 9999,
            },
        );

        let report = push_to_remote_with_api(
            &paths,
            &PushOptions {
                summary: "test conflict".to_string(),
                dry_run: false,
                force: false,
                delete: false,
                include_templates: false,
                categories_only: false,
                selection: SyncSelection::default(),
            },
            &mut api,
            Some(("bot", "pass")),
        )
        .expect("push");

        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(report.conflicts[0], "Alpha");
        assert!(api.edited_pages.is_empty());
    }

    #[test]
    fn push_dry_run_detects_remote_conflict_without_writes() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));

        pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("seed pull");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "alpha local edit",
        );
        api.page_timestamps.insert(
            "Alpha".to_string(),
            PageTimestampInfo {
                title: "Alpha".to_string(),
                timestamp: "2026-02-22T00:00:00Z".to_string(),
                revision_id: 9999,
            },
        );

        let report = push_to_remote_with_api(
            &paths,
            &PushOptions {
                summary: "test dry-run conflict".to_string(),
                dry_run: true,
                force: false,
                delete: false,
                include_templates: false,
                categories_only: false,
                selection: SyncSelection::default(),
            },
            &mut api,
            None,
        )
        .expect("push dry run");

        assert!(report.dry_run);
        assert_eq!(report.conflicts, vec!["Alpha".to_string()]);
        assert!(api.edited_pages.is_empty());
    }

    #[test]
    fn diff_content_uses_snapshots_and_reports_missing_baseline() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string(), "Beta".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
        api.page_contents
            .insert("Beta".to_string(), base_page("Beta", "beta body"));

        pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("seed pull");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "alpha local edit",
        );

        let diff = diff_local_against_sync(
            &paths,
            &DiffOptions {
                include_templates: false,
                categories_only: false,
                include_content: true,
                selection: SyncSelection::default(),
            },
        )
        .expect("diff")
        .expect("diff report");
        let alpha = diff
            .changes
            .iter()
            .find(|change| change.title == "Alpha")
            .expect("alpha diff");
        assert_eq!(alpha.baseline_status, Some(DiffBaselineStatus::Available));
        assert!(
            alpha.unified_diff.as_deref().is_some_and(
                |diff| diff.contains("-alpha body") && diff.contains("+alpha local edit")
            )
        );

        let connection = super::open_sync_connection(&paths).expect("open sync db");
        connection
            .execute("DELETE FROM sync_snapshots WHERE title = 'Alpha'", [])
            .expect("delete snapshot");

        let diff = diff_local_against_sync(
            &paths,
            &DiffOptions {
                include_templates: false,
                categories_only: false,
                include_content: true,
                selection: SyncSelection::default(),
            },
        )
        .expect("diff after snapshot delete")
        .expect("diff report");
        let alpha = diff
            .changes
            .iter()
            .find(|change| change.title == "Alpha")
            .expect("alpha diff");
        assert_eq!(
            alpha.baseline_status,
            Some(DiffBaselineStatus::MissingSnapshot)
        );
        assert!(alpha.unified_diff.is_none());
    }

    #[test]
    fn sync_plan_selection_and_changed_article_paths_honor_scope() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi::default();
        api.all_pages_by_namespace.insert(
            NS_MAIN,
            vec!["Alpha".to_string(), "Beta".to_string(), "Gamma".to_string()],
        );
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
        api.page_contents
            .insert("Beta".to_string(), base_page("Beta", "beta body"));
        api.page_contents
            .insert("Gamma".to_string(), base_page("Gamma", "gamma body"));

        pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("seed pull");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "alpha local edit",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "#REDIRECT [[Alpha]]",
        );

        let selected = plan_sync_changes(
            &paths,
            &SyncPlanOptions {
                include_templates: false,
                categories_only: false,
                include_deletes: true,
                include_remote_conflicts: false,
                selection: SyncSelection {
                    titles: vec!["Alpha".to_string()],
                    paths: Vec::new(),
                },
            },
        )
        .expect("plan selection")
        .expect("plan report");
        assert_eq!(selected.changes.len(), 1);
        assert_eq!(selected.changes[0].title, "Alpha");

        let changed_paths = collect_changed_article_paths(&paths, &SyncSelection::default(), false)
            .expect("collect changed paths")
            .expect("changed paths");
        assert_eq!(
            changed_paths,
            vec!["wiki_content/Main/Alpha.wiki".to_string()]
        );

        let selected_redirect_paths = collect_changed_article_paths(
            &paths,
            &SyncSelection {
                titles: vec!["Beta".to_string()],
                paths: Vec::new(),
            },
            true,
        )
        .expect("collect changed paths with redirect")
        .expect("changed paths");
        assert_eq!(
            selected_redirect_paths,
            vec!["wiki_content/Main/Beta.wiki".to_string()]
        );
    }
}
