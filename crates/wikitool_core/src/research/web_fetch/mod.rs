use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use serde_json::Value;

use crate::support::{env_value, env_value_u64, env_value_usize};

use super::model::{
    ExternalFetchAttempt, ExternalFetchFailure, ExternalFetchFailureError, ExternalFetchOptions,
    ExternalFetchProfile, ExternalFetchResult,
};

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

mod discovery;
mod html;

pub use discovery::MachineSurfaceDiscoveryOptions;
pub(crate) use discovery::discover_machine_surfaces;
pub(crate) use html::truncate_to_byte_limit;

use html::{
    build_html_fetch_result, build_text_fetch_result, derive_title_from_url,
    detect_access_challenge, extract_client_redirect_url, read_text_body_limited,
};
pub(super) struct TextHttpResponse {
    final_url: String,
    http_status: u16,
    content_type: String,
    cf_mitigated: Option<String>,
    crawler_price: Option<String>,
    body: String,
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

pub(super) fn request_text_candidate(
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

pub(super) fn detect_access_challenge_response(response: &TextHttpResponse) -> bool {
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

pub(super) fn access_challenge_message(response: &TextHttpResponse) -> String {
    if response
        .cf_mitigated
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("challenge"))
    {
        return "access challenge detected from cf-mitigated: challenge header".to_string();
    }
    "access challenge detected in response body".to_string()
}

pub(super) fn payment_required_message(response: &TextHttpResponse) -> String {
    if let Some(price) = response.crawler_price.as_deref() {
        return format!("payment required; crawler-price: {price}");
    }
    "payment required".to_string()
}

pub(super) fn is_supported_text_content_type(content_type: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::discovery::{
        build_access_routes, extract_machine_surfaces_from_html, extract_xml_loc_values,
        parse_robots_machine_hints, sitemap_url_matches_source,
    };
    use super::html::{
        HtmlMetadata, build_html_fetch_result, build_metadata_fallback_content,
        collapse_inline_whitespace, detect_access_challenge, detect_app_shell_html,
        extract_client_redirect_url, extract_html_metadata, extract_readable_text,
        normalize_extracted_text,
    };
    use super::{TextHttpResponse, access_challenge_message, detect_access_challenge_response};
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
    fn extracts_readable_text_handles_multibyte_text_before_tags() {
        let text = extract_readable_text(
            "<html><body><main><p>Fast cat 🐆.</p><!-- note --><p>Second paragraph.</p></main></body></html>",
            10_000,
        );

        assert!(text.contains("Fast cat 🐆."));
        assert!(text.contains("Fast cat 🐆.\n\nSecond paragraph."));
        assert!(!text.contains("note"));
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
