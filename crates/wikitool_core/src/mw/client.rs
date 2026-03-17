use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::{StatusCode, Url};
use serde::Serialize;
use serde_json::Value;

use crate::support::{env_value, env_value_u64, env_value_usize};

#[derive(Debug, Clone)]
pub struct PageTimestampInfo {
    pub title: String,
    pub timestamp: String,
    pub revision_id: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalSearchHit {
    pub title: String,
    pub namespace: i32,
    pub page_id: i64,
    pub word_count: Option<u64>,
    pub snippet: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemotePage {
    pub title: String,
    pub namespace: i32,
    pub page_id: i64,
    pub revision_id: i64,
    pub timestamp: String,
    pub content: String,
}

pub trait WikiReadApi {
    fn get_all_pages(&mut self, namespace: i32) -> Result<Vec<String>>;
    fn get_category_members(&mut self, category: &str) -> Result<Vec<String>>;
    fn get_recent_changes(&mut self, since: &str, namespaces: &[i32]) -> Result<Vec<String>>;
    fn get_page_contents(&mut self, titles: &[String]) -> Result<Vec<RemotePage>>;
    fn search(
        &mut self,
        query: &str,
        namespaces: &[i32],
        limit: usize,
    ) -> Result<Vec<ExternalSearchHit>>;
    fn request_count(&self) -> usize;
}

pub trait WikiWriteApi: WikiReadApi {
    fn login(&mut self, username: &str, password: &str) -> Result<()>;
    fn get_page_timestamps(&mut self, titles: &[String]) -> Result<Vec<PageTimestampInfo>>;
    fn edit_page(&mut self, title: &str, content: &str, summary: &str) -> Result<RemotePage>;
    fn delete_page(&mut self, title: &str, reason: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct MediaWikiClientConfig {
    pub api_url: String,
    pub user_agent: String,
    pub timeout_ms: u64,
    pub rate_limit_read_ms: u64,
    pub rate_limit_write_ms: u64,
    pub max_retries: usize,
    pub max_write_retries: usize,
    pub retry_delay_ms: u64,
}

impl MediaWikiClientConfig {
    pub fn from_env() -> Self {
        Self::from_env_with_defaults("", crate::config::DEFAULT_USER_AGENT)
    }

    pub fn from_config(config: &crate::config::WikiConfig) -> Self {
        let api_default = config.wiki.api_url.as_deref().unwrap_or("");
        Self::from_env_with_defaults(api_default, &config.user_agent())
    }

    fn from_env_with_defaults(api_url_default: &str, user_agent_default: &str) -> Self {
        Self {
            api_url: env_value("WIKI_API_URL", api_url_default),
            user_agent: env_value("WIKI_USER_AGENT", user_agent_default),
            timeout_ms: env_value_u64("WIKI_HTTP_TIMEOUT_MS", 30_000),
            rate_limit_read_ms: env_value_u64("WIKI_RATE_LIMIT_READ", 300),
            rate_limit_write_ms: env_value_u64("WIKI_RATE_LIMIT_WRITE", 1_000),
            max_retries: env_value_usize("WIKI_HTTP_RETRIES", 2),
            max_write_retries: env_value_usize("WIKI_HTTP_WRITE_RETRIES", 1),
            retry_delay_ms: env_value_u64("WIKI_HTTP_RETRY_DELAY_MS", 500),
        }
    }
}

pub struct MediaWikiClient {
    pub(crate) client: Client,
    pub(crate) config: MediaWikiClientConfig,
    pub(crate) last_request_at: Option<Instant>,
    pub(crate) request_count: usize,
    pub(crate) csrf_token: Option<String>,
}

impl MediaWikiClient {
    pub fn from_env() -> Result<Self> {
        Self::new(MediaWikiClientConfig::from_env())
    }

    pub fn from_config(config: &crate::config::WikiConfig) -> Result<Self> {
        Self::new(MediaWikiClientConfig::from_config(config))
    }

    pub fn new(config: MediaWikiClientConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .cookie_store(true)
            .build()
            .context("failed to build MediaWiki HTTP client")?;

        Ok(Self {
            client,
            config,
            last_request_at: None,
            request_count: 0,
            csrf_token: None,
        })
    }

    pub(crate) fn request_json_get(&mut self, params: &[(&str, String)]) -> Result<Value> {
        let base_url = Url::parse(&self.config.api_url)
            .with_context(|| format!("invalid WIKI_API_URL: {}", self.config.api_url))?;

        let mut pairs = Vec::with_capacity(params.len() + 2);
        pairs.push(("format".to_string(), "json".to_string()));
        pairs.push(("formatversion".to_string(), "2".to_string()));
        for (key, value) in params {
            if !value.is_empty() {
                pairs.push(((*key).to_string(), value.clone()));
            }
        }

        for attempt in 0..=self.config.max_retries {
            self.apply_rate_limit(false);
            let response = self
                .client
                .get(base_url.clone())
                .header("User-Agent", self.config.user_agent.clone())
                .query(&pairs)
                .send();

            match response {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        if attempt < self.config.max_retries && is_retryable_status(status) {
                            self.wait_before_retry(attempt, false);
                            continue;
                        }
                        bail!("MediaWiki API request failed with HTTP {status}");
                    }

                    let payload: Value = response
                        .json()
                        .context("failed to decode MediaWiki API JSON response")?;
                    if let Some(error) = payload.get("error") {
                        let code = error
                            .get("code")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown_error");
                        let info = error
                            .get("info")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown info");
                        bail!("MediaWiki API error [{code}]: {info}");
                    }
                    return Ok(payload);
                }
                Err(error) => {
                    if attempt < self.config.max_retries && is_retryable_error(&error) {
                        self.wait_before_retry(attempt, false);
                        continue;
                    }
                    return Err(error).context("failed to call MediaWiki API");
                }
            }
        }

        bail!("MediaWiki API request exhausted retry budget")
    }

    pub(crate) fn request_json_post(
        &mut self,
        params: &[(&str, String)],
        is_write: bool,
    ) -> Result<Value> {
        let max_retries = if is_write {
            self.config.max_write_retries
        } else {
            self.config.max_retries
        };
        let mut pairs = Vec::with_capacity(params.len() + 2);
        pairs.push(("format".to_string(), "json".to_string()));
        pairs.push(("formatversion".to_string(), "2".to_string()));
        for (key, value) in params {
            if !value.is_empty() {
                pairs.push(((*key).to_string(), value.clone()));
            }
        }

        for attempt in 0..=max_retries {
            self.apply_rate_limit(is_write);
            let response = self
                .client
                .post(&self.config.api_url)
                .header("User-Agent", self.config.user_agent.clone())
                .form(&pairs)
                .send();

            match response {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        if attempt < max_retries && is_retryable_status(status) {
                            self.wait_before_retry(attempt, is_write);
                            continue;
                        }
                        bail!("MediaWiki API request failed with HTTP {status}");
                    }

                    let payload: Value = response
                        .json()
                        .context("failed to decode MediaWiki API JSON response")?;
                    if let Some(error) = payload.get("error") {
                        let code = error
                            .get("code")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown_error");
                        let info = error
                            .get("info")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown info");
                        bail!("MediaWiki API error [{code}]: {info}");
                    }
                    return Ok(payload);
                }
                Err(error) => {
                    if attempt < max_retries && is_retryable_error(&error) {
                        self.wait_before_retry(attempt, is_write);
                        continue;
                    }
                    return Err(error).context("failed to call MediaWiki API");
                }
            }
        }

        bail!("MediaWiki API request exhausted retry budget")
    }

    pub(crate) fn apply_rate_limit(&mut self, is_write: bool) {
        let delay = if is_write {
            Duration::from_millis(self.config.rate_limit_write_ms)
        } else {
            Duration::from_millis(self.config.rate_limit_read_ms)
        };
        if let Some(last) = self.last_request_at {
            let elapsed = last.elapsed();
            if elapsed < delay {
                sleep(delay - elapsed);
            }
        }
        self.last_request_at = Some(Instant::now());
        self.request_count += 1;
    }

    pub(crate) fn wait_before_retry(&self, attempt: usize, is_write: bool) {
        let exponent = u32::try_from(attempt).unwrap_or(16);
        let base = self
            .config
            .retry_delay_ms
            .saturating_mul(2u64.saturating_pow(exponent));
        let jitter = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::from(duration.subsec_millis() % 100))
            .unwrap_or(0);
        let multiplier = if is_write { 2u64 } else { 1u64 };
        sleep(Duration::from_millis(
            base.saturating_mul(multiplier).saturating_add(jitter),
        ));
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn is_retryable_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}
