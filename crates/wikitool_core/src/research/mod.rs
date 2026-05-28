pub mod cache;
pub(crate) mod entities;
pub mod export;
pub mod mediawiki_fetch;
pub mod model;
pub mod session;
mod template_render;
pub mod url;
mod web_archive;
pub mod web_fetch;

use anyhow::Result;

pub use cache::{
    CachedFetchResult, CachedMediaWikiTemplateReport, ResearchCacheOptions, ResearchCacheStatus,
    fetch_mediawiki_template_report_cached, fetch_page_by_url_cached,
};
pub use export::{
    default_export_path, generate_frontmatter, sanitize_filename, source_content_to_markdown,
    wikitext_to_markdown,
};
pub use mediawiki_fetch::{
    fetch_mediawiki_page, fetch_mediawiki_template_report, fetch_pages_by_titles, list_subpages,
};
pub use model::{
    ChallengeHandoff, DEFAULT_EXPORTS_DIR, ExportFormat, ExternalAccessRoute,
    ExternalContentSignal, ExternalFetchAttempt, ExternalFetchFailure, ExternalFetchFailureError,
    ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult,
    ExternalFetchSession, ExternalMachineSurface, ExternalMachineSurfaceReport, ExtractionQuality,
    FetchMode, MediaWikiPageTemplate, MediaWikiTemplateDataParameter, MediaWikiTemplateDataSummary,
    MediaWikiTemplateInvocation, MediaWikiTemplatePage, MediaWikiTemplateQueryOptions,
    MediaWikiTemplateReport, ParsedWikiUrl, RenderedFetchMode,
};
pub use session::{
    ResearchSession, ResearchSessionImportOptions, ResearchSessionImportResult,
    ResearchSessionSummary, clear_research_session, import_research_session,
    list_research_sessions, load_research_session_for_url, prune_research_sessions,
    show_research_session,
};
pub use url::parse_wiki_url;
pub use web_archive::{WebArchiveEntry, WebArchiveOptions, WebArchiveReport, archive_web_site};
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
