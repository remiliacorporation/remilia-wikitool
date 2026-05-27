use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::runtime::ResolvedPaths;
use crate::support::{normalize_path, now_iso8601_utc, unix_timestamp};

const DEFAULT_USER_AGENT: &str = crate::config::DEFAULT_USER_AGENT;
const ARCHIVE_ACCEPT: &str = "*/*";

#[derive(Debug, Clone)]
pub struct WebArchiveOptions {
    pub max_pages: usize,
    pub max_bytes: usize,
    /// Maximum link depth from the seed URL (seed is depth 0). Bounds breadth-first
    /// crawl reach, which is the only guardrail against runaway cross-host crawls
    /// once `same_host_only` is disabled.
    pub max_depth: usize,
    /// Aggregate ceiling across the whole crawl. `max_bytes` bounds one response;
    /// this bounds the total stored, so a large `max_pages` cannot accumulate
    /// unbounded disk usage.
    pub max_total_bytes: usize,
    pub same_host_only: bool,
    pub include_page_requisites: bool,
    pub output_dir: Option<PathBuf>,
}

impl Default for WebArchiveOptions {
    fn default() -> Self {
        Self {
            max_pages: 1_000,
            max_bytes: 50_000_000,
            max_depth: 8,
            max_total_bytes: 1_000_000_000,
            same_host_only: true,
            include_page_requisites: true,
            output_dir: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebArchiveReport {
    pub schema_version: String,
    pub source_url: String,
    pub origin_host: String,
    pub crawled_at: String,
    pub output_dir: String,
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub entries: Vec<WebArchiveEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebArchiveEntry {
    pub url: String,
    pub fetched_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn archive_web_site(
    paths: &ResolvedPaths,
    source_url: &str,
    options: WebArchiveOptions,
) -> Result<WebArchiveReport> {
    if options.max_pages == 0 {
        bail!("archive requires max_pages >= 1");
    }
    if options.max_bytes == 0 {
        bail!("archive requires max_bytes >= 1");
    }
    if options.max_total_bytes == 0 {
        bail!("archive requires max_total_bytes >= 1");
    }

    let start = Url::parse(source_url)
        .with_context(|| format!("failed to parse source URL: {source_url}"))?;
    let origin_host = start
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("source URL has no host: {source_url}"))?
        .to_ascii_lowercase();
    let output_dir = options
        .output_dir
        .clone()
        .unwrap_or_else(|| default_archive_dir(paths, &origin_host));
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;

    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build web archive HTTP client")?;
    let mut queue = VecDeque::from([(strip_fragment(start), 0usize)]);
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    let mut total_bytes = 0usize;

    while let Some((url, depth)) = queue.pop_front() {
        if entries.len() >= options.max_pages {
            break;
        }
        if total_bytes >= options.max_total_bytes {
            break;
        }
        if !seen.insert(url.to_string()) {
            continue;
        }
        sleep(Duration::from_millis(100));

        let mut entry = WebArchiveEntry {
            url: url.to_string(),
            fetched_at: now_iso8601_utc(),
            final_url: None,
            status: None,
            ok: false,
            content_type: None,
            bytes: None,
            sha256: None,
            path: None,
            error: None,
        };

        let Some(byte_limit) =
            response_byte_limit(options.max_bytes, options.max_total_bytes, total_bytes)
        else {
            break;
        };

        match fetch_archive_candidate(&client, url.as_str(), byte_limit) {
            Ok(candidate) => {
                let local_path = archive_path_for_url(
                    &output_dir,
                    &candidate.final_url,
                    candidate.content_type.as_deref(),
                )?;
                if let Some(parent) = local_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create archive directory {}", parent.display())
                    })?;
                }
                fs::write(&local_path, &candidate.body)
                    .with_context(|| format!("failed to write {}", local_path.display()))?;

                let digest = Sha256::digest(&candidate.body);
                let sha256 = digest
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let content_type = candidate.content_type.unwrap_or_default();
                let final_url = candidate.final_url;
                entry.final_url = Some(final_url.to_string());
                entry.status = Some(candidate.status);
                entry.ok = (200..400).contains(&candidate.status);
                entry.content_type = if content_type.is_empty() {
                    None
                } else {
                    Some(content_type.clone())
                };
                entry.bytes = Some(candidate.body.len());
                entry.sha256 = Some(sha256);
                entry.path = Some(relative_archive_path(&output_dir, &local_path));
                total_bytes = total_bytes.saturating_add(candidate.body.len());

                if entry.ok && depth < options.max_depth {
                    let enqueue_assets =
                        options.include_page_requisites && is_parseable_requisite(&content_type);
                    let enqueue_pages = is_html_like(&content_type);
                    if enqueue_pages || enqueue_assets {
                        let body = String::from_utf8_lossy(&candidate.body);
                        let links = if is_css_like(&content_type, final_url.path()) {
                            extract_css_urls(&body)
                        } else {
                            extract_html_urls(&body)
                        };
                        for link in links {
                            if let Some(next) = resolve_archive_url(&final_url, &link)
                                && should_enqueue(&next, &origin_host, options.same_host_only)
                                && !seen.contains(next.as_str())
                            {
                                queue.push_back((next, depth + 1));
                            }
                        }
                    }
                }
            }
            Err(error) => {
                entry.error = Some(error.to_string());
            }
        }
        entries.push(entry);
    }

    let succeeded = entries.iter().filter(|entry| entry.ok).count();
    let failed = entries.len().saturating_sub(succeeded);
    let report = WebArchiveReport {
        schema_version: "web_archive_manifest_v1".to_string(),
        source_url: source_url.to_string(),
        origin_host,
        crawled_at: now_iso8601_utc(),
        output_dir: normalize_path(&output_dir),
        attempted: entries.len(),
        succeeded,
        failed,
        entries,
    };
    fs::write(
        output_dir.join("manifest.json"),
        serde_json::to_string_pretty(&report)?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            output_dir.join("manifest.json").display()
        )
    })?;
    Ok(report)
}

struct ArchiveCandidate {
    final_url: Url,
    status: u16,
    content_type: Option<String>,
    body: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct ResponseByteLimit {
    bytes: usize,
    constrained_by_total: bool,
}

fn response_byte_limit(
    max_bytes: usize,
    max_total_bytes: usize,
    total_bytes: usize,
) -> Option<ResponseByteLimit> {
    let remaining_total = max_total_bytes.saturating_sub(total_bytes);
    if remaining_total == 0 {
        return None;
    }
    Some(ResponseByteLimit {
        bytes: max_bytes.min(remaining_total),
        constrained_by_total: remaining_total < max_bytes,
    })
}

fn fetch_archive_candidate(
    client: &Client,
    url: &str,
    byte_limit: ResponseByteLimit,
) -> Result<ArchiveCandidate> {
    let mut response = client
        .get(url)
        .header("User-Agent", DEFAULT_USER_AGENT)
        .header("Accept", ARCHIVE_ACCEPT)
        .send()
        .with_context(|| format!("failed to fetch {url}"))?;
    let final_url = response.url().clone();
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let mut body = Vec::new();
    let limit = u64::try_from(byte_limit.bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    response
        .by_ref()
        .take(limit)
        .read_to_end(&mut body)
        .with_context(|| format!("failed to read {url}"))?;
    if body.len() > byte_limit.bytes {
        if byte_limit.constrained_by_total {
            bail!(
                "response would exceed --max-total-bytes remaining limit ({}) for {url}",
                byte_limit.bytes
            );
        }
        bail!(
            "response exceeded --max-bytes limit ({}) for {url}",
            byte_limit.bytes
        );
    }
    Ok(ArchiveCandidate {
        final_url,
        status,
        content_type,
        body,
    })
}

fn default_archive_dir(paths: &ResolvedPaths, host: &str) -> PathBuf {
    let timestamp = unix_timestamp().unwrap_or(0);
    paths.state_dir.join("backups").join("web").join(format!(
        "{}-{}",
        sanitize_path_component(host),
        timestamp
    ))
}

fn archive_path_for_url(root: &Path, url: &Url, content_type: Option<&str>) -> Result<PathBuf> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("archive URL has no host: {url}"))?;
    let mut path = root.join(sanitize_path_component(host));
    let mut segments: Vec<String> = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(sanitize_path_component)
                .collect()
        })
        .unwrap_or_default();
    if segments.is_empty() || url.path().ends_with('/') {
        segments.push("index.html".to_string());
    } else if Path::new(segments.last().expect("segments"))
        .extension()
        .is_none()
        && content_type.is_some_and(is_html_like)
    {
        let last = segments.last_mut().expect("segments");
        last.push_str(".html");
    }
    for segment in segments {
        path.push(segment);
    }
    Ok(path)
}

fn relative_archive_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(normalize_path)
        .unwrap_or_else(|_| normalize_path(path))
}

fn sanitize_path_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ' ') {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "_".to_string()
    } else {
        output
    }
}

fn strip_fragment(mut url: Url) -> Url {
    url.set_fragment(None);
    url
}

fn should_enqueue(url: &Url, origin_host: &str, same_host_only: bool) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    if !same_host_only {
        return true;
    }
    url.host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case(origin_host))
}

fn resolve_archive_url(base: &Url, candidate: &str) -> Option<Url> {
    let trimmed = candidate.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || starts_with_ascii_case(trimmed, "mailto:")
        || starts_with_ascii_case(trimmed, "javascript:")
        || starts_with_ascii_case(trimmed, "data:")
    {
        return None;
    }
    let mut url = base.join(trimmed).ok()?;
    url.set_fragment(None);
    Some(url)
}

fn is_parseable_requisite(content_type: &str) -> bool {
    is_html_like(content_type) || is_css_like(content_type, "")
}

fn is_html_like(content_type: &str) -> bool {
    content_type.contains("text/html") || content_type.contains("application/xhtml+xml")
}

fn is_css_like(content_type: &str, path: &str) -> bool {
    content_type.contains("text/css") || path.ends_with(".css")
}

fn extract_html_urls(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in ["href", "src", "poster", "data-src", "srcset"] {
        collect_attr_values(html, attr, &mut out);
    }
    out
}

fn collect_attr_values(html: &str, attr: &str, out: &mut Vec<String>) {
    let bytes = html.as_bytes();
    let needle = attr.as_bytes();
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if !bytes[i..i + needle.len()].eq_ignore_ascii_case(needle) {
            i += 1;
            continue;
        }
        let mut j = i + needle.len();
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'=' {
            i += 1;
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        let value = read_quoted_or_bare(bytes, &mut j);
        if attr.eq_ignore_ascii_case("srcset") {
            for candidate in split_srcset(&value) {
                out.push(candidate);
            }
        } else if !value.trim().is_empty() {
            out.push(value.trim().to_string());
        }
        i = j.saturating_add(1);
    }
}

fn read_quoted_or_bare(bytes: &[u8], cursor: &mut usize) -> String {
    if *cursor >= bytes.len() {
        return String::new();
    }
    let quote = bytes[*cursor];
    let start;
    let end;
    if quote == b'"' || quote == b'\'' {
        *cursor += 1;
        start = *cursor;
        while *cursor < bytes.len() && bytes[*cursor] != quote {
            *cursor += 1;
        }
        end = *cursor;
    } else {
        start = *cursor;
        while *cursor < bytes.len()
            && !bytes[*cursor].is_ascii_whitespace()
            && bytes[*cursor] != b'>'
        {
            *cursor += 1;
        }
        end = *cursor;
    }
    String::from_utf8_lossy(&bytes[start..end]).to_string()
}

fn split_srcset(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in value.split(',') {
        let mut item = String::new();
        for ch in part.trim().chars() {
            if ch.is_whitespace() {
                break;
            }
            item.push(ch);
        }
        if !item.is_empty() {
            out.push(item);
        }
    }
    out
}

fn extract_css_urls(css: &str) -> Vec<String> {
    let bytes = css.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if !bytes[i..i + 4].eq_ignore_ascii_case(b"url(") {
            i += 1;
            continue;
        }
        i += 4;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let value = read_css_url_value(bytes, &mut i);
        if !value.trim().is_empty() {
            out.push(value.trim().to_string());
        }
    }
    out
}

fn read_css_url_value(bytes: &[u8], cursor: &mut usize) -> String {
    if *cursor >= bytes.len() {
        return String::new();
    }
    let quote = bytes[*cursor];
    let mut value = Vec::new();
    if quote == b'"' || quote == b'\'' {
        *cursor += 1;
        while *cursor < bytes.len() && bytes[*cursor] != quote {
            value.push(bytes[*cursor]);
            *cursor += 1;
        }
    } else {
        while *cursor < bytes.len() && bytes[*cursor] != b')' {
            value.push(bytes[*cursor]);
            *cursor += 1;
        }
    }
    String::from_utf8_lossy(&value).to_string()
}

fn starts_with_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        extract_css_urls, extract_html_urls, resolve_archive_url, response_byte_limit,
        sanitize_path_component, should_enqueue, split_srcset,
    };
    use reqwest::Url;

    #[test]
    fn extracts_html_requisites_and_links() {
        let urls = extract_html_urls(
            r#"<a href="/page">p</a><img src='image.png'><source srcset="a.png 1x, b.png 2x">"#,
        );

        assert!(urls.contains(&"/page".to_string()));
        assert!(urls.contains(&"image.png".to_string()));
        assert!(urls.contains(&"a.png".to_string()));
        assert!(urls.contains(&"b.png".to_string()));
    }

    #[test]
    fn extracts_css_url_values() {
        let urls = extract_css_urls(
            "body{background:url('../img/bg.png')} @font-face{src:url(\"f.woff2\")}",
        );

        assert_eq!(urls, vec!["../img/bg.png", "f.woff2"]);
    }

    #[test]
    fn srcset_split_keeps_urls_only() {
        assert_eq!(split_srcset("a.png 1x, b.png 2x"), vec!["a.png", "b.png"]);
    }

    #[test]
    fn path_component_sanitizer_replaces_windows_reserved_chars() {
        assert_eq!(sanitize_path_component("a:b*c?"), "a_b_c_");
    }

    #[test]
    fn same_host_scope_rejects_foreign_hosts_and_non_http_schemes() {
        let origin = "example.com";
        let same = Url::parse("https://example.com/a").expect("url");
        let foreign = Url::parse("https://other.test/a").expect("url");
        let ftp = Url::parse("ftp://example.com/a").expect("url");

        assert!(should_enqueue(&same, origin, true));
        assert!(!should_enqueue(&foreign, origin, true));
        // span-hosts allows other hosts, but never non-http(s) schemes.
        assert!(should_enqueue(&foreign, origin, false));
        assert!(!should_enqueue(&ftp, origin, false));
    }

    #[test]
    fn resolve_archive_url_filters_non_navigable_links() {
        let base = Url::parse("https://example.com/dir/page.html").expect("url");

        assert_eq!(resolve_archive_url(&base, "#section"), None);
        assert_eq!(resolve_archive_url(&base, "mailto:a@example.com"), None);
        assert_eq!(resolve_archive_url(&base, "javascript:void(0)"), None);
        assert_eq!(resolve_archive_url(&base, "data:text/plain,hi"), None);
        assert_eq!(resolve_archive_url(&base, "   "), None);
        // Relative links resolve against the base and drop any fragment.
        let resolved = resolve_archive_url(&base, "../other.html#frag").expect("resolved");
        assert_eq!(resolved.as_str(), "https://example.com/other.html");
    }

    #[test]
    fn response_byte_limit_honors_remaining_total_budget() {
        let normal = response_byte_limit(50, 100, 25).expect("limit");
        assert_eq!(normal.bytes, 50);
        assert!(!normal.constrained_by_total);

        let total_constrained = response_byte_limit(50, 100, 80).expect("limit");
        assert_eq!(total_constrained.bytes, 20);
        assert!(total_constrained.constrained_by_total);

        assert!(response_byte_limit(50, 100, 100).is_none());
        assert!(response_byte_limit(50, 100, 150).is_none());
    }

    #[test]
    fn root_url_archives_to_host_index() {
        let root = std::path::Path::new("archive");
        let url = reqwest::Url::parse("https://example.com/").expect("url");
        let path = super::archive_path_for_url(root, &url, Some("text/html")).expect("path");

        assert_eq!(
            path,
            std::path::Path::new("archive")
                .join("example.com")
                .join("index.html")
        );
    }
}
