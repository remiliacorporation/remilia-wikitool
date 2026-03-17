use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ArticleLintSeverity {
    Error,
    Warning,
    Suggestion,
}

impl ArticleLintSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Suggestion => "suggestion",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextSpan {
    pub line: usize,
    pub column: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedFixKind {
    SafeAutofix,
    AssistedFix,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SuggestedFix {
    pub label: String,
    pub kind: SuggestedFixKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticleLintIssue {
    pub rule_id: String,
    pub severity: ArticleLintSeverity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<TextSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_remediation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_fixes: Vec<SuggestedFix>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticleLintResourcesStatus {
    pub capabilities_loaded: bool,
    pub template_catalog_loaded: bool,
    pub index_ready: bool,
    pub graph_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticleLintReport {
    pub schema_version: String,
    pub profile_id: String,
    pub relative_path: String,
    pub title: String,
    pub namespace: String,
    pub issue_count: usize,
    pub errors: usize,
    pub warnings: usize,
    pub suggestions: usize,
    pub resources: ArticleLintResourcesStatus,
    pub issues: Vec<ArticleLintIssue>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleFixApplyMode {
    None,
    Safe,
}

impl ArticleFixApplyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Safe => "safe",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppliedFixRecord {
    pub rule_id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticleFixResult {
    pub schema_version: String,
    pub profile_id: String,
    pub relative_path: String,
    pub title: String,
    pub namespace: String,
    pub apply_mode: String,
    pub changed: bool,
    pub applied_fix_count: usize,
    pub applied_fixes: Vec<AppliedFixRecord>,
    pub remaining_report: ArticleLintReport,
}
