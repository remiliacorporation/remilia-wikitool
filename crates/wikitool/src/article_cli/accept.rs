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
    receipt_path: String,
    content_sha256: String,
    human_editor: String,
    prose_origin: String,
    human_acceptance_attestation: String,
    editorial_quality_attestation: String,
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
    let (receipt, receipt_path) = record_article_acceptance(
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
        schema_version: "article_accept_v1",
        project_root: normalize_path(&paths.project_root),
        source_path: normalize_path(&source_absolute),
        title,
        target_path,
        receipt_path: normalize_path(&receipt_path),
        content_sha256: receipt.content_sha256,
        human_editor: receipt.human_editor,
        prose_origin: receipt.prose_origin.as_str().to_string(),
        human_acceptance_attestation: receipt.attestation,
        editorial_quality_attestation: receipt.editorial_quality_attestation,
        lint_errors: receipt.lint_errors,
        lint_warnings: receipt.lint_warnings,
        lint_suggestions: receipt.lint_suggestions,
        warnings_explicitly_accepted: receipt.warnings_explicitly_accepted,
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
        println!("receipt_path: {}", report.receipt_path);
        println!("content_sha256: {}", report.content_sha256);
        println!("human_editor: {}", report.human_editor);
        println!("prose_origin: {}", report.prose_origin);
        println!(
            "human_acceptance_attestation: {}",
            report.human_acceptance_attestation
        );
        println!(
            "editorial_quality_attestation: {}",
            report.editorial_quality_attestation
        );
        println!("lint_errors: {}", report.lint_errors);
        println!("lint_warnings: {}", report.lint_warnings);
        println!("lint_suggestions: {}", report.lint_suggestions);
        println!(
            "warnings_explicitly_accepted: {}",
            flag(report.warnings_explicitly_accepted)
        );
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        println!(
            "editorial_policy: this receipt records a human decision; agents must not self-attest"
        );
        println!("quality_attestation: specific, readable, proportionate, and source-bound");
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
