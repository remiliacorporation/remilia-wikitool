use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use serde::Serialize;

pub const DEFAULT_WIKI_URL: &str = "https://wiki.remilia.org";
const DEFAULT_USER_AGENT: &str = "wikitool-rust/0.1 (+https://wiki.remilia.org)";
const DEFAULT_EXPORTS_DIR: &str = "wikitool_exports";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LighthouseOutputFormat {
    Html,
    Json,
}

impl LighthouseOutputFormat {
    pub fn parse(value: &str) -> Result<Self> {
        if value.eq_ignore_ascii_case("html") {
            return Ok(Self::Html);
        }
        if value.eq_ignore_ascii_case("json") {
            return Ok(Self::Json);
        }
        bail!("unsupported lighthouse output format: {value}")
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Json => "json",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LighthouseRunOptions {
    pub target: Option<String>,
    pub target_url_override: Option<String>,
    pub output_format: LighthouseOutputFormat,
    pub output_path_override: Option<PathBuf>,
    pub categories: Vec<String>,
    pub chrome_flags: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LighthouseVersionInfo {
    pub path: String,
    pub version: Option<String>,
    pub code: i32,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LighthouseRunResult {
    pub url: String,
    pub format: String,
    pub report_path: String,
    pub report_bytes: Option<u64>,
    pub categories: Vec<String>,
    pub code: i32,
    pub ignored_windows_cleanup_failure: bool,
}

#[derive(Debug, Clone)]
struct TagMatch {
    attrs: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct ProcessOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

pub fn seo_inspect(target: &str, override_url: Option<&str>) -> Result<SeoInspectResult> {
    let requested_url = resolve_target_url(target, override_url)?;
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
    options: &NetInspectOptions,
) -> Result<NetInspectResult> {
    let requested_url = resolve_target_url(target, override_url)?;
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

pub fn find_lighthouse_binary(_project_root: &Path) -> Option<PathBuf> {
    if let Some(path) = env::var("LIGHTHOUSE_PATH")
        .ok()
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    let names = if cfg!(windows) {
        vec!["lighthouse.cmd", "lighthouse.exe", "lighthouse"]
    } else {
        vec!["lighthouse"]
    };

    let path_var = env::var("PATH").ok()?;
    let separator = if cfg!(windows) { ';' } else { ':' };
    for part in path_var.split(separator) {
        let dir = PathBuf::from(strip_wrapping_quotes(part.trim()));
        if dir.as_os_str().is_empty() {
            continue;
        }
        for name in &names {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

pub fn lighthouse_version(binary: &Path) -> Result<LighthouseVersionInfo> {
    let output = run_process(binary, &["--version".to_string()], true)?;
    let version = output
        .stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string);
    Ok(LighthouseVersionInfo {
        path: normalize_path(binary),
        version,
        code: output.code,
        stderr: output.stderr,
    })
}

pub fn run_lighthouse(
    project_root: &Path,
    binary: &Path,
    options: &LighthouseRunOptions,
) -> Result<LighthouseRunResult> {
    let target = options
        .target
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("target is required unless --show-version is used"))?;
    let url = resolve_target_url(target, options.target_url_override.as_deref())?;
    let output_path = resolve_lighthouse_output_path(
        project_root,
        target,
        &url,
        options.output_format,
        options.output_path_override.as_deref(),
    )?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let args = build_lighthouse_args(
        &url,
        options.output_format,
        &output_path,
        &options.categories,
        options.chrome_flags.as_deref(),
        resolve_windows_user_data_dir(project_root).as_deref(),
    );
    let output = run_process(binary, &args, false)?;
    let ignored_windows_cleanup_failure =
        output.code != 0 && is_ignorable_cleanup_failure(&output.stderr, &output_path);
    if output.code != 0 && !ignored_windows_cleanup_failure {
        let stderr = output.stderr.trim();
        if stderr.is_empty() {
            bail!("lighthouse exited with code {}", output.code);
        }
        bail!("lighthouse exited with code {}: {}", output.code, stderr);
    }

    let report_bytes = file_size(&output_path);
    Ok(LighthouseRunResult {
        url,
        format: options.output_format.as_str().to_string(),
        report_path: normalize_path(&output_path),
        report_bytes,
        categories: options.categories.clone(),
        code: output.code,
        ignored_windows_cleanup_failure,
    })
}

pub fn resolve_target_url(target: &str, override_url: Option<&str>) -> Result<String> {
    if let Some(url) = override_url {
        return Ok(url.trim().to_string());
    }
    if is_http_url(target) {
        return Ok(target.to_string());
    }

    let base = resolve_wiki_url();
    let mut url = Url::parse(&format!("{}/wiki/", trim_trailing_slash(&base)))
        .with_context(|| format!("invalid wiki base URL: {base}"))?;
    let normalized_title = target.replace(' ', "_");
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("wiki base URL does not support path segments"))?
        .pop_if_empty()
        .push(&normalized_title);
    Ok(url.to_string())
}

pub fn resolve_wiki_url() -> String {
    if let Ok(value) = env::var("WIKI_URL") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trim_trailing_slash(trimmed).to_string();
        }
    }
    if let Ok(value) = env::var("WIKI_API_URL")
        && let Some(derived) = derive_wiki_url(&value)
    {
        return derived;
    }
    DEFAULT_WIKI_URL.to_string()
}

pub fn derive_wiki_url(api_url: &str) -> Option<String> {
    let mut parsed = Url::parse(api_url.trim()).ok()?;
    let path = parsed.path().to_ascii_lowercase();
    if path.ends_with("/api.php") {
        let next = parsed.path().to_string();
        parsed.set_path(&next[..next.len().saturating_sub(8)]);
    } else if path.ends_with("api.php") {
        let next = parsed.path().to_string();
        parsed.set_path(&next[..next.len().saturating_sub(7)]);
    }
    let value = trim_trailing_slash(parsed.as_str()).to_string();
    if value.is_empty() { None } else { Some(value) }
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
    env::var("WIKI_USER_AGENT").unwrap_or_else(|_| DEFAULT_USER_AGENT.to_string())
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

fn resolve_lighthouse_output_path(
    project_root: &Path,
    target: &str,
    url: &str,
    format: LighthouseOutputFormat,
    override_path: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(path) = override_path {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
        let cwd = env::current_dir().context("failed to resolve current working directory")?;
        return Ok(cwd.join(path));
    }

    let slug = sanitize_filename(&derive_label(target, url));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")?
        .as_secs();
    let filename = format!(
        "lighthouse-{}-{}.{}",
        if slug.is_empty() { "report" } else { &slug },
        stamp,
        format.extension()
    );
    if env::var("WIKITOOL_NO_DEFAULT_EXPORTS").is_ok() {
        let cwd = env::current_dir().context("failed to resolve current working directory")?;
        Ok(cwd.join(filename))
    } else {
        Ok(project_root.join(DEFAULT_EXPORTS_DIR).join(filename))
    }
}

fn build_lighthouse_args(
    url: &str,
    output_format: LighthouseOutputFormat,
    output_path: &Path,
    categories: &[String],
    chrome_flags: Option<&str>,
    windows_user_data_dir: Option<&Path>,
) -> Vec<String> {
    let mut args = vec![
        url.to_string(),
        "--output".to_string(),
        output_format.as_str().to_string(),
        "--output-path".to_string(),
        normalize_path(output_path),
        "--quiet".to_string(),
    ];
    if !categories.is_empty() {
        args.push(format!("--only-categories={}", categories.join(",")));
    }

    let mut flags = Vec::new();
    if let Some(value) = chrome_flags {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            flags.push(trimmed.to_string());
        }
    }
    let has_user_data_dir = chrome_flags
        .map(|value| value.to_ascii_lowercase().contains("--user-data-dir"))
        .unwrap_or(false);
    if cfg!(windows)
        && !has_user_data_dir
        && let Some(dir) = windows_user_data_dir
    {
        flags.push(format!("--user-data-dir={}", normalize_path(dir)));
        flags.push("--no-first-run".to_string());
        flags.push("--no-default-browser-check".to_string());
    }
    if !flags.is_empty() {
        args.push(format!("--chrome-flags={}", flags.join(" ")));
    }
    args
}

fn run_process(binary: &Path, args: &[String], suppress_output: bool) -> Result<ProcessOutput> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {}", binary.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !suppress_output {
        if !stdout.is_empty() {
            print!("{stdout}");
        }
        if !stderr.is_empty() {
            eprint!("{}", filter_chrome_launcher_output(&stderr));
        }
    }

    Ok(ProcessOutput {
        code: output.status.code().unwrap_or(1),
        stdout,
        stderr,
    })
}

fn filter_chrome_launcher_output(stderr: &str) -> String {
    if !cfg!(windows) || stderr.trim().is_empty() {
        return stderr.to_string();
    }
    let mut output = Vec::new();
    let mut suppressing = false;
    for line in stderr.lines() {
        if suppressing {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("at ") {
                continue;
            }
            suppressing = false;
        }
        if is_chrome_launcher_cleanup_line(line) {
            suppressing = true;
            continue;
        }
        output.push(line);
    }
    if output.is_empty() {
        String::new()
    } else {
        format!("{}\n", output.join("\n"))
    }
}

fn is_ignorable_cleanup_failure(stderr: &str, output_path: &Path) -> bool {
    if !cfg!(windows) || stderr.trim().is_empty() {
        return false;
    }
    let Some(size) = file_size(output_path) else {
        return false;
    };
    if size == 0 {
        return false;
    }
    let lower = stderr.to_ascii_lowercase();
    lower.contains("eperm") && lower.contains("chrome-launcher") && lower.contains("lighthouse")
}

fn resolve_windows_user_data_dir(project_root: &Path) -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    if let Ok(value) = env::var("WIKITOOL_LIGHTHOUSE_USER_DATA_DIR") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    let candidates = [
        env::var("PUBLIC").ok(),
        env::var("ProgramData").ok(),
        env::var("LOCALAPPDATA").ok(),
        env::var("TEMP").ok(),
        env::var("TMP").ok(),
        Some(project_root.to_string_lossy().to_string()),
    ];
    let mut base = candidates
        .iter()
        .flatten()
        .find(|value| !value.trim().is_empty() && !value.chars().any(char::is_whitespace))
        .cloned();
    if base.is_none() {
        base = candidates
            .iter()
            .flatten()
            .find(|value| !value.trim().is_empty())
            .cloned();
    }
    base.map(|value| PathBuf::from(value).join("wikitool-lighthouse"))
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

fn derive_label(target: &str, url: &str) -> String {
    if !is_http_url(target) {
        return target.to_string();
    }
    if let Ok(parsed) = Url::parse(url) {
        if let Some(segment) = parsed
            .path_segments()
            .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
            && !segment.is_empty()
        {
            return segment.to_string();
        }
        if !parsed.host_str().unwrap_or_default().is_empty() {
            return parsed.host_str().unwrap_or_default().to_string();
        }
    }
    target.to_string()
}

fn sanitize_filename(value: &str) -> String {
    let mut output = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        if ch.is_whitespace() || is_invalid_filename_char(ch) {
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

fn is_invalid_filename_char(ch: char) -> bool {
    matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\')
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

fn file_size(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|meta| meta.len())
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn strip_wrapping_quotes(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn is_chrome_launcher_cleanup_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if lower.contains("chrome-launcher")
        && (lower.contains("eperm")
            || lower.contains("cleanup")
            || (lower.contains("failed") && lower.contains("remove"))
            || (lower.contains("failed") && lower.contains("delete")))
    {
        return true;
    }
    (lower.contains("runtime error encountered")
        && lower.contains("eperm")
        && lower.contains("lighthouse"))
        || (lower.contains("permission denied") && lower.contains("lighthouse"))
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_WIKI_URL, derive_wiki_url, extract_head, resolve_target_url, sanitize_filename,
        scan_tags,
    };

    #[test]
    fn derive_wiki_url_strips_api_php() {
        assert_eq!(
            derive_wiki_url("https://wiki.example.org/api.php"),
            Some("https://wiki.example.org".to_string())
        );
        assert_eq!(
            derive_wiki_url("https://wiki.example.org/w/api.php"),
            Some("https://wiki.example.org/w".to_string())
        );
    }

    #[test]
    fn resolve_target_url_builds_page_url() {
        let url = resolve_target_url("Alpha Beta", Some("https://wiki.remilia.org/wiki/Fallback"))
            .expect("url");
        assert_eq!(url, "https://wiki.remilia.org/wiki/Fallback");
        assert!(DEFAULT_WIKI_URL.starts_with("https://"));
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

    #[test]
    fn sanitize_filename_normalizes_problem_chars() {
        assert_eq!(sanitize_filename("Alpha/Beta: test"), "Alpha-Beta-test");
        assert_eq!(sanitize_filename("  A   B  "), "A-B");
    }
}
