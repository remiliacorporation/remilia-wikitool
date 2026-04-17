use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::Serialize;
use wikitool_core::article_lint::{ArticleLintReport, lint_article, lint_article_with_title};
use wikitool_core::knowledge::inspect::{ValidationReport, run_validation_checks};
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{
    PushOptions, PushReport, SyncPlanOptions, SyncPlanReport, SyncSelection,
    collect_changed_article_paths, plan_sync_changes_with_config, push_to_remote_with_config,
};

use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct ReviewArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
    #[arg(long, default_value = "remilia", value_name = "PROFILE")]
    profile: String,
    #[arg(long, help = "Treat article lint warnings as review failures")]
    strict: bool,
    #[arg(
        long,
        help = "Include template/module/mediawiki namespaces in sync checks"
    )]
    templates: bool,
    #[arg(long, help = "Limit sync checks to Category namespace pages")]
    categories: bool,
    #[arg(long = "title", value_name = "TITLE")]
    titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,
    #[arg(
        long = "draft-path",
        value_name = "PATH",
        help = "Review one off-wiki draft path; requires exactly one --title and skips push dry-run"
    )]
    draft_paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    titles_file: Option<PathBuf>,
    #[arg(
        long,
        value_name = "TEXT",
        default_value = "wikitool review dry-run",
        help = "Edit summary used for the push dry-run report"
    )]
    summary: String,
}

#[derive(Debug, Serialize)]
struct ReviewReport {
    project_root: String,
    status: &'static str,
    hard_failures: Vec<String>,
    filters: ReviewFilters,
    status_plan: ReviewStatusPlan,
    changed_article_lint: ReviewArticleLint,
    validation: ReviewValidation,
    dry_run_push: ReviewDryRunPush,
}

#[derive(Debug, Clone, Serialize)]
struct ReviewFilters {
    mode: &'static str,
    profile: String,
    strict: bool,
    templates: bool,
    categories: bool,
    selection: SyncSelection,
    draft_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ReviewStatusPlan {
    sync_ledger_ready: bool,
    selection_state: &'static str,
    selected_change_count: usize,
    plan: Option<SyncPlanReport>,
}

#[derive(Debug, Serialize)]
struct ReviewArticleLint {
    sync_ledger_ready: bool,
    target_count: usize,
    total_errors: usize,
    total_warnings: usize,
    total_suggestions: usize,
    reports: Vec<ArticleLintReport>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReviewValidation {
    index_ready: bool,
    issue_count: usize,
    summary: Option<ReviewValidationSummary>,
}

#[derive(Debug, Serialize)]
struct ReviewValidationSummary {
    broken_links: usize,
    double_redirects: usize,
    uncategorized_pages: usize,
    orphan_pages: usize,
}

#[derive(Debug, Serialize)]
struct ReviewDryRunPush {
    attempted: bool,
    success: bool,
    report: Option<PushReport>,
    error: Option<String>,
    skipped_reason: Option<String>,
}

pub(crate) fn run_review(runtime: &RuntimeOptions, args: ReviewArgs) -> Result<()> {
    if args.summary.trim().is_empty() {
        bail!("review requires a non-empty --summary");
    }

    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let runtime_status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &runtime_status)?;
    let draft_selection = review_draft_selection_from_args(&args)?;
    let selection = if draft_selection.is_some() {
        SyncSelection {
            titles: args.titles.clone(),
            paths: Vec::new(),
        }
    } else {
        review_selection_from_args(&args.titles, &args.paths, args.titles_file.as_ref())?
    };
    let filters = ReviewFilters {
        mode: if draft_selection.is_some() {
            "draft"
        } else {
            "sync"
        },
        profile: args.profile.clone(),
        strict: args.strict,
        templates: args.templates,
        categories: args.categories,
        selection: selection.clone(),
        draft_paths: args.draft_paths.iter().map(normalize_path).collect(),
    };

    let plan = if draft_selection.is_some() {
        None
    } else {
        plan_sync_changes_with_config(
            &paths,
            &SyncPlanOptions {
                include_templates: args.templates,
                categories_only: args.categories,
                include_deletes: true,
                include_remote_conflicts: false,
                selection: selection.clone(),
            },
            &config,
        )?
    };
    let selected_change_count = plan
        .as_ref()
        .map(|plan| plan.changes.len())
        .unwrap_or_default();
    let status_plan = ReviewStatusPlan {
        sync_ledger_ready: draft_selection.is_some() || plan.is_some(),
        selection_state: if draft_selection.is_some() {
            "draft_path"
        } else if plan.is_some() && selected_change_count == 0 {
            "no_selected_changes"
        } else if plan.is_some() {
            "selected_changes"
        } else {
            "sync_ledger_missing"
        },
        selected_change_count,
        plan,
    };

    let changed_article_lint = if let Some(draft_selection) = &draft_selection {
        run_draft_article_lint(&paths, draft_selection, &args.profile, args.strict)?
    } else {
        run_changed_article_lint(&paths, &selection, &args.profile, args.strict)?
    };
    let validation = run_review_validation(&paths)?;
    let dry_run_push = if draft_selection.is_some() {
        ReviewDryRunPush {
            attempted: false,
            success: true,
            report: None,
            error: None,
            skipped_reason: Some(
                "draft review skips push dry-run; promote the draft under wiki_content/ before push"
                    .to_string(),
            ),
        }
    } else {
        run_review_push_dry_run(
            &paths,
            &config,
            &selection,
            args.summary.trim(),
            args.templates,
            args.categories,
        )
    };

    let mut hard_failures = Vec::new();
    if !status_plan.sync_ledger_ready {
        hard_failures.push("sync ledger is missing; run `wikitool pull --full`".to_string());
    }
    if !changed_article_lint.sync_ledger_ready {
        hard_failures.push("changed article lint could not resolve the sync ledger".to_string());
    }
    if changed_article_lint.total_errors > 0 {
        hard_failures.push(format!(
            "changed article lint reported {} error(s)",
            changed_article_lint.total_errors
        ));
    }
    if args.strict && changed_article_lint.total_warnings > 0 {
        hard_failures.push(format!(
            "changed article lint reported {} warning(s) under --strict",
            changed_article_lint.total_warnings
        ));
    }
    if !validation.index_ready {
        hard_failures
            .push("validation index is missing; run `wikitool knowledge build`".to_string());
    }
    if !dry_run_push.success {
        hard_failures.push(
            dry_run_push
                .error
                .clone()
                .unwrap_or_else(|| "push dry-run reported conflicts or errors".to_string()),
        );
    }

    let report = ReviewReport {
        project_root: normalize_path(&paths.project_root),
        status: if hard_failures.is_empty() {
            "clean"
        } else {
            "failed"
        },
        hard_failures,
        filters,
        status_plan,
        changed_article_lint,
        validation,
        dry_run_push,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_review_report(&report);
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }

    if report.hard_failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "review failed with {} hard failure(s)",
            report.hard_failures.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_support::OutputFormat;

    fn review_args() -> ReviewArgs {
        ReviewArgs {
            format: OutputFormat::Json,
            profile: "remilia".to_string(),
            strict: false,
            templates: false,
            categories: false,
            titles: Vec::new(),
            paths: Vec::new(),
            draft_paths: Vec::new(),
            titles_file: None,
            summary: "test".to_string(),
        }
    }

    #[test]
    fn review_draft_selection_requires_exactly_one_title() {
        let mut args = review_args();
        args.draft_paths
            .push(PathBuf::from(".wikitool/drafts/Cheetah.wiki"));

        let error = review_draft_selection_from_args(&args).unwrap_err();

        assert!(error.to_string().contains("requires exactly one --title"));
    }

    #[test]
    fn review_draft_selection_rejects_sync_path_mix() {
        let mut args = review_args();
        args.titles.push("Cheetah".to_string());
        args.draft_paths
            .push(PathBuf::from(".wikitool/drafts/Cheetah.wiki"));
        args.paths
            .push(PathBuf::from("wiki_content/Main/Cheetah.wiki"));

        let error = review_draft_selection_from_args(&args).unwrap_err();

        assert!(error.to_string().contains("cannot be combined"));
    }

    #[test]
    fn review_draft_selection_accepts_one_draft_and_title() {
        let mut args = review_args();
        args.titles.push("Cheetah".to_string());
        args.draft_paths
            .push(PathBuf::from(".wikitool/drafts/Cheetah.wiki"));

        let selection = review_draft_selection_from_args(&args)
            .expect("draft selection")
            .expect("present");

        assert_eq!(selection.title, "Cheetah");
        assert_eq!(
            selection.path,
            PathBuf::from(".wikitool/drafts/Cheetah.wiki")
        );
    }
}

fn run_changed_article_lint(
    paths: &wikitool_core::runtime::ResolvedPaths,
    selection: &SyncSelection,
    profile: &str,
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
        .map(|relative_path| lint_article(paths, Path::new(relative_path), Some(profile)))
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

fn run_review_validation(
    paths: &wikitool_core::runtime::ResolvedPaths,
) -> Result<ReviewValidation> {
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

fn run_review_push_dry_run(
    paths: &wikitool_core::runtime::ResolvedPaths,
    config: &wikitool_core::config::WikiConfig,
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

#[derive(Debug, Clone)]
struct DraftReviewSelection {
    title: String,
    path: PathBuf,
}

fn review_draft_selection_from_args(args: &ReviewArgs) -> Result<Option<DraftReviewSelection>> {
    if args.draft_paths.is_empty() {
        return Ok(None);
    }
    if args.draft_paths.len() != 1 {
        bail!("review --draft-path accepts exactly one draft path");
    }
    if args.titles.len() != 1 {
        bail!("review --draft-path requires exactly one --title");
    }
    if !args.paths.is_empty() || args.titles_file.is_some() {
        bail!("review --draft-path cannot be combined with --path or --titles-file");
    }
    if args.templates || args.categories {
        bail!("review --draft-path cannot be combined with --templates or --categories");
    }
    Ok(Some(DraftReviewSelection {
        title: args.titles[0].clone(),
        path: args.draft_paths[0].clone(),
    }))
}

fn run_draft_article_lint(
    paths: &wikitool_core::runtime::ResolvedPaths,
    selection: &DraftReviewSelection,
    profile: &str,
    strict: bool,
) -> Result<ReviewArticleLint> {
    let report = lint_article_with_title(
        paths,
        &selection.path,
        Some(profile),
        Some(&selection.title),
    )?;
    let total_errors = report.errors;
    let total_warnings = report.warnings;
    let total_suggestions = report.suggestions;
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
        target_count: 1,
        total_errors,
        total_warnings,
        total_suggestions,
        reports: vec![report],
        error,
    })
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

fn review_selection_from_args(
    titles: &[String],
    paths: &[PathBuf],
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
        paths: paths.iter().map(normalize_path).collect(),
    })
}

fn print_review_report(report: &ReviewReport) {
    println!("review");
    println!("project_root: {}", report.project_root);
    println!("status: {}", report.status);
    println!("mode: {}", report.filters.mode);
    println!("profile: {}", report.filters.profile);
    println!("strict: {}", report.filters.strict);
    println!("templates: {}", report.filters.templates);
    println!("categories: {}", report.filters.categories);
    if !report.filters.selection.titles.is_empty() {
        println!(
            "selection.titles: {}",
            report.filters.selection.titles.join(" | ")
        );
    }
    if !report.filters.selection.paths.is_empty() {
        println!(
            "selection.paths: {}",
            report.filters.selection.paths.join(" | ")
        );
    }
    if !report.filters.draft_paths.is_empty() {
        println!("draft_paths: {}", report.filters.draft_paths.join(" | "));
    }
    println!(
        "status.sync_ledger_ready: {}",
        report.status_plan.sync_ledger_ready
    );
    println!(
        "status.selection_state: {}",
        report.status_plan.selection_state
    );
    println!(
        "status.selected_change_count: {}",
        report.status_plan.selected_change_count
    );
    if let Some(plan) = &report.status_plan.plan {
        println!("status.new_local: {}", plan.new_local);
        println!("status.modified_local: {}", plan.modified_local);
        println!("status.deleted_local: {}", plan.deleted_local);
        println!("status.total: {}", plan.changes.len());
        println!("status.conflicts.count: {}", plan.conflict_count);
    }
    println!(
        "article_lint.sync_ledger_ready: {}",
        report.changed_article_lint.sync_ledger_ready
    );
    println!(
        "article_lint.target_count: {}",
        report.changed_article_lint.target_count
    );
    println!(
        "article_lint.errors: {}",
        report.changed_article_lint.total_errors
    );
    println!(
        "article_lint.warnings: {}",
        report.changed_article_lint.total_warnings
    );
    println!(
        "article_lint.suggestions: {}",
        report.changed_article_lint.total_suggestions
    );
    println!("validation.index_ready: {}", report.validation.index_ready);
    println!("validation.issue_count: {}", report.validation.issue_count);
    if let Some(summary) = &report.validation.summary {
        println!("validation.broken_links.count: {}", summary.broken_links);
        println!(
            "validation.double_redirects.count: {}",
            summary.double_redirects
        );
        println!(
            "validation.uncategorized_pages.count: {}",
            summary.uncategorized_pages
        );
        println!("validation.orphan_pages.count: {}", summary.orphan_pages);
    }
    println!("push_dry_run.attempted: {}", report.dry_run_push.attempted);
    println!("push_dry_run.success: {}", report.dry_run_push.success);
    if let Some(reason) = &report.dry_run_push.skipped_reason {
        println!("push_dry_run.skipped_reason: {reason}");
    }
    if let Some(push) = &report.dry_run_push.report {
        println!("push_dry_run.pages: {}", push.pages.len());
        println!("push_dry_run.conflicts: {}", push.conflicts.len());
        println!("push_dry_run.errors: {}", push.errors.len());
    }
    if let Some(error) = &report.dry_run_push.error {
        println!("push_dry_run.error: {error}");
    }
    if report.hard_failures.is_empty() {
        println!("hard_failures: <none>");
    } else {
        for failure in &report.hard_failures {
            println!("hard_failure: {failure}");
        }
    }
}
