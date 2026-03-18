use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::runtime::ResolvedPaths;
use crate::support::{compute_hash, normalize_path};

use super::model::{
    ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult,
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchCacheStatus {
    Hit,
    Miss,
    Refresh,
    Bypass,
}

#[derive(Debug, Clone)]
pub struct ResearchCacheOptions {
    pub use_cache: bool,
    pub refresh: bool,
}

impl Default for ResearchCacheOptions {
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
    pub status: ResearchCacheStatus,
    pub cache_path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedFetchDocument {
    schema_version: String,
    result: ExternalFetchResult,
}

pub fn fetch_page_by_url_cached(
    paths: &ResolvedPaths,
    url: &str,
    options: &ExternalFetchOptions,
    cache_options: &ResearchCacheOptions,
) -> Result<Option<CachedFetchResult>> {
    fetch_page_by_url_cached_with(paths, url, options, cache_options, || {
        super::fetch_page_by_url(url, options)
    })
}

fn fetch_page_by_url_cached_with<F>(
    paths: &ResolvedPaths,
    url: &str,
    options: &ExternalFetchOptions,
    cache_options: &ResearchCacheOptions,
    fetcher: F,
) -> Result<Option<CachedFetchResult>>
where
    F: FnOnce() -> Result<Option<ExternalFetchResult>>,
{
    if !cache_options.use_cache {
        return Ok(fetcher()?.map(|result| CachedFetchResult {
            result,
            status: ResearchCacheStatus::Bypass,
            cache_path: None,
        }));
    }

    ensure_research_cache_layout(paths)?;
    let key = cache_key_for_fetch(url, options);
    if !cache_options.refresh {
        for cache_path in cache_candidate_paths(paths, &key) {
            if let Some(result) = read_cached_fetch(&cache_path)? {
                return Ok(Some(CachedFetchResult {
                    result,
                    status: ResearchCacheStatus::Hit,
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
            ResearchCacheStatus::Refresh
        } else {
            ResearchCacheStatus::Miss
        },
        cache_path: Some(cache_path),
    }))
}

fn ensure_research_cache_layout(paths: &ResolvedPaths) -> Result<()> {
    for directory in [
        paths.research_cache_dir().join("documents"),
        paths.research_cache_dir().join("rendered"),
    ] {
        fs::create_dir_all(&directory)
            .with_context(|| format!("failed to create {}", normalize_path(&directory)))?;
    }
    Ok(())
}

fn cache_key_for_fetch(url: &str, options: &ExternalFetchOptions) -> String {
    compute_hash(&format!(
        "fetch|url={}|format={}|profile={}|max_bytes={}",
        url.trim(),
        cache_format_label(options.format),
        cache_profile_label(options.profile),
        options.max_bytes
    ))
}

fn cache_candidate_paths(paths: &ResolvedPaths, key: &str) -> [PathBuf; 2] {
    [
        paths
            .research_cache_dir()
            .join("documents")
            .join(format!("{key}.json")),
        paths
            .research_cache_dir()
            .join("rendered")
            .join(format!("{key}.json")),
    ]
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
        ExternalFetchProfile::Legacy => "legacy",
        ExternalFetchProfile::Research => "research",
    }
}

fn read_cached_fetch(path: &Path) -> Result<Option<ExternalFetchResult>> {
    if !path.exists() {
        return Ok(None);
    }

    let payload = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", normalize_path(path)))?;
    match serde_json::from_str::<CachedFetchDocument>(&payload) {
        Ok(document) => Ok(Some(document.result)),
        Err(_) => Ok(None),
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
        schema_version: "research_fetch_cache_v1".to_string(),
        result: result.clone(),
    })?;
    fs::write(&path, payload.as_bytes())
        .with_context(|| format!("failed to write {}", normalize_path(&path)))?;
    if stale_path.exists() {
        fs::remove_file(&stale_path)
            .with_context(|| format!("failed to remove {}", normalize_path(&stale_path)))?;
    }
    Ok(path)
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
        ResearchCacheOptions, ResearchCacheStatus, cache_key_for_fetch, cache_path_for_result,
        ensure_research_cache_layout, fetch_page_by_url_cached_with,
    };
    use crate::research::model::{
        ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult,
    };
    use crate::runtime::{ResolvedPaths, ValueSource};

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
            timestamp: "123".to_string(),
            extract: Some("summary".to_string()),
            url: "https://example.com/article".to_string(),
            source_wiki: "web".to_string(),
            source_domain: "example.com".to_string(),
            content_format: "text".to_string(),
            content_hash: crate::support::compute_hash(content),
            revision_id: None,
            display_title: None,
            rendered_fetch_mode: None,
            canonical_url: Some("https://example.com/article".to_string()),
            site_name: Some("Example".to_string()),
            byline: None,
            published_at: None,
            fetch_mode: Some(crate::research::FetchMode::Static),
            extraction_quality: Some(crate::research::ExtractionQuality::Medium),
        }
    }

    fn mediawiki_rendered_result(content: &str) -> ExternalFetchResult {
        ExternalFetchResult {
            title: "Main Page".to_string(),
            content: content.to_string(),
            timestamp: "456".to_string(),
            extract: Some("main page".to_string()),
            url: "https://wiki.remilia.org/wiki/Main_Page".to_string(),
            source_wiki: "mediawiki".to_string(),
            source_domain: "wiki.remilia.org".to_string(),
            content_format: "html".to_string(),
            content_hash: crate::support::compute_hash(content),
            revision_id: Some(1),
            display_title: Some("Main Page".to_string()),
            rendered_fetch_mode: Some(crate::research::RenderedFetchMode::ParseApi),
            canonical_url: Some("https://wiki.remilia.org/wiki/Main_Page".to_string()),
            site_name: Some("Remilia Wiki".to_string()),
            byline: None,
            published_at: None,
            fetch_mode: None,
            extraction_quality: None,
        }
    }

    #[test]
    fn ensures_cache_layout() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());

        ensure_research_cache_layout(&paths).expect("layout should initialize");

        assert!(paths.research_cache_dir().join("documents").exists());
        assert!(paths.research_cache_dir().join("rendered").exists());
    }

    #[test]
    fn cached_fetch_hits_after_first_write() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let count = Cell::new(0usize);
        let options = ExternalFetchOptions {
            format: ExternalFetchFormat::Html,
            max_bytes: 10_000,
            profile: ExternalFetchProfile::Research,
        };

        let first = fetch_page_by_url_cached_with(
            &paths,
            "https://example.com/article",
            &options,
            &ResearchCacheOptions::default(),
            || {
                count.set(count.get() + 1);
                Ok(Some(sample_result("first body")))
            },
        )
        .expect("first fetch should succeed")
        .expect("first fetch should exist");
        assert_eq!(first.status, ResearchCacheStatus::Miss);
        assert_eq!(count.get(), 1);

        let second = fetch_page_by_url_cached_with(
            &paths,
            "https://example.com/article",
            &options,
            &ResearchCacheOptions::default(),
            || {
                count.set(count.get() + 1);
                Ok(Some(sample_result("second body")))
            },
        )
        .expect("second fetch should succeed")
        .expect("second fetch should exist");
        assert_eq!(second.status, ResearchCacheStatus::Hit);
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
            profile: ExternalFetchProfile::Research,
        };

        let _ = fetch_page_by_url_cached_with(
            &paths,
            "https://example.com/article",
            &options,
            &ResearchCacheOptions::default(),
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
            &ResearchCacheOptions {
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

        assert_eq!(refreshed.status, ResearchCacheStatus::Refresh);
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
            profile: ExternalFetchProfile::Research,
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
}
