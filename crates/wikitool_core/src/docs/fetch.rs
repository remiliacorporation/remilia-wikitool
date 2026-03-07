use std::collections::BTreeSet;
use std::env;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::WikiConfig;
use crate::support::{env_value, env_value_u64, env_value_usize};

const DEFAULT_DOCS_API_URL: &str = "https://www.mediawiki.org/w/api.php";
const DEFAULT_USER_AGENT: &str = crate::config::DEFAULT_USER_AGENT;

#[derive(Debug, Clone, Serialize)]
pub struct RemoteDocsPage {
    pub requested_title: String,
    pub title: String,
    pub timestamp: String,
    pub content: String,
}

pub trait DocsApi {
    fn get_subpages(&mut self, prefix: &str, namespace: i32, limit: usize) -> Result<Vec<String>>;
    fn get_page(&mut self, title: &str) -> Result<Option<RemoteDocsPage>>;
    fn request_count(&self) -> usize;
}

#[derive(Debug, Clone)]
pub struct DocsClientConfig {
    pub api_url: String,
    pub user_agent: String,
    pub timeout_ms: u64,
    pub rate_limit_ms: u64,
    pub max_retries: usize,
    pub retry_delay_ms: u64,
}

impl DocsClientConfig {
    pub fn from_env() -> Self {
        Self {
            api_url: env_value("WIKITOOL_DOCS_API_URL", DEFAULT_DOCS_API_URL),
            user_agent: env_value("WIKITOOL_DOCS_USER_AGENT", DEFAULT_USER_AGENT),
            timeout_ms: env_value_u64("WIKITOOL_DOCS_TIMEOUT_MS", 30_000),
            rate_limit_ms: env_value_u64("WIKITOOL_DOCS_RATE_LIMIT_MS", 300),
            max_retries: env_value_usize("WIKITOOL_DOCS_RETRIES", 2),
            retry_delay_ms: env_value_u64("WIKITOOL_DOCS_RETRY_DELAY_MS", 500),
        }
    }
}

pub struct MediaWikiDocsClient {
    client: Client,
    config: DocsClientConfig,
    last_request_at: Option<Instant>,
    request_count: usize,
}

impl MediaWikiDocsClient {
    pub fn from_env() -> Result<Self> {
        Self::new(DocsClientConfig::from_env())
    }

    pub fn new(config: DocsClientConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .context("failed to build docs HTTP client")?;

        Ok(Self {
            client,
            config,
            last_request_at: None,
            request_count: 0,
        })
    }

    fn request_json_get(&mut self, params: &[(&str, String)]) -> Result<Value> {
        let base_url = Url::parse(&self.config.api_url)
            .with_context(|| format!("invalid docs API URL: {}", self.config.api_url))?;

        let mut pairs = Vec::with_capacity(params.len() + 2);
        pairs.push(("format".to_string(), "json".to_string()));
        pairs.push(("formatversion".to_string(), "2".to_string()));
        for (key, value) in params {
            if !value.is_empty() {
                pairs.push(((*key).to_string(), value.clone()));
            }
        }

        for attempt in 0..=self.config.max_retries {
            self.apply_rate_limit();
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
                            self.wait_before_retry(attempt);
                            continue;
                        }
                        bail!("docs API request failed with HTTP {status}");
                    }
                    let payload: Value = response
                        .json()
                        .context("failed to decode docs API JSON response")?;
                    if let Some(error) = payload.get("error") {
                        let code = error
                            .get("code")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown_error");
                        let info = error
                            .get("info")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown info");
                        bail!("docs API error [{code}]: {info}");
                    }
                    return Ok(payload);
                }
                Err(error) => {
                    if attempt < self.config.max_retries && is_retryable_error(&error) {
                        self.wait_before_retry(attempt);
                        continue;
                    }
                    return Err(error).context("failed to call docs API");
                }
            }
        }

        bail!("docs API request exhausted retry budget")
    }

    fn apply_rate_limit(&mut self) {
        if let Some(last) = self.last_request_at {
            let elapsed = last.elapsed();
            let required = Duration::from_millis(self.config.rate_limit_ms);
            if elapsed < required {
                sleep(required - elapsed);
            }
        }
        self.last_request_at = Some(Instant::now());
        self.request_count = self.request_count.saturating_add(1);
    }

    fn wait_before_retry(&self, attempt: usize) {
        let exponent = u32::try_from(attempt).unwrap_or(8).min(8);
        let scale = 1u64.checked_shl(exponent).unwrap_or(256);
        let base = self.config.retry_delay_ms.saturating_mul(scale);
        let jitter = (u64::try_from(attempt).unwrap_or(0) * 17 + 31) % 97;
        sleep(Duration::from_millis(base.saturating_add(jitter)));
    }
}

impl DocsApi for MediaWikiDocsClient {
    fn get_subpages(&mut self, prefix: &str, namespace: i32, limit: usize) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let mut continue_token: Option<String> = None;
        let limit = limit.max(1);

        loop {
            if out.len() >= limit {
                break;
            }

            let remaining = limit - out.len();
            let batch_limit = remaining.min(500);
            let mut params = vec![
                ("action", "query".to_string()),
                ("list", "allpages".to_string()),
                ("apprefix", prefix.to_string()),
                ("apnamespace", namespace.to_string()),
                ("aplimit", batch_limit.to_string()),
            ];
            if let Some(token) = &continue_token {
                params.push(("apcontinue", token.clone()));
            }
            let value = self.request_json_get(&params)?;
            let parsed: QueryResponse =
                serde_json::from_value(value).context("failed to parse allpages response")?;

            for row in parsed.query.allpages {
                out.push(row.title);
                if out.len() >= limit {
                    break;
                }
            }

            continue_token = parsed.continuation.and_then(|next| next.apcontinue);
            if continue_token.is_none() {
                break;
            }
        }

        Ok(out)
    }

    fn get_page(&mut self, title: &str) -> Result<Option<RemoteDocsPage>> {
        let params = vec![
            ("action", "query".to_string()),
            ("titles", title.to_string()),
            ("redirects", "1".to_string()),
            ("prop", "revisions".to_string()),
            ("rvprop", "content|timestamp".to_string()),
            ("rvslots", "main".to_string()),
        ];
        let value = self.request_json_get(&params)?;
        let parsed: QueryResponse =
            serde_json::from_value(value).context("failed to parse page response")?;
        let Some(page) = parsed.query.pages.into_iter().next() else {
            return Ok(None);
        };
        if page.missing.unwrap_or(false) {
            return Ok(None);
        }
        let Some(revision) = page.revisions.into_iter().next() else {
            return Ok(None);
        };
        let content = revision
            .slots
            .main
            .content
            .or(revision.content)
            .unwrap_or_default();
        if content.is_empty() {
            return Ok(None);
        }
        Ok(Some(RemoteDocsPage {
            requested_title: title.to_string(),
            title: page.title,
            timestamp: revision.timestamp.unwrap_or_default(),
            content,
        }))
    }

    fn request_count(&self) -> usize {
        self.request_count
    }
}

pub fn discover_installed_extensions_from_wiki() -> Result<Vec<String>> {
    discover_installed_extensions_from_wiki_with_config(&WikiConfig::default())
}

pub fn discover_installed_extensions_from_wiki_with_config(
    config: &WikiConfig,
) -> Result<Vec<String>> {
    let api_url = env::var("WIKITOOL_INSTALLED_EXTENSIONS_API_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| config.api_url_owned())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "wiki API URL is not configured (set [wiki].api_url, WIKI_API_URL, or WIKITOOL_INSTALLED_EXTENSIONS_API_URL)"
            )
        })?;
    let user_agent = env_value("WIKITOOL_DOCS_USER_AGENT", DEFAULT_USER_AGENT);
    let timeout_ms = env_value_u64("WIKITOOL_DOCS_TIMEOUT_MS", 30_000);
    let max_retries = env_value_usize("WIKITOOL_DOCS_RETRIES", 2);
    let retry_delay_ms = env_value_u64("WIKITOOL_DOCS_RETRY_DELAY_MS", 500);

    let client = Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .context("failed to build installed-extensions HTTP client")?;

    for attempt in 0..=max_retries {
        let base_url = Url::parse(&api_url)
            .with_context(|| format!("invalid installed extensions API URL: {api_url}"))?;
        let response = client
            .get(base_url.clone())
            .header("User-Agent", user_agent.clone())
            .query(&[
                ("action", "query"),
                ("meta", "siteinfo"),
                ("siprop", "extensions"),
                ("format", "json"),
                ("formatversion", "2"),
            ])
            .send();

        match response {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    if attempt < max_retries && is_retryable_status(status) {
                        wait_retry_delay(retry_delay_ms, attempt);
                        continue;
                    }
                    bail!("installed-extensions request failed with HTTP {status}");
                }

                let payload: SiteInfoResponse = response
                    .json()
                    .context("failed to parse installed-extensions response")?;
                let mut extensions = BTreeSet::new();
                for item in payload.query.extensions {
                    let name = normalize_extension_name(&item.name);
                    if !name.is_empty() {
                        extensions.insert(name);
                    }
                }
                return Ok(extensions.into_iter().collect());
            }
            Err(error) => {
                if attempt < max_retries && is_retryable_error(&error) {
                    wait_retry_delay(retry_delay_ms, attempt);
                    continue;
                }
                return Err(error).context("failed to query installed extensions from wiki");
            }
        }
    }

    bail!("installed-extensions request exhausted retry budget")
}

fn normalize_extension_name(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("Extension:")
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn wait_retry_delay(retry_delay_ms: u64, attempt: usize) {
    let exponent = u32::try_from(attempt).unwrap_or(8).min(8);
    let scale = 1u64.checked_shl(exponent).unwrap_or(256);
    let base = retry_delay_ms.saturating_mul(scale);
    let jitter = (u64::try_from(attempt).unwrap_or(0) * 17 + 31) % 97;
    sleep(Duration::from_millis(base.saturating_add(jitter)));
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

#[derive(Debug, Deserialize, Default)]
struct SiteInfoResponse {
    #[serde(default)]
    query: SiteInfoQuery,
}

#[derive(Debug, Deserialize, Default)]
struct SiteInfoQuery {
    #[serde(default)]
    extensions: Vec<SiteInfoExtension>,
}

#[derive(Debug, Deserialize, Default)]
struct SiteInfoExtension {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct QueryResponse {
    #[serde(default)]
    query: QueryPayload,
    #[serde(default, rename = "continue")]
    continuation: Option<ContinuationPayload>,
}

#[derive(Debug, Deserialize, Default)]
struct QueryPayload {
    #[serde(default)]
    allpages: Vec<TitleQueryItem>,
    #[serde(default)]
    pages: Vec<PageQueryItem>,
}

#[derive(Debug, Deserialize, Default)]
struct ContinuationPayload {
    apcontinue: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct TitleQueryItem {
    title: String,
}

#[derive(Debug, Deserialize, Default)]
struct PageQueryItem {
    #[serde(default)]
    title: String,
    #[serde(default)]
    missing: Option<bool>,
    #[serde(default)]
    revisions: Vec<PageRevisionItem>,
}

#[derive(Debug, Deserialize, Default)]
struct PageRevisionItem {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    slots: RevisionSlots,
}

#[derive(Debug, Deserialize, Default)]
struct RevisionSlots {
    #[serde(default)]
    main: MainSlot,
}

#[derive(Debug, Deserialize, Default)]
struct MainSlot {
    #[serde(default)]
    content: Option<String>,
}
