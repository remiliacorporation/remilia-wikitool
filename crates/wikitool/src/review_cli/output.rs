use serde::Serialize;

use crate::briefs::{BriefCommand, brief_command};

use super::{ReviewInterviewBrief, ReviewNextStep, ReviewReport};

#[derive(Debug, Serialize)]
pub(super) struct ReviewBrief<'a> {
    schema_version: &'static str,
    command: &'static str,
    view: &'static str,
    status: &'a str,
    mode: &'a str,
    strict: bool,
    selection: ReviewSelectionBrief<'a>,
    counts: ReviewCountsBrief,
    interview_brief: Option<ReviewInterviewBriefCard<'a>>,
    dry_run_push: ReviewDryRunBrief<'a>,
    hard_failures: &'a [String],
    next_steps: &'a [ReviewNextStep],
    full_view_command: BriefCommand,
}

#[derive(Debug, Serialize)]
struct ReviewSelectionBrief<'a> {
    selected_change_count: usize,
    selection_state: &'a str,
    titles: &'a [String],
    paths: &'a [String],
    draft_paths: &'a [String],
}

#[derive(Debug, Serialize)]
struct ReviewCountsBrief {
    lint_targets: usize,
    lint_errors: usize,
    lint_warnings: usize,
    lint_suggestions: usize,
    validation_index_ready: bool,
    validation_issues: usize,
    sync_conflicts: usize,
}

#[derive(Debug, Serialize)]
struct ReviewInterviewBriefCard<'a> {
    status: &'a wikitool_core::knowledge_interview::InterviewValidationStatus,
    path: &'a str,
    title: Option<&'a str>,
    intent: Option<&'a str>,
    computed_freshness: &'a str,
    pending_claims: usize,
    source_leads: usize,
    open_items: usize,
    negative_evidence: usize,
    errors: &'a [String],
    warnings: &'a [String],
}

#[derive(Debug, Serialize)]
struct ReviewDryRunBrief<'a> {
    attempted: bool,
    success: bool,
    skipped_reason: Option<&'a str>,
    error: Option<&'a str>,
    page_count: usize,
    conflict_count: usize,
    error_count: usize,
}

pub(super) fn build_review_brief(report: &ReviewReport) -> ReviewBrief<'_> {
    ReviewBrief {
        schema_version: "wikitool_brief_v1",
        command: "review",
        view: "brief",
        status: report.status,
        mode: report.filters.mode,
        strict: report.filters.strict,
        selection: ReviewSelectionBrief {
            selected_change_count: report.status_plan.selected_change_count,
            selection_state: report.status_plan.selection_state,
            titles: &report.filters.selection.titles,
            paths: &report.filters.selection.paths,
            draft_paths: &report.filters.draft_paths,
        },
        counts: ReviewCountsBrief {
            lint_targets: report.changed_article_lint.target_count,
            lint_errors: report.changed_article_lint.total_errors,
            lint_warnings: report.changed_article_lint.total_warnings,
            lint_suggestions: report.changed_article_lint.total_suggestions,
            validation_index_ready: report.validation.index_ready,
            validation_issues: report.validation.issue_count,
            sync_conflicts: report
                .status_plan
                .plan
                .as_ref()
                .map(|plan| plan.conflict_count)
                .unwrap_or_default(),
        },
        interview_brief: report.interview_brief.as_ref().map(interview_brief_card),
        dry_run_push: ReviewDryRunBrief {
            attempted: report.dry_run_push.attempted,
            success: report.dry_run_push.success,
            skipped_reason: report.dry_run_push.skipped_reason.as_deref(),
            error: report.dry_run_push.error.as_deref(),
            page_count: report
                .dry_run_push
                .report
                .as_ref()
                .map(|push| push.pages.len())
                .unwrap_or_default(),
            conflict_count: report
                .dry_run_push
                .report
                .as_ref()
                .map(|push| push.conflicts.len())
                .unwrap_or_default(),
            error_count: report
                .dry_run_push
                .report
                .as_ref()
                .map(|push| push.errors.len())
                .unwrap_or_default(),
        },
        hard_failures: &report.hard_failures,
        next_steps: &report.next_steps,
        full_view_command: brief_command(&[
            "wikitool",
            "review",
            "--format",
            "json",
            "--view",
            "full",
            "--summary",
            "<summary>",
        ]),
    }
}

fn interview_brief_card(brief: &ReviewInterviewBrief) -> ReviewInterviewBriefCard<'_> {
    ReviewInterviewBriefCard {
        status: &brief.status,
        path: &brief.path,
        title: brief.summary.title.as_deref(),
        intent: brief.summary.intent.as_deref(),
        computed_freshness: &brief.summary.computed_freshness,
        pending_claims: brief.summary.claim_counts.pending_corroboration,
        source_leads: brief.summary.source_lead_count,
        open_items: brief.summary.open_item_count,
        negative_evidence: brief.summary.open_item_counts.negative_evidence,
        errors: &brief.errors,
        warnings: &brief.warnings,
    }
}

pub(super) fn print_review_report(report: &ReviewReport) {
    println!("review");
    println!("project_root: {}", report.project_root);
    println!("status: {}", report.status);
    println!("mode: {}", report.filters.mode);
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
    if let Some(brief) = &report.interview_brief {
        println!("interview_brief.path: {}", brief.path);
        println!(
            "interview_brief.status: {}",
            match brief.status {
                wikitool_core::knowledge_interview::InterviewValidationStatus::Valid => "valid",
                wikitool_core::knowledge_interview::InterviewValidationStatus::Warning => "warning",
                wikitool_core::knowledge_interview::InterviewValidationStatus::Invalid => "invalid",
            }
        );
        if let Some(title) = &brief.summary.title {
            println!("interview_brief.title: {title}");
        }
        println!(
            "interview_brief.computed_freshness: {}",
            brief.summary.computed_freshness
        );
        println!(
            "interview_brief.claims.pending_corroboration: {}",
            brief.summary.claim_counts.pending_corroboration
        );
        println!(
            "interview_brief.source_leads: {}",
            brief.summary.source_lead_count
        );
        println!(
            "interview_brief.open_items: {}",
            brief.summary.open_item_count
        );
        println!(
            "interview_brief.negative_evidence: {}",
            brief.summary.open_item_counts.negative_evidence
        );
        for error in &brief.errors {
            println!("interview_brief.error: {error}");
        }
        for warning in &brief.warnings {
            println!("interview_brief.warning: {warning}");
        }
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
    println!("next_steps.count: {}", report.next_steps.len());
    for step in &report.next_steps {
        println!("next_step.kind: {}", step.kind);
        println!("next_step.description: {}", step.description);
        if let Some(command) = &step.command {
            println!("next_step.command: {}", command.display);
        }
        if let Some(target_path) = &step.target_path {
            println!("next_step.target_path: {target_path}");
        }
    }
    if report.hard_failures.is_empty() {
        println!("hard_failures: <none>");
    } else {
        for failure in &report.hard_failures {
            println!("hard_failure: {failure}");
        }
    }
}
