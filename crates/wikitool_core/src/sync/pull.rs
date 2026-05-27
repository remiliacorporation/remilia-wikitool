use super::*;

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

pub(super) fn pull_from_remote_with_api<A: WikiReadApi>(
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
