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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalFetchResult {
    pub title: String,
    pub content: String,
    pub timestamp: String,
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
