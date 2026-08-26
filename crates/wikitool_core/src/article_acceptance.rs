use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::filesystem::validate_scoped_path;
use crate::runtime::ResolvedPaths;
use crate::support::{compute_sha256, normalize_path, unix_timestamp};

pub const ARTICLE_ACCEPTANCE_SCHEMA_VERSION: &str = "article_acceptance_v2";
pub const HUMAN_ACCEPTANCE_ATTESTATION: &str = "human_read_and_accepted_article_prose";
pub const EDITORIAL_QUALITY_ATTESTATION: &str =
    "human_judged_article_specific_readable_proportionate_and_source_bound";

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticleAcceptanceReceipt {
    pub schema_version: String,
    pub title: String,
    pub source_relative_path: String,
    pub target_relative_path: String,
    pub content_sha256: String,
    pub human_editor: String,
    pub prose_origin: ArticleProseOrigin,
    pub attestation: String,
    pub editorial_quality_attestation: String,
    pub accepted_at_unix: u64,
    pub lint_errors: usize,
    pub lint_warnings: usize,
    pub lint_suggestions: usize,
    pub warnings_explicitly_accepted: bool,
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
    pub receipt: ArticleAcceptanceReceipt,
    /// The exact byte-equivalent UTF-8 text whose hash was accepted. Callers
    /// must consume this snapshot instead of rereading the source path.
    pub content: String,
}

pub fn record_article_acceptance(
    paths: &ResolvedPaths,
    article_path: &Path,
    title: &str,
    target_relative_path: &str,
    human_editor: &str,
    prose_origin: ArticleProseOrigin,
    lint: ArticleAcceptanceLintSummary,
) -> Result<(ArticleAcceptanceReceipt, PathBuf)> {
    let human_editor = human_editor.trim();
    if human_editor.is_empty() {
        bail!("article acceptance requires a non-empty human editor identity");
    }
    if lint.errors > 0 {
        bail!("article acceptance requires zero lint errors");
    }
    if lint.warnings > 0 && !lint.warnings_explicitly_accepted {
        bail!(
            "article acceptance found {} warning(s); a human must resolve them or explicitly accept them",
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
    let receipt = ArticleAcceptanceReceipt {
        schema_version: ARTICLE_ACCEPTANCE_SCHEMA_VERSION.to_string(),
        title: title.trim().to_string(),
        source_relative_path,
        target_relative_path: normalize_relative_path(target_relative_path),
        content_sha256,
        human_editor: human_editor.to_string(),
        prose_origin,
        attestation: HUMAN_ACCEPTANCE_ATTESTATION.to_string(),
        editorial_quality_attestation: EDITORIAL_QUALITY_ATTESTATION.to_string(),
        accepted_at_unix: unix_timestamp()?,
        lint_errors: lint.errors,
        lint_warnings: lint.warnings,
        lint_suggestions: lint.suggestions,
        warnings_explicitly_accepted: lint.warnings_explicitly_accepted,
    };
    let receipt_path = article_acceptance_receipt_path(paths, target_relative_path)?;
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let encoded = serde_json::to_string_pretty(&receipt)
        .context("failed to encode article acceptance receipt")?;
    fs::write(&receipt_path, format!("{encoded}\n"))
        .with_context(|| format!("failed to write {}", receipt_path.display()))?;
    Ok((receipt, receipt_path))
}

pub fn verify_article_acceptance(
    paths: &ResolvedPaths,
    article_path: &Path,
    title: &str,
    target_relative_path: &str,
) -> Result<ArticleAcceptanceReceipt> {
    Ok(load_accepted_article(paths, article_path, title, target_relative_path)?.receipt)
}

pub fn load_accepted_article(
    paths: &ResolvedPaths,
    article_path: &Path,
    title: &str,
    target_relative_path: &str,
) -> Result<AcceptedArticle> {
    let article_absolute = absolute_scoped_path(paths, article_path)?;
    let receipt_path = article_acceptance_receipt_path(paths, target_relative_path)?;
    let encoded = fs::read_to_string(&receipt_path).with_context(|| {
        format!(
            "human acceptance receipt is missing for {title}: {}; run `wikitool article accept` after a human reads the exact prose",
            normalize_path(&receipt_path)
        )
    })?;
    let receipt: ArticleAcceptanceReceipt = serde_json::from_str(&encoded)
        .with_context(|| format!("failed to decode {}", receipt_path.display()))?;
    if receipt.schema_version != ARTICLE_ACCEPTANCE_SCHEMA_VERSION {
        bail!(
            "article acceptance receipt uses unsupported schema {}",
            receipt.schema_version
        );
    }
    if receipt.title != title.trim() {
        bail!(
            "article acceptance title mismatch: receipt has {:?}, current title is {:?}",
            receipt.title,
            title.trim()
        );
    }
    let normalized_target = normalize_relative_path(target_relative_path);
    if receipt.target_relative_path != normalized_target {
        bail!(
            "article acceptance target mismatch: receipt has {}, current target is {}",
            receipt.target_relative_path,
            normalized_target
        );
    }
    if receipt.human_editor.trim().is_empty()
        || receipt.attestation != HUMAN_ACCEPTANCE_ATTESTATION
        || receipt.editorial_quality_attestation != EDITORIAL_QUALITY_ATTESTATION
    {
        bail!("article acceptance receipt does not contain valid human editorial attestations");
    }
    let content = fs::read_to_string(&article_absolute)
        .with_context(|| format!("failed to read {}", article_absolute.display()))?;
    let current_hash = compute_sha256(&content);
    if receipt.content_sha256 != current_hash {
        bail!(
            "article prose changed after human acceptance: accepted {}, current {}; repeat human review and `wikitool article accept`",
            receipt.content_sha256,
            current_hash
        );
    }
    Ok(AcceptedArticle { receipt, content })
}

pub fn article_acceptance_receipt_path(
    paths: &ResolvedPaths,
    target_relative_path: &str,
) -> Result<PathBuf> {
    validate_target_relative_path(target_relative_path)?;
    Ok(paths.state_dir.join("acceptances").join(format!(
        "{}.json",
        normalize_relative_path(target_relative_path)
    )))
}

fn absolute_scoped_path(paths: &ResolvedPaths, path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    validate_scoped_path(paths, &absolute)?;
    Ok(absolute)
}

fn validate_target_relative_path(target_relative_path: &str) -> Result<()> {
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

fn normalize_relative_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::runtime::ValueSource;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "wikitool-article-acceptance-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("test directory");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn paths(root: &Path) -> ResolvedPaths {
        let state_dir = root.join(".wikitool");
        fs::create_dir_all(&state_dir).expect("state");
        ResolvedPaths {
            project_root: root.to_path_buf(),
            wiki_content_dir: root.join("wiki_content"),
            templates_dir: root.join("templates"),
            state_dir: state_dir.clone(),
            data_dir: state_dir.join("data"),
            db_path: state_dir.join("data/wikitool.db"),
            config_path: state_dir.join("config.toml"),
            parser_config_path: state_dir.join("parser-config.json"),
            root_source: ValueSource::Default,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn acceptance_is_bound_to_exact_content() {
        let temp = TestDir::new();
        let paths = paths(&temp.0);
        let draft = paths.state_dir.join("drafts/Cheetah.wiki");
        fs::create_dir_all(draft.parent().expect("parent")).expect("draft directory");
        fs::write(&draft, "Specific encyclopedic prose.\n").expect("draft");
        let target = "wiki_content/Main/Cheetah.wiki";
        let (receipt, _) = record_article_acceptance(
            &paths,
            &draft,
            "Cheetah",
            target,
            "human-editor",
            ArticleProseOrigin::AgentDraft,
            ArticleAcceptanceLintSummary {
                content_sha256: compute_sha256("Specific encyclopedic prose.\n"),
                errors: 0,
                warnings: 0,
                suggestions: 1,
                warnings_explicitly_accepted: false,
            },
        )
        .expect("accept");
        assert_eq!(receipt.content_sha256.len(), 64);
        verify_article_acceptance(&paths, &draft, "Cheetah", target).expect("verify");

        fs::write(&draft, "Agent changed the prose.\n").expect("change draft");
        let error = verify_article_acceptance(&paths, &draft, "Cheetah", target)
            .expect_err("stale receipt must fail");
        assert!(error.to_string().contains("changed after human acceptance"));
    }
}
