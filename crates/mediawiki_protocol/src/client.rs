use std::error::Error;
use std::fmt;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::{StatusCode, Url};
use serde::Serialize;
use serde_json::Value;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_snippet: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionLineageEntry {
    pub title: String,
    pub page_id: i64,
    pub revision_id: i64,
    pub timestamp: String,
    pub comment: Option<String>,
    pub comment_hidden: bool,
    pub content: Option<String>,
}

/// The authoritative identity returned by a successful `action=edit` call.
/// Content is deliberately absent: callers must fetch `new_revision_id`
/// explicitly before treating the mutation as reconciled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditReceipt {
    pub title: String,
    pub page_id: i64,
    pub old_revision_id: i64,
    pub new_revision_id: i64,
    pub new_timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteReceipt {
    pub title: String,
    pub log_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteLogEntry {
    pub log_id: i64,
    pub title: String,
    pub timestamp: String,
    pub comment: Option<String>,
    pub comment_hidden: bool,
    pub user: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted(DeleteReceipt),
    AlreadyMissing,
}

pub trait WikiReadApi {
    fn target_api_url(&self) -> &str;
    fn get_all_pages(&mut self, namespace: i32) -> Result<Vec<String>>;
    fn get_category_members(&mut self, category: &str) -> Result<Vec<String>>;
    fn get_recent_changes(&mut self, since: &str, namespaces: &[i32]) -> Result<Vec<String>>;
    fn get_page_contents(&mut self, titles: &[String]) -> Result<Vec<RemotePage>>;
    /// Fetch exactly one historical revision using MediaWiki's `revids`
    /// query parameter and the main content slot.
    fn get_revision_by_id(&mut self, revision_id: i64) -> Result<Option<RemotePage>>;
    /// Scan the complete visible revision lineage for one title.
    fn get_revision_lineage(&mut self, title: &str) -> Result<Vec<RevisionLineageEntry>>;
    fn request_count(&self) -> usize;
}

pub trait WikiWriteApi: WikiReadApi {
    fn login(&mut self, username: &str, password: &str) -> Result<()>;
    fn get_page_timestamps(&mut self, titles: &[String]) -> Result<Vec<PageTimestampInfo>>;
    fn edit_page(
        &mut self,
        title: &str,
        content: &str,
        summary: &str,
        constraint: EditConstraint,
    ) -> Result<EditReceipt>;
    fn delete_page(&mut self, title: &str, reason: &str) -> Result<DeleteOutcome>;
    /// Scan the complete visible delete-log lineage for one exact title.
    fn get_delete_log_entries(&mut self, title: &str) -> Result<Vec<DeleteLogEntry>>;
}

#[derive(Debug)]
pub(crate) struct MediaWikiApiError {
    code: String,
    info: String,
}

impl MediaWikiApiError {
    pub(crate) fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for MediaWikiApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "MediaWiki API error [{}]: {}",
            self.code, self.info
        )
    }
}

impl Error for MediaWikiApiError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditConstraint {
    CreateOnly,
    ExistingRevision { revision_id: i64 },
}

#[derive(Debug, Clone)]
pub struct MediaWikiClientConfig {
    pub api_url: String,
    pub user_agent: String,
    pub timeout_ms: u64,
    pub rate_limit_read_ms: u64,
    pub rate_limit_write_ms: u64,
    pub max_retries: usize,
    pub retry_delay_ms: u64,
    /// Whether `action=edit` requests include MediaWiki's `bot` marker.
    pub mark_edits_as_bot: bool,
}

pub struct MediaWikiClient {
    pub(crate) client: Client,
    pub(crate) config: MediaWikiClientConfig,
    pub(crate) last_request_at: Option<Instant>,
    pub(crate) request_count: usize,
    pub(crate) csrf_token: Option<String>,
}

impl MediaWikiClient {
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

    pub fn api_url(&self) -> &str {
        &self.config.api_url
    }

    /// Fetch a non-API text resource using the same bounded read policy as API
    /// requests. This is used for generic MediaWiki surfaces such as
    /// `Special:Version`; mutations are never routed through this method.
    pub fn request_text_get(&mut self, url: &str) -> Result<String> {
        let url = Url::parse(url).with_context(|| format!("invalid URL: {url}"))?;
        for attempt in 0..=self.config.max_retries {
            self.apply_rate_limit(false);
            let response = self
                .client
                .get(url.clone())
                .header("User-Agent", self.config.user_agent.clone())
                .send();

            match response {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        if attempt < self.config.max_retries && is_retryable_status(status) {
                            self.wait_before_retry(attempt, false);
                            continue;
                        }
                        bail!("HTTP request failed with status {status}");
                    }
                    return response.text().context("failed to read HTTP response body");
                }
                Err(error) => {
                    if attempt < self.config.max_retries && is_retryable_error(&error) {
                        self.wait_before_retry(attempt, false);
                        continue;
                    }
                    return Err(error).context("failed to call HTTP endpoint");
                }
            }
        }

        bail!("HTTP request exhausted retry budget")
    }

    /// Execute a read-only MediaWiki API query and decode its JSON response.
    /// Reads use the configured bounded retry policy; mutation helpers do not.
    pub fn query_json(&mut self, params: &[(&str, String)]) -> Result<Value> {
        let base_url = Url::parse(&self.config.api_url)
            .with_context(|| format!("invalid MediaWiki API URL: {}", self.config.api_url))?;

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
                        return Err(MediaWikiApiError {
                            code: code.to_string(),
                            info: info.to_string(),
                        }
                        .into());
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
        // A timeout or 5xx after a mutation leaves the outcome ambiguous. Never
        // replay writes automatically; callers must reconcile remote state first.
        let max_retries = if is_write { 0 } else { self.config.max_retries };
        let pairs = post_form_pairs(params);

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
                        return Err(MediaWikiApiError {
                            code: code.to_string(),
                            info: info.to_string(),
                        }
                        .into());
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

fn post_form_pairs(params: &[(&str, String)]) -> Vec<(String, String)> {
    let mut pairs = Vec::with_capacity(params.len() + 2);
    pairs.push(("format".to_string(), "json".to_string()));
    pairs.push(("formatversion".to_string(), "2".to_string()));
    pairs.extend(
        params
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone())),
    );
    pairs
}

#[cfg(test)]
mod tests {
    use super::post_form_pairs;

    #[test]
    fn post_form_preserves_explicit_empty_values() {
        let pairs = post_form_pairs(&[("action", "edit".to_string()), ("text", String::new())]);
        assert!(pairs.contains(&("text".to_string(), String::new())));
    }
}
