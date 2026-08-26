use std::fs;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use wikitool_core::article_acceptance::load_accepted_article;
use wikitool_core::filesystem::{title_to_relative_path, validate_scoped_path};

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::flag;
use super::selection::{normalize_article_title, path_is_under_state_drafts_dir};
use super::*;
#[derive(Debug, Serialize)]
struct ArticlePromoteReport {
    schema_version: &'static str,
    project_root: String,
    source_path: String,
    title: String,
    target_path: String,
    overwritten: bool,
    source_preserved: bool,
    acceptance_receipt: String,
    human_editor: String,
    prose_origin: String,
    content_sha256: String,
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
    if !target_path.starts_with("wiki_content/Main/") {
        bail!("article promote only supports Main-namespace article titles, got: {title}");
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
    let accepted = load_accepted_article(&paths, &source_absolute, &title, &target_path)?;
    let acceptance_receipt =
        wikitool_core::article_acceptance::article_acceptance_receipt_path(&paths, &target_path)?;
    if let Some(parent) = target_absolute.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }
    fs::write(&target_absolute, accepted.content.as_bytes()).with_context(|| {
        format!(
            "failed to write accepted article bytes to {}",
            normalize_path(&target_absolute),
        )
    })?;

    let report = ArticlePromoteReport {
        schema_version: "article_promote_v2",
        project_root: normalize_path(&paths.project_root),
        source_path: normalize_path(&source_absolute),
        title,
        target_path,
        overwritten,
        source_preserved: true,
        acceptance_receipt: normalize_path(&acceptance_receipt),
        human_editor: accepted.receipt.human_editor,
        prose_origin: accepted.receipt.prose_origin.as_str().to_string(),
        content_sha256: accepted.receipt.content_sha256,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("article promote");
        println!("schema_version: {}", report.schema_version);
        println!("project_root: {}", report.project_root);
        println!("source_path: {}", report.source_path);
        println!("title: {}", report.title);
        println!("target_path: {}", report.target_path);
        println!("overwritten: {}", flag(report.overwritten));
        println!("source_preserved: {}", flag(report.source_preserved));
        println!("acceptance_receipt: {}", report.acceptance_receipt);
        println!("human_editor: {}", report.human_editor);
        println!("prose_origin: {}", report.prose_origin);
        println!("content_sha256: {}", report.content_sha256);
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }
    Ok(())
}
