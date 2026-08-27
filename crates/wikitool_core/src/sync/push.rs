use super::*;

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
        let username = env::var("WIKITOOL_BOT_USER")
            .map_err(|_| anyhow::anyhow!("WIKITOOL_BOT_USER is required for push"))?;
        let password = env::var("WIKITOOL_BOT_PASS")
            .map_err(|_| anyhow::anyhow!("WIKITOOL_BOT_PASS is required for push"))?;
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

pub(super) fn push_to_remote_with_api<A: WikiWriteApi>(
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

    // Force may waive a pre-existing conflict after operator review, but it must
    // still bind the write to the exact remote revision observed in this run.
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

    let acceptance_errors = collect_article_acceptance_errors(paths, &context);
    if !acceptance_errors.is_empty() {
        for (title, detail) in &acceptance_errors {
            report.errors.push(format!(
                "{title}: exact-content acceptance ledger entry required: {detail}"
            ));
            report.pages.push(PushPageResult {
                title: title.clone(),
                action: "blocked_unaccepted_prose".to_string(),
                detail: Some(detail.clone()),
            });
        }
        if options.dry_run {
            for change in &context.changes {
                if acceptance_errors.contains_key(&change.title) {
                    continue;
                }
                report.pages.push(PushPageResult {
                    title: change.title.clone(),
                    action: push_dry_run_action(change).to_string(),
                    detail: None,
                });
            }
        }
        report.success = false;
        return Ok(report);
    }

    if options.dry_run {
        for change in &context.changes {
            if change.remote_conflict && !options.force {
                report.conflicts.push(change.title.clone());
                report.pages.push(PushPageResult {
                    title: change.title.clone(),
                    action: "conflict".to_string(),
                    detail: Some(remote_conflict_detail(change).to_string()),
                });
                continue;
            }

            report.pages.push(PushPageResult {
                title: change.title.clone(),
                action: push_dry_run_action(change).to_string(),
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
                detail: Some(remote_conflict_detail(change).to_string()),
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
                let content_result = if file.namespace == "Main" && !file.is_redirect {
                    crate::article_acceptance::load_accepted_article(
                        paths,
                        &absolute,
                        &file.title,
                        &file.relative_path,
                    )
                    .map(|accepted| accepted.content)
                } else {
                    fs::read_to_string(&absolute).with_context(|| {
                        format!("failed to read local content from {}", absolute.display())
                    })
                };
                let content = match content_result {
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

                let recreating_deleted_remote = change.change_type == DiffChangeType::ModifiedLocal
                    && change.remote_exists == Some(false);
                let constraint = match change.change_type {
                    DiffChangeType::NewLocal => EditConstraint::CreateOnly,
                    DiffChangeType::ModifiedLocal if recreating_deleted_remote => {
                        EditConstraint::CreateOnly
                    }
                    DiffChangeType::ModifiedLocal => {
                        let Some(revision_id) = change.remote_revision_id else {
                            report.errors.push(format!(
                                "{}: remote revision identity is missing; refusing unconstrained edit",
                                file.title
                            ));
                            report.pages.push(PushPageResult {
                                title: file.title.clone(),
                                action: "error".to_string(),
                                detail: Some("remote revision identity is missing".to_string()),
                            });
                            continue;
                        };
                        EditConstraint::ExistingRevision { revision_id }
                    }
                    DiffChangeType::DeletedLocal => unreachable!("delete handled separately"),
                };

                match api.edit_page(&file.title, &content, &options.summary, constraint) {
                    Ok(remote_page) => {
                        let (is_redirect, redirect_target) = parse_redirect(&remote_page.content);
                        let content_hash = compute_wiki_sync_hash(&remote_page.content);
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
                            DiffChangeType::ModifiedLocal if recreating_deleted_remote => {
                                report.created += 1;
                                report.pages.push(PushPageResult {
                                    title: file.title.clone(),
                                    action: "recreated".to_string(),
                                    detail: Some(
                                        "remote page was absent and was recreated with createonly"
                                            .to_string(),
                                    ),
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
            DiffChangeType::DeletedLocal => {
                let current_remote = match api
                    .get_page_timestamps(std::slice::from_ref(&change.title))
                {
                    Ok(pages) => pages
                        .into_iter()
                        .find(|page| normalized_title_key(&page.title) == key),
                    Err(error) => {
                        report.errors.push(format!("{}: {error}", change.title));
                        report.pages.push(PushPageResult {
                            title: change.title.clone(),
                            action: "error".to_string(),
                            detail: Some("failed immediate pre-delete revision check".to_string()),
                        });
                        continue;
                    }
                };

                let Some(current_remote) = current_remote else {
                    if let Err(error) = remove_local_sync_state(&context, &change.title) {
                        report.errors.push(format!("{}: {error}", change.title));
                        report.pages.push(PushPageResult {
                            title: change.title.clone(),
                            action: "error".to_string(),
                            detail: Some("failed to clean already-deleted sync state".to_string()),
                        });
                        continue;
                    }
                    report.unchanged += 1;
                    report.pages.push(PushPageResult {
                        title: change.title.clone(),
                        action: "already_deleted".to_string(),
                        detail: Some(
                            "remote page is already absent; no delete was sent".to_string(),
                        ),
                    });
                    continue;
                };

                let Some(observed_revision_id) = change.remote_revision_id else {
                    report.conflicts.push(change.title.clone());
                    report.pages.push(PushPageResult {
                        title: change.title.clone(),
                        action: "conflict".to_string(),
                        detail: Some(
                            "remote page appeared after planning; refusing delete".to_string(),
                        ),
                    });
                    continue;
                };
                if current_remote.revision_id != observed_revision_id {
                    report.conflicts.push(change.title.clone());
                    report.pages.push(PushPageResult {
                        title: change.title.clone(),
                        action: "conflict".to_string(),
                        detail: Some(format!(
                            "remote revision changed during push planning: observed {observed_revision_id}, current {}",
                            current_remote.revision_id
                        )),
                    });
                    continue;
                }

                match api.delete_page(
                    &change.title,
                    &format!("wikitool push delete: {}", options.summary),
                ) {
                    Ok(()) => {
                        if let Err(error) = remove_local_sync_state(&context, &change.title) {
                            report.errors.push(format!("{}: {error}", change.title));
                            report.pages.push(PushPageResult {
                                title: change.title.clone(),
                                action: "error".to_string(),
                                detail: Some("failed to update local sync state".to_string()),
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
                }
            }
        }
    }

    report.request_count = api.request_count();
    report.success = report.errors.is_empty() && report.conflicts.is_empty();
    Ok(report)
}

fn collect_article_acceptance_errors(
    paths: &ResolvedPaths,
    context: &SyncPlanningContext,
) -> BTreeMap<String, String> {
    let mut errors = BTreeMap::new();
    for change in &context.changes {
        if !matches!(
            change.change_type,
            DiffChangeType::NewLocal | DiffChangeType::ModifiedLocal
        ) {
            continue;
        }
        let Some(file) = context.local_map.get(&normalized_title_key(&change.title)) else {
            continue;
        };
        if file.namespace != "Main" || file.is_redirect {
            continue;
        }
        let absolute = absolute_path_from_relative(paths, &file.relative_path);
        if let Err(error) =
            verify_article_acceptance(paths, &absolute, &file.title, &file.relative_path)
        {
            errors.insert(file.title.clone(), format!("{error:#}"));
        }
    }
    errors
}

fn remove_local_sync_state(context: &SyncPlanningContext, title: &str) -> Result<()> {
    remove_sync_ledger_entry(&context.connection, title)?;
    remove_sync_snapshot(&context.connection, title)?;
    Ok(())
}

fn push_dry_run_action(change: &PlannedSyncChangeInternal) -> &'static str {
    match change.change_type {
        DiffChangeType::NewLocal => "would_create",
        DiffChangeType::ModifiedLocal if change.remote_exists == Some(false) => "would_recreate",
        DiffChangeType::ModifiedLocal => "would_update",
        DiffChangeType::DeletedLocal if change.remote_exists == Some(false) => "already_deleted",
        DiffChangeType::DeletedLocal => "would_delete",
    }
}

fn remote_conflict_detail(change: &PlannedSyncChangeInternal) -> &'static str {
    match change.change_type {
        DiffChangeType::ModifiedLocal if change.remote_exists == Some(false) => {
            "remote page was deleted since last sync; use --force to recreate with createonly"
        }
        DiffChangeType::NewLocal if change.remote_exists == Some(true) => {
            "remote page now exists; createonly prevents overwriting it"
        }
        _ => "remote page changed since last sync",
    }
}
