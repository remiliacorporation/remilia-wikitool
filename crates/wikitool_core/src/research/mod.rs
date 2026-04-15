pub mod cache;
mod entities;
pub mod export;
pub mod mediawiki_fetch;
pub mod model;
pub mod url;
pub mod web_fetch;

use anyhow::Result;

pub use cache::{
    CachedFetchResult, ResearchCacheOptions, ResearchCacheStatus, fetch_page_by_url_cached,
};
pub use export::{
    default_export_path, generate_frontmatter, sanitize_filename, source_content_to_markdown,
    wikitext_to_markdown,
};
pub use mediawiki_fetch::{fetch_mediawiki_page, fetch_pages_by_titles, list_subpages};
pub use model::{
    DEFAULT_EXPORTS_DIR, ExportFormat, ExternalAccessRoute, ExternalContentSignal,
    ExternalFetchAttempt, ExternalFetchFailure, ExternalFetchFailureError, ExternalFetchFormat,
    ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult, ExternalMachineSurface,
    ExternalMachineSurfaceReport, ExtractionQuality, FetchMode, ParsedWikiUrl, RenderedFetchMode,
};
pub use url::parse_wiki_url;
pub use web_fetch::MachineSurfaceDiscoveryOptions;

pub fn fetch_page_by_url(
    url: &str,
    options: &ExternalFetchOptions,
) -> Result<Option<ExternalFetchResult>> {
    if let Some(parsed) = parse_wiki_url(url)
        && let Some(result) =
            mediawiki_fetch::fetch_mediawiki_page(title_or_url(&parsed, url), &parsed, options)?
    {
        return Ok(Some(result));
    }

    web_fetch::fetch_web_url(url, options).map(Some)
}

pub fn discover_machine_surfaces(
    url: &str,
    options: web_fetch::MachineSurfaceDiscoveryOptions,
) -> Result<ExternalMachineSurfaceReport> {
    web_fetch::discover_machine_surfaces(url, options)
}

fn title_or_url<'a>(parsed: &'a ParsedWikiUrl, _url: &'a str) -> &'a str {
    &parsed.title
}
