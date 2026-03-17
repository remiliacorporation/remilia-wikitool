use anyhow::{Result, bail};
use serde::Serialize;

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
        if value.eq_ignore_ascii_case("html") {
            return Ok(Self::Html);
        }
        bail!("unsupported fetch format: {value} (expected wikitext|html)")
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

#[derive(Debug, Clone, Serialize)]
pub struct ParsedWikiUrl {
    pub domain: String,
    pub title: String,
    pub api_candidates: Vec<String>,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalFetchResult {
    pub title: String,
    pub content: String,
    pub timestamp: String,
    pub extract: Option<String>,
    pub url: String,
    pub source_wiki: String,
    pub source_domain: String,
    pub content_format: String,
}

#[derive(Debug, Clone)]
pub struct ExternalFetchOptions {
    pub format: ExternalFetchFormat,
    pub max_bytes: usize,
}

impl Default for ExternalFetchOptions {
    fn default() -> Self {
        Self {
            format: ExternalFetchFormat::Wikitext,
            max_bytes: 1_000_000,
        }
    }
}
