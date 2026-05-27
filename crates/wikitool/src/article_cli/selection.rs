use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use wikitool_core::filesystem::{
    relative_path_to_title, title_to_relative_path, validate_scoped_path,
};
use wikitool_core::sync::{SyncSelection, collect_changed_article_paths};

use crate::cli_support::{normalize_path, path_is_under_directory};

#[derive(Debug, Clone, Serialize)]
pub(super) struct ArticleTargetSelection {
    pub(super) changed: bool,
    pub(super) titles: Vec<String>,
    pub(super) paths: Vec<String>,
}

pub(super) fn uses_single_path_mode(
    path: Option<&Path>,
    titles: &[String],
    paths: &[PathBuf],
    titles_file: Option<&PathBuf>,
    changed: bool,
) -> bool {
    path.is_some() && titles.is_empty() && paths.is_empty() && titles_file.is_none() && !changed
}

pub(super) fn single_state_path_title_override<'a>(
    runtime_paths: &wikitool_core::runtime::ResolvedPaths,
    path: Option<&Path>,
    titles: &'a [String],
    paths: &[PathBuf],
    titles_file: Option<&PathBuf>,
    changed: bool,
) -> Result<Option<&'a str>> {
    let Some(path) = path else {
        return Ok(None);
    };
    if titles.len() != 1 || !paths.is_empty() || titles_file.is_some() || changed {
        return Ok(None);
    }

    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        runtime_paths.project_root.join(path)
    };
    validate_scoped_path(runtime_paths, &absolute_path)?;
    if path_is_under_state_drafts_dir(runtime_paths, &absolute_path) {
        return Ok(Some(titles[0].as_str()));
    }
    Ok(None)
}

pub(super) fn path_is_under_state_drafts_dir(
    runtime_paths: &wikitool_core::runtime::ResolvedPaths,
    absolute_path: &Path,
) -> bool {
    path_is_under_directory(absolute_path, &runtime_paths.state_dir.join("drafts"))
}

pub(super) fn normalize_article_title(title: &str) -> Result<String> {
    let normalized = title.trim().replace('_', " ");
    if normalized.is_empty() {
        bail!("article title must not be empty");
    }
    Ok(normalized)
}

pub(super) fn article_selection_from_args(
    titles: &[String],
    paths: &[PathBuf],
    titles_file: Option<&PathBuf>,
    changed: bool,
) -> Result<ArticleTargetSelection> {
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

    Ok(ArticleTargetSelection {
        changed,
        titles: loaded_titles,
        paths: paths.iter().map(normalize_path).collect(),
    })
}

pub(super) fn resolve_article_targets(
    paths: &wikitool_core::runtime::ResolvedPaths,
    positional_path: Option<&Path>,
    selection: &ArticleTargetSelection,
    include_selected_redirects: bool,
) -> Result<Vec<String>> {
    let mut target_paths = BTreeSet::new();
    if let Some(path) = positional_path {
        target_paths.insert(resolve_article_selector_path(paths, path)?);
    }

    let sync_selection = SyncSelection {
        titles: selection.titles.clone(),
        paths: selection.paths.clone(),
    };
    if selection.changed {
        let Some(changed_paths) =
            collect_changed_article_paths(paths, &sync_selection, include_selected_redirects)?
        else {
            bail!("article --changed requires a built sync ledger (run `wikitool pull --full`)");
        };
        for relative_path in changed_paths {
            target_paths.insert(relative_path);
        }
    } else {
        for title in &selection.titles {
            target_paths.insert(resolve_article_title(paths, title)?);
        }
        for path in &selection.paths {
            target_paths.insert(resolve_article_selector_path(paths, Path::new(path))?);
        }
    }

    if target_paths.is_empty() {
        if selection.changed {
            return Ok(Vec::new());
        }
        bail!("article command requires a file path, selector, or --changed");
    }

    Ok(target_paths.into_iter().collect())
}

fn resolve_article_title(
    paths: &wikitool_core::runtime::ResolvedPaths,
    title: &str,
) -> Result<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        bail!("article title selectors must not be empty");
    }

    for is_redirect in [false, true] {
        let relative_path = title_to_relative_path(paths, trimmed, is_redirect)?;
        let absolute_path = paths.project_root.join(&relative_path);
        if absolute_path.exists() {
            return Ok(relative_path);
        }
    }

    bail!("no local article file found for title: {trimmed}")
}

fn resolve_article_selector_path(
    paths: &wikitool_core::runtime::ResolvedPaths,
    path: &Path,
) -> Result<String> {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    validate_scoped_path(paths, &absolute_path)?;
    if !absolute_path.exists() {
        bail!(
            "article path does not exist: {}",
            normalize_path(&absolute_path)
        );
    }
    let relative_path = absolute_path
        .strip_prefix(&paths.project_root)
        .with_context(|| {
            format!(
                "failed to resolve {} relative to {}",
                normalize_path(&absolute_path),
                normalize_path(&paths.project_root)
            )
        })?;
    let relative_path = normalize_path(relative_path);
    if !relative_path.starts_with("wiki_content/") {
        bail!(
            "article batch selectors only support files under wiki_content/: {}. For one off-wiki draft, pass the draft path with exactly one --title.",
            relative_path
        );
    }
    let _ = relative_path_to_title(paths, &relative_path)?;
    Ok(relative_path)
}
