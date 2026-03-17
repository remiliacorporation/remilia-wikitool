pub mod export;
pub mod mediawiki_fetch;
pub mod model;
pub mod url;
pub mod web_fetch;

use anyhow::Result;

pub use export::{
    default_export_path, generate_frontmatter, sanitize_filename, wikitext_to_markdown,
};
pub use mediawiki_fetch::{fetch_mediawiki_page, fetch_pages_by_titles, list_subpages};
pub use model::{
    DEFAULT_EXPORTS_DIR, ExportFormat, ExternalFetchFormat, ExternalFetchOptions,
    ExternalFetchProfile, ExternalFetchResult, ExtractionQuality, FetchMode, ParsedWikiUrl,
    RenderedFetchMode,
};
pub use url::parse_wiki_url;

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

fn title_or_url<'a>(parsed: &'a ParsedWikiUrl, _url: &'a str) -> &'a str {
    &parsed.title
}
