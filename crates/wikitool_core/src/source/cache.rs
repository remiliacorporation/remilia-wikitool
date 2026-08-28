use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::runtime::ResolvedPaths;
use crate::support::{compute_sha256, normalize_path, unix_timestamp};

use super::model::{
    ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult,
    ExternalFetchSession, MediaWikiTemplateQueryOptions, MediaWikiTemplateReport,
};
use super::session::load_source_access_session_for_url;

const SOURCE_FETCH_CACHE_SCHEMA: &str = "source_fetch_cache_v3";
const SOURCE_MEDIAWIKI_CACHE_SCHEMA: &str = "source_mediawiki_template_report_cache_v3";
const SOURCE_CACHE_TTL_SECONDS: u64 = 86_400;
const SOURCE_EXTRACTOR_VERSION: &str = concat!("wikitool-", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceCacheStatus {
    Hit,
    Miss,
    Refresh,
    Bypass,
}

#[derive(Debug, Clone)]
pub struct SourceCacheOptions {
    pub use_cache: bool,
    pub refresh: bool,
}

impl Default for SourceCacheOptions {
    fn default() -> Self {
        Self {
            use_cache: true,
            refresh: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CachedFetchResult {
    pub result: ExternalFetchResult,
    pub status: SourceCacheStatus,
    pub cache_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct CachedMediaWikiTemplateReport {
    pub report: MediaWikiTemplateReport,
    pub status: SourceCacheStatus,
    pub cache_path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedFetchDocument {
    schema_version: String,
    written_at_unix: u64,
    result: ExternalFetchResult,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedMediaWikiTemplateReportDocument {
    schema_version: String,
    written_at_unix: u64,
    report: MediaWikiTemplateReport,
}

pub fn fetch_page_by_url_cached(
    paths: &ResolvedPaths,
    url: &str,
    options: &ExternalFetchOptions,
    cache_options: &SourceCacheOptions,
) -> Result<Option<CachedFetchResult>> {
    let mut options = options.clone();
    if options.session.is_none() {
        options.session = load_source_access_session_for_url(paths, url)?;
    }
    fetch_page_by_url_cached_with(paths, url, &options, cache_options, || {
        super::fetch_page_by_url(url, &options)
    })
}

pub fn fetch_mediawiki_template_report_cached(
    paths: &ResolvedPaths,
    url: &str,
    options: &MediaWikiTemplateQueryOptions,
    cache_options: &SourceCacheOptions,
) -> Result<CachedMediaWikiTemplateReport> {
    let session = load_source_access_session_for_url(paths, url)?;
    let session_for_fetch = session.clone();
    fetch_mediawiki_template_report_cached_with(
        paths,
        url,
        options,
        cache_options,
        session.as_ref(),
        || {
            super::mediawiki_fetch::fetch_mediawiki_template_report_with_session(
                url,
                options,
                session_for_fetch,
            )
        },
    )
}

fn fetch_mediawiki_template_report_cached_with<F>(
    paths: &ResolvedPaths,
    url: &str,
    options: &MediaWikiTemplateQueryOptions,
    cache_options: &SourceCacheOptions,
    session: Option<&ExternalFetchSession>,
    fetcher: F,
) -> Result<CachedMediaWikiTemplateReport>
where
    F: FnOnce() -> Result<MediaWikiTemplateReport>,
{
    if !cache_options.use_cache {
        let mut report = fetcher()?;
        annotate_template_report_cache(&mut report, SourceCacheStatus::Bypass, None);
        return Ok(CachedMediaWikiTemplateReport {
            report,
            status: SourceCacheStatus::Bypass,
            cache_path: None,
        });
    }

    ensure_source_cache_layout(paths)?;
    let key = cache_key_for_mediawiki_template_report(url, options, session);
    let cache_path = mediawiki_template_report_cache_path(paths, &key);
    if !cache_options.refresh
        && let Some(mut report) = read_cached_mediawiki_template_report(&cache_path)?
    {
        annotate_template_report_cache(
            &mut report,
            SourceCacheStatus::Hit,
            Some(cache_path.as_path()),
        );
        return Ok(CachedMediaWikiTemplateReport {
            report,
            status: SourceCacheStatus::Hit,
            cache_path: Some(cache_path),
        });
    }

    let mut report = fetcher()?;
    let cache_path = write_cached_mediawiki_template_report(paths, &key, &report)?;
    let status = if cache_options.refresh {
        SourceCacheStatus::Refresh
    } else {
        SourceCacheStatus::Miss
    };
    annotate_template_report_cache(&mut report, status, Some(cache_path.as_path()));
    Ok(CachedMediaWikiTemplateReport {
        report,
        status,
        cache_path: Some(cache_path),
    })
}

fn fetch_page_by_url_cached_with<F>(
    paths: &ResolvedPaths,
    url: &str,
    options: &ExternalFetchOptions,
    cache_options: &SourceCacheOptions,
    fetcher: F,
) -> Result<Option<CachedFetchResult>>
where
    F: FnOnce() -> Result<Option<ExternalFetchResult>>,
{
    if !cache_options.use_cache {
        return Ok(fetcher()?.map(|result| CachedFetchResult {
            result,
            status: SourceCacheStatus::Bypass,
            cache_path: None,
        }));
    }

    ensure_source_cache_layout(paths)?;
    let key = cache_key_for_fetch(url, options);
    if !cache_options.refresh {
        for cache_path in cache_candidate_paths(paths, &key) {
            if let Some(result) = read_cached_fetch(&cache_path)? {
                return Ok(Some(CachedFetchResult {
                    result,
                    status: SourceCacheStatus::Hit,
                    cache_path: Some(cache_path),
                }));
            }
        }
    }

    let fetched = fetcher()?;
    let Some(result) = fetched else {
        return Ok(None);
    };
    let cache_path = write_cached_fetch(paths, &key, &result)?;

    Ok(Some(CachedFetchResult {
        result,
        status: if cache_options.refresh {
            SourceCacheStatus::Refresh
        } else {
            SourceCacheStatus::Miss
        },
        cache_path: Some(cache_path),
    }))
}

fn ensure_source_cache_layout(paths: &ResolvedPaths) -> Result<()> {
    for directory in [
        paths.source_cache_dir().join("documents"),
        paths.source_cache_dir().join("rendered"),
        paths.source_cache_dir().join("mediawiki_templates"),
    ] {
        fs::create_dir_all(&directory)
            .with_context(|| format!("failed to create {}", normalize_path(&directory)))?;
    }
    Ok(())
}

fn cache_key_for_fetch(url: &str, options: &ExternalFetchOptions) -> String {
    compute_sha256(&format!(
        "fetch|schema={SOURCE_FETCH_CACHE_SCHEMA}|extractor={SOURCE_EXTRACTOR_VERSION}|url={}|format={}|profile={}|max_bytes={}|user_agent={}|session={}",
        url.trim(),
        cache_format_label(options.format),
        cache_profile_label(options.profile),
        options.max_bytes,
        fetch_user_agent(options),
        session_fingerprint(options.session.as_ref()),
    ))
}

fn cache_key_for_mediawiki_template_report(
    url: &str,
    options: &MediaWikiTemplateQueryOptions,
    session: Option<&ExternalFetchSession>,
) -> String {
    compute_sha256(&format!(
        "mediawiki_template_report|schema={SOURCE_MEDIAWIKI_CACHE_SCHEMA}|extractor={SOURCE_EXTRACTOR_VERSION}|url={}|limit={}|content_limit={}|parameter_limit={}|templates={}|session={}",
        url.trim(),
        options.limit,
        options.content_limit,
        options.parameter_limit,
        options
            .template_titles
            .iter()
            .map(|title| title.trim())
            .collect::<Vec<_>>()
            .join("|"),
        session_fingerprint(session),
    ))
}

fn fetch_user_agent(options: &ExternalFetchOptions) -> String {
    options
        .session
        .as_ref()
        .and_then(|session| session.user_agent.clone())
        .or_else(|| std::env::var("WIKITOOL_USER_AGENT").ok())
        .unwrap_or_else(|| crate::config::DEFAULT_USER_AGENT.to_string())
}

fn session_fingerprint(session: Option<&ExternalFetchSession>) -> String {
    let Some(session) = session else {
        return "public".to_string();
    };
    compute_sha256(&format!(
        "domain={}\0cookie={}\0user_agent={}",
        session.domain,
        session.cookie_header,
        session.user_agent.as_deref().unwrap_or("")
    ))
}

fn cache_candidate_paths(paths: &ResolvedPaths, key: &str) -> [PathBuf; 2] {
    [
        paths
            .source_cache_dir()
            .join("documents")
            .join(format!("{key}.json")),
        paths
            .source_cache_dir()
            .join("rendered")
            .join(format!("{key}.json")),
    ]
}

fn mediawiki_template_report_cache_path(paths: &ResolvedPaths, key: &str) -> PathBuf {
    paths
        .source_cache_dir()
        .join("mediawiki_templates")
        .join(format!("{key}.json"))
}

fn cache_path_for_result(
    paths: &ResolvedPaths,
    key: &str,
    result: &ExternalFetchResult,
) -> PathBuf {
    let [documents_path, rendered_path] = cache_candidate_paths(paths, key);
    if uses_rendered_bucket(result) {
        rendered_path
    } else {
        documents_path
    }
}

fn cache_format_label(format: ExternalFetchFormat) -> &'static str {
    match format {
        ExternalFetchFormat::Wikitext => "wikitext",
        ExternalFetchFormat::Html => "html",
    }
}

fn cache_profile_label(profile: ExternalFetchProfile) -> &'static str {
    match profile {
        ExternalFetchProfile::MediaWiki => "mediawiki",
        ExternalFetchProfile::ReadableDocument => "source",
    }
}

fn read_cached_fetch(path: &Path) -> Result<Option<ExternalFetchResult>> {
    if !path.exists() {
        return Ok(None);
    }

    let payload = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", normalize_path(path)))?;
    match serde_json::from_str::<CachedFetchDocument>(&payload) {
        Ok(document)
            if document.schema_version == SOURCE_FETCH_CACHE_SCHEMA
                && cache_document_is_fresh(document.written_at_unix) =>
        {
            Ok(Some(document.result))
        }
        Err(_) => Ok(None),
        Ok(_) => Ok(None),
    }
}

fn write_cached_fetch(
    paths: &ResolvedPaths,
    key: &str,
    result: &ExternalFetchResult,
) -> Result<PathBuf> {
    let [documents_path, rendered_path] = cache_candidate_paths(paths, key);
    let path = cache_path_for_result(paths, key, result);
    let stale_path = if path == rendered_path {
        documents_path
    } else {
        rendered_path
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }
    let payload = serde_json::to_string_pretty(&CachedFetchDocument {
        schema_version: SOURCE_FETCH_CACHE_SCHEMA.to_string(),
        written_at_unix: unix_timestamp()?,
        result: result.clone(),
    })?;
    write_cache_file(&path, &payload)?;
    if stale_path.exists() {
        fs::remove_file(&stale_path)
            .with_context(|| format!("failed to remove {}", normalize_path(&stale_path)))?;
    }
    Ok(path)
}

fn read_cached_mediawiki_template_report(path: &Path) -> Result<Option<MediaWikiTemplateReport>> {
    if !path.exists() {
        return Ok(None);
    }

    let payload = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", normalize_path(path)))?;
    match serde_json::from_str::<CachedMediaWikiTemplateReportDocument>(&payload) {
        Ok(document)
            if document.schema_version == SOURCE_MEDIAWIKI_CACHE_SCHEMA
                && cache_document_is_fresh(document.written_at_unix) =>
        {
            Ok(Some(document.report))
        }
        Err(_) => Ok(None),
        Ok(_) => Ok(None),
    }
}

fn write_cached_mediawiki_template_report(
    paths: &ResolvedPaths,
    key: &str,
    report: &MediaWikiTemplateReport,
) -> Result<PathBuf> {
    let path = mediawiki_template_report_cache_path(paths, key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }
    let mut report = report.clone();
    report.cache_status = None;
    report.cache_path = None;
    let payload = serde_json::to_string_pretty(&CachedMediaWikiTemplateReportDocument {
        schema_version: SOURCE_MEDIAWIKI_CACHE_SCHEMA.to_string(),
        written_at_unix: unix_timestamp()?,
        report,
    })?;
    write_cache_file(&path, &payload)?;
    Ok(path)
}

fn cache_document_is_fresh(written_at_unix: u64) -> bool {
    unix_timestamp()
        .map(|now| now.saturating_sub(written_at_unix) <= SOURCE_CACHE_TTL_SECONDS)
        .unwrap_or(false)
}

fn write_cache_file(path: &Path, payload: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("cache.json");
    let temporary = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    fs::write(&temporary, payload.as_bytes())
        .with_context(|| format!("failed to write {}", normalize_path(&temporary)))?;
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("failed to replace {}", normalize_path(path)))?;
    }
    fs::rename(&temporary, path).with_context(|| {
        format!(
            "failed to promote cache document {} -> {}",
            normalize_path(&temporary),
            normalize_path(path)
        )
    })?;
    Ok(())
}

fn annotate_template_report_cache(
    report: &mut MediaWikiTemplateReport,
    status: SourceCacheStatus,
    cache_path: Option<&Path>,
) {
    report.cache_status = Some(format_cache_status(status).to_string());
    report.cache_path = cache_path.map(normalize_path);
}

fn format_cache_status(status: SourceCacheStatus) -> &'static str {
    match status {
        SourceCacheStatus::Hit => "hit",
        SourceCacheStatus::Miss => "miss",
        SourceCacheStatus::Refresh => "refresh",
        SourceCacheStatus::Bypass => "bypass",
    }
}

fn uses_rendered_bucket(result: &ExternalFetchResult) -> bool {
    result.source_wiki.eq_ignore_ascii_case("mediawiki")
        && result.content_format.eq_ignore_ascii_case("html")
        && result.rendered_fetch_mode.is_some()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        SourceCacheOptions, SourceCacheStatus, cache_key_for_fetch, cache_path_for_result,
        ensure_source_cache_layout, fetch_mediawiki_template_report_cached_with,
        fetch_page_by_url_cached_with,
    };
    use crate::runtime::{ResolvedPaths, ValueSource};
    use crate::source::model::{
        ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult,
        MediaWikiTemplateQueryOptions, MediaWikiTemplateReport,
    };

    fn paths(project_root: &Path) -> ResolvedPaths {
        let state_dir = project_root.join(".wikitool");
        let data_dir = state_dir.join("data");
        ResolvedPaths {
            project_root: project_root.to_path_buf(),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: state_dir.clone(),
            data_dir: data_dir.clone(),
            db_path: data_dir.join("wikitool.db"),
            config_path: state_dir.join("config.toml"),
            parser_config_path: state_dir.join("parser-config.json"),
            root_source: ValueSource::Default,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    fn sample_result(content: &str) -> ExternalFetchResult {
        ExternalFetchResult {
            title: "Example".to_string(),
            content: content.to_string(),
            fetched_at: "2026-04-16T00:00:00Z".to_string(),
            revision_timestamp: None,
            extract: Some("summary".to_string()),
            url: "https://example.com/article".to_string(),
            source_wiki: "web".to_string(),
            source_domain: "example.com".to_string(),
            content_format: "text".to_string(),
            content_hash: crate::support::compute_sha256(content),
            revision_id: None,
            display_title: None,
            rendered_fetch_mode: None,
            canonical_url: Some("https://example.com/article".to_string()),
            site_name: Some("Example".to_string()),
            byline: None,
            published_at: None,
            fetch_mode: Some(crate::source::FetchMode::Static),
            extraction_quality: Some(crate::source::ExtractionQuality::Medium),
            fetch_attempts: Vec::new(),
        }
    }

    fn mediawiki_rendered_result(content: &str) -> ExternalFetchResult {
        ExternalFetchResult {
            title: "Main Page".to_string(),
            content: content.to_string(),
            fetched_at: "2026-04-16T00:00:00Z".to_string(),
            revision_timestamp: Some("2026-04-15T12:00:00Z".to_string()),
            extract: Some("main page".to_string()),
            url: "https://wiki.remilia.org/wiki/Main_Page".to_string(),
            source_wiki: "mediawiki".to_string(),
            source_domain: "wiki.remilia.org".to_string(),
            content_format: "html".to_string(),
            content_hash: crate::support::compute_sha256(content),
            revision_id: Some(1),
            display_title: Some("Main Page".to_string()),
            rendered_fetch_mode: Some(crate::source::RenderedFetchMode::ParseApi),
            canonical_url: Some("https://wiki.remilia.org/wiki/Main_Page".to_string()),
            site_name: Some("Remilia Wiki".to_string()),
            byline: None,
            published_at: None,
            fetch_mode: None,
            extraction_quality: None,
            fetch_attempts: Vec::new(),
        }
    }

    fn sample_template_report(title: &str) -> MediaWikiTemplateReport {
        MediaWikiTemplateReport {
            cache_status: None,
            cache_path: None,
            contract_scope: "source_mediawiki_api".to_string(),
            target_compatibility: "not_evaluated".to_string(),
            target_compatibility_note: "source wiki only".to_string(),
            source_url: "https://example.org/wiki/Article".to_string(),
            source_domain: "example.org".to_string(),
            api_endpoint: "https://example.org/api.php".to_string(),
            page_title: title.to_string(),
            canonical_url: "https://example.org/wiki/Article".to_string(),
            fetched_at: "2026-04-17T00:00:00Z".to_string(),
            page_revision_id: Some(1),
            page_revision_timestamp: Some("2026-04-16T00:00:00Z".to_string()),
            api_template_count: 1,
            page_template_count_returned: 1,
            invocation_count: 1,
            selected_template_count: 1,
            page_templates: Vec::new(),
            template_invocations: Vec::new(),
            template_pages: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn source_cache_keys_use_full_sha256() {
        let options = ExternalFetchOptions {
            format: ExternalFetchFormat::Html,
            max_bytes: 10_000,
            profile: ExternalFetchProfile::ReadableDocument,
            session: None,
        };
        assert_eq!(
            cache_key_for_fetch("https://example.com/article", &options).len(),
            64
        );
    }

    #[test]
    fn ensures_cache_layout() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());

        ensure_source_cache_layout(&paths).expect("layout should initialize");

        assert!(paths.source_cache_dir().join("documents").exists());
        assert!(paths.source_cache_dir().join("rendered").exists());
        assert!(
            paths
                .source_cache_dir()
                .join("mediawiki_templates")
                .exists()
        );
    }

    #[test]
    fn cached_fetch_hits_after_first_write() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let count = Cell::new(0usize);
        let options = ExternalFetchOptions {
            format: ExternalFetchFormat::Html,
            max_bytes: 10_000,
            profile: ExternalFetchProfile::ReadableDocument,
            session: None,
        };

        let first = fetch_page_by_url_cached_with(
            &paths,
            "https://example.com/article",
            &options,
            &SourceCacheOptions::default(),
            || {
                count.set(count.get() + 1);
                Ok(Some(sample_result("first body")))
            },
        )
        .expect("first fetch should succeed")
        .expect("first fetch should exist");
        assert_eq!(first.status, SourceCacheStatus::Miss);
        assert_eq!(count.get(), 1);

        let second = fetch_page_by_url_cached_with(
            &paths,
            "https://example.com/article",
            &options,
            &SourceCacheOptions::default(),
            || {
                count.set(count.get() + 1);
                Ok(Some(sample_result("second body")))
            },
        )
        .expect("second fetch should succeed")
        .expect("second fetch should exist");
        assert_eq!(second.status, SourceCacheStatus::Hit);
        assert_eq!(count.get(), 1);
        assert_eq!(second.result.content, "first body");
    }

    #[test]
    fn refresh_refetches_and_overwrites_cache() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let count = Cell::new(0usize);
        let options = ExternalFetchOptions {
            format: ExternalFetchFormat::Html,
            max_bytes: 10_000,
            profile: ExternalFetchProfile::ReadableDocument,
            session: None,
        };

        let _ = fetch_page_by_url_cached_with(
            &paths,
            "https://example.com/article",
            &options,
            &SourceCacheOptions::default(),
            || {
                count.set(count.get() + 1);
                Ok(Some(sample_result("first body")))
            },
        )
        .expect("first fetch should succeed");

        let refreshed = fetch_page_by_url_cached_with(
            &paths,
            "https://example.com/article",
            &options,
            &SourceCacheOptions {
                use_cache: true,
                refresh: true,
            },
            || {
                count.set(count.get() + 1);
                Ok(Some(sample_result("updated body")))
            },
        )
        .expect("refresh fetch should succeed")
        .expect("refresh fetch should exist");

        assert_eq!(refreshed.status, SourceCacheStatus::Refresh);
        assert_eq!(refreshed.result.content, "updated body");
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn routes_mediawiki_html_into_rendered_bucket() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let options = ExternalFetchOptions {
            format: ExternalFetchFormat::Html,
            max_bytes: 10_000,
            profile: ExternalFetchProfile::ReadableDocument,
            session: None,
        };
        let rendered = cache_path_for_result(
            &paths,
            &cache_key_for_fetch("https://wiki.remilia.org/wiki/Main_Page", &options),
            &mediawiki_rendered_result("<p>main page</p>"),
        );
        let document = cache_path_for_result(
            &paths,
            &cache_key_for_fetch("https://example.com/article", &options),
            &sample_result("document body"),
        );

        assert_eq!(
            rendered.parent().and_then(|path| path.file_name()),
            Some(std::ffi::OsStr::new("rendered"))
        );
        assert_eq!(
            document.parent().and_then(|path| path.file_name()),
            Some(std::ffi::OsStr::new("documents"))
        );
    }

    #[test]
    fn cached_mediawiki_template_report_hits_after_first_write() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let count = Cell::new(0usize);
        let options = MediaWikiTemplateQueryOptions {
            limit: 1,
            content_limit: 100,
            parameter_limit: 4,
            template_titles: vec!["Template:Infobox".to_string()],
        };

        let first = fetch_mediawiki_template_report_cached_with(
            &paths,
            "https://example.org/wiki/Article",
            &options,
            &SourceCacheOptions::default(),
            None,
            || {
                count.set(count.get() + 1);
                Ok(sample_template_report("First"))
            },
        )
        .expect("first report should cache");
        assert_eq!(first.status, SourceCacheStatus::Miss);
        assert_eq!(first.report.cache_status.as_deref(), Some("miss"));
        assert!(first.report.cache_path.is_some());
        assert_eq!(count.get(), 1);

        let second = fetch_mediawiki_template_report_cached_with(
            &paths,
            "https://example.org/wiki/Article",
            &options,
            &SourceCacheOptions::default(),
            None,
            || {
                count.set(count.get() + 1);
                Ok(sample_template_report("Second"))
            },
        )
        .expect("second report should hit");
        assert_eq!(second.status, SourceCacheStatus::Hit);
        assert_eq!(second.report.cache_status.as_deref(), Some("hit"));
        assert_eq!(second.report.page_title, "First");
        assert_eq!(count.get(), 1);
    }
}
