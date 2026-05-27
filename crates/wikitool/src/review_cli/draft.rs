use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use wikitool_core::article_lint::lint_article_with_title;
use wikitool_core::filesystem::validate_scoped_path;
use wikitool_core::runtime::ResolvedPaths;

use crate::cli_support::{normalize_path, path_is_under_directory};

use super::{ReviewArgs, ReviewArticleLint};

#[derive(Debug, Clone)]
pub(super) struct DraftReviewSelection {
    pub(super) title: String,
    pub(super) path: PathBuf,
}

pub(super) fn review_draft_selection_from_args(
    args: &ReviewArgs,
) -> Result<Option<DraftReviewSelection>> {
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

pub(super) fn validate_draft_review_path(paths: &ResolvedPaths, draft_path: &Path) -> Result<()> {
    let absolute_path = if draft_path.is_absolute() {
        draft_path.to_path_buf()
    } else {
        paths.project_root.join(draft_path)
    };
    validate_scoped_path(paths, &absolute_path)?;
    if !path_is_under_directory(&absolute_path, &paths.state_dir.join("drafts")) {
        bail!(
            "review --draft-path source must be under the canonical draft directory: {}/drafts/",
            normalize_path(&paths.state_dir)
        );
    }
    Ok(())
}

pub(super) fn run_draft_article_lint(
    paths: &ResolvedPaths,
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
