use std::fs;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use wikitool_core::filesystem::{title_to_relative_path, validate_scoped_path};

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::flag;
use super::selection::{normalize_article_title, path_is_under_state_drafts_dir};
use super::*;
#[derive(Debug, Serialize)]
struct ArticlePromoteReport {
    project_root: String,
    source_path: String,
    title: String,
    target_path: String,
    overwritten: bool,
    source_preserved: bool,
}

pub(super) fn run_article_promote(
    runtime: &RuntimeOptions,
    args: ArticlePromoteArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let source_absolute = if args.path.is_absolute() {
        args.path.clone()
    } else {
        paths.project_root.join(&args.path)
    };
    validate_scoped_path(&paths, &source_absolute)?;
    if !path_is_under_state_drafts_dir(&paths, &source_absolute) {
        bail!(
            "article promote source must be under the canonical draft directory: {}/drafts/",
            normalize_path(&paths.state_dir)
        );
    }
    if !source_absolute.is_file() {
        bail!(
            "article promote source path does not exist or is not a file: {}",
            normalize_path(&source_absolute)
        );
    }

    let title = normalize_article_title(&args.title)?;
    let target_path = title_to_relative_path(&paths, &title, false)?;
    if !target_path.starts_with("wiki_content/") {
        bail!("article promote only supports wiki_content/ article titles, got: {title}");
    }
    let target_absolute = paths.project_root.join(&target_path);
    validate_scoped_path(&paths, &target_absolute)?;
    let overwritten = target_absolute.exists();
    if overwritten && !args.overwrite {
        bail!(
            "article promote target already exists: {} (use --overwrite to replace it)",
            normalize_path(&target_absolute)
        );
    }
    if let Some(parent) = target_absolute.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }
    fs::copy(&source_absolute, &target_absolute).with_context(|| {
        format!(
            "failed to copy {} -> {}",
            normalize_path(&source_absolute),
            normalize_path(&target_absolute)
        )
    })?;

    let report = ArticlePromoteReport {
        project_root: normalize_path(&paths.project_root),
        source_path: normalize_path(&source_absolute),
        title,
        target_path,
        overwritten,
        source_preserved: true,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("article promote");
        println!("project_root: {}", report.project_root);
        println!("source_path: {}", report.source_path);
        println!("title: {}", report.title);
        println!("target_path: {}", report.target_path);
        println!("overwritten: {}", flag(report.overwritten));
        println!("source_preserved: {}", flag(report.source_preserved));
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }
    Ok(())
}
