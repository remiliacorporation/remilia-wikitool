use std::env;

use anyhow::Result;

use crate::catalog::content_index::rebuild_index;
use crate::config::WikiConfig;
use crate::filesystem::{ScanOptions, sync_project_paths};
use crate::mw::client_from_wikitool_config;
use crate::runtime::ResolvedPaths;

pub use mediawiki_protocol::{
    EditConstraint, EditReceipt, MediaWikiClient, MediaWikiClientConfig, NS_CATEGORY, NS_MAIN,
    NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE, PageTimestampInfo, RemotePage, WikiReadApi, WikiWriteApi,
};
pub use wikitool_sync::{
    DiffBaselineStatus, DiffChange, DiffChangeType, DiffOptions, DiffReport, PreparedPublication,
    PublicationAuthorityBinding, PublicationCandidate, PublicationPreflight, PublicationProvenance,
    PullCoverage, PullOptions, PullPageResult, PullReport, PushOptions, PushPageResult, PushReport,
    RemoteDeleteApplyRequest, RemoteDeleteError, RemoteDeleteLocalEffectPolicy, RemoteDeletePlan,
    RemoteDeleteReport, RemoteDeleteStatus, RemoteMutationClosureReceipt,
    RemoteMutationClosureReport, RemoteMutationEffect, RemoteMutationEffectKind,
    RemoteMutationInspection, RemoteMutationListReport, RemoteMutationOperation,
    RemoteMutationReconciliationReport, RemoteMutationReconciliationStatus, SyncPathEffect,
    SyncPathEffectKind, SyncPlanChange, SyncPlanOptions, SyncPlanReport, SyncSelection,
    SyncStateError, SyncStoreMigrationReport, SyncStoreMigrationStatus, SyncTargetIdentity,
};

fn target_from_config(config: &WikiConfig) -> Result<SyncTargetIdentity> {
    SyncTargetIdentity::from_api_url(config.api_url_owned().as_deref().unwrap_or_default())
}

/// Verify the read-only durable sync authority for the resolved wiki target.
/// The returned identity is normalized and may be bound into an operation
/// receipt by the caller.
pub fn verify_established_sync_target(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<SyncTargetIdentity> {
    let target = target_from_config(config)?;
    wikitool_sync::verify_established_sync_target(&sync_project_paths(paths)?, &target)?;
    Ok(target)
}

pub fn pull_from_remote(paths: &ResolvedPaths, options: &PullOptions) -> Result<PullReport> {
    pull_from_remote_with_config(paths, options, &WikiConfig::default())
}

pub fn pull_from_remote_with_config(
    paths: &ResolvedPaths,
    options: &PullOptions,
    config: &WikiConfig,
) -> Result<PullReport> {
    let sync_paths = sync_project_paths(paths)?;
    let target = target_from_config(config)?;
    let mut client = client_from_wikitool_config(config)?;
    let report =
        wikitool_sync::pull_from_remote_with_api(&sync_paths, options, &target, &mut client)?;
    if !report.changed_paths.is_empty() {
        rebuild_index(paths, &ScanOptions::default())?;
    }
    Ok(report)
}

pub fn diff_local_against_sync(
    paths: &ResolvedPaths,
    options: &DiffOptions,
) -> Result<Option<DiffReport>> {
    let config = crate::config::load_config(&paths.config_path)?;
    diff_local_against_sync_with_config(paths, options, &config)
}

pub fn diff_local_against_sync_with_config(
    paths: &ResolvedPaths,
    options: &DiffOptions,
    config: &WikiConfig,
) -> Result<Option<DiffReport>> {
    wikitool_sync::diff_local_against_sync(
        &sync_project_paths(paths)?,
        options,
        &target_from_config(config)?,
    )
}

pub fn plan_sync_changes(
    paths: &ResolvedPaths,
    options: &SyncPlanOptions,
) -> Result<Option<SyncPlanReport>> {
    let config = crate::config::load_config(&paths.config_path)?;
    plan_sync_changes_with_config(paths, options, &config)
}

pub fn plan_sync_changes_with_config(
    paths: &ResolvedPaths,
    options: &SyncPlanOptions,
    config: &WikiConfig,
) -> Result<Option<SyncPlanReport>> {
    let sync_paths = sync_project_paths(paths)?;
    let target = target_from_config(config)?;
    if options.include_remote_conflicts {
        let mut client = client_from_wikitool_config(config)?;
        wikitool_sync::plan_sync_changes_with_api(&sync_paths, options, &target, &mut client)
    } else {
        wikitool_sync::plan_sync_changes(&sync_paths, options, &target)
    }
}

pub fn collect_changed_article_paths(
    paths: &ResolvedPaths,
    selection: &SyncSelection,
    include_selected_redirects: bool,
) -> Result<Option<Vec<String>>> {
    let config = crate::config::load_config(&paths.config_path)?;
    wikitool_sync::collect_changed_article_paths(
        &sync_project_paths(paths)?,
        selection,
        include_selected_redirects,
        &target_from_config(&config)?,
    )
}

pub fn push_to_remote(
    paths: &ResolvedPaths,
    options: &PushOptions,
    preflight: &dyn PublicationPreflight,
) -> Result<PushReport> {
    push_to_remote_with_config(paths, options, &WikiConfig::default(), preflight)
}

pub fn push_to_remote_with_config(
    paths: &ResolvedPaths,
    options: &PushOptions,
    config: &WikiConfig,
    preflight: &dyn PublicationPreflight,
) -> Result<PushReport> {
    let sync_paths = sync_project_paths(paths)?;
    let target = target_from_config(config)?;
    let mut client = client_from_wikitool_config(config)?;
    let credentials = if options.dry_run {
        None
    } else {
        let username = env::var("WIKITOOL_BOT_USER")
            .map_err(|_| anyhow::anyhow!("WIKITOOL_BOT_USER is required for push"))?;
        let password = env::var("WIKITOOL_BOT_PASS")
            .map_err(|_| anyhow::anyhow!("WIKITOOL_BOT_PASS is required for push"))?;
        Some((username, password))
    };
    wikitool_sync::push_to_remote_with_api_and_preflight(
        &sync_paths,
        options,
        &target,
        &mut client,
        credentials
            .as_ref()
            .map(|(user, password)| (user.as_str(), password.as_str())),
        preflight,
    )
}

pub fn plan_remote_delete_with_config(
    paths: &ResolvedPaths,
    title: &str,
    reason: &str,
    local_effect_policy: RemoteDeleteLocalEffectPolicy,
    config: &WikiConfig,
) -> Result<RemoteDeletePlan> {
    let mut client = client_from_wikitool_config(config)?;
    wikitool_sync::plan_remote_delete_with_api(
        &sync_project_paths(paths)?,
        &target_from_config(config)?,
        &mut client,
        title,
        reason,
        local_effect_policy,
    )
}

pub fn apply_remote_delete_with_config(
    paths: &ResolvedPaths,
    title: &str,
    reason: &str,
    local_effect_policy: RemoteDeleteLocalEffectPolicy,
    plan_id: &str,
    config: &WikiConfig,
) -> Result<RemoteDeleteReport> {
    let mut client = client_from_wikitool_config(config)?;
    let username = env::var("WIKITOOL_BOT_USER").ok();
    let password = env::var("WIKITOOL_BOT_PASS").ok();
    let credentials = username
        .as_deref()
        .zip(password.as_deref())
        .filter(|(user, password)| !user.trim().is_empty() && !password.trim().is_empty());
    wikitool_sync::apply_remote_delete_with_api(
        &sync_project_paths(paths)?,
        &target_from_config(config)?,
        &mut client,
        RemoteDeleteApplyRequest {
            title,
            reason,
            local_effect_policy,
            plan_id: Some(plan_id),
            credentials,
        },
    )
}

pub fn list_remote_mutations_with_config(
    paths: &ResolvedPaths,
    unresolved_only: bool,
    config: &WikiConfig,
) -> Result<RemoteMutationListReport> {
    wikitool_sync::list_remote_mutations(
        &sync_project_paths(paths)?,
        &target_from_config(config)?,
        unresolved_only,
    )
}

pub fn show_remote_mutation_with_config(
    paths: &ResolvedPaths,
    operation: RemoteMutationOperation,
    mutation_id: i64,
    config: &WikiConfig,
) -> Result<RemoteMutationInspection> {
    wikitool_sync::show_remote_mutation(
        &sync_project_paths(paths)?,
        &target_from_config(config)?,
        operation,
        mutation_id,
    )
}

pub fn reconcile_remote_mutation_with_config(
    paths: &ResolvedPaths,
    operation: RemoteMutationOperation,
    mutation_id: i64,
    config: &WikiConfig,
) -> Result<RemoteMutationReconciliationReport> {
    let mut client = client_from_wikitool_config(config)?;
    wikitool_sync::reconcile_remote_mutation_with_api(
        &sync_project_paths(paths)?,
        &target_from_config(config)?,
        &mut client,
        operation,
        mutation_id,
    )
}

pub fn close_remote_mutation_with_config(
    paths: &ResolvedPaths,
    operation: RemoteMutationOperation,
    mutation_id: i64,
    actor: &str,
    reason: &str,
    config: &WikiConfig,
) -> Result<RemoteMutationClosureReport> {
    wikitool_sync::close_remote_mutation(
        &sync_project_paths(paths)?,
        &target_from_config(config)?,
        operation,
        mutation_id,
        actor,
        reason,
    )
}

pub fn preserve_legacy_sync_state(paths: &ResolvedPaths) -> Result<SyncStoreMigrationReport> {
    wikitool_sync::preserve_legacy_sync_state(&sync_project_paths(paths)?, Some(&paths.db_path))
}
