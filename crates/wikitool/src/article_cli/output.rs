use wikitool_core::article_lint::{ArticleFixResult, ArticleLintReport};

use super::selection::ArticleTargetSelection;
pub(super) fn print_article_target_selection(selection: &ArticleTargetSelection) {
    println!("selection.changed: {}", flag(selection.changed));
    if selection.titles.is_empty() {
        println!("selection.titles: <none>");
    } else {
        println!("selection.titles: {}", selection.titles.join(" | "));
    }
    if selection.paths.is_empty() {
        println!("selection.paths: <none>");
    } else {
        println!("selection.paths: {}", selection.paths.join(" | "));
    }
}

pub(super) fn print_report(report: &ArticleLintReport) {
    println!("relative_path: {}", report.relative_path);
    println!("title: {}", report.title);
    println!("namespace: {}", report.namespace);
    println!("profile_id: {}", report.profile_id);
    println!(
        "capabilities_loaded: {}",
        flag(report.resources.capabilities_loaded)
    );
    println!(
        "template_catalog_loaded: {}",
        flag(report.resources.template_catalog_loaded)
    );
    println!("index_ready: {}", flag(report.resources.index_ready));
    println!("graph_ready: {}", flag(report.resources.graph_ready));
    println!("errors: {}", report.errors);
    println!("warnings: {}", report.warnings);
    println!("suggestions: {}", report.suggestions);
    if report.issues.is_empty() {
        println!("issues: <none>");
        return;
    }
    for issue in &report.issues {
        let line = issue
            .span
            .as_ref()
            .map(|span| span.line.to_string())
            .unwrap_or_else(|| "<none>".to_string());
        let evidence = issue.evidence.as_deref().unwrap_or("<none>");
        let remediation = issue.suggested_remediation.as_deref().unwrap_or("<none>");
        println!(
            "issue: severity={} rule={} line={} message={} evidence={} remediation={}",
            issue.severity.as_str(),
            issue.rule_id,
            line,
            issue.message,
            evidence,
            remediation
        );
    }
}

pub(super) fn print_fix_result(result: &ArticleFixResult) {
    println!("relative_path: {}", result.relative_path);
    println!("title: {}", result.title);
    println!("namespace: {}", result.namespace);
    println!("profile_id: {}", result.profile_id);
    println!("apply_mode: {}", result.apply_mode);
    println!("changed: {}", flag(result.changed));
    println!("applied_fix_count: {}", result.applied_fix_count);
    if result.applied_fixes.is_empty() {
        println!("applied_fixes: <none>");
    } else {
        for fix in &result.applied_fixes {
            println!(
                "applied_fix: rule={} line={} label={}",
                fix.rule_id,
                fix.line
                    .map(|line| line.to_string())
                    .unwrap_or_else(|| "<none>".to_string()),
                fix.label
            );
        }
    }
    println!(
        "remaining: errors={} warnings={} suggestions={}",
        result.remaining_report.errors,
        result.remaining_report.warnings,
        result.remaining_report.suggestions
    );
}

pub(super) fn flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}
