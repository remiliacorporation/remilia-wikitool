use std::path::Path;

use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::article_lint::{ArticleLintReport, lint_article, lint_article_with_title};

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::{flag, print_article_target_selection, print_report};
use super::selection::{
    ArticleTargetSelection, article_selection_from_args, resolve_article_targets,
    single_state_path_title_override, uses_single_path_mode,
};
use super::*;
#[derive(Debug, Serialize)]
struct ArticleLintBatchReport {
    project_root: String,
    strict: bool,
    selection: ArticleTargetSelection,
    target_count: usize,
    total_errors: usize,
    total_warnings: usize,
    total_suggestions: usize,
    reports: Vec<ArticleLintReport>,
}

pub(super) fn run_article_lint(runtime: &RuntimeOptions, args: ArticleLintArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    if let Some(title_override) = single_state_path_title_override(
        &paths,
        args.path.as_deref(),
        &args.titles,
        &args.paths,
        args.titles_file.as_ref(),
        args.changed,
    )? {
        let report = lint_article_with_title(
            &paths,
            args.path.as_deref().expect("single path"),
            Some(title_override),
        )?;

        if args.format.is_json() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("article lint");
            println!("project_root: {}", normalize_path(&paths.project_root));
            print_report(&report);
            println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
            if runtime.diagnostics {
                println!("\n[diagnostics]\n{}", paths.diagnostics());
            }
        }

        if report.errors > 0 || (args.strict && report.warnings > 0) {
            bail!(
                "article lint found {} error(s), {} warning(s), and {} suggestion(s)",
                report.errors,
                report.warnings,
                report.suggestions
            );
        }
        return Ok(());
    }

    if uses_single_path_mode(
        args.path.as_deref(),
        &args.titles,
        &args.paths,
        args.titles_file.as_ref(),
        args.changed,
    ) {
        let report = lint_article(&paths, args.path.as_deref().expect("single path"))?;

        if args.format.is_json() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("article lint");
            println!("project_root: {}", normalize_path(&paths.project_root));
            print_report(&report);
            println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
            if runtime.diagnostics {
                println!("\n[diagnostics]\n{}", paths.diagnostics());
            }
        }

        if report.errors > 0 || (args.strict && report.warnings > 0) {
            bail!(
                "article lint found {} error(s), {} warning(s), and {} suggestion(s)",
                report.errors,
                report.warnings,
                report.suggestions
            );
        }
        return Ok(());
    }

    let selection = article_selection_from_args(
        &args.titles,
        &args.paths,
        args.titles_file.as_ref(),
        args.changed,
    )?;
    let target_paths = resolve_article_targets(&paths, args.path.as_deref(), &selection, false)?;
    let reports = target_paths
        .iter()
        .map(|relative_path| lint_article(&paths, Path::new(relative_path)))
        .collect::<Result<Vec<_>>>()?;
    let total_errors = reports.iter().map(|report| report.errors).sum();
    let total_warnings = reports.iter().map(|report| report.warnings).sum();
    let total_suggestions = reports.iter().map(|report| report.suggestions).sum();
    let batch_report = ArticleLintBatchReport {
        project_root: normalize_path(&paths.project_root),
        strict: args.strict,
        selection,
        target_count: reports.len(),
        total_errors,
        total_warnings,
        total_suggestions,
        reports,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&batch_report)?);
    } else {
        println!("article lint");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("strict: {}", flag(batch_report.strict));
        print_article_target_selection(&batch_report.selection);
        println!("target_count: {}", batch_report.target_count);
        println!("total_errors: {}", batch_report.total_errors);
        println!("total_warnings: {}", batch_report.total_warnings);
        println!("total_suggestions: {}", batch_report.total_suggestions);
        if batch_report.reports.is_empty() {
            println!("reports: <none>");
        } else {
            for report in &batch_report.reports {
                println!();
                print_report(report);
            }
        }
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }

    if batch_report.total_errors > 0 || (args.strict && batch_report.total_warnings > 0) {
        bail!(
            "article lint found {} error(s), {} warning(s), and {} suggestion(s) across {} file(s)",
            batch_report.total_errors,
            batch_report.total_warnings,
            batch_report.total_suggestions,
            batch_report.target_count
        );
    }
    Ok(())
}
