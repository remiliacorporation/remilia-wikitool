use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use reqwest::blocking::RequestBuilder;
use serde_json::Value;

use crate::support::{env_value, env_value_u64, env_value_usize};

use super::model::{
    ChallengeHandoff, ExternalFetchAttempt, ExternalFetchFailure, ExternalFetchFailureError,
    ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult, ExternalFetchSession,
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
    session: Option<ExternalFetchSession>,
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
    detect_access_challenge, detect_access_challenge_vendor, extract_client_redirect_url,
    read_text_body_limited,
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
                .query(&pairs);
            let response = self.apply_session_headers(response, api_url).send();
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

    fn apply_session_headers(&self, request: RequestBuilder, url: &str) -> RequestBuilder {
        let Some(session) = &self.session else {
            return request;
        };
        if !session_matches_url(session, url) || session.cookie_header.trim().is_empty() {
            return request;
        }
        request.header("Cookie", session.cookie_header.clone())
    }
}

pub(crate) fn external_client() -> Result<ExternalClient> {
    external_client_with_session(None)
}

pub(crate) fn external_client_with_session(
    session: Option<ExternalFetchSession>,
) -> Result<ExternalClient> {
    let timeout_ms = env_value_u64("WIKITOOL_HTTP_TIMEOUT_MS", DEFAULT_TIMEOUT_MS);
    let retries = env_value_usize("WIKITOOL_HTTP_RETRIES", DEFAULT_RETRIES);
    let retry_delay_ms = env_value_u64("WIKITOOL_HTTP_RETRY_DELAY_MS", DEFAULT_RETRY_DELAY_MS);
    let user_agent = session
        .as_ref()
        .and_then(|session| session.user_agent.clone())
        .unwrap_or_else(|| env_value("WIKITOOL_USER_AGENT", DEFAULT_USER_AGENT));
    let client = Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .context("failed to build external HTTP client")?;
    Ok(ExternalClient {
        client,
        user_agent,
        session,
        retries,
        retry_delay_ms,
        last_request_at: None,
    })
}

pub(crate) fn fetch_web_url(
    url: &str,
    options: &ExternalFetchOptions,
) -> Result<ExternalFetchResult> {
    let client = external_client_with_session(options.session.clone())?;
    let mut current_url = url.to_string();
    let mut client_redirects = 0usize;
    let mut attempts = Vec::new();
    let mut challenge_handoffs = Vec::new();

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
                return Err(fetch_failure(url, attempts, challenge_handoffs).into());
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
            if outcome == "access_challenge" {
                challenge_handoffs.push(challenge_handoff_for_response(
                    url,
                    &current_url,
                    &client.user_agent,
                    &response,
                ));
            }
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
            return Err(fetch_failure(url, attempts, challenge_handoffs).into());
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
            return Err(fetch_failure(url, attempts, challenge_handoffs).into());
        }
        if options.profile == ExternalFetchProfile::Research
            && detect_access_challenge_response(&response)
        {
            challenge_handoffs.push(challenge_handoff_for_response(
                url,
                &current_url,
                &client.user_agent,
                &response,
            ));
            attempts.push(ExternalFetchAttempt {
                mode: "direct_static".to_string(),
                url: current_url.clone(),
                outcome: "access_challenge".to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                message: Some(access_challenge_message(&response)),
            });
            return Err(fetch_failure(url, attempts, challenge_handoffs).into());
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
    request = client.apply_session_headers(request, url);
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
    challenge_handoffs: Vec<ChallengeHandoff>,
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
            challenge_handoffs,
        },
    }
}

fn session_matches_url(session: &ExternalFetchSession, url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let domain = session
        .domain
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn challenge_handoff_for_response(
    source_url: &str,
    current_url: &str,
    user_agent: &str,
    response: &TextHttpResponse,
) -> ChallengeHandoff {
    let vendor = challenge_vendor_for_response(response).to_string();
    let domain = Url::parse(current_url)
        .ok()
        .and_then(|url| url.host_str().map(ToString::to_string))
        .or_else(|| {
            Url::parse(source_url)
                .ok()
                .and_then(|url| url.host_str().map(ToString::to_string))
        })
        .unwrap_or_else(|| "unknown".to_string())
        .to_ascii_lowercase();
    let (required_cookies, ttl_hint_seconds, mut notes) = challenge_handoff_vendor_policy(&vendor);
    notes.push("Wikitool does not solve or bypass browser challenges; import only cookies obtained by a human with lawful source access.".to_string());
    let mut suggested_argv = vec![
        "wikitool".to_string(),
        "research".to_string(),
        "session".to_string(),
        "import".to_string(),
        source_url.to_string(),
        "--cookies".to_string(),
        "-".to_string(),
        "--user-agent".to_string(),
        user_agent.to_string(),
    ];
    if let Some(ttl) = ttl_hint_seconds {
        suggested_argv.push("--ttl-seconds".to_string());
        suggested_argv.push(ttl.to_string());
    }
    let suggested_command = suggested_argv
        .iter()
        .map(|argument| shell_display_arg(argument))
        .collect::<Vec<_>>()
        .join(" ");
    ChallengeHandoff {
        vendor,
        url: current_url.to_string(),
        domain,
        required_cookies,
        user_agent_pin: user_agent.to_string(),
        suggested_argv,
        suggested_command,
        ttl_hint_seconds,
        notes,
    }
}

fn challenge_vendor_for_response(response: &TextHttpResponse) -> &'static str {
    if response
        .cf_mitigated
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("challenge"))
    {
        return "cloudflare";
    }
    detect_access_challenge_vendor(&response.body).unwrap_or("unknown")
}

fn challenge_handoff_vendor_policy(vendor: &str) -> (Vec<String>, Option<u64>, Vec<String>) {
    match vendor {
        "cloudflare" => (
            vec!["cf_clearance".to_string()],
            Some(1_800),
            vec![
                "Cloudflare documents cf_clearance as the challenge-passage cookie; default challenge passage is 30 minutes, configurable by the site owner.".to_string(),
            ],
        ),
        "anubis" => (
            vec![
                "anubis-auth".to_string(),
                "anubis-cookie-verification".to_string(),
            ],
            Some(86_400),
            vec![
                "Anubis stores successful proof-of-work challenge solutions in signed cookies; names may vary when the site owner changes the cookie prefix.".to_string(),
            ],
        ),
        "datadome" => (
            vec!["datadome".to_string()],
            None,
            vec!["DataDome deployments commonly use a datadome cookie, but site policy determines whether a browser session can be reused.".to_string()],
        ),
        "aws_waf" => (
            vec!["aws-waf-token".to_string()],
            None,
            vec!["AWS WAF challenge token naming can vary by integration; inspect the browser cookie jar for the exact source-issued cookie.".to_string()],
        ),
        _ => (
            Vec::new(),
            None,
            vec!["Unknown challenge vendor; import the relevant source-issued cookies if a human browser session receives them.".to_string()],
        ),
    }
}

fn shell_display_arg(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | '%'))
    {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests;
