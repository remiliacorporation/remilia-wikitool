use std::path::PathBuf;

use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::article_acceptance::{ArticleAcceptanceLintSummary, record_article_acceptance};
use wikitool_core::article_lint::lint_article_with_title;
use wikitool_core::filesystem::{title_to_relative_path, validate_scoped_path};
use wikitool_core::support::parse_redirect;

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::flag;
use super::selection::normalize_article_title;
use super::*;

#[derive(Debug, Serialize)]
struct ArticleAcceptReport {
    schema_version: &'static str,
    project_root: String,
    source_path: String,
    title: String,
    target_path: String,
    acceptance_ledger_path: String,
    content_sha256: String,
    human_editor_claim: String,
    editor_identity_assurance: String,
    prose_origin: String,
    decision: String,
    lint_errors: usize,
    lint_warnings: usize,
    lint_suggestions: usize,
    warnings_explicitly_accepted: bool,
}

pub(super) fn run_article_accept(runtime: &RuntimeOptions, args: ArticleAcceptArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let source_absolute = absolute_path(&paths.project_root, &args.path);
    validate_scoped_path(&paths, &source_absolute)?;
    if !source_absolute.is_file() {
        bail!(
            "article accept source path does not exist or is not a file: {}",
            normalize_path(&source_absolute)
        );
    }
    let content = std::fs::read_to_string(&source_absolute)?;
    if parse_redirect(&content).0 {
        bail!("article accept is for prose articles, not redirects");
    }

    let title = normalize_article_title(&args.title)?;
    let target_path = title_to_relative_path(&paths, &title, false)?;
    if !target_path.starts_with("wiki_content/Main/") {
        bail!("article accept only supports Main-namespace article titles, got: {title}");
    }
    let lint = lint_article_with_title(&paths, &source_absolute, Some(&title))?;
    let (ledger_entry, ledger_path) = record_article_acceptance(
        &paths,
        &source_absolute,
        &title,
        &target_path,
        &args.human_editor,
        args.prose_origin.into(),
        ArticleAcceptanceLintSummary {
            content_sha256: lint.content_sha256.clone(),
            errors: lint.errors,
            warnings: lint.warnings,
            suggestions: lint.suggestions,
            warnings_explicitly_accepted: args.allow_warnings,
        },
    )?;
    let report = ArticleAcceptReport {
        schema_version: "article_accept_v2",
        project_root: normalize_path(&paths.project_root),
        source_path: normalize_path(&source_absolute),
        title,
        target_path,
        acceptance_ledger_path: normalize_path(&ledger_path),
        content_sha256: ledger_entry.content_sha256,
        human_editor_claim: ledger_entry.human_editor_claim,
        editor_identity_assurance: ledger_entry.editor_identity_assurance,
        prose_origin: ledger_entry.prose_origin.as_str().to_string(),
        decision: ledger_entry.decision,
        lint_errors: ledger_entry.lint_errors,
        lint_warnings: ledger_entry.lint_warnings,
        lint_suggestions: ledger_entry.lint_suggestions,
        warnings_explicitly_accepted: ledger_entry.warnings_explicitly_accepted,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("article accept");
        println!("schema_version: {}", report.schema_version);
        println!("project_root: {}", report.project_root);
        println!("source_path: {}", report.source_path);
        println!("title: {}", report.title);
        println!("target_path: {}", report.target_path);
        println!("acceptance_ledger_path: {}", report.acceptance_ledger_path);
        println!("content_sha256: {}", report.content_sha256);
        println!("human_editor_claim: {}", report.human_editor_claim);
        println!(
            "editor_identity_assurance: {}",
            report.editor_identity_assurance
        );
        println!("prose_origin: {}", report.prose_origin);
        println!("decision: {}", report.decision);
        println!("lint_errors: {}", report.lint_errors);
        println!("lint_warnings: {}", report.lint_warnings);
        println!("lint_suggestions: {}", report.lint_suggestions);
        println!(
            "warnings_explicitly_accepted: {}",
            flag(report.warnings_explicitly_accepted)
        );
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        println!(
            "ledger_scope: exact-content decision record; editor identity is self-reported and unauthenticated"
        );
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }
    Ok(())
}

fn absolute_path(project_root: &std::path::Path, path: &PathBuf) -> PathBuf {
    if path.is_absolute() {
        path.clone()
    } else {
        project_root.join(path)
    }
}
