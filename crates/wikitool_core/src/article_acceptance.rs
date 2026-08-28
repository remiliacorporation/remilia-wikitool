use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::publication::publication_workspace;
use crate::runtime::ResolvedPaths;

pub use wikitool_publication::{
    ACCEPTANCE_DECISION, ARTICLE_ACCEPTANCE_LEDGER_SCHEMA_VERSION, AcceptedArticle,
    ArticleAcceptanceDecisionBinding, ArticleAcceptanceLedgerEntry, ArticleAcceptanceLintSummary,
    ArticleAcceptanceProvenance, ArticleAcceptanceRequest, ArticleProseOrigin,
    ArticlePublicationAuthority, ArticleWarningDecision, EDITOR_IDENTITY_ASSURANCE,
};

pub fn record_article_acceptance(
    paths: &ResolvedPaths,
    article_path: &Path,
    title: &str,
    target_relative_path: &str,
    human_editor_claim: &str,
    prose_origin: ArticleProseOrigin,
    lint: ArticleAcceptanceLintSummary,
) -> Result<(ArticleAcceptanceLedgerEntry, PathBuf)> {
    wikitool_publication::record_article_acceptance(
        &publication_workspace(paths),
        &resolve_article_publication_authority(paths)?,
        ArticleAcceptanceRequest {
            article_path,
            title,
            target_relative_path,
            human_editor_claim,
            prose_origin,
            lint,
        },
    )
}

pub fn verify_article_acceptance(
    paths: &ResolvedPaths,
    article_path: &Path,
    title: &str,
    target_relative_path: &str,
) -> Result<ArticleAcceptanceLedgerEntry> {
    wikitool_publication::verify_article_acceptance(
        &publication_workspace(paths),
        &resolve_article_publication_authority(paths)?,
        article_path,
        title,
        target_relative_path,
    )
}

pub fn load_accepted_article(
    paths: &ResolvedPaths,
    article_path: &Path,
    title: &str,
    target_relative_path: &str,
) -> Result<AcceptedArticle> {
    wikitool_publication::load_accepted_article(
        &publication_workspace(paths),
        &resolve_article_publication_authority(paths)?,
        article_path,
        title,
        target_relative_path,
    )
}

pub use crate::publication::resolve_article_publication_authority;
