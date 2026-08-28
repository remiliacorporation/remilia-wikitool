use super::*;

pub fn pull_from_remote_with_api<A: WikiReadApi>(
    paths: &SyncProjectPaths,
    options: &PullOptions,
    target: &SyncTargetIdentity,
    api: &mut A,
) -> Result<PullReport> {
    target.ensure_matches_api(api.target_api_url())?;
    let authoritative_global_pull = is_authoritative_global_pull(options)?;
    let mut connection = open_sync_connection(paths)?;
    initialize_sync_schema(&connection)?;
    verify_sync_target_for_pull(paths, &connection, target)?;
    let baseline_was_established = global_baseline_is_established(paths, &connection)?;

    let mut report = PullReport {
        success: true,
        global_baseline_established: false,
        requested_pages: 0,
        pulled: 0,
        created: 0,
        updated: 0,
        deleted: 0,
        skipped: 0,
        errors: Vec::new(),
        pages: Vec::new(),
        request_count: 0,
        changed_paths: Vec::new(),
    };

    let pages_to_pull = resolve_pages_to_pull(&connection, options, api)?;
    report.requested_pages = pages_to_pull.len();
    if pages_to_pull.is_empty() {
        if authoritative_global_pull {
            let existing_local_by_title = load_existing_local_files(paths)?;
            reconcile_global_baseline_ledger(
                paths,
                &connection,
                &pages_to_pull,
                &existing_local_by_title,
                &mut report,
            )?;
            mark_global_baseline_established(paths, &mut connection, target)?;
            report.global_baseline_established = true;
        }
        report.request_count = api.request_count();
        return Ok(report);
    }

    let content_rows = api.get_page_contents(&pages_to_pull)?;
    let mut content_by_title = BTreeMap::new();
    for page in content_rows {
        content_by_title.insert(normalized_title_key(&page.title), page);
    }
    let mut ledger_by_title = load_sync_ledger_map(&connection, true)?;
    let mut refreshed_baseline_titles = BTreeSet::new();

    let mut max_timestamp: Option<String> = None;
    let namespace_mapper = NamespaceMapper::load(paths)?;
    let existing_local_by_title = load_existing_local_files(paths)?;
    let relative_paths_by_title = select_relative_paths_for_pull(
        paths,
        &pages_to_pull,
        &content_by_title,
        &namespace_mapper,
        &existing_local_by_title,
    );
    let protected_relative_path_keys = relative_paths_by_title
        .values()
        .map(|relative_path| case_insensitive_path_key(relative_path))
        .collect::<BTreeSet<_>>();

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

        if let Some((operation, mutation_id, phase)) =
            unresolved_remote_mutation(&connection, &page.title)?
        {
            report.skipped += 1;
            report.pages.push(PullPageResult {
                title: page.title.clone(),
                action: "retained_unresolved_mutation".to_string(),
                detail: Some(format!(
                    "remote {operation} mutation {mutation_id} is still in phase {phase}; pull did not rewrite local or sync authority for this title"
                )),
            });
            continue;
        }

        let (is_redirect, redirect_target) = parse_redirect(&page.content);
        let relative_path = relative_paths_by_title
            .get(&key)
            .cloned()
            .unwrap_or_else(|| {
                namespace_mapper.title_to_relative_path(paths, &page.title, is_redirect)
            });
        let absolute_path = absolute_path_from_relative(paths, &relative_path);
        validate_scoped_path(paths, &absolute_path)?;
        ensure_parent_dir(&absolute_path)?;

        let remote_hash = compute_wiki_sync_hash(&page.content);
        let ledger_entry = ledger_by_title.get(&key).cloned();
        let stale_synced_path = stale_synced_path_for_removal(
            paths,
            &ledger_entry,
            &relative_path,
            &protected_relative_path_keys,
            options.overwrite_local,
        )?;

        let local_content = fs::read_to_string(&absolute_path).ok();
        let local_hash = local_content.as_deref().map(compute_wiki_sync_hash);

        let local_modified = match (&local_hash, &ledger_entry) {
            (Some(local_hash), Some(entry)) => local_hash != &entry.content_hash,
            (Some(_), None) => true,
            (None, _) => false,
        };

        if let Some(local_hash) = &local_hash
            && local_hash == &remote_hash
        {
            if remove_stale_synced_path(stale_synced_path.as_deref())? {
                report.changed_paths.push(SyncPathEffect {
                    relative_path: stale_synced_path
                        .as_deref()
                        .and_then(|path| path.strip_prefix(&paths.project_root).ok())
                        .map(normalize_path)
                        .unwrap_or_default(),
                    kind: SyncPathEffectKind::Deleted,
                });
            }
            upsert_sync_page_state(
                paths,
                &connection,
                target,
                SyncPageState {
                    page,
                    relative_path: &relative_path,
                    content_hash: &remote_hash,
                    is_redirect,
                    redirect_target: redirect_target.as_deref(),
                },
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
            refreshed_baseline_titles.insert(key);
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
        crate::support::atomic_write(&absolute_path, &page.content)?;
        report.changed_paths.push(SyncPathEffect {
            relative_path: relative_path.clone(),
            kind: if existed_before {
                SyncPathEffectKind::Updated
            } else {
                SyncPathEffectKind::Created
            },
        });
        if remove_stale_synced_path(stale_synced_path.as_deref())? {
            report.changed_paths.push(SyncPathEffect {
                relative_path: stale_synced_path
                    .as_deref()
                    .and_then(|path| path.strip_prefix(&paths.project_root).ok())
                    .map(normalize_path)
                    .unwrap_or_default(),
                kind: SyncPathEffectKind::Deleted,
            });
        }
        upsert_sync_page_state(
            paths,
            &connection,
            target,
            SyncPageState {
                page,
                relative_path: &relative_path,
                content_hash: &remote_hash,
                is_redirect,
                redirect_target: redirect_target.as_deref(),
            },
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
        refreshed_baseline_titles.insert(key);
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

    report.request_count = api.request_count();
    let baseline_complete = pages_to_pull
        .iter()
        .all(|title| refreshed_baseline_titles.contains(&normalized_title_key(title)));
    if report.errors.is_empty() {
        if baseline_complete && authoritative_global_pull {
            reconcile_global_baseline_ledger(
                paths,
                &connection,
                &pages_to_pull,
                &existing_local_by_title,
                &mut report,
            )?;
            mark_global_baseline_established(paths, &mut connection, target)?;
            report.global_baseline_established = true;
        } else if !baseline_was_established && authoritative_global_pull {
            report.errors.push(
                "pull did not establish a complete global sync baseline because one or more discovered pages were skipped without prior revision identity"
                    .to_string(),
            );
        }
    }
    report.changed_paths.sort_by(|left, right| {
        left.relative_path
            .cmp(&right.relative_path)
            .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
    });
    report.changed_paths.dedup();
    report.success = report.errors.is_empty();
    Ok(report)
}

fn reconcile_global_baseline_ledger(
    paths: &SyncProjectPaths,
    connection: &Connection,
    remote_titles: &[String],
    local_by_title: &BTreeMap<String, ScannedFile>,
    report: &mut PullReport,
) -> Result<()> {
    let remote_keys = remote_titles
        .iter()
        .map(|title| normalized_title_key(title))
        .collect::<BTreeSet<_>>();
    for (key, entry) in load_sync_ledger_map(connection, true)? {
        if remote_keys.contains(&key) {
            continue;
        }
        if let Some((operation, mutation_id, phase)) =
            unresolved_remote_mutation(connection, &entry.title)?
        {
            report.skipped += 1;
            report.pages.push(PullPageResult {
                title: entry.title,
                action: "retained_unresolved_mutation".to_string(),
                detail: Some(format!(
                    "remote {operation} mutation {mutation_id} is still in phase {phase}; authoritative absence did not rewrite local or sync authority"
                )),
            });
            continue;
        }
        let Some(local) = local_by_title.get(&key) else {
            remove_sync_page_state(connection, &entry.title)?;
            clear_sync_title_invalidation(connection, &entry.title)?;
            report.pages.push(PullPageResult {
                title: entry.title,
                action: "pruned_absent_remote_state".to_string(),
                detail: Some("remote and local page are both absent".to_string()),
            });
            continue;
        };
        if local.content_hash != entry.content_hash {
            clear_sync_title_invalidation(connection, &entry.title)?;
            report.pages.push(PullPageResult {
                title: entry.title,
                action: "retained_remote_delete_conflict".to_string(),
                detail: Some(
                    "remote page was deleted, but the local page has edits; sync identity was retained so recreation requires explicit conflict handling"
                        .to_string(),
                ),
            });
            continue;
        }

        let absolute = absolute_path_from_relative(paths, &local.relative_path);
        validate_scoped_path(paths, &absolute)?;
        let current_content = fs::read_to_string(&absolute).with_context(|| {
            format!(
                "failed to read remotely deleted page {}",
                absolute.display()
            )
        })?;
        if compute_wiki_sync_hash(&current_content) != entry.content_hash {
            clear_sync_title_invalidation(connection, &entry.title)?;
            report.pages.push(PullPageResult {
                title: entry.title,
                action: "retained_remote_delete_conflict".to_string(),
                detail: Some(
                    "remote page was deleted, but the local page changed during reconciliation; sync identity was retained"
                        .to_string(),
                ),
            });
            continue;
        }
        fs::remove_file(&absolute).with_context(|| {
            format!(
                "failed to remove unchanged local page deleted remotely: {}",
                absolute.display()
            )
        })?;
        report.changed_paths.push(SyncPathEffect {
            relative_path: local.relative_path.clone(),
            kind: SyncPathEffectKind::Deleted,
        });
        remove_sync_page_state(connection, &entry.title).with_context(|| {
            format!(
                "removed unchanged local page {} after authoritative remote deletion, but failed to remove its transactional sync state; rerun the full pull to finish reconciliation",
                absolute.display()
            )
        })?;
        clear_sync_title_invalidation(connection, &entry.title)?;
        report.deleted += 1;
        report.pages.push(PullPageResult {
            title: entry.title,
            action: "deleted_remote_absent".to_string(),
            detail: Some(
                "unchanged local page removed after authoritative remote deletion".to_string(),
            ),
        });
    }
    Ok(())
}

fn is_authoritative_global_pull(options: &PullOptions) -> Result<bool> {
    if options.coverage == PullCoverage::Scoped {
        return Ok(false);
    }
    const REQUIRED_NAMESPACES: &[i32] =
        &[NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
    if !options.full || options.category.is_some() {
        bail!("global baseline coverage requires a full pull with no category selector");
    }
    if !REQUIRED_NAMESPACES
        .iter()
        .all(|required| options.namespaces.contains(required))
    {
        bail!(
            "global baseline coverage requires Main, Category, Template, Module, and MediaWiki namespaces"
        );
    }
    Ok(true)
}

fn load_existing_local_files(paths: &SyncProjectPaths) -> Result<BTreeMap<String, ScannedFile>> {
    let mut out = BTreeMap::new();
    for file in scan_files(
        paths,
        &ScanOptions {
            include_content: true,
            include_templates: true,
            ..ScanOptions::default()
        },
    )? {
        out.insert(normalized_title_key(&file.title), file);
    }
    Ok(out)
}

fn select_relative_paths_for_pull(
    paths: &SyncProjectPaths,
    pages_to_pull: &[String],
    content_by_title: &BTreeMap<String, RemotePage>,
    namespace_mapper: &NamespaceMapper,
    existing_local_by_title: &BTreeMap<String, ScannedFile>,
) -> BTreeMap<String, String> {
    let mut candidates = Vec::new();

    for title in pages_to_pull {
        let key = normalized_title_key(title);
        let Some(page) = content_by_title.get(&key) else {
            continue;
        };
        let (is_redirect, _) = parse_redirect(&page.content);
        let default_relative_path =
            namespace_mapper.title_to_relative_path(paths, &page.title, is_redirect);
        let (relative_path, existing_local) = existing_local_by_title
            .get(&key)
            .filter(|file| file.is_redirect == is_redirect)
            .map(|file| (file.relative_path.clone(), true))
            .unwrap_or((default_relative_path, false));
        candidates.push(PullPathCandidate {
            key,
            title: page.title.clone(),
            relative_path,
            existing_local,
        });
    }

    let mut groups = BTreeMap::<String, Vec<usize>>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        groups
            .entry(case_insensitive_path_key(&candidate.relative_path))
            .or_default()
            .push(index);
    }

    let mut out = BTreeMap::new();
    for group in groups.values() {
        let keep_index = group
            .iter()
            .copied()
            .find(|index| candidates[*index].existing_local)
            .unwrap_or(group[0]);

        for index in group {
            let candidate = &candidates[*index];
            let relative_path = if *index == keep_index {
                candidate.relative_path.clone()
            } else {
                case_safe_title_relative_path(&candidate.relative_path, &candidate.title)
            };
            out.insert(candidate.key.clone(), relative_path);
        }
    }

    out
}

#[derive(Debug)]
struct PullPathCandidate {
    key: String,
    title: String,
    relative_path: String,
    existing_local: bool,
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
    paths: &SyncProjectPaths,
    existing: &Option<SyncLedgerEntry>,
    target_relative_path: &str,
    protected_relative_path_keys: &BTreeSet<String>,
    overwrite_local: bool,
) -> Result<Option<PathBuf>> {
    let Some(existing) = existing else {
        return Ok(None);
    };
    if existing.relative_path == target_relative_path {
        return Ok(None);
    }

    let existing_path_key = case_insensitive_path_key(&existing.relative_path);
    let target_path_key = case_insensitive_path_key(target_relative_path);
    if existing_path_key == target_path_key {
        return Ok(None);
    }
    if protected_relative_path_keys.contains(&existing_path_key) {
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
    let old_hash = compute_wiki_sync_hash(&old_content);
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

fn case_insensitive_path_key(path: &str) -> String {
    normalize_path(path).to_ascii_lowercase()
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
