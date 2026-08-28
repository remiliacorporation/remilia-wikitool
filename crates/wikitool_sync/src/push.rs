use super::*;
use crate::storage::{
    DeleteMutationIntent, EditMutationIntent, NewDeleteMutation, NewEditMutation,
    advance_verified_delete_mutation, advance_verified_edit_mutation, begin_delete_mutation,
    begin_edit_mutation, bind_delete_response, bind_edit_response,
    complete_delete_mutation_without_state_change, complete_edit_mutation_without_state_change,
    ensure_no_unresolved_remote_mutation, mark_delete_mutation_unresolved,
    mark_delete_request_started, mark_edit_mutation_unresolved, mark_edit_request_started,
};

#[derive(Serialize)]
struct CanonicalPushPlan<'a> {
    schema: &'static str,
    target_api_url: &'a str,
    summary: &'a str,
    force: bool,
    delete: bool,
    include_templates: bool,
    categories_only: bool,
    all: bool,
    selected_titles: Vec<String>,
    selected_paths: Vec<String>,
    changes: Vec<CanonicalPushChange>,
}

#[derive(Serialize)]
struct CanonicalPushChange {
    title: String,
    change_type: &'static str,
    relative_path: String,
    local_hash: Option<String>,
    synced_hash: Option<String>,
    synced_wiki_timestamp: Option<String>,
    remote_conflict: bool,
    remote_exists: Option<bool>,
    remote_revision_id: Option<i64>,
    remote_wiki_timestamp: Option<String>,
    prepared_content_sha256: Option<String>,
    publication_provenance_sha256: Option<String>,
}

pub fn push_to_remote_with_api_and_preflight<A: WikiWriteApi>(
    paths: &SyncProjectPaths,
    options: &PushOptions,
    target: &SyncTargetIdentity,
    api: &mut A,
    credentials: Option<(&str, &str)>,
    preflight: &dyn PublicationPreflight,
) -> Result<PushReport> {
    if options.summary.trim().is_empty() {
        bail!("push requires a non-empty summary");
    }
    validate_push_intent(options)?;
    target.ensure_matches_api(api.target_api_url())?;

    let Some(mut context) = collect_sync_planning_context(
        paths,
        &SyncPlanOptions {
            include_templates: options.include_templates,
            categories_only: options.categories_only,
            include_deletes: options.delete,
            include_remote_conflicts: true,
            selection: options.selection.clone(),
        },
        target,
    )?
    else {
        let plan_id = canonical_push_plan_id(target, options, &[], &BTreeMap::new())?;
        if let Some(expected) = options.apply_plan_id.as_deref()
            && expected != plan_id
        {
            bail!(
                "push plan mismatch: requested {expected}, current plan is {plan_id}; preview again before applying"
            );
        }
        return Ok(PushReport {
            success: true,
            dry_run: options.dry_run,
            target_api_url: target.api_url().to_string(),
            plan_id: Some(plan_id),
            pushed: 0,
            created: 0,
            updated: 0,
            deleted: 0,
            unchanged: 0,
            conflicts: Vec::new(),
            errors: Vec::new(),
            pages: Vec::new(),
            mutation_effects: Vec::new(),
            request_count: 0,
        });
    };

    // Force may waive a pre-existing conflict after operator review, but it must
    // still bind the write to the exact remote revision observed in this run.
    hydrate_remote_conflicts(&mut context, api)?;

    let mut report = PushReport {
        success: true,
        dry_run: options.dry_run,
        target_api_url: target.api_url().to_string(),
        plan_id: None,
        pushed: 0,
        created: 0,
        updated: 0,
        deleted: 0,
        unchanged: 0,
        conflicts: Vec::new(),
        errors: Vec::new(),
        pages: Vec::new(),
        mutation_effects: Vec::new(),
        request_count: context.request_count,
    };

    if context.changes.is_empty() {
        let plan_id = canonical_push_plan_id(target, options, &[], &BTreeMap::new())?;
        if let Some(expected) = options.apply_plan_id.as_deref()
            && expected != plan_id
        {
            report.success = false;
            report.errors.push(format!(
                "push plan mismatch: requested {expected}, current plan is {plan_id}; preview again before applying"
            ));
        }
        report.plan_id = Some(plan_id);
        return Ok(report);
    }

    let (prepared_publications, preflight_errors) =
        collect_prepared_publications(paths, &context, preflight);
    if !preflight_errors.is_empty() {
        for (title, detail) in &preflight_errors {
            report
                .errors
                .push(format!("{title}: publication preflight failed: {detail}"));
            report.pages.push(PushPageResult::new(
                title,
                "blocked_publication_preflight",
                Some(detail.clone()),
            ));
        }
        if options.dry_run {
            for change in &context.changes {
                if preflight_errors.contains_key(&change.title) {
                    continue;
                }
                report.pages.push(
                    PushPageResult::new(&change.title, push_dry_run_action(change), None)
                        .with_acceptance(publication_provenance_for(
                            &prepared_publications,
                            &change.title,
                        )),
                );
            }
        }
        report.success = false;
        return Ok(report);
    }

    let plan_id =
        canonical_push_plan_id(target, options, &context.changes, &prepared_publications)?;
    report.plan_id = Some(plan_id.clone());

    if let Some(expected) = options.apply_plan_id.as_deref()
        && expected != plan_id
    {
        report.success = false;
        report.errors.push(format!(
            "push plan mismatch: requested {expected}, current plan is {plan_id}; preview again before applying"
        ));
        return Ok(report);
    }

    if options.dry_run {
        for change in &context.changes {
            if change.remote_conflict && !options.force {
                report.conflicts.push(change.title.clone());
                report.pages.push(
                    PushPageResult::new(
                        &change.title,
                        "conflict",
                        Some(remote_conflict_detail(change).to_string()),
                    )
                    .with_acceptance(publication_provenance_for(
                        &prepared_publications,
                        &change.title,
                    )),
                );
                continue;
            }

            report.pages.push(
                PushPageResult::new(&change.title, push_dry_run_action(change), None)
                    .with_acceptance(publication_provenance_for(
                        &prepared_publications,
                        &change.title,
                    )),
            );
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
            report.pages.push(
                PushPageResult::new(
                    &change.title,
                    "conflict",
                    Some(remote_conflict_detail(change).to_string()),
                )
                .with_acceptance(publication_provenance_for(
                    &prepared_publications,
                    &change.title,
                )),
            );
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
                        report.pages.push(PushPageResult::new(
                            &change.title,
                            "error",
                            Some("local file missing".to_string()),
                        ));
                        continue;
                    }
                };
                let Some(prepared) = prepared_publications.get(&key) else {
                    report.errors.push(format!(
                        "{}: publication preflight produced no prepared content",
                        file.title
                    ));
                    report.pages.push(PushPageResult::new(
                        &file.title,
                        "error",
                        Some("publication preflight result missing".to_string()),
                    ));
                    continue;
                };
                let content = &prepared.content;
                let current_acceptance = prepared.provenance.clone();

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
                            report.pages.push(
                                PushPageResult::new(
                                    &file.title,
                                    "error",
                                    Some("remote revision identity is missing".to_string()),
                                )
                                .with_acceptance(current_acceptance.clone()),
                            );
                            continue;
                        };
                        EditConstraint::ExistingRevision { revision_id }
                    }
                    DiffChangeType::DeletedLocal => unreachable!("delete handled separately"),
                };

                let intent = match begin_edit_mutation(
                    paths,
                    &context.connection,
                    target,
                    NewEditMutation {
                        title: &file.title,
                        namespace: &file.namespace,
                        namespace_id: match namespace_name_to_id(&file.namespace).or_else(|| {
                            paths
                                .custom_namespaces
                                .iter()
                                .find(|namespace| namespace.name == file.namespace)
                                .map(|namespace| namespace.id)
                        }) {
                            Some(namespace_id) => namespace_id,
                            None => {
                                report.errors.push(format!(
                                    "{}: namespace {:?} has no numeric MediaWiki identity",
                                    file.title, file.namespace
                                ));
                                report.pages.push(PushPageResult::new(
                                    &file.title,
                                    "error",
                                    Some("numeric namespace identity is missing".to_string()),
                                ));
                                continue;
                            }
                        },
                        relative_path: &file.relative_path,
                        content,
                        summary: &options.summary,
                        constraint,
                    },
                ) {
                    Ok(intent) => intent,
                    Err(error) => {
                        report.errors.push(format!("{}: {error}", file.title));
                        report.pages.push(
                            PushPageResult::new(
                                &file.title,
                                "blocked_unresolved_mutation",
                                Some(error.to_string()),
                            )
                            .with_acceptance(current_acceptance),
                        );
                        continue;
                    }
                };

                if let Err(error) = mark_edit_request_started(&context.connection, &intent) {
                    let initial_detail = format!(
                        "edit request-start persistence failed before any MediaWiki write was sent: {error:#}"
                    );
                    let (kind, action, detail) = match complete_edit_mutation_without_state_change(
                        paths,
                        &context.connection,
                        target,
                        &intent,
                        "not_applied",
                        &initial_detail,
                    ) {
                        Ok(()) => (
                            RemoteMutationEffectKind::NotApplied,
                            "not_applied",
                            initial_detail,
                        ),
                        Err(persistence_error) => (
                            RemoteMutationEffectKind::ReconciliationRequired,
                            "reconciliation_required",
                            format!(
                                "{initial_detail}; additionally failed to terminalize the durable intent as not applied: {persistence_error:#}"
                            ),
                        ),
                    };
                    report.errors.push(format!("{}: {detail}", file.title));
                    report.mutation_effects.push(remote_mutation_effect(
                        &intent,
                        kind,
                        None,
                        Some(detail.clone()),
                    ));
                    report.pages.push(
                        PushPageResult::new(&file.title, action, Some(detail))
                            .with_acceptance(current_acceptance),
                    );
                    continue;
                }

                let response = match api.edit_page(
                    &file.title,
                    content,
                    &intent.request_summary(),
                    constraint,
                ) {
                    Ok(response) => response,
                    Err(error) => {
                        let detail = format!(
                            "remote edit outcome is ambiguous after the write request: {error:#}"
                        );
                        let persistence_error = mark_edit_mutation_unresolved(
                            &context.connection,
                            &intent,
                            "outcome_ambiguous",
                            &detail,
                        )
                        .err();
                        let detail = append_receipt_persistence_error(detail, persistence_error);
                        report.errors.push(format!("{}: {detail}", file.title));
                        report.mutation_effects.push(remote_mutation_effect(
                            &intent,
                            RemoteMutationEffectKind::OutcomeAmbiguous,
                            None,
                            Some(detail.clone()),
                        ));
                        report.pages.push(
                            PushPageResult::new(&file.title, "outcome_ambiguous", Some(detail))
                                .with_acceptance(current_acceptance),
                        );
                        continue;
                    }
                };

                if let Err(error) = bind_edit_response(&context.connection, &intent, &response) {
                    let detail = format!(
                        "remote edit succeeded but its response could not be bound durably: {error:#}"
                    );
                    let persistence_error = mark_edit_mutation_unresolved(
                        &context.connection,
                        &intent,
                        "reconciliation_required",
                        &detail,
                    )
                    .err();
                    let detail = append_receipt_persistence_error(detail, persistence_error);
                    report.errors.push(format!("{}: {detail}", file.title));
                    report.mutation_effects.push(remote_mutation_effect(
                        &intent,
                        RemoteMutationEffectKind::ReconciliationRequired,
                        Some(&response),
                        Some(detail.clone()),
                    ));
                    report.pages.push(
                        PushPageResult::new(&file.title, "reconciliation_required", Some(detail))
                            .with_acceptance(current_acceptance),
                    );
                    continue;
                }

                let remote_page = match api.get_revision_by_id(response.new_revision_id) {
                    Ok(Some(page)) => page,
                    Ok(None) => {
                        let detail = format!(
                            "exact revision {} was not returned after the edit",
                            response.new_revision_id
                        );
                        record_reconciliation_failure(
                            &mut report,
                            &context.connection,
                            &intent,
                            &response,
                            current_acceptance,
                            detail,
                        );
                        continue;
                    }
                    Err(error) => {
                        let detail = format!(
                            "failed to fetch exact revision {} after the edit: {error:#}",
                            response.new_revision_id
                        );
                        record_reconciliation_failure(
                            &mut report,
                            &context.connection,
                            &intent,
                            &response,
                            current_acceptance,
                            detail,
                        );
                        continue;
                    }
                };

                if let Err(error) = verify_exact_edit_revision(&intent, &response, &remote_page) {
                    record_reconciliation_failure(
                        &mut report,
                        &context.connection,
                        &intent,
                        &response,
                        current_acceptance,
                        format!("exact revision verification failed: {error:#}"),
                    );
                    continue;
                }

                let current_remote = match crate::remote::observe_remote_page(api, &intent.title) {
                    Ok(page) => page,
                    Err(error) => {
                        record_reconciliation_failure(
                            &mut report,
                            &context.connection,
                            &intent,
                            &response,
                            current_acceptance,
                            format!(
                                "exact response revision was verified, but current-title observation failed: {error:#}"
                            ),
                        );
                        continue;
                    }
                };
                if current_remote
                    .as_ref()
                    .is_none_or(|page| page.revision_id != response.new_revision_id)
                {
                    let detail = match current_remote {
                        Some(ref page) => format!(
                            "response revision {} proves this edit applied, but current revision {} superseded it; prior sync authority was retained",
                            response.new_revision_id, page.revision_id
                        ),
                        None => format!(
                            "response revision {} proves this edit applied, but the page is now absent; prior sync authority was retained",
                            response.new_revision_id
                        ),
                    };
                    if let Err(error) = complete_edit_mutation_without_state_change(
                        paths,
                        &context.connection,
                        target,
                        &intent,
                        "applied_then_changed",
                        &detail,
                    ) {
                        record_reconciliation_failure(
                            &mut report,
                            &context.connection,
                            &intent,
                            &response,
                            current_acceptance,
                            format!("{detail}; failed to persist terminal outcome: {error:#}"),
                        );
                        continue;
                    }
                    report.conflicts.push(intent.title.clone());
                    report.mutation_effects.push(remote_mutation_effect(
                        &intent,
                        RemoteMutationEffectKind::AppliedThenChanged,
                        Some(&response),
                        Some(detail.clone()),
                    ));
                    report.pages.push(
                        PushPageResult::new(&intent.title, "applied_then_changed", Some(detail))
                            .with_acceptance(current_acceptance),
                    );
                    continue;
                }

                if let Err(error) = advance_verified_edit_mutation(
                    paths,
                    &context.connection,
                    target,
                    &intent,
                    &remote_page,
                ) {
                    record_reconciliation_failure(
                        &mut report,
                        &context.connection,
                        &intent,
                        &response,
                        current_acceptance,
                        format!("verified revision could not advance local sync state: {error:#}"),
                    );
                    continue;
                }

                report.mutation_effects.push(remote_mutation_effect(
                    &intent,
                    RemoteMutationEffectKind::StateAdvanced,
                    Some(&response),
                    None,
                ));
                report.pushed += 1;
                match change.change_type {
                    DiffChangeType::NewLocal => {
                        report.created += 1;
                        report.pages.push(
                            PushPageResult::new(&file.title, "created", None)
                                .with_acceptance(current_acceptance.clone()),
                        );
                    }
                    DiffChangeType::ModifiedLocal if recreating_deleted_remote => {
                        report.created += 1;
                        report.pages.push(
                            PushPageResult::new(
                                &file.title,
                                "recreated",
                                Some(
                                    "remote page was absent and was recreated with createonly"
                                        .to_string(),
                                ),
                            )
                            .with_acceptance(current_acceptance.clone()),
                        );
                    }
                    DiffChangeType::ModifiedLocal => {
                        report.updated += 1;
                        report.pages.push(
                            PushPageResult::new(&file.title, "updated", None)
                                .with_acceptance(current_acceptance.clone()),
                        );
                    }
                    DiffChangeType::DeletedLocal => {}
                }
            }
            DiffChangeType::DeletedLocal => {
                if let Err(error) =
                    ensure_no_unresolved_remote_mutation(&context.connection, &change.title)
                {
                    report.errors.push(format!("{}: {error}", change.title));
                    report.pages.push(PushPageResult::new(
                        &change.title,
                        "reconciliation_required",
                        Some(error.to_string()),
                    ));
                    continue;
                }
                let current_remote =
                    match api.get_page_timestamps(std::slice::from_ref(&change.title)) {
                        Ok(pages) => pages
                            .into_iter()
                            .find(|page| normalized_title_key(&page.title) == key),
                        Err(error) => {
                            report.errors.push(format!("{}: {error}", change.title));
                            report.pages.push(PushPageResult::new(
                                &change.title,
                                "error",
                                Some("failed immediate pre-delete revision check".to_string()),
                            ));
                            continue;
                        }
                    };

                let Some(current_remote) = current_remote else {
                    if let Err(error) = remove_local_sync_state(&context, &change.title) {
                        report.errors.push(format!("{}: {error}", change.title));
                        report.pages.push(PushPageResult::new(
                            &change.title,
                            "error",
                            Some("failed to clean already-deleted sync state".to_string()),
                        ));
                        continue;
                    }
                    report.unchanged += 1;
                    report.pages.push(PushPageResult::new(
                        &change.title,
                        "already_deleted",
                        Some("remote page is already absent; no delete was sent".to_string()),
                    ));
                    continue;
                };

                let Some(observed_revision_id) = change.remote_revision_id else {
                    report.conflicts.push(change.title.clone());
                    report.pages.push(PushPageResult::new(
                        &change.title,
                        "conflict",
                        Some("remote page appeared after planning; refusing delete".to_string()),
                    ));
                    continue;
                };
                if current_remote.revision_id != observed_revision_id {
                    report.conflicts.push(change.title.clone());
                    report.pages.push(PushPageResult::new(
                        &change.title,
                        "conflict",
                        Some(format!(
                            "remote revision changed during push planning: observed {observed_revision_id}, current {}",
                            current_remote.revision_id
                        )),
                    ));
                    continue;
                }

                let intent = match begin_delete_mutation(
                    paths,
                    &context.connection,
                    target,
                    NewDeleteMutation {
                        title: &current_remote.title,
                        relative_path: Some(&change.relative_path),
                        expected_revision_id: current_remote.revision_id,
                        reason: &format!("wikitool push delete: {}", options.summary),
                        local_effect_policy: &RemoteDeleteLocalEffectPolicy {
                            backup_enabled: false,
                            backup_directory: None,
                            backup_path: None,
                            local_content_sha256: None,
                        },
                    },
                ) {
                    Ok(intent) => intent,
                    Err(error) => {
                        report.errors.push(format!("{}: {error}", change.title));
                        report.pages.push(PushPageResult::new(
                            &change.title,
                            "error",
                            Some("failed to persist durable delete intent".to_string()),
                        ));
                        continue;
                    }
                };

                if let Err(error) = mark_delete_request_started(&context.connection, &intent) {
                    let initial_detail = format!(
                        "delete request-start persistence failed before any MediaWiki delete was sent: {error:#}"
                    );
                    let (kind, action, detail) = match complete_delete_mutation_without_state_change(
                        paths,
                        &context.connection,
                        target,
                        &intent,
                        "not_applied",
                        &initial_detail,
                    ) {
                        Ok(()) => (
                            RemoteMutationEffectKind::NotApplied,
                            "not_applied",
                            initial_detail,
                        ),
                        Err(persistence_error) => (
                            RemoteMutationEffectKind::ReconciliationRequired,
                            "reconciliation_required",
                            format!(
                                "{initial_detail}; additionally failed to terminalize the durable intent as not applied: {persistence_error:#}"
                            ),
                        ),
                    };
                    report.errors.push(format!("{}: {detail}", change.title));
                    report.mutation_effects.push(delete_mutation_effect(
                        &intent,
                        kind,
                        None,
                        Some(intent.expected_revision_id),
                        Some(detail.clone()),
                    ));
                    report.pages.push(PushPageResult::new(
                        &current_remote.title,
                        action,
                        Some(detail),
                    ));
                    continue;
                }

                match api.delete_page(&current_remote.title, &intent.request_reason()) {
                    Ok(DeleteOutcome::Deleted(receipt)) => {
                        if normalized_title_key(&receipt.title)
                            != normalized_title_key(&current_remote.title)
                        {
                            record_delete_reconciliation_failure(
                                &mut report,
                                &context.connection,
                                &intent,
                                Some(&receipt),
                                format!(
                                    "delete response title {:?} does not match canonical observed title {:?}; local authority was preserved",
                                    receipt.title, current_remote.title
                                ),
                            );
                            continue;
                        }
                        if let Err(error) = bind_delete_response(
                            &context.connection,
                            &intent,
                            "deleted",
                            &receipt.title,
                            Some(receipt.log_id),
                            None,
                        ) {
                            record_delete_reconciliation_failure(
                                &mut report,
                                &context.connection,
                                &intent,
                                Some(&receipt),
                                format!("failed to durably bind delete response: {error:#}"),
                            );
                            continue;
                        }
                        let post_delete = match crate::remote::observe_remote_page(
                            api,
                            &receipt.title,
                        ) {
                            Ok(page) => page,
                            Err(error) => {
                                record_delete_reconciliation_failure(
                                    &mut report,
                                    &context.connection,
                                    &intent,
                                    Some(&receipt),
                                    format!(
                                        "typed delete response could not be followed by an exact absence check: {error:#}"
                                    ),
                                );
                                continue;
                            }
                        };
                        if let Some(recreated) = post_delete {
                            if recreated.revision_id == current_remote.revision_id {
                                record_delete_reconciliation_failure(
                                    &mut report,
                                    &context.connection,
                                    &intent,
                                    Some(&receipt),
                                    format!(
                                        "MediaWiki deletion log id {} was returned, but expected revision {} is still visible; replica lag prevents proving current absence",
                                        receipt.log_id, current_remote.revision_id
                                    ),
                                );
                                continue;
                            }
                            let detail = format!(
                                "MediaWiki deletion log id {} proves the delete, but revision {} is present immediately afterward; local sync authority was retained",
                                receipt.log_id, recreated.revision_id
                            );
                            if let Err(error) = complete_delete_mutation_without_state_change(
                                paths,
                                &context.connection,
                                target,
                                &intent,
                                "applied_then_recreated",
                                &detail,
                            ) {
                                record_delete_reconciliation_failure(
                                    &mut report,
                                    &context.connection,
                                    &intent,
                                    Some(&receipt),
                                    format!(
                                        "{detail}; failed to persist terminal outcome: {error:#}"
                                    ),
                                );
                                continue;
                            }
                            report.conflicts.push(change.title.clone());
                            report.mutation_effects.push(delete_mutation_effect(
                                &intent,
                                RemoteMutationEffectKind::AppliedThenRecreated,
                                Some(&receipt),
                                Some(recreated.revision_id),
                                Some(detail.clone()),
                            ));
                            report.pages.push(PushPageResult::new(
                                &recreated.title,
                                "applied_then_recreated",
                                Some(detail),
                            ));
                            continue;
                        }
                        if let Err(error) = advance_verified_delete_mutation(
                            paths,
                            &context.connection,
                            target,
                            &intent,
                        ) {
                            record_delete_reconciliation_failure(
                                &mut report,
                                &context.connection,
                                &intent,
                                Some(&receipt),
                                format!(
                                    "failed to atomically advance verified delete state: {error:#}"
                                ),
                            );
                            continue;
                        }
                        report.pushed += 1;
                        report.deleted += 1;
                        report.mutation_effects.push(delete_mutation_effect(
                            &intent,
                            RemoteMutationEffectKind::StateAdvanced,
                            Some(&receipt),
                            Some(current_remote.revision_id),
                            None,
                        ));
                        report.pages.push(PushPageResult::new(
                            &receipt.title,
                            "deleted",
                            Some(format!("MediaWiki deletion log id {}", receipt.log_id)),
                        ));
                    }
                    Ok(DeleteOutcome::AlreadyMissing) => {
                        if let Err(error) = bind_delete_response(
                            &context.connection,
                            &intent,
                            "already_missing",
                            &current_remote.title,
                            None,
                            None,
                        ) {
                            record_delete_reconciliation_failure(
                                &mut report,
                                &context.connection,
                                &intent,
                                None,
                                format!(
                                    "failed to durably bind verified remote absence: {error:#}"
                                ),
                            );
                            continue;
                        }
                        let post_delete = match crate::remote::observe_remote_page(
                            api,
                            &current_remote.title,
                        ) {
                            Ok(page) => page,
                            Err(error) => {
                                record_delete_reconciliation_failure(
                                    &mut report,
                                    &context.connection,
                                    &intent,
                                    None,
                                    format!(
                                        "missingtitle response could not be followed by an exact absence check: {error:#}"
                                    ),
                                );
                                continue;
                            }
                        };
                        if let Some(recreated) = post_delete {
                            let detail = format!(
                                "MediaWiki reported missingtitle, but revision {} is present immediately afterward; local sync authority was retained",
                                recreated.revision_id
                            );
                            if let Err(error) = complete_delete_mutation_without_state_change(
                                paths,
                                &context.connection,
                                target,
                                &intent,
                                "remote_present_after_missing",
                                &detail,
                            ) {
                                record_delete_reconciliation_failure(
                                    &mut report,
                                    &context.connection,
                                    &intent,
                                    None,
                                    format!(
                                        "{detail}; failed to persist terminal outcome: {error:#}"
                                    ),
                                );
                                continue;
                            }
                            report.conflicts.push(change.title.clone());
                            report.mutation_effects.push(delete_mutation_effect(
                                &intent,
                                RemoteMutationEffectKind::RemotePresentAfterMissing,
                                None,
                                Some(recreated.revision_id),
                                Some(detail.clone()),
                            ));
                            report.pages.push(PushPageResult::new(
                                &recreated.title,
                                "remote_present_after_missing",
                                Some(detail),
                            ));
                            continue;
                        }
                        if let Err(error) = advance_verified_delete_mutation(
                            paths,
                            &context.connection,
                            target,
                            &intent,
                        ) {
                            record_delete_reconciliation_failure(
                                &mut report,
                                &context.connection,
                                &intent,
                                None,
                                format!(
                                    "failed to atomically reconcile verified remote absence: {error:#}"
                                ),
                            );
                            continue;
                        }
                        report.unchanged += 1;
                        report.mutation_effects.push(delete_mutation_effect(
                            &intent,
                            RemoteMutationEffectKind::StateAdvanced,
                            None,
                            Some(current_remote.revision_id),
                            Some(
                                "MediaWiki reported missingtitle during the delete request"
                                    .to_string(),
                            ),
                        ));
                        report.pages.push(PushPageResult::new(
                            &current_remote.title,
                            "already_deleted",
                            Some(
                                "MediaWiki reported missingtitle during the delete request"
                                    .to_string(),
                            ),
                        ));
                    }
                    Err(error) => {
                        let detail = mark_delete_failure(
                            &context.connection,
                            &intent,
                            "outcome_ambiguous",
                            format!(
                                "delete request failed after durable intent: {error:#}; the request will not be retried"
                            ),
                        );
                        report.errors.push(format!("{}: {detail}", change.title));
                        report.mutation_effects.push(delete_mutation_effect(
                            &intent,
                            RemoteMutationEffectKind::OutcomeAmbiguous,
                            None,
                            Some(current_remote.revision_id),
                            Some(detail.clone()),
                        ));
                        report.pages.push(PushPageResult::new(
                            &current_remote.title,
                            "outcome_ambiguous",
                            Some(detail),
                        ));
                    }
                }
            }
        }
    }

    report.request_count = api.request_count();
    report.success = report.errors.is_empty() && report.conflicts.is_empty();
    Ok(report)
}

fn validate_push_intent(options: &PushOptions) -> Result<()> {
    let has_selection = !options.selection.titles.is_empty() || !options.selection.paths.is_empty();
    match (options.all, has_selection) {
        (true, true) => {
            bail!("push scope must use either explicit all or title/path selection, not both")
        }
        (false, false) => bail!("push requires explicit scope: all or at least one title/path"),
        _ => {}
    }
    match (options.dry_run, options.apply_plan_id.as_deref()) {
        (true, Some(_)) => bail!("push preview must not include an apply plan ID"),
        (false, Some(plan_id)) if plan_id.trim().is_empty() => {
            bail!("push apply plan ID must be non-empty")
        }
        (false, None) => bail!("live push requires the plan ID returned by a current preview"),
        _ => Ok(()),
    }
}

fn canonical_push_plan_id(
    target: &SyncTargetIdentity,
    options: &PushOptions,
    changes: &[PlannedSyncChangeInternal],
    prepared: &BTreeMap<String, PreparedPublication>,
) -> Result<String> {
    let mut selected_titles = options.selection.titles.clone();
    selected_titles.sort();
    selected_titles.dedup();
    let mut selected_paths = options.selection.paths.clone();
    selected_paths.sort();
    selected_paths.dedup();

    let mut canonical_changes = Vec::with_capacity(changes.len());
    for change in changes {
        let prepared = prepared.get(&normalized_title_key(&change.title));
        let publication_provenance_sha256 = prepared
            .and_then(|item| item.provenance.as_ref())
            .map(serde_json::to_string)
            .transpose()
            .context("failed to encode publication provenance for push plan")?
            .map(|encoded| compute_sha256(&encoded));
        canonical_changes.push(CanonicalPushChange {
            title: change.title.clone(),
            change_type: match change.change_type {
                DiffChangeType::NewLocal => "new_local",
                DiffChangeType::ModifiedLocal => "modified_local",
                DiffChangeType::DeletedLocal => "deleted_local",
            },
            relative_path: change.relative_path.clone(),
            local_hash: change.local_hash.clone(),
            synced_hash: change.synced_hash.clone(),
            synced_wiki_timestamp: change.synced_wiki_timestamp.clone(),
            remote_conflict: change.remote_conflict,
            remote_exists: change.remote_exists,
            remote_revision_id: change.remote_revision_id,
            remote_wiki_timestamp: change.remote_wiki_timestamp.clone(),
            prepared_content_sha256: prepared.map(|item| compute_sha256(&item.content)),
            publication_provenance_sha256,
        });
    }
    canonical_changes.sort_by(|left, right| {
        normalized_title_key(&left.title)
            .cmp(&normalized_title_key(&right.title))
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });

    let encoded = serde_json::to_string(&CanonicalPushPlan {
        schema: "wikitool_push_plan_v1",
        target_api_url: target.api_url(),
        summary: options.summary.trim(),
        force: options.force,
        delete: options.delete,
        include_templates: options.include_templates,
        categories_only: options.categories_only,
        all: options.all,
        selected_titles,
        selected_paths,
        changes: canonical_changes,
    })
    .context("failed to encode canonical push plan")?;
    Ok(compute_sha256(&encoded))
}

fn collect_prepared_publications(
    paths: &SyncProjectPaths,
    context: &SyncPlanningContext,
    preflight: &dyn PublicationPreflight,
) -> (
    BTreeMap<String, PreparedPublication>,
    BTreeMap<String, String>,
) {
    let mut prepared = BTreeMap::new();
    let mut errors = BTreeMap::new();
    for change in &context.changes {
        if !matches!(
            change.change_type,
            DiffChangeType::NewLocal | DiffChangeType::ModifiedLocal
        ) {
            continue;
        }
        let key = normalized_title_key(&change.title);
        let Some(file) = context.local_map.get(&key) else {
            errors.insert(change.title.clone(), "local file is missing".to_string());
            continue;
        };
        let absolute = absolute_path_from_relative(paths, &file.relative_path);
        let candidate = PublicationCandidate {
            title: &file.title,
            namespace: &file.namespace,
            is_redirect: file.is_redirect,
            relative_path: &file.relative_path,
            absolute_path: &absolute,
        };
        match preflight.prepare(candidate) {
            Ok(item) => {
                prepared.insert(key, item);
            }
            Err(error) => {
                errors.insert(file.title.clone(), format!("{error:#}"));
            }
        }
    }
    (prepared, errors)
}

fn publication_provenance_for(
    prepared: &BTreeMap<String, PreparedPublication>,
    title: &str,
) -> Option<PublicationProvenance> {
    prepared
        .get(&normalized_title_key(title))
        .and_then(|item| item.provenance.clone())
}

fn verify_exact_edit_revision(
    intent: &EditMutationIntent,
    response: &EditReceipt,
    page: &RemotePage,
) -> Result<()> {
    if response.new_revision_id <= 0 {
        bail!(
            "edit response returned invalid new revision ID {}",
            response.new_revision_id
        );
    }
    if normalized_title_key(&response.title) != normalized_title_key(&intent.title) {
        bail!(
            "edit response title {:?} does not match intended title {:?}",
            response.title,
            intent.title
        );
    }
    if let EditConstraint::ExistingRevision { revision_id } = intent.constraint
        && response.old_revision_id != revision_id
    {
        bail!(
            "edit response old revision {} does not match constrained revision {revision_id}",
            response.old_revision_id
        );
    }
    if page.revision_id != response.new_revision_id {
        bail!(
            "exact-revision query returned revision {} instead of {}",
            page.revision_id,
            response.new_revision_id
        );
    }
    if page.page_id != response.page_id {
        bail!(
            "exact revision returned page ID {} instead of {}",
            page.page_id,
            response.page_id
        );
    }
    if normalized_title_key(&page.title) != normalized_title_key(&intent.title) {
        bail!(
            "exact revision title {:?} does not match intended title {:?}",
            page.title,
            intent.title
        );
    }
    if page.timestamp != response.new_timestamp {
        bail!(
            "exact revision timestamp {:?} does not match edit response {:?}",
            page.timestamp,
            response.new_timestamp
        );
    }
    let returned_normalized_sha256 = compute_sha256(&normalize_wiki_content(&page.content));
    if returned_normalized_sha256 != intent.intended_normalized_sha256 {
        bail!(
            "exact revision content hash {returned_normalized_sha256} does not match intended normalized content hash {}",
            intent.intended_normalized_sha256
        );
    }
    Ok(())
}

fn record_reconciliation_failure(
    report: &mut PushReport,
    connection: &Connection,
    intent: &EditMutationIntent,
    response: &EditReceipt,
    acceptance: Option<PublicationProvenance>,
    detail: String,
) {
    let persistence_error =
        mark_edit_mutation_unresolved(connection, intent, "reconciliation_required", &detail).err();
    let detail = append_receipt_persistence_error(detail, persistence_error);
    report.errors.push(format!("{}: {detail}", intent.title));
    report.mutation_effects.push(remote_mutation_effect(
        intent,
        RemoteMutationEffectKind::ReconciliationRequired,
        Some(response),
        Some(detail.clone()),
    ));
    report.pages.push(
        PushPageResult::new(&intent.title, "reconciliation_required", Some(detail))
            .with_acceptance(acceptance),
    );
}

fn record_delete_reconciliation_failure(
    report: &mut PushReport,
    connection: &Connection,
    intent: &DeleteMutationIntent,
    response: Option<&DeleteReceipt>,
    detail: String,
) {
    let detail = mark_delete_failure(connection, intent, "reconciliation_required", detail);
    report.errors.push(format!("{}: {detail}", intent.title));
    report.mutation_effects.push(delete_mutation_effect(
        intent,
        RemoteMutationEffectKind::ReconciliationRequired,
        response,
        Some(intent.expected_revision_id),
        Some(detail.clone()),
    ));
    report.pages.push(PushPageResult::new(
        &intent.title,
        "reconciliation_required",
        Some(detail),
    ));
}

fn mark_delete_failure(
    connection: &Connection,
    intent: &DeleteMutationIntent,
    phase: &'static str,
    detail: String,
) -> String {
    match mark_delete_mutation_unresolved(connection, intent, phase, &detail) {
        Ok(()) => detail,
        Err(error) => format!(
            "{detail}; additionally failed to persist delete reconciliation state: {error:#}"
        ),
    }
}

fn delete_mutation_effect(
    intent: &DeleteMutationIntent,
    kind: RemoteMutationEffectKind,
    response: Option<&DeleteReceipt>,
    observed_revision_id: Option<i64>,
    detail: Option<String>,
) -> RemoteMutationEffect {
    RemoteMutationEffect {
        mutation_id: intent.mutation_id,
        operation: RemoteMutationOperation::Delete,
        target_api_url: intent.target_api_url.clone(),
        title: response
            .map(|item| item.title.clone())
            .unwrap_or_else(|| intent.title.clone()),
        kind,
        old_revision_id: observed_revision_id,
        new_revision_id: None,
        new_timestamp: None,
        deletion_log_id: response.map(|item| item.log_id),
        detail,
    }
}

fn remote_mutation_effect(
    intent: &EditMutationIntent,
    kind: RemoteMutationEffectKind,
    response: Option<&EditReceipt>,
    detail: Option<String>,
) -> RemoteMutationEffect {
    RemoteMutationEffect {
        mutation_id: intent.mutation_id,
        operation: RemoteMutationOperation::Edit,
        target_api_url: intent.target_api_url.clone(),
        title: intent.title.clone(),
        kind,
        old_revision_id: response.map(|item| item.old_revision_id),
        new_revision_id: response.map(|item| item.new_revision_id),
        new_timestamp: response.map(|item| item.new_timestamp.clone()),
        deletion_log_id: None,
        detail,
    }
}

fn append_receipt_persistence_error(
    detail: String,
    persistence_error: Option<anyhow::Error>,
) -> String {
    match persistence_error {
        Some(error) => {
            format!("{detail}; additionally failed to persist reconciliation state: {error:#}")
        }
        None => detail,
    }
}

fn remove_local_sync_state(context: &SyncPlanningContext, title: &str) -> Result<()> {
    remove_sync_page_state(&context.connection, title)
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
