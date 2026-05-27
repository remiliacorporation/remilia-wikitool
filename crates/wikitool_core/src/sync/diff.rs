use super::*;

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
