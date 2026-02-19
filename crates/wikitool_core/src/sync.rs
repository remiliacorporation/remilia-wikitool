use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::{StatusCode, Url};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::phase1::ResolvedPaths;
use crate::phase2::{ScanOptions, scan_files, title_to_relative_path, validate_scoped_path};
use crate::phase3::{RebuildReport, rebuild_index};

const SYNC_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sync_ledger_pages (
    title TEXT PRIMARY KEY,
    namespace INTEGER NOT NULL,
    relative_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    wiki_modified_at TEXT,
    revision_id INTEGER,
    page_id INTEGER,
    is_redirect INTEGER NOT NULL,
    redirect_target TEXT,
    last_synced_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sync_ledger_pages_namespace ON sync_ledger_pages(namespace);
CREATE INDEX IF NOT EXISTS idx_sync_ledger_pages_relative_path ON sync_ledger_pages(relative_path);

CREATE TABLE IF NOT EXISTS sync_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

pub const NS_MAIN: i32 = 0;
pub const NS_CATEGORY: i32 = 14;
pub const NS_TEMPLATE: i32 = 10;
pub const NS_MODULE: i32 = 828;
pub const NS_MEDIAWIKI: i32 = 8;

#[derive(Debug, Clone)]
pub struct PullOptions {
    pub namespaces: Vec<i32>,
    pub category: Option<String>,
    pub full: bool,
    pub overwrite_local: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullPageResult {
    pub title: String,
    pub action: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullReport {
    pub success: bool,
    pub requested_pages: usize,
    pub pulled: usize,
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
    pub pages: Vec<PullPageResult>,
    pub request_count: usize,
    pub reindex: Option<RebuildReport>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffChangeType {
    NewLocal,
    ModifiedLocal,
    DeletedLocal,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffChange {
    pub title: String,
    pub change_type: DiffChangeType,
    pub relative_path: String,
    pub local_hash: Option<String>,
    pub synced_hash: Option<String>,
    pub synced_wiki_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffReport {
    pub new_local: usize,
    pub modified_local: usize,
    pub deleted_local: usize,
    pub changes: Vec<DiffChange>,
}

#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    pub include_templates: bool,
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

#[derive(Debug, Clone)]
struct SyncLedgerEntry {
    title: String,
    namespace: i32,
    relative_path: String,
    content_hash: String,
    wiki_modified_at: Option<String>,
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

#[derive(Debug, Clone)]
pub struct MediaWikiClientConfig {
    pub api_url: String,
    pub user_agent: String,
    pub timeout_ms: u64,
    pub rate_limit_read_ms: u64,
    pub max_retries: usize,
    pub retry_delay_ms: u64,
}

impl MediaWikiClientConfig {
    pub fn from_env() -> Self {
        Self {
            api_url: env_value("WIKI_API_URL", "https://wiki.remilia.org/api.php"),
            user_agent: env_value(
                "WIKI_USER_AGENT",
                "wikitool-rust/0.1 (+https://wiki.remilia.org)",
            ),
            timeout_ms: env_value_u64("WIKI_HTTP_TIMEOUT_MS", 30_000),
            rate_limit_read_ms: env_value_u64("WIKI_RATE_LIMIT_READ", 300),
            max_retries: env_value_usize("WIKI_HTTP_RETRIES", 2),
            retry_delay_ms: env_value_u64("WIKI_HTTP_RETRY_DELAY_MS", 500),
        }
    }
}

pub struct MediaWikiClient {
    client: Client,
    config: MediaWikiClientConfig,
    last_request_at: Option<Instant>,
    request_count: usize,
}

impl MediaWikiClient {
    pub fn from_env() -> Result<Self> {
        Self::new(MediaWikiClientConfig::from_env())
    }

    pub fn new(config: MediaWikiClientConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .context("failed to build MediaWiki HTTP client")?;

        Ok(Self {
            client,
            config,
            last_request_at: None,
            request_count: 0,
        })
    }

    fn request_json(&mut self, params: &[(&str, String)]) -> Result<Value> {
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
                        self.wait_before_retry(attempt);
                        continue;
                    }
                    return Err(error).context("failed to call MediaWiki API");
                }
            }
        }

        bail!("MediaWiki API request exhausted retry budget")
    }

    fn apply_rate_limit(&mut self) {
        let delay = Duration::from_millis(self.config.rate_limit_read_ms);
        if let Some(last) = self.last_request_at {
            let elapsed = last.elapsed();
            if elapsed < delay {
                sleep(delay - elapsed);
            }
        }
        self.last_request_at = Some(Instant::now());
        self.request_count += 1;
    }

    fn wait_before_retry(&self, attempt: usize) {
        let exponent = u32::try_from(attempt).unwrap_or(16);
        let base = self
            .config
            .retry_delay_ms
            .saturating_mul(2u64.saturating_pow(exponent));
        let jitter = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::from(duration.subsec_millis() % 100))
            .unwrap_or(0);
        sleep(Duration::from_millis(base.saturating_add(jitter)));
    }
}

impl WikiReadApi for MediaWikiClient {
    fn get_all_pages(&mut self, namespace: i32) -> Result<Vec<String>> {
        let mut titles = Vec::new();
        let mut continue_token: Option<String> = None;

        loop {
            let mut params = vec![
                ("action", "query".to_string()),
                ("list", "allpages".to_string()),
                ("apnamespace", namespace.to_string()),
                ("aplimit", "500".to_string()),
            ];
            if let Some(token) = &continue_token {
                params.push(("apcontinue", token.clone()));
            }

            let response = self.request_json(&params)?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode allpages API response")?;

            for item in parsed.query.allpages {
                titles.push(item.title);
            }

            continue_token = parsed.continuation.and_then(|cont| cont.apcontinue);
            if continue_token.is_none() {
                break;
            }
        }

        Ok(titles)
    }

    fn get_category_members(&mut self, category: &str) -> Result<Vec<String>> {
        let mut titles = Vec::new();
        let mut continue_token: Option<String> = None;
        let category_title = if category.starts_with("Category:") {
            category.to_string()
        } else {
            format!("Category:{category}")
        };

        loop {
            let mut params = vec![
                ("action", "query".to_string()),
                ("list", "categorymembers".to_string()),
                ("cmtitle", category_title.clone()),
                ("cmtype", "page".to_string()),
                ("cmlimit", "500".to_string()),
            ];
            if let Some(token) = &continue_token {
                params.push(("cmcontinue", token.clone()));
            }

            let response = self.request_json(&params)?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode categorymembers API response")?;
            for item in parsed.query.categorymembers {
                titles.push(item.title);
            }

            continue_token = parsed.continuation.and_then(|cont| cont.cmcontinue);
            if continue_token.is_none() {
                break;
            }
        }

        Ok(titles)
    }

    fn get_recent_changes(&mut self, since: &str, namespaces: &[i32]) -> Result<Vec<String>> {
        let mut titles = BTreeSet::new();
        let mut continue_token: Option<String> = None;
        let namespace_filter = namespaces
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("|");

        loop {
            let mut params = vec![
                ("action", "query".to_string()),
                ("list", "recentchanges".to_string()),
                ("rcstart", since.to_string()),
                ("rcdir", "newer".to_string()),
                ("rcnamespace", namespace_filter.clone()),
                ("rcprop", "title".to_string()),
                ("rclimit", "500".to_string()),
                ("rctype", "edit|new".to_string()),
            ];
            if let Some(token) = &continue_token {
                params.push(("rccontinue", token.clone()));
            }

            let response = self.request_json(&params)?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode recentchanges API response")?;
            for item in parsed.query.recentchanges {
                titles.insert(item.title);
            }
            continue_token = parsed.continuation.and_then(|cont| cont.rccontinue);
            if continue_token.is_none() {
                break;
            }
        }

        Ok(titles.into_iter().collect())
    }

    fn get_page_contents(&mut self, titles: &[String]) -> Result<Vec<RemotePage>> {
        let mut results = Vec::new();
        for batch in titles.chunks(50) {
            let params = vec![
                ("action", "query".to_string()),
                ("titles", batch.join("|")),
                ("prop", "revisions".to_string()),
                ("rvprop", "content|timestamp|ids".to_string()),
                ("rvslots", "main".to_string()),
            ];

            let response = self.request_json(&params)?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode page content API response")?;

            for page in parsed.query.pages {
                if page.missing.unwrap_or(false) {
                    continue;
                }
                let revision = match page.revisions.first() {
                    Some(revision) => revision,
                    None => continue,
                };
                let slot = match revision
                    .slots
                    .as_ref()
                    .and_then(|slots| slots.main.as_ref())
                {
                    Some(slot) => slot,
                    None => continue,
                };
                let page_id = match page.pageid {
                    Some(value) => value,
                    None => continue,
                };

                results.push(RemotePage {
                    title: page.title,
                    namespace: page.ns,
                    page_id,
                    revision_id: revision.revid,
                    timestamp: revision.timestamp.clone(),
                    content: slot.content.clone(),
                });
            }
        }
        Ok(results)
    }

    fn search(
        &mut self,
        query: &str,
        namespaces: &[i32],
        limit: usize,
    ) -> Result<Vec<ExternalSearchHit>> {
        let namespace_filter = namespaces
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("|");
        let params = vec![
            ("action", "query".to_string()),
            ("list", "search".to_string()),
            ("srsearch", query.to_string()),
            ("srnamespace", namespace_filter),
            ("srlimit", limit.to_string()),
        ];

        let response = self.request_json(&params)?;
        let parsed: QueryResponse =
            serde_json::from_value(response).context("failed to decode search API response")?;

        let mut hits = Vec::new();
        for item in parsed.query.search {
            hits.push(ExternalSearchHit {
                title: item.title,
                namespace: item.ns,
                page_id: item.pageid,
                word_count: item.wordcount.and_then(|value| u64::try_from(value).ok()),
                snippet: item.snippet.unwrap_or_default(),
                timestamp: item.timestamp,
            });
        }
        Ok(hits)
    }

    fn request_count(&self) -> usize {
        self.request_count
    }
}

pub fn pull_from_remote(paths: &ResolvedPaths, options: &PullOptions) -> Result<PullReport> {
    let mut client = MediaWikiClient::from_env()?;
    pull_from_remote_with_api(paths, options, &mut client)
}

pub fn search_external_wiki(
    query: &str,
    namespaces: &[i32],
    limit: usize,
) -> Result<Vec<ExternalSearchHit>> {
    let mut client = MediaWikiClient::from_env()?;
    client.search(query, namespaces, limit)
}

pub fn diff_local_against_sync(
    paths: &ResolvedPaths,
    options: &DiffOptions,
) -> Result<Option<DiffReport>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }
    let connection = open_sync_connection(paths)?;
    if !table_exists(&connection, "sync_ledger_pages")? {
        return Ok(None);
    }

    let local_files = scan_files(
        paths,
        &ScanOptions {
            include_content: true,
            include_templates: options.include_templates,
        },
    )?;
    let ledger = load_sync_ledger_map(&connection, options.include_templates)?;

    let mut local_map = BTreeMap::new();
    for file in local_files {
        local_map.insert(normalized_title_key(&file.title), file);
    }

    let mut changes = Vec::new();
    for file in local_map.values() {
        let key = normalized_title_key(&file.title);
        match ledger.get(&key) {
            None => changes.push(DiffChange {
                title: file.title.clone(),
                change_type: DiffChangeType::NewLocal,
                relative_path: file.relative_path.clone(),
                local_hash: Some(file.content_hash.clone()),
                synced_hash: None,
                synced_wiki_timestamp: None,
            }),
            Some(entry) if entry.content_hash != file.content_hash => changes.push(DiffChange {
                title: file.title.clone(),
                change_type: DiffChangeType::ModifiedLocal,
                relative_path: file.relative_path.clone(),
                local_hash: Some(file.content_hash.clone()),
                synced_hash: Some(entry.content_hash.clone()),
                synced_wiki_timestamp: entry.wiki_modified_at.clone(),
            }),
            Some(_) => {}
        }
    }

    for entry in ledger.values() {
        let key = normalized_title_key(&entry.title);
        if !local_map.contains_key(&key) {
            changes.push(DiffChange {
                title: entry.title.clone(),
                change_type: DiffChangeType::DeletedLocal,
                relative_path: entry.relative_path.clone(),
                local_hash: None,
                synced_hash: Some(entry.content_hash.clone()),
                synced_wiki_timestamp: entry.wiki_modified_at.clone(),
            });
        }
    }

    changes.sort_by(|left, right| {
        change_order(&left.change_type)
            .cmp(&change_order(&right.change_type))
            .then(left.title.cmp(&right.title))
    });

    let new_local = changes
        .iter()
        .filter(|item| item.change_type == DiffChangeType::NewLocal)
        .count();
    let modified_local = changes
        .iter()
        .filter(|item| item.change_type == DiffChangeType::ModifiedLocal)
        .count();
    let deleted_local = changes
        .iter()
        .filter(|item| item.change_type == DiffChangeType::DeletedLocal)
        .count();

    Ok(Some(DiffReport {
        new_local,
        modified_local,
        deleted_local,
        changes,
    }))
}

fn pull_from_remote_with_api<A: WikiReadApi>(
    paths: &ResolvedPaths,
    options: &PullOptions,
    api: &mut A,
) -> Result<PullReport> {
    let connection = open_sync_connection(paths)?;
    initialize_sync_schema(&connection)?;

    let mut report = PullReport {
        success: true,
        requested_pages: 0,
        pulled: 0,
        created: 0,
        updated: 0,
        skipped: 0,
        errors: Vec::new(),
        pages: Vec::new(),
        request_count: 0,
        reindex: None,
    };

    let pages_to_pull = resolve_pages_to_pull(&connection, options, api)?;
    report.requested_pages = pages_to_pull.len();
    if pages_to_pull.is_empty() {
        report.request_count = api.request_count();
        return Ok(report);
    }

    let content_rows = api.get_page_contents(&pages_to_pull)?;
    let mut content_by_title = BTreeMap::new();
    for page in content_rows {
        content_by_title.insert(normalized_title_key(&page.title), page);
    }
    let mut ledger_by_title = load_sync_ledger_map(&connection, true)?;

    let mut wrote_files = false;
    let mut max_timestamp: Option<String> = None;

    for title in &pages_to_pull {
        let key = normalized_title_key(title);
        let page = match content_by_title.get(&key) {
            Some(page) => page,
            None => {
                let message = format!("{title}: page content missing in API response");
                report.errors.push(message);
                report.pages.push(PullPageResult {
                    title: title.clone(),
                    action: "error".to_string(),
                    detail: Some("missing content".to_string()),
                });
                continue;
            }
        };

        if max_timestamp
            .as_ref()
            .is_none_or(|current| page.timestamp > *current)
        {
            max_timestamp = Some(page.timestamp.clone());
        }

        let (is_redirect, redirect_target) = parse_redirect(&page.content);
        let relative_path = title_to_relative_path(paths, &page.title, is_redirect);
        let absolute_path = absolute_path_from_relative(paths, &relative_path);
        validate_scoped_path(paths, &absolute_path)?;
        ensure_parent_dir(&absolute_path)?;

        let remote_hash = compute_hash(&page.content);
        let ledger_entry = ledger_by_title.get(&key).cloned();
        remove_stale_synced_path_if_safe(
            paths,
            &ledger_entry,
            &relative_path,
            options.overwrite_local,
        )?;

        let local_content = fs::read_to_string(&absolute_path).ok();
        let local_hash = local_content.as_deref().map(compute_hash);

        let local_modified = match (&local_hash, &ledger_entry) {
            (Some(local_hash), Some(entry)) => local_hash != &entry.content_hash,
            (Some(_), None) => true,
            (None, _) => false,
        };

        if let Some(local_hash) = &local_hash
            && local_hash == &remote_hash
        {
            upsert_sync_ledger(
                &connection,
                page,
                &relative_path,
                &remote_hash,
                is_redirect,
                redirect_target.as_deref(),
            )?;
            ledger_by_title.insert(
                key.clone(),
                SyncLedgerEntry {
                    title: page.title.clone(),
                    namespace: page.namespace,
                    relative_path: relative_path.clone(),
                    content_hash: remote_hash,
                    wiki_modified_at: Some(page.timestamp.clone()),
                },
            );
            report.skipped += 1;
            report.pulled += 1;
            report.pages.push(PullPageResult {
                title: page.title.clone(),
                action: "skipped".to_string(),
                detail: Some("unchanged".to_string()),
            });
            continue;
        }

        if local_modified && !options.overwrite_local {
            report.skipped += 1;
            report.pages.push(PullPageResult {
                title: page.title.clone(),
                action: "skipped".to_string(),
                detail: Some("local content differs (use --overwrite-local)".to_string()),
            });
            continue;
        }

        let existed_before = absolute_path.exists();
        fs::write(&absolute_path, &page.content)
            .with_context(|| format!("failed to write {}", absolute_path.display()))?;
        wrote_files = true;
        upsert_sync_ledger(
            &connection,
            page,
            &relative_path,
            &remote_hash,
            is_redirect,
            redirect_target.as_deref(),
        )?;
        ledger_by_title.insert(
            key.clone(),
            SyncLedgerEntry {
                title: page.title.clone(),
                namespace: page.namespace,
                relative_path: relative_path.clone(),
                content_hash: remote_hash,
                wiki_modified_at: Some(page.timestamp.clone()),
            },
        );

        report.pulled += 1;
        if existed_before {
            report.updated += 1;
            report.pages.push(PullPageResult {
                title: page.title.clone(),
                action: "updated".to_string(),
                detail: None,
            });
        } else {
            report.created += 1;
            report.pages.push(PullPageResult {
                title: page.title.clone(),
                action: "created".to_string(),
                detail: None,
            });
        }
    }

    if let Some(config_key) = pull_config_key(options)
        && let Some(timestamp) = max_timestamp
    {
        set_sync_config(&connection, &config_key, &timestamp)?;
    }

    if wrote_files {
        report.reindex = Some(rebuild_index(paths, &ScanOptions::default())?);
    }

    report.request_count = api.request_count();
    report.success = report.errors.is_empty();
    Ok(report)
}

fn resolve_pages_to_pull<A: WikiReadApi>(
    connection: &Connection,
    options: &PullOptions,
    api: &mut A,
) -> Result<Vec<String>> {
    let mut titles = BTreeSet::new();

    if let Some(category) = &options.category {
        for title in api.get_category_members(category)? {
            let normalized = normalize_title_for_storage(&title);
            if !normalized.is_empty() {
                titles.insert(normalized);
            }
        }
        return Ok(titles.into_iter().collect());
    }

    if options.namespaces.is_empty() {
        bail!("pull requires at least one namespace");
    }

    if !options.full
        && let Some(config_key) = pull_config_key(options)
        && let Some(last_pull) = get_sync_config(connection, &config_key)?
    {
        for title in api.get_recent_changes(&last_pull, &options.namespaces)? {
            let normalized = normalize_title_for_storage(&title);
            if !normalized.is_empty() {
                titles.insert(normalized);
            }
        }
        return Ok(titles.into_iter().collect());
    }

    for namespace in &options.namespaces {
        for title in api.get_all_pages(*namespace)? {
            let normalized = normalize_title_for_storage(&title);
            if !normalized.is_empty() {
                titles.insert(normalized);
            }
        }
    }

    Ok(titles.into_iter().collect())
}

fn remove_stale_synced_path_if_safe(
    paths: &ResolvedPaths,
    existing: &Option<SyncLedgerEntry>,
    target_relative_path: &str,
    overwrite_local: bool,
) -> Result<()> {
    let Some(existing) = existing else {
        return Ok(());
    };
    if existing.relative_path == target_relative_path {
        return Ok(());
    }

    let old_absolute = absolute_path_from_relative(paths, &existing.relative_path);
    if !old_absolute.exists() {
        return Ok(());
    }
    validate_scoped_path(paths, &old_absolute)?;

    let old_content = fs::read_to_string(&old_absolute).with_context(|| {
        format!(
            "failed to read previous synced file {}",
            old_absolute.display()
        )
    })?;
    let old_hash = compute_hash(&old_content);
    let old_modified = old_hash != existing.content_hash;
    if old_modified && !overwrite_local {
        bail!(
            "cannot update path for {} because previous synced path has local modifications: {} (use --overwrite-local)",
            existing.title,
            normalize_path(&old_absolute)
        );
    }

    fs::remove_file(&old_absolute).with_context(|| {
        format!(
            "failed to remove stale synced file {}",
            old_absolute.display()
        )
    })?;
    Ok(())
}

fn pull_config_key(options: &PullOptions) -> Option<String> {
    if options.category.is_some() {
        return None;
    }
    let mut namespaces = options.namespaces.clone();
    namespaces.sort_unstable();
    namespaces.dedup();
    if namespaces.is_empty() {
        return None;
    }
    Some(format!(
        "last_pull_ns_{}",
        namespaces
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("_")
    ))
}

fn change_order(change_type: &DiffChangeType) -> u8 {
    match change_type {
        DiffChangeType::NewLocal => 0,
        DiffChangeType::ModifiedLocal => 1,
        DiffChangeType::DeletedLocal => 2,
    }
}

fn load_sync_ledger_map(
    connection: &Connection,
    include_templates: bool,
) -> Result<BTreeMap<String, SyncLedgerEntry>> {
    if !table_exists(connection, "sync_ledger_pages")? {
        return Ok(BTreeMap::new());
    }

    let mut statement = connection
        .prepare(
            "SELECT title, namespace, relative_path, content_hash, wiki_modified_at
             FROM sync_ledger_pages",
        )
        .context("failed to prepare sync ledger query")?;
    let rows = statement
        .query_map([], |row| {
            Ok(SyncLedgerEntry {
                title: row.get(0)?,
                namespace: row.get(1)?,
                relative_path: row.get(2)?,
                content_hash: row.get(3)?,
                wiki_modified_at: row.get(4)?,
            })
        })
        .context("failed to run sync ledger query")?;

    let mut out = BTreeMap::new();
    for row in rows {
        let row = row.context("failed to decode sync ledger row")?;
        if !include_templates && is_template_namespace_id(row.namespace) {
            continue;
        }
        out.insert(normalized_title_key(&row.title), row);
    }
    Ok(out)
}

fn upsert_sync_ledger(
    connection: &Connection,
    page: &RemotePage,
    relative_path: &str,
    content_hash: &str,
    is_redirect: bool,
    redirect_target: Option<&str>,
) -> Result<()> {
    initialize_sync_schema(connection)?;
    let now = unix_timestamp()?;
    connection
        .execute(
            "INSERT INTO sync_ledger_pages (
                title, namespace, relative_path, content_hash, wiki_modified_at, revision_id,
                page_id, is_redirect, redirect_target, last_synced_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(title) DO UPDATE SET
                namespace = excluded.namespace,
                relative_path = excluded.relative_path,
                content_hash = excluded.content_hash,
                wiki_modified_at = excluded.wiki_modified_at,
                revision_id = excluded.revision_id,
                page_id = excluded.page_id,
                is_redirect = excluded.is_redirect,
                redirect_target = excluded.redirect_target,
                last_synced_at_unix = excluded.last_synced_at_unix",
            params![
                page.title,
                page.namespace,
                relative_path,
                content_hash,
                page.timestamp,
                page.revision_id,
                page.page_id,
                if is_redirect { 1i64 } else { 0i64 },
                redirect_target,
                i64::try_from(now).context("timestamp does not fit into i64")?
            ],
        )
        .with_context(|| format!("failed to upsert sync ledger row for {}", page.title))?;
    Ok(())
}

fn get_sync_config(connection: &Connection, key: &str) -> Result<Option<String>> {
    if !table_exists(connection, "sync_config")? {
        return Ok(None);
    }
    let mut statement = connection
        .prepare("SELECT value FROM sync_config WHERE key = ?1 LIMIT 1")
        .context("failed to prepare sync config query")?;
    let mut rows = statement
        .query([key])
        .with_context(|| format!("failed to read sync config key {key}"))?;
    let row = match rows.next().context("failed to decode sync config row")? {
        Some(row) => row,
        None => return Ok(None),
    };
    let value = row.get(0).context("failed to decode sync config value")?;
    Ok(Some(value))
}

fn set_sync_config(connection: &Connection, key: &str, value: &str) -> Result<()> {
    initialize_sync_schema(connection)?;
    connection
        .execute(
            "INSERT INTO sync_config (key, value) VALUES (?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .with_context(|| format!("failed to set sync config key {key}"))?;
    Ok(())
}

fn open_sync_connection(paths: &ResolvedPaths) -> Result<Connection> {
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

fn initialize_sync_schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(SYNC_SCHEMA_SQL)
        .context("failed to initialize sync schema")
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool> {
    let exists: i64 = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to inspect sqlite_master for table {table_name}"))?;
    Ok(exists == 1)
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

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))
}

fn absolute_path_from_relative(paths: &ResolvedPaths, relative: &str) -> PathBuf {
    let mut output = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            output.push(segment);
        }
    }
    output
}

fn parse_redirect(content: &str) -> (bool, Option<String>) {
    let trimmed = content.trim();
    if !trimmed.to_ascii_uppercase().starts_with("#REDIRECT") {
        return (false, None);
    }
    if let Some(start) = trimmed.find("[[")
        && let Some(end) = trimmed[start + 2..].find("]]")
    {
        let target = trimmed[start + 2..start + 2 + end].trim().to_string();
        if !target.is_empty() {
            return (true, Some(target));
        }
    }
    (true, None)
}

fn compute_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut output = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn normalized_title_key(title: &str) -> String {
    normalize_title_for_storage(title).to_ascii_lowercase()
}

fn normalize_title_for_storage(title: &str) -> String {
    title.replace('_', " ").trim().to_string()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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

fn is_template_namespace_id(namespace: i32) -> bool {
    matches!(namespace, NS_TEMPLATE | NS_MODULE | NS_MEDIAWIKI)
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
    categorymembers: Vec<TitleQueryItem>,
    #[serde(default)]
    recentchanges: Vec<RecentChangeItem>,
    #[serde(default)]
    pages: Vec<PageQueryItem>,
    #[serde(default)]
    search: Vec<SearchQueryItem>,
}

#[derive(Debug, Deserialize, Default)]
struct ContinuationPayload {
    apcontinue: Option<String>,
    cmcontinue: Option<String>,
    rccontinue: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TitleQueryItem {
    title: String,
}

#[derive(Debug, Deserialize)]
struct RecentChangeItem {
    title: String,
}

#[derive(Debug, Deserialize)]
struct PageQueryItem {
    pageid: Option<i64>,
    ns: i32,
    title: String,
    missing: Option<bool>,
    #[serde(default)]
    revisions: Vec<RevisionQueryItem>,
}

#[derive(Debug, Deserialize)]
struct RevisionQueryItem {
    revid: i64,
    timestamp: String,
    slots: Option<RevisionSlotContainer>,
}

#[derive(Debug, Deserialize)]
struct RevisionSlotContainer {
    main: Option<RevisionMainSlot>,
}

#[derive(Debug, Deserialize)]
struct RevisionMainSlot {
    content: String,
}

#[derive(Debug, Deserialize)]
struct SearchQueryItem {
    title: String,
    ns: i32,
    pageid: i64,
    wordcount: Option<i64>,
    snippet: Option<String>,
    timestamp: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        DiffChangeType, DiffOptions, ExternalSearchHit, NS_MAIN, PullOptions, RemotePage,
        WikiReadApi, diff_local_against_sync, pull_from_remote_with_api,
    };
    use crate::phase1::{ResolvedPaths, ValueSource};

    #[derive(Default)]
    struct MockApi {
        all_pages_by_namespace: BTreeMap<i32, Vec<String>>,
        recent_changes: Vec<String>,
        category_members: Vec<String>,
        page_contents: BTreeMap<String, RemotePage>,
        search_hits: Vec<ExternalSearchHit>,
        request_count: usize,
    }

    impl WikiReadApi for MockApi {
        fn get_all_pages(&mut self, namespace: i32) -> anyhow::Result<Vec<String>> {
            self.request_count += 1;
            Ok(self
                .all_pages_by_namespace
                .get(&namespace)
                .cloned()
                .unwrap_or_default())
        }

        fn get_category_members(&mut self, _category: &str) -> anyhow::Result<Vec<String>> {
            self.request_count += 1;
            Ok(self.category_members.clone())
        }

        fn get_recent_changes(
            &mut self,
            _since: &str,
            _namespaces: &[i32],
        ) -> anyhow::Result<Vec<String>> {
            self.request_count += 1;
            Ok(self.recent_changes.clone())
        }

        fn get_page_contents(&mut self, titles: &[String]) -> anyhow::Result<Vec<RemotePage>> {
            self.request_count += 1;
            let mut output = Vec::new();
            for title in titles {
                if let Some(page) = self.page_contents.get(title) {
                    output.push(page.clone());
                }
            }
            Ok(output)
        }

        fn search(
            &mut self,
            _query: &str,
            _namespaces: &[i32],
            _limit: usize,
        ) -> anyhow::Result<Vec<ExternalSearchHit>> {
            self.request_count += 1;
            Ok(self.search_hits.clone())
        }

        fn request_count(&self) -> usize {
            self.request_count
        }
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(path, content).expect("write file");
    }

    fn paths(project_root: &Path) -> ResolvedPaths {
        ResolvedPaths {
            project_root: project_root.to_path_buf(),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool").join("data"),
            db_path: project_root
                .join(".wikitool")
                .join("data")
                .join("wikitool.db"),
            config_path: project_root.join(".wikitool").join("config.toml"),
            parser_config_path: project_root.join(".wikitool").join("remilia-parser.json"),
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    fn base_page(title: &str, content: &str) -> RemotePage {
        RemotePage {
            title: title.to_string(),
            namespace: NS_MAIN,
            page_id: 100,
            revision_id: 200,
            timestamp: "2026-02-19T00:00:00Z".to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn pull_writes_files_and_reindexes() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string(), "Beta".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
        api.page_contents
            .insert("Beta".to_string(), base_page("Beta", "[[Alpha]]"));

        let report = pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("pull");

        assert!(report.success);
        assert_eq!(report.created, 2);
        assert_eq!(report.updated, 0);
        assert_eq!(report.skipped, 0);
        assert!(
            paths
                .wiki_content_dir
                .join("Main")
                .join("Alpha.wiki")
                .exists()
        );
        assert!(
            paths
                .wiki_content_dir
                .join("Main")
                .join("Beta.wiki")
                .exists()
        );
        assert!(report.reindex.is_some());
    }

    #[test]
    fn pull_skips_modified_local_when_overwrite_is_disabled() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "local edited",
        );

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "remote version"));

        let report = pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("pull");

        assert_eq!(report.created, 0);
        assert_eq!(report.updated, 0);
        assert_eq!(report.skipped, 1);
        let current = fs::read_to_string(paths.wiki_content_dir.join("Main").join("Alpha.wiki"))
            .expect("read local file");
        assert_eq!(current, "local edited");
    }

    #[test]
    fn diff_detects_new_modified_and_deleted_local_pages() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        let mut api = MockApi::default();
        api.all_pages_by_namespace
            .insert(NS_MAIN, vec!["Alpha".to_string(), "Beta".to_string()]);
        api.page_contents
            .insert("Alpha".to_string(), base_page("Alpha", "alpha body"));
        api.page_contents
            .insert("Beta".to_string(), base_page("Beta", "beta body"));

        pull_from_remote_with_api(
            &paths,
            &PullOptions {
                namespaces: vec![NS_MAIN],
                category: None,
                full: true,
                overwrite_local: false,
            },
            &mut api,
        )
        .expect("seed pull");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "alpha local edit",
        );
        fs::remove_file(paths.wiki_content_dir.join("Main").join("Beta.wiki"))
            .expect("delete beta");
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "gamma local",
        );

        let diff = diff_local_against_sync(
            &paths,
            &DiffOptions {
                include_templates: false,
            },
        )
        .expect("diff")
        .expect("diff report");

        assert_eq!(diff.new_local, 1);
        assert_eq!(diff.modified_local, 1);
        assert_eq!(diff.deleted_local, 1);
        assert!(
            diff.changes
                .iter()
                .any(|item| item.title == "Gamma" && item.change_type == DiffChangeType::NewLocal)
        );
        assert!(
            diff.changes
                .iter()
                .any(|item| item.title == "Alpha"
                    && item.change_type == DiffChangeType::ModifiedLocal)
        );
        assert!(
            diff.changes.iter().any(
                |item| item.title == "Beta" && item.change_type == DiffChangeType::DeletedLocal
            )
        );
    }
}
