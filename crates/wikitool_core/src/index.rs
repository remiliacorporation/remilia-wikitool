use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params, params_from_iter};
use serde::Serialize;

use crate::filesystem::{Namespace, ScanOptions, ScanStats, ScannedFile, scan_files};
use crate::runtime::ResolvedPaths;

const INDEX_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS indexed_pages (
    relative_path TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    namespace TEXT NOT NULL,
    is_redirect INTEGER NOT NULL,
    redirect_target TEXT,
    content_hash TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    indexed_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_indexed_pages_title ON indexed_pages(title);
CREATE INDEX IF NOT EXISTS idx_indexed_pages_namespace ON indexed_pages(namespace);

CREATE TABLE IF NOT EXISTS indexed_links (
    source_relative_path TEXT NOT NULL,
    source_title TEXT NOT NULL,
    target_title TEXT NOT NULL,
    target_namespace TEXT NOT NULL,
    is_category_membership INTEGER NOT NULL,
    PRIMARY KEY (source_relative_path, target_title, is_category_membership),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_links_target ON indexed_links(target_title);
CREATE INDEX IF NOT EXISTS idx_indexed_links_source ON indexed_links(source_title);
CREATE INDEX IF NOT EXISTS idx_indexed_links_category_membership ON indexed_links(is_category_membership, target_title);

CREATE TABLE IF NOT EXISTS indexed_page_chunks (
    source_relative_path TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    source_title TEXT NOT NULL,
    source_namespace TEXT NOT NULL,
    section_heading TEXT,
    chunk_text TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    PRIMARY KEY (source_relative_path, chunk_index),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_page_chunks_title ON indexed_page_chunks(source_title);
CREATE INDEX IF NOT EXISTS idx_indexed_page_chunks_tokens ON indexed_page_chunks(token_estimate);

CREATE TABLE IF NOT EXISTS indexed_template_invocations (
    source_relative_path TEXT NOT NULL,
    source_title TEXT NOT NULL,
    template_title TEXT NOT NULL,
    parameter_keys TEXT NOT NULL,
    PRIMARY KEY (source_relative_path, template_title, parameter_keys),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_indexed_template_invocations_template ON indexed_template_invocations(template_title);
CREATE INDEX IF NOT EXISTS idx_indexed_template_invocations_source ON indexed_template_invocations(source_title);
"#;

const INDEX_CHUNK_WORD_TARGET: usize = 96;
const CONTEXT_CHUNK_LIMIT: usize = 8;
const CONTEXT_TOKEN_BUDGET: usize = 720;
const TEMPLATE_INVOCATION_LIMIT: usize = 24;
const NO_PARAMETER_KEYS_SENTINEL: &str = "__none__";
const CHUNK_CANDIDATE_MULTIPLIER_SINGLE: usize = 6;
const CHUNK_CANDIDATE_MULTIPLIER_ACROSS: usize = 10;
const CHUNK_LEXICAL_SIMILARITY_THRESHOLD: f32 = 0.86;
const AUTHORING_TEMPLATE_KEY_LIMIT: usize = 12;

#[derive(Debug, Clone, Serialize)]
pub struct RebuildReport {
    pub db_path: String,
    pub inserted_rows: usize,
    pub inserted_links: usize,
    pub scan: ScanStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredIndexStats {
    pub indexed_rows: usize,
    pub redirects: usize,
    pub by_namespace: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalSearchHit {
    pub title: String,
    pub namespace: String,
    pub is_redirect: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalContextHeading {
    pub level: u8,
    pub heading: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalContextChunk {
    pub section_heading: Option<String>,
    pub token_estimate: usize,
    pub chunk_text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalTemplateInvocation {
    pub template_title: String,
    pub parameter_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalContextBundle {
    pub title: String,
    pub namespace: String,
    pub is_redirect: bool,
    pub redirect_target: Option<String>,
    pub relative_path: String,
    pub bytes: u64,
    pub word_count: usize,
    pub content_preview: String,
    pub sections: Vec<LocalContextHeading>,
    pub context_chunks: Vec<LocalContextChunk>,
    pub context_tokens_estimate: usize,
    pub outgoing_links: Vec<String>,
    pub backlinks: Vec<String>,
    pub categories: Vec<String>,
    pub templates: Vec<String>,
    pub modules: Vec<String>,
    pub template_invocations: Vec<LocalTemplateInvocation>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalChunkRetrievalResult {
    pub title: String,
    pub namespace: String,
    pub relative_path: String,
    pub query: Option<String>,
    pub retrieval_mode: String,
    pub chunks: Vec<LocalContextChunk>,
    pub token_estimate_total: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum LocalChunkRetrieval {
    IndexMissing,
    TitleMissing { title: String },
    Found(LocalChunkRetrievalResult),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RetrievedChunk {
    pub source_title: String,
    pub source_namespace: String,
    pub source_relative_path: String,
    pub section_heading: Option<String>,
    pub token_estimate: usize,
    pub chunk_text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalChunkAcrossPagesResult {
    pub query: String,
    pub retrieval_mode: String,
    pub max_pages: usize,
    pub source_page_count: usize,
    pub chunks: Vec<RetrievedChunk>,
    pub token_estimate_total: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum LocalChunkAcrossRetrieval {
    IndexMissing,
    QueryMissing,
    Found(LocalChunkAcrossPagesResult),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthoringInventory {
    pub indexed_pages_total: usize,
    pub main_pages: usize,
    pub template_pages: usize,
    pub indexed_links_total: usize,
    pub template_invocation_rows: usize,
    pub distinct_templates_invoked: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthoringPageCandidate {
    pub title: String,
    pub namespace: String,
    pub is_redirect: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TemplateUsageSummary {
    pub template_title: String,
    pub usage_count: usize,
    pub parameter_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthoringKnowledgePackResult {
    pub topic: String,
    pub query: String,
    pub inventory: AuthoringInventory,
    pub related_pages: Vec<AuthoringPageCandidate>,
    pub suggested_links: Vec<String>,
    pub suggested_categories: Vec<String>,
    pub suggested_templates: Vec<TemplateUsageSummary>,
    pub template_baseline: Vec<TemplateUsageSummary>,
    pub stub_existing_links: Vec<String>,
    pub stub_missing_links: Vec<String>,
    pub stub_detected_templates: Vec<String>,
    pub retrieval_mode: String,
    pub chunks: Vec<RetrievedChunk>,
    pub token_estimate_total: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum AuthoringKnowledgePack {
    IndexMissing,
    QueryMissing,
    Found(Box<AuthoringKnowledgePackResult>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoringKnowledgePackOptions {
    pub related_page_limit: usize,
    pub chunk_limit: usize,
    pub token_budget: usize,
    pub max_pages: usize,
    pub link_limit: usize,
    pub category_limit: usize,
    pub template_limit: usize,
    pub diversify: bool,
}

impl Default for AuthoringKnowledgePackOptions {
    fn default() -> Self {
        Self {
            related_page_limit: 18,
            chunk_limit: 10,
            token_budget: 1200,
            max_pages: 8,
            link_limit: 18,
            category_limit: 8,
            template_limit: 16,
            diversify: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BrokenLinkIssue {
    pub source_title: String,
    pub target_title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoubleRedirectIssue {
    pub title: String,
    pub first_target: String,
    pub final_target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub broken_links: Vec<BrokenLinkIssue>,
    pub double_redirects: Vec<DoubleRedirectIssue>,
    pub uncategorized_pages: Vec<String>,
    pub orphan_pages: Vec<String>,
}

pub fn rebuild_index(paths: &ResolvedPaths, options: &ScanOptions) -> Result<RebuildReport> {
    let files = scan_files(paths, options)?;
    let scan = summarize_files(&files);
    ensure_db_parent(paths)?;
    let mut connection = open_connection(&paths.db_path)?;
    initialize_schema(&connection)?;
    let indexed_at_unix = unix_timestamp()?;

    let transaction = connection
        .transaction()
        .context("failed to start index rebuild transaction")?;
    transaction
        .execute("DELETE FROM indexed_pages", [])
        .context("failed to clear indexed_pages table")?;

    let mut page_statement = transaction
        .prepare(
            "INSERT INTO indexed_pages (
                relative_path,
                title,
                namespace,
                is_redirect,
                redirect_target,
                content_hash,
                bytes,
                indexed_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .context("failed to prepare indexed_pages insert")?;

    let mut link_statement = transaction
        .prepare(
            "INSERT OR IGNORE INTO indexed_links (
                source_relative_path,
                source_title,
                target_title,
                target_namespace,
                is_category_membership
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .context("failed to prepare indexed_links insert")?;

    let mut chunk_statement = transaction
        .prepare(
            "INSERT INTO indexed_page_chunks (
                source_relative_path,
                chunk_index,
                source_title,
                source_namespace,
                section_heading,
                chunk_text,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .context("failed to prepare indexed_page_chunks insert")?;

    let mut template_invocation_statement = transaction
        .prepare(
            "INSERT OR IGNORE INTO indexed_template_invocations (
                source_relative_path,
                source_title,
                template_title,
                parameter_keys
            ) VALUES (?1, ?2, ?3, ?4)",
        )
        .context("failed to prepare indexed_template_invocations insert")?;

    let mut inserted_rows = 0usize;
    let mut inserted_links = 0usize;
    for file in &files {
        page_statement
            .execute(params![
                file.relative_path,
                file.title,
                file.namespace,
                if file.is_redirect { 1i64 } else { 0i64 },
                file.redirect_target,
                file.content_hash,
                i64::try_from(file.bytes).context("bytes value does not fit into i64")?,
                i64::try_from(indexed_at_unix).context("timestamp does not fit into i64")?,
            ])
            .with_context(|| format!("failed to insert {}", file.relative_path))?;
        inserted_rows += 1;

        let content = load_scanned_file_content(paths, file)?;
        for link in extract_wikilinks(&content) {
            let affected = link_statement
                .execute(params![
                    file.relative_path,
                    file.title,
                    link.target_title,
                    link.target_namespace,
                    if link.is_category_membership {
                        1i64
                    } else {
                        0i64
                    }
                ])
                .with_context(|| format!("failed to insert links for {}", file.relative_path))?;
            inserted_links += affected;
        }

        let context_chunks = chunk_article_context(&content);
        for (chunk_index, chunk) in context_chunks.iter().enumerate() {
            chunk_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(chunk_index).context("chunk index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    chunk.section_heading.as_deref(),
                    chunk.chunk_text.as_str(),
                    i64::try_from(chunk.token_estimate)
                        .context("chunk token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!("failed to insert context chunks for {}", file.relative_path)
                })?;
        }

        let mut seen_signatures = BTreeSet::new();
        for invocation in extract_template_invocations(&content) {
            let parameter_keys = canonical_parameter_key_list(&invocation.parameter_keys);
            let signature = format!("{}|{}", invocation.template_title, parameter_keys);
            if !seen_signatures.insert(signature) {
                continue;
            }
            template_invocation_statement
                .execute(params![
                    file.relative_path,
                    file.title,
                    invocation.template_title,
                    parameter_keys,
                ])
                .with_context(|| {
                    format!(
                        "failed to insert template invocations for {}",
                        file.relative_path
                    )
                })?;
        }
    }
    drop(template_invocation_statement);
    drop(chunk_statement);
    drop(link_statement);
    drop(page_statement);

    transaction
        .commit()
        .context("failed to commit index rebuild transaction")?;

    // Rebuild FTS5 index if the virtual table exists (created by migration v003)
    rebuild_fts_index(&connection);

    Ok(RebuildReport {
        db_path: normalize_path(&paths.db_path),
        inserted_rows,
        inserted_links,
        scan,
    })
}

pub fn load_stored_index_stats(paths: &ResolvedPaths) -> Result<Option<StoredIndexStats>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }

    let connection = open_connection(&paths.db_path)?;
    if !table_exists(&connection, "indexed_pages")? {
        return Ok(None);
    }

    let indexed_rows = count_query(&connection, "SELECT COUNT(*) FROM indexed_pages")
        .context("failed to count indexed rows")?;
    let redirects = count_query(
        &connection,
        "SELECT COUNT(*) FROM indexed_pages WHERE is_redirect = 1",
    )
    .context("failed to count redirects")?;
    let by_namespace = namespace_counts(&connection)?;

    Ok(Some(StoredIndexStats {
        indexed_rows,
        redirects,
        by_namespace,
    }))
}

pub fn query_search_local(
    paths: &ResolvedPaths,
    query: &str,
    limit: usize,
) -> Result<Option<Vec<LocalSearchHit>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    let normalized = normalize_spaces(&query.replace('_', " "));
    if normalized.is_empty() {
        return Ok(Some(Vec::new()));
    }

    // Try FTS5 first if the virtual table exists
    if fts_table_exists(&connection, "indexed_pages_fts")
        && let Ok(hits) = query_search_fts(&connection, &normalized, limit)
        && !hits.is_empty()
    {
        return Ok(Some(hits));
    }

    // Fallback to LIKE-based search
    query_search_like(&connection, &normalized, limit).map(Some)
}

fn query_search_fts(
    connection: &Connection,
    normalized: &str,
    limit: usize,
) -> Result<Vec<LocalSearchHit>> {
    let limit_i64 = i64::try_from(limit).context("search limit does not fit into i64")?;
    // FTS5 match expression: quote the term for phrase matching, add * for prefix
    let fts_query = format!("\"{normalized}\" *");
    let mut statement = connection
        .prepare(
            "SELECT ip.title, ip.namespace, ip.is_redirect
             FROM indexed_pages_fts fts
             JOIN indexed_pages ip ON ip.rowid = fts.rowid
             WHERE indexed_pages_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .context("failed to prepare FTS search query")?;
    let rows = statement
        .query_map(params![fts_query, limit_i64], |row| {
            let title: String = row.get(0)?;
            let namespace: String = row.get(1)?;
            let is_redirect: i64 = row.get(2)?;
            Ok(LocalSearchHit {
                title,
                namespace,
                is_redirect: is_redirect == 1,
            })
        })
        .context("failed to run FTS search query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode FTS search row")?);
    }
    Ok(out)
}

fn query_search_like(
    connection: &Connection,
    normalized: &str,
    limit: usize,
) -> Result<Vec<LocalSearchHit>> {
    let wildcard = format!("%{normalized}%");
    let prefix = format!("{normalized}%");
    let limit_i64 = i64::try_from(limit).context("search limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT title, namespace, is_redirect
             FROM indexed_pages
             WHERE lower(title) LIKE lower(?1)
             ORDER BY
               CASE
                 WHEN lower(title) = lower(?2) THEN 0
                 WHEN lower(title) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               title ASC
             LIMIT ?4",
        )
        .context("failed to prepare local search query")?;
    let rows = statement
        .query_map(params![wildcard, normalized, prefix, limit_i64], |row| {
            let title: String = row.get(0)?;
            let namespace: String = row.get(1)?;
            let is_redirect: i64 = row.get(2)?;
            Ok(LocalSearchHit {
                title,
                namespace,
                is_redirect: is_redirect == 1,
            })
        })
        .context("failed to run local search query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode local search row")?);
    }
    Ok(out)
}

pub fn build_local_context(
    paths: &ResolvedPaths,
    title: &str,
) -> Result<Option<LocalContextBundle>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    let normalized = normalize_query_title(title);
    if normalized.is_empty() {
        return Ok(None);
    }

    let page = match load_page_record(&connection, &normalized)? {
        Some(page) => page,
        None => return Ok(None),
    };
    let absolute = absolute_path_from_relative(paths, &page.relative_path);
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))?;

    let link_rows = load_outgoing_link_rows(&connection, &page.relative_path)?;
    let backlinks = query_backlinks_for_connection(&connection, &page.title)?;
    let sections = parse_section_headings(&content, 24);
    let context_chunks =
        load_context_chunks_for_bundle(&connection, &page.relative_path, &content)?;
    let context_tokens_estimate = context_chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();
    let template_invocations =
        load_template_invocations_for_bundle(&connection, &page.relative_path, &content)?;

    let mut outgoing_set = BTreeSet::new();
    let mut category_set = BTreeSet::new();
    let mut template_set = BTreeSet::new();
    let mut module_set = BTreeSet::new();
    for link in &link_rows {
        outgoing_set.insert(link.target_title.clone());
        if link.is_category_membership {
            category_set.insert(link.target_title.clone());
        }
        if link.target_namespace == Namespace::Template.as_str() {
            template_set.insert(link.target_title.clone());
        }
        if link.target_namespace == Namespace::Module.as_str() {
            module_set.insert(link.target_title.clone());
        }
    }
    for invocation in &template_invocations {
        template_set.insert(invocation.template_title.clone());
    }

    Ok(Some(LocalContextBundle {
        title: page.title,
        namespace: page.namespace,
        is_redirect: page.is_redirect,
        redirect_target: page.redirect_target,
        relative_path: page.relative_path,
        bytes: page.bytes,
        word_count: count_words(&content),
        content_preview: make_content_preview(&content, 280),
        sections,
        context_chunks,
        context_tokens_estimate,
        outgoing_links: outgoing_set.into_iter().collect(),
        backlinks,
        categories: category_set.into_iter().collect(),
        templates: template_set.into_iter().collect(),
        modules: module_set.into_iter().collect(),
        template_invocations,
    }))
}

pub fn retrieve_local_context_chunks(
    paths: &ResolvedPaths,
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
) -> Result<LocalChunkRetrieval> {
    retrieve_local_context_chunks_with_options(paths, title, query, limit, token_budget, true)
}

pub fn retrieve_local_context_chunks_with_options(
    paths: &ResolvedPaths,
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
    diversify: bool,
) -> Result<LocalChunkRetrieval> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(LocalChunkRetrieval::IndexMissing),
    };
    let normalized_title = normalize_query_title(title);
    if normalized_title.is_empty() {
        return Ok(LocalChunkRetrieval::TitleMissing {
            title: title.to_string(),
        });
    }
    let page = match load_page_record(&connection, &normalized_title)? {
        Some(page) => page,
        None => {
            return Ok(LocalChunkRetrieval::TitleMissing {
                title: normalized_title,
            });
        }
    };
    let normalized_query = query
        .map(|value| normalize_spaces(&value.replace('_', " ")))
        .filter(|value| !value.is_empty());
    let max_chunks = limit.max(1);
    let max_tokens = token_budget.max(1);
    let candidate_limit = candidate_limit(max_chunks, CHUNK_CANDIDATE_MULTIPLIER_SINGLE);
    let (chunks, retrieval_mode) = load_chunks_for_query(
        paths,
        &connection,
        &page.relative_path,
        normalized_query.as_deref(),
        candidate_limit,
    )?;
    let chunk_candidates = chunks
        .into_iter()
        .map(|chunk| RetrievedChunk {
            source_title: page.title.clone(),
            source_namespace: page.namespace.clone(),
            source_relative_path: page.relative_path.clone(),
            section_heading: chunk.section_heading,
            token_estimate: chunk.token_estimate,
            chunk_text: chunk.chunk_text,
        })
        .collect::<Vec<_>>();
    let selected = select_retrieved_chunks(
        chunk_candidates,
        max_chunks,
        max_tokens,
        diversify,
        Some(1),
        false,
    );
    let chunks = selected
        .into_iter()
        .map(|chunk| LocalContextChunk {
            section_heading: chunk.section_heading,
            token_estimate: chunk.token_estimate,
            chunk_text: chunk.chunk_text,
        })
        .collect::<Vec<_>>();
    let token_estimate_total = chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();

    Ok(LocalChunkRetrieval::Found(LocalChunkRetrievalResult {
        title: page.title,
        namespace: page.namespace,
        relative_path: page.relative_path,
        query: normalized_query,
        retrieval_mode,
        chunks,
        token_estimate_total,
    }))
}

pub fn retrieve_local_context_chunks_across_pages(
    paths: &ResolvedPaths,
    query: &str,
    limit: usize,
    token_budget: usize,
    max_pages: usize,
    diversify: bool,
) -> Result<LocalChunkAcrossRetrieval> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(LocalChunkAcrossRetrieval::IndexMissing),
    };
    let normalized_query = normalize_spaces(&query.replace('_', " "));
    if normalized_query.is_empty() {
        return Ok(LocalChunkAcrossRetrieval::QueryMissing);
    }

    let max_chunks = limit.max(1);
    let max_tokens = token_budget.max(1);
    let max_pages = max_pages.max(1);
    let candidate_cap = candidate_limit(max_chunks, CHUNK_CANDIDATE_MULTIPLIER_ACROSS);

    let (candidates, retrieval_mode) = if table_exists(&connection, "indexed_page_chunks")? {
        if fts_table_exists(&connection, "indexed_page_chunks_fts") {
            let hits = query_chunks_fts_across_pages_for_connection(
                &connection,
                &normalized_query,
                candidate_cap,
            )?;
            if !hits.is_empty() {
                (hits, "fts-across".to_string())
            } else {
                (
                    query_chunks_like_across_pages_for_connection(
                        &connection,
                        &normalized_query,
                        candidate_cap,
                    )?,
                    "like-across".to_string(),
                )
            }
        } else {
            (
                query_chunks_like_across_pages_for_connection(
                    &connection,
                    &normalized_query,
                    candidate_cap,
                )?,
                "like-across".to_string(),
            )
        }
    } else {
        (
            query_chunks_scan_across_pages(paths, &normalized_query, candidate_cap)?,
            "scan-across".to_string(),
        )
    };

    let chunks = select_retrieved_chunks(
        candidates,
        max_chunks,
        max_tokens,
        diversify,
        Some(max_pages),
        true,
    );
    let source_page_count = chunks
        .iter()
        .map(|chunk| chunk.source_relative_path.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let token_estimate_total = chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();

    Ok(LocalChunkAcrossRetrieval::Found(
        LocalChunkAcrossPagesResult {
            query: normalized_query,
            retrieval_mode,
            max_pages,
            source_page_count,
            chunks,
            token_estimate_total,
        },
    ))
}

pub fn build_authoring_knowledge_pack(
    paths: &ResolvedPaths,
    topic: Option<&str>,
    stub_content: Option<&str>,
    options: &AuthoringKnowledgePackOptions,
) -> Result<AuthoringKnowledgePack> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(AuthoringKnowledgePack::IndexMissing),
    };

    let normalized_topic = topic
        .map(|value| normalize_spaces(&value.replace('_', " ")))
        .unwrap_or_default();
    let (stub_link_titles, stub_template_titles) = analyze_stub_hints(stub_content);

    let query = if !normalized_topic.is_empty() {
        normalized_topic.clone()
    } else if let Some(first_link) = stub_link_titles.first() {
        first_link.clone()
    } else {
        String::new()
    };
    if query.is_empty() {
        return Ok(AuthoringKnowledgePack::QueryMissing);
    }
    let topic = if normalized_topic.is_empty() {
        query.clone()
    } else {
        normalized_topic
    };

    let related_limit = options.related_page_limit.max(1);
    let chunk_limit = options.chunk_limit.max(1);
    let token_budget = options.token_budget.max(1);
    let max_pages = options.max_pages.max(1);
    let link_limit = options.link_limit.max(1);
    let category_limit = options.category_limit.max(1);
    let template_limit = options.template_limit.max(1);

    let search_limit = candidate_limit(related_limit, 2);
    let search_hits = query_local_search_for_connection(&connection, &query, search_limit)?;
    let related_pages = collect_related_pages_for_authoring(
        &connection,
        &stub_link_titles,
        search_hits,
        related_limit,
    )?;
    let source_titles = related_pages
        .iter()
        .map(|page| page.title.clone())
        .collect::<Vec<_>>();

    let mut stub_existing_links = Vec::new();
    let mut stub_missing_links = Vec::new();
    for link in stub_link_titles {
        if let Some(page) = load_page_record(&connection, &normalize_query_title(&link))? {
            stub_existing_links.push(page.title);
        } else {
            stub_missing_links.push(link);
        }
    }
    stub_existing_links.sort();
    stub_existing_links.dedup();
    stub_missing_links.sort();
    stub_missing_links.dedup();

    let stub_detected_templates = stub_template_titles;

    let chunk_retrieval = retrieve_local_context_chunks_across_pages(
        paths,
        &query,
        chunk_limit,
        token_budget,
        max_pages,
        options.diversify,
    )?;
    let (retrieval_mode, chunks, token_estimate_total) = match chunk_retrieval {
        LocalChunkAcrossRetrieval::Found(report) => (
            report.retrieval_mode,
            report.chunks,
            report.token_estimate_total,
        ),
        LocalChunkAcrossRetrieval::IndexMissing => return Ok(AuthoringKnowledgePack::IndexMissing),
        LocalChunkAcrossRetrieval::QueryMissing => return Ok(AuthoringKnowledgePack::QueryMissing),
    };

    let graph_links =
        query_suggested_main_links_for_sources(&connection, &source_titles, link_limit)?;
    let mut suggested_links = Vec::new();
    let mut seen_suggested_links = BTreeSet::new();
    for page in &related_pages {
        if suggested_links.len() >= link_limit {
            break;
        }
        if page.namespace == Namespace::Main.as_str()
            && !page.is_redirect
            && seen_suggested_links.insert(page.title.to_ascii_lowercase())
        {
            suggested_links.push(page.title.clone());
            if suggested_links.len() >= link_limit {
                break;
            }
        }
    }
    for link in graph_links {
        if suggested_links.len() >= link_limit {
            break;
        }
        if seen_suggested_links.insert(link.to_ascii_lowercase()) {
            suggested_links.push(link);
            if suggested_links.len() >= link_limit {
                break;
            }
        }
    }
    for chunk in &chunks {
        if suggested_links.len() >= link_limit {
            break;
        }
        if chunk.source_namespace != Namespace::Main.as_str() {
            continue;
        }
        if seen_suggested_links.insert(chunk.source_title.to_ascii_lowercase()) {
            suggested_links.push(chunk.source_title.clone());
            if suggested_links.len() >= link_limit {
                break;
            }
        }
    }

    let suggested_categories =
        query_suggested_categories_for_sources(&connection, &source_titles, category_limit)?;
    let suggested_templates =
        summarize_template_usage_for_sources(&connection, Some(&source_titles), template_limit)?;
    let template_baseline =
        summarize_template_usage_for_sources(&connection, None, template_limit)?;

    let inventory = load_authoring_inventory(&connection)?;

    Ok(AuthoringKnowledgePack::Found(Box::new(
        AuthoringKnowledgePackResult {
            topic,
            query,
            inventory,
            related_pages,
            suggested_links,
            suggested_categories,
            suggested_templates,
            template_baseline,
            stub_existing_links,
            stub_missing_links,
            stub_detected_templates,
            retrieval_mode,
            chunks,
            token_estimate_total,
        },
    )))
}

pub fn run_validation_checks(paths: &ResolvedPaths) -> Result<Option<ValidationReport>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };

    Ok(Some(ValidationReport {
        broken_links: query_broken_links_for_connection(&connection)?,
        double_redirects: query_double_redirects_for_connection(&connection)?,
        uncategorized_pages: query_uncategorized_pages_for_connection(&connection)?,
        orphan_pages: query_orphans_for_connection(&connection)?,
    }))
}

pub fn query_backlinks(paths: &ResolvedPaths, title: &str) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    let normalized = normalize_query_title(title);
    if normalized.is_empty() {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(query_backlinks_for_connection(
        &connection,
        &normalized,
    )?))
}

pub fn query_orphans(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    Ok(Some(query_orphans_for_connection(&connection)?))
}

pub fn query_empty_categories(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };

    let mut statement = connection
        .prepare(
            "SELECT p.title
             FROM indexed_pages p
             WHERE p.namespace = 'Category'
               AND NOT EXISTS (
                   SELECT 1
                   FROM indexed_links l
                   WHERE l.target_title = p.title
                     AND l.is_category_membership = 1
               )
             ORDER BY p.title ASC",
        )
        .context("failed to prepare empty category query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .context("failed to run empty category query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode empty category row")?);
    }
    Ok(Some(out))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLink {
    target_title: String,
    target_namespace: String,
    is_category_membership: bool,
}

#[derive(Debug, Clone)]
struct IndexedPageRecord {
    title: String,
    namespace: String,
    is_redirect: bool,
    redirect_target: Option<String>,
    relative_path: String,
    bytes: u64,
}

#[derive(Debug, Clone)]
struct IndexedLinkRow {
    target_title: String,
    target_namespace: String,
    is_category_membership: bool,
}

#[derive(Debug, Clone)]
struct IndexedContextChunkRow {
    section_heading: Option<String>,
    token_estimate: usize,
    chunk_text: String,
}

#[derive(Debug, Clone)]
struct RetrievedChunkCandidate {
    chunk: RetrievedChunk,
    lexical_signature: String,
    lexical_terms: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct ParsedTemplateInvocation {
    template_title: String,
    parameter_keys: Vec<String>,
}

#[derive(Debug, Clone)]
struct ArticleContextChunkRow {
    section_heading: Option<String>,
    chunk_text: String,
    token_estimate: usize,
}

fn load_context_chunks_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
    content: &str,
) -> Result<Vec<LocalContextChunk>> {
    if table_exists(connection, "indexed_page_chunks")? {
        let db_rows = load_indexed_context_chunks_for_connection(
            connection,
            source_relative_path,
            CONTEXT_CHUNK_LIMIT,
            CONTEXT_TOKEN_BUDGET,
        )?;
        if !db_rows.is_empty() {
            return Ok(db_rows);
        }
    }

    let fallback_rows = chunk_article_context(content);
    Ok(apply_context_chunk_budget(
        fallback_rows
            .into_iter()
            .map(|row| LocalContextChunk {
                section_heading: row.section_heading,
                token_estimate: row.token_estimate,
                chunk_text: row.chunk_text,
            })
            .collect(),
        CONTEXT_CHUNK_LIMIT,
        CONTEXT_TOKEN_BUDGET,
    ))
}

fn load_template_invocations_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
    content: &str,
) -> Result<Vec<LocalTemplateInvocation>> {
    if table_exists(connection, "indexed_template_invocations")? {
        let db_rows = load_indexed_template_invocations_for_connection(
            connection,
            source_relative_path,
            TEMPLATE_INVOCATION_LIMIT,
        )?;
        if !db_rows.is_empty() {
            return Ok(db_rows);
        }
    }
    Ok(summarize_template_invocations(
        extract_template_invocations(content),
        TEMPLATE_INVOCATION_LIMIT,
    ))
}

fn load_chunks_for_query(
    paths: &ResolvedPaths,
    connection: &Connection,
    source_relative_path: &str,
    normalized_query: Option<&str>,
    limit: usize,
) -> Result<(Vec<LocalContextChunk>, String)> {
    if table_exists(connection, "indexed_page_chunks")? {
        if let Some(query) = normalized_query {
            if fts_table_exists(connection, "indexed_page_chunks_fts")
                && let Ok(hits) = query_page_chunks_fts_for_connection(
                    connection,
                    source_relative_path,
                    query,
                    limit,
                )
                && !hits.is_empty()
            {
                return Ok((hits, "fts".to_string()));
            }

            let hits = query_page_chunks_like_for_connection(
                connection,
                source_relative_path,
                query,
                limit,
            )?;
            return Ok((hits, "like".to_string()));
        }

        let hits = load_indexed_context_chunks_for_connection(
            connection,
            source_relative_path,
            limit,
            usize::MAX,
        )?;
        return Ok((hits, "ordered".to_string()));
    }

    let content = fs::read_to_string(absolute_path_from_relative(paths, source_relative_path))
        .with_context(|| format!("failed to read indexed source file {source_relative_path}"))?;
    let mut chunks = chunk_article_context(&content)
        .into_iter()
        .map(|row| LocalContextChunk {
            section_heading: row.section_heading,
            token_estimate: row.token_estimate,
            chunk_text: row.chunk_text,
        })
        .collect::<Vec<_>>();
    if let Some(query) = normalized_query {
        let lowered = query.to_ascii_lowercase();
        chunks.retain(|chunk| chunk.chunk_text.to_ascii_lowercase().contains(&lowered));
        return Ok((chunks, "scan-like".to_string()));
    }
    Ok((chunks, "scan-ordered".to_string()))
}

fn candidate_limit(limit: usize, multiplier: usize) -> usize {
    limit
        .saturating_mul(multiplier.max(1))
        .clamp(limit.max(1), 512)
}

fn select_retrieved_chunks(
    candidates: Vec<RetrievedChunk>,
    limit: usize,
    token_budget: usize,
    diversify: bool,
    max_pages: Option<usize>,
    round_robin_pages: bool,
) -> Vec<RetrievedChunk> {
    let capped_limit = limit.max(1);
    let capped_token_budget = token_budget.max(1);
    let max_pages = max_pages.map(|value| value.max(1));

    let mut candidates = candidates
        .into_iter()
        .map(|chunk| {
            let lexical_terms = lexical_terms(&chunk.chunk_text);
            RetrievedChunkCandidate {
                lexical_signature: lexical_signature_from_terms(&lexical_terms),
                lexical_terms,
                chunk,
            }
        })
        .collect::<Vec<_>>();
    if round_robin_pages && max_pages.is_some() {
        candidates = round_robin_by_source(candidates, max_pages.unwrap_or(1));
    }

    let mut out = Vec::new();
    let mut used_tokens = 0usize;
    let mut used_signatures = BTreeSet::<String>::new();
    let mut selected_terms = Vec::<BTreeSet<String>>::new();
    let mut selected_pages = BTreeSet::<String>::new();

    for candidate in candidates {
        if out.len() >= capped_limit {
            break;
        }
        if used_signatures.contains(&candidate.lexical_signature) {
            continue;
        }
        if let Some(max_pages) = max_pages
            && !selected_pages.contains(&candidate.chunk.source_relative_path)
            && selected_pages.len() >= max_pages
        {
            continue;
        }
        if diversify
            && !selected_terms.is_empty()
            && selected_terms.iter().any(|terms| {
                lexical_similarity_terms(terms, &candidate.lexical_terms)
                    >= CHUNK_LEXICAL_SIMILARITY_THRESHOLD
            })
        {
            continue;
        }

        let next_tokens = used_tokens.saturating_add(candidate.chunk.token_estimate);
        if !out.is_empty() && next_tokens > capped_token_budget {
            continue;
        }

        used_tokens = next_tokens;
        used_signatures.insert(candidate.lexical_signature);
        selected_terms.push(candidate.lexical_terms);
        selected_pages.insert(candidate.chunk.source_relative_path.clone());
        out.push(candidate.chunk);
    }

    out
}

fn round_robin_by_source(
    candidates: Vec<RetrievedChunkCandidate>,
    max_pages: usize,
) -> Vec<RetrievedChunkCandidate> {
    let mut source_order = Vec::<String>::new();
    let mut buckets =
        BTreeMap::<String, std::collections::VecDeque<RetrievedChunkCandidate>>::new();
    for candidate in candidates {
        let source = candidate.chunk.source_relative_path.clone();
        if !buckets.contains_key(&source) {
            if source_order.len() >= max_pages {
                continue;
            }
            source_order.push(source.clone());
        }
        buckets.entry(source).or_default().push_back(candidate);
    }

    let mut out = Vec::new();
    loop {
        let mut made_progress = false;
        for source in &source_order {
            if let Some(bucket) = buckets.get_mut(source)
                && let Some(candidate) = bucket.pop_front()
            {
                out.push(candidate);
                made_progress = true;
            }
        }
        if !made_progress {
            break;
        }
    }
    out
}

fn lexical_signature_from_terms(terms: &BTreeSet<String>) -> String {
    terms.iter().cloned().collect::<Vec<_>>().join(" ")
}

fn lexical_terms(value: &str) -> BTreeSet<String> {
    value
        .split_whitespace()
        .map(|token| {
            token
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|token| token.len() >= 3)
        .collect()
}

fn lexical_similarity_terms(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(right).count();
    let union = left.union(right).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f32 / union as f32
}

fn query_chunks_fts_across_pages_for_connection(
    connection: &Connection,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<RetrievedChunk>> {
    let limit_i64 = i64::try_from(limit).context("chunk query limit does not fit into i64")?;
    let fts_query = format!("\"{normalized_query}\" *");
    let mut statement = connection
        .prepare(
            "SELECT c.source_title, c.source_namespace, c.source_relative_path, c.section_heading, c.token_estimate, c.chunk_text
             FROM indexed_page_chunks_fts fts
             JOIN indexed_page_chunks c ON c.rowid = fts.rowid
             WHERE indexed_page_chunks_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )
        .context("failed to prepare cross-page chunk FTS query")?;
    let rows = statement
        .query_map(params![fts_query, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(4)?;
            Ok(RetrievedChunk {
                source_title: row.get(0)?,
                source_namespace: row.get(1)?,
                source_relative_path: row.get(2)?,
                section_heading: row.get(3)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                chunk_text: row.get(5)?,
            })
        })
        .context("failed to run cross-page chunk FTS query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode cross-page chunk FTS row")?);
    }
    Ok(out)
}

fn query_chunks_like_across_pages_for_connection(
    connection: &Connection,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<RetrievedChunk>> {
    let wildcard = format!("%{normalized_query}%");
    let prefix = format!("{normalized_query}%");
    let limit_i64 = i64::try_from(limit).context("chunk query limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT source_title, source_namespace, source_relative_path, section_heading, token_estimate, chunk_text
             FROM indexed_page_chunks
             WHERE lower(chunk_text) LIKE lower(?1)
             ORDER BY
               CASE
                 WHEN lower(chunk_text) LIKE lower(?2) THEN 0
                 ELSE 1
               END,
               source_title ASC,
               chunk_index ASC
             LIMIT ?3",
        )
        .context("failed to prepare cross-page chunk LIKE query")?;
    let rows = statement
        .query_map(params![wildcard, prefix, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(4)?;
            Ok(RetrievedChunk {
                source_title: row.get(0)?,
                source_namespace: row.get(1)?,
                source_relative_path: row.get(2)?,
                section_heading: row.get(3)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                chunk_text: row.get(5)?,
            })
        })
        .context("failed to run cross-page chunk LIKE query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode cross-page chunk LIKE row")?);
    }
    Ok(out)
}

fn query_chunks_scan_across_pages(
    paths: &ResolvedPaths,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<RetrievedChunk>> {
    let lowered_query = normalized_query.to_ascii_lowercase();
    let files = scan_files(paths, &ScanOptions::default())?;
    let mut out = Vec::new();
    for file in files {
        let content = load_scanned_file_content(paths, &file)?;
        for chunk in chunk_article_context(&content) {
            if !chunk
                .chunk_text
                .to_ascii_lowercase()
                .contains(&lowered_query)
            {
                continue;
            }
            out.push(RetrievedChunk {
                source_title: file.title.clone(),
                source_namespace: file.namespace.clone(),
                source_relative_path: file.relative_path.clone(),
                section_heading: chunk.section_heading,
                token_estimate: chunk.token_estimate,
                chunk_text: chunk.chunk_text,
            });
            if out.len() >= limit {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

fn analyze_stub_hints(stub_content: Option<&str>) -> (Vec<String>, Vec<String>) {
    let Some(content) = stub_content else {
        return (Vec::new(), Vec::new());
    };

    let mut links = BTreeSet::new();
    for link in extract_wikilinks(content) {
        let normalized = normalize_query_title(&link.target_title);
        if !normalized.is_empty() {
            links.insert(normalized);
        }
    }

    let mut templates = BTreeSet::new();
    for invocation in extract_template_invocations(content) {
        templates.insert(invocation.template_title);
    }

    (links.into_iter().collect(), templates.into_iter().collect())
}

fn query_local_search_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<LocalSearchHit>> {
    let normalized = normalize_spaces(&query.replace('_', " "));
    if normalized.is_empty() {
        return Ok(Vec::new());
    }
    if fts_table_exists(connection, "indexed_pages_fts")
        && let Ok(hits) = query_search_fts(connection, &normalized, limit)
        && !hits.is_empty()
    {
        return Ok(hits);
    }
    query_search_like(connection, &normalized, limit)
}

fn collect_related_pages_for_authoring(
    connection: &Connection,
    stub_link_titles: &[String],
    search_hits: Vec<LocalSearchHit>,
    limit: usize,
) -> Result<Vec<AuthoringPageCandidate>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::<String>::new();

    for title in stub_link_titles {
        let normalized = normalize_query_title(title);
        if normalized.is_empty() {
            continue;
        }
        if let Some(page) = load_page_record(connection, &normalized)?
            && seen.insert(page.title.to_ascii_lowercase())
        {
            out.push(AuthoringPageCandidate {
                title: page.title,
                namespace: page.namespace,
                is_redirect: page.is_redirect,
                source: "stub-link".to_string(),
            });
            if out.len() >= limit {
                return Ok(out);
            }
        }
    }

    for hit in search_hits {
        if !seen.insert(hit.title.to_ascii_lowercase()) {
            continue;
        }
        out.push(AuthoringPageCandidate {
            title: hit.title,
            namespace: hit.namespace,
            is_redirect: hit.is_redirect,
            source: "topic-search".to_string(),
        });
        if out.len() >= limit {
            break;
        }
    }

    Ok(out)
}

fn query_suggested_main_links_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<String>> {
    if source_titles.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    if !table_exists(connection, "indexed_links")? {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT target_title, COUNT(*) AS frequency
         FROM indexed_links
         WHERE source_title IN ({placeholders})
           AND is_category_membership = 0
           AND target_namespace = ?
         GROUP BY target_title
         ORDER BY frequency DESC, target_title ASC
         LIMIT ?"
    );
    let limit_i64 = i64::try_from(limit).context("link suggestion limit does not fit into i64")?;
    let mut values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    values.push(rusqlite::types::Value::from(
        Namespace::Main.as_str().to_string(),
    ));
    values.push(rusqlite::types::Value::from(limit_i64));

    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare suggested link query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| row.get::<_, String>(0))
        .context("failed to run suggested link query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode suggested link row")?);
    }
    Ok(out)
}

fn query_suggested_categories_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<String>> {
    if source_titles.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    if !table_exists(connection, "indexed_links")? {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT target_title, COUNT(*) AS frequency
         FROM indexed_links
         WHERE source_title IN ({placeholders})
           AND is_category_membership = 1
         GROUP BY target_title
         ORDER BY frequency DESC, target_title ASC
         LIMIT ?"
    );
    let limit_i64 =
        i64::try_from(limit).context("category suggestion limit does not fit into i64")?;
    let mut values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    values.push(rusqlite::types::Value::from(limit_i64));

    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare suggested category query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| row.get::<_, String>(0))
        .context("failed to run suggested category query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode suggested category row")?);
    }
    Ok(out)
}

#[derive(Default)]
struct TemplateUsageAccumulator {
    usage_count: usize,
    parameter_key_counts: BTreeMap<String, usize>,
}

fn summarize_template_usage_for_sources(
    connection: &Connection,
    source_titles: Option<&[String]>,
    limit: usize,
) -> Result<Vec<TemplateUsageSummary>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if !table_exists(connection, "indexed_template_invocations")? {
        return Ok(Vec::new());
    }
    if let Some(source_titles) = source_titles
        && source_titles.is_empty()
    {
        return Ok(Vec::new());
    }

    let rows = load_template_invocation_rows_for_sources(connection, source_titles)?;
    let mut template_map = BTreeMap::<String, TemplateUsageAccumulator>::new();
    for (template_title, parameter_keys_serialized) in rows {
        let entry = template_map.entry(template_title).or_default();
        entry.usage_count = entry.usage_count.saturating_add(1);
        for key in parse_parameter_key_list(&parameter_keys_serialized) {
            let count = entry.parameter_key_counts.entry(key).or_insert(0);
            *count = count.saturating_add(1);
        }
    }

    let mut out = template_map
        .into_iter()
        .map(|(template_title, accumulator)| {
            let mut parameter_keys = accumulator
                .parameter_key_counts
                .into_iter()
                .collect::<Vec<_>>();
            parameter_keys
                .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
            TemplateUsageSummary {
                template_title,
                usage_count: accumulator.usage_count,
                parameter_keys: parameter_keys
                    .into_iter()
                    .map(|(key, _)| key)
                    .take(AUTHORING_TEMPLATE_KEY_LIMIT)
                    .collect(),
            }
        })
        .collect::<Vec<_>>();

    out.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| left.template_title.cmp(&right.template_title))
    });
    out.truncate(limit);
    Ok(out)
}

fn load_template_invocation_rows_for_sources(
    connection: &Connection,
    source_titles: Option<&[String]>,
) -> Result<Vec<(String, String)>> {
    let (sql, values) = if let Some(source_titles) = source_titles {
        let placeholders = std::iter::repeat_n("?", source_titles.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT template_title, parameter_keys
             FROM indexed_template_invocations
             WHERE source_title IN ({placeholders})"
        );
        let values = source_titles
            .iter()
            .cloned()
            .map(rusqlite::types::Value::from)
            .collect::<Vec<_>>();
        (sql, values)
    } else {
        (
            "SELECT template_title, parameter_keys FROM indexed_template_invocations".to_string(),
            Vec::new(),
        )
    };

    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare template invocation summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .context("failed to run template invocation summary query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode template invocation summary row")?);
    }
    Ok(out)
}

fn load_authoring_inventory(connection: &Connection) -> Result<AuthoringInventory> {
    let indexed_pages_total = count_query(connection, "SELECT COUNT(*) FROM indexed_pages")
        .context("failed to count indexed pages for authoring inventory")?;
    let main_pages = count_query(
        connection,
        "SELECT COUNT(*) FROM indexed_pages WHERE namespace = 'Main'",
    )
    .context("failed to count main pages for authoring inventory")?;
    let template_pages = count_query(
        connection,
        "SELECT COUNT(*) FROM indexed_pages WHERE namespace = 'Template'",
    )
    .context("failed to count template pages for authoring inventory")?;
    let indexed_links_total = if table_exists(connection, "indexed_links")? {
        count_query(connection, "SELECT COUNT(*) FROM indexed_links")
            .context("failed to count indexed links for authoring inventory")?
    } else {
        0
    };

    let (template_invocation_rows, distinct_templates_invoked) =
        if table_exists(connection, "indexed_template_invocations")? {
            (
                count_query(
                    connection,
                    "SELECT COUNT(*) FROM indexed_template_invocations",
                )
                .context("failed to count template invocation rows for authoring inventory")?,
                count_query(
                    connection,
                    "SELECT COUNT(DISTINCT template_title) FROM indexed_template_invocations",
                )
                .context("failed to count distinct templates for authoring inventory")?,
            )
        } else {
            (0, 0)
        };

    Ok(AuthoringInventory {
        indexed_pages_total,
        main_pages,
        template_pages,
        indexed_links_total,
        template_invocation_rows,
        distinct_templates_invoked,
    })
}

fn load_indexed_context_chunks_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    max_chunks: usize,
    token_budget: usize,
) -> Result<Vec<LocalContextChunk>> {
    let mut statement = connection
        .prepare(
            "SELECT section_heading, token_estimate, chunk_text
             FROM indexed_page_chunks
             WHERE source_relative_path = ?1
             ORDER BY chunk_index ASC",
        )
        .context("failed to prepare indexed_page_chunks query")?;
    let rows = statement
        .query_map([source_relative_path], |row| {
            let token_estimate_i64: i64 = row.get(1)?;
            Ok(IndexedContextChunkRow {
                section_heading: row.get(0)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                chunk_text: row.get(2)?,
            })
        })
        .context("failed to run indexed_page_chunks query")?;

    let mut out = Vec::new();
    for row in rows {
        let row = row.context("failed to decode indexed_page_chunks row")?;
        out.push(LocalContextChunk {
            section_heading: row.section_heading,
            token_estimate: row.token_estimate,
            chunk_text: row.chunk_text,
        });
    }
    Ok(apply_context_chunk_budget(out, max_chunks, token_budget))
}

fn query_page_chunks_fts_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<LocalContextChunk>> {
    let limit_i64 = i64::try_from(limit).context("chunk query limit does not fit into i64")?;
    let fts_query = format!("\"{normalized_query}\" *");
    let mut statement = connection
        .prepare(
            "SELECT c.section_heading, c.token_estimate, c.chunk_text
             FROM indexed_page_chunks_fts fts
             JOIN indexed_page_chunks c ON c.rowid = fts.rowid
             WHERE c.source_relative_path = ?1
               AND indexed_page_chunks_fts MATCH ?2
             ORDER BY rank
             LIMIT ?3",
        )
        .context("failed to prepare chunk FTS query")?;
    let rows = statement
        .query_map(params![source_relative_path, fts_query, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(1)?;
            Ok(LocalContextChunk {
                section_heading: row.get(0)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                chunk_text: row.get(2)?,
            })
        })
        .context("failed to run chunk FTS query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode chunk FTS row")?);
    }
    Ok(out)
}

fn query_page_chunks_like_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    normalized_query: &str,
    limit: usize,
) -> Result<Vec<LocalContextChunk>> {
    let wildcard = format!("%{normalized_query}%");
    let prefix = format!("{normalized_query}%");
    let limit_i64 = i64::try_from(limit).context("chunk query limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT section_heading, token_estimate, chunk_text
             FROM indexed_page_chunks
             WHERE source_relative_path = ?1
               AND lower(chunk_text) LIKE lower(?2)
             ORDER BY
               CASE
                 WHEN lower(chunk_text) LIKE lower(?3) THEN 0
                 ELSE 1
               END,
               chunk_index ASC
             LIMIT ?4",
        )
        .context("failed to prepare chunk LIKE query")?;
    let rows = statement
        .query_map(
            params![source_relative_path, wildcard, prefix, limit_i64],
            |row| {
                let token_estimate_i64: i64 = row.get(1)?;
                Ok(LocalContextChunk {
                    section_heading: row.get(0)?,
                    token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
                    chunk_text: row.get(2)?,
                })
            },
        )
        .context("failed to run chunk LIKE query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode chunk LIKE row")?);
    }
    Ok(out)
}

fn load_indexed_template_invocations_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    limit: usize,
) -> Result<Vec<LocalTemplateInvocation>> {
    let limit_i64 =
        i64::try_from(limit).context("template invocation limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT template_title, parameter_keys
             FROM indexed_template_invocations
             WHERE source_relative_path = ?1
             ORDER BY template_title ASC, parameter_keys ASC
             LIMIT ?2",
        )
        .context("failed to prepare indexed_template_invocations query")?;
    let rows = statement
        .query_map(params![source_relative_path, limit_i64], |row| {
            let template_title: String = row.get(0)?;
            let parameter_keys: String = row.get(1)?;
            Ok(LocalTemplateInvocation {
                template_title,
                parameter_keys: parse_parameter_key_list(&parameter_keys),
            })
        })
        .context("failed to run indexed_template_invocations query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode indexed_template_invocations row")?);
    }
    Ok(out)
}

fn parse_parameter_key_list(value: &str) -> Vec<String> {
    if value.trim().is_empty() || value == NO_PARAMETER_KEYS_SENTINEL {
        return Vec::new();
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn canonical_parameter_key_list(keys: &[String]) -> String {
    if keys.is_empty() {
        return NO_PARAMETER_KEYS_SENTINEL.to_string();
    }
    let mut normalized = Vec::new();
    for key in keys {
        let key = normalize_template_parameter_key(key);
        if !key.is_empty() {
            normalized.push(key);
        }
    }
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        return NO_PARAMETER_KEYS_SENTINEL.to_string();
    }
    normalized.join(",")
}

fn apply_context_chunk_budget(
    chunks: Vec<LocalContextChunk>,
    max_chunks: usize,
    token_budget: usize,
) -> Vec<LocalContextChunk> {
    let mut out = Vec::new();
    let mut used_tokens = 0usize;
    for chunk in chunks {
        if out.len() >= max_chunks {
            break;
        }
        let next_tokens = used_tokens.saturating_add(chunk.token_estimate);
        if !out.is_empty() && next_tokens > token_budget {
            break;
        }
        used_tokens = next_tokens;
        out.push(chunk);
    }
    out
}

fn chunk_article_context(content: &str) -> Vec<ArticleContextChunkRow> {
    let mut out = Vec::new();
    for (section_heading, text) in split_content_sections(content) {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        let mut cursor = 0usize;
        while cursor < words.len() {
            let end = (cursor + INDEX_CHUNK_WORD_TARGET).min(words.len());
            let chunk_text = words[cursor..end].join(" ");
            if !chunk_text.is_empty() {
                out.push(ArticleContextChunkRow {
                    section_heading: section_heading.clone(),
                    token_estimate: estimate_tokens(&chunk_text),
                    chunk_text,
                });
            }
            cursor = end;
        }
    }
    out
}

fn split_content_sections(content: &str) -> Vec<(Option<String>, String)> {
    let mut out = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(heading) = parse_heading_line(trimmed) {
            flush_content_section(&mut out, current_heading.take(), &current_lines);
            current_lines.clear();
            current_heading = Some(heading);
            continue;
        }
        current_lines.push(line);
    }
    flush_content_section(&mut out, current_heading, &current_lines);
    out
}

fn flush_content_section(
    out: &mut Vec<(Option<String>, String)>,
    section_heading: Option<String>,
    lines: &[&str],
) {
    let text = normalize_spaces(&lines.join(" "));
    if text.is_empty() {
        return;
    }
    out.push((section_heading, text));
}

fn parse_heading_line(value: &str) -> Option<String> {
    if value.len() < 4 || !value.starts_with('=') || !value.ends_with('=') {
        return None;
    }
    let leading = value.chars().take_while(|ch| *ch == '=').count();
    let trailing = value.chars().rev().take_while(|ch| *ch == '=').count();
    if leading != trailing || !(2..=6).contains(&leading) {
        return None;
    }
    if leading * 2 >= value.len() {
        return None;
    }
    let heading = value[leading..value.len() - trailing].trim();
    if heading.is_empty() {
        return None;
    }
    Some(heading.to_string())
}

fn estimate_tokens(value: &str) -> usize {
    value.chars().count().div_ceil(4)
}

fn summarize_template_invocations(
    invocations: Vec<ParsedTemplateInvocation>,
    limit: usize,
) -> Vec<LocalTemplateInvocation> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for invocation in invocations {
        let parameter_keys = canonical_parameter_key_list(&invocation.parameter_keys);
        let signature = format!("{}|{}", invocation.template_title, parameter_keys);
        if !seen.insert(signature) {
            continue;
        }
        out.push(LocalTemplateInvocation {
            template_title: invocation.template_title,
            parameter_keys: parse_parameter_key_list(&parameter_keys),
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn extract_template_invocations(content: &str) -> Vec<ParsedTemplateInvocation> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut stack = Vec::new();

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            stack.push(cursor + 2);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'}' && bytes[cursor + 1] == b'}' {
            if let Some(start) = stack.pop()
                && cursor >= start
            {
                let inner = &content[start..cursor];
                if let Some(invocation) = parse_template_invocation(inner) {
                    out.push(invocation);
                }
            }
            cursor += 2;
            continue;
        }
        cursor += 1;
    }

    out
}

fn parse_template_invocation(inner: &str) -> Option<ParsedTemplateInvocation> {
    let segments = split_template_segments(inner);
    let raw_name = segments.first()?.trim();
    let template_title = canonical_template_title(raw_name)?;

    let mut parameter_keys = Vec::new();
    let mut positional_index = 1usize;
    for segment in segments.iter().skip(1) {
        let value = segment.trim();
        if value.is_empty() {
            continue;
        }
        if let Some((key, _)) = split_once_top_level_equals(value) {
            let normalized = normalize_template_parameter_key(&key);
            if !normalized.is_empty() {
                parameter_keys.push(normalized);
                continue;
            }
        }
        parameter_keys.push(format!("${positional_index}"));
        positional_index += 1;
    }
    parameter_keys.sort();
    parameter_keys.dedup();

    Some(ParsedTemplateInvocation {
        template_title,
        parameter_keys,
    })
}

fn split_template_segments(inner: &str) -> Vec<String> {
    let chars: Vec<char> = inner.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut cursor = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;

    while cursor < chars.len() {
        let current_char = chars[cursor];
        let next_char = chars.get(cursor + 1).copied();
        if current_char == '{' && next_char == Some('{') {
            template_depth += 1;
            current.push('{');
            current.push('{');
            cursor += 2;
            continue;
        }
        if current_char == '}' && next_char == Some('}') {
            template_depth = template_depth.saturating_sub(1);
            current.push('}');
            current.push('}');
            cursor += 2;
            continue;
        }
        if current_char == '[' && next_char == Some('[') {
            link_depth += 1;
            current.push('[');
            current.push('[');
            cursor += 2;
            continue;
        }
        if current_char == ']' && next_char == Some(']') {
            link_depth = link_depth.saturating_sub(1);
            current.push(']');
            current.push(']');
            cursor += 2;
            continue;
        }
        if current_char == '|' && template_depth == 0 && link_depth == 0 {
            out.push(current.trim().to_string());
            current.clear();
            cursor += 1;
            continue;
        }
        current.push(current_char);
        cursor += 1;
    }

    out.push(current.trim().to_string());
    out
}

fn split_once_top_level_equals(value: &str) -> Option<(String, String)> {
    let chars: Vec<char> = value.chars().collect();
    let mut cursor = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;
    while cursor < chars.len() {
        let current_char = chars[cursor];
        let next_char = chars.get(cursor + 1).copied();
        if current_char == '{' && next_char == Some('{') {
            template_depth += 1;
            cursor += 2;
            continue;
        }
        if current_char == '}' && next_char == Some('}') {
            template_depth = template_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if current_char == '[' && next_char == Some('[') {
            link_depth += 1;
            cursor += 2;
            continue;
        }
        if current_char == ']' && next_char == Some(']') {
            link_depth = link_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if current_char == '=' && template_depth == 0 && link_depth == 0 {
            let key = chars[..cursor].iter().collect::<String>();
            let value = chars[cursor + 1..].iter().collect::<String>();
            return Some((key, value));
        }
        cursor += 1;
    }
    None
}

fn canonical_template_title(raw: &str) -> Option<String> {
    let mut name = normalize_spaces(&raw.replace('_', " "));
    while let Some(stripped) = name.strip_prefix(':') {
        name = stripped.trim_start().to_string();
    }
    if name.is_empty() {
        return None;
    }
    if name.starts_with('#')
        || name.starts_with('!')
        || name.contains('{')
        || name.contains('}')
        || name.contains('[')
        || name.contains(']')
    {
        return None;
    }

    if let Some((prefix, rest)) = name.split_once(':') {
        if !prefix.eq_ignore_ascii_case("Template") {
            return None;
        }
        let body = normalize_spaces(rest);
        if body.is_empty() {
            return None;
        }
        return Some(format!("Template:{body}"));
    }
    Some(format!("Template:{name}"))
}

fn normalize_template_parameter_key(value: &str) -> String {
    normalize_spaces(&value.replace('_', " ")).to_ascii_lowercase()
}

fn extract_wikilinks(content: &str) -> Vec<ParsedLink> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
            let start = cursor + 2;
            let mut end = start;
            while end + 1 < bytes.len() {
                if bytes[end] == b']' && bytes[end + 1] == b']' {
                    break;
                }
                end += 1;
            }
            if end + 1 >= bytes.len() {
                break;
            }

            let inner = &content[start..end];
            if let Some(link) = parse_wikilink(inner) {
                out.push(link);
            }
            cursor = end + 2;
            continue;
        }
        cursor += 1;
    }

    out
}

fn parse_wikilink(inner: &str) -> Option<ParsedLink> {
    let target_part = inner.split('|').next().unwrap_or("").trim();
    if target_part.is_empty() {
        return None;
    }

    let mut target = target_part;
    let mut leading_colon = false;
    while let Some(stripped) = target.strip_prefix(':') {
        leading_colon = true;
        target = stripped.trim_start();
    }
    if target.is_empty() {
        return None;
    }

    if let Some((without_fragment, _)) = target.split_once('#') {
        target = without_fragment.trim_end();
    }
    if target.is_empty() {
        return None;
    }

    if target.starts_with("http://") || target.starts_with("https://") || target.starts_with("//") {
        return None;
    }

    let target = normalize_spaces(&target.replace('_', " "));
    if target.is_empty() {
        return None;
    }

    let (title, namespace) = normalize_title_and_namespace(&target)?;
    let is_category_membership = namespace == Namespace::Category.as_str() && !leading_colon;

    Some(ParsedLink {
        target_title: title,
        target_namespace: namespace.to_string(),
        is_category_membership,
    })
}

fn normalize_title_and_namespace(value: &str) -> Option<(String, &'static str)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((prefix, rest)) = trimmed.split_once(':')
        && let Some(namespace) = canonical_namespace(prefix)
    {
        let body = normalize_spaces(rest);
        if body.is_empty() {
            return None;
        }
        return Some((format!("{namespace}:{body}"), namespace));
    }

    Some((trimmed.to_string(), Namespace::Main.as_str()))
}

fn canonical_namespace(prefix: &str) -> Option<&'static str> {
    let trimmed = prefix.trim();
    if trimmed.eq_ignore_ascii_case("Category") {
        return Some(Namespace::Category.as_str());
    }
    if trimmed.eq_ignore_ascii_case("File") {
        return Some(Namespace::File.as_str());
    }
    if trimmed.eq_ignore_ascii_case("User") {
        return Some(Namespace::User.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Goldenlight") {
        return Some(Namespace::Goldenlight.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Template") {
        return Some(Namespace::Template.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Module") {
        return Some(Namespace::Module.as_str());
    }
    if trimmed.eq_ignore_ascii_case("MediaWiki") {
        return Some(Namespace::MediaWiki.as_str());
    }
    None
}

fn normalize_query_title(title: &str) -> String {
    let normalized = normalize_spaces(&title.replace('_', " "));
    if normalized.is_empty() {
        return normalized;
    }
    match normalize_title_and_namespace(&normalized) {
        Some((value, _)) => value,
        None => String::new(),
    }
}

fn normalize_spaces(value: &str) -> String {
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

fn load_page_record(connection: &Connection, title: &str) -> Result<Option<IndexedPageRecord>> {
    let mut statement = connection
        .prepare(
            "SELECT
                title,
                namespace,
                is_redirect,
                redirect_target,
                relative_path,
                bytes
             FROM indexed_pages
             WHERE lower(title) = lower(?1)
             LIMIT 1",
        )
        .context("failed to prepare page record lookup")?;

    let mut rows = statement
        .query([title])
        .context("failed to run page record lookup")?;
    let row = match rows.next().context("failed to read page record row")? {
        Some(row) => row,
        None => return Ok(None),
    };

    let bytes_i64: i64 = row.get(5).context("failed to decode page bytes")?;
    let bytes = u64::try_from(bytes_i64).context("page bytes are negative")?;
    Ok(Some(IndexedPageRecord {
        title: row.get(0).context("failed to decode page title")?,
        namespace: row.get(1).context("failed to decode page namespace")?,
        is_redirect: row
            .get::<_, i64>(2)
            .context("failed to decode redirect flag")?
            == 1,
        redirect_target: row.get(3).context("failed to decode redirect target")?,
        relative_path: row.get(4).context("failed to decode relative path")?,
        bytes,
    }))
}

fn load_outgoing_link_rows(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Vec<IndexedLinkRow>> {
    let mut statement = connection
        .prepare(
            "SELECT target_title, target_namespace, is_category_membership
             FROM indexed_links
             WHERE source_relative_path = ?1
             ORDER BY target_title ASC",
        )
        .context("failed to prepare outgoing links query")?;
    let rows = statement
        .query_map([source_relative_path], |row| {
            let target_title: String = row.get(0)?;
            let target_namespace: String = row.get(1)?;
            let is_category_membership: i64 = row.get(2)?;
            Ok(IndexedLinkRow {
                target_title,
                target_namespace,
                is_category_membership: is_category_membership == 1,
            })
        })
        .context("failed to run outgoing links query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode outgoing link row")?);
    }
    Ok(out)
}

fn query_backlinks_for_connection(connection: &Connection, title: &str) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT source_title
             FROM indexed_links
             WHERE target_title = ?1
             ORDER BY source_title ASC",
        )
        .context("failed to prepare backlinks query")?;
    let rows = statement
        .query_map([title], |row| row.get::<_, String>(0))
        .context("failed to run backlinks query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode backlinks row")?);
    }
    Ok(out)
}

fn query_orphans_for_connection(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT p.title
             FROM indexed_pages p
             WHERE p.namespace = 'Main'
               AND p.is_redirect = 0
               AND NOT EXISTS (
                   SELECT 1
                   FROM indexed_links l
                   JOIN indexed_pages src ON src.relative_path = l.source_relative_path
                   WHERE l.target_title = p.title
                     AND src.namespace = 'Main'
                     AND src.is_redirect = 0
                     AND src.title <> p.title
               )
             ORDER BY p.title ASC",
        )
        .context("failed to prepare orphan query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .context("failed to run orphan query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode orphan row")?);
    }
    Ok(out)
}

fn query_broken_links_for_connection(connection: &Connection) -> Result<Vec<BrokenLinkIssue>> {
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT l.source_title, l.target_title
             FROM indexed_links l
             LEFT JOIN indexed_pages p ON p.title = l.target_title
             WHERE l.target_namespace = 'Main'
               AND p.title IS NULL
             ORDER BY l.source_title ASC, l.target_title ASC",
        )
        .context("failed to prepare broken-links query")?;
    let rows = statement
        .query_map([], |row| {
            Ok(BrokenLinkIssue {
                source_title: row.get(0)?,
                target_title: row.get(1)?,
            })
        })
        .context("failed to run broken-links query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode broken-link row")?);
    }
    Ok(out)
}

fn query_double_redirects_for_connection(
    connection: &Connection,
) -> Result<Vec<DoubleRedirectIssue>> {
    let mut statement = connection
        .prepare(
            "SELECT
                p.title,
                p.redirect_target,
                p2.redirect_target
             FROM indexed_pages p
             JOIN indexed_pages p2 ON p.redirect_target = p2.title
             WHERE p.is_redirect = 1
               AND p2.is_redirect = 1
             ORDER BY p.title ASC",
        )
        .context("failed to prepare double-redirect query")?;
    let rows = statement
        .query_map([], |row| {
            let first_target: String = row.get(1)?;
            let final_target: String = row.get(2)?;
            Ok(DoubleRedirectIssue {
                title: row.get(0)?,
                first_target,
                final_target,
            })
        })
        .context("failed to run double-redirect query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode double-redirect row")?);
    }
    Ok(out)
}

fn query_uncategorized_pages_for_connection(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT p.title
             FROM indexed_pages p
             WHERE p.namespace = 'Main'
               AND p.is_redirect = 0
               AND NOT EXISTS (
                   SELECT 1
                   FROM indexed_links l
                   WHERE l.source_relative_path = p.relative_path
                     AND l.is_category_membership = 1
               )
             ORDER BY p.title ASC",
        )
        .context("failed to prepare uncategorized query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .context("failed to run uncategorized query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode uncategorized row")?);
    }
    Ok(out)
}

fn count_words(content: &str) -> usize {
    content
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .count()
}

fn make_content_preview(content: &str, max_chars: usize) -> String {
    let normalized = normalize_spaces(content);
    if normalized.len() <= max_chars {
        return normalized;
    }
    let output = normalized.chars().take(max_chars).collect::<String>();
    format!("{output}...")
}

fn parse_section_headings(content: &str, max_sections: usize) -> Vec<LocalContextHeading> {
    let mut out = Vec::new();
    if max_sections == 0 {
        return out;
    }

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.len() < 4 || !trimmed.starts_with('=') || !trimmed.ends_with('=') {
            continue;
        }

        let leading = trimmed.chars().take_while(|ch| *ch == '=').count();
        let trailing = trimmed.chars().rev().take_while(|ch| *ch == '=').count();
        if leading != trailing || !(2..=6).contains(&leading) {
            continue;
        }
        if leading * 2 >= trimmed.len() {
            continue;
        }

        let heading = trimmed[leading..trimmed.len() - trailing].trim();
        if heading.is_empty() {
            continue;
        }
        out.push(LocalContextHeading {
            level: u8::try_from(leading).unwrap_or(6),
            heading: heading.to_string(),
        });
        if out.len() >= max_sections {
            break;
        }
    }

    out
}

fn summarize_files(files: &[ScannedFile]) -> ScanStats {
    let mut by_namespace = BTreeMap::new();
    let mut content_files = 0usize;
    let mut template_files = 0usize;
    let mut redirects = 0usize;

    for file in files {
        *by_namespace.entry(file.namespace.clone()).or_insert(0) += 1;
        match file.namespace.as_str() {
            value
                if value == Namespace::Template.as_str()
                    || value == Namespace::Module.as_str()
                    || value == Namespace::MediaWiki.as_str() =>
            {
                template_files += 1;
            }
            _ => {
                content_files += 1;
            }
        }
        if file.is_redirect {
            redirects += 1;
        }
    }

    ScanStats {
        total_files: files.len(),
        content_files,
        template_files,
        redirects,
        by_namespace,
    }
}

fn load_scanned_file_content(paths: &ResolvedPaths, file: &ScannedFile) -> Result<String> {
    let absolute = absolute_path_from_relative(paths, &file.relative_path);
    fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))
}

fn absolute_path_from_relative(paths: &ResolvedPaths, relative: &str) -> PathBuf {
    let mut out = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            out.push(segment);
        }
    }
    out
}

fn open_connection(db_path: &Path) -> Result<Connection> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
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

fn open_indexed_connection(paths: &ResolvedPaths) -> Result<Option<Connection>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }
    let connection = open_connection(&paths.db_path)?;
    if !table_exists(&connection, "indexed_pages")? || !table_exists(&connection, "indexed_links")?
    {
        return Ok(None);
    }
    Ok(Some(connection))
}

fn initialize_schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(INDEX_SCHEMA_SQL)
        .context("failed to initialize index schema")
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

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool> {
    let exists: i64 = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to check sqlite_master for table {table_name}"))?;
    Ok(exists == 1)
}

fn count_query(connection: &Connection, sql: &str) -> Result<usize> {
    let count: i64 = connection
        .query_row(sql, [], |row| row.get(0))
        .with_context(|| format!("failed query: {sql}"))?;
    usize::try_from(count).context("count does not fit into usize")
}

fn namespace_counts(connection: &Connection) -> Result<BTreeMap<String, usize>> {
    let mut statement = connection
        .prepare(
            "SELECT namespace, COUNT(*) AS count
             FROM indexed_pages
             GROUP BY namespace
             ORDER BY namespace ASC",
        )
        .context("failed to prepare namespace aggregation query")?;

    let rows = statement
        .query_map([], |row| {
            let namespace: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((namespace, count))
        })
        .context("failed to run namespace aggregation query")?;

    let mut out = BTreeMap::new();
    for row in rows {
        let (namespace, count) = row.context("failed to read namespace aggregation row")?;
        let count = usize::try_from(count).context("namespace count does not fit into usize")?;
        out.insert(namespace, count);
    }
    Ok(out)
}

fn fts_table_exists(connection: &Connection, table_name: &str) -> bool {
    table_exists(connection, table_name).unwrap_or(false)
}

fn rebuild_fts_index(connection: &Connection) {
    if fts_table_exists(connection, "indexed_pages_fts") {
        let _ = connection
            .execute_batch("INSERT INTO indexed_pages_fts(indexed_pages_fts) VALUES('rebuild')");
    }
    if fts_table_exists(connection, "indexed_page_chunks_fts") {
        let _ = connection.execute_batch(
            "INSERT INTO indexed_page_chunks_fts(indexed_page_chunks_fts) VALUES('rebuild')",
        );
    }
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use super::{
        AuthoringKnowledgePack, AuthoringKnowledgePackOptions, BrokenLinkIssue,
        LocalChunkAcrossRetrieval, LocalChunkRetrieval, build_authoring_knowledge_pack,
        build_local_context, extract_template_invocations, extract_wikilinks,
        load_stored_index_stats, query_backlinks, query_empty_categories, query_orphans,
        query_search_local, rebuild_index, retrieve_local_context_chunks,
        retrieve_local_context_chunks_across_pages, run_validation_checks,
    };
    use crate::filesystem::{Namespace, ScanOptions};
    use crate::runtime::{ResolvedPaths, ValueSource};

    fn write_file(path: &Path, content: &str) {
        let parent = path.parent().expect("parent");
        fs::create_dir_all(parent).expect("create parent");
        fs::write(path, content).expect("write file");
    }

    fn paths(project_root: &Path) -> ResolvedPaths {
        ResolvedPaths {
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
            project_root: project_root.to_path_buf(),
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn extract_wikilinks_parses_titles_and_category_membership() {
        let content = "[[Alpha|label]] [[Category:People]] [[:Category:People]] [[Module:Navbar/configuration]] [[Alpha#History]] [[https://example.com]]";
        let links = extract_wikilinks(content);

        assert_eq!(links.len(), 5);
        assert_eq!(links[0].target_title, "Alpha");
        assert!(!links[0].is_category_membership);
        assert_eq!(links[1].target_title, "Category:People");
        assert!(links[1].is_category_membership);
        assert_eq!(links[2].target_title, "Category:People");
        assert!(!links[2].is_category_membership);
        assert_eq!(links[3].target_title, "Module:Navbar/configuration");
        assert_eq!(links[4].target_title, "Alpha");
    }

    #[test]
    fn rebuild_index_persists_scan_rows() {
        let temp = tempdir().expect("tempdir");
        let project_root: PathBuf = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Alpha article",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("Foo.wiki"),
            "#REDIRECT [[Category:Bar]]",
        );
        write_file(
            &paths
                .templates_dir
                .join("navbox")
                .join("Module_Navbar")
                .join("configuration.lua"),
            "return {}",
        );

        let report = rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
        assert!(paths.db_path.exists());
        assert_eq!(report.inserted_rows, 3);
        assert_eq!(report.scan.total_files, 3);
        assert_eq!(report.scan.redirects, 1);

        let stored = load_stored_index_stats(&paths)
            .expect("load stats")
            .expect("stats must exist");
        assert_eq!(stored.indexed_rows, 3);
        assert_eq!(stored.redirects, 1);
        assert_eq!(
            stored.by_namespace,
            BTreeMap::from([
                (Namespace::Category.as_str().to_string(), 1usize),
                (Namespace::Main.as_str().to_string(), 1usize),
                (Namespace::Module.as_str().to_string(), 1usize),
            ])
        );
    }

    #[test]
    fn query_backlinks_orphans_and_empty_categories() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "[[Beta]] [[Category:People]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "No links here",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "[[Beta]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("People.wiki"),
            "People category",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("Empty.wiki"),
            "Empty category",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let backlinks = query_backlinks(&paths, "Beta")
            .expect("backlinks query")
            .expect("backlinks should exist");
        assert_eq!(backlinks, vec!["Alpha".to_string(), "Gamma".to_string()]);

        let orphans = query_orphans(&paths)
            .expect("orphans query")
            .expect("orphans should exist");
        assert_eq!(orphans, vec!["Alpha".to_string(), "Gamma".to_string()]);

        let empty_categories = query_empty_categories(&paths)
            .expect("empty category query")
            .expect("empty categories should exist");
        assert_eq!(empty_categories, vec!["Category:Empty".to_string()]);
    }

    #[test]
    fn query_search_and_context_bundle() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Lead paragraph\n{{Infobox person|name=Alpha|birth_date={{Birth date|2000|1|1}}}}\n== History ==\n[[Beta]] [[Module:Navbar]] [[Category:People]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "No links here",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "[[Beta]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("People.wiki"),
            "People category",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let search = query_search_local(&paths, "be", 20)
            .expect("search query")
            .expect("search should be available");
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].title, "Beta");

        let context = build_local_context(&paths, "Alpha")
            .expect("context query")
            .expect("alpha context exists");
        assert_eq!(context.title, "Alpha");
        assert_eq!(context.namespace, "Main");
        assert_eq!(context.sections.len(), 1);
        assert_eq!(context.sections[0].heading, "History");
        assert_eq!(context.categories, vec!["Category:People".to_string()]);
        assert!(
            context
                .templates
                .contains(&"Template:Infobox person".to_string())
        );
        assert!(
            context
                .templates
                .contains(&"Template:Birth date".to_string())
        );
        assert_eq!(context.modules, vec!["Module:Navbar".to_string()]);
        assert_eq!(context.backlinks.len(), 0);
        let infobox_invocation = context
            .template_invocations
            .iter()
            .find(|invocation| invocation.template_title == "Template:Infobox person")
            .expect("infobox invocation");
        assert_eq!(
            infobox_invocation.parameter_keys,
            vec!["birth date".to_string(), "name".to_string()]
        );
        let birth_date_invocation = context
            .template_invocations
            .iter()
            .find(|invocation| invocation.template_title == "Template:Birth date")
            .expect("birth date invocation");
        assert_eq!(
            birth_date_invocation.parameter_keys,
            vec!["$1".to_string(), "$2".to_string(), "$3".to_string()]
        );

        let beta_context = build_local_context(&paths, "Beta")
            .expect("beta context query")
            .expect("beta context exists");
        assert_eq!(
            beta_context.backlinks,
            vec!["Alpha".to_string(), "Gamma".to_string()]
        );
    }

    #[test]
    fn extract_template_invocations_captures_nested_templates() {
        let content =
            "{{Infobox person|name=Alpha|birth_date={{Birth date|2000|1|1}}}} {{#if:foo|bar|baz}}";
        let invocations = extract_template_invocations(content);

        let infobox = invocations
            .iter()
            .find(|invocation| invocation.template_title == "Template:Infobox person")
            .expect("infobox invocation");
        assert_eq!(
            infobox.parameter_keys,
            vec!["birth date".to_string(), "name".to_string()]
        );

        let birth_date = invocations
            .iter()
            .find(|invocation| invocation.template_title == "Template:Birth date")
            .expect("birth date invocation");
        assert_eq!(
            birth_date.parameter_keys,
            vec!["$1".to_string(), "$2".to_string(), "$3".to_string()]
        );
        assert!(
            invocations
                .iter()
                .all(|invocation| !invocation.template_title.starts_with("Template:#"))
        );
    }

    #[test]
    fn retrieve_local_context_chunks_returns_index_missing_when_not_built() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        let retrieval = retrieve_local_context_chunks(&paths, "Alpha", None, 4, 200)
            .expect("retrieve chunks without index");
        assert_eq!(retrieval, LocalChunkRetrieval::IndexMissing);
    }

    #[test]
    fn retrieve_local_context_chunks_supports_query_and_budget() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Lead paragraph with CinderSignal marker and extra tokens for chunking.\n== History ==\nThis section carries CinderSignal data for retrieval testing and deterministic filtering.",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let retrieval = retrieve_local_context_chunks(&paths, "Alpha", Some("CinderSignal"), 3, 80)
            .expect("retrieve chunks with query");
        let report = match retrieval {
            LocalChunkRetrieval::Found(report) => report,
            other => panic!("expected found report, got {other:?}"),
        };
        assert_eq!(report.title, "Alpha");
        assert_eq!(report.query.as_deref(), Some("CinderSignal"));
        assert_eq!(report.retrieval_mode, "like");
        assert!(!report.chunks.is_empty());
        assert!(report.token_estimate_total <= 80);
        assert!(
            report
                .chunks
                .iter()
                .all(|chunk| chunk.chunk_text.contains("CinderSignal"))
        );
    }

    #[test]
    fn retrieve_local_context_chunks_across_pages_requires_query() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);
        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Alpha chunk body",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let retrieval = retrieve_local_context_chunks_across_pages(&paths, " ", 4, 200, 2, true)
            .expect("across-pages retrieval");
        assert_eq!(retrieval, LocalChunkAcrossRetrieval::QueryMissing);
    }

    #[test]
    fn retrieve_local_context_chunks_across_pages_returns_multi_source_chunks() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Lead AlphaSignal signal chunk one.\n== A ==\nAlphaSignal chunk two with overlap.",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "Lead AlphaSignal beta chunk one.\n== B ==\nAlphaSignal beta chunk two with overlap.",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "Lead AlphaSignal gamma chunk one.\n== C ==\nAlphaSignal gamma chunk two with overlap.",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let retrieval =
            retrieve_local_context_chunks_across_pages(&paths, "AlphaSignal", 4, 140, 2, true)
                .expect("across-pages retrieval");
        let report = match retrieval {
            LocalChunkAcrossRetrieval::Found(report) => report,
            other => panic!("expected found report, got {other:?}"),
        };
        assert!(report.retrieval_mode.contains("across"));
        assert!(report.source_page_count <= 2);
        assert!(report.token_estimate_total <= 140);
        assert!(!report.chunks.is_empty());
        let unique_sources = report
            .chunks
            .iter()
            .map(|chunk| chunk.source_relative_path.as_str())
            .collect::<BTreeSet<_>>();
        assert!(unique_sources.len() <= 2);
        assert!(
            report
                .chunks
                .iter()
                .all(|chunk| chunk.chunk_text.contains("AlphaSignal"))
        );
    }

    #[test]
    fn build_authoring_knowledge_pack_requires_topic_or_stub_signal() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);
        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Alpha body text",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let report = build_authoring_knowledge_pack(
            &paths,
            None,
            None,
            &AuthoringKnowledgePackOptions::default(),
        )
        .expect("authoring pack");
        assert_eq!(report, AuthoringKnowledgePack::QueryMissing);
    }

    #[test]
    fn build_authoring_knowledge_pack_collects_templates_links_and_chunks() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "{{Infobox person|name=Alpha|born=2020}}\n'''Alpha''' works with [[Beta]] and [[Gamma]].\n[[Category:People]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "{{Infobox organization|name=Beta Org|founder=Alpha}}\n'''Beta''' references [[Alpha]] and [[Gamma]].\n[[Category:Organizations]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "{{Navbox|name=Gamma nav|list1=[[Alpha]]}}\n'''Gamma''' is linked with [[Alpha]].\n[[Category:People]]",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let options = AuthoringKnowledgePackOptions {
            related_page_limit: 6,
            chunk_limit: 6,
            token_budget: 420,
            max_pages: 4,
            link_limit: 8,
            category_limit: 4,
            template_limit: 6,
            diversify: true,
        };
        let report = build_authoring_knowledge_pack(
            &paths,
            Some("Alpha"),
            Some("{{Infobox person|name=Draft}}\nDraft body with [[Alpha]] and [[Missing Page]]."),
            &options,
        )
        .expect("authoring pack");

        let report = match report {
            AuthoringKnowledgePack::Found(report) => *report,
            other => panic!("expected found authoring pack, got {other:?}"),
        };
        assert_eq!(report.topic, "Alpha");
        assert_eq!(report.query, "Alpha");
        assert!(report.inventory.indexed_pages_total >= 3);
        assert!(!report.related_pages.is_empty());
        assert!(report.suggested_links.contains(&"Alpha".to_string()));
        assert!(
            report
                .suggested_templates
                .iter()
                .any(|entry| entry.template_title == "Template:Infobox person")
        );
        assert!(!report.template_baseline.is_empty());
        assert!(report.stub_existing_links.contains(&"Alpha".to_string()));
        assert!(
            report
                .stub_missing_links
                .contains(&"Missing Page".to_string())
        );
        assert!(
            report
                .stub_detected_templates
                .contains(&"Template:Infobox person".to_string())
        );
        assert!(report.retrieval_mode.contains("across"));
        assert!(report.token_estimate_total <= 420);
    }

    #[test]
    fn validation_checks_report_expected_issues() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "[[Beta]] [[MissingTarget]] [[Category:People]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "#REDIRECT [[Gamma]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "#REDIRECT [[Delta]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("NoCategory.wiki"),
            "Standalone page",
        );
        write_file(
            &paths.wiki_content_dir.join("Category").join("People.wiki"),
            "People category",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
        let report = run_validation_checks(&paths)
            .expect("validate query")
            .expect("validation should be available");

        assert_eq!(report.broken_links.len(), 2);
        assert!(report.broken_links.contains(&BrokenLinkIssue {
            source_title: "Alpha".to_string(),
            target_title: "MissingTarget".to_string(),
        }));
        assert!(report.broken_links.contains(&BrokenLinkIssue {
            source_title: "Gamma".to_string(),
            target_title: "Delta".to_string(),
        }));
        assert_eq!(report.double_redirects.len(), 1);
        assert_eq!(report.double_redirects[0].title, "Beta");
        assert!(
            report
                .uncategorized_pages
                .contains(&"NoCategory".to_string())
        );
        assert!(report.orphan_pages.contains(&"Alpha".to_string()));
    }

    #[test]
    fn load_stored_index_stats_returns_none_when_db_is_missing() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        let stored = load_stored_index_stats(&paths).expect("load stats");
        assert!(stored.is_none());
    }
}
