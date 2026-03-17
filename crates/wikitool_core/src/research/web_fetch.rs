use std::io::Read;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use reqwest::blocking::Client;
use serde_json::Value;

use crate::support::{env_value, env_value_u64, env_value_usize, unix_timestamp};

use super::model::ExternalFetchResult;
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

pub(crate) fn fetch_web_url(url: &str, max_bytes: usize) -> Result<ExternalFetchResult> {
    let client = external_client()?;
    let response = client
        .client
        .get(url)
        .header("User-Agent", DEFAULT_USER_AGENT)
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
    let content = read_text_body_limited(response, max_bytes)?;

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

pub(crate) fn read_text_body_limited<R: Read>(reader: R, max_bytes: usize) -> Result<String> {
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
