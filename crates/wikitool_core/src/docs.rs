use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::{StatusCode, Url};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::ResolvedPaths;

const DOCS_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS extension_docs (
    extension_name TEXT PRIMARY KEY,
    source_wiki TEXT NOT NULL,
    version TEXT,
    pages_count INTEGER NOT NULL,
    fetched_at_unix INTEGER NOT NULL,
    expires_at_unix INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS extension_doc_pages (
    extension_name TEXT NOT NULL,
    page_title TEXT NOT NULL,
    local_path TEXT NOT NULL,
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    fetched_at_unix INTEGER NOT NULL,
    PRIMARY KEY (extension_name, page_title),
    FOREIGN KEY (extension_name) REFERENCES extension_docs(extension_name) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_extension_doc_pages_title ON extension_doc_pages(page_title);

CREATE TABLE IF NOT EXISTS technical_docs (
    doc_type TEXT NOT NULL,
    page_title TEXT NOT NULL,
    local_path TEXT NOT NULL,
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    fetched_at_unix INTEGER NOT NULL,
    expires_at_unix INTEGER NOT NULL,
    PRIMARY KEY (doc_type, page_title)
);

CREATE INDEX IF NOT EXISTS idx_technical_docs_type ON technical_docs(doc_type);
CREATE INDEX IF NOT EXISTS idx_technical_docs_title ON technical_docs(page_title);
"#;

const DEFAULT_DOCS_API_URL: &str = "https://www.mediawiki.org/w/api.php";
const DEFAULT_INSTALLED_EXT_API_URL: &str = "https://wiki.remilia.org/api.php";
const DEFAULT_USER_AGENT: &str = "wikitool-rust/0.1 (+https://wiki.remilia.org)";
const DOCS_NAMESPACE_MAIN: i32 = 0;
const DOCS_CACHE_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
const DOCS_SUBPAGE_LIMIT_DEFAULT: usize = 100;
const DOCS_BUNDLE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

    fn subpage_prefix(self) -> &'static str {
        match self {
            Self::Hooks => "Manual:Hooks/",
            Self::Config => "Manual:$wg",
            Self::Api => "API:",
            Self::Manual => "Manual:",
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
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone)]
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
    pub imported_by_type: BTreeMap<String, usize>,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsStats {
    pub extension_count: usize,
    pub extension_pages_count: usize,
    pub technical_count: usize,
    pub technical_by_type: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtensionDocSummary {
    pub extension_name: String,
    pub source_wiki: String,
    pub version: Option<String>,
    pub pages_count: usize,
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
    pub extensions: Vec<ExtensionDocSummary>,
    pub technical: Vec<TechnicalDocSummary>,
    pub outdated: DocsOutdatedReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsUpdateReport {
    pub updated_extensions: usize,
    pub updated_technical_types: usize,
    pub updated_pages: usize,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocsRemoveKind {
    Extension,
    TechnicalType,
    TechnicalPage,
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
    pub title: String,
    pub snippet: String,
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
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteDocsPage {
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
    let api_url = env_value(
        "WIKITOOL_INSTALLED_EXTENSIONS_API_URL",
        &env_value("WIKI_API_URL", DEFAULT_INSTALLED_EXT_API_URL),
    );
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
    let mut failures = Vec::new();
    let mut needs_extension_fts_rebuild = false;
    let mut needs_technical_fts_rebuild = false;

    for extension in &bundle.extensions {
        let extension_name = normalize_extension_name(&extension.extension_name);
        if extension_name.is_empty() {
            failures.push("bundle extension entry with empty extension_name".to_string());
            continue;
        }

        let mut fetched_pages = Vec::new();
        for page in &extension.pages {
            let page_title = normalize_title(&page.page_title);
            if page_title.is_empty() {
                continue;
            }
            let content = page.content.clone();
            if content.trim().is_empty() {
                continue;
            }
            let local_path = page
                .local_path
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| extension_local_path(&extension_name, &page_title));
            fetched_pages.push(FetchedDocsPage {
                page_title,
                local_path,
                content,
            });
        }

        if fetched_pages.is_empty() {
            failures.push(format!(
                "{extension_name}: bundle entry has no usable pages"
            ));
            continue;
        }

        persist_extension_docs(
            paths,
            &extension_name,
            extension.source_wiki.as_deref().unwrap_or(&source),
            extension.version.as_deref(),
            &fetched_pages,
            now_unix,
            expires_at_unix,
            false,
        )?;
        imported_extensions += 1;
        imported_pages += fetched_pages.len();
        needs_extension_fts_rebuild = true;
    }

    let mut technical_pages_by_type = BTreeMap::<TechnicalDocType, Vec<FetchedDocsPage>>::new();
    for technical in &bundle.technical {
        let Some(doc_type) = TechnicalDocType::parse(&technical.doc_type) else {
            failures.push(format!(
                "bundle technical entry has unsupported doc_type `{}`",
                technical.doc_type
            ));
            continue;
        };

        for page in &technical.pages {
            let page_title = normalize_title(&page.page_title);
            if page_title.is_empty() || page.content.trim().is_empty() {
                continue;
            }
            let local_path = page
                .local_path
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| technical_local_path(doc_type, &page_title));
            technical_pages_by_type
                .entry(doc_type)
                .or_default()
                .push(FetchedDocsPage {
                    page_title,
                    local_path,
                    content: page.content.clone(),
                });
        }
    }

    for (doc_type, pages) in technical_pages_by_type {
        if pages.is_empty() {
            continue;
        }
        persist_technical_docs(
            paths,
            doc_type,
            &pages,
            now_unix,
            expires_at_unix,
            true,
            false,
        )?;
        imported_technical_types += 1;
        imported_pages += pages.len();
        needs_technical_fts_rebuild = true;
    }

    rebuild_docs_fts_indexes(
        paths,
        needs_extension_fts_rebuild,
        needs_technical_fts_rebuild,
    )?;

    Ok(DocsBundleImportReport {
        schema_version: bundle.schema_version,
        source,
        imported_extensions,
        imported_technical_types,
        imported_pages,
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
    let mut failures = Vec::new();
    let mut needs_extension_fts_rebuild = false;

    for extension in normalize_extensions(&options.extensions) {
        let main_page = format!("Extension:{extension}");
        let mut pages_to_fetch = vec![main_page.clone()];
        if options.include_subpages {
            let mut subpages = api.get_subpages(
                &format!("Extension:{extension}/"),
                DOCS_NAMESPACE_MAIN,
                usize::MAX,
            )?;
            pages_to_fetch.append(&mut subpages);
        }
        dedupe_titles_in_order(&mut pages_to_fetch);

        let mut fetched_pages = Vec::new();
        for title in &pages_to_fetch {
            match api.get_page(title) {
                Ok(Some(page)) => {
                    fetched_pages.push(FetchedDocsPage {
                        page_title: page.title,
                        local_path: extension_local_path(&extension, title),
                        content: page.content,
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    failures.push(format!("{extension}: failed to fetch {title}: {error}"));
                }
            }
        }

        if fetched_pages.is_empty() {
            failures.push(format!("{extension}: no pages fetched"));
            continue;
        }

        let version = fetched_pages
            .iter()
            .find(|page| page.page_title.eq_ignore_ascii_case(&main_page))
            .and_then(|page| extract_extension_version(&page.content));
        persist_extension_docs(
            paths,
            &extension,
            "mediawiki.org",
            version.as_deref(),
            &fetched_pages,
            now_unix,
            expires_at_unix,
            false,
        )?;

        imported_extensions += 1;
        imported_pages += fetched_pages.len();
        needs_extension_fts_rebuild = true;
    }

    rebuild_docs_fts_indexes(paths, needs_extension_fts_rebuild, false)?;

    Ok(DocsImportReport {
        requested_extensions,
        imported_extensions,
        imported_pages,
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
    let mut imported_by_type = BTreeMap::new();
    let mut failures = Vec::new();
    let mut needs_technical_fts_rebuild = false;

    for task in &options.tasks {
        let mut pages_to_fetch = Vec::new();
        if let Some(page_title) = task.page_title.as_deref() {
            let normalized = normalize_title(page_title);
            if !normalized.is_empty() {
                pages_to_fetch.push(normalized.clone());
                if task.include_subpages {
                    let mut subpages = api.get_subpages(
                        &format!("{normalized}/"),
                        DOCS_NAMESPACE_MAIN,
                        options.limit.max(1),
                    )?;
                    pages_to_fetch.append(&mut subpages);
                }
            }
        } else {
            pages_to_fetch.push(task.doc_type.main_page().to_string());
            if task.include_subpages {
                let mut subpages = api.get_subpages(
                    task.doc_type.subpage_prefix(),
                    DOCS_NAMESPACE_MAIN,
                    options.limit.max(1),
                )?;
                pages_to_fetch.append(&mut subpages);
            }
        }
        dedupe_titles_in_order(&mut pages_to_fetch);

        let mut fetched_pages = Vec::new();
        for title in &pages_to_fetch {
            match api.get_page(title) {
                Ok(Some(page)) => {
                    fetched_pages.push(FetchedDocsPage {
                        page_title: page.title,
                        local_path: technical_local_path(task.doc_type, title),
                        content: page.content,
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    failures.push(format!(
                        "{}: failed to fetch {title}: {error}",
                        task.doc_type.as_str()
                    ));
                }
            }
        }

        if fetched_pages.is_empty() {
            failures.push(format!(
                "{}: no pages fetched for task",
                task.doc_type.as_str()
            ));
            continue;
        }

        persist_technical_docs(
            paths,
            task.doc_type,
            &fetched_pages,
            now_unix,
            expires_at_unix,
            task.page_title.is_none(),
            false,
        )?;

        imported_pages += fetched_pages.len();
        *imported_by_type
            .entry(task.doc_type.as_str().to_string())
            .or_insert(0) += fetched_pages.len();
        needs_technical_fts_rebuild = true;
    }

    rebuild_docs_fts_indexes(paths, false, needs_technical_fts_rebuild)?;

    Ok(DocsImportTechnicalReport {
        requested_tasks: options.tasks.len(),
        imported_pages,
        imported_by_type,
        failures,
        request_count: api.request_count(),
    })
}

pub fn list_docs(paths: &ResolvedPaths, options: &DocsListOptions) -> Result<DocsListReport> {
    let connection = open_docs_connection(paths)?;
    initialize_docs_schema(&connection)?;
    let now_unix = unix_timestamp()?;
    let stats = load_docs_stats(&connection)?;
    let extensions = load_extension_docs(&connection, now_unix)?;
    let technical = load_technical_docs(&connection, options.technical_type.as_deref(), now_unix)?;
    let outdated = load_outdated_docs(&connection, now_unix)?;

    Ok(DocsListReport {
        now_unix,
        stats,
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
    initialize_docs_schema(&connection)?;
    let now_unix = unix_timestamp()?;
    let outdated = load_outdated_docs(&connection, now_unix)?;

    if outdated.extensions.is_empty() && outdated.technical.is_empty() {
        return Ok(DocsUpdateReport {
            updated_extensions: 0,
            updated_technical_types: 0,
            updated_pages: 0,
            failures: Vec::new(),
            request_count: api.request_count(),
        });
    }

    let mut updated_extensions = 0usize;
    let mut updated_technical_types = 0usize;
    let mut updated_pages = 0usize;
    let mut failures = Vec::new();

    if !outdated.extensions.is_empty() {
        let extension_names = outdated
            .extensions
            .iter()
            .map(|extension| extension.extension_name.clone())
            .collect::<Vec<_>>();
        let report = import_extension_docs_with_api(
            paths,
            &DocsImportOptions {
                extensions: extension_names,
                include_subpages: true,
            },
            api,
        )?;
        updated_extensions += report.imported_extensions;
        updated_pages += report.imported_pages;
        failures.extend(report.failures);
    }

    let mut technical_types = BTreeSet::new();
    for technical in &outdated.technical {
        if let Some(doc_type) = TechnicalDocType::parse(&technical.doc_type) {
            technical_types.insert(doc_type);
        }
    }

    let technical_tasks = technical_types
        .into_iter()
        .map(|doc_type| TechnicalImportTask {
            doc_type,
            page_title: None,
            include_subpages: true,
        })
        .collect::<Vec<_>>();
    if !technical_tasks.is_empty() {
        let report = import_technical_docs_with_api(
            paths,
            &DocsImportTechnicalOptions {
                tasks: technical_tasks,
                limit: DOCS_SUBPAGE_LIMIT_DEFAULT,
            },
            api,
        )?;
        updated_technical_types += report.imported_by_type.len();
        updated_pages += report.imported_pages;
        failures.extend(report.failures);
    }

    Ok(DocsUpdateReport {
        updated_extensions,
        updated_technical_types,
        updated_pages,
        failures,
        request_count: api.request_count(),
    })
}

pub fn remove_docs(paths: &ResolvedPaths, target: &str) -> Result<DocsRemoveReport> {
    let mut connection = open_docs_connection(paths)?;
    initialize_docs_schema(&connection)?;

    let normalized_target = normalize_title(target);
    if normalized_target.is_empty() {
        bail!("docs remove target is empty");
    }

    let extension_name = normalize_extension_name(&normalized_target);
    let has_extension: i64 = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM extension_docs WHERE lower(extension_name) = lower(?1))",
            params![extension_name],
            |row| row.get(0),
        )
        .context("failed to check extension docs existence")?;
    if has_extension == 1 {
        let tx = connection
            .transaction()
            .context("failed to start extension docs removal transaction")?;
        let page_rows = tx
            .execute(
                "DELETE FROM extension_doc_pages WHERE lower(extension_name) = lower(?1)",
                params![extension_name],
            )
            .context("failed to delete extension docs pages")?;
        tx.execute(
            "DELETE FROM extension_docs WHERE lower(extension_name) = lower(?1)",
            params![extension_name],
        )
        .context("failed to delete extension docs metadata")?;
        tx.commit()
            .context("failed to commit extension docs removal transaction")?;
        return Ok(DocsRemoveReport {
            kind: DocsRemoveKind::Extension,
            target: extension_name,
            removed_rows: page_rows,
        });
    }

    if let Some(doc_type) = TechnicalDocType::parse(&normalized_target) {
        let removed = connection
            .execute(
                "DELETE FROM technical_docs WHERE doc_type = ?1",
                params![doc_type.as_str()],
            )
            .with_context(|| {
                format!("failed to delete technical docs for {}", doc_type.as_str())
            })?;
        return Ok(DocsRemoveReport {
            kind: DocsRemoveKind::TechnicalType,
            target: doc_type.as_str().to_string(),
            removed_rows: removed,
        });
    }

    let removed = connection
        .execute(
            "DELETE FROM technical_docs WHERE lower(page_title) = lower(?1)",
            params![normalized_target],
        )
        .context("failed to delete technical docs by title")?;
    if removed > 0 {
        return Ok(DocsRemoveReport {
            kind: DocsRemoveKind::TechnicalPage,
            target: normalized_target,
            removed_rows: removed,
        });
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
    initialize_docs_schema(&connection)?;

    let normalized_query = collapse_whitespace(query).to_ascii_lowercase();
    if normalized_query.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let tier_filter = tier.map(str::trim).filter(|value| !value.is_empty());
    if let Some(value) = tier_filter
        && !value.eq_ignore_ascii_case("extension")
        && !value.eq_ignore_ascii_case("technical")
    {
        bail!("unsupported docs tier `{value}`; expected extension or technical");
    }

    let per_tier_limit = i64::try_from(limit).context("docs search limit does not fit into i64")?;
    let mut hits = Vec::new();

    let search_extension = tier_filter.is_none()
        || tier_filter.is_some_and(|value| value.eq_ignore_ascii_case("extension"));
    let search_technical = tier_filter.is_none()
        || tier_filter.is_some_and(|value| value.eq_ignore_ascii_case("technical"));

    // Try FTS5 first, fall back to LIKE if FTS tables don't exist
    let use_fts_ext = search_extension && fts_table_exists(&connection, "extension_doc_pages_fts");
    let use_fts_tech = search_technical && fts_table_exists(&connection, "technical_docs_fts");

    if search_extension {
        let ext_hits = if use_fts_ext {
            match search_extension_docs_fts(&connection, &normalized_query, per_tier_limit) {
                Ok(hits) if !hits.is_empty() => hits,
                Ok(_) | Err(_) => {
                    search_extension_docs_like(&connection, &normalized_query, per_tier_limit)?
                }
            }
        } else {
            search_extension_docs_like(&connection, &normalized_query, per_tier_limit)?
        };
        hits.extend(ext_hits);
    }

    if search_technical {
        let tech_hits = if use_fts_tech {
            match search_technical_docs_fts(&connection, &normalized_query, per_tier_limit) {
                Ok(hits) if !hits.is_empty() => hits,
                Ok(_) | Err(_) => {
                    search_technical_docs_like(&connection, &normalized_query, per_tier_limit)?
                }
            }
        } else {
            search_technical_docs_like(&connection, &normalized_query, per_tier_limit)?
        };
        hits.extend(tech_hits);
    }

    hits.sort_by(|left, right| {
        left.tier
            .cmp(&right.tier)
            .then_with(|| left.title.cmp(&right.title))
    });
    hits.truncate(limit);
    Ok(hits)
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

fn rebuild_docs_fts_indexes(
    paths: &ResolvedPaths,
    rebuild_extension_docs: bool,
    rebuild_technical_docs: bool,
) -> Result<()> {
    if !rebuild_extension_docs && !rebuild_technical_docs {
        return Ok(());
    }
    let connection = open_docs_connection(paths)?;
    initialize_docs_schema(&connection)?;

    if rebuild_extension_docs && fts_table_exists(&connection, "extension_doc_pages_fts") {
        connection
            .execute_batch(
                "INSERT INTO extension_doc_pages_fts(extension_doc_pages_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild extension_doc_pages_fts")?;
    }
    if rebuild_technical_docs && fts_table_exists(&connection, "technical_docs_fts") {
        connection
            .execute_batch("INSERT INTO technical_docs_fts(technical_docs_fts) VALUES('rebuild')")
            .context("failed to rebuild technical_docs_fts")?;
    }

    Ok(())
}

fn search_extension_docs_fts(
    connection: &Connection,
    normalized_query: &str,
    per_tier_limit: i64,
) -> Result<Vec<DocsSearchHit>> {
    let fts_query = format!("\"{normalized_query}\" *");
    let mut statement = connection
        .prepare(
            "SELECT edp.page_title, edp.content
             FROM extension_doc_pages_fts fts
             JOIN extension_doc_pages edp ON edp.rowid = fts.rowid
             WHERE extension_doc_pages_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .context("failed to prepare FTS extension docs search")?;
    let rows = statement
        .query_map(params![fts_query, per_tier_limit], |row| {
            let title: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok((title, content))
        })
        .context("failed to run FTS extension docs search")?;
    let mut hits = Vec::new();
    for row in rows {
        let (title, content) = row.context("failed to decode FTS extension docs row")?;
        hits.push(DocsSearchHit {
            tier: "extension".to_string(),
            title,
            snippet: make_snippet(&content, normalized_query),
        });
    }
    Ok(hits)
}

fn search_extension_docs_like(
    connection: &Connection,
    normalized_query: &str,
    per_tier_limit: i64,
) -> Result<Vec<DocsSearchHit>> {
    let pattern = format!("%{normalized_query}%");
    let mut statement = connection
        .prepare(
            "SELECT page_title, content
             FROM extension_doc_pages
             WHERE lower(page_title) LIKE ?1 OR lower(content) LIKE ?1
             ORDER BY page_title ASC
             LIMIT ?2",
        )
        .context("failed to prepare extension docs search query")?;
    let rows = statement
        .query_map(params![pattern, per_tier_limit], |row| {
            let title: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok((title, content))
        })
        .context("failed to query extension docs search rows")?;
    let mut hits = Vec::new();
    for row in rows {
        let (title, content) = row.context("failed to decode extension docs search row")?;
        hits.push(DocsSearchHit {
            tier: "extension".to_string(),
            title,
            snippet: make_snippet(&content, normalized_query),
        });
    }
    Ok(hits)
}

fn search_technical_docs_fts(
    connection: &Connection,
    normalized_query: &str,
    per_tier_limit: i64,
) -> Result<Vec<DocsSearchHit>> {
    let fts_query = format!("\"{normalized_query}\" *");
    let mut statement = connection
        .prepare(
            "SELECT td.page_title, td.content
             FROM technical_docs_fts fts
             JOIN technical_docs td ON td.rowid = fts.rowid
             WHERE technical_docs_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .context("failed to prepare FTS technical docs search")?;
    let rows = statement
        .query_map(params![fts_query, per_tier_limit], |row| {
            let title: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok((title, content))
        })
        .context("failed to run FTS technical docs search")?;
    let mut hits = Vec::new();
    for row in rows {
        let (title, content) = row.context("failed to decode FTS technical docs row")?;
        hits.push(DocsSearchHit {
            tier: "technical".to_string(),
            title,
            snippet: make_snippet(&content, normalized_query),
        });
    }
    Ok(hits)
}

fn search_technical_docs_like(
    connection: &Connection,
    normalized_query: &str,
    per_tier_limit: i64,
) -> Result<Vec<DocsSearchHit>> {
    let pattern = format!("%{normalized_query}%");
    let mut statement = connection
        .prepare(
            "SELECT page_title, content
             FROM technical_docs
             WHERE lower(page_title) LIKE ?1 OR lower(content) LIKE ?1
             ORDER BY page_title ASC
             LIMIT ?2",
        )
        .context("failed to prepare technical docs search query")?;
    let rows = statement
        .query_map(params![pattern, per_tier_limit], |row| {
            let title: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok((title, content))
        })
        .context("failed to query technical docs search rows")?;
    let mut hits = Vec::new();
    for row in rows {
        let (title, content) = row.context("failed to decode technical docs search row")?;
        hits.push(DocsSearchHit {
            tier: "technical".to_string(),
            title,
            snippet: make_snippet(&content, normalized_query),
        });
    }
    Ok(hits)
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
struct FetchedDocsPage {
    page_title: String,
    local_path: String,
    content: String,
}

#[allow(clippy::too_many_arguments)]
fn persist_extension_docs(
    paths: &ResolvedPaths,
    extension_name: &str,
    source_wiki: &str,
    version: Option<&str>,
    pages: &[FetchedDocsPage],
    fetched_at_unix: u64,
    expires_at_unix: u64,
    rebuild_fts_after_commit: bool,
) -> Result<()> {
    let mut connection = open_docs_connection(paths)?;
    initialize_docs_schema(&connection)?;
    let transaction = connection
        .transaction()
        .context("failed to start extension docs transaction")?;

    transaction
        .execute(
            "INSERT INTO extension_docs (
                extension_name,
                source_wiki,
                version,
                pages_count,
                fetched_at_unix,
                expires_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(extension_name) DO UPDATE SET
                source_wiki = excluded.source_wiki,
                version = excluded.version,
                pages_count = excluded.pages_count,
                fetched_at_unix = excluded.fetched_at_unix,
                expires_at_unix = excluded.expires_at_unix",
            params![
                extension_name,
                source_wiki,
                version,
                i64::try_from(pages.len()).context("pages count does not fit into i64")?,
                i64::try_from(fetched_at_unix).context("fetched_at_unix does not fit into i64")?,
                i64::try_from(expires_at_unix).context("expires_at_unix does not fit into i64")?,
            ],
        )
        .with_context(|| {
            format!("failed to upsert extension docs metadata for {extension_name}")
        })?;

    transaction
        .execute(
            "DELETE FROM extension_doc_pages WHERE extension_name = ?1",
            params![extension_name],
        )
        .with_context(|| format!("failed to clear extension docs pages for {extension_name}"))?;

    let mut statement = transaction
        .prepare(
            "INSERT INTO extension_doc_pages (
                extension_name,
                page_title,
                local_path,
                content,
                content_hash,
                fetched_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .context("failed to prepare extension docs page insert")?;

    for page in pages {
        statement
            .execute(params![
                extension_name,
                page.page_title,
                page.local_path,
                page.content,
                compute_hash(&page.content),
                i64::try_from(fetched_at_unix).context("fetched_at_unix does not fit into i64")?,
            ])
            .with_context(|| {
                format!(
                    "failed to insert extension docs page {} for {}",
                    page.page_title, extension_name
                )
            })?;
    }

    drop(statement);
    transaction
        .commit()
        .context("failed to commit extension docs transaction")?;

    if rebuild_fts_after_commit {
        rebuild_docs_fts_indexes(paths, true, false)?;
    }

    Ok(())
}

fn persist_technical_docs(
    paths: &ResolvedPaths,
    doc_type: TechnicalDocType,
    pages: &[FetchedDocsPage],
    fetched_at_unix: u64,
    expires_at_unix: u64,
    replace_existing_for_type: bool,
    rebuild_fts_after_commit: bool,
) -> Result<()> {
    let mut connection = open_docs_connection(paths)?;
    initialize_docs_schema(&connection)?;
    let transaction = connection
        .transaction()
        .context("failed to start technical docs transaction")?;

    if replace_existing_for_type {
        transaction
            .execute(
                "DELETE FROM technical_docs WHERE doc_type = ?1",
                params![doc_type.as_str()],
            )
            .with_context(|| {
                format!(
                    "failed to clear technical docs for type {}",
                    doc_type.as_str()
                )
            })?;
    }

    let mut statement = transaction
        .prepare(
            "INSERT INTO technical_docs (
                doc_type,
                page_title,
                local_path,
                content,
                content_hash,
                fetched_at_unix,
                expires_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(doc_type, page_title) DO UPDATE SET
                local_path = excluded.local_path,
                content = excluded.content,
                content_hash = excluded.content_hash,
                fetched_at_unix = excluded.fetched_at_unix,
                expires_at_unix = excluded.expires_at_unix",
        )
        .context("failed to prepare technical docs insert")?;

    for page in pages {
        statement
            .execute(params![
                doc_type.as_str(),
                page.page_title,
                page.local_path,
                page.content,
                compute_hash(&page.content),
                i64::try_from(fetched_at_unix).context("fetched_at_unix does not fit into i64")?,
                i64::try_from(expires_at_unix).context("expires_at_unix does not fit into i64")?,
            ])
            .with_context(|| {
                format!(
                    "failed to upsert technical docs page {} ({})",
                    page.page_title,
                    doc_type.as_str()
                )
            })?;
    }

    drop(statement);
    transaction
        .commit()
        .context("failed to commit technical docs transaction")?;

    if rebuild_fts_after_commit {
        rebuild_docs_fts_indexes(paths, false, true)?;
    }

    Ok(())
}

fn load_docs_stats(connection: &Connection) -> Result<DocsStats> {
    let extension_count = count_query(connection, "SELECT COUNT(*) FROM extension_docs")
        .context("failed to count extension docs")?;
    let extension_pages_count = count_query(connection, "SELECT COUNT(*) FROM extension_doc_pages")
        .context("failed to count extension docs pages")?;
    let technical_count = count_query(connection, "SELECT COUNT(*) FROM technical_docs")
        .context("failed to count technical docs")?;

    let mut technical_by_type = BTreeMap::new();
    let mut statement = connection
        .prepare(
            "SELECT doc_type, COUNT(*) as count
             FROM technical_docs
             GROUP BY doc_type
             ORDER BY doc_type ASC",
        )
        .context("failed to prepare technical-by-type stats query")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .context("failed to query technical-by-type stats")?;
    for row in rows {
        let (doc_type, count) = row.context("failed to decode technical-by-type row")?;
        technical_by_type.insert(
            doc_type,
            usize::try_from(count).context("negative technical-by-type count")?,
        );
    }

    Ok(DocsStats {
        extension_count,
        extension_pages_count,
        technical_count,
        technical_by_type,
    })
}

fn load_extension_docs(connection: &Connection, now_unix: u64) -> Result<Vec<ExtensionDocSummary>> {
    let mut statement = connection
        .prepare(
            "SELECT extension_name, source_wiki, version, pages_count, fetched_at_unix, expires_at_unix
             FROM extension_docs
             ORDER BY extension_name ASC",
        )
        .context("failed to prepare extension docs listing query")?;
    let rows = statement
        .query_map([], |row| {
            let pages_count: i64 = row.get(3)?;
            let fetched_at_unix: i64 = row.get(4)?;
            let expires_at_unix: i64 = row.get(5)?;
            Ok(ExtensionDocSummary {
                extension_name: row.get(0)?,
                source_wiki: row.get(1)?,
                version: row.get(2)?,
                pages_count: usize::try_from(pages_count).unwrap_or(0),
                fetched_at_unix: u64::try_from(fetched_at_unix).unwrap_or(0),
                expires_at_unix: u64::try_from(expires_at_unix).unwrap_or(0),
                expired: u64::try_from(expires_at_unix).unwrap_or(0) <= now_unix,
            })
        })
        .context("failed to query extension docs listing rows")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode extension docs listing row")?);
    }
    Ok(out)
}

fn load_technical_docs(
    connection: &Connection,
    doc_type: Option<&str>,
    now_unix: u64,
) -> Result<Vec<TechnicalDocSummary>> {
    let mut out = Vec::new();
    if let Some(doc_type) = doc_type {
        let normalized = normalize_title(doc_type).to_ascii_lowercase();
        let mut statement = connection
            .prepare(
                "SELECT doc_type, page_title, local_path, fetched_at_unix, expires_at_unix
                 FROM technical_docs
                 WHERE lower(doc_type) = ?1
                 ORDER BY page_title ASC",
            )
            .context("failed to prepare typed technical docs listing query")?;
        let rows = statement
            .query_map(params![normalized], |row| {
                let fetched_at_unix: i64 = row.get(3)?;
                let expires_at_unix: i64 = row.get(4)?;
                Ok(TechnicalDocSummary {
                    doc_type: row.get(0)?,
                    page_title: row.get(1)?,
                    local_path: row.get(2)?,
                    fetched_at_unix: u64::try_from(fetched_at_unix).unwrap_or(0),
                    expires_at_unix: u64::try_from(expires_at_unix).unwrap_or(0),
                    expired: u64::try_from(expires_at_unix).unwrap_or(0) <= now_unix,
                })
            })
            .context("failed to query typed technical docs listing rows")?;
        for row in rows {
            out.push(row.context("failed to decode typed technical docs listing row")?);
        }
        return Ok(out);
    }

    let mut statement = connection
        .prepare(
            "SELECT doc_type, page_title, local_path, fetched_at_unix, expires_at_unix
             FROM technical_docs
             ORDER BY doc_type ASC, page_title ASC",
        )
        .context("failed to prepare technical docs listing query")?;
    let rows = statement
        .query_map([], |row| {
            let fetched_at_unix: i64 = row.get(3)?;
            let expires_at_unix: i64 = row.get(4)?;
            Ok(TechnicalDocSummary {
                doc_type: row.get(0)?,
                page_title: row.get(1)?,
                local_path: row.get(2)?,
                fetched_at_unix: u64::try_from(fetched_at_unix).unwrap_or(0),
                expires_at_unix: u64::try_from(expires_at_unix).unwrap_or(0),
                expired: u64::try_from(expires_at_unix).unwrap_or(0) <= now_unix,
            })
        })
        .context("failed to query technical docs listing rows")?;
    for row in rows {
        out.push(row.context("failed to decode technical docs listing row")?);
    }
    Ok(out)
}

fn load_outdated_docs(connection: &Connection, now_unix: u64) -> Result<DocsOutdatedReport> {
    let now_i64 = i64::try_from(now_unix).context("now_unix does not fit into i64")?;

    let mut extensions = Vec::new();
    let mut ext_statement = connection
        .prepare(
            "SELECT extension_name, expires_at_unix
             FROM extension_docs
             WHERE expires_at_unix <= ?1
             ORDER BY extension_name ASC",
        )
        .context("failed to prepare outdated extensions query")?;
    let ext_rows = ext_statement
        .query_map(params![now_i64], |row| {
            let expires_at_unix: i64 = row.get(1)?;
            Ok(OutdatedExtensionDoc {
                extension_name: row.get(0)?,
                expires_at_unix: u64::try_from(expires_at_unix).unwrap_or(0),
            })
        })
        .context("failed to query outdated extensions")?;
    for row in ext_rows {
        extensions.push(row.context("failed to decode outdated extension row")?);
    }

    let mut technical = Vec::new();
    let mut tech_statement = connection
        .prepare(
            "SELECT doc_type, page_title, expires_at_unix
             FROM technical_docs
             WHERE expires_at_unix <= ?1
             ORDER BY doc_type ASC, page_title ASC",
        )
        .context("failed to prepare outdated technical docs query")?;
    let tech_rows = tech_statement
        .query_map(params![now_i64], |row| {
            let expires_at_unix: i64 = row.get(2)?;
            Ok(OutdatedTechnicalDoc {
                doc_type: row.get(0)?,
                page_title: row.get(1)?,
                expires_at_unix: u64::try_from(expires_at_unix).unwrap_or(0),
            })
        })
        .context("failed to query outdated technical docs")?;
    for row in tech_rows {
        technical.push(row.context("failed to decode outdated technical row")?);
    }

    Ok(DocsOutdatedReport {
        extensions,
        technical,
    })
}

fn open_docs_connection(paths: &ResolvedPaths) -> Result<Connection> {
    ensure_db_parent(paths)?;
    let connection = Connection::open(&paths.db_path)
        .with_context(|| format!("failed to open {}", paths.db_path.display()))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .context("failed to set sqlite busy timeout")?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .context("failed to enable foreign_keys pragma")?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .context("failed to enable WAL journal mode")?;
    Ok(connection)
}

fn initialize_docs_schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(DOCS_SCHEMA_SQL)
        .context("failed to initialize docs schema")
}

fn ensure_db_parent(paths: &ResolvedPaths) -> Result<()> {
    let parent = paths
        .db_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("db path has no parent: {}", paths.db_path.display()))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create database parent directory {}",
            parent.display()
        )
    })
}

fn count_query(connection: &Connection, sql: &str) -> Result<usize> {
    let count: i64 = connection
        .query_row(sql, [], |row| row.get(0))
        .with_context(|| format!("failed to execute count query: {sql}"))?;
    usize::try_from(count).context("count query returned negative value")
}

fn normalize_extensions(extensions: &[String]) -> Vec<String> {
    let mut out = BTreeSet::new();
    for extension in extensions {
        let normalized = normalize_extension_name(extension);
        if !normalized.is_empty() {
            out.insert(normalized);
        }
    }
    out.into_iter().collect()
}

fn normalize_extension_name(value: &str) -> String {
    let normalized = normalize_title(value);
    if normalized.len() >= "Extension:".len() && normalized[..10].eq_ignore_ascii_case("Extension:")
    {
        return normalize_title(&normalized[10..]);
    }
    normalized
}

fn normalize_title(value: &str) -> String {
    collapse_whitespace(&value.replace('_', " "))
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

fn make_snippet(content: &str, lowered_query: &str) -> String {
    let normalized = collapse_whitespace(content);
    if normalized.is_empty() {
        return "<empty>".to_string();
    }
    let lowered = normalized.to_ascii_lowercase();
    let Some(index) = lowered.find(lowered_query) else {
        return truncate_text(&normalized, 200);
    };

    let start = clamp_to_char_boundary(&normalized, index.saturating_sub(80));
    let end = clamp_to_char_boundary(
        &normalized,
        index
            .saturating_add(lowered_query.len())
            .saturating_add(120)
            .min(normalized.len()),
    );
    let mut snippet = normalized[start..end].trim().to_string();
    if start > 0 {
        snippet = format!("...{snippet}");
    }
    if end < normalized.len() {
        snippet.push_str("...");
    }
    snippet
}

fn truncate_text(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let end = clamp_to_char_boundary(value, max_len);
    format!("{}...", &value[..end])
}

fn clamp_to_char_boundary(value: &str, mut index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    while !value.is_char_boundary(index) {
        index = index.saturating_sub(1);
    }
    index
}

fn collapse_whitespace(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
                previous_was_space = true;
            }
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }
    output.trim().to_string()
}

fn compute_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(content.as_bytes());
    let mut output = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn wait_retry_delay(retry_delay_ms: u64, attempt: usize) {
    let exponent = u32::try_from(attempt).unwrap_or(8).min(8);
    let scale = 1u64.checked_shl(exponent).unwrap_or(256);
    let base = retry_delay_ms.saturating_mul(scale);
    let jitter = (u64::try_from(attempt).unwrap_or(0) * 17 + 31) % 97;
    sleep(Duration::from_millis(base.saturating_add(jitter)));
}

fn unix_timestamp() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")
        .map(|duration| duration.as_secs())
}

fn env_value(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_value_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_value_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(default)
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::{TempDir, tempdir};

    use crate::runtime::{
        InitOptions, PathOverrides, ResolutionContext, init_layout, resolve_paths,
    };

    use super::{
        DocsApi, DocsBundle, DocsBundleExtension, DocsBundlePage, DocsBundleTechnical,
        DocsImportOptions, DocsImportTechnicalOptions, DocsListOptions, RemoteDocsPage,
        TechnicalDocType, TechnicalImportTask, import_docs_bundle, import_extension_docs_with_api,
        import_technical_docs_with_api, open_docs_connection, remove_docs, search_docs,
        update_outdated_docs_with_api,
    };

    struct TestRuntime {
        _temp: TempDir,
        paths: crate::runtime::ResolvedPaths,
    }

    impl TestRuntime {
        fn new() -> anyhow::Result<Self> {
            let temp = tempdir().expect("tempdir");
            let root = temp.path().join("project");
            fs::create_dir_all(&root).expect("create root");
            let context = ResolutionContext {
                cwd: root.clone(),
                executable_dir: None,
            };
            let paths = resolve_paths(
                &context,
                &PathOverrides {
                    project_root: Some(root.clone()),
                    ..PathOverrides::default()
                },
            )?;
            init_layout(
                &paths,
                &InitOptions {
                    include_templates: true,
                    ..InitOptions::default()
                },
            )?;
            Ok(Self { _temp: temp, paths })
        }
    }

    struct MockDocsApi {
        pages: BTreeMap<String, String>,
        prefixes: BTreeMap<String, Vec<String>>,
        request_count: usize,
    }

    impl MockDocsApi {
        fn new() -> Self {
            Self {
                pages: BTreeMap::new(),
                prefixes: BTreeMap::new(),
                request_count: 0,
            }
        }

        fn with_page(mut self, title: &str, content: &str) -> Self {
            self.pages.insert(title.to_string(), content.to_string());
            self
        }

        fn with_prefix(mut self, prefix: &str, titles: Vec<&str>) -> Self {
            self.prefixes.insert(
                prefix.to_string(),
                titles.into_iter().map(str::to_string).collect(),
            );
            self
        }
    }

    impl DocsApi for MockDocsApi {
        fn get_subpages(
            &mut self,
            prefix: &str,
            _namespace: i32,
            limit: usize,
        ) -> anyhow::Result<Vec<String>> {
            self.request_count += 1;
            let mut out = self.prefixes.get(prefix).cloned().unwrap_or_default();
            out.truncate(limit);
            Ok(out)
        }

        fn get_page(&mut self, title: &str) -> anyhow::Result<Option<RemoteDocsPage>> {
            self.request_count += 1;
            Ok(self.pages.get(title).map(|content| RemoteDocsPage {
                title: title.to_string(),
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                content: content.clone(),
            }))
        }

        fn request_count(&self) -> usize {
            self.request_count
        }
    }

    #[test]
    fn import_extension_and_technical_docs_and_search_roundtrip() {
        let runtime = TestRuntime::new().expect("runtime");
        let mut api = MockDocsApi::new()
            .with_page(
                "Extension:CirrusSearch",
                "{{Extension\n|version=1.2.3\n}}\nAdds search.",
            )
            .with_page(
                "Extension:CirrusSearch/Configuration",
                "Configuration details for CirrusSearch.",
            )
            .with_page("Manual:Hooks", "Hooks entry page.")
            .with_page(
                "Manual:Hooks/PageContentSave",
                "Hook docs with parser output.",
            )
            .with_prefix(
                "Extension:CirrusSearch/",
                vec!["Extension:CirrusSearch/Configuration"],
            )
            .with_prefix("Manual:Hooks/", vec!["Manual:Hooks/PageContentSave"]);

        let ext_report = import_extension_docs_with_api(
            &runtime.paths,
            &DocsImportOptions {
                extensions: vec!["CirrusSearch".to_string()],
                include_subpages: true,
            },
            &mut api,
        )
        .expect("import extension docs");
        assert_eq!(ext_report.imported_extensions, 1);
        assert_eq!(ext_report.imported_pages, 2);

        let tech_report = import_technical_docs_with_api(
            &runtime.paths,
            &DocsImportTechnicalOptions {
                tasks: vec![TechnicalImportTask {
                    doc_type: TechnicalDocType::Hooks,
                    page_title: None,
                    include_subpages: true,
                }],
                limit: 50,
            },
            &mut api,
        )
        .expect("import technical docs");
        assert_eq!(tech_report.imported_pages, 2);

        let listing =
            super::list_docs(&runtime.paths, &DocsListOptions::default()).expect("list docs");
        assert_eq!(listing.stats.extension_count, 1);
        assert_eq!(listing.stats.extension_pages_count, 2);
        assert_eq!(listing.stats.technical_count, 2);
        assert!(
            listing
                .extensions
                .iter()
                .any(|item| item.extension_name == "CirrusSearch")
        );

        let hits = search_docs(&runtime.paths, "parser", None, 10).expect("search docs");
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|hit| hit.tier == "technical"));
    }

    #[test]
    fn update_outdated_docs_refreshes_entries() {
        let runtime = TestRuntime::new().expect("runtime");
        let mut api = MockDocsApi::new()
            .with_page("Extension:Echo", "{{Extension|version=2.0}}")
            .with_page("Manual:Hooks", "Hooks main page")
            .with_prefix("Extension:Echo/", vec![])
            .with_prefix("Manual:Hooks/", vec![]);

        import_extension_docs_with_api(
            &runtime.paths,
            &DocsImportOptions {
                extensions: vec!["Echo".to_string()],
                include_subpages: true,
            },
            &mut api,
        )
        .expect("import extension docs");
        import_technical_docs_with_api(
            &runtime.paths,
            &DocsImportTechnicalOptions {
                tasks: vec![TechnicalImportTask {
                    doc_type: TechnicalDocType::Hooks,
                    page_title: None,
                    include_subpages: true,
                }],
                limit: 20,
            },
            &mut api,
        )
        .expect("import technical docs");

        let connection = open_docs_connection(&runtime.paths).expect("open docs connection");
        connection
            .execute_batch(
                "UPDATE extension_docs SET expires_at_unix = 1;
                 UPDATE technical_docs SET expires_at_unix = 1;",
            )
            .expect("force outdated");

        let report = update_outdated_docs_with_api(&runtime.paths, &mut api).expect("update docs");
        assert!(report.updated_extensions >= 1);
        assert!(report.updated_technical_types >= 1);
    }

    #[test]
    fn remove_docs_supports_extension_and_doc_type() {
        let runtime = TestRuntime::new().expect("runtime");
        let mut api = MockDocsApi::new()
            .with_page("Extension:Popups", "Popups page")
            .with_page("Manual:Contents", "Manual page")
            .with_prefix("Extension:Popups/", vec![])
            .with_prefix("Manual:", vec![]);

        import_extension_docs_with_api(
            &runtime.paths,
            &DocsImportOptions {
                extensions: vec!["Popups".to_string()],
                include_subpages: true,
            },
            &mut api,
        )
        .expect("import extension docs");
        import_technical_docs_with_api(
            &runtime.paths,
            &DocsImportTechnicalOptions {
                tasks: vec![TechnicalImportTask {
                    doc_type: TechnicalDocType::Manual,
                    page_title: None,
                    include_subpages: false,
                }],
                limit: 10,
            },
            &mut api,
        )
        .expect("import technical docs");

        let removed_extension =
            remove_docs(&runtime.paths, "Extension:Popups").expect("remove extension");
        assert!(matches!(
            removed_extension.kind,
            super::DocsRemoveKind::Extension
        ));
        assert!(removed_extension.removed_rows >= 1);

        let removed_type = remove_docs(&runtime.paths, "manual").expect("remove type");
        assert!(matches!(
            removed_type.kind,
            super::DocsRemoveKind::TechnicalType
        ));
        assert!(removed_type.removed_rows >= 1);
    }

    #[test]
    fn import_docs_bundle_populates_extension_and_technical_rows() {
        let runtime = TestRuntime::new().expect("runtime");
        let bundle_path = runtime.paths.state_dir.join("bundle.json");
        let bundle = DocsBundle {
            schema_version: 1,
            generated_at_unix: Some(1_739_000_000),
            source: Some("ai_pack".to_string()),
            extensions: vec![DocsBundleExtension {
                extension_name: "ParserFunctions".to_string(),
                source_wiki: Some("mediawiki.org".to_string()),
                version: Some("stable".to_string()),
                pages: vec![DocsBundlePage {
                    page_title: "Extension:ParserFunctions".to_string(),
                    content: "ParserFunctions content".to_string(),
                    local_path: None,
                }],
            }],
            technical: vec![DocsBundleTechnical {
                doc_type: "manual".to_string(),
                pages: vec![DocsBundlePage {
                    page_title: "Manual:Remilia AI/Writing Guide".to_string(),
                    content: "Precomposed writing guidance".to_string(),
                    local_path: None,
                }],
            }],
        };
        fs::write(
            &bundle_path,
            serde_json::to_string_pretty(&bundle).expect("bundle json"),
        )
        .expect("write bundle");

        let report = import_docs_bundle(&runtime.paths, &bundle_path).expect("import bundle");
        assert_eq!(report.imported_extensions, 1);
        assert_eq!(report.imported_technical_types, 1);
        assert_eq!(report.imported_pages, 2);
        assert!(report.failures.is_empty());

        let listing =
            super::list_docs(&runtime.paths, &DocsListOptions::default()).expect("list docs");
        assert_eq!(listing.stats.extension_count, 1);
        assert_eq!(listing.stats.technical_count, 1);
    }
}
