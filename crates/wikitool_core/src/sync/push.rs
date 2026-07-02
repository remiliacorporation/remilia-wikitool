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

    if options.force {
        context.request_count = api.request_count();
    } else {
        hydrate_remote_conflicts(&mut context, api)?;
    }

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

fn push_dry_run_action(change_type: &DiffChangeType) -> &'static str {
    match change_type {
        DiffChangeType::NewLocal => "would_create",
        DiffChangeType::ModifiedLocal => "would_update",
        DiffChangeType::DeletedLocal => "would_delete",
    }
}
