use anyhow::{Result, bail};
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{SyncPlanOptions, SyncSelection, plan_sync_changes_with_config};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::checks::{run_changed_article_lint, run_review_push_dry_run, run_review_validation};
use super::draft::{
    review_draft_selection_from_args, run_draft_article_lint, validate_draft_review_path,
};
use super::next_steps::build_review_next_steps;
use super::output::{build_review_brief, print_review_report};
use super::selection::review_selection_from_args;
use super::{ReviewArgs, ReviewDryRunPush, ReviewFilters, ReviewReport, ReviewStatusPlan};

pub(super) fn run_review(runtime: &RuntimeOptions, args: ReviewArgs) -> Result<()> {
    if args.summary.trim().is_empty() {
        bail!("review requires a non-empty --summary");
    }

    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let runtime_status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &runtime_status)?;
    let draft_selection = review_draft_selection_from_args(&args)?;
    if let Some(selection) = &draft_selection {
        validate_draft_review_path(&paths, &selection.path)?;
    }
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
        run_draft_article_lint(&paths, draft_selection, args.strict)?
    } else {
        run_changed_article_lint(&paths, &selection, args.strict)?
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
    let next_steps =
        build_review_next_steps(&paths, draft_selection.as_ref(), args.summary.trim())?;

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
        next_steps,
    };

    if args.format.is_json() {
        if args.view.is_full() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&build_review_brief(&report))?
            );
        }
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
