use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use serde_json::Value;

use crate::support::{compute_hash, env_value, env_value_u64, env_value_usize, now_iso8601_utc};

use super::entities::decode_html_entities;
use super::model::{
    ExternalAccessRoute, ExternalContentSignal, ExternalFetchAttempt, ExternalFetchFailure,
    ExternalFetchFailureError, ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult,
    ExternalMachineSurface, ExternalMachineSurfaceReport, ExtractionQuality, FetchMode,
};
use super::url::decode_title;

const DEFAULT_USER_AGENT: &str = crate::config::DEFAULT_USER_AGENT;
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_RETRIES: usize = 2;
const DEFAULT_RETRY_DELAY_MS: u64 = 350;
const MAX_CLIENT_REDIRECTS: usize = 1;
const DIRECT_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,text/plain;q=0.8,text/markdown;q=0.8,application/json;q=0.7,*/*;q=0.1";

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

struct TextHttpResponse {
    final_url: String,
    http_status: u16,
    content_type: String,
    cf_mitigated: Option<String>,
    crawler_price: Option<String>,
    body: String,
}

#[derive(Debug, Clone, Copy)]
pub struct MachineSurfaceDiscoveryOptions {
    pub max_bytes: usize,
    pub surface_limit: usize,
    pub probe_source_page: bool,
    /// Hint that the caller already knows the source page is blocked (for example,
    /// discovery is being run from a fetch-error handler that saw the 403 upstream).
    /// When true, access-route synthesis emits the blocked-source fallbacks even if
    /// the discovery itself did not probe the source page.
    pub source_known_blocked: bool,
}

impl Default for MachineSurfaceDiscoveryOptions {
    fn default() -> Self {
        Self {
            max_bytes: 1_000_000,
            surface_limit: 20,
            probe_source_page: true,
            source_known_blocked: false,
        }
    }
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
    let mut current_url = url.to_string();
    let mut client_redirects = 0usize;
    let mut attempts = Vec::new();

    loop {
        let response =
            request_text_candidate(&client, "direct_static", &current_url, options.max_bytes);
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                attempts.push(ExternalFetchAttempt {
                    mode: "direct_static".to_string(),
                    url: current_url.clone(),
                    outcome: "request_error".to_string(),
                    http_status: None,
                    content_type: None,
                    message: Some(error.to_string()),
                });
                return Err(fetch_failure(url, attempts).into());
            }
        };
        if response.http_status >= 400 {
            let outcome = if response.http_status == 402 || response.crawler_price.is_some() {
                "payment_required"
            } else if detect_access_challenge_response(&response) {
                "access_challenge"
            } else {
                "http_error"
            };
            attempts.push(ExternalFetchAttempt {
                mode: "direct_static".to_string(),
                url: current_url.clone(),
                outcome: outcome.to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                message: Some(if outcome == "access_challenge" {
                    access_challenge_message(&response)
                } else if outcome == "payment_required" {
                    payment_required_message(&response)
                } else {
                    format!(
                        "HTTP {} while fetching {}",
                        response.http_status, current_url
                    )
                }),
            });
            return Err(fetch_failure(url, attempts).into());
        }
        if !is_supported_text_content_type(&response.content_type) {
            attempts.push(ExternalFetchAttempt {
                mode: "direct_static".to_string(),
                url: current_url.clone(),
                outcome: "unsupported_content_type".to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                message: Some(format!(
                    "unsupported content-type: {}",
                    response.content_type
                )),
            });
            return Err(fetch_failure(url, attempts).into());
        }
        if options.profile == ExternalFetchProfile::Research
            && detect_access_challenge_response(&response)
        {
            attempts.push(ExternalFetchAttempt {
                mode: "direct_static".to_string(),
                url: current_url.clone(),
                outcome: "access_challenge".to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                message: Some(access_challenge_message(&response)),
            });
            return Err(fetch_failure(url, attempts).into());
        }
        attempts.push(ExternalFetchAttempt {
            mode: "direct_static".to_string(),
            url: current_url.clone(),
            outcome: "success".to_string(),
            http_status: Some(response.http_status),
            content_type: Some(response.content_type.clone()),
            message: None,
        });

        if response.content_type.contains("text/html")
            && options.profile == ExternalFetchProfile::Research
            && client_redirects < MAX_CLIENT_REDIRECTS
            && let Some(redirect_url) =
                extract_client_redirect_url(&response.body, &response.final_url)
            && redirect_url != response.final_url
        {
            current_url = redirect_url;
            client_redirects += 1;
            continue;
        }

        let parsed_url = Url::parse(&response.final_url).ok();
        let fallback_title = derive_title_from_url(parsed_url.as_ref(), &response.final_url);
        let source_domain = parsed_url
            .as_ref()
            .and_then(|value| value.host_str())
            .unwrap_or("web")
            .to_string();

        let mut result = if response.content_type.contains("text/html") {
            build_html_fetch_result(
                &response.body,
                &response.final_url,
                &source_domain,
                &fallback_title,
                options,
            )
        } else {
            build_text_fetch_result(
                &response.body,
                &response.final_url,
                &source_domain,
                &fallback_title,
                content_format_for_content_type(&response.content_type),
                options,
            )
        };
        result.fetch_attempts = attempts;
        return Ok(result);
    }
}

fn request_text_candidate(
    client: &ExternalClient,
    mode: &str,
    url: &str,
    max_bytes: usize,
) -> Result<TextHttpResponse> {
    let mut request = client
        .client
        .get(url)
        .header("User-Agent", client.user_agent.clone())
        .header("Accept", DIRECT_ACCEPT)
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Cache-Control", "no-cache")
        .header("Pragma", "no-cache");
    if mode == "direct_static" {
        request = request
            .header("Upgrade-Insecure-Requests", "1")
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Site", "none")
            .header("Sec-Fetch-User", "?1");
    }
    let response = request
        .send()
        .with_context(|| format!("failed to fetch {url}"))?;
    let final_url = response.url().to_string();
    let http_status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let cf_mitigated = response
        .headers()
        .get("cf-mitigated")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let crawler_price = response
        .headers()
        .get("crawler-price")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let body = if is_supported_text_content_type(&content_type) {
        read_text_body_limited(response, max_bytes)?
    } else {
        String::new()
    };
    Ok(TextHttpResponse {
        final_url,
        http_status,
        content_type,
        cf_mitigated,
        crawler_price,
        body,
    })
}

fn detect_access_challenge_response(response: &TextHttpResponse) -> bool {
    response
        .cf_mitigated
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("challenge"))
        || (response.content_type.contains("text/html")
            && (detect_access_challenge(&response.body)
                || response
                    .body
                    .to_ascii_lowercase()
                    .contains("just a moment...")))
}

fn access_challenge_message(response: &TextHttpResponse) -> String {
    if response
        .cf_mitigated
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("challenge"))
    {
        return "access challenge detected from cf-mitigated: challenge header".to_string();
    }
    "access challenge detected in response body".to_string()
}

fn payment_required_message(response: &TextHttpResponse) -> String {
    if let Some(price) = response.crawler_price.as_deref() {
        return format!("payment required; crawler-price: {price}");
    }
    "payment required".to_string()
}

fn is_supported_text_content_type(content_type: &str) -> bool {
    content_type.contains("text/html")
        || content_type.contains("text/plain")
        || content_type.contains("text/markdown")
        || content_type.contains("application/json")
        || content_type.contains("application/xml")
        || content_type.contains("text/xml")
        || content_type.contains("application/rss+xml")
        || content_type.contains("application/atom+xml")
        || content_type.is_empty()
}

fn content_format_for_content_type(content_type: &str) -> &'static str {
    if content_type.contains("text/markdown") {
        "markdown"
    } else if content_type.contains("application/json") {
        "json"
    } else {
        "text"
    }
}

fn fetch_failure(
    source_url: &str,
    attempts: Vec<ExternalFetchAttempt>,
) -> ExternalFetchFailureError {
    let kind = if attempts
        .iter()
        .any(|attempt| attempt.outcome == "access_challenge")
    {
        "access_challenge"
    } else if attempts
        .iter()
        .any(|attempt| attempt.outcome == "payment_required")
    {
        "payment_required"
    } else if attempts
        .iter()
        .any(|attempt| attempt.outcome == "http_error")
    {
        "http_error"
    } else if attempts
        .iter()
        .any(|attempt| attempt.outcome == "unsupported_content_type")
    {
        "unsupported_content_type"
    } else {
        "fetch_failed"
    };
    let message = match kind {
        "access_challenge" => format!("access challenge prevented readable fetch for {source_url}"),
        "payment_required" => format!("payment required for readable fetch of {source_url}"),
        "http_error" => format!("HTTP error prevented readable fetch for {source_url}"),
        "unsupported_content_type" => {
            format!("unsupported content type prevented readable fetch for {source_url}")
        }
        _ => format!("failed to fetch readable source content for {source_url}"),
    };
    ExternalFetchFailureError {
        failure: ExternalFetchFailure {
            source_url: source_url.to_string(),
            kind: kind.to_string(),
            message,
            attempts,
        },
    }
}

pub(crate) fn discover_machine_surfaces(
    source_url: &str,
    options: MachineSurfaceDiscoveryOptions,
) -> Result<ExternalMachineSurfaceReport> {
    let parsed = Url::parse(source_url)
        .with_context(|| format!("failed to parse source URL for discovery: {source_url}"))?;
    let origin_url = origin_url(&parsed)?;
    let client = external_client()?;
    let mut report = ExternalMachineSurfaceReport {
        source_url: source_url.to_string(),
        origin_url: origin_url.clone(),
        content_signals: Vec::new(),
        surfaces: Vec::new(),
        access_routes: Vec::new(),
        attempts: Vec::new(),
    };
    let mut sitemap_candidates = BTreeSet::new();

    let robots_url = join_origin_path(&origin_url, "/robots.txt")?;
    if let Some(response) = fetch_discovery_candidate(
        &client,
        "robots_txt",
        &robots_url,
        options.max_bytes,
        &mut report.attempts,
    )? && response_is_readable(&response)
    {
        push_machine_surface(
            &mut report.surfaces,
            options.surface_limit,
            ExternalMachineSurface {
                kind: "robots_txt".to_string(),
                url: response.final_url.clone(),
                source: "well_known".to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                description: Some("Robots directives and optional content signals.".to_string()),
                notes: Vec::new(),
            },
        );
        let (signals, sitemaps) = parse_robots_machine_hints(&response.body, &response.final_url);
        report.content_signals.extend(signals);
        sitemap_candidates.extend(sitemaps);
    }

    sitemap_candidates.insert(join_origin_path(&origin_url, "/sitemap.xml")?);
    discover_sitemap_surfaces(
        &client,
        source_url,
        options,
        &mut report,
        &mut sitemap_candidates,
    )?;

    if options.probe_source_page
        && let Some(response) = fetch_discovery_candidate(
            &client,
            "source_page_static",
            source_url,
            options.max_bytes,
            &mut report.attempts,
        )?
        && response_is_readable(&response)
        && response.content_type.contains("text/html")
    {
        push_machine_surface(
            &mut report.surfaces,
            options.surface_limit,
            ExternalMachineSurface {
                kind: "source_html".to_string(),
                url: response.final_url.clone(),
                source: "source_page".to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                description: Some(
                    "Source page static HTML was readable for metadata discovery.".to_string(),
                ),
                notes: Vec::new(),
            },
        );
        for surface in extract_machine_surfaces_from_html(&response.body, &response.final_url) {
            push_machine_surface(&mut report.surfaces, options.surface_limit, surface);
        }
    }

    report.access_routes = build_access_routes(&report, options.source_known_blocked);
    Ok(report)
}

fn discover_sitemap_surfaces(
    client: &ExternalClient,
    source_url: &str,
    options: MachineSurfaceDiscoveryOptions,
    report: &mut ExternalMachineSurfaceReport,
    sitemap_candidates: &mut BTreeSet<String>,
) -> Result<()> {
    let mut attempted = BTreeSet::new();
    let mut sitemap_fetches = 0usize;
    while let Some(sitemap_url) = sitemap_candidates.pop_first() {
        if sitemap_fetches >= 8 || !attempted.insert(sitemap_url.clone()) {
            continue;
        }
        sitemap_fetches += 1;
        let Some(response) = fetch_discovery_candidate(
            client,
            "sitemap",
            &sitemap_url,
            options.max_bytes,
            &mut report.attempts,
        )?
        else {
            continue;
        };
        if !response_is_readable(&response) {
            continue;
        }
        let locs = extract_xml_loc_values(&response.body);
        let is_index = xml_contains_tag(&response.body, "sitemapindex");
        push_machine_surface(
            &mut report.surfaces,
            options.surface_limit,
            ExternalMachineSurface {
                kind: if is_index {
                    "sitemap_index".to_string()
                } else {
                    "sitemap".to_string()
                },
                url: response.final_url.clone(),
                source: "robots_or_well_known".to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                description: Some(format!("Sitemap document with {} URL entries.", locs.len())),
                notes: Vec::new(),
            },
        );
        if is_index {
            for loc in locs {
                if Url::parse(&loc).is_ok() {
                    sitemap_candidates.insert(loc);
                }
            }
            continue;
        }
        let mut relevant = Vec::new();
        let mut samples = Vec::new();
        for loc in locs {
            if sitemap_url_matches_source(source_url, &loc) {
                relevant.push(loc);
            } else if samples.len() < 3 {
                samples.push(loc);
            }
        }
        for loc in relevant.into_iter().chain(samples.into_iter()) {
            push_machine_surface(
                &mut report.surfaces,
                options.surface_limit,
                ExternalMachineSurface {
                    kind: "sitemap_url".to_string(),
                    url: loc,
                    source: response.final_url.clone(),
                    http_status: None,
                    content_type: None,
                    description: Some("URL declared by a readable sitemap.".to_string()),
                    notes: Vec::new(),
                },
            );
        }
    }
    Ok(())
}

fn fetch_discovery_candidate(
    client: &ExternalClient,
    mode: &str,
    url: &str,
    max_bytes: usize,
    attempts: &mut Vec<ExternalFetchAttempt>,
) -> Result<Option<TextHttpResponse>> {
    let response = match request_text_candidate(client, mode, url, max_bytes) {
        Ok(response) => response,
        Err(error) => {
            attempts.push(ExternalFetchAttempt {
                mode: mode.to_string(),
                url: url.to_string(),
                outcome: "request_error".to_string(),
                http_status: None,
                content_type: None,
                message: Some(error.to_string()),
            });
            return Ok(None);
        }
    };
    let outcome = if response.http_status == 402 || response.crawler_price.is_some() {
        "payment_required"
    } else if detect_access_challenge_response(&response) {
        "access_challenge"
    } else if response.http_status >= 400 {
        "http_error"
    } else if !is_supported_text_content_type(&response.content_type) {
        "unsupported_content_type"
    } else {
        "success"
    };
    attempts.push(ExternalFetchAttempt {
        mode: mode.to_string(),
        url: url.to_string(),
        outcome: outcome.to_string(),
        http_status: Some(response.http_status),
        content_type: Some(response.content_type.clone()),
        message: discovery_attempt_message(outcome, &response),
    });
    Ok(Some(response))
}

fn discovery_attempt_message(outcome: &str, response: &TextHttpResponse) -> Option<String> {
    match outcome {
        "access_challenge" => Some(access_challenge_message(response)),
        "payment_required" => Some(payment_required_message(response)),
        "http_error" => Some(format!("HTTP {}", response.http_status)),
        "unsupported_content_type" => Some(format!(
            "unsupported content-type: {}",
            response.content_type
        )),
        _ => None,
    }
}

fn response_is_readable(response: &TextHttpResponse) -> bool {
    response.http_status < 400
        && !detect_access_challenge_response(response)
        && is_supported_text_content_type(&response.content_type)
}

fn origin_url(parsed: &Url) -> Result<String> {
    let mut origin = parsed.clone();
    origin.set_path("/");
    origin.set_query(None);
    origin.set_fragment(None);
    Ok(origin.to_string())
}

fn join_origin_path(origin_url: &str, path: &str) -> Result<String> {
    Url::parse(origin_url)
        .with_context(|| format!("failed to parse origin URL: {origin_url}"))?
        .join(path)
        .with_context(|| format!("failed to build discovery URL for path: {path}"))
        .map(|value| value.to_string())
}

fn push_machine_surface(
    surfaces: &mut Vec<ExternalMachineSurface>,
    limit: usize,
    surface: ExternalMachineSurface,
) {
    if surfaces.len() >= limit {
        return;
    }
    if surfaces
        .iter()
        .any(|existing| existing.kind == surface.kind && existing.url == surface.url)
    {
        return;
    }
    surfaces.push(surface);
}

fn parse_robots_machine_hints(
    robots_text: &str,
    robots_url: &str,
) -> (Vec<ExternalContentSignal>, Vec<String>) {
    let mut signals = Vec::new();
    let mut sitemaps = Vec::new();
    for (index, line) in robots_text.lines().enumerate() {
        if let Some(value) = directive_value(line, "content-signal") {
            for pair in value.split(',') {
                let Some((key, value)) = pair.split_once('=') else {
                    continue;
                };
                let key = key.trim().to_ascii_lowercase();
                let value = value.trim().to_ascii_lowercase();
                if key.is_empty() || value.is_empty() {
                    continue;
                }
                signals.push(ExternalContentSignal {
                    key,
                    value,
                    source_url: robots_url.to_string(),
                    line: index + 1,
                });
            }
        }
        if let Some(value) = directive_value(line, "sitemap")
            && Url::parse(value).is_ok()
        {
            sitemaps.push(value.to_string());
        }
    }
    (signals, sitemaps)
}

fn directive_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    let (name, value) = trimmed.split_once(':')?;
    if name.trim().eq_ignore_ascii_case(key) {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

fn extract_xml_loc_values(xml: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0usize;
    while let Some(start) = index_of_ignore_case(xml, "<loc", index) {
        let Some(tag_end_offset) = xml[start..].find('>') else {
            break;
        };
        let content_start = start + tag_end_offset + 1;
        let Some(end) = index_of_ignore_case(xml, "</loc>", content_start) else {
            break;
        };
        let value = decode_html(xml[content_start..end].trim());
        if !value.is_empty() {
            values.push(value);
        }
        index = end + "</loc>".len();
    }
    values
}

fn xml_contains_tag(xml: &str, tag: &str) -> bool {
    let open = format!("<{tag}");
    index_of_ignore_case(xml, &open, 0).is_some()
}

fn sitemap_url_matches_source(source_url: &str, sitemap_url: &str) -> bool {
    let normalized_source = source_url.trim_end_matches('/');
    let normalized_sitemap = sitemap_url.trim_end_matches('/');
    if normalized_source.eq_ignore_ascii_case(normalized_sitemap) {
        return true;
    }
    let Some(token) = Url::parse(source_url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_string))
        })
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| value.len() >= 4)
    else {
        return false;
    };
    sitemap_url.to_ascii_lowercase().contains(&token)
}

fn extract_machine_surfaces_from_html(html: &str, final_url: &str) -> Vec<ExternalMachineSurface> {
    let mut surfaces = Vec::new();
    for tag in scan_tags(&extract_head(html), "link") {
        let rel = tag
            .attrs
            .get("rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let content_type = tag
            .attrs
            .get("type")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let href = tag.attrs.get("href").map(String::as_str).unwrap_or("");
        if href.is_empty() || !rel.contains("alternate") {
            continue;
        }
        let kind = if content_type.contains("rss") {
            Some("rss_feed")
        } else if content_type.contains("atom") {
            Some("atom_feed")
        } else if content_type.contains("json") && content_type.contains("feed") {
            Some("json_feed")
        } else {
            None
        };
        let Some(kind) = kind else {
            continue;
        };
        let Ok(url) = Url::parse(final_url).and_then(|base| base.join(href)) else {
            continue;
        };
        surfaces.push(ExternalMachineSurface {
            kind: kind.to_string(),
            url: url.to_string(),
            source: final_url.to_string(),
            http_status: None,
            content_type: Some(content_type),
            description: Some("Feed link declared by source page HTML.".to_string()),
            notes: Vec::new(),
        });
    }
    if index_of_ignore_case(html, "application/ld+json", 0).is_some() {
        surfaces.push(ExternalMachineSurface {
            kind: "structured_data".to_string(),
            url: final_url.to_string(),
            source: "source_page".to_string(),
            http_status: None,
            content_type: Some("application/ld+json".to_string()),
            description: Some("Source page declares JSON-LD structured data.".to_string()),
            notes: Vec::new(),
        });
    }
    surfaces
}

fn build_access_routes(
    report: &ExternalMachineSurfaceReport,
    source_known_blocked: bool,
) -> Vec<ExternalAccessRoute> {
    // The source-page attempt is the only signal from the discovery report itself
    // that determines whether the *source* is blocked. Ancillary blocks (an incidental
    // sitemap 404 on a readable site) should not recommend "blocked source" routes.
    // When a caller already knows the source is blocked (fetch-error path), the
    // `source_known_blocked` flag supplies that signal directly.
    let source_page_attempt = report
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.mode == "source_page_static");
    let source_page_blocked = source_known_blocked
        || source_page_attempt.is_some_and(|attempt| {
            matches!(
                attempt.outcome.as_str(),
                "access_challenge" | "payment_required" | "http_error" | "request_error"
            )
        });
    let any_attempt_blocked = source_known_blocked
        || report.attempts.iter().any(|attempt| {
            matches!(
                attempt.outcome.as_str(),
                "access_challenge" | "payment_required"
            )
        });

    let mut routes = Vec::new();
    if !report.surfaces.is_empty() {
        routes.push(ExternalAccessRoute {
            kind: "machine_surfaces".to_string(),
            status: "available".to_string(),
            description: "Inspect declared robots, sitemap, feed, or structured-data surfaces before selecting alternate citable sources.".to_string(),
        });
    }
    if !report.content_signals.is_empty() {
        routes.push(ExternalAccessRoute {
            kind: "robots_content_signals".to_string(),
            status: "detected".to_string(),
            description: "Robots.txt declares content-use signals; agents should preserve these as source-access context rather than treating them as article evidence.".to_string(),
        });
    }
    if any_attempt_blocked {
        // Challenge/payment hints apply whenever the server sends them, even for
        // ancillary surfaces, because they signal the owner's posture on automated
        // access as a whole.
        routes.push(ExternalAccessRoute {
            kind: "site_owner_access".to_string(),
            status: "requires_source_owner".to_string(),
            description: "If the source owner wants agentic access, use an explicit API/feed/export, allowlist, service token, or documented crawler policy. Wikitool does not solve browser challenges.".to_string(),
        });
        routes.push(ExternalAccessRoute {
            kind: "verified_bot_program".to_string(),
            status: "not_applicable_to_local_one_off_fetches".to_string(),
            description: "Cloudflare Verified Bots are for documented crawler operators with owner consent, robots etiquette, public behavior docs, and stable IP validation; this does not make local ad-hoc fetches eligible.".to_string(),
        });
    }
    if source_page_blocked {
        // The "blocked source" fallbacks are only recommended when the source page
        // itself could not be fetched — not when only ancillary surfaces fail.
        routes.push(ExternalAccessRoute {
            kind: "user_supplied_source_artifact".to_string(),
            status: "manual_provenance_required".to_string(),
            description: "If a human has lawful access, they may provide a saved HTML, PDF, text excerpt, or other artifact; wikitool should preserve that provenance explicitly.".to_string(),
        });
        routes.push(ExternalAccessRoute {
            kind: "alternate_accessible_source".to_string(),
            status: "recommended_when_blocked".to_string(),
            description: "When readable source text remains unavailable, use an accessible source with equivalent authority instead of citing fetch diagnostics or challenge pages.".to_string(),
        });
    }
    routes
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
    let challenge_notice = detect_access_challenge(html)
        .then(|| format!(
            "Access challenge detected while fetching {final_url}. The site returned anti-bot HTML instead of readable article content."
        ));

    match options.profile {
        ExternalFetchProfile::Legacy => {
            let extract = metadata
                .description
                .clone()
                .or_else(|| summarize_text(html, 280));
            ExternalFetchResult {
                title,
                content: html.to_string(),
                fetched_at: now_iso8601_utc(),
                revision_timestamp: None,
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
                fetch_attempts: Vec::new(),
            }
        }
        ExternalFetchProfile::Research => {
            if let Some(note) = challenge_notice {
                let content_hash = compute_hash(&note);
                return ExternalFetchResult {
                    title,
                    content: note.clone(),
                    fetched_at: now_iso8601_utc(),
                    revision_timestamp: None,
                    extract: Some(note),
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
                    byline: None,
                    published_at: None,
                    fetch_mode: Some(FetchMode::Static),
                    extraction_quality: Some(ExtractionQuality::Low),
                    fetch_attempts: Vec::new(),
                };
            }
            let extracted = extract_readable_text(html, options.max_bytes);
            let app_shell = detect_app_shell_html(html);
            if extracted.is_empty() {
                let content = build_metadata_fallback_content(
                    &metadata,
                    final_url,
                    app_shell,
                    options.max_bytes,
                );
                let extract = metadata
                    .description
                    .clone()
                    .or_else(|| summarize_text(&content, 280));
                let content_hash = compute_hash(&content);

                return ExternalFetchResult {
                    title,
                    content,
                    fetched_at: now_iso8601_utc(),
                    revision_timestamp: None,
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
                    extraction_quality: Some(ExtractionQuality::Low),
                    fetch_attempts: Vec::new(),
                };
            }
            let content = extracted;
            let extract = metadata
                .description
                .clone()
                .or_else(|| summarize_text(&content, 280));
            let extraction_quality = Some(score_extraction_quality(&content, extract.as_deref()));
            let content_hash = compute_hash(&content);

            ExternalFetchResult {
                title,
                content,
                fetched_at: now_iso8601_utc(),
                revision_timestamp: None,
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
                fetch_attempts: Vec::new(),
            }
        }
    }
}

fn build_metadata_fallback_content(
    metadata: &HtmlMetadata,
    final_url: &str,
    app_shell: bool,
    max_bytes: usize,
) -> String {
    let mut lines = Vec::new();
    if app_shell {
        lines.push(format!(
            "Client-rendered or app-shell page detected at {final_url}. Full article text could not be extracted reliably from the static HTML."
        ));
    } else {
        lines.push(format!(
            "Readable article text could not be extracted reliably from {final_url}."
        ));
    }
    if let Some(title) = metadata.title.as_deref() {
        lines.push(format!("Title: {title}"));
    }
    if let Some(description) = metadata.description.as_deref() {
        lines.push(format!("Description: {description}"));
    }
    if let Some(byline) = metadata.byline.as_deref() {
        lines.push(format!("Author: {byline}"));
    }
    if let Some(published_at) = metadata.published_at.as_deref() {
        lines.push(format!("Published: {published_at}"));
    }
    if let Some(site_name) = metadata.site_name.as_deref() {
        lines.push(format!("Site: {site_name}"));
    }
    if let Some(canonical_url) = metadata.canonical_url.as_deref() {
        lines.push(format!("Canonical URL: {canonical_url}"));
    }
    truncate_to_byte_limit(&lines.join("\n"), max_bytes)
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
        fetched_at: now_iso8601_utc(),
        revision_timestamp: None,
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
        fetch_attempts: Vec::new(),
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

fn extract_client_redirect_url(html: &str, final_url: &str) -> Option<String> {
    let head = extract_head(html);
    let base_url = Url::parse(final_url).ok()?;
    for tag in scan_tags(&head, "meta") {
        let http_equiv = tag
            .attrs
            .get("http-equiv")
            .map(|value| value.to_ascii_lowercase());
        let id = tag.attrs.get("id").map(|value| value.to_ascii_lowercase());
        if http_equiv.as_deref() != Some("refresh") && id.as_deref() != Some("__next-page-redirect")
        {
            continue;
        }
        let content = tag.attrs.get("content")?;
        let target = parse_meta_refresh_target(content)?;
        let joined = base_url.join(target).ok()?.to_string();
        return Some(joined);
    }
    None
}

fn parse_meta_refresh_target(content: &str) -> Option<&str> {
    let lowered = content.to_ascii_lowercase();
    let marker = "url=";
    let at = lowered.find(marker)?;
    let target = content[at + marker.len()..].trim();
    let target = target.trim_matches(|ch| matches!(ch, '"' | '\'' | ' '));
    if target.is_empty() {
        None
    } else {
        Some(target)
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
            if skip_depth == 0 {
                if is_paragraph_block_tag(tag_name) {
                    append_separator(&mut output, "\n\n");
                } else if is_block_tag(tag_name) {
                    append_separator(&mut output, "\n");
                }
            }
        } else if is_skip_tag(tag_name) && !is_self_closing {
            skip_depth += 1;
        } else if skip_depth == 0 {
            if tag_name == "br" {
                append_separator(&mut output, "\n");
            } else if tag_name == "li" {
                append_separator(&mut output, "\n- ");
            } else if is_paragraph_block_tag(tag_name) {
                append_separator(&mut output, "\n\n");
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
    match separator {
        "\n\n" => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if output.ends_with("\n\n") {
                return;
            }
            if output.ends_with('\n') {
                output.push('\n');
            } else {
                output.push_str("\n\n");
            }
        }
        "\n- " => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if output.ends_with('\n') {
                output.push_str("- ");
            } else {
                output.push_str("\n- ");
            }
        }
        "\n" => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }
        other => output.push_str(other),
    }
}

fn normalize_extracted_text(value: &str, max_bytes: usize) -> String {
    let mut lines = Vec::new();
    let mut blank_count = 0usize;
    for line in value.lines() {
        let collapsed = collapse_inline_whitespace(line);
        if collapsed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                lines.push(String::new());
            }
            continue;
        }
        blank_count = 0;
        lines.push(collapsed);
    }
    let mut merged = merge_isolated_bullet_markers(&lines);
    compact_adjacent_list_item_spacing(&mut merged);
    while matches!(merged.first(), Some(line) if line.is_empty()) {
        merged.remove(0);
    }
    while matches!(merged.last(), Some(line) if line.is_empty()) {
        merged.pop();
    }
    truncate_to_byte_limit(&merged.join("\n"), max_bytes)
}

/// Drop isolated single-character list markers (`-`, `*`, `•`) emitted as their own
/// line by HTML-to-text extractors, joining them with the following text line. Also
/// strips leading image/credit attribution lines (`© …`) that sit above their related
/// prose but carry no reader-facing content on their own.
fn merge_isolated_bullet_markers(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    while index < lines.len() {
        let line = &lines[index];
        let trimmed = line.trim();
        if matches!(trimmed, "-" | "*" | "\u{2022}")
            && let Some((next_index, next)) = next_nonblank_line(lines, index + 1)
            && !next.trim().is_empty()
        {
            out.push(format!("- {}", next.trim()));
            index = next_index + 1;
            continue;
        }
        index += 1;
        out.push(line.clone());
    }
    out
}

fn next_nonblank_line(lines: &[String], start: usize) -> Option<(usize, &str)> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| (index, line.as_str()))
}

fn compact_adjacent_list_item_spacing(lines: &mut Vec<String>) {
    let mut index = 1usize;
    while index + 1 < lines.len() {
        if lines[index].is_empty()
            && is_markdown_unordered_list_item(&lines[index - 1])
            && is_markdown_unordered_list_item(&lines[index + 1])
        {
            lines.remove(index);
            continue;
        }
        index += 1;
    }
}

fn is_markdown_unordered_list_item(line: &str) -> bool {
    line.trim_start().starts_with("- ")
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

fn detect_app_shell_html(html: &str) -> bool {
    let lowered = html.to_ascii_lowercase();
    let signals = [
        "__next_f.push(",
        "_next/static/",
        "__next_data__",
        "data-reactroot",
        "ng-version",
        "window.__nuxt__",
        "id=\"app\"",
        "id='app'",
    ];
    signals
        .iter()
        .filter(|signal| lowered.contains(**signal))
        .count()
        >= 2
}

fn detect_access_challenge(html: &str) -> bool {
    let lowered = html.to_ascii_lowercase();
    let vendor_signals = [
        "awswafintegration",
        "awswafcookiedomainlist",
        "challenge-container",
        "__cf_chl_",
        "cf-browser-verification",
        "captcha-delivery",
        "datadome",
        "perimeterx",
        "px-captcha",
    ];
    if vendor_signals.iter().any(|signal| lowered.contains(signal)) {
        return true;
    }

    let generic_signals = [
        "verify that you're not a robot",
        "checking your browser",
        "enable javascript and then reload the page",
        "javascript is disabled",
        "access denied",
        "captcha",
        "challenge.js",
    ];
    generic_signals
        .iter()
        .filter(|signal| lowered.contains(**signal))
        .count()
        >= 2
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

fn is_paragraph_block_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "p" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
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
    decode_html_entities(text)
}

#[cfg(test)]
mod tests {
    use super::{
        HtmlMetadata, TextHttpResponse, access_challenge_message, build_access_routes,
        build_html_fetch_result, build_metadata_fallback_content, collapse_inline_whitespace,
        detect_access_challenge, detect_access_challenge_response, detect_app_shell_html,
        extract_client_redirect_url, extract_html_metadata, extract_machine_surfaces_from_html,
        extract_readable_text, extract_xml_loc_values, normalize_extracted_text,
        parse_robots_machine_hints, sitemap_url_matches_source,
    };
    use crate::research::model::{ExternalFetchAttempt, ExternalMachineSurfaceReport};
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
        assert!(text.contains("Headline\n\nFirst paragraph."));
        assert!(text.contains("First paragraph.\n\nSecond paragraph."));
        assert!(text.contains("- Alpha\n- Beta"));
        assert!(!text.contains("Site navigation"));
        assert!(!text.contains("Footer links"));
    }

    #[test]
    fn access_routes_suppress_blocked_fallbacks_when_only_ancillary_attempts_failed() {
        let mut report = ExternalMachineSurfaceReport {
            source_url: "https://example.com/article".to_string(),
            origin_url: "https://example.com/".to_string(),
            content_signals: Vec::new(),
            surfaces: Vec::new(),
            access_routes: Vec::new(),
            attempts: vec![
                ExternalFetchAttempt {
                    mode: "robots_txt".to_string(),
                    url: "https://example.com/robots.txt".to_string(),
                    outcome: "success".to_string(),
                    http_status: Some(200),
                    content_type: Some("text/plain".to_string()),
                    message: None,
                },
                ExternalFetchAttempt {
                    mode: "sitemap".to_string(),
                    url: "https://example.com/sitemap.xml".to_string(),
                    outcome: "http_error".to_string(),
                    http_status: Some(404),
                    content_type: Some("text/html".to_string()),
                    message: Some("HTTP 404".to_string()),
                },
                ExternalFetchAttempt {
                    mode: "source_page_static".to_string(),
                    url: "https://example.com/article".to_string(),
                    outcome: "success".to_string(),
                    http_status: Some(200),
                    content_type: Some("text/html".to_string()),
                    message: None,
                },
            ],
        };
        report.access_routes = build_access_routes(&report, false);
        let kinds: Vec<&str> = report
            .access_routes
            .iter()
            .map(|route| route.kind.as_str())
            .collect();
        assert!(
            !kinds.contains(&"user_supplied_source_artifact"),
            "readable source should not recommend manual provenance"
        );
        assert!(
            !kinds.contains(&"alternate_accessible_source"),
            "readable source should not recommend alternate accessible source"
        );
        assert!(
            !kinds.contains(&"site_owner_access"),
            "ancillary HTTP error should not trigger owner-access route"
        );
    }

    #[test]
    fn access_routes_include_blocked_fallbacks_when_source_known_blocked() {
        let mut report = ExternalMachineSurfaceReport {
            source_url: "https://example.com/article".to_string(),
            origin_url: "https://example.com/".to_string(),
            content_signals: Vec::new(),
            surfaces: Vec::new(),
            access_routes: Vec::new(),
            attempts: vec![ExternalFetchAttempt {
                mode: "robots_txt".to_string(),
                url: "https://example.com/robots.txt".to_string(),
                outcome: "success".to_string(),
                http_status: Some(200),
                content_type: Some("text/plain".to_string()),
                message: None,
            }],
        };
        report.access_routes = build_access_routes(&report, true);
        let kinds: Vec<&str> = report
            .access_routes
            .iter()
            .map(|route| route.kind.as_str())
            .collect();
        assert!(kinds.contains(&"user_supplied_source_artifact"));
        assert!(kinds.contains(&"alternate_accessible_source"));
        assert!(kinds.contains(&"site_owner_access"));
    }

    #[test]
    fn normalize_extracted_text_merges_isolated_bullet_markers() {
        let input = "Status\n\n-\n\nNear threatened\n-\nVulnerable\n\u{2022}\nEndangered";
        let cleaned = normalize_extracted_text(input, 1_000);
        assert!(cleaned.contains("- Near threatened"));
        assert!(cleaned.contains("- Vulnerable"));
        assert!(cleaned.contains("- Endangered"));
        assert!(!cleaned.contains("\n-\n"));
        assert!(!cleaned.contains("\n\u{2022}\n"));
    }

    #[test]
    fn normalize_extracted_text_preserves_paragraphs_and_compact_lists() {
        let cleaned = normalize_extracted_text(
            "Heading\n\nFirst paragraph.\n\nSecond paragraph.\n\n- Alpha\n\n- Beta",
            1_000,
        );

        assert!(cleaned.contains("Heading\n\nFirst paragraph."));
        assert!(cleaned.contains("First paragraph.\n\nSecond paragraph."));
        assert!(cleaned.contains("- Alpha\n- Beta"));
        assert!(!cleaned.contains("- Alpha\n\n- Beta"));
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

    #[test]
    fn research_profile_reports_access_challenge_pages_cleanly() {
        let result = build_html_fetch_result(
            r#"
            <html>
              <head><title></title></head>
              <body>
                <script>window.awsWafCookieDomainList = [];</script>
                <div id="challenge-container"></div>
                <noscript>
                  <h1>JavaScript is disabled</h1>
                  verify that you're not a robot
                </noscript>
              </body>
            </html>
            "#,
            "https://example.com/protected",
            "example.com",
            "protected",
            &ExternalFetchOptions {
                format: ExternalFetchFormat::Html,
                max_bytes: 10_000,
                profile: ExternalFetchProfile::Research,
            },
        );

        assert_eq!(result.content_format, "text");
        assert_eq!(result.fetch_mode, Some(FetchMode::Static));
        assert_eq!(result.extraction_quality, Some(ExtractionQuality::Low));
        assert!(
            result.content.contains(
                "Access challenge detected while fetching https://example.com/protected."
            )
        );
        assert!(!result.content.contains("window.awsWafCookieDomainList"));
    }

    #[test]
    fn research_profile_falls_back_to_metadata_when_static_extraction_fails() {
        let result = build_html_fetch_result(
            r#"
            <html>
              <head>
                <title>Notes on Reading Remilia's art</title>
                <meta name="description" content="A reflection on Remilia's aesthetics and reading practices." />
                <meta name="author" content="Charlotte Fang" />
                <meta property="og:site_name" content="Paragraph" />
              </head>
              <body>
                <div id="app"></div>
                <script>self.__next_f.push([1, "payload"]);</script>
                <script src="/_next/static/chunks/main.js"></script>
              </body>
            </html>
            "#,
            "https://paragraph.com/@charlemagnefang/rjACW1CDER8t7UQDDgwd",
            "paragraph.com",
            "rjACW1CDER8t7UQDDgwd",
            &ExternalFetchOptions {
                format: ExternalFetchFormat::Html,
                max_bytes: 10_000,
                profile: ExternalFetchProfile::Research,
            },
        );

        assert_eq!(result.content_format, "text");
        assert_eq!(result.fetch_mode, Some(FetchMode::Static));
        assert_eq!(result.extraction_quality, Some(ExtractionQuality::Low));
        assert!(
            result
                .content
                .contains("Client-rendered or app-shell page detected")
        );
        assert!(
            result
                .content
                .contains("Title: Notes on Reading Remilia's art")
        );
        assert!(
            result
                .content
                .contains("Description: A reflection on Remilia's aesthetics")
        );
        assert!(!result.content.contains("__next_f.push"));
        assert!(!result.content.contains("<html>"));
    }

    #[test]
    fn detects_vendor_and_generic_access_challenge_signals() {
        assert!(detect_access_challenge(
            "<html><body><script>window.awsWafCookieDomainList = [];</script></body></html>"
        ));
        assert!(detect_access_challenge(
            "<html><body>JavaScript is disabled. Verify that you're not a robot.</body></html>"
        ));
        assert!(!detect_access_challenge(
            "<html><body><article><p>Readable essay text.</p></article></body></html>"
        ));
    }

    #[test]
    fn detects_cloudflare_challenge_header() {
        let response = TextHttpResponse {
            final_url: "https://example.com/protected".to_string(),
            http_status: 403,
            content_type: "text/html; charset=UTF-8".to_string(),
            cf_mitigated: Some("challenge".to_string()),
            crawler_price: None,
            body: "<html><body>generic challenge shell</body></html>".to_string(),
        };

        assert!(detect_access_challenge_response(&response));
        assert!(access_challenge_message(&response).contains("cf-mitigated"));
    }

    #[test]
    fn parses_robots_content_signals_and_sitemaps() {
        let (signals, sitemaps) = parse_robots_machine_hints(
            r#"
            User-agent: *
            Content-Signal: search=yes, ai-train=no
            Allow: /
            Sitemap: https://example.com/sitemap.xml
            "#,
            "https://example.com/robots.txt",
        );

        assert_eq!(signals.len(), 2);
        assert_eq!(signals[0].key, "search");
        assert_eq!(signals[0].value, "yes");
        assert_eq!(signals[1].key, "ai-train");
        assert_eq!(signals[1].value, "no");
        assert_eq!(sitemaps, vec!["https://example.com/sitemap.xml"]);
    }

    #[test]
    fn extracts_sitemap_locs_and_matches_source_token() {
        let locs = extract_xml_loc_values(
            r#"
            <urlset>
              <url><loc>https://example.com/animals/cheetah</loc></url>
              <url><loc>https://example.com/animals/lion</loc></url>
            </urlset>
            "#,
        );

        assert_eq!(locs.len(), 2);
        assert!(sitemap_url_matches_source(
            "https://example.com/animals/cheetah",
            &locs[0]
        ));
        assert!(!sitemap_url_matches_source(
            "https://example.com/animals/cheetah",
            &locs[1]
        ));
    }

    #[test]
    fn extracts_feed_and_structured_data_surfaces_from_html() {
        let surfaces = extract_machine_surfaces_from_html(
            r#"
            <html>
              <head>
                <link rel="alternate" type="application/rss+xml" href="/feed.xml" />
                <script type="application/ld+json">{"@type":"Article"}</script>
              </head>
            </html>
            "#,
            "https://example.com/article",
        );

        assert!(surfaces.iter().any(|surface| surface.kind == "rss_feed"));
        assert!(
            surfaces
                .iter()
                .any(|surface| surface.kind == "structured_data")
        );
    }

    #[test]
    fn detects_framework_app_shell_markup() {
        assert!(detect_app_shell_html(
            "<html><body><div id=\"app\"></div><script>self.__next_f.push([1, \"payload\"])</script><script src=\"/_next/static/chunks/main.js\"></script></body></html>"
        ));
        assert!(!detect_app_shell_html(
            "<html><body><article><p>Readable essay text.</p></article></body></html>"
        ));
    }

    #[test]
    fn extracts_client_redirect_url_from_meta_refresh() {
        let redirect = extract_client_redirect_url(
            r#"
            <html>
              <head>
                <meta id="__next-page-redirect" http-equiv="refresh" content="1;url=../@charlemagnefang/notes-towards-a-study-of-remilia-s-art" />
              </head>
            </html>
            "#,
            "https://paragraph.com/@charlemagnefang/rjACW1CDER8t7UQDDgwd",
        )
        .expect("client redirect");

        assert_eq!(
            redirect,
            "https://paragraph.com/@charlemagnefang/notes-towards-a-study-of-remilia-s-art"
        );
    }

    #[test]
    fn metadata_fallback_content_includes_useful_context() {
        let content = build_metadata_fallback_content(
            &HtmlMetadata {
                title: Some("Example title".to_string()),
                canonical_url: Some("https://example.com/article".to_string()),
                site_name: Some("Example".to_string()),
                byline: Some("Author".to_string()),
                published_at: Some("2026-03-17".to_string()),
                description: Some("Summary text".to_string()),
            },
            "https://example.com/article",
            true,
            10_000,
        );

        assert!(content.contains("Client-rendered or app-shell page detected"));
        assert!(content.contains("Title: Example title"));
        assert!(content.contains("Description: Summary text"));
        assert!(content.contains("Author: Author"));
        assert!(content.contains("Published: 2026-03-17"));
        assert!(content.contains("Site: Example"));
        assert!(content.contains("Canonical URL: https://example.com/article"));
    }
}
