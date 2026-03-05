use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::Value;

use crate::support::{env_value, env_value_u64, env_value_usize, unix_timestamp};

const DEFAULT_USER_AGENT: &str = crate::config::DEFAULT_USER_AGENT;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_MAX_BYTES: usize = 1_000_000;
const DEFAULT_RETRIES: usize = 2;
const DEFAULT_RETRY_DELAY_MS: u64 = 350;
const DEFAULT_MEDIAWIKI_TITLE_BATCH_SIZE: usize = 50;

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
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

pub fn parse_wiki_url(url: &str) -> Option<ParsedWikiUrl> {
    let parsed = Url::parse(url).ok()?;
    let domain = parsed.host_str()?.to_string();
    let scheme = parsed.scheme().to_string();
    let path = parsed.path();

    let mut title = None::<String>;
    let mut base_url = format!("{scheme}://{domain}/wiki/");
    let mut api_candidates = api_candidates_for_domain(&scheme, &domain);

    if let Some(rest) = path.strip_prefix("/wiki/") {
        if !rest.trim().is_empty() {
            title = Some(decode_title(rest));
        }
    } else if path.ends_with("/w/index.php") || path.ends_with("/index.php") {
        for (key, value) in parsed.query_pairs() {
            if key.eq_ignore_ascii_case("title") {
                let value = value.trim().to_string();
                if !value.is_empty() {
                    title = Some(decode_title(&value));
                }
                break;
            }
        }
        if path.ends_with("/index.php") {
            api_candidates = vec![format!("{scheme}://{domain}/api.php")];
        }
    } else {
        let segments = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if parsed.query().is_none() && segments.len() == 1 {
            title = Some(decode_title(segments[0]));
            base_url = format!("{scheme}://{domain}/");
            api_candidates = vec![
                format!("{scheme}://{domain}/api.php"),
                format!("{scheme}://{domain}/w/api.php"),
            ];
        }
    }

    let title = title?;
    Some(ParsedWikiUrl {
        domain,
        title,
        api_candidates: dedupe(api_candidates),
        base_url,
    })
}

pub fn fetch_page_by_url(
    url: &str,
    options: &ExternalFetchOptions,
) -> Result<Option<ExternalFetchResult>> {
    if let Some(parsed) = parse_wiki_url(url) {
        let mut client = external_client()?;
        match fetch_mediawiki_page_with_client(&mut client, &parsed.title, &parsed, options)? {
            MediaWikiFetchOutcome::Found(result) => return Ok(Some(result)),
            MediaWikiFetchOutcome::Missing => return Ok(None),
            MediaWikiFetchOutcome::NotExportable => {}
        }
    }

    fetch_web_url(url, options.max_bytes).map(Some)
}

pub fn fetch_mediawiki_page(
    title: &str,
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<Option<ExternalFetchResult>> {
    let mut client = external_client()?;
    match fetch_mediawiki_page_with_client(&mut client, title, parsed, options)? {
        MediaWikiFetchOutcome::Found(result) => Ok(Some(result)),
        MediaWikiFetchOutcome::Missing | MediaWikiFetchOutcome::NotExportable => Ok(None),
    }
}

fn fetch_mediawiki_page_with_client(
    client: &mut ExternalClient,
    title: &str,
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<MediaWikiFetchOutcome> {
    let mut candidate_errors = Vec::new();
    let mut saw_not_exportable = false;
    for api_url in &parsed.api_candidates {
        let response = mediawiki_query_content(client, api_url, title, options);
        match response {
            Ok(MediaWikiFetchOutcome::Found(result)) => {
                return Ok(MediaWikiFetchOutcome::Found(ExternalFetchResult {
                    source_wiki: "mediawiki".to_string(),
                    source_domain: parsed.domain.clone(),
                    url: format!("{}{}", parsed.base_url, encode_title(&result.title)),
                    ..result
                }));
            }
            Ok(MediaWikiFetchOutcome::Missing) => return Ok(MediaWikiFetchOutcome::Missing),
            Ok(MediaWikiFetchOutcome::NotExportable) => saw_not_exportable = true,
            Err(error) => candidate_errors.push(format!("{api_url}: {error:#}")),
        }
    }
    if saw_not_exportable {
        return Ok(MediaWikiFetchOutcome::NotExportable);
    }
    if !candidate_errors.is_empty() {
        bail!(
            "all MediaWiki API candidates failed for `{title}` on {}:\n  - {}",
            parsed.domain,
            candidate_errors.join("\n  - ")
        );
    }
    Ok(MediaWikiFetchOutcome::NotExportable)
}

pub fn list_subpages(
    parent_title: &str,
    parsed: &ParsedWikiUrl,
    limit: usize,
) -> Result<Vec<String>> {
    let mut client = external_client()?;
    let prefix = format!("{}/", parent_title.trim_end_matches('/'));
    let mut candidate_errors = Vec::new();
    for api_url in &parsed.api_candidates {
        let response = mediawiki_query_allpages(&mut client, api_url, &prefix, limit.max(1));
        match response {
            Ok(value) => return Ok(value),
            Err(error) => candidate_errors.push(format!("{api_url}: {error:#}")),
        }
    }
    if !candidate_errors.is_empty() {
        bail!(
            "all MediaWiki API candidates failed while listing subpages for `{parent_title}` on {}:\n  - {}",
            parsed.domain,
            candidate_errors.join("\n  - ")
        );
    }
    Ok(Vec::new())
}

pub fn fetch_pages_by_titles(
    titles: &[String],
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<Vec<ExternalFetchResult>> {
    let mut client = external_client()?;
    let mut output = Vec::new();
    let mut failures = Vec::new();
    for batch in titles.chunks(DEFAULT_MEDIAWIKI_TITLE_BATCH_SIZE) {
        match fetch_mediawiki_pages_with_client(&mut client, batch, parsed, options) {
            Ok(batch_results) => {
                for (title, outcome) in batch.iter().zip(batch_results) {
                    match outcome {
                        MediaWikiFetchOutcome::Found(page) => output.push(page),
                        MediaWikiFetchOutcome::Missing | MediaWikiFetchOutcome::NotExportable => {}
                    }
                    let _ = title;
                }
            }
            Err(error) => {
                for title in batch {
                    failures.push(format!("{title}: {error:#}"));
                }
            }
        }
    }
    if output.is_empty() && !failures.is_empty() {
        bail!(
            "failed to fetch {} page(s) from {}:\n  - {}",
            failures.len(),
            parsed.domain,
            failures.join("\n  - ")
        );
    }
    Ok(output)
}

fn fetch_mediawiki_pages_with_client(
    client: &mut ExternalClient,
    titles: &[String],
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<Vec<MediaWikiFetchOutcome>> {
    let mut candidate_errors = Vec::new();
    for api_url in &parsed.api_candidates {
        let response = mediawiki_query_content_batch(client, api_url, titles, options);
        match response {
            Ok(outcomes) => {
                let enriched = outcomes
                    .into_iter()
                    .map(|outcome| match outcome {
                        MediaWikiFetchOutcome::Found(result) => {
                            MediaWikiFetchOutcome::Found(ExternalFetchResult {
                                source_wiki: "mediawiki".to_string(),
                                source_domain: parsed.domain.clone(),
                                url: format!("{}{}", parsed.base_url, encode_title(&result.title)),
                                ..result
                            })
                        }
                        MediaWikiFetchOutcome::Missing => MediaWikiFetchOutcome::Missing,
                        MediaWikiFetchOutcome::NotExportable => {
                            MediaWikiFetchOutcome::NotExportable
                        }
                    })
                    .collect();
                return Ok(enriched);
            }
            Err(error) => candidate_errors.push(format!("{api_url}: {error:#}")),
        }
    }
    if !candidate_errors.is_empty() {
        bail!(
            "all MediaWiki API candidates failed for {} page(s) on {}:\n  - {}",
            titles.len(),
            parsed.domain,
            candidate_errors.join("\n  - ")
        );
    }
    Ok(vec![MediaWikiFetchOutcome::NotExportable; titles.len()])
}

pub fn wikitext_to_markdown(content: &str, _code_language: Option<&str>) -> String {
    let mut lines = Vec::new();
    for line in content.lines() {
        if let Some(converted) = convert_heading(line) {
            lines.push(converted);
        } else {
            lines.push(convert_internal_links(line));
        }
    }
    lines.join("\n")
}

pub fn generate_frontmatter(
    title: &str,
    source_url: &str,
    domain: &str,
    timestamp: &str,
    extra: &[(String, String)],
) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("title: \"{}\"", title.replace('"', "\\\"")),
        format!("source: {source_url}"),
        format!("wiki: {domain}"),
        format!("fetched: {timestamp}"),
    ];
    for (key, value) in extra {
        lines.push(format!("{key}: {value}"));
    }
    lines.push("---".to_string());
    lines.push(String::new());
    lines.join("\n")
}

pub fn sanitize_filename(value: &str) -> String {
    let mut output = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        if ch.is_whitespace() || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\')
        {
            if !previous_dash && !output.is_empty() {
                output.push('-');
                previous_dash = true;
            }
            continue;
        }
        output.push(ch);
        previous_dash = false;
    }
    while output.ends_with('-') {
        output.pop();
    }
    output
}

pub fn default_export_path(
    project_root: &Path,
    title: &str,
    is_directory: bool,
    format: ExportFormat,
) -> Option<PathBuf> {
    if env::var("WIKITOOL_NO_DEFAULT_EXPORTS").is_ok() {
        return None;
    }
    let filename = sanitize_filename(title);
    let exports_dir = project_root.join(DEFAULT_EXPORTS_DIR);
    if is_directory {
        return Some(exports_dir.join(filename));
    }
    Some(exports_dir.join(format!("{}.{}", filename, format.file_extension())))
}

fn mediawiki_query_content(
    client: &mut ExternalClient,
    api_url: &str,
    title: &str,
    options: &ExternalFetchOptions,
) -> Result<MediaWikiFetchOutcome> {
    let mut params = vec![
        ("action", "query".to_string()),
        ("titles", title.to_string()),
        ("prop", "revisions|extracts".to_string()),
        ("rvprop", "content|timestamp".to_string()),
        ("rvslots", "main".to_string()),
        ("exintro", "1".to_string()),
        ("explaintext", "1".to_string()),
    ];
    if options.format == ExternalFetchFormat::Html {
        params.push(("rvparse", "1".to_string()));
    }

    let payload = client.request_json(api_url, &params)?;
    parse_mediawiki_content_payload(&payload, title, options)
}

fn mediawiki_query_content_batch(
    client: &mut ExternalClient,
    api_url: &str,
    titles: &[String],
    options: &ExternalFetchOptions,
) -> Result<Vec<MediaWikiFetchOutcome>> {
    let mut params = vec![
        ("action", "query".to_string()),
        ("titles", titles.join("|")),
        ("prop", "revisions|extracts".to_string()),
        ("rvprop", "content|timestamp".to_string()),
        ("rvslots", "main".to_string()),
        ("exintro", "1".to_string()),
        ("explaintext", "1".to_string()),
    ];
    if options.format == ExternalFetchFormat::Html {
        params.push(("rvparse", "1".to_string()));
    }

    let payload = client.request_json(api_url, &params)?;
    parse_mediawiki_batch_content_payload(&payload, titles, options)
}

fn parse_mediawiki_content_payload(
    payload: &Value,
    requested_title: &str,
    options: &ExternalFetchOptions,
) -> Result<MediaWikiFetchOutcome> {
    let page = payload
        .get("query")
        .and_then(|value| value.get("pages"))
        .and_then(Value::as_array)
        .and_then(|pages| pages.first())
        .ok_or_else(|| anyhow::anyhow!("invalid MediaWiki response shape"))?;

    parse_mediawiki_content_page(page, requested_title, options)
}

fn parse_mediawiki_batch_content_payload(
    payload: &Value,
    requested_titles: &[String],
    options: &ExternalFetchOptions,
) -> Result<Vec<MediaWikiFetchOutcome>> {
    let pages = payload
        .get("query")
        .and_then(|value| value.get("pages"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("invalid MediaWiki response shape"))?;

    let mut page_outcomes = std::collections::HashMap::new();
    for page in pages {
        let title = page
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if title.trim().is_empty() {
            continue;
        }
        let outcome = parse_mediawiki_content_page(page, &title, options)?;
        page_outcomes.insert(title.clone(), outcome.clone());
        page_outcomes.insert(title.replace(' ', "_"), outcome);
    }

    Ok(requested_titles
        .iter()
        .map(|title| {
            page_outcomes
                .get(title)
                .or_else(|| {
                    let normalized = title.replace('_', " ");
                    page_outcomes.get(&normalized)
                })
                .cloned()
                .unwrap_or(MediaWikiFetchOutcome::Missing)
        })
        .collect())
}

fn parse_mediawiki_content_page(
    page: &Value,
    requested_title: &str,
    options: &ExternalFetchOptions,
) -> Result<MediaWikiFetchOutcome> {
    if page.get("missing").is_some() {
        return Ok(MediaWikiFetchOutcome::Missing);
    }

    let title = page
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(requested_title)
        .to_string();
    let extract = page
        .get("extract")
        .and_then(Value::as_str)
        .map(|value| truncate_to_byte_limit(value, options.max_bytes));

    let revision = match page
        .get("revisions")
        .and_then(Value::as_array)
        .and_then(|revisions| revisions.first())
    {
        Some(revision) => revision,
        None => return Ok(MediaWikiFetchOutcome::NotExportable),
    };
    let content = match revision
        .get("slots")
        .and_then(|value| value.get("main"))
        .and_then(|value| value.get("content"))
        .and_then(Value::as_str)
    {
        Some(content) => truncate_to_byte_limit(content, options.max_bytes),
        None => return Ok(MediaWikiFetchOutcome::NotExportable),
    };
    let timestamp = revision
        .get("timestamp")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(now_timestamp_string);

    Ok(MediaWikiFetchOutcome::Found(ExternalFetchResult {
        title,
        content,
        timestamp,
        extract,
        url: String::new(),
        source_wiki: String::new(),
        source_domain: String::new(),
        content_format: match options.format {
            ExternalFetchFormat::Wikitext => "wikitext".to_string(),
            ExternalFetchFormat::Html => "html".to_string(),
        },
    }))
}

fn mediawiki_query_allpages(
    client: &mut ExternalClient,
    api_url: &str,
    prefix: &str,
    limit: usize,
) -> Result<Vec<String>> {
    let target = limit.max(1);
    let mut titles = Vec::new();
    let mut continuation = None::<String>;

    while titles.len() < target {
        let mut params = vec![
            ("action", "query".to_string()),
            ("list", "allpages".to_string()),
            ("apprefix", prefix.to_string()),
            (
                "aplimit",
                target.saturating_sub(titles.len()).min(500).to_string(),
            ),
        ];
        if let Some(token) = &continuation {
            params.push(("apcontinue", token.clone()));
        }

        let payload = client.request_json(api_url, &params)?;
        let (batch_titles, next_continuation) = parse_allpages_payload(&payload);
        titles.extend(batch_titles);

        let Some(token) = next_continuation else {
            break;
        };
        if token.trim().is_empty() {
            break;
        }
        continuation = Some(token);
    }

    if titles.len() > target {
        titles.truncate(target);
    }
    Ok(titles)
}

fn parse_allpages_payload(payload: &Value) -> (Vec<String>, Option<String>) {
    let mut titles = Vec::new();
    if let Some(allpages) = payload
        .get("query")
        .and_then(|value| value.get("allpages"))
        .and_then(Value::as_array)
    {
        for page in allpages {
            if let Some(title) = page.get("title").and_then(Value::as_str)
                && !title.trim().is_empty()
            {
                titles.push(title.to_string());
            }
        }
    }

    let continuation = payload
        .get("continue")
        .and_then(|value| value.get("apcontinue"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    (titles, continuation)
}

fn fetch_web_url(url: &str, max_bytes: usize) -> Result<ExternalFetchResult> {
    let client = external_client()?;
    let response = client
        .client
        .get(url)
        .header("User-Agent", client.user_agent.clone())
        .header("Accept", "text/html, text/plain;q=0.9,*/*;q=0.1")
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
    let text = response.text().context("failed to read response body")?;
    let content = truncate_to_byte_limit(&text, max_bytes);

    let parsed_url = Url::parse(&final_url).ok();
    let title = parsed_url
        .as_ref()
        .and_then(|value| value.path_segments())
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.trim().is_empty())
        .map(decode_title)
        .unwrap_or_else(|| final_url.clone());
    let source_domain = parsed_url
        .as_ref()
        .and_then(|value| value.host_str())
        .unwrap_or("web")
        .to_string();

    Ok(ExternalFetchResult {
        title,
        content,
        timestamp: now_timestamp_string(),
        extract: None,
        url: final_url,
        source_wiki: "web".to_string(),
        source_domain,
        content_format: if content_type.contains("text/html") {
            "html".to_string()
        } else if content_type.contains("text/markdown") {
            "markdown".to_string()
        } else {
            "text".to_string()
        },
    })
}

fn convert_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('=') || !trimmed.ends_with('=') || trimmed.len() < 4 {
        return None;
    }
    let start_equals = trimmed.chars().take_while(|ch| *ch == '=').count();
    let end_equals = trimmed.chars().rev().take_while(|ch| *ch == '=').count();
    if start_equals < 2 || start_equals != end_equals {
        return None;
    }
    let level = start_equals.min(6);
    let content = trimmed[start_equals..trimmed.len() - end_equals].trim();
    if content.is_empty() {
        return None;
    }
    Some(format!("{} {}", "#".repeat(level), content))
}

fn convert_internal_links(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if index + 1 < chars.len() && chars[index] == '[' && chars[index + 1] == '[' {
            let mut cursor = index + 2;
            let mut found = None::<usize>;
            while cursor + 1 < chars.len() {
                if chars[cursor] == ']' && chars[cursor + 1] == ']' {
                    found = Some(cursor);
                    break;
                }
                cursor += 1;
            }
            if let Some(end) = found {
                let inner = chars[index + 2..end].iter().collect::<String>();
                let mut parts = inner.splitn(2, '|');
                let target = parts.next().unwrap_or("").trim();
                let label = parts.next().map(str::trim).unwrap_or(target);
                if !target.is_empty() && !label.is_empty() {
                    output.push_str(&format!("[{label}](wiki://{target})"));
                    index = end + 2;
                    continue;
                }
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn decode_title(raw: &str) -> String {
    raw.replace('_', " ").trim().to_string()
}

fn encode_title(title: &str) -> String {
    title.trim().replace(' ', "_")
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        if seen.insert(value.clone()) {
            output.push(value);
        }
    }
    output
}

fn api_candidates_for_domain(scheme: &str, domain: &str) -> Vec<String> {
    if domain.ends_with("fandom.com") {
        return vec![
            format!("{scheme}://{domain}/api.php"),
            format!("{scheme}://{domain}/w/api.php"),
        ];
    }
    vec![
        format!("{scheme}://{domain}/w/api.php"),
        format!("{scheme}://{domain}/api.php"),
    ]
}

fn now_timestamp_string() -> String {
    unix_timestamp()
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "0".to_string())
}

struct ExternalClient {
    client: Client,
    user_agent: String,
    retries: usize,
    retry_delay_ms: u64,
    last_request_at: Option<Instant>,
}

#[derive(Clone)]
enum MediaWikiFetchOutcome {
    Found(ExternalFetchResult),
    Missing,
    NotExportable,
}

impl ExternalClient {
    fn request_json(&mut self, api_url: &str, params: &[(&str, String)]) -> Result<Value> {
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

fn external_client() -> Result<ExternalClient> {
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

fn truncate_to_byte_limit(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        ExportFormat, ExternalFetchFormat, ExternalFetchOptions, MediaWikiFetchOutcome,
        convert_heading, convert_internal_links, default_export_path, parse_allpages_payload,
        parse_mediawiki_batch_content_payload, parse_mediawiki_content_payload, parse_wiki_url,
        sanitize_filename, truncate_to_byte_limit, wikitext_to_markdown,
    };
    use serde_json::json;

    #[test]
    fn parse_wiki_url_supports_wiki_and_index_forms() {
        let parsed = parse_wiki_url("https://www.mediawiki.org/wiki/Manual:Hooks").expect("parse");
        assert_eq!(parsed.domain, "www.mediawiki.org");
        assert_eq!(parsed.title, "Manual:Hooks");

        let parsed = parse_wiki_url("https://wowdev.wiki/index.php?title=M2").expect("parse");
        assert_eq!(parsed.title, "M2");
    }

    #[test]
    fn heading_and_link_conversion_are_deterministic() {
        assert_eq!(
            convert_heading("== Heading =="),
            Some("## Heading".to_string())
        );
        assert_eq!(
            convert_internal_links("See [[Alpha|A]] and [[Beta]]"),
            "See [A](wiki://Alpha) and [Beta](wiki://Beta)"
        );
    }

    #[test]
    fn markdown_conversion_preserves_lines() {
        let markdown = wikitext_to_markdown("== Heading ==\nText [[Alpha]]", None);
        assert!(markdown.contains("## Heading"));
        assert!(markdown.contains("[Alpha](wiki://Alpha)"));
    }

    #[test]
    fn sanitize_filename_strips_invalid_characters() {
        assert_eq!(sanitize_filename("A/B:C"), "A-B-C");
        assert_eq!(sanitize_filename("  A   B  "), "A-B");
    }

    #[test]
    fn default_export_path_respects_project_root() {
        let root = std::path::Path::new("/tmp/wiki");
        let file = default_export_path(root, "Alpha", false, ExportFormat::Markdown).expect("path");
        assert!(file.ends_with("wikitool_exports/Alpha.md"));
    }

    #[test]
    fn truncate_to_byte_limit_preserves_char_boundaries() {
        assert_eq!(truncate_to_byte_limit("abcd", 3), "abc");
        assert_eq!(truncate_to_byte_limit("éclair", 1), "");
        assert_eq!(truncate_to_byte_limit("éclair", 2), "é");
    }

    #[test]
    fn mediawiki_payload_distinguishes_missing_from_not_exportable() {
        let options = ExternalFetchOptions {
            format: ExternalFetchFormat::Wikitext,
            max_bytes: 8,
        };
        let missing = json!({
            "query": {
                "pages": [{
                    "title": "Missing",
                    "missing": true
                }]
            }
        });
        assert!(matches!(
            parse_mediawiki_content_payload(&missing, "Missing", &options).expect("missing"),
            MediaWikiFetchOutcome::Missing
        ));

        let not_exportable = json!({
            "query": {
                "pages": [{
                    "title": "Special:Page"
                }]
            }
        });
        assert!(matches!(
            parse_mediawiki_content_payload(&not_exportable, "Special:Page", &options)
                .expect("not exportable"),
            MediaWikiFetchOutcome::NotExportable
        ));
    }

    #[test]
    fn mediawiki_payload_applies_byte_limit_to_content_and_extract() {
        let options = ExternalFetchOptions {
            format: ExternalFetchFormat::Wikitext,
            max_bytes: 5,
        };
        let payload = json!({
            "query": {
                "pages": [{
                    "title": "Alpha",
                    "extract": "123456789",
                    "revisions": [{
                        "timestamp": "2026-01-01T00:00:00Z",
                        "slots": {
                            "main": {
                                "content": "abcdefghi"
                            }
                        }
                    }]
                }]
            }
        });

        let MediaWikiFetchOutcome::Found(result) =
            parse_mediawiki_content_payload(&payload, "Alpha", &options).expect("payload")
        else {
            panic!("expected found result");
        };
        assert_eq!(result.content, "abcde");
        assert_eq!(result.extract.as_deref(), Some("12345"));
    }

    #[test]
    fn mediawiki_batch_payload_preserves_requested_order() {
        let options = ExternalFetchOptions {
            format: ExternalFetchFormat::Wikitext,
            max_bytes: 4,
        };
        let payload = json!({
            "query": {
                "pages": [
                    {
                        "title": "Alpha",
                        "extract": "alpha extract",
                        "revisions": [{
                            "timestamp": "2026-01-01T00:00:00Z",
                            "slots": {
                                "main": {
                                    "content": "alphabet"
                                }
                            }
                        }]
                    },
                    {
                        "title": "Missing",
                        "missing": true
                    },
                    {
                        "title": "Special:Page"
                    },
                    {
                        "title": "Beta",
                        "extract": "beta extract",
                        "revisions": [{
                            "timestamp": "2026-01-02T00:00:00Z",
                            "slots": {
                                "main": {
                                    "content": "betatron"
                                }
                            }
                        }]
                    }
                ]
            }
        });
        let titles = vec![
            "Beta".to_string(),
            "Alpha".to_string(),
            "Missing".to_string(),
            "Special:Page".to_string(),
        ];

        let results =
            parse_mediawiki_batch_content_payload(&payload, &titles, &options).expect("payload");

        assert!(matches!(
            &results[0],
            MediaWikiFetchOutcome::Found(result)
                if result.title == "Beta"
                    && result.content == "beta"
                    && result.extract.as_deref() == Some("beta")
        ));
        assert!(matches!(
            &results[1],
            MediaWikiFetchOutcome::Found(result)
                if result.title == "Alpha"
                    && result.content == "alph"
                    && result.extract.as_deref() == Some("alph")
        ));
        assert!(matches!(&results[2], MediaWikiFetchOutcome::Missing));
        assert!(matches!(&results[3], MediaWikiFetchOutcome::NotExportable));
    }

    #[test]
    fn mediawiki_batch_payload_matches_requested_underscored_titles() {
        let options = ExternalFetchOptions {
            format: ExternalFetchFormat::Wikitext,
            max_bytes: 32,
        };
        let payload = json!({
            "query": {
                "pages": [{
                    "title": "Foo Bar",
                    "revisions": [{
                        "timestamp": "2026-01-01T00:00:00Z",
                        "slots": {
                            "main": {
                                "content": "payload"
                            }
                        }
                    }]
                }]
            }
        });
        let titles = vec!["Foo_Bar".to_string()];

        let results =
            parse_mediawiki_batch_content_payload(&payload, &titles, &options).expect("payload");

        assert!(matches!(
            &results[0],
            MediaWikiFetchOutcome::Found(result)
                if result.title == "Foo Bar" && result.content == "payload"
        ));
    }

    #[test]
    fn allpages_payload_extracts_titles_and_continuation() {
        let payload = json!({
            "continue": {
                "continue": "-||",
                "apcontinue": "Parent/Subpage 3"
            },
            "query": {
                "allpages": [
                    { "title": "Parent/Subpage 1" },
                    { "title": "Parent/Subpage 2" },
                    { "title": "" }
                ]
            }
        });

        let (titles, continuation) = parse_allpages_payload(&payload);
        assert_eq!(
            titles,
            vec![
                "Parent/Subpage 1".to_string(),
                "Parent/Subpage 2".to_string()
            ]
        );
        assert_eq!(continuation.as_deref(), Some("Parent/Subpage 3"));
    }
}
