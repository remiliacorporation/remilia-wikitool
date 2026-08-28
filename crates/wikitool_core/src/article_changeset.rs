use std::path::Path;

use anyhow::Result;

use crate::article_lint::{
    ArticleLintResources, lint_article_with_resources, load_article_lint_resources,
};
use crate::filesystem::title_to_relative_path;
use crate::publication::{publication_workspace, resolve_article_publication_authority};
use crate::runtime::ResolvedPaths;

pub use wikitool_publication::{
    ARTICLE_CHANGESET_DECISION_SCHEMA_VERSION, ARTICLE_REVIEW_CHANGESET_SCHEMA_VERSION,
    ArticleChangesetAcceptanceResult, ArticleChangesetDecisionItem,
    ArticleChangesetDecisionReceipt, ArticleChangesetWarningPolicy, ArticleReviewChangeset,
    ArticleReviewChangesetInput, ArticleReviewChangesetItem, ArticleReviewLintSnapshot,
};

struct CoreReviewEvidence<'a> {
    paths: &'a ResolvedPaths,
    resources: ArticleLintResources,
}

impl wikitool_publication::ArticleReviewEvidenceProvider for CoreReviewEvidence<'_> {
    fn target_relative_path(&self, title: &str) -> Result<String> {
        title_to_relative_path(self.paths, title, false)
    }

    fn lint(&self, source_path: &Path, title: &str) -> Result<ArticleReviewLintSnapshot> {
        let report =
            lint_article_with_resources(self.paths, source_path, Some(title), &self.resources)?;
        Ok(ArticleReviewLintSnapshot {
            site_adapter_id: report.site_adapter_id,
            content_sha256: report.content_sha256,
            errors: report.errors,
            warnings: report.warnings,
            suggestions: report.suggestions,
            issues: report
                .issues
                .into_iter()
                .map(serde_json::to_value)
                .collect::<serde_json::Result<Vec<_>>>()?,
        })
    }
}

fn evidence(paths: &ResolvedPaths) -> Result<CoreReviewEvidence<'_>> {
    Ok(CoreReviewEvidence {
        paths,
        resources: load_article_lint_resources(paths)?,
    })
}

pub fn prepare_article_review_changeset(
    paths: &ResolvedPaths,
    manifest_path: &Path,
    inputs: Vec<ArticleReviewChangesetInput>,
    replace: bool,
) -> Result<ArticleReviewChangeset> {
    wikitool_publication::prepare_article_review_changeset(
        &publication_workspace(paths),
        &resolve_article_publication_authority(paths)?,
        &evidence(paths)?,
        manifest_path,
        inputs,
        replace,
    )
}

pub fn accept_article_review_changeset(
    paths: &ResolvedPaths,
    manifest_path: &Path,
    human_editor_claim: &str,
    warning_policy: ArticleChangesetWarningPolicy,
) -> Result<ArticleChangesetAcceptanceResult> {
    wikitool_publication::accept_article_review_changeset(
        &publication_workspace(paths),
        &resolve_article_publication_authority(paths)?,
        &evidence(paths)?,
        manifest_path,
        human_editor_claim,
        warning_policy,
    )
}
