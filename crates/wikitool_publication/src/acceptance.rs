use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::PublicationWorkspace;
use crate::support::{compute_sha256, normalize_path, unix_timestamp};

pub const ARTICLE_ACCEPTANCE_LEDGER_SCHEMA_VERSION: &str = "article_acceptance_ledger_v3";
pub const ACCEPTANCE_DECISION: &str = "accepted_for_main_namespace_promotion";
pub const EDITOR_IDENTITY_ASSURANCE: &str = "self_reported_unverified";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleProseOrigin {
    HumanDraft,
    HumanRevision,
    AgentDraft,
    CollaborativeDraft,
    MechanicalConversionOfHumanProse,
    HumanReviewedLegacy,
}

impl ArticleProseOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HumanDraft => "human_draft",
            Self::HumanRevision => "human_revision",
            Self::AgentDraft => "agent_draft",
            Self::CollaborativeDraft => "collaborative_draft",
            Self::MechanicalConversionOfHumanProse => "mechanical_conversion_of_human_prose",
            Self::HumanReviewedLegacy => "human_reviewed_legacy",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleWarningDecision {
    NoWarnings,
    Accepted,
}

impl ArticleWarningDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoWarnings => "no_warnings",
            Self::Accepted => "accepted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArticleAcceptanceDecisionBinding {
    pub decision_id: String,
    pub changeset_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArticlePublicationAuthority {
    pub target_api_url: String,
    pub site_adapter_id: String,
    pub publication_policy_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticleAcceptanceLedgerEntry {
    pub schema_version: String,
    pub title: String,
    pub source_relative_path: String,
    pub target_relative_path: String,
    pub content_sha256: String,
    pub human_editor_claim: String,
    pub editor_identity_assurance: String,
    pub prose_origin: ArticleProseOrigin,
    pub decision: String,
    pub accepted_at_unix: u64,
    pub lint_errors: usize,
    pub lint_warnings: usize,
    pub lint_suggestions: usize,
    pub warnings_explicitly_accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning_decision: Option<ArticleWarningDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changeset_decision: Option<ArticleAcceptanceDecisionBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_authority: Option<ArticlePublicationAuthority>,
}

impl ArticleAcceptanceLedgerEntry {
    pub fn resolved_warning_decision(&self) -> Result<ArticleWarningDecision> {
        let inferred = if self.lint_warnings == 0 {
            ArticleWarningDecision::NoWarnings
        } else if self.warnings_explicitly_accepted {
            ArticleWarningDecision::Accepted
        } else {
            bail!(
                "article acceptance ledger has unresolved lint warnings and no acceptance decision"
            );
        };
        if let Some(recorded) = self.warning_decision
            && recorded != inferred
        {
            bail!("article acceptance ledger warning decision contradicts its lint summary");
        }
        Ok(inferred)
    }

    fn provenance(
        &self,
        decision_id: String,
        changeset_sha256: Option<String>,
    ) -> Result<ArticleAcceptanceProvenance> {
        Ok(ArticleAcceptanceProvenance {
            content_sha256: self.content_sha256.clone(),
            accepted_at_unix: self.accepted_at_unix,
            prose_origin: self.prose_origin,
            editor_identity_assurance: self.editor_identity_assurance.clone(),
            warning_decision: self.resolved_warning_decision()?,
            decision_id,
            changeset_sha256,
            publication_authority: self.publication_authority.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ArticleAcceptanceProvenance {
    pub content_sha256: String,
    pub accepted_at_unix: u64,
    pub prose_origin: ArticleProseOrigin,
    pub editor_identity_assurance: String,
    pub warning_decision: ArticleWarningDecision,
    pub decision_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changeset_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_authority: Option<ArticlePublicationAuthority>,
}

#[derive(Debug, Clone)]
pub struct ArticleAcceptanceLintSummary {
    pub content_sha256: String,
    pub errors: usize,
    pub warnings: usize,
    pub suggestions: usize,
    pub warnings_explicitly_accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedArticle {
    pub ledger_entry: ArticleAcceptanceLedgerEntry,
    /// The exact byte-equivalent UTF-8 text whose hash was accepted. Callers
    /// must consume this snapshot instead of rereading the source path.
    pub content: String,
    /// Immutable transactional decision that authorized this exact content.
    pub decision_id: String,
    /// Present only when the authorization was one member of a changeset.
    pub changeset_sha256: Option<String>,
}

impl AcceptedArticle {
    pub fn provenance(&self) -> Result<ArticleAcceptanceProvenance> {
        self.ledger_entry
            .provenance(self.decision_id.clone(), self.changeset_sha256.clone())
    }
}

pub struct ArticleAcceptanceRequest<'a> {
    pub article_path: &'a Path,
    pub title: &'a str,
    pub target_relative_path: &'a str,
    pub human_editor_claim: &'a str,
    pub prose_origin: ArticleProseOrigin,
    pub lint: ArticleAcceptanceLintSummary,
}

pub fn record_article_acceptance(
    paths: &PublicationWorkspace,
    publication_authority: &ArticlePublicationAuthority,
    request: ArticleAcceptanceRequest<'_>,
) -> Result<(ArticleAcceptanceLedgerEntry, PathBuf)> {
    let ArticleAcceptanceRequest {
        article_path,
        title,
        target_relative_path,
        human_editor_claim,
        prose_origin,
        lint,
    } = request;
    let human_editor_claim = human_editor_claim.trim();
    if human_editor_claim.is_empty() {
        bail!("article acceptance requires a non-empty self-reported human editor claim");
    }
    if lint.errors > 0 {
        bail!("article acceptance requires zero lint errors");
    }
    if lint.warnings > 0 && !lint.warnings_explicitly_accepted {
        bail!(
            "article acceptance found {} warning(s); resolve them or explicitly record their acceptance",
            lint.warnings
        );
    }

    let article_absolute = absolute_scoped_path(paths, article_path)?;
    if !article_absolute.is_file() {
        bail!(
            "article acceptance source does not exist or is not a file: {}",
            normalize_path(&article_absolute)
        );
    }
    validate_target_relative_path(target_relative_path)?;
    let content = fs::read_to_string(&article_absolute)
        .with_context(|| format!("failed to read {}", article_absolute.display()))?;
    let content_sha256 = compute_sha256(&content);
    if lint.content_sha256 != content_sha256 {
        bail!(
            "article changed after lint: linted {}, current {}; lint the exact prose again before acceptance",
            lint.content_sha256,
            content_sha256
        );
    }
    let source_relative_path = article_absolute
        .strip_prefix(&paths.project_root)
        .map(normalize_path)
        .unwrap_or_else(|_| normalize_path(&article_absolute));
    let ledger_entry = ArticleAcceptanceLedgerEntry {
        schema_version: ARTICLE_ACCEPTANCE_LEDGER_SCHEMA_VERSION.to_string(),
        title: title.trim().to_string(),
        source_relative_path,
        target_relative_path: normalize_relative_path(target_relative_path),
        content_sha256,
        human_editor_claim: human_editor_claim.to_string(),
        editor_identity_assurance: EDITOR_IDENTITY_ASSURANCE.to_string(),
        prose_origin,
        decision: ACCEPTANCE_DECISION.to_string(),
        accepted_at_unix: unix_timestamp()?,
        lint_errors: lint.errors,
        lint_warnings: lint.warnings,
        lint_suggestions: lint.suggestions,
        warnings_explicitly_accepted: lint.warnings_explicitly_accepted,
        warning_decision: Some(if lint.warnings == 0 {
            ArticleWarningDecision::NoWarnings
        } else {
            ArticleWarningDecision::Accepted
        }),
        changeset_decision: None,
        publication_authority: Some(publication_authority.clone()),
    };
    ensure_publication_authority_current(publication_authority, publication_authority)?;
    let store_path = crate::store::commit_single_article_acceptance(paths, &ledger_entry)?;
    Ok((ledger_entry, store_path))
}

pub fn verify_article_acceptance(
    paths: &PublicationWorkspace,
    current_authority: &ArticlePublicationAuthority,
    article_path: &Path,
    title: &str,
    target_relative_path: &str,
) -> Result<ArticleAcceptanceLedgerEntry> {
    Ok(load_accepted_article(
        paths,
        current_authority,
        article_path,
        title,
        target_relative_path,
    )?
    .ledger_entry)
}

pub fn load_accepted_article(
    paths: &PublicationWorkspace,
    current_authority: &ArticlePublicationAuthority,
    article_path: &Path,
    title: &str,
    target_relative_path: &str,
) -> Result<AcceptedArticle> {
    let article_absolute = absolute_scoped_path(paths, article_path)?;
    let authorization = crate::store::load_article_authorization(paths, target_relative_path)?;
    let ledger_entry = authorization.ledger_entry;
    let publication_authority = ledger_entry.publication_authority.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "article acceptance ledger is unbound to a wiki target and publication policy; repeat named-human review and acceptance"
        )
    })?;
    ensure_publication_authority_current(current_authority, publication_authority)?;
    if ledger_entry.title != title.trim() {
        bail!(
            "article acceptance title mismatch: ledger has {:?}, current title is {:?}",
            ledger_entry.title,
            title.trim()
        );
    }
    let normalized_target = normalize_relative_path(target_relative_path);
    if ledger_entry.target_relative_path != normalized_target {
        bail!(
            "article acceptance target mismatch: ledger has {}, current target is {}",
            ledger_entry.target_relative_path,
            normalized_target
        );
    }
    let current_relative_path = article_absolute
        .strip_prefix(&paths.project_root)
        .map(normalize_path)
        .unwrap_or_else(|_| normalize_path(&article_absolute));
    if current_relative_path != ledger_entry.source_relative_path
        && current_relative_path != ledger_entry.target_relative_path
    {
        bail!(
            "article acceptance path mismatch: ledger permits source {} or target {}, current path is {}",
            ledger_entry.source_relative_path,
            ledger_entry.target_relative_path,
            current_relative_path
        );
    }
    if ledger_entry.human_editor_claim.trim().is_empty()
        || ledger_entry.editor_identity_assurance != EDITOR_IDENTITY_ASSURANCE
        || ledger_entry.decision != ACCEPTANCE_DECISION
    {
        bail!("article acceptance ledger entry is incomplete or uses unsupported assertions");
    }
    let _ = ledger_entry.resolved_warning_decision()?;
    if let Some(binding) = &ledger_entry.changeset_decision {
        crate::changeset::verify_changeset_decision_binding(
            paths,
            current_authority,
            &ledger_entry,
            binding,
        )?;
    }
    let content = fs::read_to_string(&article_absolute)
        .with_context(|| format!("failed to read {}", article_absolute.display()))?;
    let current_hash = compute_sha256(&content);
    if ledger_entry.content_sha256 != current_hash {
        bail!(
            "article changed after the recorded acceptance decision: accepted {}, current {}; repeat review and run `wikitool article accept` again",
            ledger_entry.content_sha256,
            current_hash
        );
    }
    Ok(AcceptedArticle {
        ledger_entry,
        content,
        decision_id: authorization.decision.decision_id,
        changeset_sha256: authorization.decision.changeset_sha256,
    })
}

pub(crate) fn ensure_publication_authority_current(
    current: &ArticlePublicationAuthority,
    recorded: &ArticlePublicationAuthority,
) -> Result<()> {
    if recorded.target_api_url != current.target_api_url {
        bail!(
            "article acceptance belongs to wiki target {}, not {}; repeat named-human review and acceptance for the configured target",
            recorded.target_api_url,
            current.target_api_url
        );
    }
    if recorded.site_adapter_id != current.site_adapter_id
        || recorded.publication_policy_sha256 != current.publication_policy_sha256
    {
        bail!(
            "article acceptance publication policy changed from {} ({}) to {} ({}); repeat named-human review and acceptance",
            recorded.site_adapter_id,
            recorded.publication_policy_sha256,
            current.site_adapter_id,
            current.publication_policy_sha256
        );
    }
    Ok(())
}

pub(crate) fn absolute_scoped_path(paths: &PublicationWorkspace, path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    paths.validate_scoped_path(&absolute)?;
    Ok(absolute)
}

pub(crate) fn validate_target_relative_path(target_relative_path: &str) -> Result<()> {
    let path = Path::new(target_relative_path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("article acceptance target must be a project-relative path");
    }
    let normalized = normalize_relative_path(target_relative_path);
    if !normalized.starts_with("wiki_content/Main/") || !normalized.ends_with(".wiki") {
        bail!("article acceptance only supports non-redirect Main namespace article paths");
    }
    Ok(())
}

pub(crate) fn normalize_relative_path(path: &str) -> String {
    path.replace('\\', "/")
}
