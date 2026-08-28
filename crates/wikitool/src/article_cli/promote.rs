use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::article_acceptance::load_accepted_article;
use wikitool_core::filesystem::{title_to_relative_path, validate_scoped_path};
use wikitool_core::support::atomic_write;

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
    acceptance_store_path: String,
    human_editor_claim: String,
    editor_identity_assurance: String,
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
    atomic_write(&target_absolute, accepted.content.as_bytes())?;

    let report = ArticlePromoteReport {
        schema_version: "article_promote_v4",
        project_root: normalize_path(&paths.project_root),
        source_path: normalize_path(&source_absolute),
        title,
        target_path,
        overwritten,
        source_preserved: true,
        acceptance_store_path: normalize_path(paths.acceptance_store_path()),
        human_editor_claim: accepted.ledger_entry.human_editor_claim,
        editor_identity_assurance: accepted.ledger_entry.editor_identity_assurance,
        prose_origin: accepted.ledger_entry.prose_origin.as_str().to_string(),
        content_sha256: accepted.ledger_entry.content_sha256,
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
        println!("acceptance_store_path: {}", report.acceptance_store_path);
        println!("human_editor_claim: {}", report.human_editor_claim);
        println!(
            "editor_identity_assurance: {}",
            report.editor_identity_assurance
        );
        println!("prose_origin: {}", report.prose_origin);
        println!("content_sha256: {}", report.content_sha256);
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }
    Ok(())
}
