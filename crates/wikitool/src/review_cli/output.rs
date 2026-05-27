use super::ReviewReport;

pub(super) fn print_review_report(report: &ReviewReport) {
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
