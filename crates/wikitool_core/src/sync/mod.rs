use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use similar::TextDiff;

use crate::filesystem::{
    NamespaceMapper, ScanOptions, ScannedFile, scan_files, validate_scoped_path,
};
use crate::knowledge::content_index::rebuild_index;
pub use crate::mw::{
    ExternalSearchHit, ExternalSearchReport, MediaWikiClient, MediaWikiClientConfig,
    MediaWikiSearchOptions, MediaWikiSearchWhat, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE,
    NS_TEMPLATE, PageTimestampInfo, RemotePage, WikiReadApi, WikiWriteApi, search_pages_report,
};
use crate::runtime::ResolvedPaths;
use crate::schema::{ensure_database_schema_connection, open_initialized_database_connection};
use crate::support::{
    compute_wiki_sync_hash, normalize_path, parse_redirect, table_exists, unix_timestamp,
};

mod diff;
mod model;
mod namespaces;
mod planning;
mod pull;
mod push;
mod remote;
mod storage;
mod timestamps;

pub use diff::diff_local_against_sync;
pub use model::*;
pub use planning::{
    collect_changed_article_paths, plan_sync_changes, plan_sync_changes_with_config,
};
pub use pull::{pull_from_remote, pull_from_remote_with_config};
pub use push::{push_to_remote, push_to_remote_with_config};
pub use remote::{
    delete_remote_page, delete_remote_page_with_config, discover_custom_namespaces,
    search_external_wiki, search_external_wiki_report, search_external_wiki_report_with_config,
    search_external_wiki_with_config,
};

use model::{
    PlannedSyncChangeInternal, ResolvedSyncSelection, SyncLedgerEntry, SyncPlanningContext,
    SyncSnapshotEntry,
};
use namespaces::{is_template_namespace_id, namespace_name_to_id};
use planning::{collect_sync_planning_context, count_changes, hydrate_remote_conflicts};
use storage::{
    absolute_path_from_relative, backfill_sync_snapshots_from_local, ensure_parent_dir,
    get_sync_config, initialize_sync_schema, load_sync_ledger_map, load_sync_snapshot_map,
    normalize_title_for_storage, normalized_title_key, open_sync_connection,
    remove_sync_ledger_entry, remove_sync_snapshot, set_sync_config, upsert_sync_ledger,
    upsert_sync_snapshot,
};
use timestamps::timestamps_match_with_tolerance;

#[cfg(test)]
pub(crate) use namespaces::{
    SiteInfoNamespace, namespace_display_name, should_include_discovered_namespace,
};
#[cfg(test)]
use pull::pull_from_remote_with_api;
#[cfg(test)]
use push::push_to_remote_with_api;

#[cfg(test)]
mod tests;
