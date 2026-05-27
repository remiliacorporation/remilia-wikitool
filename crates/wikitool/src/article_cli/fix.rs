use std::path::Path;

use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::article_lint::{
    ArticleFixApplyMode, ArticleFixResult, fix_article, fix_article_with_title,
};

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::{print_article_target_selection, print_fix_result};
use super::selection::{
    ArticleTargetSelection, article_selection_from_args, resolve_article_targets,
    single_state_path_title_override, uses_single_path_mode,
};
use super::*;
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

pub(super) fn run_article_fix(runtime: &RuntimeOptions, args: ArticleFixArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let apply_mode = ArticleFixApplyMode::from(args.apply);
    if let Some(title_override) = single_state_path_title_override(
        &paths,
        args.path.as_deref(),
        &args.titles,
        &args.paths,
        args.titles_file.as_ref(),
        args.changed,
    )? {
        let result = fix_article_with_title(
            &paths,
            args.path.as_deref().expect("single path"),
            Some(&args.profile),
            apply_mode,
            Some(title_override),
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
