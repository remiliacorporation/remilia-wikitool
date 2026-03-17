use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use wikitool_core::article_lint::{
    ArticleFixApplyMode, ArticleFixResult, ArticleLintReport, fix_article, lint_article,
};

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::query_cli::normalize_output;
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct ArticleArgs {
    #[command(subcommand)]
    command: ArticleSubcommand,
}

#[derive(Debug, Subcommand)]
enum ArticleSubcommand {
    #[command(about = "Lint article wikitext against wiki/profile rules")]
    Lint(ArticleLintArgs),
    #[command(about = "Apply safe mechanical fixes to article wikitext")]
    Fix(ArticleFixArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ArticleLintArgs {
    path: PathBuf,
    #[arg(long, default_value = "remilia", value_name = "PROFILE")]
    profile: String,
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    format: String,
    #[arg(long, help = "Treat warnings as errors")]
    strict: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ArticleFixArgs {
    path: PathBuf,
    #[arg(long, default_value = "remilia", value_name = "PROFILE")]
    profile: String,
    #[arg(
        long,
        default_value = "none",
        value_name = "MODE",
        help = "Apply mode: none|safe"
    )]
    apply: String,
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    format: String,
}

pub(crate) fn run_article(runtime: &RuntimeOptions, args: ArticleArgs) -> Result<()> {
    match args.command {
        ArticleSubcommand::Lint(args) => run_article_lint(runtime, args),
        ArticleSubcommand::Fix(args) => run_article_fix(runtime, args),
    }
}

fn run_article_lint(runtime: &RuntimeOptions, args: ArticleLintArgs) -> Result<()> {
    let format = normalize_output(&args.format)?;
    let paths = resolve_runtime_paths(runtime)?;
    let report = lint_article(&paths, &args.path, Some(&args.profile))?;

    if format == "json" {
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
    Ok(())
}

fn run_article_fix(runtime: &RuntimeOptions, args: ArticleFixArgs) -> Result<()> {
    let format = normalize_output(&args.format)?;
    let paths = resolve_runtime_paths(runtime)?;
    let apply_mode = parse_apply_mode(&args.apply)?;
    let result = fix_article(&paths, &args.path, Some(&args.profile), apply_mode)?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("article fix");
        println!("project_root: {}", normalize_path(&paths.project_root));
        print_fix_result(&result);
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }

    if result.remaining_report.errors > 0 {
        bail!(
            "article fix left {} error(s), {} warning(s), and {} suggestion(s)",
            result.remaining_report.errors,
            result.remaining_report.warnings,
            result.remaining_report.suggestions
        );
    }
    Ok(())
}

fn parse_apply_mode(value: &str) -> Result<ArticleFixApplyMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Ok(ArticleFixApplyMode::None),
        "safe" => Ok(ArticleFixApplyMode::Safe),
        other => bail!("unsupported article fix apply mode: {other} (expected none|safe)"),
    }
}

fn print_report(report: &ArticleLintReport) {
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

fn print_fix_result(result: &ArticleFixResult) {
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

fn flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}
