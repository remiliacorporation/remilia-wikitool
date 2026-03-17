use std::collections::{BTreeMap, HashSet};
use std::env;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use serde::Serialize;

const DEFAULT_USER_AGENT: &str = crate::config::DEFAULT_USER_AGENT;

#[derive(Debug, Clone, Serialize)]
pub struct SeoInspectResult {
    pub url: String,
    pub title: Option<String>,
    pub meta: BTreeMap<String, Vec<String>>,
    pub canonical: Option<String>,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetResource {
    pub url: String,
    pub resource_type: String,
    pub tag: String,
    pub size_bytes: Option<u64>,
    pub content_type: Option<String>,
    pub cache_control: Option<String>,
    pub age: Option<String>,
    pub x_cache: Option<String>,
    pub x_varnish: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetSummary {
    pub known_bytes: u64,
    pub unknown_count: usize,
    pub largest: Vec<NetResource>,
    pub cache_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetInspectResult {
    pub url: String,
    pub total_resources: usize,
    pub inspected: usize,
    pub summary: NetSummary,
    pub resources: Vec<NetResource>,
}

#[derive(Debug, Clone)]
pub struct NetInspectOptions {
    pub limit: usize,
    pub probe: bool,
}

impl Default for NetInspectOptions {
    fn default() -> Self {
        Self {
            limit: 25,
            probe: true,
        }
    }
}

#[derive(Debug, Clone)]
struct TagMatch {
    attrs: BTreeMap<String, String>,
}

pub fn seo_inspect(
    target: &str,
    override_url: Option<&str>,
    default_wiki_url: Option<&str>,
    article_path: Option<&str>,
) -> Result<SeoInspectResult> {
    let requested_url = resolve_target_url(target, override_url, default_wiki_url, article_path)?;
    let client = build_http_client()?;
    let response = client
        .get(&requested_url)
        .header("User-Agent", user_agent())
        .send()
        .with_context(|| format!("failed to fetch {requested_url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("HTTP {} while fetching {}", status.as_u16(), requested_url);
    }
    let final_url = response.url().to_string();
    let html = response.text().context("failed to read response body")?;
    let head = extract_head(&html);
    let title = extract_title(&head);
    let meta_tags = scan_tags(&head, "meta");
    let link_tags = scan_tags(&head, "link");
    let meta = collect_meta(&meta_tags);
    let canonical = find_canonical(&link_tags);
    let missing = detect_missing(&meta, title.as_deref(), canonical.as_deref());

    Ok(SeoInspectResult {
        url: final_url,
        title,
        meta,
        canonical,
        missing,
    })
}

pub fn net_inspect(
    target: &str,
    override_url: Option<&str>,
    default_wiki_url: Option<&str>,
    article_path: Option<&str>,
    options: &NetInspectOptions,
) -> Result<NetInspectResult> {
    let requested_url = resolve_target_url(target, override_url, default_wiki_url, article_path)?;
    let client = build_http_client()?;
    let response = client
        .get(&requested_url)
        .header("User-Agent", user_agent())
        .send()
        .with_context(|| format!("failed to fetch {requested_url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("HTTP {} while fetching {}", status.as_u16(), requested_url);
    }
    let final_url = response.url().to_string();
    let html = response.text().context("failed to read response body")?;
    let head = extract_head(&html);

    let resources = collect_resources(&head, &html, &final_url);
    let total_resources = resources.len();
    let limit = options.limit.max(1);
    let mut inspected = resources.into_iter().take(limit).collect::<Vec<_>>();
    if options.probe {
        for resource in &mut inspected {
            probe_resource(&client, resource);
        }
    }
    let summary = build_net_summary(&inspected);

    Ok(NetInspectResult {
        url: final_url,
        total_resources,
        inspected: inspected.len(),
        summary,
        resources: inspected,
    })
}

pub fn resolve_target_url(
    target: &str,
    override_url: Option<&str>,
    default_wiki_url: Option<&str>,
    article_path: Option<&str>,
) -> Result<String> {
    if let Some(url) = override_url {
        return Ok(url.trim().to_string());
    }
    if is_http_url(target) {
        return Ok(target.to_string());
    }

    let base = default_wiki_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no wiki URL configured; set wiki.url in config.toml, WIKI_URL env, or use --url"
            )
        })?;
    let normalized_title = target.replace(' ', "_");
    let pattern = article_path.unwrap_or(crate::config::DEFAULT_ARTICLE_PATH);
    let path = pattern.replace("$1", &normalized_title);
    let full = format!("{}{}", trim_trailing_slash(base), path);
    Url::parse(&full).with_context(|| format!("invalid constructed URL: {full}"))?;
    Ok(full)
}

fn build_http_client() -> Result<Client> {
    let timeout_ms = env::var("WIKI_HTTP_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(30_000);
    Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .context("failed to build HTTP client")
}

fn user_agent() -> String {
    env::var("WIKI_USER_AGENT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string())
}

fn extract_head(html: &str) -> String {
    let Some(head_start) = find_tag_start(html, "head", 0) else {
        return html.to_string();
    };
    let Some(open_end) = find_tag_end(html, head_start) else {
        return html.to_string();
    };
    let Some(close_index) = index_of_ignore_case(html, "</head>", open_end + 1) else {
        return html[open_end + 1..].to_string();
    };
    html[open_end + 1..close_index].to_string()
}

fn extract_title(html: &str) -> Option<String> {
    let start = find_tag_start(html, "title", 0)?;
    let open_end = find_tag_end(html, start)?;
    let close = index_of_ignore_case(html, "</title>", open_end + 1)?;
    let raw = &html[open_end + 1..close];
    let decoded = decode_html(raw);
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn scan_tags(html: &str, tag_name: &str) -> Vec<TagMatch> {
    let name = tag_name.to_ascii_lowercase();
    let mut output = Vec::new();
    let mut index = 0usize;

    while index < html.len() {
        let Some(lt) = html[index..].find('<') else {
            break;
        };
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", at + 4) {
                index = end + 3;
            } else {
                index = html.len();
            }
            continue;
        }
        if is_tag_at(html, at, &name) {
            let Some(end) = find_tag_end(html, at) else {
                break;
            };
            let raw = &html[at..=end];
            output.push(TagMatch {
                attrs: parse_attributes(raw, &name),
            });
            index = end + 1;
            continue;
        }
        index = at + 1;
    }

    output
}

fn collect_meta(tags: &[TagMatch]) -> BTreeMap<String, Vec<String>> {
    let mut meta = BTreeMap::new();
    for tag in tags {
        let key = tag
            .attrs
            .get("property")
            .or_else(|| tag.attrs.get("name"))
            .map(|value| value.to_ascii_lowercase());
        let Some(key) = key else {
            continue;
        };
        let Some(content) = tag.attrs.get("content") else {
            continue;
        };
        let content = decode_html(content).trim().to_string();
        if content.is_empty() {
            continue;
        }
        meta.entry(key).or_insert_with(Vec::new).push(content);
    }
    meta
}

fn find_canonical(tags: &[TagMatch]) -> Option<String> {
    for tag in tags {
        let rel = tag
            .attrs
            .get("rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if !rel.contains("canonical") {
            continue;
        }
        if let Some(href) = tag.attrs.get("href") {
            let decoded = decode_html(href).trim().to_string();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    None
}

fn detect_missing(
    meta: &BTreeMap<String, Vec<String>>,
    title: Option<&str>,
    canonical: Option<&str>,
) -> Vec<String> {
    let mut missing = Vec::new();
    if title.is_none() {
        missing.push("title tag".to_string());
    }
    if !meta.contains_key("description") {
        missing.push("meta description".to_string());
    }
    if !meta.contains_key("og:title") {
        missing.push("og:title".to_string());
    }
    if !meta.contains_key("og:type") {
        missing.push("og:type".to_string());
    }
    if !meta.contains_key("og:image") {
        missing.push("og:image".to_string());
    }
    if !meta.contains_key("og:url") {
        missing.push("og:url".to_string());
    }
    if canonical.is_none() {
        missing.push("canonical link".to_string());
    }
    if !meta.contains_key("twitter:card") {
        missing.push("twitter:card".to_string());
    }
    missing
}

fn collect_resources(head: &str, html: &str, base_url: &str) -> Vec<NetResource> {
    let mut resources = Vec::new();
    let mut seen = HashSet::new();

    for tag in scan_tags(html, "script") {
        if let Some(src) = tag.attrs.get("src") {
            push_resource(&mut resources, &mut seen, src, base_url, "script", "script");
        }
    }

    for tag in scan_tags(head, "link") {
        let rel = tag
            .attrs
            .get("rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let Some(href) = tag.attrs.get("href") else {
            continue;
        };
        if rel.contains("stylesheet") {
            push_resource(&mut resources, &mut seen, href, base_url, "style", "link");
        } else if rel.contains("preload") || rel.contains("modulepreload") {
            let resource_type = tag
                .attrs
                .get("as")
                .map(|value| value.to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "preload".to_string());
            push_resource(
                &mut resources,
                &mut seen,
                href,
                base_url,
                &resource_type,
                "link",
            );
        }
    }

    for tag in scan_tags(html, "img") {
        if let Some(src) = tag.attrs.get("src").cloned().or_else(|| {
            tag.attrs
                .get("srcset")
                .and_then(|value| extract_srcset(value))
        }) {
            push_resource(&mut resources, &mut seen, &src, base_url, "image", "img");
        }
    }

    for tag in scan_tags(html, "source") {
        if let Some(src) = tag.attrs.get("src").cloned().or_else(|| {
            tag.attrs
                .get("srcset")
                .and_then(|value| extract_srcset(value))
        }) {
            let resource_type = tag
                .attrs
                .get("type")
                .and_then(|value| value.split('/').next())
                .filter(|value| !value.is_empty())
                .unwrap_or("source");
            push_resource(
                &mut resources,
                &mut seen,
                &src,
                base_url,
                resource_type,
                "source",
            );
        }
    }

    resources
}

fn push_resource(
    resources: &mut Vec<NetResource>,
    seen: &mut HashSet<String>,
    raw_url: &str,
    base_url: &str,
    resource_type: &str,
    tag: &str,
) {
    let decoded = decode_html(raw_url.trim());
    let normalized = decoded.trim();
    if normalized.is_empty() {
        return;
    }
    if starts_with_ignore_case(normalized, "data:")
        || starts_with_ignore_case(normalized, "javascript:")
    {
        return;
    }

    let absolute = match Url::parse(normalized).or_else(|_| Url::parse(base_url)?.join(normalized))
    {
        Ok(url) => url.to_string(),
        Err(_) => return,
    };
    if !seen.insert(absolute.clone()) {
        return;
    }

    resources.push(NetResource {
        url: absolute,
        resource_type: resource_type.to_string(),
        tag: tag.to_string(),
        size_bytes: None,
        content_type: None,
        cache_control: None,
        age: None,
        x_cache: None,
        x_varnish: None,
    });
}

fn probe_resource(client: &Client, resource: &mut NetResource) {
    let Ok(response) = client
        .head(&resource.url)
        .header("User-Agent", user_agent())
        .send()
    else {
        return;
    };
    if !response.status().is_success() {
        return;
    }

    if let Some(length) = response.headers().get("content-length")
        && let Ok(length) = length.to_str()
        && let Ok(length) = length.parse::<u64>()
    {
        resource.size_bytes = Some(length);
    }
    resource.content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    resource.cache_control = response
        .headers()
        .get("cache-control")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    resource.age = response
        .headers()
        .get("age")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    resource.x_cache = response
        .headers()
        .get("x-cache")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    resource.x_varnish = response
        .headers()
        .get("x-varnish")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
}

fn build_net_summary(resources: &[NetResource]) -> NetSummary {
    let mut known_bytes = 0u64;
    let mut unknown_count = 0usize;
    let mut cache_warnings = Vec::new();

    for resource in resources {
        if let Some(size) = resource.size_bytes {
            known_bytes = known_bytes.saturating_add(size);
        } else {
            unknown_count += 1;
        }

        if let Some(cache_control) = resource.cache_control.as_deref() {
            if cache_control.to_ascii_lowercase().contains("no-store") {
                cache_warnings.push(format!("no-store: {}", resource.url));
            }
        } else {
            cache_warnings.push(format!("missing cache-control: {}", resource.url));
        }
    }

    let mut largest = resources
        .iter()
        .filter(|resource| resource.size_bytes.is_some())
        .cloned()
        .collect::<Vec<_>>();
    largest.sort_by(|left, right| right.size_bytes.cmp(&left.size_bytes));
    largest.truncate(5);

    NetSummary {
        known_bytes,
        unknown_count,
        largest,
        cache_warnings,
    }
}

fn find_tag_start(html: &str, tag_name: &str, start: usize) -> Option<usize> {
    let mut index = start;
    while index < html.len() {
        let lt = html[index..].find('<')?;
        let at = index + lt;
        if is_tag_at(html, at, tag_name) {
            return Some(at);
        }
        index = at + 1;
    }
    None
}

fn is_tag_at(html: &str, at: usize, tag_name: &str) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(at).copied() != Some(b'<') {
        return false;
    }
    let mut index = at + 1;
    if index >= bytes.len() {
        return false;
    }
    if bytes[index] == b'/' {
        return false;
    }
    for expected in tag_name.as_bytes() {
        let Some(actual) = bytes.get(index) else {
            return false;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return false;
        }
        index += 1;
    }
    matches!(
        bytes.get(index).copied(),
        Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/')
    )
}

fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut index = start;
    let mut quote = None::<u8>;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if byte == b'>' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn parse_attributes(tag_raw: &str, tag_name: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = tag_raw.as_bytes();
    let mut index = tag_name.len() + 1;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'>' {
            break;
        }
        if byte == b'/' || byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        let name_start = index;
        while index < bytes.len() {
            let ch = bytes[index];
            if ch.is_ascii_whitespace() || ch == b'=' || ch == b'>' || ch == b'/' {
                break;
            }
            index += 1;
        }
        if name_start == index {
            index += 1;
            continue;
        }
        let name = tag_raw[name_start..index].trim().to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let mut value = String::new();
        if bytes.get(index).copied() == Some(b'=') {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if let Some(quote) = bytes
                .get(index)
                .copied()
                .filter(|byte| *byte == b'"' || *byte == b'\'')
            {
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                value = tag_raw[value_start..index].to_string();
                if bytes.get(index).copied() == Some(quote) {
                    index += 1;
                }
            } else {
                let value_start = index;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                {
                    index += 1;
                }
                value = tag_raw[value_start..index].to_string();
            }
        }

        if !value.is_empty() {
            attrs.insert(name, value);
        } else {
            attrs.entry(name).or_default();
        }
    }

    attrs
}

fn index_of_ignore_case(text: &str, search: &str, start: usize) -> Option<usize> {
    if search.is_empty() {
        return Some(start);
    }
    let text_bytes = text.as_bytes();
    let search_bytes = search.as_bytes();
    if search_bytes.len() > text_bytes.len() || start >= text_bytes.len() {
        return None;
    }

    let last_start = text_bytes.len().saturating_sub(search_bytes.len());
    for index in start..=last_start {
        let mut matched = true;
        for offset in 0..search_bytes.len() {
            if !text_bytes[index + offset].eq_ignore_ascii_case(&search_bytes[offset]) {
                matched = false;
                break;
            }
        }
        if matched {
            return Some(index);
        }
    }
    None
}

fn starts_with_at(text: &str, index: usize, sequence: &str) -> bool {
    if index + sequence.len() > text.len() {
        return false;
    }
    &text[index..index + sequence.len()] == sequence
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    if text.len() < prefix.len() {
        return false;
    }
    text[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn decode_html(text: &str) -> String {
    let mut value = text.to_string();
    value = value.replace("&amp;", "&");
    value = value.replace("&quot;", "\"");
    value = value.replace("&#39;", "'");
    value = value.replace("&lt;", "<");
    value = value.replace("&gt;", ">");
    value
}

fn extract_srcset(srcset: &str) -> Option<String> {
    let first = srcset.split(',').next()?.trim();
    if first.is_empty() {
        return None;
    }
    let value = first
        .split_whitespace()
        .next()
        .map(ToString::to_string)
        .unwrap_or_default();
    if value.is_empty() { None } else { Some(value) }
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn trim_trailing_slash(value: &str) -> &str {
    let mut end = value.len();
    let bytes = value.as_bytes();
    while end > 0 && bytes[end - 1] == b'/' {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::{extract_head, resolve_target_url, scan_tags};

    #[test]
    fn resolve_target_url_uses_override() {
        let url = resolve_target_url(
            "Alpha Beta",
            Some("https://wiki.example.org/wiki/Fallback"),
            None,
            None,
        )
        .expect("url");
        assert_eq!(url, "https://wiki.example.org/wiki/Fallback");
    }

    #[test]
    fn resolve_target_url_uses_default_article_path() {
        let url = resolve_target_url("Alpha Beta", None, Some("https://wiki.example.org"), None)
            .expect("url");
        assert_eq!(url, "https://wiki.example.org/Alpha_Beta");
    }

    #[test]
    fn resolve_target_url_uses_custom_article_path() {
        let url = resolve_target_url(
            "Alpha Beta",
            None,
            Some("https://wiki.example.org"),
            Some("/wiki/$1"),
        )
        .expect("url");
        assert_eq!(url, "https://wiki.example.org/wiki/Alpha_Beta");
    }

    #[test]
    fn html_scanner_extracts_head_meta_and_links() {
        let html = r#"
<html><head>
<title>Example</title>
<meta name="description" content="hello" />
<link rel="canonical" href="https://example.org/wiki/Alpha" />
</head><body><script src="/x.js"></script></body></html>
"#;
        let head = extract_head(html);
        let meta = scan_tags(&head, "meta");
        let links = scan_tags(&head, "link");
        assert_eq!(meta.len(), 1);
        assert_eq!(links.len(), 1);
        assert_eq!(
            meta[0].attrs.get("name").map(String::as_str),
            Some("description")
        );
    }
}
