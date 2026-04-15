use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use wikitool_core::article_lint::{
    ArticleFixApplyMode, ArticleFixResult, ArticleLintReport, fix_article, lint_article,
};
use wikitool_core::filesystem::{
    relative_path_to_title, title_to_relative_path, validate_scoped_path,
};
use wikitool_core::sync::{SyncSelection, collect_changed_article_paths};

use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_paths};
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
    path: Option<PathBuf>,
    #[arg(long, default_value = "remilia", value_name = "PROFILE")]
    profile: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
    #[arg(long, help = "Treat warnings as errors")]
    strict: bool,
    #[arg(long = "title", value_name = "TITLE")]
    titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    titles_file: Option<PathBuf>,
    #[arg(long, help = "Lint the current changed main-namespace article set")]
    changed: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ArticleFixArgs {
    path: Option<PathBuf>,
    #[arg(long, default_value = "remilia", value_name = "PROFILE")]
    profile: String,
    #[arg(
        long,
        value_enum,
        default_value_t = ArticleFixApplyArg::None,
        value_name = "MODE",
        help = "Apply mode: none|safe"
    )]
    apply: ArticleFixApplyArg,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
    #[arg(long = "title", value_name = "TITLE")]
    titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    titles_file: Option<PathBuf>,
    #[arg(long, help = "Fix the current changed main-namespace article set")]
    changed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ArticleTargetSelection {
    changed: bool,
    titles: Vec<String>,
    paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ArticleLintBatchReport {
    project_root: String,
    profile: String,
    strict: bool,
    selection: ArticleTargetSelection,
    target_count: usize,
    total_errors: usize,
    total_warnings: usize,
    total_suggestions: usize,
    reports: Vec<ArticleLintReport>,
}

#[derive(Debug, Serialize)]
struct ArticleFixBatchReport {
    project_root: String,
    profile: String,
    apply_mode: String,
    selection: ArticleTargetSelection,
    target_count: usize,
    changed_files: usize,
    applied_fix_count: usize,
    remaining_errors: usize,
    remaining_warnings: usize,
    remaining_suggestions: usize,
    results: Vec<ArticleFixResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ArticleFixApplyArg {
    None,
    Safe,
}

impl ArticleFixApplyArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Safe => "safe",
        }
    }
}

impl From<ArticleFixApplyArg> for ArticleFixApplyMode {
    fn from(value: ArticleFixApplyArg) -> Self {
        match value {
            ArticleFixApplyArg::None => Self::None,
            ArticleFixApplyArg::Safe => Self::Safe,
        }
    }
}

impl std::fmt::Display for ArticleFixApplyArg {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub(crate) fn run_article(runtime: &RuntimeOptions, args: ArticleArgs) -> Result<()> {
    match args.command {
        ArticleSubcommand::Lint(args) => run_article_lint(runtime, args),
        ArticleSubcommand::Fix(args) => run_article_fix(runtime, args),
    }
}

fn run_article_lint(runtime: &RuntimeOptions, args: ArticleLintArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    if uses_single_path_mode(
        args.path.as_deref(),
        &args.titles,
        &args.paths,
        args.titles_file.as_ref(),
        args.changed,
    ) {
        let report = lint_article(
            &paths,
            args.path.as_deref().expect("single path"),
            Some(&args.profile),
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

    let selection = article_selection_from_args(
        &args.titles,
        &args.paths,
        args.titles_file.as_ref(),
        args.changed,
    )?;
    let target_paths = resolve_article_targets(&paths, args.path.as_deref(), &selection, false)?;
    let reports = target_paths
        .iter()
        .map(|relative_path| lint_article(&paths, Path::new(relative_path), Some(&args.profile)))
        .collect::<Result<Vec<_>>>()?;
    let total_errors = reports.iter().map(|report| report.errors).sum();
    let total_warnings = reports.iter().map(|report| report.warnings).sum();
    let total_suggestions = reports.iter().map(|report| report.suggestions).sum();
    let batch_report = ArticleLintBatchReport {
        project_root: normalize_path(&paths.project_root),
        profile: args.profile.clone(),
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
        println!("profile: {}", batch_report.profile);
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

fn run_article_fix(runtime: &RuntimeOptions, args: ArticleFixArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let apply_mode = ArticleFixApplyMode::from(args.apply);
    if uses_single_path_mode(
        args.path.as_deref(),
        &args.titles,
        &args.paths,
        args.titles_file.as_ref(),
        args.changed,
    ) {
        let result = fix_article(
            &paths,
            args.path.as_deref().expect("single path"),
            Some(&args.profile),
            apply_mode,
        )?;

        if args.format.is_json() {
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
        return Ok(());
    }

    if apply_mode != ArticleFixApplyMode::Safe {
        bail!("article fix batch mode requires --apply safe");
    }

    let selection = article_selection_from_args(
        &args.titles,
        &args.paths,
        args.titles_file.as_ref(),
        args.changed,
    )?;
    let target_paths = resolve_article_targets(&paths, args.path.as_deref(), &selection, true)?;
    let results = target_paths
        .iter()
        .map(|relative_path| {
            fix_article(
                &paths,
                Path::new(relative_path),
                Some(&args.profile),
                apply_mode,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let changed_files = results.iter().filter(|result| result.changed).count();
    let applied_fix_count = results.iter().map(|result| result.applied_fix_count).sum();
    let remaining_errors = results
        .iter()
        .map(|result| result.remaining_report.errors)
        .sum();
    let remaining_warnings = results
        .iter()
        .map(|result| result.remaining_report.warnings)
        .sum();
    let remaining_suggestions = results
        .iter()
        .map(|result| result.remaining_report.suggestions)
        .sum();
    let batch_report = ArticleFixBatchReport {
        project_root: normalize_path(&paths.project_root),
        profile: args.profile.clone(),
        apply_mode: apply_mode.as_str().to_string(),
        selection,
        target_count: results.len(),
        changed_files,
        applied_fix_count,
        remaining_errors,
        remaining_warnings,
        remaining_suggestions,
        results,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&batch_report)?);
    } else {
        println!("article fix");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("profile: {}", batch_report.profile);
        println!("apply_mode: {}", batch_report.apply_mode);
        print_article_target_selection(&batch_report.selection);
        println!("target_count: {}", batch_report.target_count);
        println!("changed_files: {}", batch_report.changed_files);
        println!("applied_fix_count: {}", batch_report.applied_fix_count);
        println!("remaining_errors: {}", batch_report.remaining_errors);
        println!("remaining_warnings: {}", batch_report.remaining_warnings);
        println!(
            "remaining_suggestions: {}",
            batch_report.remaining_suggestions
        );
        if batch_report.results.is_empty() {
            println!("results: <none>");
        } else {
            for result in &batch_report.results {
                println!();
                print_fix_result(result);
            }
        }
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }

    if batch_report.remaining_errors > 0 {
        bail!(
            "article fix left {} error(s), {} warning(s), and {} suggestion(s) across {} file(s)",
            batch_report.remaining_errors,
            batch_report.remaining_warnings,
            batch_report.remaining_suggestions,
            batch_report.target_count
        );
    }
    Ok(())
}

fn uses_single_path_mode(
    path: Option<&Path>,
    titles: &[String],
    paths: &[PathBuf],
    titles_file: Option<&PathBuf>,
    changed: bool,
) -> bool {
    path.is_some() && titles.is_empty() && paths.is_empty() && titles_file.is_none() && !changed
}

fn article_selection_from_args(
    titles: &[String],
    paths: &[PathBuf],
    titles_file: Option<&PathBuf>,
    changed: bool,
) -> Result<ArticleTargetSelection> {
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

    Ok(ArticleTargetSelection {
        changed,
        titles: loaded_titles,
        paths: paths.iter().map(normalize_path).collect(),
    })
}

fn resolve_article_targets(
    paths: &wikitool_core::runtime::ResolvedPaths,
    positional_path: Option<&Path>,
    selection: &ArticleTargetSelection,
    include_selected_redirects: bool,
) -> Result<Vec<String>> {
    let mut target_paths = BTreeSet::new();
    if let Some(path) = positional_path {
        target_paths.insert(resolve_article_selector_path(paths, path)?);
    }

    let sync_selection = SyncSelection {
        titles: selection.titles.clone(),
        paths: selection.paths.clone(),
    };
    if selection.changed {
        let Some(changed_paths) =
            collect_changed_article_paths(paths, &sync_selection, include_selected_redirects)?
        else {
            bail!("article --changed requires a built sync ledger (run `wikitool pull --full`)");
        };
        for relative_path in changed_paths {
            target_paths.insert(relative_path);
        }
    } else {
        for title in &selection.titles {
            target_paths.insert(resolve_article_title(paths, title)?);
        }
        for path in &selection.paths {
            target_paths.insert(resolve_article_selector_path(paths, Path::new(path))?);
        }
    }

    if target_paths.is_empty() {
        bail!("article command requires a file path, selector, or --changed");
    }

    Ok(target_paths.into_iter().collect())
}

fn resolve_article_title(
    paths: &wikitool_core::runtime::ResolvedPaths,
    title: &str,
) -> Result<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        bail!("article title selectors must not be empty");
    }

    for is_redirect in [false, true] {
        let relative_path = title_to_relative_path(paths, trimmed, is_redirect)?;
        let absolute_path = paths.project_root.join(&relative_path);
        if absolute_path.exists() {
            return Ok(relative_path);
        }
    }

    bail!("no local article file found for title: {trimmed}")
}

fn resolve_article_selector_path(
    paths: &wikitool_core::runtime::ResolvedPaths,
    path: &Path,
) -> Result<String> {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    validate_scoped_path(paths, &absolute_path)?;
    if !absolute_path.exists() {
        bail!(
            "article path does not exist: {}",
            normalize_path(&absolute_path)
        );
    }
    let relative_path = absolute_path
        .strip_prefix(&paths.project_root)
        .with_context(|| {
            format!(
                "failed to resolve {} relative to {}",
                normalize_path(&absolute_path),
                normalize_path(&paths.project_root)
            )
        })?;
    let relative_path = normalize_path(relative_path);
    if !relative_path.starts_with("wiki_content/") {
        bail!(
            "article command only supports files under wiki_content/: {}",
            relative_path
        );
    }
    let _ = relative_path_to_title(paths, &relative_path)?;
    Ok(relative_path)
}

fn print_article_target_selection(selection: &ArticleTargetSelection) {
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
