use super::*;

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

pub(super) fn collect_sync_planning_context(
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

pub(super) fn hydrate_remote_conflicts<A: WikiWriteApi>(
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
        .map(|change| (normalized_title_key(&change.title), change.title.clone()))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect::<Vec<_>>();
    if titles.is_empty() {
        context.request_count = api.request_count();
        return Ok(());
    }
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

pub(super) fn count_changes(
    changes: &[PlannedSyncChangeInternal],
    change_type: DiffChangeType,
) -> usize {
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
            resolved
                .title_keys
                .insert(normalized_title_key(&normalized));
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

fn change_order(change_type: &DiffChangeType) -> u8 {
    match change_type {
        DiffChangeType::NewLocal => 0,
        DiffChangeType::ModifiedLocal => 1,
        DiffChangeType::DeletedLocal => 2,
    }
}
