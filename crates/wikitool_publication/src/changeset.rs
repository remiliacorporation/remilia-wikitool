use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::PublicationWorkspace;
use crate::acceptance::{
    ACCEPTANCE_DECISION, ARTICLE_ACCEPTANCE_LEDGER_SCHEMA_VERSION,
    ArticleAcceptanceDecisionBinding, ArticleAcceptanceLedgerEntry, ArticleProseOrigin,
    ArticlePublicationAuthority, ArticleWarningDecision, EDITOR_IDENTITY_ASSURANCE,
    absolute_scoped_path, ensure_publication_authority_current, normalize_relative_path,
    validate_target_relative_path,
};
use crate::store::{
    AcceptanceDecisionKind, AcceptanceStoreDecision, commit_acceptance_transaction,
    load_acceptance_decision,
};
use crate::support::{
    atomic_write, compute_sha256, normalize_path, parse_redirect, unix_timestamp,
};

pub const ARTICLE_REVIEW_CHANGESET_SCHEMA_VERSION: &str = "article_review_changeset_v2";
pub const ARTICLE_CHANGESET_DECISION_SCHEMA_VERSION: &str = "article_changeset_decision_v2";

#[derive(Debug, Clone)]
pub struct ArticleReviewChangesetInput {
    pub source_path: PathBuf,
    pub title: String,
    pub prose_origin: ArticleProseOrigin,
}

pub trait ArticleReviewEvidenceProvider {
    fn target_relative_path(&self, title: &str) -> Result<String>;
    fn lint(&self, source_path: &Path, title: &str) -> Result<ArticleReviewLintSnapshot>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArticleReviewLintSnapshot {
    pub site_adapter_id: String,
    pub content_sha256: String,
    pub errors: usize,
    pub warnings: usize,
    pub suggestions: usize,
    pub issues: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArticleReviewChangesetItem {
    pub title: String,
    pub source_relative_path: String,
    pub target_relative_path: String,
    pub content_sha256: String,
    pub prose_origin: ArticleProseOrigin,
    pub lint: ArticleReviewLintSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArticleReviewChangeset {
    pub schema_version: String,
    pub changeset_sha256: String,
    pub prepared_at_unix: u64,
    pub publication_authority: ArticlePublicationAuthority,
    pub items: Vec<ArticleReviewChangesetItem>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleChangesetWarningPolicy {
    RequireNone,
    Accept,
}

impl ArticleChangesetWarningPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RequireNone => "require_none",
            Self::Accept => "accept",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArticleChangesetDecisionItem {
    pub review_item: ArticleReviewChangesetItem,
    pub warning_decision: ArticleWarningDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArticleChangesetDecisionReceipt {
    pub schema_version: String,
    pub decision_id: String,
    pub changeset_sha256: String,
    pub changeset_prepared_at_unix: u64,
    pub human_editor_claim: String,
    pub editor_identity_assurance: String,
    pub decision: String,
    pub warning_policy: ArticleChangesetWarningPolicy,
    pub accepted_at_unix: u64,
    pub publication_authority: ArticlePublicationAuthority,
    pub items: Vec<ArticleChangesetDecisionItem>,
}

#[derive(Debug, Clone)]
pub struct ArticleChangesetAcceptanceResult {
    pub decision: ArticleChangesetDecisionReceipt,
    pub acceptance_store_path: PathBuf,
}

pub fn prepare_article_review_changeset(
    paths: &PublicationWorkspace,
    publication_authority: &ArticlePublicationAuthority,
    evidence: &dyn ArticleReviewEvidenceProvider,
    manifest_path: &Path,
    inputs: Vec<ArticleReviewChangesetInput>,
    replace: bool,
) -> Result<ArticleReviewChangeset> {
    if inputs.is_empty() {
        bail!("article changeset requires at least one article");
    }
    let manifest_absolute = changeset_manifest_path(paths, manifest_path)?;
    if manifest_absolute.exists() && !replace {
        bail!(
            "article changeset manifest already exists: {}; pass --replace after reviewing the target",
            normalize_path(&manifest_absolute)
        );
    }

    let mut items = Vec::with_capacity(inputs.len());
    for input in inputs {
        let title = normalize_title(&input.title)?;
        let source_absolute = absolute_scoped_path(paths, &input.source_path)?;
        if !source_absolute.is_file() {
            bail!(
                "article changeset source does not exist or is not a file: {}",
                normalize_path(&source_absolute)
            );
        }
        if source_absolute == manifest_absolute {
            bail!("article changeset manifest cannot overwrite an article source");
        }
        let content = fs::read_to_string(&source_absolute)
            .with_context(|| format!("failed to read {}", source_absolute.display()))?;
        if parse_redirect(&content).0 {
            bail!("article changeset only supports prose articles, not redirects: {title}");
        }
        let target_relative_path = evidence.target_relative_path(&title)?;
        validate_target_relative_path(&target_relative_path)?;
        let lint = evidence.lint(&source_absolute, &title)?;
        let source_relative_path = source_absolute
            .strip_prefix(&paths.project_root)
            .map(normalize_path)
            .with_context(|| {
                format!(
                    "article changeset source is outside the project root: {}",
                    normalize_path(&source_absolute)
                )
            })?;
        items.push(ArticleReviewChangesetItem {
            title,
            source_relative_path,
            target_relative_path: normalize_relative_path(&target_relative_path),
            content_sha256: compute_sha256(&content),
            prose_origin: input.prose_origin,
            lint,
        });
    }
    items.sort_by(|left, right| {
        left.target_relative_path
            .cmp(&right.target_relative_path)
            .then_with(|| left.title.cmp(&right.title))
    });
    validate_item_set(evidence, &items)?;

    let prepared_at_unix = unix_timestamp()?;
    ensure_publication_authority_current(publication_authority, publication_authority)?;
    let changeset_sha256 = changeset_digest(prepared_at_unix, publication_authority, &items)?;
    let manifest = ArticleReviewChangeset {
        schema_version: ARTICLE_REVIEW_CHANGESET_SCHEMA_VERSION.to_string(),
        changeset_sha256,
        prepared_at_unix,
        publication_authority: publication_authority.clone(),
        items,
    };
    let encoded = serde_json::to_string_pretty(&manifest)
        .context("failed to encode article review changeset")?;
    atomic_write(&manifest_absolute, format!("{encoded}\n"))?;
    Ok(manifest)
}

pub fn load_article_review_changeset(
    paths: &PublicationWorkspace,
    current_authority: &ArticlePublicationAuthority,
    evidence: &dyn ArticleReviewEvidenceProvider,
    manifest_path: &Path,
) -> Result<(ArticleReviewChangeset, PathBuf)> {
    let manifest_absolute = changeset_manifest_path(paths, manifest_path)?;
    let encoded = fs::read_to_string(&manifest_absolute)
        .with_context(|| format!("failed to read {}", manifest_absolute.display()))?;
    let manifest: ArticleReviewChangeset = serde_json::from_str(&encoded)
        .with_context(|| format!("failed to decode {}", manifest_absolute.display()))?;
    validate_changeset(current_authority, evidence, &manifest)?;
    Ok((manifest, manifest_absolute))
}

pub fn accept_article_review_changeset(
    paths: &PublicationWorkspace,
    current_authority: &ArticlePublicationAuthority,
    evidence: &dyn ArticleReviewEvidenceProvider,
    manifest_path: &Path,
    human_editor_claim: &str,
    warning_policy: ArticleChangesetWarningPolicy,
) -> Result<ArticleChangesetAcceptanceResult> {
    let human_editor_claim = human_editor_claim.trim();
    if human_editor_claim.is_empty() {
        bail!("article changeset acceptance requires a non-empty self-reported human editor claim");
    }
    let (manifest, _) =
        load_article_review_changeset(paths, current_authority, evidence, manifest_path)?;
    let mut decision_items = Vec::with_capacity(manifest.items.len());
    for item in &manifest.items {
        let source_absolute = absolute_scoped_path(paths, Path::new(&item.source_relative_path))?;
        let content = fs::read_to_string(&source_absolute)
            .with_context(|| format!("failed to read {}", source_absolute.display()))?;
        let current_hash = compute_sha256(&content);
        if current_hash != item.content_sha256 {
            bail!(
                "article changeset item changed after preparation: {} prepared {}, current {}; prepare a new changeset",
                item.title,
                item.content_sha256,
                current_hash
            );
        }
        if parse_redirect(&content).0 {
            bail!(
                "article changeset item became a redirect after preparation: {}",
                item.title
            );
        }
        let current_lint = evidence.lint(&source_absolute, &item.title)?;
        if current_lint != item.lint {
            bail!(
                "article changeset lint evidence changed after preparation for {}; prepare a new changeset",
                item.title
            );
        }
        if current_lint.errors > 0 {
            bail!(
                "article changeset acceptance requires zero lint errors; {} has {}",
                item.title,
                current_lint.errors
            );
        }
        let warning_decision = if current_lint.warnings == 0 {
            ArticleWarningDecision::NoWarnings
        } else if warning_policy == ArticleChangesetWarningPolicy::Accept {
            ArticleWarningDecision::Accepted
        } else {
            bail!(
                "article changeset contains {} warning(s) for {}; resolve them or rerun with --warnings accept after the named human reviews every warning",
                current_lint.warnings,
                item.title
            );
        };
        decision_items.push(ArticleChangesetDecisionItem {
            review_item: item.clone(),
            warning_decision,
        });
    }

    let accepted_at_unix = unix_timestamp()?;
    ensure_publication_authority_current(current_authority, &manifest.publication_authority)?;
    let decision_id = decision_digest(
        &manifest.changeset_sha256,
        manifest.prepared_at_unix,
        human_editor_claim,
        warning_policy,
        accepted_at_unix,
        &manifest.publication_authority,
        &decision_items,
    )?;
    let decision = ArticleChangesetDecisionReceipt {
        schema_version: ARTICLE_CHANGESET_DECISION_SCHEMA_VERSION.to_string(),
        decision_id: decision_id.clone(),
        changeset_sha256: manifest.changeset_sha256.clone(),
        changeset_prepared_at_unix: manifest.prepared_at_unix,
        human_editor_claim: human_editor_claim.to_string(),
        editor_identity_assurance: EDITOR_IDENTITY_ASSURANCE.to_string(),
        decision: ACCEPTANCE_DECISION.to_string(),
        warning_policy,
        accepted_at_unix,
        publication_authority: manifest.publication_authority.clone(),
        items: decision_items,
    };
    validate_decision_receipt(&decision)?;

    let mut ledger_entries = Vec::with_capacity(decision.items.len());
    for item in &decision.items {
        let review_item = &item.review_item;
        let ledger_entry = ArticleAcceptanceLedgerEntry {
            schema_version: ARTICLE_ACCEPTANCE_LEDGER_SCHEMA_VERSION.to_string(),
            title: review_item.title.clone(),
            source_relative_path: review_item.source_relative_path.clone(),
            target_relative_path: review_item.target_relative_path.clone(),
            content_sha256: review_item.content_sha256.clone(),
            human_editor_claim: human_editor_claim.to_string(),
            editor_identity_assurance: EDITOR_IDENTITY_ASSURANCE.to_string(),
            prose_origin: review_item.prose_origin,
            decision: ACCEPTANCE_DECISION.to_string(),
            accepted_at_unix,
            lint_errors: review_item.lint.errors,
            lint_warnings: review_item.lint.warnings,
            lint_suggestions: review_item.lint.suggestions,
            warnings_explicitly_accepted: item.warning_decision == ArticleWarningDecision::Accepted,
            warning_decision: Some(item.warning_decision),
            changeset_decision: Some(ArticleAcceptanceDecisionBinding {
                decision_id: decision_id.clone(),
                changeset_sha256: manifest.changeset_sha256.clone(),
            }),
            publication_authority: Some(manifest.publication_authority.clone()),
        };
        ledger_entries.push(ledger_entry);
    }
    let decision_record = AcceptanceStoreDecision {
        decision_id: decision.decision_id.clone(),
        kind: AcceptanceDecisionKind::ArticleChangeset,
        changeset_sha256: Some(decision.changeset_sha256.clone()),
        human_editor_claim: decision.human_editor_claim.clone(),
        editor_identity_assurance: decision.editor_identity_assurance.clone(),
        decision: decision.decision.clone(),
        accepted_at_unix: decision.accepted_at_unix,
        publication_authority: decision.publication_authority.clone(),
        receipt_json: serde_json::to_string(&decision)
            .context("failed to encode article changeset decision")?,
    };
    let acceptance_store_path =
        commit_acceptance_transaction(paths, &decision_record, &ledger_entries)?;

    for item in &decision.items {
        let source = paths
            .project_root
            .join(&item.review_item.source_relative_path);
        crate::acceptance::verify_article_acceptance(
            paths,
            current_authority,
            &source,
            &item.review_item.title,
            &item.review_item.target_relative_path,
        )?;
    }

    Ok(ArticleChangesetAcceptanceResult {
        decision,
        acceptance_store_path,
    })
}

pub(crate) fn verify_changeset_decision_binding(
    paths: &PublicationWorkspace,
    current_authority: &ArticlePublicationAuthority,
    ledger: &ArticleAcceptanceLedgerEntry,
    binding: &ArticleAcceptanceDecisionBinding,
) -> Result<()> {
    if !is_sha256(&binding.decision_id) || !is_sha256(&binding.changeset_sha256) {
        bail!("article acceptance changeset binding contains an invalid SHA-256 identity");
    }
    let stored = load_acceptance_decision(paths, &binding.decision_id)?;
    if stored.kind != AcceptanceDecisionKind::ArticleChangeset
        || stored.changeset_sha256.as_deref() != Some(binding.changeset_sha256.as_str())
    {
        bail!("article acceptance ledger does not match its transactional decision kind");
    }
    let decision: ArticleChangesetDecisionReceipt = serde_json::from_str(&stored.receipt_json)
        .context("failed to decode transactional article changeset decision")?;
    validate_decision_receipt(&decision)?;
    if decision.decision_id != binding.decision_id
        || decision.changeset_sha256 != binding.changeset_sha256
    {
        bail!("article acceptance ledger does not match its changeset decision identity");
    }
    if decision.human_editor_claim != ledger.human_editor_claim
        || decision.editor_identity_assurance != ledger.editor_identity_assurance
        || decision.decision != ledger.decision
        || decision.accepted_at_unix != ledger.accepted_at_unix
    {
        bail!("article acceptance ledger assertions do not match its changeset decision");
    }
    if stored.accepted_at_unix != decision.accepted_at_unix
        || stored.publication_authority != decision.publication_authority
    {
        bail!("article changeset decision receipt does not match its transactional store row");
    }
    if ledger.publication_authority.as_ref() != Some(&decision.publication_authority) {
        bail!(
            "article acceptance ledger publication authority does not match its changeset decision"
        );
    }
    ensure_publication_authority_current(current_authority, &decision.publication_authority)?;
    let warning_decision = ledger.resolved_warning_decision()?;
    let matches = decision
        .items
        .iter()
        .filter(|item| {
            let reviewed = &item.review_item;
            reviewed.title == ledger.title
                && reviewed.source_relative_path == ledger.source_relative_path
                && reviewed.target_relative_path == ledger.target_relative_path
                && reviewed.content_sha256 == ledger.content_sha256
                && reviewed.prose_origin == ledger.prose_origin
                && reviewed.lint.errors == ledger.lint_errors
                && reviewed.lint.warnings == ledger.lint_warnings
                && reviewed.lint.suggestions == ledger.lint_suggestions
                && item.warning_decision == warning_decision
        })
        .count();
    if matches != 1 {
        bail!("article acceptance ledger item is not uniquely bound by its changeset decision");
    }
    Ok(())
}

fn changeset_manifest_path(paths: &PublicationWorkspace, manifest_path: &Path) -> Result<PathBuf> {
    let absolute = absolute_scoped_path(paths, manifest_path)?;
    if absolute.extension().and_then(|value| value.to_str()) != Some("json") {
        bail!("article changeset manifest path must end in .json");
    }
    Ok(absolute)
}

fn validate_changeset(
    current_authority: &ArticlePublicationAuthority,
    evidence: &dyn ArticleReviewEvidenceProvider,
    manifest: &ArticleReviewChangeset,
) -> Result<()> {
    if manifest.schema_version != ARTICLE_REVIEW_CHANGESET_SCHEMA_VERSION {
        bail!(
            "article review changeset uses unsupported schema {}",
            manifest.schema_version
        );
    }
    validate_item_set(evidence, &manifest.items)?;
    ensure_publication_authority_current(current_authority, &manifest.publication_authority)?;
    let expected = changeset_digest(
        manifest.prepared_at_unix,
        &manifest.publication_authority,
        &manifest.items,
    )?;
    if manifest.changeset_sha256 != expected {
        bail!(
            "article review changeset digest mismatch: recorded {}, computed {}",
            manifest.changeset_sha256,
            expected
        );
    }
    Ok(())
}

fn validate_item_set(
    evidence: &dyn ArticleReviewEvidenceProvider,
    items: &[ArticleReviewChangesetItem],
) -> Result<()> {
    if items.is_empty() {
        bail!("article review changeset must contain at least one item");
    }
    let mut titles = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut previous_key: Option<(&str, &str)> = None;
    for item in items {
        if item.title != normalize_title(&item.title)? {
            bail!(
                "article review changeset title is not canonical: {:?}",
                item.title
            );
        }
        validate_project_relative_path(&item.source_relative_path)?;
        validate_target_relative_path(&item.target_relative_path)?;
        let expected_target = evidence.target_relative_path(&item.title)?;
        if normalize_relative_path(&expected_target) != item.target_relative_path {
            bail!(
                "article review changeset target mismatch for {}: expected {}, got {}",
                item.title,
                expected_target,
                item.target_relative_path
            );
        }
        if !is_sha256(&item.content_sha256) || item.lint.content_sha256 != item.content_sha256 {
            bail!(
                "article review changeset has invalid content identity for {}",
                item.title
            );
        }
        if !titles.insert(portable_identity(&item.title)) {
            bail!(
                "article review changeset title collides under portable MediaWiki/filesystem identity: {}",
                item.title
            );
        }
        if !sources.insert(portable_identity(&item.source_relative_path)) {
            bail!(
                "article review changeset source path collides under case-insensitive filesystem identity: {}",
                item.source_relative_path
            );
        }
        if !targets.insert(portable_identity(&item.target_relative_path)) {
            bail!(
                "article review changeset target path collides under case-insensitive filesystem identity: {}",
                item.target_relative_path
            );
        }
        let key = (item.target_relative_path.as_str(), item.title.as_str());
        if previous_key.is_some_and(|previous| previous >= key) {
            bail!("article review changeset items are not in canonical order");
        }
        previous_key = Some(key);
    }
    Ok(())
}

fn validate_decision_receipt(decision: &ArticleChangesetDecisionReceipt) -> Result<()> {
    if decision.schema_version != ARTICLE_CHANGESET_DECISION_SCHEMA_VERSION
        || decision.editor_identity_assurance != EDITOR_IDENTITY_ASSURANCE
        || decision.decision != ACCEPTANCE_DECISION
        || decision.human_editor_claim.trim().is_empty()
        || decision.items.is_empty()
        || !is_sha256(&decision.changeset_sha256)
    {
        bail!("article changeset decision contains incomplete or unsupported assertions");
    }
    let expected = decision_digest(
        &decision.changeset_sha256,
        decision.changeset_prepared_at_unix,
        &decision.human_editor_claim,
        decision.warning_policy,
        decision.accepted_at_unix,
        &decision.publication_authority,
        &decision.items,
    )?;
    if decision.decision_id != expected {
        bail!(
            "article changeset decision digest mismatch: recorded {}, computed {}",
            decision.decision_id,
            expected
        );
    }
    let review_items = decision
        .items
        .iter()
        .map(|item| item.review_item.clone())
        .collect::<Vec<_>>();
    if decision
        .publication_authority
        .target_api_url
        .trim()
        .is_empty()
        || decision
            .publication_authority
            .site_adapter_id
            .trim()
            .is_empty()
        || !is_sha256(&decision.publication_authority.publication_policy_sha256)
    {
        bail!("article changeset decision has invalid publication authority");
    }
    let expected_changeset = changeset_digest(
        decision.changeset_prepared_at_unix,
        &decision.publication_authority,
        &review_items,
    )?;
    if decision.changeset_sha256 != expected_changeset {
        bail!("article changeset decision items do not match the bound review changeset");
    }
    let mut titles = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for item in &decision.items {
        if !titles.insert(portable_identity(&item.review_item.title))
            || !sources.insert(portable_identity(&item.review_item.source_relative_path))
            || !targets.insert(portable_identity(&item.review_item.target_relative_path))
        {
            bail!(
                "article changeset decision contains colliding portable article or path identities"
            );
        }
        let expected_warning = if item.review_item.lint.warnings == 0 {
            ArticleWarningDecision::NoWarnings
        } else {
            ArticleWarningDecision::Accepted
        };
        if item.warning_decision != expected_warning
            || (item.warning_decision == ArticleWarningDecision::Accepted
                && decision.warning_policy != ArticleChangesetWarningPolicy::Accept)
        {
            bail!("article changeset decision has an inconsistent warning decision");
        }
    }
    Ok(())
}

fn changeset_digest(
    prepared_at_unix: u64,
    publication_authority: &ArticlePublicationAuthority,
    items: &[ArticleReviewChangesetItem],
) -> Result<String> {
    #[derive(Serialize)]
    struct DigestPayload<'a> {
        schema_version: &'static str,
        prepared_at_unix: u64,
        publication_authority: &'a ArticlePublicationAuthority,
        items: &'a [ArticleReviewChangesetItem],
    }
    let payload = DigestPayload {
        schema_version: ARTICLE_REVIEW_CHANGESET_SCHEMA_VERSION,
        prepared_at_unix,
        publication_authority,
        items,
    };
    Ok(compute_sha256(&serde_json::to_string(&payload)?))
}

fn decision_digest(
    changeset_sha256: &str,
    changeset_prepared_at_unix: u64,
    human_editor_claim: &str,
    warning_policy: ArticleChangesetWarningPolicy,
    accepted_at_unix: u64,
    publication_authority: &ArticlePublicationAuthority,
    items: &[ArticleChangesetDecisionItem],
) -> Result<String> {
    #[derive(Serialize)]
    struct DigestPayload<'a> {
        schema_version: &'static str,
        changeset_sha256: &'a str,
        changeset_prepared_at_unix: u64,
        human_editor_claim: &'a str,
        editor_identity_assurance: &'static str,
        decision: &'static str,
        warning_policy: ArticleChangesetWarningPolicy,
        accepted_at_unix: u64,
        publication_authority: &'a ArticlePublicationAuthority,
        items: &'a [ArticleChangesetDecisionItem],
    }
    let payload = DigestPayload {
        schema_version: ARTICLE_CHANGESET_DECISION_SCHEMA_VERSION,
        changeset_sha256,
        changeset_prepared_at_unix,
        human_editor_claim,
        editor_identity_assurance: EDITOR_IDENTITY_ASSURANCE,
        decision: ACCEPTANCE_DECISION,
        warning_policy,
        accepted_at_unix,
        publication_authority,
        items,
    };
    Ok(compute_sha256(&serde_json::to_string(&payload)?))
}

fn normalize_title(title: &str) -> Result<String> {
    let normalized = title.trim().replace('_', " ");
    if normalized.is_empty() {
        bail!("article changeset title must not be empty");
    }
    Ok(normalized)
}

fn portable_identity(value: &str) -> String {
    normalize_relative_path(value).to_lowercase()
}

fn validate_project_relative_path(path: &str) -> Result<()> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("article changeset source must be a project-relative path");
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
