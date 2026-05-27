use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use serde::Serialize;
use wikitool_core::article_lint::ArticleLintReport;
use wikitool_core::sync::{PushReport, SyncPlanReport, SyncSelection};

use crate::RuntimeOptions;
use crate::cli_support::OutputFormat;

mod checks;
mod draft;
mod next_steps;
mod output;
mod selection;
mod workflow;

#[cfg(test)]
mod tests;

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
        help = "Review one off-wiki draft path under .wikitool/drafts/; requires exactly one --title and skips push dry-run"
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
    next_steps: Vec<ReviewNextStep>,
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

#[derive(Debug, Serialize)]
struct ReviewNextStep {
    kind: &'static str,
    description: String,
    command: Option<ReviewNextStepCommand>,
    target_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReviewNextStepCommand {
    argv: Vec<String>,
    display: String,
}

pub(crate) fn run_review(runtime: &RuntimeOptions, args: ReviewArgs) -> Result<()> {
    workflow::run_review(runtime, args)
}
