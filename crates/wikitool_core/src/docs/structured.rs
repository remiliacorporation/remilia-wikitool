use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::{StatusCode, Url};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{compute_hash, env_value, env_value_u64, env_value_usize, unix_timestamp};

use super::parse::{
    DocsPageParseInput, ParsedDocsPage, collapse_whitespace, estimate_token_count,
    is_translation_variant, make_summary_text, normalize_title, parse_docs_page, truncate_text,
};

const DEFAULT_DOCS_API_URL: &str = "https://www.mediawiki.org/w/api.php";
const DEFAULT_USER_AGENT: &str = crate::config::DEFAULT_USER_AGENT;
const DOCS_CACHE_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
const DOCS_SUBPAGE_LIMIT_DEFAULT: usize = 100;
const DOCS_BUNDLE_SCHEMA_VERSION: u32 = 1;
const DOCS_NAMESPACE_HELP: i32 = 12;
const DOCS_NAMESPACE_MANUAL: i32 = 100;
const DOCS_NAMESPACE_EXTENSION: i32 = 102;
const DOCS_NAMESPACE_API: i32 = 104;
const MEDIAWIKI_VERSION_DEFAULT: &str = "1.44";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TechnicalDocType {
    Hooks,
    Config,
    Api,
    Manual,
}

impl TechnicalDocType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hooks => "hooks",
            Self::Config => "config",
            Self::Api => "api",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("hooks") {
            return Some(Self::Hooks);
        }
        if value.eq_ignore_ascii_case("config") {
            return Some(Self::Config);
        }
        if value.eq_ignore_ascii_case("api") {
            return Some(Self::Api);
        }
        if value.eq_ignore_ascii_case("manual") {
            return Some(Self::Manual);
        }
        None
    }

    fn main_page(self) -> &'static str {
        match self {
            Self::Hooks => "Manual:Hooks",
            Self::Config => "Manual:Configuration settings",
            Self::Api => "API:Main page",
            Self::Manual => "Manual:Contents",
        }
    }

    fn subpage_query(self) -> Option<(&'static str, i32)> {
        match self {
            Self::Hooks => Some(("Hooks/", DOCS_NAMESPACE_MANUAL)),
            Self::Config => Some(("$wg", DOCS_NAMESPACE_MANUAL)),
            Self::Api => Some(("", DOCS_NAMESPACE_API)),
            Self::Manual => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DocsImportOptions {
    pub extensions: Vec<String>,
    pub include_subpages: bool,
}

impl Default for DocsImportOptions {
    fn default() -> Self {
        Self {
            extensions: Vec::new(),
            include_subpages: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsImportReport {
    pub requested_extensions: usize,
    pub imported_extensions: usize,
    pub imported_pages: usize,
    pub imported_sections: usize,
    pub imported_symbols: usize,
    pub imported_examples: usize,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalImportTask {
    pub doc_type: TechnicalDocType,
    pub page_title: Option<String>,
    pub include_subpages: bool,
}

#[derive(Debug, Clone)]
pub struct DocsImportTechnicalOptions {
    pub tasks: Vec<TechnicalImportTask>,
    pub limit: usize,
}

impl Default for DocsImportTechnicalOptions {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            limit: DOCS_SUBPAGE_LIMIT_DEFAULT,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsImportTechnicalReport {
    pub requested_tasks: usize,
    pub imported_pages: usize,
    pub imported_sections: usize,
    pub imported_symbols: usize,
    pub imported_examples: usize,
    pub imported_by_type: BTreeMap<String, usize>,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone)]
pub struct DocsImportMediaWikiOptions {
    pub mw_version: String,
    pub hooks: bool,
    pub config: bool,
    pub api: bool,
    pub manual: bool,
    pub parser: bool,
    pub tags: bool,
    pub lua: bool,
    pub limit: usize,
}

impl Default for DocsImportMediaWikiOptions {
    fn default() -> Self {
        Self {
            mw_version: MEDIAWIKI_VERSION_DEFAULT.to_string(),
            hooks: true,
            config: true,
            api: true,
            manual: true,
            parser: true,
            tags: true,
            lua: true,
            limit: DOCS_SUBPAGE_LIMIT_DEFAULT,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsImportMediaWikiReport {
    pub mw_version: String,
    pub imported_corpora: usize,
    pub imported_pages: usize,
    pub imported_sections: usize,
    pub imported_symbols: usize,
    pub imported_examples: usize,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsStats {
    pub corpus_count: usize,
    pub page_count: usize,
    pub section_count: usize,
    pub symbol_count: usize,
    pub example_count: usize,
    pub extension_count: usize,
    pub extension_pages_count: usize,
    pub technical_count: usize,
    pub technical_by_type: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsCorpusSummary {
    pub corpus_id: String,
    pub corpus_kind: String,
    pub label: String,
    pub source_wiki: String,
    pub source_version: String,
    pub source_profile: String,
    pub technical_type: String,
    pub pages_count: usize,
    pub sections_count: usize,
    pub symbols_count: usize,
    pub examples_count: usize,
    pub fetched_at_unix: u64,
    pub expires_at_unix: u64,
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtensionDocSummary {
    pub extension_name: String,
    pub source_wiki: String,
    pub version: Option<String>,
    pub pages_count: usize,
    pub sections_count: usize,
    pub symbols_count: usize,
    pub examples_count: usize,
    pub fetched_at_unix: u64,
    pub expires_at_unix: u64,
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TechnicalDocSummary {
    pub doc_type: String,
    pub page_title: String,
    pub local_path: String,
    pub fetched_at_unix: u64,
    pub expires_at_unix: u64,
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutdatedExtensionDoc {
    pub extension_name: String,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutdatedTechnicalDoc {
    pub doc_type: String,
    pub page_title: String,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsOutdatedReport {
    pub extensions: Vec<OutdatedExtensionDoc>,
    pub technical: Vec<OutdatedTechnicalDoc>,
}

#[derive(Debug, Clone, Default)]
pub struct DocsListOptions {
    pub technical_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsListReport {
    pub now_unix: u64,
    pub stats: DocsStats,
    pub corpora: Vec<DocsCorpusSummary>,
    pub extensions: Vec<ExtensionDocSummary>,
    pub technical: Vec<TechnicalDocSummary>,
    pub outdated: DocsOutdatedReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsUpdateReport {
    pub updated_extensions: usize,
    pub updated_technical_types: usize,
    pub updated_corpora: usize,
    pub updated_pages: usize,
    pub updated_sections: usize,
    pub updated_symbols: usize,
    pub updated_examples: usize,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocsRemoveKind {
    Extension,
    TechnicalType,
    TechnicalPage,
    Corpus,
    Profile,
    NotFound,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsRemoveReport {
    pub kind: DocsRemoveKind,
    pub target: String,
    pub removed_rows: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsSearchHit {
    pub tier: String,
    pub corpus_id: String,
    pub corpus_kind: String,
    pub corpus_label: String,
    pub title: String,
    pub page_title: String,
    pub section_heading: Option<String>,
    pub detail_kind: String,
    pub retrieval_reason: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct DocsContextOptions {
    pub source_profile: Option<String>,
    pub source_version: Option<String>,
    pub max_pages: usize,
    pub max_sections: usize,
    pub max_examples: usize,
    pub max_symbols: usize,
    pub token_budget: usize,
}

impl Default for DocsContextOptions {
    fn default() -> Self {
        Self {
            source_profile: None,
            source_version: None,
            max_pages: 4,
            max_sections: 8,
            max_examples: 4,
            max_symbols: 8,
            token_budget: 1600,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsContextPage {
    pub corpus_label: String,
    pub page_title: String,
    pub doc_type: String,
    pub summary_text: String,
    pub retrieval_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsContextSection {
    pub page_title: String,
    pub section_heading: Option<String>,
    pub summary_text: String,
    pub section_text: String,
    pub retrieval_reason: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsContextSymbol {
    pub page_title: String,
    pub section_heading: Option<String>,
    pub symbol_kind: String,
    pub symbol_name: String,
    pub aliases: Vec<String>,
    pub summary_text: String,
    pub retrieval_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsContextExample {
    pub page_title: String,
    pub section_heading: Option<String>,
    pub example_kind: String,
    pub language_hint: Option<String>,
    pub summary_text: String,
    pub example_text: String,
    pub retrieval_reason: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsContextReport {
    pub query: String,
    pub source_profile: Option<String>,
    pub source_version: Option<String>,
    pub pages: Vec<DocsContextPage>,
    pub sections: Vec<DocsContextSection>,
    pub symbols: Vec<DocsContextSymbol>,
    pub examples: Vec<DocsContextExample>,
    pub related_pages: Vec<String>,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocsBundle {
    pub schema_version: u32,
    pub generated_at_unix: Option<u64>,
    pub source: Option<String>,
    #[serde(default)]
    pub extensions: Vec<DocsBundleExtension>,
    #[serde(default)]
    pub technical: Vec<DocsBundleTechnical>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocsBundleExtension {
    pub extension_name: String,
    pub source_wiki: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub pages: Vec<DocsBundlePage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocsBundleTechnical {
    pub doc_type: String,
    #[serde(default)]
    pub pages: Vec<DocsBundlePage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocsBundlePage {
    pub page_title: String,
    pub content: String,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsBundleImportReport {
    pub schema_version: u32,
    pub source: String,
    pub imported_extensions: usize,
    pub imported_technical_types: usize,
    pub imported_pages: usize,
    pub imported_sections: usize,
    pub imported_symbols: usize,
    pub imported_examples: usize,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteDocsPage {
    pub title: String,
    pub timestamp: String,
    pub revision_id: Option<i64>,
    pub parent_revision_id: Option<i64>,
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

#[derive(Debug, Clone)]
struct CorpusMaterialization {
    corpus_id: String,
    corpus_kind: String,
    label: String,
    source_wiki: String,
    source_version: String,
    source_profile: String,
    technical_type: String,
    refresh_kind: String,
    refresh_spec: String,
    fetched_at_unix: u64,
    expires_at_unix: u64,
    pages: Vec<ParsedDocsPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtensionRefreshSpec {
    extension_name: String,
    include_subpages: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TechnicalRefreshSpec {
    doc_type: String,
    page_title: Option<String>,
    include_subpages: bool,
    limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MediaWikiRefreshSpec {
    mw_version: String,
    family: String,
    limit: usize,
}

#[derive(Debug, Clone)]
struct SearchCandidate {
    weight: i64,
    hit: DocsSearchHit,
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
    #[serde(default, rename = "revid")]
    revision_id: Option<i64>,
    #[serde(default, rename = "parentid")]
    parent_revision_id: Option<i64>,
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
            ("prop", "revisions".to_string()),
            ("rvprop", "ids|content|timestamp".to_string()),
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
            title: page.title,
            timestamp: revision.timestamp.unwrap_or_default(),
            revision_id: revision.revision_id,
            parent_revision_id: revision.parent_revision_id,
            content,
        }))
    }

    fn request_count(&self) -> usize {
        self.request_count
    }
}

pub fn discover_installed_extensions_from_wiki() -> Result<Vec<String>> {
    discover_installed_extensions_from_wiki_with_config(&crate::config::WikiConfig::default())
}

pub fn discover_installed_extensions_from_wiki_with_config(
    config: &crate::config::WikiConfig,
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

pub fn import_docs_bundle(
    paths: &ResolvedPaths,
    bundle_path: &Path,
) -> Result<DocsBundleImportReport> {
    let bundle_data = fs::read_to_string(bundle_path)
        .with_context(|| format!("failed to read docs bundle {}", bundle_path.display()))?;
    let bundle: DocsBundle =
        serde_json::from_str(&bundle_data).context("failed to parse docs bundle JSON")?;
    if bundle.schema_version != DOCS_BUNDLE_SCHEMA_VERSION {
        bail!(
            "unsupported docs bundle schema version {} (expected {})",
            bundle.schema_version,
            DOCS_BUNDLE_SCHEMA_VERSION
        );
    }

    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let source = bundle
        .source
        .clone()
        .unwrap_or_else(|| "precomposed_bundle".to_string());

    let mut imported_extensions = 0usize;
    let mut imported_technical_types = 0usize;
    let mut imported_pages = 0usize;
    let mut imported_sections = 0usize;
    let mut imported_symbols = 0usize;
    let mut imported_examples = 0usize;
    let mut failures = Vec::new();

    for extension in &bundle.extensions {
        let extension_name = normalize_extension_name(&extension.extension_name);
        if extension_name.is_empty() {
            failures.push("bundle extension entry with empty extension_name".to_string());
            continue;
        }
        let pages = extension
            .pages
            .iter()
            .filter_map(|page| {
                let page_title = normalize_title(&page.page_title);
                if page_title.is_empty() || page.content.trim().is_empty() {
                    return None;
                }
                Some(BundlePageInput {
                    page_title,
                    local_path: page
                        .local_path
                        .clone()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| extension_local_path(&extension_name, &page.page_title)),
                    content: page.content.clone(),
                })
            })
            .collect::<Vec<_>>();
        if pages.is_empty() {
            failures.push(format!("{extension_name}: bundle entry has no usable pages"));
            continue;
        }

        let corpus = materialize_docs_corpus(
            CorpusMaterializationInput {
                corpus_id: format!("extension:{}", normalize_identifier(&extension_name)),
                corpus_kind: "extension".to_string(),
                label: extension_name.clone(),
                source_wiki: extension
                    .source_wiki
                    .clone()
                    .unwrap_or_else(|| source.clone()),
                source_version: extension
                    .version
                    .clone()
                    .unwrap_or_else(|| "bundle".to_string()),
                source_profile: "bundle/extensions".to_string(),
                technical_type: String::new(),
                refresh_kind: "bundle".to_string(),
                refresh_spec: "{}".to_string(),
                fetched_at_unix: now_unix,
                expires_at_unix,
                pages,
            },
            None,
        )?;
        imported_pages += corpus.pages.len();
        imported_sections += corpus.pages.iter().map(|page| page.sections.len()).sum::<usize>();
        imported_symbols += corpus.pages.iter().map(|page| page.symbols.len()).sum::<usize>();
        imported_examples += corpus.pages.iter().map(|page| page.examples.len()).sum::<usize>();
        persist_docs_corpus(paths, &corpus)?;
        imported_extensions += 1;
    }

    for technical in &bundle.technical {
        let Some(doc_type) = TechnicalDocType::parse(&technical.doc_type) else {
            failures.push(format!(
                "bundle technical entry has unsupported doc_type `{}`",
                technical.doc_type
            ));
            continue;
        };
        let pages = technical
            .pages
            .iter()
            .filter_map(|page| {
                let page_title = normalize_title(&page.page_title);
                if page_title.is_empty() || page.content.trim().is_empty() {
                    return None;
                }
                Some(BundlePageInput {
                    page_title,
                    local_path: page
                        .local_path
                        .clone()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| technical_local_path(doc_type, &page.page_title)),
                    content: page.content.clone(),
                })
            })
            .collect::<Vec<_>>();
        if pages.is_empty() {
            failures.push(format!(
                "{}: bundle entry has no usable pages",
                doc_type.as_str()
            ));
            continue;
        }
        let corpus = materialize_docs_corpus(
            CorpusMaterializationInput {
                corpus_id: format!(
                    "technical:{}",
                    normalize_identifier(&format!("{}-bundle", doc_type.as_str()))
                ),
                corpus_kind: "technical".to_string(),
                label: format!("{} bundle docs", doc_type.as_str()),
                source_wiki: source.clone(),
                source_version: "bundle".to_string(),
                source_profile: "bundle/technical".to_string(),
                technical_type: doc_type.as_str().to_string(),
                refresh_kind: "bundle".to_string(),
                refresh_spec: "{}".to_string(),
                fetched_at_unix: now_unix,
                expires_at_unix,
                pages,
            },
            None,
        )?;
        imported_pages += corpus.pages.len();
        imported_sections += corpus.pages.iter().map(|page| page.sections.len()).sum::<usize>();
        imported_symbols += corpus.pages.iter().map(|page| page.symbols.len()).sum::<usize>();
        imported_examples += corpus.pages.iter().map(|page| page.examples.len()).sum::<usize>();
        persist_docs_corpus(paths, &corpus)?;
        imported_technical_types += 1;
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsBundleImportReport {
        schema_version: bundle.schema_version,
        source,
        imported_extensions,
        imported_technical_types,
        imported_pages,
        imported_sections,
        imported_symbols,
        imported_examples,
        failures,
    })
}

pub fn import_extension_docs(
    paths: &ResolvedPaths,
    options: &DocsImportOptions,
) -> Result<DocsImportReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    import_extension_docs_with_api(paths, options, &mut api)
}

pub fn import_extension_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportOptions,
    api: &mut A,
) -> Result<DocsImportReport> {
    if options.extensions.is_empty() {
        bail!("no extensions specified for docs import");
    }

    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let requested_extensions = options.extensions.len();
    let mut imported_extensions = 0usize;
    let mut imported_pages = 0usize;
    let mut imported_sections = 0usize;
    let mut imported_symbols = 0usize;
    let mut imported_examples = 0usize;
    let mut failures = Vec::new();

    for extension in normalize_extensions(&options.extensions) {
        let page_titles = collect_extension_page_titles(api, &extension, options.include_subpages)
            .with_context(|| format!("failed to collect pages for extension {extension}"));
        let page_titles = match page_titles {
            Ok(titles) => titles,
            Err(error) => {
                failures.push(error.to_string());
                continue;
            }
        };
        let fetched_pages = fetch_remote_pages(api, &page_titles, |title| {
            extension_local_path(&extension, title)
        })?;
        if fetched_pages.failures.is_empty() && !fetched_pages.pages.is_empty() {
            let version = fetched_pages
                .pages
                .iter()
                .find(|page| page.page_title.eq_ignore_ascii_case(&format!("Extension:{extension}")))
                .and_then(|page| extract_extension_version(&page.content));
            let corpus = materialize_docs_corpus(
                CorpusMaterializationInput {
                    corpus_id: format!("extension:{}", normalize_identifier(&extension)),
                    corpus_kind: "extension".to_string(),
                    label: extension.clone(),
                    source_wiki: "mediawiki.org".to_string(),
                    source_version: version.clone().unwrap_or_else(|| "live".to_string()),
                    source_profile: "extensions".to_string(),
                    technical_type: String::new(),
                    refresh_kind: "extension".to_string(),
                    refresh_spec: serde_json::to_string(&ExtensionRefreshSpec {
                        extension_name: extension.clone(),
                        include_subpages: options.include_subpages,
                    })?,
                    fetched_at_unix: now_unix,
                    expires_at_unix,
                    pages: fetched_pages
                        .pages
                        .into_iter()
                        .map(|page| BundlePageInput {
                            page_title: page.page_title,
                            local_path: page.local_path,
                            content: page.content,
                        })
                        .collect(),
                },
                fetched_pages.metadata.as_ref(),
            )?;
            imported_pages += corpus.pages.len();
            imported_sections += corpus.pages.iter().map(|page| page.sections.len()).sum::<usize>();
            imported_symbols += corpus.pages.iter().map(|page| page.symbols.len()).sum::<usize>();
            imported_examples += corpus.pages.iter().map(|page| page.examples.len()).sum::<usize>();
            persist_docs_corpus(paths, &corpus)?;
            imported_extensions += 1;
        } else if fetched_pages.pages.is_empty() {
            failures.push(format!("{extension}: no pages fetched"));
        }
        failures.extend(fetched_pages.failures);
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsImportReport {
        requested_extensions,
        imported_extensions,
        imported_pages,
        imported_sections,
        imported_symbols,
        imported_examples,
        failures,
        request_count: api.request_count(),
    })
}

pub fn import_technical_docs(
    paths: &ResolvedPaths,
    options: &DocsImportTechnicalOptions,
) -> Result<DocsImportTechnicalReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    import_technical_docs_with_api(paths, options, &mut api)
}

pub fn import_technical_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportTechnicalOptions,
    api: &mut A,
) -> Result<DocsImportTechnicalReport> {
    if options.tasks.is_empty() {
        bail!("no technical docs tasks specified");
    }

    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let mut imported_pages = 0usize;
    let mut imported_sections = 0usize;
    let mut imported_symbols = 0usize;
    let mut imported_examples = 0usize;
    let mut imported_by_type = BTreeMap::new();
    let mut failures = Vec::new();

    for task in &options.tasks {
        let page_titles = collect_technical_page_titles(api, task, options.limit.max(1))
            .with_context(|| format!("failed to collect {:?} docs", task.doc_type));
        let page_titles = match page_titles {
            Ok(titles) => titles,
            Err(error) => {
                failures.push(error.to_string());
                continue;
            }
        };
        let fetched_pages = fetch_remote_pages(api, &page_titles, |title| {
            technical_local_path(task.doc_type, title)
        })?;
        if !fetched_pages.failures.is_empty() {
            failures.extend(fetched_pages.failures);
            continue;
        }
        if fetched_pages.pages.is_empty() {
            failures.push(format!("{}: no pages fetched for task", task.doc_type.as_str()));
            continue;
        }
        let label = task
            .page_title
            .clone()
            .unwrap_or_else(|| task.doc_type.main_page().to_string());
        let corpus_id = if let Some(page_title) = &task.page_title {
            format!(
                "technical:{}",
                normalize_identifier(&format!("{}:{}", task.doc_type.as_str(), page_title))
            )
        } else {
            format!("technical:{}", normalize_identifier(task.doc_type.as_str()))
        };
        let corpus = materialize_docs_corpus(
            CorpusMaterializationInput {
                corpus_id,
                corpus_kind: "technical".to_string(),
                label,
                source_wiki: "mediawiki.org".to_string(),
                source_version: "live".to_string(),
                source_profile: "technical".to_string(),
                technical_type: task.doc_type.as_str().to_string(),
                refresh_kind: "technical".to_string(),
                refresh_spec: serde_json::to_string(&TechnicalRefreshSpec {
                    doc_type: task.doc_type.as_str().to_string(),
                    page_title: task.page_title.clone(),
                    include_subpages: task.include_subpages,
                    limit: options.limit.max(1),
                })?,
                fetched_at_unix: now_unix,
                expires_at_unix,
                pages: fetched_pages
                    .pages
                    .into_iter()
                    .map(|page| BundlePageInput {
                        page_title: page.page_title,
                        local_path: page.local_path,
                        content: page.content,
                    })
                    .collect(),
            },
            fetched_pages.metadata.as_ref(),
        )?;
        imported_pages += corpus.pages.len();
        imported_sections += corpus.pages.iter().map(|page| page.sections.len()).sum::<usize>();
        imported_symbols += corpus.pages.iter().map(|page| page.symbols.len()).sum::<usize>();
        imported_examples += corpus.pages.iter().map(|page| page.examples.len()).sum::<usize>();
        *imported_by_type
            .entry(task.doc_type.as_str().to_string())
            .or_insert(0) += corpus.pages.len();
        persist_docs_corpus(paths, &corpus)?;
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsImportTechnicalReport {
        requested_tasks: options.tasks.len(),
        imported_pages,
        imported_sections,
        imported_symbols,
        imported_examples,
        imported_by_type,
        failures,
        request_count: api.request_count(),
    })
}

pub fn import_mediawiki_docs(
    paths: &ResolvedPaths,
    options: &DocsImportMediaWikiOptions,
) -> Result<DocsImportMediaWikiReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    import_mediawiki_docs_with_api(paths, options, &mut api)
}

pub fn import_mediawiki_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportMediaWikiOptions,
    api: &mut A,
) -> Result<DocsImportMediaWikiReport> {
    let normalized = normalize_mediawiki_options(options);
    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let mut imported_corpora = 0usize;
    let mut imported_pages = 0usize;
    let mut imported_sections = 0usize;
    let mut imported_symbols = 0usize;
    let mut imported_examples = 0usize;
    let mut failures = Vec::new();

    for family in selected_mediawiki_families(&normalized) {
        let page_titles = collect_mediawiki_family_page_titles(api, family, normalized.limit)
            .with_context(|| format!("failed to collect {family} docs"))?;
        let fetched_pages = fetch_remote_pages(api, &page_titles, |title| {
            mediawiki_profile_local_path(&normalized.mw_version, family, title)
        })?;
        if !fetched_pages.failures.is_empty() {
            failures.extend(fetched_pages.failures);
            continue;
        }
        if fetched_pages.pages.is_empty() {
            failures.push(format!("{family}: no pages fetched"));
            continue;
        }
        let corpus = materialize_docs_corpus(
            CorpusMaterializationInput {
                corpus_id: format!(
                    "mediawiki:{}:{}",
                    normalize_identifier(&normalized.mw_version),
                    normalize_identifier(family)
                ),
                corpus_kind: "mediawiki".to_string(),
                label: format!("MediaWiki {} {}", normalized.mw_version, family),
                source_wiki: "mediawiki.org".to_string(),
                source_version: normalized.mw_version.clone(),
                source_profile: format!("mw-{}-core", normalized.mw_version),
                technical_type: family.to_string(),
                refresh_kind: "mediawiki_family".to_string(),
                refresh_spec: serde_json::to_string(&MediaWikiRefreshSpec {
                    mw_version: normalized.mw_version.clone(),
                    family: family.to_string(),
                    limit: normalized.limit,
                })?,
                fetched_at_unix: now_unix,
                expires_at_unix,
                pages: fetched_pages
                    .pages
                    .into_iter()
                    .map(|page| BundlePageInput {
                        page_title: page.page_title,
                        local_path: page.local_path,
                        content: page.content,
                    })
                    .collect(),
            },
            fetched_pages.metadata.as_ref(),
        )?;
        imported_pages += corpus.pages.len();
        imported_sections += corpus.pages.iter().map(|page| page.sections.len()).sum::<usize>();
        imported_symbols += corpus.pages.iter().map(|page| page.symbols.len()).sum::<usize>();
        imported_examples += corpus.pages.iter().map(|page| page.examples.len()).sum::<usize>();
        persist_docs_corpus(paths, &corpus)?;
        imported_corpora += 1;
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsImportMediaWikiReport {
        mw_version: normalized.mw_version,
        imported_corpora,
        imported_pages,
        imported_sections,
        imported_symbols,
        imported_examples,
        failures,
        request_count: api.request_count(),
    })
}

pub fn list_docs(paths: &ResolvedPaths, options: &DocsListOptions) -> Result<DocsListReport> {
    let connection = open_docs_connection(paths)?;
    let now_unix = unix_timestamp()?;
    let stats = load_docs_stats(&connection)?;
    let corpora = load_docs_corpora(&connection, now_unix)?;
    let extensions = load_extension_docs(&connection, now_unix)?;
    let technical = load_technical_docs(&connection, options.technical_type.as_deref(), now_unix)?;
    let outdated = load_outdated_docs(&connection, now_unix)?;

    Ok(DocsListReport {
        now_unix,
        stats,
        corpora,
        extensions,
        technical,
        outdated,
    })
}

pub fn update_outdated_docs(paths: &ResolvedPaths) -> Result<DocsUpdateReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    update_outdated_docs_with_api(paths, &mut api)
}

pub fn update_outdated_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    api: &mut A,
) -> Result<DocsUpdateReport> {
    let connection = open_docs_connection(paths)?;
    let now_unix = unix_timestamp()?;
    let outdated = load_outdated_corpora(&connection, now_unix)?;
    if outdated.is_empty() {
        return Ok(DocsUpdateReport {
            updated_extensions: 0,
            updated_technical_types: 0,
            updated_corpora: 0,
            updated_pages: 0,
            updated_sections: 0,
            updated_symbols: 0,
            updated_examples: 0,
            failures: Vec::new(),
            request_count: api.request_count(),
        });
    }

    let mut updated_extensions = 0usize;
    let mut updated_technical_types = 0usize;
    let mut updated_corpora = 0usize;
    let mut updated_pages = 0usize;
    let mut updated_sections = 0usize;
    let mut updated_symbols = 0usize;
    let mut updated_examples = 0usize;
    let mut failures = Vec::new();

    for corpus in outdated {
        match refresh_corpus(paths, &corpus, api) {
            Ok(report) => {
                updated_corpora += 1;
                updated_pages += report.pages;
                updated_sections += report.sections;
                updated_symbols += report.symbols;
                updated_examples += report.examples;
                if corpus.corpus_kind == "extension" {
                    updated_extensions += 1;
                } else if !corpus.technical_type.is_empty() {
                    updated_technical_types += 1;
                }
            }
            Err(error) => failures.push(format!("{}: {error}", corpus.label)),
        }
    }

    Ok(DocsUpdateReport {
        updated_extensions,
        updated_technical_types,
        updated_corpora,
        updated_pages,
        updated_sections,
        updated_symbols,
        updated_examples,
        failures,
        request_count: api.request_count(),
    })
}

pub fn remove_docs(paths: &ResolvedPaths, target: &str) -> Result<DocsRemoveReport> {
    let mut connection = open_docs_connection(paths)?;
    let normalized_target = normalize_title(target);
    if normalized_target.is_empty() {
        bail!("documentation target cannot be empty");
    }

    if let Some(report) = try_remove_docs_corpus(&mut connection, &normalized_target)? {
        return Ok(report);
    }
    if let Some(report) = try_remove_docs_profile(&mut connection, &normalized_target)? {
        return Ok(report);
    }
    if let Some(report) = try_remove_docs_extension(&mut connection, &normalized_target)? {
        return Ok(report);
    }
    if let Some(report) = try_remove_docs_technical_type(&mut connection, &normalized_target)? {
        return Ok(report);
    }
    if let Some(report) = try_remove_docs_page(&mut connection, &normalized_target)? {
        return Ok(report);
    }

    Ok(DocsRemoveReport {
        kind: DocsRemoveKind::NotFound,
        target: target.to_string(),
        removed_rows: 0,
    })
}

pub fn search_docs(
    paths: &ResolvedPaths,
    query: &str,
    tier: Option<&str>,
    limit: usize,
) -> Result<Vec<DocsSearchHit>> {
    let connection = open_docs_connection(paths)?;
    search_docs_with_connection(&connection, query, tier, limit)
}

pub fn docs_context(
    paths: &ResolvedPaths,
    query: &str,
    options: &DocsContextOptions,
) -> Result<DocsContextReport> {
    let connection = open_docs_connection(paths)?;
    build_docs_context(&connection, query, options)
}

pub fn format_expiration(now_unix: u64, expires_at_unix: u64) -> String {
    if expires_at_unix <= now_unix {
        return "expired".to_string();
    }
    let delta = expires_at_unix - now_unix;
    let day = 24 * 60 * 60;
    let hour = 60 * 60;
    if delta >= day {
        let days = delta / day;
        return format!("expires in {days} day{}", if days == 1 { "" } else { "s" });
    }
    if delta >= hour {
        let hours = delta / hour;
        return format!(
            "expires in {hours} hour{}",
            if hours == 1 { "" } else { "s" }
        );
    }
    "expires soon".to_string()
}

#[derive(Debug, Clone)]
struct BundlePageInput {
    page_title: String,
    local_path: String,
    content: String,
}

#[derive(Debug, Clone)]
struct RemotePageInput {
    page_title: String,
    local_path: String,
    content: String,
    revision_id: Option<i64>,
    parent_revision_id: Option<i64>,
    timestamp: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct FetchRemotePagesReport {
    pages: Vec<RemotePageInput>,
    failures: Vec<String>,
    metadata: Option<RemotePageMetadata>,
}

#[derive(Debug, Clone, Default)]
struct RemotePageMetadata {
    revision_ids: BTreeMap<String, i64>,
    parent_revision_ids: BTreeMap<String, i64>,
    timestamps: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct CorpusMaterializationInput {
    corpus_id: String,
    corpus_kind: String,
    label: String,
    source_wiki: String,
    source_version: String,
    source_profile: String,
    technical_type: String,
    refresh_kind: String,
    refresh_spec: String,
    fetched_at_unix: u64,
    expires_at_unix: u64,
    pages: Vec<BundlePageInput>,
}

#[derive(Debug, Clone)]
struct RefreshResult {
    pages: usize,
    sections: usize,
    symbols: usize,
    examples: usize,
}

#[derive(Debug, Clone)]
struct OutdatedCorpusRow {
    corpus_id: String,
    corpus_kind: String,
    label: String,
    source_profile: String,
    technical_type: String,
    refresh_kind: String,
    refresh_spec: String,
}

fn normalize_extensions(values: &[String]) -> Vec<String> {
    let mut normalized = values
        .iter()
        .map(|value| normalize_extension_name(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort_unstable_by_key(|value| value.to_ascii_lowercase());
    normalized.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    normalized
}

fn normalize_extension_name(value: &str) -> String {
    let normalized = normalize_title(value);
    normalized.trim_start_matches("Extension:").trim().to_string()
}

fn normalize_identifier(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if !output.ends_with('-') {
            output.push('-');
        }
    }
    output.trim_matches('-').to_string()
}

fn extension_local_path(extension: &str, title: &str) -> String {
    format!(
        "docs/extensions/{}/{}.wiki",
        sanitize_path_segment(extension),
        sanitize_title_for_filename(title),
    )
}

fn technical_local_path(doc_type: TechnicalDocType, title: &str) -> String {
    format!(
        "docs/technical/{}/{}.wiki",
        doc_type.as_str(),
        sanitize_title_for_filename(title),
    )
}

fn mediawiki_profile_local_path(version: &str, family: &str, title: &str) -> String {
    format!(
        "docs/mediawiki/mw-{}/{}/{}.wiki",
        sanitize_path_segment(version),
        sanitize_path_segment(family),
        sanitize_title_for_filename(title),
    )
}

fn sanitize_path_segment(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "_".to_string()
    } else {
        output
    }
}

fn sanitize_title_for_filename(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            output.push('_');
        } else {
            output.push(ch);
        }
    }
    if output.is_empty() {
        "_".to_string()
    } else {
        output
    }
}

fn collect_extension_page_titles<A: DocsApi>(
    api: &mut A,
    extension: &str,
    include_subpages: bool,
) -> Result<Vec<String>> {
    let mut titles = vec![format!("Extension:{extension}")];
    if include_subpages {
        let mut subpages = api.get_subpages(
            &format!("{extension}/"),
            DOCS_NAMESPACE_EXTENSION,
            usize::MAX,
        )?;
        titles.append(&mut subpages);
    }
    dedupe_titles_in_order(&mut titles);
    titles.retain(|title| !is_translation_variant(title));
    Ok(titles)
}

fn collect_technical_page_titles<A: DocsApi>(
    api: &mut A,
    task: &TechnicalImportTask,
    limit: usize,
) -> Result<Vec<String>> {
    let mut titles = Vec::new();
    if let Some(page_title) = task.page_title.as_deref() {
        let normalized = normalize_title(page_title);
        if !normalized.is_empty() {
            titles.push(normalized.clone());
            if task.include_subpages
                && let Some((prefix, namespace)) = subpage_query_for_title(&normalized)
            {
                let mut subpages = api.get_subpages(&prefix, namespace, limit)?;
                titles.append(&mut subpages);
            }
        }
    } else {
        titles.push(task.doc_type.main_page().to_string());
        if task.include_subpages
            && let Some((prefix, namespace)) = task.doc_type.subpage_query()
        {
            let mut subpages = api.get_subpages(prefix, namespace, limit)?;
            titles.append(&mut subpages);
        }
    }
    dedupe_titles_in_order(&mut titles);
    titles.retain(|title| !is_translation_variant(title));
    Ok(titles)
}

fn collect_mediawiki_family_page_titles<A: DocsApi>(
    api: &mut A,
    family: &str,
    limit: usize,
) -> Result<Vec<String>> {
    match family {
        "hooks" => collect_technical_page_titles(
            api,
            &TechnicalImportTask {
                doc_type: TechnicalDocType::Hooks,
                page_title: None,
                include_subpages: true,
            },
            limit.max(1),
        ),
        "config" => collect_technical_page_titles(
            api,
            &TechnicalImportTask {
                doc_type: TechnicalDocType::Config,
                page_title: None,
                include_subpages: true,
            },
            limit.max(1),
        ),
        "api" => collect_technical_page_titles(
            api,
            &TechnicalImportTask {
                doc_type: TechnicalDocType::Api,
                page_title: None,
                include_subpages: true,
            },
            limit.max(1),
        ),
        "manual" => collect_manual_profile_titles(api, limit.max(1)),
        "parser" => Ok(vec!["Help:Extension:ParserFunctions".to_string()]),
        "tags" => Ok(vec!["Help:Tags".to_string(), "Help:Magic words".to_string()]),
        "lua" => Ok(vec!["Extension:Scribunto/Lua reference manual".to_string()]),
        other => bail!("unsupported MediaWiki docs family `{other}`"),
    }
}

fn collect_manual_profile_titles<A: DocsApi>(api: &mut A, limit: usize) -> Result<Vec<String>> {
    let Some(contents_page) = api.get_page("Manual:Contents")? else {
        return Ok(vec!["Manual:Contents".to_string()]);
    };
    let mut titles = vec!["Manual:Contents".to_string()];
    let mut linked = extract_manual_link_targets(&contents_page.content);
    linked.retain(|title| !title.starts_with("Manual:Hooks"));
    linked.retain(|title| !title.starts_with("Manual:$wg"));
    linked.retain(|title| !is_translation_variant(title));
    linked.truncate(limit);
    titles.extend(linked);
    dedupe_titles_in_order(&mut titles);
    Ok(titles)
}

fn extract_manual_link_targets(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    let mut titles = Vec::new();
    while cursor + 1 < bytes.len() {
        if bytes[cursor..].starts_with(b"[[") {
            if let Some(end) = find_delimited(bytes, cursor + 2, b"]]") {
                let body = &content[cursor + 2..end];
                let target = body
                    .split('|')
                    .next()
                    .unwrap_or(body)
                    .split('#')
                    .next()
                    .unwrap_or(body)
                    .trim()
                    .trim_start_matches(':');
                let normalized = normalize_title(target);
                if normalized.starts_with("Manual:") {
                    titles.push(normalized);
                }
                cursor = end + 2;
                continue;
            }
        }
        cursor += 1;
    }
    dedupe_titles_in_order(&mut titles);
    titles
}

fn subpage_query_for_title(title: &str) -> Option<(String, i32)> {
    if let Some(rest) = title.strip_prefix("Manual:") {
        return Some((format!("{rest}/"), DOCS_NAMESPACE_MANUAL));
    }
    if let Some(rest) = title.strip_prefix("API:") {
        return Some((format!("{rest}/"), DOCS_NAMESPACE_API));
    }
    if let Some(rest) = title.strip_prefix("Extension:") {
        return Some((format!("{rest}/"), DOCS_NAMESPACE_EXTENSION));
    }
    None
}

fn dedupe_titles_in_order(titles: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    titles.retain(|title| {
        let key = title.to_ascii_lowercase();
        if seen.contains(&key) {
            return false;
        }
        seen.insert(key);
        true
    });
}

fn fetch_remote_pages<A: DocsApi, F: Fn(&str) -> String>(
    api: &mut A,
    titles: &[String],
    local_path: F,
) -> Result<FetchRemotePagesReport> {
    let mut pages = Vec::new();
    let mut failures = Vec::new();
    let mut metadata = RemotePageMetadata::default();
    for title in titles {
        match api.get_page(title) {
            Ok(Some(page)) => {
                if let Some(revision_id) = page.revision_id {
                    metadata.revision_ids.insert(page.title.clone(), revision_id);
                }
                if let Some(parent_revision_id) = page.parent_revision_id {
                    metadata
                        .parent_revision_ids
                        .insert(page.title.clone(), parent_revision_id);
                }
                if !page.timestamp.is_empty() {
                    metadata
                        .timestamps
                        .insert(page.title.clone(), page.timestamp.clone());
                }
                pages.push(RemotePageInput {
                    page_title: page.title.clone(),
                    local_path: local_path(&page.title),
                    content: page.content,
                    revision_id: page.revision_id,
                    parent_revision_id: page.parent_revision_id,
                    timestamp: if page.timestamp.is_empty() {
                        None
                    } else {
                        Some(page.timestamp)
                    },
                });
            }
            Ok(None) => failures.push(format!("page missing during refresh: {title}")),
            Err(error) => failures.push(format!("failed to fetch {title}: {error}")),
        }
    }
    Ok(FetchRemotePagesReport {
        pages,
        failures,
        metadata: Some(metadata),
    })
}

fn materialize_docs_corpus(
    input: CorpusMaterializationInput,
    metadata: Option<&RemotePageMetadata>,
) -> Result<CorpusMaterialization> {
    let pages = input
        .pages
        .into_iter()
        .map(|page| {
            let revision_id = metadata
                .and_then(|value| value.revision_ids.get(&page.page_title))
                .copied();
            let parent_revision_id = metadata
                .and_then(|value| value.parent_revision_ids.get(&page.page_title))
                .copied();
            let timestamp = metadata
                .and_then(|value| value.timestamps.get(&page.page_title))
                .cloned();
            Ok(parse_docs_page(DocsPageParseInput {
                page_title: page.page_title,
                local_path: page.local_path,
                content: page.content,
                source_revision_id: revision_id,
                source_parent_revision_id: parent_revision_id,
                source_timestamp: timestamp,
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(CorpusMaterialization {
        corpus_id: input.corpus_id,
        corpus_kind: input.corpus_kind,
        label: input.label,
        source_wiki: input.source_wiki,
        source_version: input.source_version,
        source_profile: input.source_profile,
        technical_type: input.technical_type,
        refresh_kind: input.refresh_kind,
        refresh_spec: input.refresh_spec,
        fetched_at_unix: input.fetched_at_unix,
        expires_at_unix: input.expires_at_unix,
        pages,
    })
}

fn persist_docs_corpus(paths: &ResolvedPaths, corpus: &CorpusMaterialization) -> Result<()> {
    let mut connection = open_docs_connection(paths)?;
    let transaction = connection
        .transaction()
        .context("failed to start docs corpus transaction")?;

    transaction
        .execute("DELETE FROM docs_corpora WHERE corpus_id = ?1", [&corpus.corpus_id])
        .with_context(|| format!("failed to clear docs corpus {}", corpus.corpus_id))?;

    let pages_count = i64::try_from(corpus.pages.len()).context("pages count too large")?;
    let sections_count = i64::try_from(
        corpus
            .pages
            .iter()
            .map(|page| page.sections.len())
            .sum::<usize>(),
    )
    .context("sections count too large")?;
    let symbols_count = i64::try_from(
        corpus
            .pages
            .iter()
            .map(|page| page.symbols.len())
            .sum::<usize>(),
    )
    .context("symbols count too large")?;
    let examples_count = i64::try_from(
        corpus
            .pages
            .iter()
            .map(|page| page.examples.len())
            .sum::<usize>(),
    )
    .context("examples count too large")?;

    transaction
        .execute(
            "INSERT INTO docs_corpora (
                corpus_id,
                corpus_kind,
                label,
                source_wiki,
                source_version,
                source_profile,
                technical_type,
                refresh_kind,
                refresh_spec,
                pages_count,
                sections_count,
                symbols_count,
                examples_count,
                fetched_at_unix,
                expires_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                corpus.corpus_id,
                corpus.corpus_kind,
                corpus.label,
                corpus.source_wiki,
                corpus.source_version,
                corpus.source_profile,
                corpus.technical_type,
                corpus.refresh_kind,
                corpus.refresh_spec,
                pages_count,
                sections_count,
                symbols_count,
                examples_count,
                i64::try_from(corpus.fetched_at_unix).context("fetched_at_unix too large")?,
                i64::try_from(corpus.expires_at_unix).context("expires_at_unix too large")?,
            ],
        )
        .with_context(|| format!("failed to insert docs corpus {}", corpus.corpus_id))?;

    let mut page_stmt = transaction.prepare(
        "INSERT INTO docs_pages (
            corpus_id,
            page_title,
            normalized_title_key,
            page_namespace,
            doc_type,
            title_aliases,
            local_path,
            raw_content,
            normalized_content,
            content_hash,
            summary_text,
            semantic_text,
            fetched_at_unix,
            token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    let mut section_stmt = transaction.prepare(
        "INSERT INTO docs_sections (
            corpus_id,
            page_title,
            section_index,
            section_level,
            section_heading,
            summary_text,
            section_text,
            semantic_text,
            token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    let mut symbol_stmt = transaction.prepare(
        "INSERT INTO docs_symbols (
            corpus_id,
            page_title,
            symbol_index,
            symbol_kind,
            symbol_name,
            normalized_symbol_key,
            aliases,
            section_heading,
            signature_text,
            summary_text,
            detail_text,
            retrieval_text,
            token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    let mut example_stmt = transaction.prepare(
        "INSERT INTO docs_examples (
            corpus_id,
            page_title,
            example_index,
            example_kind,
            section_heading,
            language_hint,
            summary_text,
            example_text,
            retrieval_text,
            token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    let mut link_stmt = transaction.prepare(
        "INSERT INTO docs_links (
            corpus_id,
            page_title,
            link_index,
            target_title,
            relation_kind,
            display_text
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    for page in &corpus.pages {
        page_stmt.execute(params![
            corpus.corpus_id,
            page.page_title,
            normalize_query_key(&page.page_title),
            page.page_namespace,
            page.page_kind,
            page.alias_titles.join("\n"),
            page.local_path,
            page.content,
            collapse_whitespace(&page.content),
            compute_hash(&page.content),
            page.summary_text,
            page.semantic_text,
            i64::try_from(corpus.fetched_at_unix).context("fetched_at_unix too large")?,
            i64::try_from(page.token_estimate).context("page token estimate too large")?,
        ])?;
        for section in &page.sections {
            section_stmt.execute(params![
                corpus.corpus_id,
                page.page_title,
                i64::try_from(section.section_index).context("section index too large")?,
                i64::from(section.section_level),
                if section.section_kind == "lead" {
                    None::<String>
                } else {
                    Some(section.heading.clone())
                },
                section.summary_text,
                section.section_text,
                collapse_whitespace(&format!(
                    "{} {} {} {}",
                    page.page_title,
                    section.heading_path,
                    section.symbol_names.join(" "),
                    section.section_text
                )),
                i64::try_from(section.token_estimate).context("section token estimate too large")?,
            ])?;
        }
        for (index, symbol) in page.symbols.iter().enumerate() {
            symbol_stmt.execute(params![
                corpus.corpus_id,
                page.page_title,
                i64::try_from(index).context("symbol index too large")?,
                symbol.symbol_kind,
                symbol.symbol_name,
                normalize_query_key(&symbol.canonical_name),
                symbol.aliases.join("\n"),
                symbol.section_heading,
                symbol.signature_text,
                symbol.summary_text,
                symbol.retrieval_text,
                symbol.retrieval_text,
                i64::try_from(symbol.token_estimate).context("symbol token estimate too large")?,
            ])?;
        }
        for example in &page.examples {
            example_stmt.execute(params![
                corpus.corpus_id,
                page.page_title,
                i64::try_from(example.example_index).context("example index too large")?,
                example.example_kind,
                example.section_heading,
                example.language.clone().unwrap_or_default(),
                example.summary_text,
                example.example_text,
                example.retrieval_text,
                i64::try_from(example.token_estimate).context("example token estimate too large")?,
            ])?;
        }
        for (index, target_title) in page.link_titles.iter().enumerate() {
            link_stmt.execute(params![
                corpus.corpus_id,
                page.page_title,
                i64::try_from(index).context("link index too large")?,
                target_title,
                "wikilink",
                target_title,
            ])?;
        }
    }

    drop(link_stmt);
    drop(example_stmt);
    drop(symbol_stmt);
    drop(section_stmt);
    drop(page_stmt);
    transaction.commit().context("failed to commit docs corpus")?;
    Ok(())
}

fn rebuild_docs_fts_indexes(paths: &ResolvedPaths) -> Result<()> {
    let connection = open_docs_connection(paths)?;
    for table in [
        "docs_pages_fts",
        "docs_sections_fts",
        "docs_symbols_fts",
        "docs_examples_fts",
    ] {
        if fts_table_exists(&connection, table) {
            connection
                .execute_batch(&format!("INSERT INTO {table}({table}) VALUES('rebuild')"))
                .with_context(|| format!("failed to rebuild {table}"))?;
        }
    }
    Ok(())
}

fn load_docs_stats(connection: &Connection) -> Result<DocsStats> {
    let corpus_count = count_query(connection, "SELECT COUNT(*) FROM docs_corpora")?;
    let page_count = count_query(connection, "SELECT COUNT(*) FROM docs_pages")?;
    let section_count = count_query(connection, "SELECT COUNT(*) FROM docs_sections")?;
    let symbol_count = count_query(connection, "SELECT COUNT(*) FROM docs_symbols")?;
    let example_count = count_query(connection, "SELECT COUNT(*) FROM docs_examples")?;
    let extension_count = count_query(
        connection,
        "SELECT COUNT(*) FROM docs_corpora WHERE corpus_kind = 'extension'",
    )?;
    let extension_pages_count = count_query(
        connection,
        "SELECT COALESCE(SUM(pages_count), 0) FROM docs_corpora WHERE corpus_kind = 'extension'",
    )?;
    let technical_count = count_query(
        connection,
        "SELECT COUNT(*) FROM docs_pages dp JOIN docs_corpora dc ON dc.corpus_id = dp.corpus_id WHERE dc.corpus_kind IN ('technical', 'mediawiki')",
    )?;
    let mut technical_by_type = BTreeMap::new();
    let mut stmt = connection.prepare(
        "SELECT technical_type, COUNT(*) FROM docs_corpora
         WHERE technical_type != ''
         GROUP BY technical_type
         ORDER BY technical_type ASC",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    for row in rows {
        let (technical_type, count) = row?;
        technical_by_type.insert(
            technical_type,
            usize::try_from(count).context("negative technical count")?,
        );
    }

    Ok(DocsStats {
        corpus_count,
        page_count,
        section_count,
        symbol_count,
        example_count,
        extension_count,
        extension_pages_count,
        technical_count,
        technical_by_type,
    })
}

fn load_docs_corpora(connection: &Connection, now_unix: u64) -> Result<Vec<DocsCorpusSummary>> {
    let mut stmt = connection.prepare(
        "SELECT corpus_id, corpus_kind, label, source_wiki, source_version, source_profile,
                technical_type, pages_count, sections_count, symbols_count, examples_count,
                fetched_at_unix, expires_at_unix
         FROM docs_corpora
         ORDER BY corpus_kind ASC, label ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(DocsCorpusSummary {
            corpus_id: row.get(0)?,
            corpus_kind: row.get(1)?,
            label: row.get(2)?,
            source_wiki: row.get(3)?,
            source_version: row.get(4)?,
            source_profile: row.get(5)?,
            technical_type: row.get(6)?,
            pages_count: usize::try_from(row.get::<_, i64>(7)?).unwrap_or(0),
            sections_count: usize::try_from(row.get::<_, i64>(8)?).unwrap_or(0),
            symbols_count: usize::try_from(row.get::<_, i64>(9)?).unwrap_or(0),
            examples_count: usize::try_from(row.get::<_, i64>(10)?).unwrap_or(0),
            fetched_at_unix: u64::try_from(row.get::<_, i64>(11)?).unwrap_or(0),
            expires_at_unix: u64::try_from(row.get::<_, i64>(12)?).unwrap_or(0),
            expired: u64::try_from(row.get::<_, i64>(12)?).unwrap_or(0) <= now_unix,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().context("failed to read docs corpora")
}

fn load_extension_docs(connection: &Connection, now_unix: u64) -> Result<Vec<ExtensionDocSummary>> {
    let mut stmt = connection.prepare(
        "SELECT label, source_wiki, source_version, pages_count, sections_count, symbols_count,
                examples_count, fetched_at_unix, expires_at_unix
         FROM docs_corpora
         WHERE corpus_kind = 'extension'
         ORDER BY label ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let expires_at_unix = u64::try_from(row.get::<_, i64>(8)?).unwrap_or(0);
        Ok(ExtensionDocSummary {
            extension_name: row.get(0)?,
            source_wiki: row.get(1)?,
            version: Some(row.get::<_, String>(2)?),
            pages_count: usize::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
            sections_count: usize::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
            symbols_count: usize::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
            examples_count: usize::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
            fetched_at_unix: u64::try_from(row.get::<_, i64>(7)?).unwrap_or(0),
            expires_at_unix,
            expired: expires_at_unix <= now_unix,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .context("failed to read extension docs summary")
}

fn load_technical_docs(
    connection: &Connection,
    technical_type: Option<&str>,
    now_unix: u64,
) -> Result<Vec<TechnicalDocSummary>> {
    let mut query = String::from(
        "SELECT dc.technical_type, dp.page_title, dp.local_path, dc.fetched_at_unix, dc.expires_at_unix
         FROM docs_pages dp
         JOIN docs_corpora dc ON dc.corpus_id = dp.corpus_id
         WHERE dc.corpus_kind IN ('technical', 'mediawiki')",
    );
    if technical_type.is_some() {
        query.push_str(" AND dc.technical_type = ?1");
    }
    query.push_str(" ORDER BY dc.technical_type ASC, dp.page_title ASC");
    let mut stmt = connection.prepare(&query)?;
    let mapper = |row: &rusqlite::Row<'_>| {
        let expires_at_unix = u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0);
        Ok(TechnicalDocSummary {
            doc_type: row.get(0)?,
            page_title: row.get(1)?,
            local_path: row.get(2)?,
            fetched_at_unix: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
            expires_at_unix,
            expired: expires_at_unix <= now_unix,
        })
    };
    let rows = if let Some(technical_type) = technical_type {
        stmt.query_map([technical_type], mapper)?
    } else {
        stmt.query_map([], mapper)?
    };
    rows.collect::<Result<Vec<_>, _>>()
        .context("failed to read technical docs summary")
}

fn load_outdated_docs(connection: &Connection, now_unix: u64) -> Result<DocsOutdatedReport> {
    let mut extensions = Vec::new();
    let mut extension_stmt = connection.prepare(
        "SELECT label, expires_at_unix
         FROM docs_corpora
         WHERE corpus_kind = 'extension' AND expires_at_unix <= ?1
         ORDER BY label ASC",
    )?;
    let extension_rows = extension_stmt.query_map([i64::try_from(now_unix).unwrap_or(i64::MAX)], |row| {
        Ok(OutdatedExtensionDoc {
            extension_name: row.get(0)?,
            expires_at_unix: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
        })
    })?;
    for row in extension_rows {
        extensions.push(row?);
    }

    let mut technical = Vec::new();
    let mut technical_stmt = connection.prepare(
        "SELECT technical_type, label, expires_at_unix
         FROM docs_corpora
         WHERE corpus_kind IN ('technical', 'mediawiki') AND technical_type != '' AND expires_at_unix <= ?1
         ORDER BY technical_type ASC, label ASC",
    )?;
    let technical_rows = technical_stmt.query_map([i64::try_from(now_unix).unwrap_or(i64::MAX)], |row| {
        Ok(OutdatedTechnicalDoc {
            doc_type: row.get(0)?,
            page_title: row.get(1)?,
            expires_at_unix: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
        })
    })?;
    for row in technical_rows {
        technical.push(row?);
    }

    Ok(DocsOutdatedReport {
        extensions,
        technical,
    })
}

fn load_outdated_corpora(connection: &Connection, now_unix: u64) -> Result<Vec<OutdatedCorpusRow>> {
    let mut stmt = connection.prepare(
        "SELECT corpus_id, corpus_kind, label, source_profile, technical_type, refresh_kind, refresh_spec
         FROM docs_corpora
         WHERE expires_at_unix <= ?1 AND refresh_kind != 'bundle'
         ORDER BY label ASC",
    )?;
    let rows = stmt.query_map([i64::try_from(now_unix).unwrap_or(i64::MAX)], |row| {
        Ok(OutdatedCorpusRow {
            corpus_id: row.get(0)?,
            corpus_kind: row.get(1)?,
            label: row.get(2)?,
            source_profile: row.get(3)?,
            technical_type: row.get(4)?,
            refresh_kind: row.get(5)?,
            refresh_spec: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .context("failed to read outdated corpora")
}

fn refresh_corpus<A: DocsApi>(
    paths: &ResolvedPaths,
    corpus: &OutdatedCorpusRow,
    api: &mut A,
) -> Result<RefreshResult> {
    match corpus.refresh_kind.as_str() {
        "extension" => {
            let spec: ExtensionRefreshSpec =
                serde_json::from_str(&corpus.refresh_spec).context("invalid extension refresh spec")?;
            let report = import_extension_docs_with_api(
                paths,
                &DocsImportOptions {
                    extensions: vec![spec.extension_name],
                    include_subpages: spec.include_subpages,
                },
                api,
            )?;
            Ok(RefreshResult {
                pages: report.imported_pages,
                sections: report.imported_sections,
                symbols: report.imported_symbols,
                examples: report.imported_examples,
            })
        }
        "technical" => {
            let spec: TechnicalRefreshSpec =
                serde_json::from_str(&corpus.refresh_spec).context("invalid technical refresh spec")?;
            let doc_type = TechnicalDocType::parse(&spec.doc_type)
                .ok_or_else(|| anyhow::anyhow!("unsupported technical refresh type {}", spec.doc_type))?;
            let report = import_technical_docs_with_api(
                paths,
                &DocsImportTechnicalOptions {
                    tasks: vec![TechnicalImportTask {
                        doc_type,
                        page_title: spec.page_title,
                        include_subpages: spec.include_subpages,
                    }],
                    limit: spec.limit,
                },
                api,
            )?;
            Ok(RefreshResult {
                pages: report.imported_pages,
                sections: report.imported_sections,
                symbols: report.imported_symbols,
                examples: report.imported_examples,
            })
        }
        "mediawiki_family" => {
            let spec: MediaWikiRefreshSpec =
                serde_json::from_str(&corpus.refresh_spec).context("invalid mediawiki refresh spec")?;
            let mut options = DocsImportMediaWikiOptions {
                mw_version: spec.mw_version,
                hooks: false,
                config: false,
                api: false,
                manual: false,
                parser: false,
                tags: false,
                lua: false,
                limit: spec.limit,
            };
            match spec.family.as_str() {
                "hooks" => options.hooks = true,
                "config" => options.config = true,
                "api" => options.api = true,
                "manual" => options.manual = true,
                "parser" => options.parser = true,
                "tags" => options.tags = true,
                "lua" => options.lua = true,
                other => bail!("unsupported mediawiki refresh family `{other}`"),
            }
            let report = import_mediawiki_docs_with_api(paths, &options, api)?;
            Ok(RefreshResult {
                pages: report.imported_pages,
                sections: report.imported_sections,
                symbols: report.imported_symbols,
                examples: report.imported_examples,
            })
        }
        other => bail!("unsupported refresh kind `{other}`"),
    }
}

fn try_remove_docs_corpus(
    connection: &mut Connection,
    target: &str,
) -> Result<Option<DocsRemoveReport>> {
    let removed = connection.execute("DELETE FROM docs_corpora WHERE corpus_id = ?1", [target])?;
    if removed > 0 {
        return Ok(Some(DocsRemoveReport {
            kind: DocsRemoveKind::Corpus,
            target: target.to_string(),
            removed_rows: removed,
        }));
    }
    Ok(None)
}

fn try_remove_docs_profile(
    connection: &mut Connection,
    target: &str,
) -> Result<Option<DocsRemoveReport>> {
    let removed = connection.execute("DELETE FROM docs_corpora WHERE source_profile = ?1", [target])?;
    if removed > 0 {
        return Ok(Some(DocsRemoveReport {
            kind: DocsRemoveKind::Profile,
            target: target.to_string(),
            removed_rows: removed,
        }));
    }
    Ok(None)
}

fn try_remove_docs_extension(
    connection: &mut Connection,
    target: &str,
) -> Result<Option<DocsRemoveReport>> {
    let removed = connection.execute(
        "DELETE FROM docs_corpora WHERE corpus_kind = 'extension' AND lower(label) = lower(?1)",
        [target],
    )?;
    if removed > 0 {
        return Ok(Some(DocsRemoveReport {
            kind: DocsRemoveKind::Extension,
            target: target.to_string(),
            removed_rows: removed,
        }));
    }
    Ok(None)
}

fn try_remove_docs_technical_type(
    connection: &mut Connection,
    target: &str,
) -> Result<Option<DocsRemoveReport>> {
    let removed = connection.execute(
        "DELETE FROM docs_corpora WHERE technical_type != '' AND lower(technical_type) = lower(?1)",
        [target],
    )?;
    if removed > 0 {
        return Ok(Some(DocsRemoveReport {
            kind: DocsRemoveKind::TechnicalType,
            target: target.to_string(),
            removed_rows: removed,
        }));
    }
    Ok(None)
}

fn try_remove_docs_page(
    connection: &mut Connection,
    target: &str,
) -> Result<Option<DocsRemoveReport>> {
    let removed = connection.execute("DELETE FROM docs_pages WHERE page_title = ?1", [target])?;
    if removed > 0 {
        prune_empty_docs_corpora(connection)?;
        return Ok(Some(DocsRemoveReport {
            kind: DocsRemoveKind::TechnicalPage,
            target: target.to_string(),
            removed_rows: removed,
        }));
    }
    Ok(None)
}

fn prune_empty_docs_corpora(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "DELETE FROM docs_corpora
         WHERE corpus_id IN (
             SELECT dc.corpus_id
             FROM docs_corpora dc
             LEFT JOIN docs_pages dp ON dp.corpus_id = dc.corpus_id
             GROUP BY dc.corpus_id
             HAVING COUNT(dp.page_title) = 0
         );",
    )?;
    Ok(())
}

fn search_docs_with_connection(
    connection: &Connection,
    query: &str,
    tier: Option<&str>,
    limit: usize,
) -> Result<Vec<DocsSearchHit>> {
    let normalized_query = collapse_whitespace(query);
    if normalized_query.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let lowered_query = normalized_query.to_ascii_lowercase();
    let query_key = normalize_query_key(&normalized_query);
    let pattern = format!("%{lowered_query}%");
    let fts_query = build_fts_query(&normalized_query);
    let mut candidates = BTreeMap::<String, SearchCandidate>::new();

    let mut page_stmt = connection.prepare(
        "SELECT dc.corpus_id, dc.corpus_kind, dc.label, dp.page_title, dp.doc_type, dp.summary_text, dp.raw_content
         FROM docs_pages dp
         JOIN docs_corpora dc ON dc.corpus_id = dp.corpus_id
         WHERE dp.normalized_title_key = ?1 OR lower(dp.page_title) LIKE ?2 OR lower(dp.title_aliases) LIKE ?2
         ORDER BY dp.page_title ASC
         LIMIT ?3",
    )?;
    let page_rows = page_stmt.query_map(params![query_key, pattern, i64::try_from(limit * 3).unwrap_or(i64::MAX)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;
    for row in page_rows {
        let (corpus_id, corpus_kind, corpus_label, page_title, doc_type, summary_text, content) = row?;
        add_search_candidate(
            &mut candidates,
            SearchCandidate {
                weight: 120,
                hit: DocsSearchHit {
                    tier: "page".to_string(),
                    corpus_id,
                    corpus_kind,
                    corpus_label,
                    title: page_title.clone(),
                    page_title,
                    section_heading: None,
                    detail_kind: doc_type,
                    retrieval_reason: "page-title".to_string(),
                    snippet: make_snippet(&content, &lowered_query),
                },
            },
            tier,
        );
    }

    let mut symbol_stmt = connection.prepare(
        "SELECT dc.corpus_id, dc.corpus_kind, dc.label, ds.page_title, ds.section_heading, ds.symbol_kind,
                ds.symbol_name, ds.aliases, ds.detail_text
         FROM docs_symbols ds
         JOIN docs_corpora dc ON dc.corpus_id = ds.corpus_id
         WHERE ds.normalized_symbol_key = ?1 OR lower(ds.symbol_name) LIKE ?2 OR lower(ds.aliases) LIKE ?2
         ORDER BY ds.symbol_name ASC
         LIMIT ?3",
    )?;
    let symbol_rows = symbol_stmt.query_map(params![query_key, pattern, i64::try_from(limit * 4).unwrap_or(i64::MAX)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
        ))
    })?;
    for row in symbol_rows {
        let (corpus_id, corpus_kind, corpus_label, page_title, section_heading, symbol_kind, symbol_name, aliases, detail_text) = row?;
        add_search_candidate(
            &mut candidates,
            SearchCandidate {
                weight: 140,
                hit: DocsSearchHit {
                    tier: "symbol".to_string(),
                    corpus_id,
                    corpus_kind,
                    corpus_label,
                    title: symbol_name.clone(),
                    page_title,
                    section_heading,
                    detail_kind: symbol_kind,
                    retrieval_reason: if aliases.to_ascii_lowercase().contains(&lowered_query) {
                        "symbol-alias".to_string()
                    } else {
                        "symbol-name".to_string()
                    },
                    snippet: make_snippet(&detail_text, &lowered_query),
                },
            },
            tier,
        );
    }

    let mut section_stmt = connection.prepare(
        "SELECT dc.corpus_id, dc.corpus_kind, dc.label, ds.page_title, ds.section_heading, ds.summary_text, ds.section_text
         FROM docs_sections ds
         JOIN docs_corpora dc ON dc.corpus_id = ds.corpus_id
         WHERE lower(COALESCE(ds.section_heading, '')) LIKE ?1 OR lower(ds.semantic_text) LIKE ?1
         ORDER BY ds.page_title ASC, ds.section_index ASC
         LIMIT ?2",
    )?;
    let section_rows = section_stmt.query_map(params![pattern, i64::try_from(limit * 4).unwrap_or(i64::MAX)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;
    for row in section_rows {
        let (corpus_id, corpus_kind, corpus_label, page_title, section_heading, summary_text, section_text) = row?;
        add_search_candidate(
            &mut candidates,
            SearchCandidate {
                weight: 90,
                hit: DocsSearchHit {
                    tier: "section".to_string(),
                    corpus_id,
                    corpus_kind,
                    corpus_label,
                    title: section_heading.clone().unwrap_or_else(|| page_title.clone()),
                    page_title,
                    section_heading,
                    detail_kind: "section".to_string(),
                    retrieval_reason: "section-text".to_string(),
                    snippet: make_snippet(&format!("{summary_text} {section_text}"), &lowered_query),
                },
            },
            tier,
        );
    }

    let mut example_stmt = connection.prepare(
        "SELECT dc.corpus_id, dc.corpus_kind, dc.label, de.page_title, de.section_heading,
                de.example_kind, de.language_hint, de.summary_text, de.example_text
         FROM docs_examples de
         JOIN docs_corpora dc ON dc.corpus_id = de.corpus_id
         WHERE lower(de.retrieval_text) LIKE ?1
         ORDER BY de.page_title ASC, de.example_index ASC
         LIMIT ?2",
    )?;
    let example_rows = example_stmt.query_map(params![pattern, i64::try_from(limit * 3).unwrap_or(i64::MAX)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
        ))
    })?;
    for row in example_rows {
        let (corpus_id, corpus_kind, corpus_label, page_title, section_heading, example_kind, language_hint, summary_text, example_text) = row?;
        add_search_candidate(
            &mut candidates,
            SearchCandidate {
                weight: 70,
                hit: DocsSearchHit {
                    tier: "example".to_string(),
                    corpus_id,
                    corpus_kind,
                    corpus_label,
                    title: page_title.clone(),
                    page_title,
                    section_heading,
                    detail_kind: if language_hint.is_empty() {
                        example_kind
                    } else {
                        format!("{example_kind}/{language_hint}")
                    },
                    retrieval_reason: "example".to_string(),
                    snippet: make_snippet(&format!("{summary_text} {example_text}"), &lowered_query),
                },
            },
            tier,
        );
    }

    if !fts_query.is_empty() {
        load_fts_page_candidates(connection, &fts_query, &lowered_query, tier, limit, &mut candidates)?;
        load_fts_symbol_candidates(connection, &fts_query, &lowered_query, tier, limit, &mut candidates)?;
    }

    let mut out = candidates.into_values().collect::<Vec<_>>();
    out.sort_by(|left, right| {
        right
            .weight
            .cmp(&left.weight)
            .then_with(|| left.hit.title.cmp(&right.hit.title))
    });
    out.truncate(limit);
    Ok(out.into_iter().map(|candidate| candidate.hit).collect())
}

fn build_docs_context(
    connection: &Connection,
    query: &str,
    options: &DocsContextOptions,
) -> Result<DocsContextReport> {
    let query = collapse_whitespace(query);
    if query.is_empty() {
        bail!("docs context requires a non-empty query");
    }
    let hits = search_docs_with_connection(connection, &query, None, options.max_pages * 6)?;
    let mut selected_pages = Vec::<(String, String, String)>::new();
    let mut seen_pages = BTreeSet::new();
    for hit in &hits {
        let key = format!("{}|{}", hit.corpus_id, hit.page_title);
        if seen_pages.insert(key) {
            selected_pages.push((
                hit.corpus_id.clone(),
                hit.page_title.clone(),
                hit.retrieval_reason.clone(),
            ));
        }
        if selected_pages.len() >= options.max_pages {
            break;
        }
    }

    let mut pages = Vec::new();
    let mut sections = Vec::new();
    let mut symbols = Vec::new();
    let mut examples = Vec::new();
    let mut related_pages = BTreeSet::new();
    let mut remaining_tokens = options.token_budget;

    for (corpus_id, page_title, retrieval_reason) in selected_pages {
        if let Some(page) = load_context_page(connection, &corpus_id, &page_title, &retrieval_reason)? {
            pages.push(page);
        }
        for section in load_context_sections(connection, &corpus_id, &page_title, &query, options.max_sections)? {
            if sections.len() >= options.max_sections || section.token_estimate > remaining_tokens {
                break;
            }
            remaining_tokens = remaining_tokens.saturating_sub(section.token_estimate);
            sections.push(section);
        }
        for symbol in load_context_symbols(connection, &corpus_id, &page_title, &query, options.max_symbols)? {
            if symbols.len() >= options.max_symbols {
                break;
            }
            symbols.push(symbol);
        }
        for example in load_context_examples(connection, &corpus_id, &page_title, &query, options.max_examples)? {
            if examples.len() >= options.max_examples || example.token_estimate > remaining_tokens {
                break;
            }
            remaining_tokens = remaining_tokens.saturating_sub(example.token_estimate);
            examples.push(example);
        }
        for related in load_related_pages(connection, &corpus_id, &page_title)? {
            related_pages.insert(related);
        }
    }

    Ok(DocsContextReport {
        query,
        source_profile: options.source_profile.clone(),
        source_version: options.source_version.clone(),
        pages,
        sections,
        symbols,
        examples,
        related_pages: related_pages.into_iter().take(12).collect(),
        token_estimate: options.token_budget.saturating_sub(remaining_tokens),
    })
}

fn add_search_candidate(
    candidates: &mut BTreeMap<String, SearchCandidate>,
    candidate: SearchCandidate,
    requested_tier: Option<&str>,
) {
    if !tier_matches_request(requested_tier, &candidate.hit.corpus_kind, &candidate.hit.tier) {
        return;
    }
    let key = format!(
        "{}|{}|{}|{}",
        candidate.hit.tier,
        candidate.hit.corpus_id,
        candidate.hit.page_title,
        candidate.hit.section_heading.as_deref().unwrap_or(&candidate.hit.title)
    );
    match candidates.get_mut(&key) {
        Some(existing) if candidate.weight > existing.weight => *existing = candidate,
        None => {
            candidates.insert(key, candidate);
        }
        _ => {}
    }
}

fn tier_matches_request(requested: Option<&str>, corpus_kind: &str, actual_tier: &str) -> bool {
    let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    if requested.eq_ignore_ascii_case("extension") {
        return corpus_kind == "extension";
    }
    if requested.eq_ignore_ascii_case("technical") {
        return corpus_kind == "technical" || corpus_kind == "mediawiki";
    }
    requested.eq_ignore_ascii_case(actual_tier)
}

fn load_fts_page_candidates(
    connection: &Connection,
    fts_query: &str,
    lowered_query: &str,
    tier: Option<&str>,
    limit: usize,
    candidates: &mut BTreeMap<String, SearchCandidate>,
) -> Result<()> {
    let mut stmt = connection.prepare(
        "SELECT dc.corpus_id, dc.corpus_kind, dc.label, dp.page_title, dp.doc_type, dp.raw_content
         FROM docs_pages_fts fts
         JOIN docs_pages dp ON dp.rowid = fts.rowid
         JOIN docs_corpora dc ON dc.corpus_id = dp.corpus_id
         WHERE docs_pages_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![fts_query, i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    for row in rows {
        let (corpus_id, corpus_kind, corpus_label, page_title, doc_type, raw_content) = row?;
        add_search_candidate(
            candidates,
            SearchCandidate {
                weight: 60,
                hit: DocsSearchHit {
                    tier: "page".to_string(),
                    corpus_id,
                    corpus_kind,
                    corpus_label,
                    title: page_title.clone(),
                    page_title,
                    section_heading: None,
                    detail_kind: doc_type,
                    retrieval_reason: "page-fts".to_string(),
                    snippet: make_snippet(&raw_content, lowered_query),
                },
            },
            tier,
        );
    }
    Ok(())
}

fn load_fts_symbol_candidates(
    connection: &Connection,
    fts_query: &str,
    lowered_query: &str,
    tier: Option<&str>,
    limit: usize,
    candidates: &mut BTreeMap<String, SearchCandidate>,
) -> Result<()> {
    let mut stmt = connection.prepare(
        "SELECT dc.corpus_id, dc.corpus_kind, dc.label, ds.page_title, ds.section_heading,
                ds.symbol_kind, ds.symbol_name, ds.detail_text
         FROM docs_symbols_fts fts
         JOIN docs_symbols ds ON ds.rowid = fts.rowid
         JOIN docs_corpora dc ON dc.corpus_id = ds.corpus_id
         WHERE docs_symbols_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![fts_query, i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
        ))
    })?;
    for row in rows {
        let (corpus_id, corpus_kind, corpus_label, page_title, section_heading, symbol_kind, symbol_name, detail_text) = row?;
        add_search_candidate(
            candidates,
            SearchCandidate {
                weight: 55,
                hit: DocsSearchHit {
                    tier: "symbol".to_string(),
                    corpus_id,
                    corpus_kind,
                    corpus_label,
                    title: symbol_name,
                    page_title,
                    section_heading,
                    detail_kind: symbol_kind,
                    retrieval_reason: "symbol-fts".to_string(),
                    snippet: make_snippet(&detail_text, lowered_query),
                },
            },
            tier,
        );
    }
    Ok(())
}

fn load_context_page(
    connection: &Connection,
    corpus_id: &str,
    page_title: &str,
    retrieval_reason: &str,
) -> Result<Option<DocsContextPage>> {
    connection
        .query_row(
            "SELECT dc.label, dp.doc_type, dp.summary_text
             FROM docs_pages dp
             JOIN docs_corpora dc ON dc.corpus_id = dp.corpus_id
             WHERE dp.corpus_id = ?1 AND dp.page_title = ?2",
            params![corpus_id, page_title],
            |row| {
                Ok(DocsContextPage {
                    corpus_label: row.get(0)?,
                    page_title: page_title.to_string(),
                    doc_type: row.get(1)?,
                    summary_text: row.get(2)?,
                    retrieval_reason: retrieval_reason.to_string(),
                })
            },
        )
        .optional()
        .context("failed to load docs context page")
}

fn load_context_sections(
    connection: &Connection,
    corpus_id: &str,
    page_title: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<DocsContextSection>> {
    let pattern = format!("%{}%", query.to_ascii_lowercase());
    let mut stmt = connection.prepare(
        "SELECT section_heading, summary_text, section_text, token_estimate
         FROM docs_sections
         WHERE corpus_id = ?1 AND page_title = ?2
         ORDER BY CASE WHEN lower(semantic_text) LIKE ?3 THEN 0 ELSE 1 END, section_index ASC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(
        params![corpus_id, page_title, pattern, i64::try_from(limit).unwrap_or(i64::MAX)],
        |row| {
            Ok(DocsContextSection {
                page_title: page_title.to_string(),
                section_heading: row.get(0)?,
                summary_text: row.get(1)?,
                section_text: row.get(2)?,
                retrieval_reason: "section-match".to_string(),
                token_estimate: usize::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>()
        .context("failed to load docs context sections")
}

fn load_context_symbols(
    connection: &Connection,
    corpus_id: &str,
    page_title: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<DocsContextSymbol>> {
    let pattern = format!("%{}%", query.to_ascii_lowercase());
    let mut stmt = connection.prepare(
        "SELECT section_heading, symbol_kind, symbol_name, aliases, summary_text
         FROM docs_symbols
         WHERE corpus_id = ?1 AND page_title = ?2
         ORDER BY CASE WHEN lower(retrieval_text) LIKE ?3 THEN 0 ELSE 1 END, symbol_index ASC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(
        params![corpus_id, page_title, pattern, i64::try_from(limit).unwrap_or(i64::MAX)],
        |row| {
            let aliases = row
                .get::<_, String>(3)?
                .lines()
                .map(str::to_string)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            Ok(DocsContextSymbol {
                page_title: page_title.to_string(),
                section_heading: row.get(0)?,
                symbol_kind: row.get(1)?,
                symbol_name: row.get(2)?,
                aliases,
                summary_text: row.get(4)?,
                retrieval_reason: "symbol-match".to_string(),
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>()
        .context("failed to load docs context symbols")
}

fn load_context_examples(
    connection: &Connection,
    corpus_id: &str,
    page_title: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<DocsContextExample>> {
    let pattern = format!("%{}%", query.to_ascii_lowercase());
    let mut stmt = connection.prepare(
        "SELECT section_heading, example_kind, language_hint, summary_text, example_text, token_estimate
         FROM docs_examples
         WHERE corpus_id = ?1 AND page_title = ?2
         ORDER BY CASE WHEN lower(retrieval_text) LIKE ?3 THEN 0 ELSE 1 END, example_index ASC
         LIMIT ?4",
    )?;
    let rows = stmt.query_map(
        params![corpus_id, page_title, pattern, i64::try_from(limit).unwrap_or(i64::MAX)],
        |row| {
            let language_hint: String = row.get(2)?;
            Ok(DocsContextExample {
                page_title: page_title.to_string(),
                section_heading: row.get(0)?,
                example_kind: row.get(1)?,
                language_hint: if language_hint.is_empty() {
                    None
                } else {
                    Some(language_hint)
                },
                summary_text: row.get(3)?,
                example_text: row.get(4)?,
                retrieval_reason: "example-match".to_string(),
                token_estimate: usize::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>()
        .context("failed to load docs context examples")
}

fn load_related_pages(connection: &Connection, corpus_id: &str, page_title: &str) -> Result<Vec<String>> {
    let mut stmt = connection.prepare(
        "SELECT target_title
         FROM docs_links
         WHERE corpus_id = ?1 AND page_title = ?2
         ORDER BY target_title ASC
         LIMIT 12",
    )?;
    let rows = stmt.query_map(params![corpus_id, page_title], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>()
        .context("failed to load related docs pages")
}

fn normalize_query_key(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        }
    }
    output
}

fn build_fts_query(query: &str) -> String {
    let terms = query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return String::new();
    }
    terms
        .into_iter()
        .map(|term| format!("\"{term}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn make_snippet(content: &str, lowered_query: &str) -> String {
    let normalized = collapse_whitespace(content);
    if normalized.is_empty() {
        return "<empty>".to_string();
    }
    let lowered = normalized.to_ascii_lowercase();
    let Some(index) = lowered.find(lowered_query) else {
        return truncate_text(&normalized, 220);
    };
    let start = index.saturating_sub(80);
    let end = (index + lowered_query.len() + 120).min(normalized.len());
    let snippet = normalized[start..end].trim().to_string();
    if start > 0 {
        format!("...{snippet}...")
    } else if end < normalized.len() {
        format!("{snippet}...")
    } else {
        snippet
    }
}

fn count_query(connection: &Connection, sql: &str) -> Result<usize> {
    let count: i64 = connection.query_row(sql, [], |row| row.get(0))?;
    usize::try_from(count).context("negative count encountered")
}

fn fts_table_exists(connection: &Connection, table_name: &str) -> bool {
    let exists: i64 = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get(0),
        )
        .unwrap_or(0);
    exists == 1
}

fn open_docs_connection(paths: &ResolvedPaths) -> Result<Connection> {
    open_initialized_database_connection(&paths.db_path)
}

fn find_delimited(bytes: &[u8], start: usize, pattern: &[u8]) -> Option<usize> {
    let mut cursor = start;
    while cursor + pattern.len() <= bytes.len() {
        if bytes[cursor..].starts_with(pattern) {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn normalize_mediawiki_options(options: &DocsImportMediaWikiOptions) -> DocsImportMediaWikiOptions {
    let mut normalized = options.clone();
    normalized.mw_version = collapse_whitespace(&normalized.mw_version);
    if normalized.mw_version.is_empty() {
        normalized.mw_version = MEDIAWIKI_VERSION_DEFAULT.to_string();
    }
    if !normalized.hooks
        && !normalized.config
        && !normalized.api
        && !normalized.manual
        && !normalized.parser
        && !normalized.tags
        && !normalized.lua
    {
        normalized = DocsImportMediaWikiOptions::default();
    }
    normalized.limit = normalized.limit.max(1);
    normalized
}

fn selected_mediawiki_families(options: &DocsImportMediaWikiOptions) -> Vec<&'static str> {
    let mut out = Vec::new();
    if options.hooks {
        out.push("hooks");
    }
    if options.config {
        out.push("config");
    }
    if options.api {
        out.push("api");
    }
    if options.manual {
        out.push("manual");
    }
    if options.parser {
        out.push("parser");
    }
    if options.tags {
        out.push("tags");
    }
    if options.lua {
        out.push("lua");
    }
    out
}

fn extract_extension_version(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let body = trimmed.trim_start_matches('|').trim_start();
        let mut parts = body.splitn(2, '=');
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("version") {
            let normalized = collapse_whitespace(value);
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }
    None
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
