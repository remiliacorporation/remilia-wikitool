use std::path::Path;

use anyhow::Result;
use wikitool_core::article_lint::lint_article;
use wikitool_core::config::WikiConfig;
use wikitool_core::knowledge::inspect::{ValidationReport, run_validation_checks};
use wikitool_core::runtime::ResolvedPaths;
use wikitool_core::sync::{
    PushOptions, SyncSelection, collect_changed_article_paths, push_to_remote_with_config,
};

use super::{ReviewArticleLint, ReviewDryRunPush, ReviewValidation, ReviewValidationSummary};

pub(super) fn run_changed_article_lint(
    paths: &ResolvedPaths,
    selection: &SyncSelection,
    strict: bool,
) -> Result<ReviewArticleLint> {
    let Some(target_paths) = collect_changed_article_paths(paths, selection, false)? else {
        return Ok(ReviewArticleLint {
            sync_ledger_ready: false,
            target_count: 0,
            total_errors: 0,
            total_warnings: 0,
            total_suggestions: 0,
            reports: Vec::new(),
            error: None,
        });
    };

    let reports = target_paths
        .iter()
        .map(|relative_path| lint_article(paths, Path::new(relative_path)))
        .collect::<Result<Vec<_>>>()?;
    let total_errors = reports.iter().map(|report| report.errors).sum();
    let total_warnings = reports.iter().map(|report| report.warnings).sum();
    let total_suggestions = reports.iter().map(|report| report.suggestions).sum();
    let error = if total_errors > 0 || (strict && total_warnings > 0) {
        Some(format!(
            "{} error(s), {} warning(s), and {} suggestion(s)",
            total_errors, total_warnings, total_suggestions
        ))
    } else {
        None
    };

    Ok(ReviewArticleLint {
        sync_ledger_ready: true,
        target_count: reports.len(),
        total_errors,
        total_warnings,
        total_suggestions,
        reports,
        error,
    })
}

pub(super) fn run_review_validation(paths: &ResolvedPaths) -> Result<ReviewValidation> {
    let Some(report) = run_validation_checks(paths)? else {
        return Ok(ReviewValidation {
            index_ready: false,
            issue_count: 0,
            summary: None,
        });
    };

    Ok(ReviewValidation {
        index_ready: true,
        issue_count: validation_issue_count(&report),
        summary: Some(validation_summary(&report)),
    })
}

pub(super) fn run_review_push_dry_run(
    paths: &ResolvedPaths,
    config: &WikiConfig,
    selection: &SyncSelection,
    summary: &str,
    templates: bool,
    categories: bool,
) -> ReviewDryRunPush {
    match push_to_remote_with_config(
        paths,
        &PushOptions {
            summary: summary.to_string(),
            dry_run: true,
            force: false,
            delete: false,
            include_templates: templates,
            categories_only: categories,
            selection: selection.clone(),
        },
        config,
    ) {
        Ok(report) => ReviewDryRunPush {
            attempted: true,
            success: report.success,
            report: Some(report),
            error: None,
            skipped_reason: None,
        },
        Err(error) => ReviewDryRunPush {
            attempted: true,
            success: false,
            report: None,
            error: Some(error.to_string()),
            skipped_reason: None,
        },
    }
}

fn validation_issue_count(report: &ValidationReport) -> usize {
    report.broken_links.len()
        + report.double_redirects.len()
        + report.uncategorized_pages.len()
        + report.orphan_pages.len()
}

fn validation_summary(report: &ValidationReport) -> ReviewValidationSummary {
    ReviewValidationSummary {
        broken_links: report.broken_links.len(),
        double_redirects: report.double_redirects.len(),
        uncategorized_pages: report.uncategorized_pages.len(),
        orphan_pages: report.orphan_pages.len(),
    }
}
