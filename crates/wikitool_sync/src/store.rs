use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use url::Url;

use crate::SyncProjectPaths;

const SYNC_STORE_SCHEMA_VERSION: i32 = 7;
const MUTATION_CLOSURE_SCHEMA: &str = r#"
CREATE TABLE sync_mutation_closures (
    closure_id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation TEXT NOT NULL CHECK (operation IN ('edit', 'delete')),
    mutation_id INTEGER NOT NULL,
    target_api_url TEXT NOT NULL,
    title TEXT NOT NULL,
    previous_phase TEXT NOT NULL,
    actor TEXT NOT NULL,
    reason TEXT NOT NULL,
    closed_at_unix INTEGER NOT NULL,
    UNIQUE(operation, mutation_id)
);
CREATE TABLE sync_invalidated_titles (
    title_key TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    target_api_url TEXT NOT NULL,
    closure_id INTEGER NOT NULL REFERENCES sync_mutation_closures(closure_id),
    invalidated_at_unix INTEGER NOT NULL
);
"#;
const EDIT_MUTATION_SCHEMA: &str = r#"
CREATE TABLE sync_edit_mutations (
    mutation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_api_url TEXT NOT NULL,
    title TEXT NOT NULL,
    namespace TEXT NOT NULL,
    namespace_id INTEGER,
    relative_path TEXT NOT NULL,
    intended_content_sha256 TEXT NOT NULL,
    intended_normalized_sha256 TEXT NOT NULL,
    summary TEXT NOT NULL,
    summary_marker TEXT UNIQUE,
    constraint_kind TEXT NOT NULL CHECK (constraint_kind IN ('create_only', 'existing_revision')),
    expected_revision_id INTEGER,
    phase TEXT NOT NULL CHECK (phase IN (
        'intent_persisted',
        'response_bound',
        'outcome_ambiguous',
        'reconciliation_required',
        'resolved',
        'state_advanced'
    )),
    response_title TEXT,
    response_page_id INTEGER,
    response_old_revision_id INTEGER,
    response_new_revision_id INTEGER,
    response_new_timestamp TEXT,
    request_started_at_unix INTEGER,
    terminal_outcome TEXT,
    detail TEXT,
    created_at_unix INTEGER NOT NULL,
    updated_at_unix INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_sync_edit_mutations_one_unresolved_title
    ON sync_edit_mutations(title)
    WHERE phase NOT IN ('state_advanced', 'resolved');
CREATE INDEX idx_sync_edit_mutations_phase ON sync_edit_mutations(phase);
CREATE INDEX idx_sync_edit_mutations_new_revision
    ON sync_edit_mutations(response_new_revision_id);
"#;
const DELETE_MUTATION_SCHEMA: &str = r#"
CREATE TABLE sync_delete_mutations (
    mutation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_api_url TEXT NOT NULL,
    title TEXT NOT NULL,
    relative_path TEXT,
    expected_revision_id INTEGER NOT NULL,
    reason TEXT NOT NULL,
    reason_marker TEXT NOT NULL UNIQUE,
    phase TEXT NOT NULL CHECK (phase IN (
        'intent_persisted',
        'response_bound',
        'outcome_ambiguous',
        'reconciliation_required',
        'resolved',
        'state_advanced'
    )),
    response_kind TEXT CHECK (response_kind IN ('deleted', 'already_missing')),
    response_title TEXT,
    response_log_id INTEGER,
    response_log_timestamp TEXT,
    request_started_at_unix INTEGER,
    terminal_outcome TEXT,
    backup_enabled INTEGER NOT NULL DEFAULT 0,
    backup_directory TEXT,
    backup_path TEXT,
    local_content_sha256 TEXT,
    local_effect_status TEXT NOT NULL DEFAULT 'not_applicable' CHECK (local_effect_status IN (
        'not_applicable',
        'pending',
        'backup_ready',
        'source_staging',
        'source_staged',
        'complete'
    )),
    detail TEXT,
    created_at_unix INTEGER NOT NULL,
    updated_at_unix INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_sync_delete_mutations_one_unresolved_title
    ON sync_delete_mutations(title)
    WHERE phase NOT IN ('state_advanced', 'resolved');
CREATE INDEX idx_sync_delete_mutations_phase ON sync_delete_mutations(phase);
CREATE INDEX idx_sync_delete_mutations_log_id
    ON sync_delete_mutations(response_log_id);
"#;
const SYNC_STORE_SCHEMA: &str = r#"
CREATE TABLE sync_ledger_pages (
    title TEXT PRIMARY KEY,
    namespace INTEGER NOT NULL,
    relative_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    wiki_modified_at TEXT,
    revision_id INTEGER,
    page_id INTEGER,
    is_redirect INTEGER NOT NULL,
    redirect_target TEXT,
    last_synced_at_unix INTEGER NOT NULL
);
CREATE INDEX idx_sync_ledger_pages_namespace ON sync_ledger_pages(namespace);
CREATE INDEX idx_sync_ledger_pages_relative_path ON sync_ledger_pages(relative_path);
CREATE INDEX idx_sync_ledger_pages_lower_title ON sync_ledger_pages(lower(title));

CREATE TABLE sync_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE sync_snapshots (
    title TEXT PRIMARY KEY,
    relative_path TEXT NOT NULL,
    content_text TEXT NOT NULL
);
CREATE INDEX idx_sync_snapshots_relative_path ON sync_snapshots(relative_path);
CREATE INDEX idx_sync_snapshots_lower_title ON sync_snapshots(lower(title));

CREATE TABLE sync_store_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE sync_edit_mutations (
    mutation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_api_url TEXT NOT NULL,
    title TEXT NOT NULL,
    namespace TEXT NOT NULL,
    namespace_id INTEGER,
    relative_path TEXT NOT NULL,
    intended_content_sha256 TEXT NOT NULL,
    intended_normalized_sha256 TEXT NOT NULL,
    summary TEXT NOT NULL,
    summary_marker TEXT UNIQUE,
    constraint_kind TEXT NOT NULL CHECK (constraint_kind IN ('create_only', 'existing_revision')),
    expected_revision_id INTEGER,
    phase TEXT NOT NULL CHECK (phase IN (
        'intent_persisted',
        'response_bound',
        'outcome_ambiguous',
        'reconciliation_required',
        'resolved',
        'state_advanced'
    )),
    response_title TEXT,
    response_page_id INTEGER,
    response_old_revision_id INTEGER,
    response_new_revision_id INTEGER,
    response_new_timestamp TEXT,
    request_started_at_unix INTEGER,
    terminal_outcome TEXT,
    detail TEXT,
    created_at_unix INTEGER NOT NULL,
    updated_at_unix INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_sync_edit_mutations_one_unresolved_title
    ON sync_edit_mutations(title)
    WHERE phase NOT IN ('state_advanced', 'resolved');
CREATE INDEX idx_sync_edit_mutations_phase ON sync_edit_mutations(phase);
CREATE INDEX idx_sync_edit_mutations_new_revision
    ON sync_edit_mutations(response_new_revision_id);

CREATE TABLE sync_delete_mutations (
    mutation_id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_api_url TEXT NOT NULL,
    title TEXT NOT NULL,
    relative_path TEXT,
    expected_revision_id INTEGER NOT NULL,
    reason TEXT NOT NULL,
    reason_marker TEXT NOT NULL UNIQUE,
    phase TEXT NOT NULL CHECK (phase IN (
        'intent_persisted',
        'response_bound',
        'outcome_ambiguous',
        'reconciliation_required',
        'resolved',
        'state_advanced'
    )),
    response_kind TEXT CHECK (response_kind IN ('deleted', 'already_missing')),
    response_title TEXT,
    response_log_id INTEGER,
    response_log_timestamp TEXT,
    request_started_at_unix INTEGER,
    terminal_outcome TEXT,
    backup_enabled INTEGER NOT NULL DEFAULT 0,
    backup_directory TEXT,
    backup_path TEXT,
    local_content_sha256 TEXT,
    local_effect_status TEXT NOT NULL DEFAULT 'not_applicable' CHECK (local_effect_status IN (
        'not_applicable',
        'pending',
        'backup_ready',
        'source_staging',
        'source_staged',
        'complete'
    )),
    detail TEXT,
    created_at_unix INTEGER NOT NULL,
    updated_at_unix INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_sync_delete_mutations_one_unresolved_title
    ON sync_delete_mutations(title)
    WHERE phase NOT IN ('state_advanced', 'resolved');
CREATE INDEX idx_sync_delete_mutations_phase ON sync_delete_mutations(phase);
CREATE INDEX idx_sync_delete_mutations_log_id
    ON sync_delete_mutations(response_log_id);

CREATE TABLE sync_mutation_closures (
    closure_id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation TEXT NOT NULL CHECK (operation IN ('edit', 'delete')),
    mutation_id INTEGER NOT NULL,
    target_api_url TEXT NOT NULL,
    title TEXT NOT NULL,
    previous_phase TEXT NOT NULL,
    actor TEXT NOT NULL,
    reason TEXT NOT NULL,
    closed_at_unix INTEGER NOT NULL,
    UNIQUE(operation, mutation_id)
);
CREATE TABLE sync_invalidated_titles (
    title_key TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    target_api_url TEXT NOT NULL,
    closure_id INTEGER NOT NULL REFERENCES sync_mutation_closures(closure_id),
    invalidated_at_unix INTEGER NOT NULL
);
"#;

const REQUIRED_TABLES: &[&str] = &[
    "sync_ledger_pages",
    "sync_config",
    "sync_snapshots",
    "sync_store_meta",
    "sync_edit_mutations",
    "sync_delete_mutations",
    "sync_mutation_closures",
    "sync_invalidated_titles",
];
const ESTABLISHED_KEY: &str = "global_baseline_established_v1";
const TARGET_API_URL_KEY: &str = "target_api_url_v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncTargetIdentity {
    api_url: String,
}

impl SyncTargetIdentity {
    pub fn from_api_url(api_url: &str) -> Result<Self> {
        let trimmed = api_url.trim();
        if trimmed.is_empty() {
            anyhow::bail!(
                "wiki API URL is not configured; set [wiki].api_url or WIKITOOL_WIKI_API_URL"
            );
        }
        let mut parsed = Url::parse(trimmed)
            .with_context(|| format!("invalid wiki API URL for sync identity: {trimmed}"))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            anyhow::bail!("wiki API URL must be an absolute HTTP(S) URL: {trimmed}");
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            anyhow::bail!("wiki API URL must not contain embedded credentials");
        }
        if parsed.query().is_some() {
            anyhow::bail!("wiki API URL must not contain a query string");
        }
        parsed.set_fragment(None);
        Ok(Self {
            api_url: parsed.to_string(),
        })
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    pub fn ensure_matches_api(&self, api_url: &str) -> Result<()> {
        let observed = Self::from_api_url(api_url)?;
        if self != &observed {
            anyhow::bail!(
                "configured sync target {} does not match API client target {}",
                self.api_url,
                observed.api_url
            );
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum SyncStateError {
    Missing {
        path: PathBuf,
    },
    Unestablished {
        path: PathBuf,
    },
    TargetUnbound {
        path: PathBuf,
        requested_api_url: String,
    },
    TargetMismatch {
        path: PathBuf,
        stored_api_url: String,
        requested_api_url: String,
    },
    Incompatible {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for SyncStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { path } => write!(
                formatter,
                "sync state is missing at {}; run a successful `wikitool pull --full --all` to establish a global revision baseline before diff, status, review, or push",
                path.display()
            ),
            Self::Unestablished { path } => write!(
                formatter,
                "sync state at {} has no authoritative global baseline; run a successful `wikitool pull --full --all` before diff, status, review, or push",
                path.display()
            ),
            Self::TargetUnbound {
                path,
                requested_api_url,
            } => write!(
                formatter,
                "sync state at {} has no durable target identity and cannot authorize operations against {}; preserve or move the store aside, then establish a fresh baseline with `wikitool pull --full --all`",
                path.display(),
                requested_api_url
            ),
            Self::TargetMismatch {
                path,
                stored_api_url,
                requested_api_url,
            } => write!(
                formatter,
                "sync state at {} belongs to {}, not {}; preserve or move the store aside, then establish a fresh baseline for the new target with `wikitool pull --full --all`",
                path.display(),
                stored_api_url,
                requested_api_url
            ),
            Self::Incompatible { path, reason } => write!(
                formatter,
                "sync state at {} is incompatible: {reason}; preserve the file and repair or repull deliberately",
                path.display()
            ),
        }
    }
}

impl Error for SyncStateError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStoreMigrationStatus {
    AlreadyCurrent,
    MigratedLegacy,
    NoLegacyState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStoreMigrationReport {
    pub status: SyncStoreMigrationStatus,
    pub sync_store_path: PathBuf,
    pub legacy_path: PathBuf,
    pub established: bool,
    pub ledger_rows: usize,
    pub snapshot_rows: usize,
    pub config_rows: usize,
}

pub fn preserve_legacy_sync_state(
    paths: &SyncProjectPaths,
    legacy_path: Option<&Path>,
) -> Result<SyncStoreMigrationReport> {
    let sync_store_path = paths.sync_store_path();
    let legacy_path_display = legacy_path.unwrap_or(Path::new(""));
    if sync_store_path.exists() {
        let connection = open_existing_store(&sync_store_path)?;
        let (ledger_rows, snapshot_rows, config_rows) = store_counts(&connection)?;
        return Ok(SyncStoreMigrationReport {
            status: SyncStoreMigrationStatus::AlreadyCurrent,
            sync_store_path,
            legacy_path: legacy_path_display.to_path_buf(),
            established: global_baseline_is_established(paths, &connection)?,
            ledger_rows,
            snapshot_rows,
            config_rows,
        });
    }

    let Some(legacy_path) = legacy_path else {
        return Ok(SyncStoreMigrationReport {
            status: SyncStoreMigrationStatus::NoLegacyState,
            sync_store_path,
            legacy_path: PathBuf::new(),
            established: false,
            ledger_rows: 0,
            snapshot_rows: 0,
            config_rows: 0,
        });
    };
    let Some(_) = legacy_sync_counts(legacy_path)? else {
        return Ok(SyncStoreMigrationReport {
            status: SyncStoreMigrationStatus::NoLegacyState,
            sync_store_path,
            legacy_path: legacy_path.to_path_buf(),
            established: false,
            ledger_rows: 0,
            snapshot_rows: 0,
            config_rows: 0,
        });
    };

    let copied_counts = migrate_legacy_store(legacy_path, &sync_store_path)?;
    let connection = open_existing_store(&sync_store_path)?;
    let migrated_counts = store_counts(&connection)?;
    if migrated_counts != copied_counts {
        return Err(SyncStateError::Incompatible {
            path: sync_store_path,
            reason: format!(
                "legacy migration row counts changed: expected {:?}, found {:?}",
                copied_counts, migrated_counts
            ),
        }
        .into());
    }

    Ok(SyncStoreMigrationReport {
        status: SyncStoreMigrationStatus::MigratedLegacy,
        sync_store_path,
        legacy_path: legacy_path.to_path_buf(),
        established: false,
        ledger_rows: migrated_counts.0,
        snapshot_rows: migrated_counts.1,
        config_rows: migrated_counts.2,
    })
}

pub(super) fn open_or_create_sync_store(paths: &SyncProjectPaths) -> Result<Connection> {
    let report = preserve_legacy_sync_state(paths, None)?;
    if report.status == SyncStoreMigrationStatus::NoLegacyState {
        create_empty_store(&report.sync_store_path)?;
    }
    open_existing_store(&report.sync_store_path)
}

pub(super) fn require_sync_store(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
) -> Result<Connection> {
    let report = preserve_legacy_sync_state(paths, None)?;
    if report.status == SyncStoreMigrationStatus::NoLegacyState {
        return Err(SyncStateError::Missing {
            path: report.sync_store_path,
        }
        .into());
    }
    let connection = open_existing_store(&report.sync_store_path)?;
    if !global_baseline_is_established(paths, &connection)? {
        return Err(SyncStateError::Unestablished {
            path: report.sync_store_path,
        }
        .into());
    }
    verify_sync_target(paths, &connection, target)?;
    Ok(connection)
}

/// Prove that the durable sync authority is established for exactly `target`
/// without migrating, creating, or otherwise mutating the store.
pub fn verify_established_sync_target(
    paths: &SyncProjectPaths,
    target: &SyncTargetIdentity,
) -> Result<()> {
    let path = paths.sync_store_path();
    if !path.is_file() {
        return Err(SyncStateError::Missing { path }.into());
    }
    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to open durable sync store {}", path.display()))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .context("failed to set sync-store read timeout")?;
    validate_sync_store_connection(&connection).map_err(|error| {
        anyhow::Error::new(SyncStateError::Incompatible {
            path: path.clone(),
            reason: format!("{error:#}"),
        })
    })?;
    if !global_baseline_is_established(paths, &connection)? {
        return Err(SyncStateError::Unestablished { path }.into());
    }
    verify_sync_target(paths, &connection, target)
}

pub(super) fn verify_sync_target_for_pull(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &SyncTargetIdentity,
) -> Result<()> {
    validate_sync_store_connection(connection)?;
    match stored_sync_target(connection)? {
        Some(stored) if stored == *target => Ok(()),
        Some(stored) => Err(SyncStateError::TargetMismatch {
            path: paths.sync_store_path(),
            stored_api_url: stored.api_url,
            requested_api_url: target.api_url.clone(),
        }
        .into()),
        None if global_baseline_is_established(paths, connection)? => {
            Err(SyncStateError::TargetUnbound {
                path: paths.sync_store_path(),
                requested_api_url: target.api_url.clone(),
            }
            .into())
        }
        None => Ok(()),
    }
}

pub(super) fn bind_sync_target_for_state_write(
    paths: &SyncProjectPaths,
    connection: &Connection,
    target: &SyncTargetIdentity,
) -> Result<()> {
    match stored_sync_target(connection)? {
        Some(stored) if stored == *target => Ok(()),
        Some(stored) => Err(SyncStateError::TargetMismatch {
            path: paths.sync_store_path(),
            stored_api_url: stored.api_url,
            requested_api_url: target.api_url.clone(),
        }
        .into()),
        None if global_baseline_is_established(paths, connection)? => {
            Err(SyncStateError::TargetUnbound {
                path: paths.sync_store_path(),
                requested_api_url: target.api_url.clone(),
            }
            .into())
        }
        None => {
            connection
                .execute(
                    "INSERT INTO sync_store_meta (key, value) VALUES (?1, ?2)",
                    params![TARGET_API_URL_KEY, target.api_url],
                )
                .context("failed to bind durable sync state to its target wiki")?;
            Ok(())
        }
    }
}

pub(super) fn mark_global_baseline_established(
    paths: &SyncProjectPaths,
    connection: &mut Connection,
    target: &SyncTargetIdentity,
) -> Result<()> {
    validate_sync_store_connection(connection)?;
    let transaction = connection
        .transaction()
        .context("failed to begin global sync baseline establishment")?;
    bind_sync_target_for_state_write(paths, &transaction, target)?;
    transaction
        .execute(
            "INSERT INTO sync_store_meta (key, value) VALUES (?1, 'true')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [ESTABLISHED_KEY],
        )
        .context("failed to mark global sync baseline established")?;
    transaction
        .commit()
        .context("failed to commit global sync baseline establishment")?;
    Ok(())
}

pub(super) fn validate_sync_store_connection(connection: &Connection) -> Result<()> {
    let version: i32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("failed to read sync-store schema version")?;
    if version != SYNC_STORE_SCHEMA_VERSION {
        anyhow::bail!(
            "expected sync-store schema version {SYNC_STORE_SCHEMA_VERSION}, found {version}"
        );
    }
    for table in REQUIRED_TABLES {
        if !table_exists(connection, table)? {
            anyhow::bail!("required sync-store table `{table}` is missing");
        }
    }
    Ok(())
}

fn create_empty_store(path: &Path) -> Result<()> {
    publish_store(path, None).map(|_| ())
}

fn migrate_legacy_store(legacy_path: &Path, path: &Path) -> Result<(usize, usize, usize)> {
    publish_store(path, Some(legacy_path))?.ok_or_else(|| {
        anyhow::anyhow!(
            "legacy sync-state migration produced no source counts for {}",
            legacy_path.display()
        )
    })
}

fn publish_store(path: &Path, legacy_path: Option<&Path>) -> Result<Option<(usize, usize, usize)>> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("sync-store path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create sync-store directory {}", parent.display()))?;

    let temporary = tempfile::Builder::new()
        .prefix(".sync-store-")
        .suffix(".sqlite3.tmp")
        .tempfile_in(parent)
        .with_context(|| {
            format!(
                "failed to create temporary sync store in {}",
                parent.display()
            )
        })?
        .into_temp_path();

    let mut copied_counts = None;
    {
        let mut connection = Connection::open(&temporary).with_context(|| {
            format!(
                "failed to open temporary sync store {}",
                temporary.display()
            )
        })?;
        configure_connection(&connection, false)?;
        connection
            .execute_batch(SYNC_STORE_SCHEMA)
            .context("failed to initialize temporary sync-store schema")?;
        connection
            .pragma_update(None, "user_version", SYNC_STORE_SCHEMA_VERSION)
            .context("failed to stamp sync-store schema version")?;

        if let Some(legacy_path) = legacy_path {
            let legacy = legacy_path.to_str().ok_or_else(|| {
                anyhow::anyhow!(
                    "legacy sync-state path is not valid Unicode: {}",
                    legacy_path.display()
                )
            })?;
            connection
                .execute("ATTACH DATABASE ?1 AS legacy", [legacy])
                .with_context(|| {
                    format!(
                        "failed to attach legacy sync state {}",
                        legacy_path.display()
                    )
                })?;
            {
                let transaction = connection
                    .transaction()
                    .context("failed to begin legacy sync-state migration")?;
                let has_snapshots =
                    table_exists_in_schema(&transaction, "legacy", "sync_snapshots")?;
                let has_config = table_exists_in_schema(&transaction, "legacy", "sync_config")?;
                let source_counts = (
                    row_count_in_schema(&transaction, "legacy", "sync_ledger_pages")?,
                    optional_row_count_in_schema(&transaction, "legacy", "sync_snapshots")?,
                    optional_row_count_in_schema(&transaction, "legacy", "sync_config")?,
                );
                if source_counts == (0, 0, 0) {
                    anyhow::bail!(
                        "legacy sync state at {} became empty before migration",
                        legacy_path.display()
                    );
                }
                transaction
                    .execute_batch(
                        "INSERT INTO sync_ledger_pages (
                            title, namespace, relative_path, content_hash, wiki_modified_at,
                            revision_id, page_id, is_redirect, redirect_target,
                            last_synced_at_unix
                         )
                         SELECT title, namespace, relative_path, content_hash, wiki_modified_at,
                            revision_id, page_id, is_redirect, redirect_target,
                            last_synced_at_unix
                         FROM legacy.sync_ledger_pages;",
                    )
                    .context("failed to copy legacy sync ledger")?;
                if has_snapshots {
                    transaction
                        .execute_batch(
                            "INSERT INTO sync_snapshots (title, relative_path, content_text)
                             SELECT title, relative_path, content_text
                             FROM legacy.sync_snapshots;",
                        )
                        .context("failed to copy legacy sync snapshots")?;
                }
                if has_config {
                    transaction
                        .execute_batch(
                            "INSERT INTO sync_config (key, value)
                             SELECT key, value FROM legacy.sync_config;",
                        )
                        .context("failed to copy legacy sync configuration")?;
                }
                let destination_counts = store_counts(&transaction)?;
                if destination_counts != source_counts {
                    anyhow::bail!(
                        "legacy sync-state copy changed row counts: expected {:?}, found {:?}",
                        source_counts,
                        destination_counts
                    );
                }
                transaction
                    .commit()
                    .context("failed to commit legacy sync-state migration")?;
                copied_counts = Some(source_counts);
            }
            connection
                .execute_batch("DETACH DATABASE legacy")
                .context("failed to detach legacy sync state")?;
        }
        validate_sync_store_connection(&connection)?;
    }

    match temporary.persist_noclobber(path) {
        Ok(_) => Ok(copied_counts),
        Err(error) if path.exists() => {
            drop(error);
            let _ = open_existing_store(path)?;
            Ok(copied_counts)
        }
        Err(error) => Err(error.error)
            .with_context(|| format!("failed to publish durable sync store {}", path.display())),
    }
}

fn open_existing_store(path: &Path) -> Result<Connection> {
    let mut connection = Connection::open(path)
        .with_context(|| format!("failed to open durable sync store {}", path.display()))?;
    configure_connection(&connection, true)?;
    migrate_sync_store_connection(&mut connection).map_err(|error| {
        anyhow::Error::new(SyncStateError::Incompatible {
            path: path.to_path_buf(),
            reason: format!("{error:#}"),
        })
    })?;
    validate_sync_store_connection(&connection).map_err(|error| {
        anyhow::Error::new(SyncStateError::Incompatible {
            path: path.to_path_buf(),
            reason: format!("{error:#}"),
        })
    })?;
    Ok(connection)
}

fn migrate_sync_store_connection(connection: &mut Connection) -> Result<()> {
    let version: i32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("failed to read sync-store schema version before migration")?;
    match version {
        SYNC_STORE_SCHEMA_VERSION => Ok(()),
        1 => {
            let transaction = connection
                .transaction()
                .context("failed to begin sync-store v1 to v7 migration")?;
            transaction
                .execute_batch(EDIT_MUTATION_SCHEMA)
                .context("failed to create durable edit-mutation receipts")?;
            transaction
                .execute_batch(DELETE_MUTATION_SCHEMA)
                .context("failed to create durable delete-mutation receipts")?;
            transaction
                .execute_batch(MUTATION_CLOSURE_SCHEMA)
                .context("failed to create mutation closure authority")?;
            transaction
                .pragma_update(None, "user_version", SYNC_STORE_SCHEMA_VERSION)
                .context("failed to stamp sync-store schema version 7")?;
            transaction
                .commit()
                .context("failed to commit sync-store v1 to v7 migration")
        }
        2 => {
            let transaction = connection
                .transaction()
                .context("failed to begin sync-store v2 to v7 migration")?;
            migrate_edit_mutations_to_v4(&transaction)?;
            transaction
                .execute_batch(DELETE_MUTATION_SCHEMA)
                .context("failed to create durable delete-mutation receipts")?;
            transaction
                .execute_batch(MUTATION_CLOSURE_SCHEMA)
                .context("failed to create mutation closure authority")?;
            transaction
                .pragma_update(None, "user_version", SYNC_STORE_SCHEMA_VERSION)
                .context("failed to stamp sync-store schema version 7")?;
            transaction
                .commit()
                .context("failed to commit sync-store v2 to v7 migration")
        }
        3 => {
            let transaction = connection
                .transaction()
                .context("failed to begin sync-store v3 to v7 migration")?;
            migrate_edit_mutations_to_v4(&transaction)?;
            migrate_delete_mutations_to_v4(&transaction)?;
            transaction
                .execute_batch(MUTATION_CLOSURE_SCHEMA)
                .context("failed to create mutation closure authority")?;
            transaction
                .pragma_update(None, "user_version", SYNC_STORE_SCHEMA_VERSION)
                .context("failed to stamp sync-store schema version 7")?;
            transaction
                .commit()
                .context("failed to commit sync-store v3 to v7 migration")
        }
        4 => {
            let transaction = connection
                .transaction()
                .context("failed to begin sync-store v4 to v7 migration")?;
            transaction
                .execute(
                    "ALTER TABLE sync_edit_mutations ADD COLUMN namespace_id INTEGER",
                    [],
                )
                .context("failed to add numeric edit namespace identity")?;
            migrate_delete_mutations_from_v4(&transaction)?;
            transaction
                .execute_batch(MUTATION_CLOSURE_SCHEMA)
                .context("failed to create mutation closure authority")?;
            transaction
                .pragma_update(None, "user_version", SYNC_STORE_SCHEMA_VERSION)
                .context("failed to stamp sync-store schema version 7")?;
            transaction
                .commit()
                .context("failed to commit sync-store v4 to v7 migration")
        }
        5 => {
            let transaction = connection
                .transaction()
                .context("failed to begin sync-store v5 to v7 migration")?;
            migrate_delete_mutations_to_v7(&transaction)?;
            transaction
                .execute_batch(MUTATION_CLOSURE_SCHEMA)
                .context("failed to create mutation closure authority")?;
            transaction
                .pragma_update(None, "user_version", SYNC_STORE_SCHEMA_VERSION)
                .context("failed to stamp sync-store schema version 7")?;
            transaction
                .commit()
                .context("failed to commit sync-store v5 to v7 migration")
        }
        6 => {
            let transaction = connection
                .transaction()
                .context("failed to begin sync-store v6 to v7 migration")?;
            migrate_delete_mutations_to_v7(&transaction)?;
            transaction
                .pragma_update(None, "user_version", SYNC_STORE_SCHEMA_VERSION)
                .context("failed to stamp sync-store schema version 7")?;
            transaction
                .commit()
                .context("failed to commit sync-store v6 to v7 migration")
        }
        other => anyhow::bail!(
            "expected sync-store schema version 1, 2, 3, 4, 5, 6, or {SYNC_STORE_SCHEMA_VERSION}, found {other}"
        ),
    }
}

fn migrate_edit_mutations_to_v4(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction
        .execute_batch(
            "DROP INDEX IF EXISTS idx_sync_edit_mutations_one_unresolved_title;
             DROP INDEX IF EXISTS idx_sync_edit_mutations_phase;
             DROP INDEX IF EXISTS idx_sync_edit_mutations_new_revision;
             ALTER TABLE sync_edit_mutations RENAME TO sync_edit_mutations_pre_v4;",
        )
        .context("failed to preserve pre-v4 edit mutations")?;
    transaction
        .execute_batch(EDIT_MUTATION_SCHEMA)
        .context("failed to create v4 edit-mutation schema")?;
    transaction
        .execute_batch(
            "INSERT INTO sync_edit_mutations (
                mutation_id, target_api_url, title, namespace, relative_path,
                intended_content_sha256, intended_normalized_sha256, summary,
                constraint_kind, expected_revision_id, phase, response_title,
                response_page_id, response_old_revision_id, response_new_revision_id,
                response_new_timestamp, request_started_at_unix,
                detail, created_at_unix, updated_at_unix
             )
             SELECT mutation_id, target_api_url, title, namespace, relative_path,
                    intended_content_sha256, intended_normalized_sha256, summary,
                    constraint_kind, expected_revision_id, phase, response_title,
                    response_page_id, response_old_revision_id, response_new_revision_id,
                    response_new_timestamp, created_at_unix,
                    detail, created_at_unix, updated_at_unix
             FROM sync_edit_mutations_pre_v4;
             DROP TABLE sync_edit_mutations_pre_v4;",
        )
        .context("failed to migrate edit mutations to v4")
}

fn migrate_delete_mutations_to_v4(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction
        .execute_batch(
            "DROP INDEX IF EXISTS idx_sync_delete_mutations_one_unresolved_title;
             DROP INDEX IF EXISTS idx_sync_delete_mutations_phase;
             DROP INDEX IF EXISTS idx_sync_delete_mutations_log_id;
             ALTER TABLE sync_delete_mutations RENAME TO sync_delete_mutations_pre_v4;",
        )
        .context("failed to preserve pre-v4 delete mutations")?;
    transaction
        .execute_batch(DELETE_MUTATION_SCHEMA)
        .context("failed to create v4 delete-mutation schema")?;
    transaction
        .execute_batch(
            "INSERT INTO sync_delete_mutations (
                mutation_id, target_api_url, title, relative_path, expected_revision_id,
                reason, reason_marker, phase, response_kind, response_title,
                response_log_id, response_log_timestamp, detail,
                request_started_at_unix, created_at_unix, updated_at_unix
             )
             SELECT mutation_id, target_api_url, title, relative_path, expected_revision_id,
                    reason, reason_marker, phase, response_kind, response_title,
                    response_log_id, response_log_timestamp, detail,
                    created_at_unix, created_at_unix, updated_at_unix
             FROM sync_delete_mutations_pre_v4;
             DROP TABLE sync_delete_mutations_pre_v4;",
        )
        .context("failed to migrate delete mutations to v4")
}

fn migrate_delete_mutations_from_v4(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction
        .execute_batch(
            "DROP INDEX IF EXISTS idx_sync_delete_mutations_one_unresolved_title;
             DROP INDEX IF EXISTS idx_sync_delete_mutations_phase;
             DROP INDEX IF EXISTS idx_sync_delete_mutations_log_id;
             ALTER TABLE sync_delete_mutations RENAME TO sync_delete_mutations_pre_v5;",
        )
        .context("failed to preserve v4 delete mutations")?;
    transaction
        .execute_batch(DELETE_MUTATION_SCHEMA)
        .context("failed to create v5 delete-mutation schema")?;
    transaction
        .execute_batch(
            "INSERT INTO sync_delete_mutations (
                mutation_id, target_api_url, title, relative_path, expected_revision_id,
                reason, reason_marker, phase, response_kind, response_title,
                response_log_id, response_log_timestamp, request_started_at_unix,
                terminal_outcome, detail, created_at_unix, updated_at_unix
             )
             SELECT mutation_id, target_api_url, title, relative_path, expected_revision_id,
                    reason, reason_marker, phase, response_kind, response_title,
                    response_log_id, response_log_timestamp, request_started_at_unix,
                    terminal_outcome, detail, created_at_unix, updated_at_unix
             FROM sync_delete_mutations_pre_v5;
             DROP TABLE sync_delete_mutations_pre_v5;",
        )
        .context("failed to migrate delete mutations from v4 to v5")
}

fn migrate_delete_mutations_to_v7(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
    transaction
        .execute_batch(
            "DROP INDEX IF EXISTS idx_sync_delete_mutations_one_unresolved_title;
             DROP INDEX IF EXISTS idx_sync_delete_mutations_phase;
             DROP INDEX IF EXISTS idx_sync_delete_mutations_log_id;
             ALTER TABLE sync_delete_mutations RENAME TO sync_delete_mutations_pre_v7;",
        )
        .context("failed to preserve pre-v7 delete mutations")?;
    transaction
        .execute_batch(DELETE_MUTATION_SCHEMA)
        .context("failed to create v7 delete-mutation schema")?;
    transaction
        .execute_batch(
            "INSERT INTO sync_delete_mutations (
                mutation_id, target_api_url, title, relative_path, expected_revision_id,
                reason, reason_marker, phase, response_kind, response_title,
                response_log_id, response_log_timestamp, request_started_at_unix,
                terminal_outcome, backup_enabled, backup_directory, backup_path,
                local_content_sha256, local_effect_status, detail,
                created_at_unix, updated_at_unix
             )
             SELECT mutation_id, target_api_url, title, relative_path, expected_revision_id,
                    reason, reason_marker, phase, response_kind, response_title,
                    response_log_id, response_log_timestamp, request_started_at_unix,
                    terminal_outcome, backup_enabled, backup_directory, backup_path,
                    local_content_sha256, local_effect_status, detail,
                    created_at_unix, updated_at_unix
             FROM sync_delete_mutations_pre_v7;
             DROP TABLE sync_delete_mutations_pre_v7;",
        )
        .context("failed to migrate delete mutations from v6 to v7")
}

fn configure_connection(connection: &Connection, wal: bool) -> Result<()> {
    connection
        .busy_timeout(Duration::from_secs(5))
        .context("failed to set sync-store busy timeout")?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .context("failed to enable sync-store foreign keys")?;
    if wal {
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .context("failed to enable sync-store WAL mode")?;
    }
    connection
        .pragma_update(None, "synchronous", "FULL")
        .context("failed to set durable sync-store fsync policy")?;
    Ok(())
}

fn legacy_sync_counts(path: &Path) -> Result<Option<(usize, usize, usize)>> {
    if !path.exists() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("failed to inspect legacy sync state {}", path.display()))?;
    if !table_exists(&connection, "sync_ledger_pages")? {
        return Ok(None);
    }
    let counts = (
        row_count(&connection, "sync_ledger_pages")?,
        optional_row_count(&connection, "sync_snapshots")?,
        optional_row_count(&connection, "sync_config")?,
    );
    if counts == (0, 0, 0) {
        Ok(None)
    } else {
        Ok(Some(counts))
    }
}

fn store_counts(connection: &Connection) -> Result<(usize, usize, usize)> {
    Ok((
        row_count(connection, "sync_ledger_pages")?,
        row_count(connection, "sync_snapshots")?,
        row_count(connection, "sync_config")?,
    ))
}

fn optional_row_count(connection: &Connection, table: &str) -> Result<usize> {
    if table_exists(connection, table)? {
        row_count(connection, table)
    } else {
        Ok(0)
    }
}

fn optional_row_count_in_schema(
    connection: &Connection,
    schema: &str,
    table: &str,
) -> Result<usize> {
    if table_exists_in_schema(connection, schema, table)? {
        row_count_in_schema(connection, schema, table)
    } else {
        Ok(0)
    }
}

fn row_count(connection: &Connection, table: &str) -> Result<usize> {
    row_count_in_schema(connection, "main", table)
}

fn row_count_in_schema(connection: &Connection, schema: &str, table: &str) -> Result<usize> {
    if !matches!(schema, "main" | "legacy") {
        anyhow::bail!("unsupported sqlite schema name `{schema}`");
    }
    let sql = format!("SELECT COUNT(*) FROM {schema}.{table}");
    let count: i64 = connection
        .query_row(&sql, [], |row| row.get(0))
        .with_context(|| format!("failed to count sync-store table {table}"))?;
    usize::try_from(count).context("sync-store row count does not fit into usize")
}

pub(super) fn global_baseline_is_established(
    paths: &SyncProjectPaths,
    connection: &Connection,
) -> Result<bool> {
    let value = connection
        .query_row(
            "SELECT value FROM sync_store_meta WHERE key = ?1",
            [ESTABLISHED_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("failed to inspect global sync baseline establishment")?;
    match value.as_deref() {
        None => Ok(false),
        Some("true") => Ok(true),
        Some(invalid) => Err(SyncStateError::Incompatible {
            path: paths.sync_store_path(),
            reason: format!(
                "global baseline marker {ESTABLISHED_KEY:?} has invalid value {invalid:?}; expected literal \"true\" or no row"
            ),
        }
        .into()),
    }
}

fn stored_sync_target(connection: &Connection) -> Result<Option<SyncTargetIdentity>> {
    let stored = connection
        .query_row(
            "SELECT value FROM sync_store_meta WHERE key = ?1",
            [TARGET_API_URL_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("failed to read durable sync target identity")?;
    stored
        .map(|api_url| SyncTargetIdentity::from_api_url(&api_url))
        .transpose()
}

fn verify_sync_target(
    paths: &SyncProjectPaths,
    connection: &Connection,
    requested: &SyncTargetIdentity,
) -> Result<()> {
    let Some(stored) = stored_sync_target(connection)? else {
        return Err(SyncStateError::TargetUnbound {
            path: paths.sync_store_path(),
            requested_api_url: requested.api_url.clone(),
        }
        .into());
    };
    if stored != *requested {
        return Err(SyncStateError::TargetMismatch {
            path: paths.sync_store_path(),
            stored_api_url: stored.api_url,
            requested_api_url: requested.api_url.clone(),
        }
        .into());
    }
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    table_exists_in_schema(connection, "main", table)
}

fn table_exists_in_schema(connection: &Connection, schema: &str, table: &str) -> Result<bool> {
    if !matches!(schema, "main" | "legacy") {
        anyhow::bail!("unsupported sqlite schema name `{schema}`");
    }
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {schema}.sqlite_master WHERE type = 'table' AND name = ?1)"
    );
    connection
        .query_row(&sql, [table], |row| row.get::<_, i64>(0))
        .map(|value| value != 0)
        .with_context(|| format!("failed to inspect sqlite table {schema}.{table}"))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn test_paths(project_root: &Path) -> SyncProjectPaths {
        SyncProjectPaths {
            project_root: project_root.to_path_buf(),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            sync_store_path: project_root.join(".wikitool/sync/sync.sqlite3"),
            custom_namespaces: Vec::new(),
            template_category_mappings: Vec::new(),
        }
    }

    fn legacy_path(paths: &SyncProjectPaths) -> PathBuf {
        paths.project_root.join(".wikitool/data/wikitool.db")
    }

    fn create_legacy_sync_state(paths: &SyncProjectPaths) {
        let legacy_path = legacy_path(paths);
        fs::create_dir_all(legacy_path.parent().expect("legacy database parent"))
            .expect("create legacy data directory");
        let connection = Connection::open(&legacy_path).expect("open legacy database");
        connection
            .execute_batch(SYNC_STORE_SCHEMA)
            .expect("create legacy sync tables");
        connection
            .execute(
                "INSERT INTO sync_ledger_pages (
                    title, namespace, relative_path, content_hash, wiki_modified_at,
                    revision_id, page_id, is_redirect, redirect_target,
                    last_synced_at_unix
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    "Alpha",
                    0,
                    "wiki_content/Main/Alpha.wiki",
                    "sha256:alpha",
                    "2026-08-27T00:00:00Z",
                    42,
                    7,
                    0,
                    Option::<String>::None,
                    1_777_000_000_i64,
                ],
            )
            .expect("insert legacy ledger row");
        connection
            .execute(
                "INSERT INTO sync_snapshots (title, relative_path, content_text)
                 VALUES (?1, ?2, ?3)",
                ["Alpha", "wiki_content/Main/Alpha.wiki", "== Alpha =="],
            )
            .expect("insert legacy snapshot row");
        connection
            .execute(
                "INSERT INTO sync_config (key, value) VALUES (?1, ?2)",
                ["last_pull_ns_0", "2026-08-27T00:00:00Z"],
            )
            .expect("insert legacy config row");
    }

    fn target() -> SyncTargetIdentity {
        SyncTargetIdentity::from_api_url("https://wiki-a.example/api.php")
            .expect("test target identity")
    }

    #[test]
    fn missing_sync_state_is_a_typed_failure() {
        let temp = tempdir().expect("tempdir");
        let paths = test_paths(temp.path());

        let error = require_sync_store(&paths, &target()).expect_err("sync state must be required");
        let typed = error
            .downcast_ref::<SyncStateError>()
            .expect("typed sync-state error");
        assert!(
            matches!(typed, SyncStateError::Missing { path } if path == &paths.sync_store_path())
        );
        assert!(!paths.sync_store_path().exists());
    }

    #[test]
    fn read_only_target_verification_requires_established_exact_authority() {
        let temp = tempdir().expect("tempdir");
        let paths = test_paths(temp.path());
        let missing = verify_established_sync_target(&paths, &target())
            .expect_err("verification must not create missing authority");
        assert!(matches!(
            missing.downcast_ref::<SyncStateError>(),
            Some(SyncStateError::Missing { path }) if path == &paths.sync_store_path()
        ));
        assert!(!paths.sync_store_path().exists());

        let mut connection = open_or_create_sync_store(&paths).expect("create sync store");
        mark_global_baseline_established(&paths, &mut connection, &target())
            .expect("establish target authority");
        drop(connection);

        verify_established_sync_target(&paths, &target()).expect("exact target verifies");
        let other = SyncTargetIdentity::from_api_url("https://wiki-b.example/api.php")
            .expect("other target");
        let mismatch = verify_established_sync_target(&paths, &other)
            .expect_err("different target must fail closed");
        assert!(matches!(
            mismatch.downcast_ref::<SyncStateError>(),
            Some(SyncStateError::TargetMismatch {
                stored_api_url,
                requested_api_url,
                ..
            }) if stored_api_url == "https://wiki-a.example/api.php"
                && requested_api_url == "https://wiki-b.example/api.php"
        ));
    }

    #[test]
    fn legacy_migration_preserves_rows_without_claiming_global_coverage() {
        let temp = tempdir().expect("tempdir");
        let paths = test_paths(temp.path());
        create_legacy_sync_state(&paths);
        let legacy_path = legacy_path(&paths);

        let report =
            preserve_legacy_sync_state(&paths, Some(&legacy_path)).expect("migrate legacy state");
        assert_eq!(report.status, SyncStoreMigrationStatus::MigratedLegacy);
        assert!(!report.established);
        assert_eq!(
            (report.ledger_rows, report.snapshot_rows, report.config_rows),
            (1, 1, 1)
        );
        assert!(legacy_path.exists(), "migration must not delete its source");

        fs::remove_file(&legacy_path).expect("simulate catalog reset");
        let error = require_sync_store(&paths, &target())
            .expect_err("legacy row presence cannot prove authoritative baseline coverage");
        assert!(matches!(
            error.downcast_ref::<SyncStateError>(),
            Some(SyncStateError::Unestablished { path }) if path == &paths.sync_store_path()
        ));

        let connection = open_existing_store(&paths.sync_store_path())
            .expect("open preserved sync rows without granting planning authority");
        assert_eq!(
            store_counts(&connection).expect("count preserved state"),
            (1, 1, 1)
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT revision_id FROM sync_ledger_pages WHERE title = 'Alpha'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("read preserved revision identity"),
            42
        );
    }

    #[test]
    fn fresh_pull_store_is_created_outside_disposable_catalog() {
        let temp = tempdir().expect("tempdir");
        let paths = test_paths(temp.path());

        let connection = open_or_create_sync_store(&paths).expect("create sync store");
        assert_eq!(
            store_counts(&connection).expect("count fresh store"),
            (0, 0, 0)
        );
        assert!(paths.sync_store_path().exists());
        assert!(!legacy_path(&paths).exists());
        let error =
            require_sync_store(&paths, &target()).expect_err("empty store is not a pull baseline");
        assert!(matches!(
            error.downcast_ref::<SyncStateError>(),
            Some(SyncStateError::Unestablished { path }) if path == &paths.sync_store_path()
        ));
    }

    #[test]
    fn only_literal_true_can_authorize_the_global_baseline_marker() {
        let temp = tempdir().expect("tempdir");
        for invalid in ["false", "0", "garbage"] {
            let paths = test_paths(&temp.path().join(invalid));
            let connection = open_or_create_sync_store(&paths).expect("create sync store");
            connection
                .execute(
                    "INSERT INTO sync_store_meta (key, value) VALUES (?1, ?2)",
                    params![ESTABLISHED_KEY, invalid],
                )
                .expect("insert corrupt marker");
            drop(connection);

            let error = require_sync_store(&paths, &target())
                .expect_err("non-literal marker must fail as corrupt state");
            assert!(matches!(
                error.downcast_ref::<SyncStateError>(),
                Some(SyncStateError::Incompatible { path, reason })
                    if path == &paths.sync_store_path()
                        && reason.contains("invalid value")
                        && reason.contains(invalid)
            ));
        }
    }

    #[test]
    fn version_one_store_migrates_transactionally_to_edit_receipts() {
        let temp = tempdir().expect("tempdir");
        let paths = test_paths(temp.path());
        let connection = open_or_create_sync_store(&paths).expect("create current store");
        connection
            .execute(
                "INSERT INTO sync_config (key, value) VALUES ('sentinel', 'preserved')",
                [],
            )
            .expect("insert sentinel");
        connection
            .execute_batch(
                "DROP TABLE sync_invalidated_titles;
                 DROP TABLE sync_mutation_closures;
                 DROP TABLE sync_edit_mutations;
                 DROP TABLE sync_delete_mutations;
                 PRAGMA user_version = 1;",
            )
            .expect("downgrade fixture to schema version 1");
        drop(connection);

        let migrated = open_existing_store(&paths.sync_store_path()).expect("migrate v1 store");
        assert!(table_exists(&migrated, "sync_edit_mutations").expect("receipt table"));
        assert_eq!(
            migrated
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
                .expect("schema version"),
            SYNC_STORE_SCHEMA_VERSION
        );
        assert_eq!(
            migrated
                .query_row(
                    "SELECT value FROM sync_config WHERE key = 'sentinel'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("preserved sentinel"),
            "preserved"
        );
    }

    #[test]
    fn version_two_ambiguous_edit_migrates_as_request_started() {
        let temp = tempdir().expect("tempdir");
        let paths = test_paths(temp.path());
        let connection = open_or_create_sync_store(&paths).expect("create current store");
        connection
            .execute_batch(
                "DROP TABLE sync_invalidated_titles;
                 DROP TABLE sync_mutation_closures;
                 DROP TABLE sync_delete_mutations;
                 DROP TABLE sync_edit_mutations;
                 CREATE TABLE sync_edit_mutations (
                    mutation_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    target_api_url TEXT NOT NULL,
                    title TEXT NOT NULL,
                    namespace TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    intended_content_sha256 TEXT NOT NULL,
                    intended_normalized_sha256 TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    constraint_kind TEXT NOT NULL,
                    expected_revision_id INTEGER,
                    phase TEXT NOT NULL,
                    response_title TEXT,
                    response_page_id INTEGER,
                    response_old_revision_id INTEGER,
                    response_new_revision_id INTEGER,
                    response_new_timestamp TEXT,
                    detail TEXT,
                    created_at_unix INTEGER NOT NULL,
                    updated_at_unix INTEGER NOT NULL
                 );
                 INSERT INTO sync_edit_mutations (
                    target_api_url, title, namespace, relative_path,
                    intended_content_sha256, intended_normalized_sha256, summary,
                    constraint_kind, expected_revision_id, phase, detail,
                    created_at_unix, updated_at_unix
                 ) VALUES (
                    'https://wiki.example/api.php', 'Alpha', 'Main',
                    'wiki_content/Main/Alpha.wiki', 'exact', 'normalized', 'edit',
                    'existing_revision', 41, 'outcome_ambiguous', 'pending', 7, 8
                 );
                 PRAGMA user_version = 2;",
            )
            .expect("build literal pre-change v2 fixture");
        drop(connection);

        let migrated = open_existing_store(&paths.sync_store_path()).expect("migrate v2 store");
        assert!(
            migrated
                .query_row(
                    "SELECT phase = 'outcome_ambiguous'
                            AND request_started_at_unix = created_at_unix
                            AND terminal_outcome IS NULL
                     FROM sync_edit_mutations WHERE title = 'Alpha'",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .expect("legacy ambiguous edit remains conservatively request-started")
        );
        assert!(table_exists(&migrated, "sync_delete_mutations").expect("delete table"));
        assert!(table_exists(&migrated, "sync_mutation_closures").expect("closure table"));
    }

    #[test]
    fn version_five_store_adds_operator_closure_authority_transactionally() {
        let temp = tempdir().expect("tempdir");
        let paths = test_paths(temp.path());
        let connection = open_or_create_sync_store(&paths).expect("create current store");
        connection
            .execute_batch(
                "DROP TABLE sync_invalidated_titles;
                 DROP TABLE sync_mutation_closures;
                 DROP TABLE sync_delete_mutations;
                 PRAGMA user_version = 5;",
            )
            .expect("downgrade fixture to schema version 5");
        let version_five_schema = DELETE_MUTATION_SCHEMA.replace("        'source_staging',\n", "");
        connection
            .execute_batch(&version_five_schema)
            .expect("create literal version-five delete table");
        drop(connection);

        let migrated = open_existing_store(&paths.sync_store_path()).expect("migrate v5 store");
        assert!(table_exists(&migrated, "sync_mutation_closures").expect("closure table"));
        assert!(table_exists(&migrated, "sync_invalidated_titles").expect("invalidation table"));
        migrated
            .execute(
                "INSERT INTO sync_delete_mutations (
                    target_api_url, title, expected_revision_id, reason,
                    reason_marker, phase, local_effect_status,
                    created_at_unix, updated_at_unix
                 ) VALUES (
                    'https://wiki.example/api.php', 'Alpha', 42, 'delete',
                    'wikitool-delete:v5', 'response_bound', 'source_staging', 7, 7
                 )",
                [],
            )
            .expect("migrated v5 store accepts pre-rename authority status");
        assert_eq!(
            migrated
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
                .expect("schema version"),
            SYNC_STORE_SCHEMA_VERSION
        );
    }

    #[test]
    fn version_six_store_adds_pre_rename_source_staging_authority() {
        let temp = tempdir().expect("tempdir");
        let paths = test_paths(temp.path());
        let connection = open_or_create_sync_store(&paths).expect("create current store");
        connection
            .execute_batch(
                "DROP TABLE sync_delete_mutations;
                 PRAGMA user_version = 6;",
            )
            .expect("remove current delete table");
        let version_six_schema = DELETE_MUTATION_SCHEMA.replace("        'source_staging',\n", "");
        connection
            .execute_batch(&version_six_schema)
            .expect("create literal version-six delete table");
        connection
            .execute(
                "INSERT INTO sync_delete_mutations (
                    target_api_url, title, relative_path, expected_revision_id,
                    reason, reason_marker, phase, response_kind, response_title,
                    request_started_at_unix, backup_enabled, backup_directory,
                    backup_path, local_content_sha256, local_effect_status,
                    detail, created_at_unix, updated_at_unix
                 ) VALUES (
                    'https://wiki.example/api.php', 'Alpha',
                    'wiki_content/Main/Alpha.wiki', 42, 'delete',
                    'wikitool-delete:v6', 'response_bound', 'already_missing',
                    'Alpha', 7, 1, '.wikitool/sync/backups',
                    '.wikitool/sync/backups/Alpha.wiki', 'exact',
                    'backup_ready', 'preserved cause', 7, 8
                 )",
                [],
            )
            .expect("insert version-six delete mutation");
        drop(connection);

        let migrated = open_existing_store(&paths.sync_store_path()).expect("migrate v6 store");
        assert_eq!(
            migrated
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
                .expect("schema version"),
            SYNC_STORE_SCHEMA_VERSION
        );
        assert_eq!(
            migrated
                .query_row(
                    "SELECT relative_path, local_effect_status, detail
                     FROM sync_delete_mutations WHERE title = 'Alpha'",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .expect("preserved mutation authority"),
            (
                "wiki_content/Main/Alpha.wiki".to_string(),
                "backup_ready".to_string(),
                "preserved cause".to_string(),
            )
        );
        migrated
            .execute(
                "UPDATE sync_delete_mutations
                 SET local_effect_status = 'source_staging'
                 WHERE title = 'Alpha'",
                [],
            )
            .expect("new pre-rename authority status is accepted");
    }

    #[test]
    fn version_three_store_migrates_pending_mutations_without_losing_delete_path() {
        let temp = tempdir().expect("tempdir");
        let paths = test_paths(temp.path());
        let connection = open_or_create_sync_store(&paths).expect("create current store");
        connection
            .execute_batch(
                "DROP TABLE sync_invalidated_titles;
                 DROP TABLE sync_mutation_closures;
                 DROP TABLE sync_edit_mutations;
                 DROP TABLE sync_delete_mutations;
                 CREATE TABLE sync_edit_mutations (
                    mutation_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    target_api_url TEXT NOT NULL,
                    title TEXT NOT NULL,
                    namespace TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    intended_content_sha256 TEXT NOT NULL,
                    intended_normalized_sha256 TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    constraint_kind TEXT NOT NULL,
                    expected_revision_id INTEGER,
                    phase TEXT NOT NULL,
                    response_title TEXT,
                    response_page_id INTEGER,
                    response_old_revision_id INTEGER,
                    response_new_revision_id INTEGER,
                    response_new_timestamp TEXT,
                    detail TEXT,
                    created_at_unix INTEGER NOT NULL,
                    updated_at_unix INTEGER NOT NULL
                 );
                 CREATE TABLE sync_delete_mutations (
                    mutation_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    target_api_url TEXT NOT NULL,
                    title TEXT NOT NULL,
                    relative_path TEXT,
                    expected_revision_id INTEGER NOT NULL,
                    reason TEXT NOT NULL,
                    reason_marker TEXT NOT NULL UNIQUE,
                    phase TEXT NOT NULL,
                    response_kind TEXT,
                    response_title TEXT,
                    response_log_id INTEGER,
                    response_log_timestamp TEXT,
                    detail TEXT,
                    created_at_unix INTEGER NOT NULL,
                    updated_at_unix INTEGER NOT NULL
                 );
                 INSERT INTO sync_edit_mutations (
                    target_api_url, title, namespace, relative_path,
                    intended_content_sha256, intended_normalized_sha256, summary,
                    constraint_kind, expected_revision_id, phase, detail,
                    created_at_unix, updated_at_unix
                 ) VALUES (
                    'https://wiki.example/api.php', 'Alpha', 'Main',
                    'wiki_content/Main/Alpha.wiki', 'exact', 'normalized', 'edit',
                    'existing_revision', 41, 'outcome_ambiguous', 'pending', 1, 1
                 );
                 INSERT INTO sync_delete_mutations (
                    target_api_url, title, relative_path, expected_revision_id,
                    reason, reason_marker, phase, detail, created_at_unix, updated_at_unix
                 ) VALUES (
                    'https://wiki.example/api.php', 'Beta',
                    'wiki_content/Main/Beta.wiki', 42, 'delete',
                    'wikitool-delete:fixture', 'outcome_ambiguous', 'pending', 1, 1
                 );
                 PRAGMA user_version = 3;",
            )
            .expect("build literal pre-change v3 fixture");
        drop(connection);

        let migrated = open_existing_store(&paths.sync_store_path()).expect("migrate v3 store");
        assert_eq!(
            migrated
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
                .expect("schema version"),
            SYNC_STORE_SCHEMA_VERSION
        );
        assert_eq!(
            migrated
                .query_row(
                    "SELECT relative_path FROM sync_delete_mutations WHERE title = 'Beta'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("preserved delete cleanup path"),
            "wiki_content/Main/Beta.wiki"
        );
        assert_eq!(
            migrated
                .query_row(
                    "SELECT phase FROM sync_edit_mutations WHERE title = 'Alpha'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("preserved edit phase"),
            "outcome_ambiguous"
        );
        assert!(
            migrated
                .query_row(
                    "SELECT request_started_at_unix = created_at_unix AND terminal_outcome IS NULL
                     FROM sync_delete_mutations WHERE title = 'Beta'",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .expect("legacy ambiguous mutation is conservatively request-started")
        );
        assert!(
            migrated
                .query_row(
                    "SELECT request_started_at_unix = created_at_unix
                     FROM sync_edit_mutations WHERE title = 'Alpha'",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .expect("legacy ambiguous edit is conservatively request-started")
        );
    }
}
