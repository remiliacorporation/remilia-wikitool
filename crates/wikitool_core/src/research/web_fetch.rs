use std::collections::BTreeMap;
use std::io::Read;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use serde_json::Value;

use crate::support::{compute_hash, env_value, env_value_u64, env_value_usize, unix_timestamp};

use super::model::{
    ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult, ExtractionQuality, FetchMode,
};
use super::url::decode_title;

const DEFAULT_USER_AGENT: &str = crate::config::DEFAULT_USER_AGENT;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_RETRIES: usize = 2;
const DEFAULT_RETRY_DELAY_MS: u64 = 350;

pub(crate) struct ExternalClient {
    pub(crate) client: Client,
    user_agent: String,
    retries: usize,
    retry_delay_ms: u64,
    last_request_at: Option<Instant>,
}

#[derive(Debug, Clone)]
struct TagMatch {
    attrs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct HtmlMetadata {
    title: Option<String>,
    canonical_url: Option<String>,
    site_name: Option<String>,
    byline: Option<String>,
    published_at: Option<String>,
    description: Option<String>,
}

impl ExternalClient {
    pub(crate) fn request_json(
        &mut self,
        api_url: &str,
        params: &[(&str, String)],
    ) -> Result<Value> {
        let mut pairs = Vec::with_capacity(params.len() + 2);
        pairs.push(("format".to_string(), "json".to_string()));
        pairs.push(("formatversion".to_string(), "2".to_string()));
        for (key, value) in params {
            if !value.trim().is_empty() {
                pairs.push(((*key).to_string(), value.clone()));
            }
        }

        let mut last_error = None::<String>;
        for attempt in 0..=self.retries {
            if let Some(last) = self.last_request_at {
                let elapsed = last.elapsed();
                let min_delay = Duration::from_millis(100);
                if elapsed < min_delay {
                    sleep(min_delay - elapsed);
                }
            }

            let response = self
                .client
                .get(api_url)
                .header("User-Agent", self.user_agent.clone())
                .query(&pairs)
                .send();
            self.last_request_at = Some(Instant::now());

            match response {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        last_error = Some(format!("HTTP {status}"));
                        if attempt < self.retries {
                            sleep(Duration::from_millis(
                                self.retry_delay_ms.saturating_mul(attempt as u64 + 1),
                            ));
                            continue;
                        }
                        break;
                    }
                    let payload: Value = response
                        .json()
                        .context("failed to decode external API JSON response")?;
                    if let Some(error) = payload.get("error") {
                        let code = error
                            .get("code")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown_error");
                        let info = error
                            .get("info")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown info");
                        last_error = Some(format!("api error [{code}]: {info}"));
                        if attempt < self.retries {
                            sleep(Duration::from_millis(
                                self.retry_delay_ms.saturating_mul(attempt as u64 + 1),
                            ));
                            continue;
                        }
                        break;
                    }
                    return Ok(payload);
                }
                Err(error) => {
                    last_error = Some(error.to_string());
                    if attempt < self.retries {
                        sleep(Duration::from_millis(
                            self.retry_delay_ms.saturating_mul(attempt as u64 + 1),
                        ));
                        continue;
                    }
                }
            }
        }

        let message = last_error.unwrap_or_else(|| "external API request failed".to_string());
        bail!("{message}")
    }
}

pub(crate) fn external_client() -> Result<ExternalClient> {
    let timeout_ms = env_value_u64("WIKI_HTTP_TIMEOUT_MS", DEFAULT_TIMEOUT_MS);
    let retries = env_value_usize("WIKI_HTTP_RETRIES", DEFAULT_RETRIES);
    let retry_delay_ms = env_value_u64("WIKI_HTTP_RETRY_DELAY_MS", DEFAULT_RETRY_DELAY_MS);
    let user_agent = env_value("WIKI_USER_AGENT", DEFAULT_USER_AGENT);
    let client = Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .context("failed to build external HTTP client")?;
    Ok(ExternalClient {
        client,
        user_agent,
        retries,
        retry_delay_ms,
        last_request_at: None,
    })
}

pub(crate) fn fetch_web_url(
    url: &str,
    options: &ExternalFetchOptions,
) -> Result<ExternalFetchResult> {
    let client = external_client()?;
    let response = client
        .client
        .get(url)
        .header("User-Agent", DEFAULT_USER_AGENT)
        .header(
            "Accept",
            "text/html, text/plain;q=0.9, text/markdown;q=0.9,*/*;q=0.1",
        )
        .send()
        .with_context(|| format!("failed to fetch {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("HTTP {} while fetching {}", status.as_u16(), url);
    }
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let is_text = content_type.contains("text/html")
        || content_type.contains("text/plain")
        || content_type.contains("text/markdown");
    if !is_text {
        bail!("unsupported content-type: {content_type}");
    }
    let body = read_text_body_limited(response, options.max_bytes)?;

    let parsed_url = Url::parse(&final_url).ok();
    let fallback_title = derive_title_from_url(parsed_url.as_ref(), &final_url);
    let source_domain = parsed_url
        .as_ref()
        .and_then(|value| value.host_str())
        .unwrap_or("web")
        .to_string();

    if content_type.contains("text/html") {
        return Ok(build_html_fetch_result(
            &body,
            &final_url,
            &source_domain,
            &fallback_title,
            options,
        ));
    }

    Ok(build_text_fetch_result(
        &body,
        &final_url,
        &source_domain,
        &fallback_title,
        if content_type.contains("text/markdown") {
            "markdown"
        } else {
            "text"
        },
        options,
    ))
}

fn build_html_fetch_result(
    html: &str,
    final_url: &str,
    source_domain: &str,
    fallback_title: &str,
    options: &ExternalFetchOptions,
) -> ExternalFetchResult {
    let metadata = extract_html_metadata(html, final_url);
    let title = metadata
        .title
        .clone()
        .unwrap_or_else(|| fallback_title.to_string());
    let canonical_url = metadata
        .canonical_url
        .clone()
        .or_else(|| Some(final_url.to_string()));

    match options.profile {
        ExternalFetchProfile::Legacy => {
            let extract = metadata
                .description
                .clone()
                .or_else(|| summarize_text(html, 280));
            ExternalFetchResult {
                title,
                content: html.to_string(),
                timestamp: now_timestamp_string(),
                extract,
                url: final_url.to_string(),
                source_wiki: "web".to_string(),
                source_domain: source_domain.to_string(),
                content_format: "html".to_string(),
                content_hash: compute_hash(html),
                revision_id: None,
                display_title: None,
                rendered_fetch_mode: None,
                canonical_url,
                site_name: metadata.site_name,
                byline: metadata.byline,
                published_at: metadata.published_at,
                fetch_mode: Some(FetchMode::Static),
                extraction_quality: None,
            }
        }
        ExternalFetchProfile::Research => {
            let extracted = extract_readable_text(html, options.max_bytes);
            let content = if extracted.is_empty() {
                truncate_to_byte_limit(html, options.max_bytes)
            } else {
                extracted
            };
            let extract = metadata
                .description
                .clone()
                .or_else(|| summarize_text(&content, 280));
            let extraction_quality = Some(score_extraction_quality(&content, extract.as_deref()));
            let content_hash = compute_hash(&content);

            ExternalFetchResult {
                title,
                content,
                timestamp: now_timestamp_string(),
                extract,
                url: final_url.to_string(),
                source_wiki: "web".to_string(),
                source_domain: source_domain.to_string(),
                content_format: "text".to_string(),
                content_hash,
                revision_id: None,
                display_title: None,
                rendered_fetch_mode: None,
                canonical_url,
                site_name: metadata.site_name,
                byline: metadata.byline,
                published_at: metadata.published_at,
                fetch_mode: Some(FetchMode::Static),
                extraction_quality,
            }
        }
    }
}

fn build_text_fetch_result(
    content: &str,
    final_url: &str,
    source_domain: &str,
    fallback_title: &str,
    content_format: &str,
    options: &ExternalFetchOptions,
) -> ExternalFetchResult {
    let extract = summarize_text(content, 280);
    let extraction_quality = match options.profile {
        ExternalFetchProfile::Legacy => None,
        ExternalFetchProfile::Research => {
            Some(score_extraction_quality(content, extract.as_deref()))
        }
    };
    let content = truncate_to_byte_limit(content, options.max_bytes);

    ExternalFetchResult {
        title: fallback_title.to_string(),
        content: content.clone(),
        timestamp: now_timestamp_string(),
        extract,
        url: final_url.to_string(),
        source_wiki: "web".to_string(),
        source_domain: source_domain.to_string(),
        content_format: content_format.to_string(),
        content_hash: compute_hash(&content),
        revision_id: None,
        display_title: None,
        rendered_fetch_mode: None,
        canonical_url: Some(final_url.to_string()),
        site_name: None,
        byline: None,
        published_at: None,
        fetch_mode: Some(FetchMode::Static),
        extraction_quality,
    }
}

fn derive_title_from_url(parsed_url: Option<&Url>, final_url: &str) -> String {
    parsed_url
        .and_then(|value| value.path_segments())
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.trim().is_empty())
        .map(decode_title)
        .unwrap_or_else(|| final_url.to_string())
}

fn extract_html_metadata(html: &str, final_url: &str) -> HtmlMetadata {
    let head = extract_head(html);
    let title = extract_title(&head);
    let meta = collect_meta(&scan_tags(&head, "meta"));
    let canonical_url = find_canonical(&scan_tags(&head, "link"))
        .or_else(|| meta_first(&meta, &["og:url", "twitter:url"]))
        .or_else(|| Some(final_url.to_string()));

    HtmlMetadata {
        title: meta_first(&meta, &["og:title", "twitter:title"]).or(title),
        canonical_url,
        site_name: meta_first(&meta, &["og:site_name", "application-name"]),
        byline: meta_first(
            &meta,
            &[
                "author",
                "article:author",
                "parsely-author",
                "dc.creator",
                "dc.creator.creator",
            ],
        ),
        published_at: meta_first(
            &meta,
            &[
                "article:published_time",
                "og:published_time",
                "pubdate",
                "publish-date",
                "parsely-pub-date",
                "date",
            ],
        ),
        description: meta_first(
            &meta,
            &["description", "og:description", "twitter:description"],
        ),
    }
}

fn meta_first(meta: &BTreeMap<String, Vec<String>>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        meta.get(*key)
            .and_then(|values| values.first())
            .cloned()
            .filter(|value| !value.trim().is_empty())
    })
}

fn extract_readable_text(html: &str, max_bytes: usize) -> String {
    let candidate = extract_tag_contents(html, "article")
        .or_else(|| extract_tag_contents(html, "main"))
        .or_else(|| extract_tag_contents(html, "body"))
        .unwrap_or(html);
    let mut output = String::new();
    let mut index = 0usize;
    let mut skip_depth = 0usize;

    while index < candidate.len() {
        let Some(next_lt) = candidate[index..].find('<') else {
            if skip_depth == 0 {
                append_text_segment(&mut output, &candidate[index..]);
            }
            break;
        };

        let tag_start = index + next_lt;
        if skip_depth == 0 {
            append_text_segment(&mut output, &candidate[index..tag_start]);
        }

        if starts_with_at(candidate, tag_start, "<!--") {
            if let Some(end) = index_of_ignore_case(candidate, "-->", tag_start + 4) {
                index = end + 3;
            } else {
                break;
            }
            continue;
        }

        let Some(tag_end) = find_tag_end(candidate, tag_start) else {
            break;
        };
        let raw_tag = &candidate[tag_start..=tag_end];
        let Some((tag_name, is_closing, is_self_closing)) = parse_tag_descriptor(raw_tag) else {
            index = tag_end + 1;
            continue;
        };

        if is_closing {
            if is_skip_tag(tag_name) && skip_depth > 0 {
                skip_depth -= 1;
            }
            if skip_depth == 0 && is_block_tag(tag_name) {
                append_separator(&mut output, "\n");
            }
        } else if is_skip_tag(tag_name) && !is_self_closing {
            skip_depth += 1;
        } else if skip_depth == 0 {
            if tag_name == "br" {
                append_separator(&mut output, "\n");
            } else if tag_name == "li" {
                append_separator(&mut output, "\n- ");
            } else if is_block_tag(tag_name) {
                append_separator(&mut output, "\n");
            }
        }

        index = tag_end + 1;
    }

    normalize_extracted_text(&output, max_bytes)
}

fn append_text_segment(output: &mut String, text: &str) {
    let decoded = decode_html(text);
    if !decoded.is_empty() {
        output.push_str(&decoded);
    }
}

fn append_separator(output: &mut String, separator: &str) {
    if output.is_empty() {
        return;
    }
    if separator == "\n" && output.ends_with('\n') {
        return;
    }
    output.push_str(separator);
}

fn normalize_extracted_text(value: &str, max_bytes: usize) -> String {
    let mut lines = Vec::new();
    for line in value.lines() {
        let collapsed = collapse_inline_whitespace(line);
        if !collapsed.is_empty() {
            lines.push(collapsed);
        }
    }
    truncate_to_byte_limit(&lines.join("\n"), max_bytes)
}

fn collapse_inline_whitespace(value: &str) -> String {
    let mut output = String::new();
    let mut pending_space = false;

    for ch in value.chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !output.is_empty() {
            output.push(' ');
        }
        output.push(ch);
        pending_space = false;
    }

    output.trim().to_string()
}

fn summarize_text(value: &str, max_chars: usize) -> Option<String> {
    let text = collapse_inline_whitespace(value);
    if text.is_empty() {
        return None;
    }
    let mut output = String::new();
    for ch in text.chars().take(max_chars) {
        output.push(ch);
    }
    Some(output)
}

fn score_extraction_quality(text: &str, extract: Option<&str>) -> ExtractionQuality {
    let word_count = text.split_whitespace().count();
    if word_count >= 350 {
        return ExtractionQuality::High;
    }
    if word_count >= 40 || extract.is_some_and(|value| value.len() >= 40) {
        return ExtractionQuality::Medium;
    }
    ExtractionQuality::Low
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

fn extract_tag_contents<'a>(html: &'a str, tag_name: &str) -> Option<&'a str> {
    let start = find_tag_start(html, tag_name, 0)?;
    let open_end = find_tag_end(html, start)?;
    let close_start = find_matching_close_tag(html, tag_name, open_end + 1)?;
    Some(&html[open_end + 1..close_start])
}

fn find_matching_close_tag(html: &str, tag_name: &str, start: usize) -> Option<usize> {
    let mut index = start;
    let mut depth = 1usize;

    while index < html.len() {
        let lt = html[index..].find('<')?;
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", at + 4) {
                index = end + 3;
            } else {
                return None;
            }
            continue;
        }
        if is_close_tag_at(html, at, tag_name) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(at);
            }
            index = find_tag_end(html, at)? + 1;
            continue;
        }
        if is_tag_at(html, at, tag_name) {
            let end = find_tag_end(html, at)?;
            if !is_self_closing_tag(&html[at..=end], tag_name) {
                depth += 1;
            }
            index = end + 1;
            continue;
        }
        index = at + 1;
    }

    None
}

fn parse_tag_descriptor(tag_raw: &str) -> Option<(&str, bool, bool)> {
    let bytes = tag_raw.as_bytes();
    if bytes.first().copied() != Some(b'<') {
        return None;
    }

    let mut index = 1usize;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let is_closing = bytes.get(index).copied() == Some(b'/');
    if is_closing {
        index += 1;
    }
    let name_start = index;
    while index < bytes.len() {
        let ch = bytes[index];
        if ch.is_ascii_whitespace() || ch == b'>' || ch == b'/' {
            break;
        }
        index += 1;
    }
    if name_start == index {
        return None;
    }
    let tag_name = &tag_raw[name_start..index];
    Some((tag_name, is_closing, is_self_closing_tag(tag_raw, tag_name)))
}

fn is_skip_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "script"
            | "style"
            | "noscript"
            | "template"
            | "svg"
            | "canvas"
            | "nav"
            | "header"
            | "footer"
            | "aside"
            | "form"
    )
}

fn is_block_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "p" | "div"
            | "section"
            | "article"
            | "main"
            | "li"
            | "ul"
            | "ol"
            | "table"
            | "tr"
            | "blockquote"
            | "pre"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

fn is_self_closing_tag(tag_raw: &str, tag_name: &str) -> bool {
    let normalized = tag_name.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "br" | "hr" | "img" | "meta" | "link" | "input" | "source"
    ) {
        return true;
    }
    tag_raw.trim_end().ends_with("/>")
}

fn read_text_body_limited<R: Read>(reader: R, max_bytes: usize) -> Result<String> {
    if max_bytes == 0 {
        return Ok(String::new());
    }

    let mut body = Vec::with_capacity(max_bytes.min(8192));
    let mut limited = reader.take(max_bytes as u64);
    limited
        .read_to_end(&mut body)
        .context("failed to read response body")?;

    let body = strip_utf8_bom(&body);
    let text = String::from_utf8_lossy(body);
    Ok(truncate_to_byte_limit(&text, max_bytes))
}

fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    const UTF8_BOM: &[u8; 3] = b"\xEF\xBB\xBF";

    bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes)
}

pub(crate) fn truncate_to_byte_limit(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

pub(crate) fn now_timestamp_string() -> String {
    unix_timestamp()
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "0".to_string())
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

fn is_close_tag_at(html: &str, at: usize, tag_name: &str) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(at).copied() != Some(b'<') || bytes.get(at + 1).copied() != Some(b'/') {
        return false;
    }
    let mut index = at + 2;
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
        Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>')
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

fn decode_html(text: &str) -> String {
    let mut value = text.to_string();
    value = value.replace("&amp;", "&");
    value = value.replace("&quot;", "\"");
    value = value.replace("&#39;", "'");
    value = value.replace("&lt;", "<");
    value = value.replace("&gt;", ">");
    value = value.replace("&nbsp;", " ");
    value
}

#[cfg(test)]
mod tests {
    use super::{
        build_html_fetch_result, collapse_inline_whitespace, extract_html_metadata,
        extract_readable_text,
    };
    use crate::research::model::{
        ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile, ExtractionQuality,
        FetchMode,
    };

    #[test]
    fn extracts_html_metadata() {
        let metadata = extract_html_metadata(
            r#"
            <html>
              <head>
                <title>Fallback Title</title>
                <meta property="og:title" content="OpenGraph Title" />
                <meta property="og:site_name" content="Example Site" />
                <meta name="author" content="Onno" />
                <meta property="article:published_time" content="2026-03-17T12:00:00Z" />
                <meta name="description" content="Readable summary" />
                <link rel="canonical" href="https://example.com/article" />
              </head>
            </html>
            "#,
            "https://example.com/fallback",
        );

        assert_eq!(metadata.title.as_deref(), Some("OpenGraph Title"));
        assert_eq!(
            metadata.canonical_url.as_deref(),
            Some("https://example.com/article")
        );
        assert_eq!(metadata.site_name.as_deref(), Some("Example Site"));
        assert_eq!(metadata.byline.as_deref(), Some("Onno"));
        assert_eq!(
            metadata.published_at.as_deref(),
            Some("2026-03-17T12:00:00Z")
        );
        assert_eq!(metadata.description.as_deref(), Some("Readable summary"));
    }

    #[test]
    fn extracts_readable_text_from_article() {
        let text = extract_readable_text(
            r#"
            <html>
              <body>
                <header>Site navigation</header>
                <article>
                  <h1>Headline</h1>
                  <p>First paragraph.</p>
                  <p>Second <strong>paragraph</strong>.</p>
                  <ul><li>Alpha</li><li>Beta</li></ul>
                </article>
                <footer>Footer links</footer>
              </body>
            </html>
            "#,
            10_000,
        );

        assert!(text.contains("Headline"));
        assert!(text.contains("First paragraph."));
        assert!(text.contains("Second paragraph."));
        assert!(text.contains("- Alpha"));
        assert!(text.contains("- Beta"));
        assert!(!text.contains("Site navigation"));
        assert!(!text.contains("Footer links"));
    }

    #[test]
    fn research_profile_returns_clean_text_and_metadata() {
        let result = build_html_fetch_result(
            r#"
            <html>
              <head>
                <title>Example Article</title>
                <meta name="description" content="Summary text" />
                <meta property="og:site_name" content="Example" />
              </head>
              <body>
                <main>
                  <p>This is one long paragraph with enough words to qualify as readable content.</p>
                  <p>Another paragraph keeps the extraction meaningful and focused.</p>
                </main>
              </body>
            </html>
            "#,
            "https://example.com/article",
            "example.com",
            "article",
            &ExternalFetchOptions {
                format: ExternalFetchFormat::Html,
                max_bytes: 10_000,
                profile: ExternalFetchProfile::Research,
            },
        );

        assert_eq!(result.content_format, "text");
        assert_eq!(result.fetch_mode, Some(FetchMode::Static));
        assert_eq!(result.site_name.as_deref(), Some("Example"));
        assert_eq!(
            result.canonical_url.as_deref(),
            Some("https://example.com/article")
        );
        assert_eq!(result.extract.as_deref(), Some("Summary text"));
        assert_eq!(result.extraction_quality, Some(ExtractionQuality::Low));
        assert!(!result.content_hash.is_empty());
        assert!(!collapse_inline_whitespace(&result.content).contains("<html>"));
    }
}
