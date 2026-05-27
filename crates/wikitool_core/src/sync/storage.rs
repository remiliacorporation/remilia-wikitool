use super::*;

pub(super) fn remove_sync_ledger_entry(connection: &Connection, title: &str) -> Result<()> {
    initialize_sync_schema(connection)?;
    connection
        .execute(
            "DELETE FROM sync_ledger_pages WHERE lower(title) = lower(?1)",
            [title],
        )
        .with_context(|| format!("failed to delete sync ledger row for {title}"))?;
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

pub(super) fn upsert_sync_snapshot(
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

pub(super) fn remove_sync_snapshot(connection: &Connection, title: &str) -> Result<()> {
    initialize_sync_schema(connection)?;
    connection
        .execute(
            "DELETE FROM sync_snapshots WHERE lower(title) = lower(?1)",
            [title],
        )
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

pub(super) fn upsert_sync_ledger(
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

pub(super) fn open_sync_connection(paths: &ResolvedPaths) -> Result<Connection> {
    open_initialized_database_connection(&paths.db_path)
}

pub(super) fn initialize_sync_schema(connection: &Connection) -> Result<()> {
    ensure_database_schema_connection(connection)
}

pub(super) fn ensure_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))
}

pub(super) fn absolute_path_from_relative(paths: &ResolvedPaths, relative: &str) -> PathBuf {
    let mut output = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            output.push(segment);
        }
    }
    output
}

pub(super) fn normalized_title_key(title: &str) -> String {
    normalize_title_for_storage(title).to_ascii_lowercase()
}

pub(super) fn normalize_title_for_storage(title: &str) -> String {
    title.replace('_', " ").trim().to_string()
}
