use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;
use wikitool_core::config::WikiConfig;
use wikitool_core::runtime::{ResolvedPaths, RuntimeStatus};
use wikitool_core::sync::{
    DiffBaselineStatus, DiffChangeType, NS_CATEGORY, NS_MAIN, NS_MEDIAWIKI, NS_MODULE, NS_TEMPLATE,
    SyncPlanChange, SyncPlanReport, SyncSelection,
};

use crate::cli_support::normalize_path;

use super::PullArgs;

#[derive(Debug, Serialize)]
pub(super) struct RuntimeStatusJson {
    project_root_exists: bool,
    wiki_content_exists: bool,
    templates_exists: bool,
    state_dir_exists: bool,
    data_dir_exists: bool,
    db_exists: bool,
    db_size_bytes: Option<u64>,
    config_exists: bool,
    parser_config_exists: bool,
    warnings: Vec<String>,
}

pub(super) fn runtime_status_json(status: &RuntimeStatus) -> RuntimeStatusJson {
    RuntimeStatusJson {
        project_root_exists: status.project_root_exists,
        wiki_content_exists: status.wiki_content_exists,
        templates_exists: status.templates_exists,
        state_dir_exists: status.state_dir_exists,
        data_dir_exists: status.data_dir_exists,
        db_exists: status.db_exists,
        db_size_bytes: status.db_size_bytes,
        config_exists: status.config_exists,
        parser_config_exists: status.parser_config_exists,
        warnings: status.warnings.clone(),
    }
}

pub(super) fn load_sync_selection(
    titles: &[String],
    paths: &[String],
    titles_file: Option<&PathBuf>,
) -> Result<SyncSelection> {
    let mut loaded_titles = titles.to_vec();
    if let Some(titles_file) = titles_file {
        let content = fs::read_to_string(titles_file)
            .with_context(|| format!("failed to read {}", normalize_path(titles_file)))?;
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                loaded_titles.push(trimmed.to_string());
            }
        }
    }
    Ok(SyncSelection {
        titles: loaded_titles,
        paths: paths.to_vec(),
    })
}

pub(super) fn materialize_custom_namespace_dirs(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<Vec<PathBuf>> {
    let mut created = Vec::new();
    for namespace in &config.wiki.custom_namespaces {
        let folder = namespace.folder().trim();
        if folder.is_empty() {
            continue;
        }
        let namespace_dir = paths.wiki_content_dir.join(folder);
        if !namespace_dir.exists() {
            fs::create_dir_all(&namespace_dir)
                .with_context(|| format!("failed to create {}", normalize_path(&namespace_dir)))?;
            created.push(namespace_dir.clone());
        }
        let redirects = namespace_dir.join("_redirects");
        if !redirects.exists() {
            fs::create_dir_all(&redirects)
                .with_context(|| format!("failed to create {}", normalize_path(&redirects)))?;
            created.push(redirects);
        }
    }
    Ok(created)
}

pub(super) fn pull_namespaces_from_args(args: &PullArgs, config: &WikiConfig) -> Vec<i32> {
    if args.templates {
        return vec![NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
    }
    if args.categories {
        return vec![NS_CATEGORY];
    }
    if args.all {
        let mut namespaces = vec![NS_MAIN, NS_CATEGORY, NS_TEMPLATE, NS_MODULE, NS_MEDIAWIKI];
        for custom in &config.wiki.custom_namespaces {
            if custom.id >= 0 {
                namespaces.push(custom.id);
            }
        }
        namespaces.sort_unstable();
        namespaces.dedup();
        return namespaces;
    }
    vec![NS_MAIN]
}

pub(super) fn format_baseline_status(value: Option<&DiffBaselineStatus>) -> &'static str {
    match value {
        Some(DiffBaselineStatus::Available) => "available",
        Some(DiffBaselineStatus::MissingSnapshot) => "missing_snapshot",
        Some(DiffBaselineStatus::NotApplicable) => "not_applicable",
        None => "<none>",
    }
}

pub(super) fn format_diff_change_type(value: &DiffChangeType) -> &'static str {
    match value {
        DiffChangeType::NewLocal => "new_local",
        DiffChangeType::ModifiedLocal => "modified_local",
        DiffChangeType::DeletedLocal => "deleted_local",
    }
}

pub(super) fn status_display_changes(
    plan: &SyncPlanReport,
    modified_only: bool,
    conflicts_only: bool,
) -> Vec<&SyncPlanChange> {
    plan.changes
        .iter()
        .filter(|change| {
            if conflicts_only && !change.remote_conflict {
                return false;
            }
            if modified_only && conflicts_only {
                return true;
            }
            if modified_only {
                return true;
            }
            true
        })
        .collect()
}
