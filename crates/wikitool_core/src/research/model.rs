use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const DEFAULT_EXPORTS_DIR: &str = "wikitool_exports";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalFetchFormat {
    Wikitext,
    Html,
}

impl ExternalFetchFormat {
    pub fn parse(value: &str) -> Result<Self> {
        if value.eq_ignore_ascii_case("wikitext") {
            return Ok(Self::Wikitext);
        }
        if value.eq_ignore_ascii_case("html")
            || value.eq_ignore_ascii_case("rendered-html")
            || value.eq_ignore_ascii_case("rendered_html")
        {
            return Ok(Self::Html);
        }
        bail!("unsupported fetch format: {value} (expected wikitext|html|rendered-html)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Wikitext,
}

impl ExportFormat {
    pub fn parse(value: &str) -> Result<Self> {
        if value.eq_ignore_ascii_case("markdown") || value.eq_ignore_ascii_case("md") {
            return Ok(Self::Markdown);
        }
        if value.eq_ignore_ascii_case("wikitext") || value.eq_ignore_ascii_case("wiki") {
            return Ok(Self::Wikitext);
        }
        bail!("unsupported export format: {value} (expected markdown|wikitext)")
    }

    pub fn file_extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Wikitext => "wiki",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderedFetchMode {
    ParseApi,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FetchMode {
    Static,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionQuality {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalFetchProfile {
    Legacy,
    Research,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedWikiUrl {
    pub domain: String,
    pub title: String,
    pub api_candidates: Vec<String>,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalFetchAttempt {
    pub mode: String,
    pub url: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalFetchFailure {
    pub source_url: String,
    pub kind: String,
    pub message: String,
    pub attempts: Vec<ExternalFetchAttempt>,
}

#[derive(Debug, Clone)]
pub struct ExternalFetchFailureError {
    pub failure: ExternalFetchFailure,
}

impl std::fmt::Display for ExternalFetchFailureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.failure.message)
    }
}

impl std::error::Error for ExternalFetchFailureError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalContentSignal {
    pub key: String,
    pub value: String,
    pub source_url: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalMachineSurface {
    pub kind: String,
    pub url: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalAccessRoute {
    pub kind: String,
    pub status: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalMachineSurfaceReport {
    pub source_url: String,
    pub origin_url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_signals: Vec<ExternalContentSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<ExternalMachineSurface>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub access_routes: Vec<ExternalAccessRoute>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ExternalFetchAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalFetchResult {
    pub title: String,
    pub content: String,
    /// Wall-clock time at which wikitool fetched this document, in ISO-8601 UTC.
    /// Populated for every fetch outcome regardless of source.
    pub fetched_at: String,
    /// Source-side timestamp: for MediaWiki, the revision timestamp (ISO-8601 from the
    /// API); for arbitrary web pages, absent. Distinguishing this from `fetched_at`
    /// lets agents reason about source freshness separately from our retrieval time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_timestamp: Option<String>,
    pub extract: Option<String>,
    pub url: String,
    pub source_wiki: String,
    pub source_domain: String,
    pub content_format: String,
    pub content_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_fetch_mode: Option<RenderedFetchMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetch_mode: Option<FetchMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extraction_quality: Option<ExtractionQuality>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fetch_attempts: Vec<ExternalFetchAttempt>,
}

#[derive(Debug, Clone)]
pub struct ExternalFetchOptions {
    pub format: ExternalFetchFormat,
    pub max_bytes: usize,
    pub profile: ExternalFetchProfile,
}

impl Default for ExternalFetchOptions {
    fn default() -> Self {
        Self {
            format: ExternalFetchFormat::Wikitext,
            max_bytes: 1_000_000,
            profile: ExternalFetchProfile::Legacy,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaWikiTemplateQueryOptions {
    pub limit: usize,
    pub content_limit: usize,
    pub parameter_limit: usize,
    pub template_titles: Vec<String>,
}

impl Default for MediaWikiTemplateQueryOptions {
    fn default() -> Self {
        Self {
            limit: 16,
            content_limit: 2_400,
            parameter_limit: 64,
            template_titles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaWikiTemplateReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<String>,
    pub contract_scope: String,
    pub target_compatibility: String,
    pub target_compatibility_note: String,
    pub source_url: String,
    pub source_domain: String,
    pub api_endpoint: String,
    pub page_title: String,
    pub canonical_url: String,
    pub fetched_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_revision_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_revision_timestamp: Option<String>,
    pub api_template_count: usize,
    pub page_template_count_returned: usize,
    pub invocation_count: usize,
    pub selected_template_count: usize,
    pub page_templates: Vec<MediaWikiPageTemplate>,
    pub template_invocations: Vec<MediaWikiTemplateInvocation>,
    pub template_pages: Vec<MediaWikiTemplatePage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaWikiPageTemplate {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaWikiTemplateInvocation {
    pub template_title: String,
    pub parameter_keys: Vec<String>,
    pub raw_wikitext: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaWikiTemplatePage {
    pub title: String,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_preview: Option<String>,
    pub content_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub templatedata: Option<MediaWikiTemplateDataSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaWikiTemplateDataSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    pub parameter_count: usize,
    pub parameters: Vec<MediaWikiTemplateDataParameter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaWikiTemplateDataParameter {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub param_type: Option<String>,
    pub required: bool,
    pub suggested: bool,
    pub deprecated: bool,
}
