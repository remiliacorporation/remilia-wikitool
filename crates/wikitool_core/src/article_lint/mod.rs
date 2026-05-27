mod document;
mod fix;
mod model;
mod resources;
mod rules;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::runtime::ResolvedPaths;

pub use model::{
    AppliedFixRecord, ArticleFixApplyMode, ArticleFixResult, ArticleLintIssue, ArticleLintReport,
    ArticleLintResourcesStatus, ArticleLintSeverity, SuggestedFix, SuggestedFixKind, TextSpan,
};

use document::{ParsedArticleDocument, load_article_document_with_title};
use fix::apply_text_edits;
use resources::{LoadedResources, load_resources};
use rules::{IssueMatch, SafeFixEdit, collect_issue_matches};

#[cfg(test)]
use crate::knowledge::status::KNOWLEDGE_GENERATION;
#[cfg(test)]
use crate::profile::WikiCapabilityManifest;
#[cfg(test)]
use crate::schema::open_initialized_database_connection;

const ARTICLE_LINT_SCHEMA_VERSION: &str = "article_lint_v1";
const ARTICLE_FIX_SCHEMA_VERSION: &str = "article_fix_v1";
const REMILIA_PROFILE_ID: &str = "remilia";

pub fn lint_article(
    paths: &ResolvedPaths,
    article_path: &Path,
    profile_id: Option<&str>,
) -> Result<ArticleLintReport> {
    lint_article_with_title(paths, article_path, profile_id, None)
}

pub fn lint_article_with_title(
    paths: &ResolvedPaths,
    article_path: &Path,
    profile_id: Option<&str>,
    title_override: Option<&str>,
) -> Result<ArticleLintReport> {
    let profile_id = normalize_profile_id(profile_id)?;
    let document = load_article_document_with_title(paths, article_path, title_override)?;
    let resources = load_resources(paths, &profile_id)?;
    let matches = collect_issue_matches(paths, &document, &resources)?;
    Ok(build_report(&document, &profile_id, &resources, matches))
}

pub fn fix_article(
    paths: &ResolvedPaths,
    article_path: &Path,
    profile_id: Option<&str>,
    apply_mode: ArticleFixApplyMode,
) -> Result<ArticleFixResult> {
    fix_article_with_title(paths, article_path, profile_id, apply_mode, None)
}

pub fn fix_article_with_title(
    paths: &ResolvedPaths,
    article_path: &Path,
    profile_id: Option<&str>,
    apply_mode: ArticleFixApplyMode,
    title_override: Option<&str>,
) -> Result<ArticleFixResult> {
    let profile_id = normalize_profile_id(profile_id)?;
    let document = load_article_document_with_title(paths, article_path, title_override)?;
    let resources = load_resources(paths, &profile_id)?;
    let matches = collect_issue_matches(paths, &document, &resources)?;
    let safe_fixes = collect_safe_fixes(&matches);
    let changed = apply_mode == ArticleFixApplyMode::Safe && !safe_fixes.is_empty();
    if changed {
        let new_content = apply_text_edits(
            &document.content,
            &safe_fixes
                .iter()
                .map(|fix| fix.edit.clone())
                .collect::<Vec<_>>(),
        )?;
        let absolute_path = paths.project_root.join(&document.relative_path);
        fs::write(&absolute_path, new_content)
            .with_context(|| format!("failed to write {}", absolute_path.display()))?;
    }

    let remaining_report =
        lint_article_with_title(paths, article_path, Some(&profile_id), title_override)?;
    Ok(ArticleFixResult {
        schema_version: ARTICLE_FIX_SCHEMA_VERSION.to_string(),
        profile_id,
        relative_path: remaining_report.relative_path.clone(),
        title: remaining_report.title.clone(),
        namespace: remaining_report.namespace.clone(),
        apply_mode: apply_mode.as_str().to_string(),
        changed,
        applied_fix_count: if changed { safe_fixes.len() } else { 0 },
        applied_fixes: if changed {
            safe_fixes
                .into_iter()
                .map(|fix| AppliedFixRecord {
                    rule_id: fix.rule_id,
                    label: fix.label,
                    line: fix.line,
                })
                .collect()
        } else {
            Vec::new()
        },
        remaining_report,
    })
}

fn normalize_profile_id(profile_id: Option<&str>) -> Result<String> {
    let profile_id = profile_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(REMILIA_PROFILE_ID);
    if !profile_id.eq_ignore_ascii_case(REMILIA_PROFILE_ID) {
        bail!("unsupported article lint profile: {profile_id} (expected remilia)");
    }
    Ok(REMILIA_PROFILE_ID.to_string())
}

fn build_report(
    document: &ParsedArticleDocument,
    profile_id: &str,
    resources: &LoadedResources,
    matches: Vec<IssueMatch>,
) -> ArticleLintReport {
    let issues = matches
        .into_iter()
        .map(|item| item.issue)
        .collect::<Vec<_>>();
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == ArticleLintSeverity::Error)
        .count();
    let warnings = issues
        .iter()
        .filter(|issue| issue.severity == ArticleLintSeverity::Warning)
        .count();
    let suggestions = issues
        .iter()
        .filter(|issue| issue.severity == ArticleLintSeverity::Suggestion)
        .count();

    ArticleLintReport {
        schema_version: ARTICLE_LINT_SCHEMA_VERSION.to_string(),
        profile_id: profile_id.to_string(),
        relative_path: document.relative_path.clone(),
        title: document.title.clone(),
        namespace: document.namespace.clone(),
        issue_count: issues.len(),
        errors,
        warnings,
        suggestions,
        resources: ArticleLintResourcesStatus {
            capabilities_loaded: resources.capabilities.is_some(),
            template_catalog_loaded: resources.template_catalog.is_some(),
            index_ready: resources.index_connection.is_some(),
            graph_ready: resources.index_connection.is_some(),
        },
        issues,
    }
}

fn collect_safe_fixes(matches: &[IssueMatch]) -> Vec<SafeFixEdit> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for issue in matches {
        for fix in &issue.safe_fixes {
            let key = (
                fix.edit.start,
                fix.edit.end,
                fix.edit.replacement.clone(),
                fix.rule_id.clone(),
                fix.label.clone(),
            );
            if seen.insert(key) {
                out.push(fix.clone());
            }
        }
    }
    out.sort_by(|left, right| {
        left.edit
            .start
            .cmp(&right.edit.start)
            .then(left.edit.end.cmp(&right.edit.end))
            .then(left.label.cmp(&right.label))
    });
    out
}

#[cfg(test)]
mod tests;
