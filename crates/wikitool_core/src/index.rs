use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use reqwest::Url;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::Serialize;
use serde_json::json;

use crate::filesystem::{
    Namespace, ScanOptions, ScanStats, ScannedFile, scan_files, validate_scoped_path,
};
use crate::knowledge::status::{DEFAULT_DOCS_PROFILE, record_content_index_artifact};
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{normalize_path, table_exists, unix_timestamp};

const INDEX_CHUNK_WORD_TARGET: usize = 96;
const CONTEXT_CHUNK_LIMIT: usize = 8;
const CONTEXT_TOKEN_BUDGET: usize = 720;
const TEMPLATE_INVOCATION_LIMIT: usize = 24;
const NO_PARAMETER_KEYS_SENTINEL: &str = "__none__";
const CHUNK_CANDIDATE_MULTIPLIER_SINGLE: usize = 6;
const CHUNK_CANDIDATE_MULTIPLIER_ACROSS: usize = 10;
const CHUNK_LEXICAL_SIMILARITY_THRESHOLD: f32 = 0.86;
const AUTHORING_TEMPLATE_KEY_LIMIT: usize = 12;
const TEMPLATE_REFERENCE_EXAMPLE_LIMIT: usize = 3;
const TEMPLATE_IMPLEMENTATION_PAGE_LIMIT: usize = 6;
const TEMPLATE_PARAMETER_VALUE_LIMIT: usize = 3;
const MODULE_REFERENCE_EXAMPLE_LIMIT: usize = 4;
const AUTHORING_MODULE_FUNCTION_LIMIT: usize = 8;
const AUTHORING_TEMPLATE_REFERENCE_LIMIT: usize = 4;
const AUTHORING_MODULE_PATTERN_LIMIT: usize = 6;
const AUTHORING_REFERENCE_LIMIT: usize = 8;
const AUTHORING_REFERENCE_EXAMPLE_LIMIT: usize = 3;
const AUTHORING_REFERENCE_AUTHORITY_LIMIT: usize = 8;
const AUTHORING_REFERENCE_DOMAIN_LIMIT: usize = 6;
const AUTHORING_REFERENCE_FLAG_LIMIT: usize = 8;
const AUTHORING_REFERENCE_IDENTIFIER_LIMIT: usize = 8;
const AUTHORING_MEDIA_LIMIT: usize = 8;
const AUTHORING_MEDIA_EXAMPLE_LIMIT: usize = 3;
const AUTHORING_PAGE_SUMMARY_WORD_LIMIT: usize = 36;
const AUTHORING_QUERY_EXPANSION_LIMIT: usize = 8;
const AUTHORING_SECTION_LIMIT: usize = 24;
const AUTHORING_SUGGESTION_EVIDENCE_LIMIT: usize = 4;
const AUTHORING_SEED_CHUNKS_PER_PAGE: usize = 2;
const CONTEXT_REFERENCE_LIMIT: usize = 12;
const CONTEXT_MEDIA_LIMIT: usize = 12;
const NO_STRING_LIST_SENTINEL: &str = "__none__";

#[path = "index/authoring.rs"]
mod authoring;
#[path = "index/references.rs"]
mod references;
#[path = "index/retrieval.rs"]
mod retrieval;
#[path = "index/ingest.rs"]
mod ingest;
#[path = "index/templates.rs"]
mod templates;
#[path = "index/validation.rs"]
mod validation;

use self::retrieval::candidate_limit;

pub use authoring::build_authoring_knowledge_pack;
pub use retrieval::{
    build_local_context, query_search_local, retrieve_local_context_chunks,
    retrieve_local_context_chunks_across_pages, retrieve_local_context_chunks_with_options,
};
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalReferenceUsage {
    pub section_heading: Option<String>,
    pub reference_name: Option<String>,
    pub reference_group: Option<String>,
    pub citation_profile: String,
    pub citation_family: String,
    pub primary_template_title: Option<String>,
    pub source_type: String,
    pub source_origin: String,
    pub source_family: String,
    pub authority_kind: String,
    pub source_authority: String,
    pub reference_title: String,
    pub source_container: String,
    pub source_author: String,
    pub source_domain: String,
    pub source_date: String,
    pub canonical_url: String,
    pub identifier_keys: Vec<String>,
    pub identifier_entries: Vec<String>,
    pub source_urls: Vec<String>,
    pub retrieval_signals: Vec<String>,
    pub summary_text: String,
    pub template_titles: Vec<String>,
    pub link_titles: Vec<String>,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalMediaUsage {
    pub section_heading: Option<String>,
    pub file_title: String,
    pub media_kind: String,
    pub caption_text: String,
    pub options: Vec<String>,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalSectionSummary {
    pub section_heading: Option<String>,
    pub section_level: u8,
    pub summary_text: String,
    pub token_estimate: usize,
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
    pub section_summaries: Vec<LocalSectionSummary>,
    pub context_chunks: Vec<LocalContextChunk>,
    pub context_tokens_estimate: usize,
    pub outgoing_links: Vec<String>,
    pub backlinks: Vec<String>,
    pub categories: Vec<String>,
    pub templates: Vec<String>,
    pub modules: Vec<String>,
    pub template_invocations: Vec<LocalTemplateInvocation>,
    pub references: Vec<LocalReferenceUsage>,
    pub media: Vec<LocalMediaUsage>,
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
    pub semantic_profiles_total: usize,
    pub main_pages: usize,
    pub template_pages: usize,
    pub indexed_links_total: usize,
    pub template_invocation_rows: usize,
    pub distinct_templates_invoked: usize,
    pub module_invocation_rows_total: usize,
    pub distinct_modules_invoked: usize,
    pub reference_rows_total: usize,
    pub reference_authority_rows_total: usize,
    pub reference_identifier_rows_total: usize,
    pub distinct_reference_profiles: usize,
    pub media_rows_total: usize,
    pub distinct_media_files: usize,
    pub template_implementation_rows_total: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthoringPageCandidate {
    pub title: String,
    pub namespace: String,
    pub is_redirect: bool,
    pub source: String,
    pub retrieval_weight: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthoringSuggestion {
    pub title: String,
    pub support_count: usize,
    pub evidence_titles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TemplateParameterUsage {
    pub key: String,
    pub usage_count: usize,
    pub example_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TemplateInvocationExample {
    pub source_title: String,
    pub source_relative_path: String,
    pub parameter_keys: Vec<String>,
    pub invocation_text: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModuleFunctionUsage {
    pub function_name: String,
    pub usage_count: usize,
    pub example_parameter_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModuleInvocationExample {
    pub source_title: String,
    pub source_relative_path: String,
    pub function_name: String,
    pub parameter_keys: Vec<String>,
    pub invocation_text: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModuleUsageSummary {
    pub module_title: String,
    pub usage_count: usize,
    pub distinct_page_count: usize,
    pub function_stats: Vec<ModuleFunctionUsage>,
    pub example_pages: Vec<String>,
    pub example_invocations: Vec<ModuleInvocationExample>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TemplateUsageSummary {
    pub template_title: String,
    pub aliases: Vec<String>,
    pub usage_count: usize,
    pub distinct_page_count: usize,
    pub parameter_stats: Vec<TemplateParameterUsage>,
    pub example_pages: Vec<String>,
    pub implementation_titles: Vec<String>,
    pub implementation_preview: Option<String>,
    pub example_invocations: Vec<TemplateInvocationExample>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TemplateImplementationPage {
    pub page_title: String,
    pub namespace: String,
    pub role: String,
    pub summary_text: String,
    pub section_summaries: Vec<LocalSectionSummary>,
    pub context_chunks: Vec<LocalContextChunk>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReferenceUsageExample {
    pub source_title: String,
    pub source_relative_path: String,
    pub section_heading: Option<String>,
    pub reference_name: Option<String>,
    pub reference_group: Option<String>,
    pub citation_family: String,
    pub primary_template_title: Option<String>,
    pub source_type: String,
    pub source_origin: String,
    pub source_family: String,
    pub authority_kind: String,
    pub source_authority: String,
    pub reference_title: String,
    pub source_container: String,
    pub source_author: String,
    pub source_domain: String,
    pub source_date: String,
    pub canonical_url: String,
    pub identifier_keys: Vec<String>,
    pub identifier_entries: Vec<String>,
    pub source_urls: Vec<String>,
    pub retrieval_signals: Vec<String>,
    pub summary_text: String,
    pub template_titles: Vec<String>,
    pub link_titles: Vec<String>,
    pub reference_wikitext: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReferenceUsageSummary {
    pub citation_profile: String,
    pub citation_family: String,
    pub source_type: String,
    pub source_origin: String,
    pub source_family: String,
    pub usage_count: usize,
    pub distinct_page_count: usize,
    pub example_pages: Vec<String>,
    pub common_templates: Vec<String>,
    pub common_links: Vec<String>,
    pub common_domains: Vec<String>,
    pub common_authorities: Vec<String>,
    pub common_identifier_keys: Vec<String>,
    pub common_identifier_entries: Vec<String>,
    pub common_retrieval_signals: Vec<String>,
    pub example_references: Vec<ReferenceUsageExample>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MediaUsageExample {
    pub source_title: String,
    pub source_relative_path: String,
    pub section_heading: Option<String>,
    pub caption_text: String,
    pub options: Vec<String>,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MediaUsageSummary {
    pub file_title: String,
    pub media_kind: String,
    pub usage_count: usize,
    pub distinct_page_count: usize,
    pub example_pages: Vec<String>,
    pub example_usages: Vec<MediaUsageExample>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TemplateReference {
    pub template: TemplateUsageSummary,
    pub implementation_pages: Vec<TemplateImplementationPage>,
    pub implementation_sections: Vec<LocalSectionSummary>,
    pub implementation_chunks: Vec<LocalContextChunk>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthoringDocsContext {
    pub profile: String,
    pub queries: Vec<String>,
    pub pages: Vec<crate::docs::DocsSearchHit>,
    pub sections: Vec<crate::docs::DocsContextSection>,
    pub symbols: Vec<crate::docs::DocsSymbolHit>,
    pub examples: Vec<crate::docs::DocsContextExample>,
    pub token_estimate_total: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActiveTemplateCatalog {
    pub active_template_count: usize,
    pub templates: Vec<TemplateUsageSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum TemplateReferenceLookup {
    IndexMissing,
    TemplateMissing { template_title: String },
    Found(Box<TemplateReference>),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum ActiveTemplateCatalogLookup {
    IndexMissing,
    Found(ActiveTemplateCatalog),
}

pub fn query_active_template_catalog(
    paths: &ResolvedPaths,
    limit: usize,
) -> Result<ActiveTemplateCatalogLookup> {
    templates::query_active_template_catalog(paths, limit)
}

pub fn query_template_reference(
    paths: &ResolvedPaths,
    template_title: &str,
) -> Result<TemplateReferenceLookup> {
    templates::query_template_reference(paths, template_title)
}

pub fn run_validation_checks(paths: &ResolvedPaths) -> Result<Option<ValidationReport>> {
    validation::run_validation_checks(paths)
}

pub fn query_backlinks(paths: &ResolvedPaths, title: &str) -> Result<Option<Vec<String>>> {
    validation::query_backlinks(paths, title)
}

pub fn query_orphans(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    validation::query_orphans(paths)
}

pub fn query_empty_categories(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    validation::query_empty_categories(paths)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StubTemplateHint {
    pub template_title: String,
    pub parameter_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthoringKnowledgePackResult {
    pub topic: String,
    pub query: String,
    pub query_terms: Vec<String>,
    pub inventory: AuthoringInventory,
    pub pack_token_budget: usize,
    pub pack_token_estimate_total: usize,
    pub related_pages: Vec<AuthoringPageCandidate>,
    pub suggested_links: Vec<AuthoringSuggestion>,
    pub suggested_categories: Vec<AuthoringSuggestion>,
    pub suggested_templates: Vec<TemplateUsageSummary>,
    pub suggested_references: Vec<ReferenceUsageSummary>,
    pub suggested_media: Vec<MediaUsageSummary>,
    pub template_baseline: Vec<TemplateUsageSummary>,
    pub template_references: Vec<TemplateReference>,
    pub module_patterns: Vec<ModuleUsageSummary>,
    pub docs_context: Option<AuthoringDocsContext>,
    pub stub_existing_links: Vec<String>,
    pub stub_missing_links: Vec<String>,
    pub stub_detected_templates: Vec<StubTemplateHint>,
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
    pub docs_profile: String,
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
            docs_profile: DEFAULT_DOCS_PROFILE.to_string(),
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
    ingest::rebuild_index(paths, options)
}

pub fn load_stored_index_stats(paths: &ResolvedPaths) -> Result<Option<StoredIndexStats>> {
    ingest::load_stored_index_stats(paths)
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
struct IndexedSectionRecord {
    section_heading: Option<String>,
    section_level: u8,
    summary_text: String,
    section_text: String,
    token_estimate: usize,
}

#[derive(Debug, Clone)]
struct IndexedReferenceRecord {
    section_heading: Option<String>,
    reference_name: Option<String>,
    reference_group: Option<String>,
    citation_profile: String,
    citation_family: String,
    primary_template_title: Option<String>,
    source_type: String,
    source_origin: String,
    source_family: String,
    authority_kind: String,
    source_authority: String,
    reference_title: String,
    source_container: String,
    source_author: String,
    source_domain: String,
    source_date: String,
    canonical_url: String,
    identifier_keys: Vec<String>,
    identifier_entries: Vec<String>,
    source_urls: Vec<String>,
    retrieval_signals: Vec<String>,
    summary_text: String,
    reference_wikitext: String,
    template_titles: Vec<String>,
    link_titles: Vec<String>,
    token_estimate: usize,
}

#[derive(Debug, Clone)]
struct IndexedSemanticProfileRecord {
    source_title: String,
    source_namespace: String,
    summary_text: String,
    section_headings: Vec<String>,
    category_titles: Vec<String>,
    template_titles: Vec<String>,
    template_parameter_keys: Vec<String>,
    link_titles: Vec<String>,
    reference_titles: Vec<String>,
    reference_containers: Vec<String>,
    reference_domains: Vec<String>,
    reference_source_families: Vec<String>,
    reference_authorities: Vec<String>,
    reference_identifiers: Vec<String>,
    media_titles: Vec<String>,
    media_captions: Vec<String>,
    template_implementation_titles: Vec<String>,
    semantic_text: String,
    token_estimate: usize,
}

#[derive(Debug, Clone)]
struct IndexedMediaRecord {
    section_heading: Option<String>,
    file_title: String,
    media_kind: String,
    caption_text: String,
    options: Vec<String>,
    token_estimate: usize,
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
    raw_wikitext: String,
    token_estimate: usize,
}

#[derive(Debug, Clone)]
struct ParsedModuleInvocation {
    module_title: String,
    function_name: String,
    parameter_keys: Vec<String>,
    raw_wikitext: String,
    token_estimate: usize,
}

#[derive(Debug, Clone)]
struct ParsedPageArtifacts {
    section_records: Vec<IndexedSectionRecord>,
    context_chunks: Vec<ArticleContextChunkRow>,
    template_invocations: Vec<ParsedTemplateInvocation>,
    module_invocations: Vec<ParsedModuleInvocation>,
    references: Vec<IndexedReferenceRecord>,
    media: Vec<IndexedMediaRecord>,
}

#[derive(Debug, Clone, Default)]
struct TemplateImplementationSeed {
    template_dependencies: Vec<String>,
    module_dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedIdentifierEntry {
    key: String,
    value: String,
    normalized_value: String,
}

#[derive(Debug, Clone, Copy)]
struct ChunkRetrievalPlan {
    limit: usize,
    token_budget: usize,
    max_pages: usize,
    diversify: bool,
}

#[derive(Debug, Clone, Default)]
struct ChunkRerankSignals {
    related_page_weights: BTreeMap<String, usize>,
    template_page_weights: BTreeMap<String, usize>,
    semantic_page_weights: BTreeMap<String, usize>,
    authority_page_weights: BTreeMap<String, usize>,
    identifier_page_weights: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticPageHit {
    title: String,
    retrieval_weight: usize,
}

#[derive(Debug, Clone)]
struct ArticleContextChunkRow {
    section_heading: Option<String>,
    chunk_text: String,
    token_estimate: usize,
}

#[derive(Debug, Clone)]
struct ParsedContentSection {
    section_heading: Option<String>,
    section_level: u8,
    section_text: String,
}

#[derive(Debug, Clone, Default)]
struct ReferenceTemplateDetails {
    template_title: String,
    named_params: BTreeMap<String, String>,
    positional_params: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct ReferenceAnalysis {
    citation_profile: String,
    citation_family: String,
    primary_template_title: Option<String>,
    source_type: String,
    source_origin: String,
    source_family: String,
    authority_kind: String,
    source_authority: String,
    reference_title: String,
    source_container: String,
    source_author: String,
    source_domain: String,
    source_date: String,
    canonical_url: String,
    identifier_keys: Vec<String>,
    identifier_entries: Vec<String>,
    source_urls: Vec<String>,
    retrieval_signals: Vec<String>,
    summary_hint: String,
}

fn query_page_records_from_sections_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_page_sections")? {
        return Ok(Vec::new());
    }

    let limit_i64 = i64::try_from(limit).context("section query limit does not fit into i64")?;
    if fts_table_exists(connection, "indexed_page_sections_fts") {
        let fts_query = format!("\"{}\" *", normalize_spaces(&query.replace('_', " ")));
        let mut statement = connection
            .prepare(
                "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
                 FROM indexed_page_sections_fts fts
                 JOIN indexed_page_sections s ON s.rowid = fts.rowid
                 JOIN indexed_pages p ON p.relative_path = s.source_relative_path
                 WHERE indexed_page_sections_fts MATCH ?1
                 GROUP BY p.relative_path
                 ORDER BY COUNT(*) DESC, p.title ASC
                 LIMIT ?2",
            )
            .context("failed to prepare section FTS query")?;
        let rows = statement
            .query_map(params![fts_query, limit_i64], decode_page_record_row)
            .context("failed to run section FTS query")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("failed to decode section FTS page row")?);
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    let wildcard = format!("%{query}%");
    let prefix = format!("{query}%");
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_page_sections s
             JOIN indexed_pages p ON p.relative_path = s.source_relative_path
             WHERE lower(s.section_text) LIKE lower(?1)
                OR lower(s.summary_text) LIKE lower(?1)
                OR lower(COALESCE(s.section_heading, '')) LIKE lower(?1)
             GROUP BY p.relative_path
             ORDER BY
               CASE
                 WHEN lower(p.title) = lower(?2) THEN 0
                 WHEN lower(p.title) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               COUNT(*) DESC,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare section LIKE query")?;
    let rows = statement
        .query_map(
            params![wildcard, query, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run section LIKE query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode section LIKE page row")?);
    }
    Ok(out)
}

fn query_page_records_from_semantics_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_page_semantics")? {
        return Ok(Vec::new());
    }

    let limit_i64 = i64::try_from(limit).context("semantic query limit does not fit into i64")?;
    if fts_table_exists(connection, "indexed_page_semantics_fts") {
        let fts_query = format!("\"{}\" *", normalize_spaces(&query.replace('_', " ")));
        let mut statement = connection
            .prepare(
                "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
                 FROM indexed_page_semantics_fts fts
                 JOIN indexed_page_semantics s ON s.rowid = fts.rowid
                 JOIN indexed_pages p ON p.relative_path = s.source_relative_path
                 WHERE indexed_page_semantics_fts MATCH ?1
                 ORDER BY bm25(indexed_page_semantics_fts) ASC, p.title ASC
                 LIMIT ?2",
            )
            .context("failed to prepare semantic FTS query")?;
        let rows = statement
            .query_map(params![fts_query, limit_i64], decode_page_record_row)
            .context("failed to run semantic FTS query")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("failed to decode semantic FTS row")?);
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    let wildcard = format!("%{query}%");
    let prefix = format!("{query}%");
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_page_semantics s
             JOIN indexed_pages p ON p.relative_path = s.source_relative_path
             WHERE lower(s.semantic_text) LIKE lower(?1)
                OR lower(s.summary_text) LIKE lower(?1)
                OR lower(s.source_title) LIKE lower(?1)
             ORDER BY
               CASE
                 WHEN lower(s.source_title) = lower(?2) THEN 0
                 WHEN lower(s.source_title) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare semantic LIKE query")?;
    let rows = statement
        .query_map(
            params![wildcard, query, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run semantic LIKE query")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode semantic LIKE row")?);
    }
    Ok(out)
}

fn load_semantic_page_hits(
    connection: &Connection,
    query_terms: &[String],
    limit: usize,
) -> Result<Vec<SemanticPageHit>> {
    if limit == 0 || query_terms.is_empty() {
        return Ok(Vec::new());
    }

    let search_limit = candidate_limit(limit.max(1), 2);
    let mut weights = BTreeMap::<String, usize>::new();
    let mut titles = BTreeMap::<String, String>::new();
    for (query_index, term) in query_terms.iter().enumerate() {
        let base_weight = 220usize
            .saturating_sub(query_index.saturating_mul(18))
            .max(40);
        for (rank, page) in
            query_page_records_from_semantics_for_connection(connection, term, search_limit)?
                .into_iter()
                .enumerate()
        {
            let key = page.title.to_ascii_lowercase();
            titles.entry(key.clone()).or_insert(page.title);
            let weight = base_weight.saturating_sub(rank.saturating_mul(14)).max(20);
            let entry = weights.entry(key).or_insert(0);
            *entry = entry.saturating_add(weight);
        }
    }

    materialize_page_hits(weights, titles, limit)
}

fn materialize_page_hits(
    weights: BTreeMap<String, usize>,
    titles: BTreeMap<String, String>,
    limit: usize,
) -> Result<Vec<SemanticPageHit>> {
    let mut hits = weights
        .into_iter()
        .filter_map(|(key, retrieval_weight)| {
            titles.get(&key).map(|title| SemanticPageHit {
                title: title.clone(),
                retrieval_weight,
            })
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.title.cmp(&right.title))
    });
    hits.truncate(limit);
    Ok(hits)
}

fn build_semantic_page_weight_map(semantic_hits: &[SemanticPageHit]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for hit in semantic_hits {
        out.insert(hit.title.to_ascii_lowercase(), hit.retrieval_weight);
    }
    out
}

fn build_authority_page_weight_map(hits: &[SemanticPageHit]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for hit in hits {
        out.insert(hit.title.to_ascii_lowercase(), hit.retrieval_weight);
    }
    out
}

fn build_identifier_page_weight_map(hits: &[SemanticPageHit]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for hit in hits {
        out.insert(hit.title.to_ascii_lowercase(), hit.retrieval_weight);
    }
    out
}

fn query_page_records_from_aliases_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_page_aliases")? {
        return Ok(Vec::new());
    }

    let wildcard = format!("%{query}%");
    let prefix = format!("{query}%");
    let limit_i64 = i64::try_from(limit).context("alias query limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_page_aliases a
             JOIN indexed_pages p ON p.relative_path = a.source_relative_path
             WHERE lower(a.alias_title) LIKE lower(?1)
             GROUP BY p.relative_path
             ORDER BY
               CASE
                 WHEN lower(a.alias_title) = lower(?2) THEN 0
                 WHEN lower(a.alias_title) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare alias page query")?;
    let rows = statement
        .query_map(
            params![wildcard, query, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run alias page query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode alias page row")?);
    }
    Ok(out)
}

fn decode_page_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedPageRecord> {
    let bytes_i64: i64 = row.get(5)?;
    Ok(IndexedPageRecord {
        title: row.get(0)?,
        namespace: row.get(1)?,
        is_redirect: row.get::<_, i64>(2)? == 1,
        redirect_target: row.get(3)?,
        relative_path: row.get(4)?,
        bytes: u64::try_from(bytes_i64).unwrap_or(0),
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

fn load_indexed_reference_rows_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    limit: usize,
) -> Result<Vec<LocalReferenceUsage>> {
    let limit_i64 = i64::try_from(limit).context("reference limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT section_heading, reference_name, reference_group, citation_profile, citation_family,
                    primary_template_title, source_type, source_origin, source_family, authority_kind,
                    source_authority, reference_title, source_container, source_author, source_domain,
                    source_date, canonical_url, identifier_keys, identifier_entries, source_urls,
                    retrieval_signals, summary_text, template_titles, link_titles, token_estimate
             FROM indexed_page_references
             WHERE source_relative_path = ?1
             ORDER BY reference_index ASC
             LIMIT ?2",
        )
        .context("failed to prepare indexed_page_references query")?;
    let rows = statement
        .query_map(params![source_relative_path, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(24)?;
            Ok(LocalReferenceUsage {
                section_heading: row.get(0)?,
                reference_name: row.get(1)?,
                reference_group: row.get(2)?,
                citation_profile: row.get(3)?,
                citation_family: row.get(4)?,
                primary_template_title: normalize_non_empty_string(row.get::<_, String>(5)?),
                source_type: row.get(6)?,
                source_origin: row.get(7)?,
                source_family: row.get(8)?,
                authority_kind: row.get(9)?,
                source_authority: row.get(10)?,
                reference_title: row.get(11)?,
                source_container: row.get(12)?,
                source_author: row.get(13)?,
                source_domain: row.get(14)?,
                source_date: row.get(15)?,
                canonical_url: row.get(16)?,
                identifier_keys: parse_string_list(&row.get::<_, String>(17)?),
                identifier_entries: parse_string_list(&row.get::<_, String>(18)?),
                source_urls: parse_string_list(&row.get::<_, String>(19)?),
                retrieval_signals: parse_string_list(&row.get::<_, String>(20)?),
                summary_text: row.get(21)?,
                template_titles: parse_string_list(&row.get::<_, String>(22)?),
                link_titles: parse_string_list(&row.get::<_, String>(23)?),
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run indexed_page_references query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode indexed_page_references row")?);
    }
    Ok(out)
}

fn load_indexed_media_rows_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    limit: usize,
) -> Result<Vec<LocalMediaUsage>> {
    let limit_i64 = i64::try_from(limit).context("media limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT section_heading, file_title, media_kind, caption_text, options_text, token_estimate
             FROM indexed_page_media
             WHERE source_relative_path = ?1
             ORDER BY media_index ASC
             LIMIT ?2",
        )
        .context("failed to prepare indexed_page_media query")?;
    let rows = statement
        .query_map(params![source_relative_path, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(5)?;
            Ok(LocalMediaUsage {
                section_heading: row.get(0)?,
                file_title: row.get(1)?,
                media_kind: row.get(2)?,
                caption_text: row.get(3)?,
                options: parse_string_list(&row.get::<_, String>(4)?),
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run indexed_page_media query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode indexed_page_media row")?);
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

fn serialize_string_list(values: &[String]) -> String {
    let normalized = values
        .iter()
        .map(|value| normalize_spaces(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        return NO_STRING_LIST_SENTINEL.to_string();
    }
    normalized.join("\n")
}

fn parse_string_list(value: &str) -> Vec<String> {
    if value.trim().is_empty() || value == NO_STRING_LIST_SENTINEL {
        return Vec::new();
    }
    value
        .lines()
        .map(normalize_spaces)
        .filter(|item| !item.is_empty())
        .collect()
}

fn normalize_non_empty_string(value: String) -> Option<String> {
    let normalized = normalize_spaces(&value);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
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

fn extract_page_artifacts(content: &str) -> ParsedPageArtifacts {
    let sections = parse_content_sections(content);
    ParsedPageArtifacts {
        section_records: extract_section_records_from_sections(&sections),
        context_chunks: chunk_article_context_from_sections(&sections),
        template_invocations: extract_template_invocations(content),
        module_invocations: extract_module_invocations(content),
        references: extract_reference_records_from_sections(&sections),
        media: extract_media_records_from_sections(&sections),
    }
}

fn build_page_semantic_profile(
    file: &ScannedFile,
    links: &[ParsedLink],
    artifacts: &ParsedPageArtifacts,
) -> IndexedSemanticProfileRecord {
    let summary_text = artifacts
        .section_records
        .iter()
        .find(|section| section.section_heading.is_none())
        .map(|section| section.summary_text.clone())
        .filter(|summary| !summary.is_empty())
        .or_else(|| {
            artifacts
                .context_chunks
                .first()
                .map(|chunk| summarize_words(&chunk.chunk_text, AUTHORING_PAGE_SUMMARY_WORD_LIMIT))
                .filter(|summary| !summary.is_empty())
        })
        .unwrap_or_default();
    let section_headings = collect_normalized_string_list(
        artifacts
            .section_records
            .iter()
            .filter_map(|section| section.section_heading.clone()),
    );
    let category_titles = collect_normalized_string_list(
        links
            .iter()
            .filter(|link| link.is_category_membership)
            .map(|link| link.target_title.clone()),
    );
    let link_titles = collect_normalized_string_list(
        links
            .iter()
            .filter(|link| !link.is_category_membership)
            .map(|link| link.target_title.clone()),
    );
    let template_titles = collect_normalized_string_list(
        artifacts
            .template_invocations
            .iter()
            .map(|invocation| invocation.template_title.clone()),
    );
    let template_parameter_keys = collect_normalized_string_list(
        artifacts
            .template_invocations
            .iter()
            .flat_map(|invocation| invocation.parameter_keys.iter().cloned()),
    );
    let reference_titles = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.reference_title.clone()),
    );
    let reference_containers = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.source_container.clone()),
    );
    let reference_domains = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.source_domain.clone()),
    );
    let reference_source_families = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.source_family.clone()),
    );
    let reference_authorities = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.source_authority.clone()),
    );
    let reference_identifiers = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .flat_map(|reference| reference.identifier_entries.iter().cloned()),
    );
    let media_titles = collect_normalized_string_list(
        artifacts.media.iter().map(|media| media.file_title.clone()),
    );
    let media_captions = collect_normalized_string_list(
        artifacts
            .media
            .iter()
            .map(|media| media.caption_text.clone()),
    );
    let template_implementation_titles = collect_template_implementation_terms(file, artifacts);
    let module_terms = collect_normalized_string_list(
        artifacts.module_invocations.iter().flat_map(|invocation| {
            [
                invocation.module_title.clone(),
                invocation.function_name.clone(),
            ]
        }),
    );
    let mut profile = IndexedSemanticProfileRecord {
        source_title: file.title.clone(),
        source_namespace: file.namespace.clone(),
        summary_text,
        section_headings,
        category_titles,
        template_titles,
        template_parameter_keys,
        link_titles,
        reference_titles,
        reference_containers,
        reference_domains,
        reference_source_families,
        reference_authorities,
        reference_identifiers,
        media_titles,
        media_captions,
        template_implementation_titles,
        semantic_text: String::new(),
        token_estimate: 0,
    };
    profile.semantic_text = build_page_semantic_text(file, &profile, &module_terms);
    profile.token_estimate = estimate_tokens(&profile.semantic_text);
    profile
}

fn build_page_semantic_text(
    file: &ScannedFile,
    profile: &IndexedSemanticProfileRecord,
    module_terms: &[String],
) -> String {
    let mut terms = Vec::new();
    let mut seen = BTreeSet::new();

    push_semantic_term(&mut terms, &mut seen, &file.title);
    push_semantic_term(&mut terms, &mut seen, &profile.summary_text);
    for values in [
        &profile.section_headings,
        &profile.category_titles,
        &profile.template_titles,
        &profile.template_parameter_keys,
        &profile.link_titles,
        &profile.reference_titles,
        &profile.reference_containers,
        &profile.reference_domains,
        &profile.reference_source_families,
        &profile.reference_authorities,
        &profile.reference_identifiers,
        &profile.media_titles,
        &profile.media_captions,
        &profile.template_implementation_titles,
        module_terms,
    ] {
        for value in values {
            push_semantic_term(&mut terms, &mut seen, value);
        }
    }

    if terms.is_empty() {
        push_semantic_term(&mut terms, &mut seen, &file.title);
    }
    terms.join("\n")
}

fn collect_normalized_string_list<I>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    values
        .into_iter()
        .map(|value| normalize_spaces(&value.replace('_', " ")))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_template_implementation_terms(
    file: &ScannedFile,
    artifacts: &ParsedPageArtifacts,
) -> Vec<String> {
    if file.namespace != Namespace::Template.as_str() {
        return Vec::new();
    }

    let mut terms = Vec::new();
    terms.extend(artifacts.module_invocations.iter().flat_map(|invocation| {
        [
            invocation.module_title.clone(),
            invocation.function_name.clone(),
        ]
    }));
    terms.extend(
        artifacts
            .template_invocations
            .iter()
            .map(|invocation| invocation.template_title.clone())
            .filter(|title| !title.eq_ignore_ascii_case(&file.title)),
    );
    if file.title.ends_with("/doc") {
        if let Some(base_title) = file.title.strip_suffix("/doc") {
            terms.push(base_title.to_string());
        }
    } else {
        terms.push(format!("{}/doc", file.title));
    }
    collect_normalized_string_list(terms)
}

fn maybe_record_template_implementation_seed(
    seeds: &mut BTreeMap<String, TemplateImplementationSeed>,
    file: &ScannedFile,
    artifacts: &ParsedPageArtifacts,
) {
    if file.namespace != Namespace::Template.as_str() {
        return;
    }

    let mut seed = TemplateImplementationSeed {
        template_dependencies: artifacts
            .template_invocations
            .iter()
            .map(|invocation| invocation.template_title.clone())
            .filter(|title| !title.eq_ignore_ascii_case(&file.title))
            .collect(),
        module_dependencies: artifacts
            .module_invocations
            .iter()
            .map(|invocation| invocation.module_title.clone())
            .collect(),
    };
    seed.template_dependencies.sort();
    seed.template_dependencies.dedup();
    seed.module_dependencies.sort();
    seed.module_dependencies.dedup();
    seeds.insert(file.title.to_ascii_lowercase(), seed);
}

fn persist_template_implementation_pages(
    statement: &mut rusqlite::Statement<'_>,
    files: &[ScannedFile],
    seeds: &BTreeMap<String, TemplateImplementationSeed>,
) -> Result<()> {
    let page_map = files
        .iter()
        .map(|file| (file.title.to_ascii_lowercase(), file))
        .collect::<BTreeMap<_, _>>();

    let mut active_templates = BTreeSet::new();
    for file in files {
        if file.namespace == Namespace::Template.as_str() && !file.title.ends_with("/doc") {
            active_templates.insert(file.title.clone());
        }
    }
    for seed in seeds.values() {
        for dependency in &seed.template_dependencies {
            active_templates.insert(dependency.clone());
        }
    }

    for template_title in active_templates {
        let normalized_key = template_title.to_ascii_lowercase();
        if let Some(file) = page_map.get(&normalized_key) {
            statement
                .execute(params![
                    template_title.as_str(),
                    file.title.as_str(),
                    file.namespace.as_str(),
                    file.relative_path.as_str(),
                    "template",
                ])
                .with_context(|| {
                    format!(
                        "failed to insert template implementation page for {}",
                        template_title
                    )
                })?;
        }

        let doc_title = format!("{template_title}/doc");
        if let Some(file) = page_map.get(&doc_title.to_ascii_lowercase()) {
            statement
                .execute(params![
                    template_title.as_str(),
                    file.title.as_str(),
                    file.namespace.as_str(),
                    file.relative_path.as_str(),
                    "documentation",
                ])
                .with_context(|| {
                    format!(
                        "failed to insert template documentation page for {}",
                        template_title
                    )
                })?;
        }

        for seed_key in [normalized_key.clone(), doc_title.to_ascii_lowercase()] {
            let Some(seed) = seeds.get(&seed_key) else {
                continue;
            };

            for module_title in &seed.module_dependencies {
                if let Some(file) = page_map.get(&module_title.to_ascii_lowercase()) {
                    statement
                        .execute(params![
                            template_title.as_str(),
                            file.title.as_str(),
                            file.namespace.as_str(),
                            file.relative_path.as_str(),
                            "module",
                        ])
                        .with_context(|| {
                            format!(
                                "failed to insert template module implementation for {}",
                                template_title
                            )
                        })?;
                }
            }

            for dependency_title in &seed.template_dependencies {
                if let Some(file) = page_map.get(&dependency_title.to_ascii_lowercase()) {
                    statement
                        .execute(params![
                            template_title.as_str(),
                            file.title.as_str(),
                            file.namespace.as_str(),
                            file.relative_path.as_str(),
                            "dependency",
                        ])
                        .with_context(|| {
                            format!(
                                "failed to insert template dependency implementation for {}",
                                template_title
                            )
                        })?;
                }
            }
        }
    }

    Ok(())
}

fn push_semantic_term(out: &mut Vec<String>, seen: &mut BTreeSet<String>, value: &str) {
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return;
    }
    let key = normalized.to_ascii_lowercase();
    if seen.insert(key) {
        out.push(normalized.clone());
    }
    if let Some((_, body)) = normalized.split_once(':') {
        let body = normalize_spaces(body);
        if body.is_empty() {
            return;
        }
        let body_key = body.to_ascii_lowercase();
        if seen.insert(body_key) {
            out.push(body);
        }
    }
}

fn chunk_article_context(content: &str) -> Vec<ArticleContextChunkRow> {
    let sections = parse_content_sections(content);
    chunk_article_context_from_sections(&sections)
}

fn extract_section_records(content: &str) -> Vec<IndexedSectionRecord> {
    let sections = parse_content_sections(content);
    extract_section_records_from_sections(&sections)
}

fn extract_section_records_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<IndexedSectionRecord> {
    sections
        .iter()
        .map(|section| IndexedSectionRecord {
            section_heading: section.section_heading.clone(),
            section_level: section.section_level,
            summary_text: summarize_words(
                &normalize_multiline_spaces(&section.section_text),
                AUTHORING_PAGE_SUMMARY_WORD_LIMIT,
            ),
            token_estimate: estimate_tokens(&section.section_text),
            section_text: normalize_multiline_spaces(&section.section_text),
        })
        .collect()
}

fn chunk_article_context_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<ArticleContextChunkRow> {
    let mut out = Vec::new();
    for section in sections {
        for chunk_text in chunk_section_text(&section.section_text) {
            out.push(ArticleContextChunkRow {
                section_heading: section.section_heading.clone(),
                token_estimate: estimate_tokens(&chunk_text),
                chunk_text,
            });
        }
    }
    out
}

fn parse_content_sections(content: &str) -> Vec<ParsedContentSection> {
    let mut out = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_level = 1u8;
    let mut current_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((level, heading)) = parse_heading_line(trimmed) {
            flush_content_section(
                &mut out,
                current_heading.take(),
                current_level,
                &current_lines,
            );
            current_lines.clear();
            current_heading = Some(heading);
            current_level = level;
            continue;
        }
        current_lines.push(line);
    }
    flush_content_section(&mut out, current_heading, current_level, &current_lines);
    out
}

fn flush_content_section(
    out: &mut Vec<ParsedContentSection>,
    section_heading: Option<String>,
    section_level: u8,
    lines: &[&str],
) {
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() {
        return;
    }
    out.push(ParsedContentSection {
        section_heading,
        section_level,
        section_text: text,
    });
}

fn chunk_section_text(section_text: &str) -> Vec<String> {
    let paragraphs = section_text
        .split("\n\n")
        .map(normalize_multiline_spaces)
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>();
    if paragraphs.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current_parts = Vec::<String>::new();
    let mut current_words = 0usize;
    for paragraph in paragraphs {
        let paragraph_words = count_words(&paragraph);
        if paragraph_words > INDEX_CHUNK_WORD_TARGET {
            if !current_parts.is_empty() {
                out.push(current_parts.join(" "));
                current_parts.clear();
                current_words = 0;
            }
            out.extend(split_text_by_words(&paragraph, INDEX_CHUNK_WORD_TARGET));
            continue;
        }
        if !current_parts.is_empty()
            && current_words.saturating_add(paragraph_words) > INDEX_CHUNK_WORD_TARGET
        {
            out.push(current_parts.join(" "));
            current_parts.clear();
            current_words = 0;
        }
        current_words = current_words.saturating_add(paragraph_words);
        current_parts.push(paragraph);
    }
    if !current_parts.is_empty() {
        out.push(current_parts.join(" "));
    }
    out
}

fn extract_reference_records_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<IndexedReferenceRecord> {
    let mut out = Vec::new();
    for section in sections {
        out.extend(extract_reference_records_for_section(
            section.section_heading.clone(),
            &section.section_text,
        ));
    }
    out
}

fn extract_reference_records(content: &str) -> Vec<LocalReferenceUsage> {
    extract_reference_records_from_sections(&parse_content_sections(content))
        .into_iter()
        .map(|record| LocalReferenceUsage {
            section_heading: record.section_heading,
            reference_name: record.reference_name,
            reference_group: record.reference_group,
            citation_profile: record.citation_profile,
            citation_family: record.citation_family,
            primary_template_title: record.primary_template_title,
            source_type: record.source_type,
            source_origin: record.source_origin,
            source_family: record.source_family,
            authority_kind: record.authority_kind,
            source_authority: record.source_authority,
            reference_title: record.reference_title,
            source_container: record.source_container,
            source_author: record.source_author,
            source_domain: record.source_domain,
            source_date: record.source_date,
            canonical_url: record.canonical_url,
            identifier_keys: record.identifier_keys,
            identifier_entries: record.identifier_entries,
            source_urls: record.source_urls,
            retrieval_signals: record.retrieval_signals,
            summary_text: record.summary_text,
            template_titles: record.template_titles,
            link_titles: record.link_titles,
            token_estimate: record.token_estimate,
        })
        .take(CONTEXT_REFERENCE_LIMIT)
        .collect()
}

fn extract_reference_records_for_section(
    section_heading: Option<String>,
    content: &str,
) -> Vec<IndexedReferenceRecord> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if !starts_with_html_tag(bytes, cursor, "ref") {
            cursor += 1;
            continue;
        }
        let Some((tag_end, tag_body, self_closing)) = parse_open_tag(content, cursor, "ref") else {
            cursor += 1;
            continue;
        };
        let attributes = parse_html_attributes(&tag_body);
        let reference_name = attributes
            .get("name")
            .map(|value| normalize_spaces(value))
            .filter(|value| !value.is_empty());
        let reference_group = attributes
            .get("group")
            .map(|value| normalize_spaces(value))
            .filter(|value| !value.is_empty());

        let (reference_wikitext, reference_body, next_cursor) = if self_closing {
            (content[cursor..tag_end].to_string(), String::new(), tag_end)
        } else if let Some((close_start, close_end)) =
            find_closing_html_tag(content, tag_end, "ref")
        {
            (
                content[cursor..close_end].to_string(),
                content[tag_end..close_start].to_string(),
                close_end,
            )
        } else {
            (content[cursor..tag_end].to_string(), String::new(), tag_end)
        };

        let template_titles = extract_template_titles(&reference_body);
        let link_titles = extract_link_titles(&reference_body);
        let analysis = analyze_reference_body(
            &reference_body,
            &template_titles,
            &link_titles,
            reference_name.as_deref(),
            reference_group.as_deref(),
        );
        let mut summary_text = flatten_markup_excerpt(&reference_body);
        if summary_text.is_empty() {
            summary_text = analysis.summary_hint.clone();
        }
        if summary_text.is_empty() && !template_titles.is_empty() {
            summary_text = template_titles.join(", ");
        }
        if summary_text.is_empty()
            && let Some(name) = &reference_name
        {
            summary_text = format!("Named reference {name}");
        }
        if summary_text.is_empty() {
            summary_text = "<ref>".to_string();
        }

        let token_estimate = estimate_tokens(&reference_wikitext);
        out.push(IndexedReferenceRecord {
            section_heading: section_heading.clone(),
            reference_name,
            reference_group,
            citation_profile: analysis.citation_profile,
            citation_family: analysis.citation_family,
            primary_template_title: analysis.primary_template_title,
            source_type: analysis.source_type,
            source_origin: analysis.source_origin,
            source_family: analysis.source_family,
            authority_kind: analysis.authority_kind,
            source_authority: analysis.source_authority,
            reference_title: analysis.reference_title,
            source_container: analysis.source_container,
            source_author: analysis.source_author,
            source_domain: analysis.source_domain,
            source_date: analysis.source_date,
            canonical_url: analysis.canonical_url,
            identifier_keys: analysis.identifier_keys,
            identifier_entries: analysis.identifier_entries,
            source_urls: analysis.source_urls,
            retrieval_signals: analysis.retrieval_signals,
            summary_text: summarize_words(&summary_text, AUTHORING_PAGE_SUMMARY_WORD_LIMIT),
            reference_wikitext,
            template_titles,
            link_titles,
            token_estimate,
        });
        cursor = next_cursor.max(cursor.saturating_add(1));
    }

    out
}

fn extract_media_records_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<IndexedMediaRecord> {
    let mut out = Vec::new();
    for section in sections {
        out.extend(extract_media_records_for_section(
            section.section_heading.clone(),
            &section.section_text,
        ));
    }
    out
}

fn extract_media_records(content: &str) -> Vec<LocalMediaUsage> {
    extract_media_records_from_sections(&parse_content_sections(content))
        .into_iter()
        .map(|record| LocalMediaUsage {
            section_heading: record.section_heading,
            file_title: record.file_title,
            media_kind: record.media_kind,
            caption_text: record.caption_text,
            options: record.options,
            token_estimate: record.token_estimate,
        })
        .take(CONTEXT_MEDIA_LIMIT)
        .collect()
}

fn extract_media_records_for_section(
    section_heading: Option<String>,
    content: &str,
) -> Vec<IndexedMediaRecord> {
    let mut out = extract_inline_media_records(section_heading.clone(), content);
    out.extend(extract_gallery_media_records(section_heading, content));
    out
}

fn extract_inline_media_records(
    section_heading: Option<String>,
    content: &str,
) -> Vec<IndexedMediaRecord> {
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
            if let Some(record) = parse_inline_media_record(section_heading.clone(), inner) {
                out.push(record);
            }
            cursor = end + 2;
            continue;
        }
        cursor += 1;
    }

    out
}

fn extract_gallery_media_records(
    section_heading: Option<String>,
    content: &str,
) -> Vec<IndexedMediaRecord> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if !starts_with_html_tag(bytes, cursor, "gallery") {
            cursor += 1;
            continue;
        }
        let Some((tag_end, tag_body, self_closing)) = parse_open_tag(content, cursor, "gallery")
        else {
            cursor += 1;
            continue;
        };
        if self_closing {
            cursor = tag_end;
            continue;
        }
        let Some((close_start, close_end)) = find_closing_html_tag(content, tag_end, "gallery")
        else {
            cursor = tag_end;
            continue;
        };
        let gallery_options = parse_html_attributes(&tag_body)
            .into_iter()
            .map(|(key, value)| {
                if value.is_empty() {
                    key
                } else {
                    format!("{key}={value}")
                }
            })
            .collect::<Vec<_>>();
        let body = &content[tag_end..close_start];
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(record) =
                parse_gallery_media_line(section_heading.clone(), trimmed, &gallery_options)
            {
                out.push(record);
            }
        }
        cursor = close_end;
    }

    out
}

fn parse_inline_media_record(
    section_heading: Option<String>,
    inner: &str,
) -> Option<IndexedMediaRecord> {
    let trimmed = inner.trim();
    if trimmed.starts_with(':') {
        return None;
    }
    let segments = split_template_segments(trimmed);
    let target = segments.first()?.trim();
    let (file_title, namespace) =
        normalize_title_and_namespace(&normalize_spaces(&target.replace('_', " ")))?;
    if namespace != Namespace::File.as_str() {
        return None;
    }

    let options = segments
        .iter()
        .skip(1)
        .map(|segment| normalize_spaces(segment))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let caption_text = options
        .iter()
        .rev()
        .find(|segment| !is_media_option(segment))
        .map(|segment| flatten_markup_excerpt(segment))
        .unwrap_or_default();

    Some(IndexedMediaRecord {
        section_heading,
        file_title,
        media_kind: "inline".to_string(),
        caption_text: summarize_words(&caption_text, AUTHORING_PAGE_SUMMARY_WORD_LIMIT),
        options,
        token_estimate: estimate_tokens(trimmed),
    })
}

fn parse_gallery_media_line(
    section_heading: Option<String>,
    line: &str,
    gallery_options: &[String],
) -> Option<IndexedMediaRecord> {
    let segments = split_template_segments(line);
    let target = segments.first()?.trim();
    let (file_title, namespace) =
        normalize_title_and_namespace(&normalize_spaces(&target.replace('_', " ")))?;
    if namespace != Namespace::File.as_str() {
        return None;
    }

    let line_options = segments
        .iter()
        .skip(1)
        .map(|segment| normalize_spaces(segment))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let caption_text = line_options
        .iter()
        .rev()
        .find(|segment| !is_media_option(segment))
        .map(|segment| flatten_markup_excerpt(segment))
        .unwrap_or_default();
    let mut options = gallery_options.to_vec();
    options.extend(line_options);

    Some(IndexedMediaRecord {
        section_heading,
        file_title,
        media_kind: "gallery".to_string(),
        caption_text: summarize_words(&caption_text, AUTHORING_PAGE_SUMMARY_WORD_LIMIT),
        options,
        token_estimate: estimate_tokens(line),
    })
}

fn split_text_by_words(text: &str, word_target: usize) -> Vec<String> {
    let words = text.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < words.len() {
        let end = (cursor + word_target.max(1)).min(words.len());
        let chunk_text = words[cursor..end].join(" ");
        if !chunk_text.is_empty() {
            out.push(chunk_text);
        }
        cursor = end;
    }
    out
}

fn parse_heading_line(value: &str) -> Option<(u8, String)> {
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
    Some((u8::try_from(leading).unwrap_or(6), heading.to_string()))
}

fn summarize_words(value: &str, max_words: usize) -> String {
    normalize_spaces(value)
        .split_whitespace()
        .take(max_words.max(1))
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_multiline_spaces(value: &str) -> String {
    value
        .lines()
        .map(normalize_spaces)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
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

fn extract_module_invocations(content: &str) -> Vec<ParsedModuleInvocation> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
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
                if let Some(invocation) = parse_module_invocation(inner) {
                    let signature = format!(
                        "{}|{}|{}",
                        invocation.module_title.to_ascii_lowercase(),
                        invocation.function_name.to_ascii_lowercase(),
                        canonical_parameter_key_list(&invocation.parameter_keys)
                    );
                    if seen.insert(signature) {
                        out.push(invocation);
                    }
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
        raw_wikitext: format!("{{{{{inner}}}}}"),
        token_estimate: estimate_tokens(inner),
    })
}

fn parse_module_invocation(inner: &str) -> Option<ParsedModuleInvocation> {
    let segments = split_template_segments(inner);
    let raw_name = segments.first()?.trim();
    let remainder = raw_name.strip_prefix("#invoke:")?;
    let module_name = normalize_spaces(remainder);
    if module_name.is_empty() {
        return None;
    }
    let function_name = normalize_spaces(segments.get(1).map(String::as_str).unwrap_or(""));
    if function_name.is_empty() {
        return None;
    }

    let mut parameter_keys = Vec::new();
    let mut positional_index = 1usize;
    for segment in segments.iter().skip(2) {
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

    Some(ParsedModuleInvocation {
        module_title: format!("Module:{module_name}"),
        function_name,
        parameter_keys,
        raw_wikitext: format!("{{{{{inner}}}}}"),
        token_estimate: estimate_tokens(inner),
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

fn starts_with_html_tag(bytes: &[u8], cursor: usize, tag_name: &str) -> bool {
    let tag_bytes = tag_name.as_bytes();
    if cursor + tag_bytes.len() + 1 >= bytes.len() || bytes[cursor] != b'<' {
        return false;
    }
    let start = cursor + 1;
    let end = start + tag_bytes.len();
    if end > bytes.len() || !bytes[start..end].eq_ignore_ascii_case(tag_bytes) {
        return false;
    }
    matches!(
        bytes.get(end).copied(),
        Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
    )
}

fn parse_open_tag(content: &str, start: usize, tag_name: &str) -> Option<(usize, String, bool)> {
    let bytes = content.as_bytes();
    if !starts_with_html_tag(bytes, start, tag_name) {
        return None;
    }

    let mut cursor = start + tag_name.len() + 1;
    let mut quote = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            cursor += 1;
            continue;
        }
        if byte == b'>' {
            let raw_body = &content[start + tag_name.len() + 1..cursor];
            let trimmed = raw_body.trim();
            let self_closing = trimmed.ends_with('/');
            let body = if self_closing {
                trimmed.trim_end_matches('/').trim_end().to_string()
            } else {
                trimmed.to_string()
            };
            return Some((cursor + 1, body, self_closing));
        }
        cursor += 1;
    }
    None
}

fn find_closing_html_tag(content: &str, start: usize, tag_name: &str) -> Option<(usize, usize)> {
    let bytes = content.as_bytes();
    let needle = format!("</{tag_name}");
    let needle_bytes = needle.as_bytes();
    let mut cursor = start;

    while cursor + needle_bytes.len() < bytes.len() {
        if bytes[cursor] == b'<'
            && bytes[cursor..cursor + needle_bytes.len()].eq_ignore_ascii_case(needle_bytes)
        {
            let boundary = bytes.get(cursor + needle_bytes.len()).copied();
            if !matches!(
                boundary,
                Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
            ) {
                cursor += 1;
                continue;
            }
            let mut end = cursor + needle_bytes.len();
            while end < bytes.len() && bytes[end] != b'>' {
                end += 1;
            }
            if end < bytes.len() {
                return Some((cursor, end + 1));
            }
        }
        cursor += 1;
    }
    None
}

fn parse_html_attributes(value: &str) -> BTreeMap<String, String> {
    let chars = value.chars().collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut out = BTreeMap::new();

    while cursor < chars.len() {
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor >= chars.len() {
            break;
        }

        let key_start = cursor;
        while cursor < chars.len()
            && !chars[cursor].is_whitespace()
            && chars[cursor] != '='
            && chars[cursor] != '/'
        {
            cursor += 1;
        }
        let key = chars[key_start..cursor]
            .iter()
            .collect::<String>()
            .trim()
            .to_ascii_lowercase();
        if key.is_empty() {
            cursor += 1;
            continue;
        }

        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        let mut value_out = String::new();
        if cursor < chars.len() && chars[cursor] == '=' {
            cursor += 1;
            while cursor < chars.len() && chars[cursor].is_whitespace() {
                cursor += 1;
            }
            if cursor < chars.len() && (chars[cursor] == '"' || chars[cursor] == '\'') {
                let quote = chars[cursor];
                cursor += 1;
                let start = cursor;
                while cursor < chars.len() && chars[cursor] != quote {
                    cursor += 1;
                }
                value_out = chars[start..cursor].iter().collect::<String>();
                if cursor < chars.len() {
                    cursor += 1;
                }
            } else {
                let start = cursor;
                while cursor < chars.len() && !chars[cursor].is_whitespace() && chars[cursor] != '/'
                {
                    cursor += 1;
                }
                value_out = chars[start..cursor].iter().collect::<String>();
            }
        }

        out.insert(key, normalize_spaces(&value_out));
    }

    out
}

fn extract_template_titles(content: &str) -> Vec<String> {
    extract_template_invocations(content)
        .into_iter()
        .map(|invocation| invocation.template_title)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn extract_link_titles(content: &str) -> Vec<String> {
    extract_wikilinks(content)
        .into_iter()
        .map(|link| link.target_title)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn flatten_markup_excerpt(value: &str) -> String {
    let mut output = String::new();
    let bytes = value.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if cursor + 1 < bytes.len() && bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
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
            if let Some(display) = display_text_for_wikilink(&value[start..end])
                && !display.is_empty()
            {
                if !output.ends_with(' ') && !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&display);
                output.push(' ');
            }
            cursor = end + 2;
            continue;
        }

        if bytes[cursor] == b'<' {
            let mut end = cursor + 1;
            while end < bytes.len() && bytes[end] != b'>' {
                end += 1;
            }
            cursor = end.saturating_add(1);
            continue;
        }

        if cursor + 1 < bytes.len() && bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            let mut depth = 1usize;
            let mut end = cursor + 2;
            while end + 1 < bytes.len() && depth > 0 {
                if bytes[end] == b'{' && bytes[end + 1] == b'{' {
                    depth += 1;
                    end += 2;
                    continue;
                }
                if bytes[end] == b'}' && bytes[end + 1] == b'}' {
                    depth = depth.saturating_sub(1);
                    end += 2;
                    continue;
                }
                end += 1;
            }
            cursor = end.min(bytes.len());
            continue;
        }

        if bytes[cursor] == b'[' && (cursor + 1 >= bytes.len() || bytes[cursor + 1] != b'[') {
            let mut end = cursor + 1;
            while end < bytes.len() && bytes[end] != b']' {
                end += 1;
            }
            let inner = if end < bytes.len() {
                &value[cursor + 1..end]
            } else {
                &value[cursor + 1..]
            };
            let label = inner
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            if !label.is_empty() {
                if !output.ends_with(' ') && !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&label);
                output.push(' ');
            }
            cursor = end.saturating_add(1);
            continue;
        }

        output.push(bytes[cursor] as char);
        cursor += 1;
    }

    normalize_spaces(&output)
}

fn display_text_for_wikilink(inner: &str) -> Option<String> {
    let segments = split_template_segments(inner);
    let target = segments.first()?.trim();
    if target.is_empty() {
        return None;
    }
    let display = segments.last().map(String::as_str).unwrap_or(target).trim();
    if let Some((_, tail)) = display.rsplit_once(':') {
        return Some(normalize_spaces(&tail.replace('_', " ")));
    }
    Some(normalize_spaces(&display.replace('_', " ")))
}

fn analyze_reference_body(
    reference_body: &str,
    template_titles: &[String],
    link_titles: &[String],
    reference_name: Option<&str>,
    reference_group: Option<&str>,
) -> ReferenceAnalysis {
    let templates = parse_reference_templates(reference_body);
    let primary_template = choose_primary_reference_template(&templates);
    let primary_template_title = primary_template
        .map(|template| template.template_title.clone())
        .or_else(|| template_titles.first().cloned());

    let mut reference_title = first_reference_text_param(
        primary_template,
        &["title", "chapter", "entry", "article", "script-title"],
    );
    if reference_title.is_empty()
        && let Some(template) = primary_template
        && let Some(value) = template.positional_params.first()
    {
        reference_title = flatten_markup_excerpt(value);
    }
    if reference_title.is_empty()
        && let Some(first_link) = link_titles.first()
    {
        reference_title = first_link.clone();
    }

    let mut source_container = first_reference_text_param(
        primary_template,
        &[
            "website",
            "work",
            "journal",
            "newspaper",
            "magazine",
            "periodical",
            "encyclopedia",
            "publisher",
            "publication",
        ],
    );
    let source_author = reference_author_text(primary_template);
    let source_date = first_reference_text_param(
        primary_template,
        &["date", "year", "publication-date", "access-date"],
    );
    let has_quote =
        !first_reference_text_param(primary_template, &["quote", "quotation"]).is_empty();
    let source_urls = collect_reference_source_urls(primary_template, reference_body);
    let canonical_url = source_urls.first().cloned().unwrap_or_default();
    let archive_url = first_reference_raw_param(primary_template, &["archive-url", "archiveurl"]);
    let source_domain = normalize_source_domain(&canonical_url)
        .or_else(|| archive_url.as_deref().and_then(normalize_source_domain))
        .unwrap_or_default();
    let source_type = classify_reference_source_type(
        primary_template,
        &source_domain,
        !source_urls.is_empty(),
        reference_body,
    );
    if source_container.is_empty()
        && !source_domain.is_empty()
        && matches!(
            source_type.as_str(),
            "web" | "news" | "social" | "video" | "wiki"
        )
    {
        source_container = source_domain.clone();
    }
    let source_origin = source_origin_for_reference(&source_domain, &source_type);
    let source_family = classify_reference_source_family(&source_type, &source_origin);
    let (authority_kind, source_authority) = choose_reference_authority(
        &source_domain,
        &source_container,
        &source_author,
        primary_template_title.as_deref(),
        reference_name,
        &source_type,
    );
    let citation_family = citation_family_for_reference(
        primary_template_title.as_deref(),
        &source_type,
        reference_group,
    );
    let identifier_keys = collect_reference_identifier_keys(
        primary_template,
        !source_urls.is_empty(),
        archive_url.is_some(),
    );
    let identifier_entries = collect_reference_identifier_entries(primary_template);
    let signal_inputs = ReferenceSignalInputs {
        primary_template_title: primary_template_title.as_deref(),
        source_type: &source_type,
        source_origin: &source_origin,
        source_family: &source_family,
        authority_kind: &authority_kind,
        reference_title: &reference_title,
        source_container: &source_container,
        source_author: &source_author,
        source_domain: &source_domain,
        source_date: &source_date,
        identifier_keys: &identifier_keys,
        identifier_entries: &identifier_entries,
        has_quote,
        has_links: !link_titles.is_empty(),
        has_archive: archive_url.is_some(),
        reference_name,
        reference_group,
        reference_body,
    };
    let retrieval_signals = collect_reference_signals(signal_inputs);
    let summary_hint = build_reference_summary_hint(
        &reference_title,
        &source_container,
        &source_author,
        &source_domain,
        &source_authority,
        primary_template_title.as_deref(),
        reference_name,
    );
    let citation_profile = build_reference_citation_profile(
        &source_type,
        &source_origin,
        &citation_family,
        &source_domain,
        &authority_kind,
        &source_authority,
    );

    ReferenceAnalysis {
        citation_profile,
        citation_family,
        primary_template_title,
        source_type,
        source_origin,
        source_family,
        authority_kind,
        source_authority,
        reference_title,
        source_container,
        source_author,
        source_domain,
        source_date,
        canonical_url,
        identifier_keys,
        identifier_entries,
        source_urls,
        retrieval_signals,
        summary_hint,
    }
}

fn parse_reference_templates(reference_body: &str) -> Vec<ReferenceTemplateDetails> {
    extract_template_invocations(reference_body)
        .into_iter()
        .filter_map(|invocation| {
            let inner = invocation
                .raw_wikitext
                .strip_prefix("{{")
                .and_then(|value| value.strip_suffix("}}"))?;
            let segments = split_template_segments(inner);
            let mut named_params = BTreeMap::new();
            let mut positional_params = Vec::new();
            for segment in segments.into_iter().skip(1) {
                if let Some((key, value)) = split_once_top_level_equals(&segment) {
                    named_params.insert(
                        normalize_template_parameter_key(&key),
                        value.trim().to_string(),
                    );
                } else {
                    positional_params.push(segment.trim().to_string());
                }
            }
            Some(ReferenceTemplateDetails {
                template_title: invocation.template_title,
                named_params,
                positional_params,
            })
        })
        .collect()
}

fn choose_primary_reference_template(
    templates: &[ReferenceTemplateDetails],
) -> Option<&ReferenceTemplateDetails> {
    templates.iter().min_by(|left, right| {
        reference_template_priority(&left.template_title)
            .cmp(&reference_template_priority(&right.template_title))
            .then_with(|| left.template_title.cmp(&right.template_title))
    })
}

fn reference_template_priority(template_title: &str) -> u8 {
    let lowered = template_title.to_ascii_lowercase();
    if lowered.contains("cite ") || lowered.contains("citation") {
        return 0;
    }
    if lowered.contains("sfn") || lowered.contains("harv") {
        return 1;
    }
    if lowered.contains("ref") || lowered.contains("note") {
        return 2;
    }
    3
}

fn first_reference_text_param(
    template: Option<&ReferenceTemplateDetails>,
    keys: &[&str],
) -> String {
    let Some(template) = template else {
        return String::new();
    };
    for key in keys {
        if let Some(value) = template.named_params.get(*key) {
            let normalized = flatten_markup_excerpt(value);
            if !normalized.is_empty() {
                return normalized;
            }
        }
    }
    String::new()
}

fn first_reference_raw_param(
    template: Option<&ReferenceTemplateDetails>,
    keys: &[&str],
) -> Option<String> {
    let template = template?;
    for key in keys {
        if let Some(value) = template.named_params.get(*key) {
            let normalized = normalize_spaces(value);
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }
    None
}

fn reference_author_text(template: Option<&ReferenceTemplateDetails>) -> String {
    let Some(template) = template else {
        return String::new();
    };
    for key in ["author", "authors", "last", "last1", "editor"] {
        if let Some(value) = template.named_params.get(key) {
            let normalized = flatten_markup_excerpt(value);
            if !normalized.is_empty() {
                if key == "last" || key == "last1" {
                    let first = template
                        .named_params
                        .get("first")
                        .or_else(|| template.named_params.get("first1"))
                        .map(|value| flatten_markup_excerpt(value))
                        .unwrap_or_default();
                    if !first.is_empty() {
                        return format!("{normalized}, {first}");
                    }
                }
                return normalized;
            }
        }
    }
    String::new()
}

fn collect_reference_identifier_keys(
    template: Option<&ReferenceTemplateDetails>,
    has_url: bool,
    has_archive: bool,
) -> Vec<String> {
    let mut out = BTreeSet::new();
    if let Some(template) = template {
        for key in [
            "doi", "isbn", "issn", "oclc", "pmid", "pmcid", "arxiv", "jstor", "id",
        ] {
            if template
                .named_params
                .get(key)
                .is_some_and(|value| !normalize_spaces(value).is_empty())
            {
                out.insert(key.to_string());
            }
        }
    }
    if has_url {
        out.insert("url".to_string());
    }
    if has_archive {
        out.insert("archive-url".to_string());
    }
    out.into_iter().collect()
}

fn collect_reference_identifier_entries(
    template: Option<&ReferenceTemplateDetails>,
) -> Vec<String> {
    let Some(template) = template else {
        return Vec::new();
    };

    let mut out = BTreeSet::new();
    for key in [
        "doi", "isbn", "issn", "oclc", "pmid", "pmcid", "arxiv", "jstor", "id",
    ] {
        let Some(value) = template.named_params.get(key) else {
            continue;
        };
        let normalized_value = normalize_reference_identifier_value(key, value);
        if normalized_value.is_empty() {
            continue;
        }
        out.insert(format!("{key}:{normalized_value}"));
    }
    out.into_iter().collect()
}

fn collect_reference_source_urls(
    template: Option<&ReferenceTemplateDetails>,
    reference_body: &str,
) -> Vec<String> {
    let mut out = BTreeSet::new();
    if let Some(template) = template {
        for key in [
            "url",
            "chapter-url",
            "article-url",
            "archive-url",
            "archiveurl",
        ] {
            if let Some(value) = template.named_params.get(key)
                && let Some(normalized) = normalize_reference_url(value)
            {
                out.insert(normalized);
            }
        }
    }
    if let Some(url) = extract_first_url(reference_body)
        && let Some(normalized) = normalize_reference_url(&url)
    {
        out.insert(normalized);
    }
    out.into_iter().collect()
}

fn normalize_reference_url(value: &str) -> Option<String> {
    let candidate = normalize_spaces(value);
    if candidate.is_empty() {
        return None;
    }
    if candidate.starts_with("//") {
        return Some(format!("https:{candidate}"));
    }
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        return Some(candidate);
    }
    None
}

fn choose_reference_authority(
    source_domain: &str,
    source_container: &str,
    source_author: &str,
    primary_template_title: Option<&str>,
    reference_name: Option<&str>,
    source_type: &str,
) -> (String, String) {
    if !source_domain.is_empty() {
        return ("domain".to_string(), source_domain.to_string());
    }
    if !source_container.is_empty() {
        return ("container".to_string(), source_container.to_string());
    }
    if !source_author.is_empty() {
        return ("author".to_string(), source_author.to_string());
    }
    if let Some(template_title) = primary_template_title {
        return ("template".to_string(), template_title.to_string());
    }
    if let Some(name) = reference_name {
        let normalized = normalize_spaces(name);
        if !normalized.is_empty() {
            return ("named-reference".to_string(), normalized);
        }
    }
    if !source_type.is_empty() {
        return ("source-type".to_string(), source_type.to_string());
    }
    ("unknown".to_string(), String::new())
}

fn classify_reference_source_family(source_type: &str, source_origin: &str) -> String {
    if source_type.is_empty() {
        return "unknown".to_string();
    }
    if source_origin == "first-party" {
        return format!("first-party-{source_type}");
    }
    source_type.to_string()
}

fn normalize_reference_identifier_value(key: &str, value: &str) -> String {
    let flattened = flatten_markup_excerpt(value);
    if flattened.is_empty() {
        return String::new();
    }
    let lowered = flattened.to_ascii_lowercase();
    match key {
        "doi" => {
            let trimmed = lowered
                .trim_start_matches("https://doi.org/")
                .trim_start_matches("http://doi.org/")
                .trim_start_matches("doi:")
                .trim();
            normalize_reference_identifier_token(trimmed, true)
        }
        "isbn" | "issn" | "oclc" | "pmid" | "pmcid" | "jstor" => {
            normalize_reference_identifier_token(&lowered, false)
        }
        "arxiv" => {
            normalize_reference_identifier_token(lowered.trim_start_matches("arxiv:").trim(), true)
        }
        _ => normalize_spaces(&flattened),
    }
}

fn normalize_reference_identifier_token(value: &str, preserve_slash: bool) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            continue;
        }
        if preserve_slash && matches!(ch, '.' | '/' | '_' | '-') {
            out.push(ch);
        }
    }
    out
}

fn parse_identifier_entries(entries: &[String]) -> Vec<ParsedIdentifierEntry> {
    let mut out = Vec::new();
    for entry in entries {
        let Some((key, value)) = entry.split_once(':') else {
            continue;
        };
        let key = normalize_template_parameter_key(key);
        let value = normalize_spaces(value);
        if key.is_empty() || value.is_empty() {
            continue;
        }
        let normalized_value = normalize_reference_identifier_value(&key, &value);
        if normalized_value.is_empty() {
            continue;
        }
        out.push(ParsedIdentifierEntry {
            key,
            value,
            normalized_value,
        });
    }
    out
}

fn build_reference_authority_key(authority_kind: &str, source_authority: &str) -> String {
    let normalized_authority = normalize_spaces(source_authority);
    if normalized_authority.is_empty() {
        return authority_kind.to_string();
    }
    format!(
        "{}:{}",
        authority_kind,
        normalized_authority.to_ascii_lowercase()
    )
}

fn build_reference_authority_retrieval_text(reference: &IndexedReferenceRecord) -> String {
    let mut values = vec![
        reference.source_authority.clone(),
        reference.reference_title.clone(),
        reference.source_container.clone(),
        reference.source_author.clone(),
        reference.source_domain.clone(),
        reference.source_family.clone(),
        reference.source_type.clone(),
        reference.source_origin.clone(),
        reference.summary_text.clone(),
    ];
    values.extend(reference.identifier_entries.iter().cloned());
    values.extend(reference.template_titles.iter().cloned());
    values.extend(reference.link_titles.iter().cloned());
    collect_normalized_string_list(values).join("\n")
}

#[derive(Clone, Copy)]
struct ReferenceSignalInputs<'a> {
    primary_template_title: Option<&'a str>,
    source_type: &'a str,
    source_origin: &'a str,
    source_family: &'a str,
    authority_kind: &'a str,
    reference_title: &'a str,
    source_container: &'a str,
    source_author: &'a str,
    source_domain: &'a str,
    source_date: &'a str,
    identifier_keys: &'a [String],
    identifier_entries: &'a [String],
    has_quote: bool,
    has_links: bool,
    has_archive: bool,
    reference_name: Option<&'a str>,
    reference_group: Option<&'a str>,
    reference_body: &'a str,
}

fn collect_reference_signals(input: ReferenceSignalInputs<'_>) -> Vec<String> {
    let mut flags = BTreeSet::new();
    if input.primary_template_title.is_some() {
        flags.insert("citation-template".to_string());
    }
    if !input.reference_title.is_empty() {
        flags.insert("has-title".to_string());
    }
    if !input.source_container.is_empty() {
        flags.insert("has-container".to_string());
    }
    if !input.source_author.is_empty() {
        flags.insert("has-author".to_string());
    }
    if !input.source_domain.is_empty() {
        flags.insert("has-domain".to_string());
    }
    if !input.source_date.is_empty() {
        flags.insert("has-date".to_string());
    }
    if !input.identifier_keys.is_empty() {
        flags.insert("has-identifier".to_string());
    }
    if !input.source_family.is_empty() {
        flags.insert(format!("source-family:{}", input.source_family));
    }
    if !input.authority_kind.is_empty() {
        flags.insert(format!("authority:{}", input.authority_kind));
    }
    if input.has_archive {
        flags.insert("has-archive".to_string());
    }
    if input.has_quote {
        flags.insert("has-quote".to_string());
    }
    if input.has_links {
        flags.insert("has-links".to_string());
    }
    if input.reference_name.is_some() {
        flags.insert("named-reference".to_string());
    }
    if input.reference_group.is_some() {
        flags.insert("grouped-reference".to_string());
    }
    if input.reference_body.trim().is_empty() {
        flags.insert("reused-reference".to_string());
    }
    if input.primary_template_title.is_none() && !input.source_domain.is_empty() {
        flags.insert("bare-url".to_string());
    }
    if input.source_origin == "first-party" {
        flags.insert("first-party".to_string());
    }
    for key in input.identifier_keys {
        flags.insert(format!("identifier:{key}"));
    }
    for entry in input.identifier_entries {
        if let Some((key, _)) = entry.split_once(':') {
            flags.insert(format!("identifier-entry:{key}"));
        }
    }
    if matches!(input.source_type, "social" | "video" | "wiki") {
        flags.insert(format!("source-type:{}", input.source_type));
    }
    flags.into_iter().collect()
}

fn classify_reference_source_type(
    template: Option<&ReferenceTemplateDetails>,
    source_domain: &str,
    has_url: bool,
    reference_body: &str,
) -> String {
    if let Some(template) = template {
        let lowered = template.template_title.to_ascii_lowercase();
        if lowered.contains("cite journal") || lowered.contains("journal") {
            return "journal".to_string();
        }
        if lowered.contains("cite book") || lowered.contains("book") {
            return "book".to_string();
        }
        if lowered.contains("cite news") || lowered.contains("news") {
            return "news".to_string();
        }
        if lowered.contains("cite video") || lowered.contains("video") {
            return "video".to_string();
        }
        if lowered.contains("tweet") || lowered.contains("social") {
            return "social".to_string();
        }
        if lowered.contains("wiki") {
            return "wiki".to_string();
        }
        if lowered.contains("sfn") || lowered.contains("harv") {
            return "short-footnote".to_string();
        }
        if lowered.contains("cite web") || lowered.contains("web") {
            return "web".to_string();
        }
    }
    if is_video_domain(source_domain) {
        return "video".to_string();
    }
    if is_social_domain(source_domain) {
        return "social".to_string();
    }
    if is_wiki_domain(source_domain) {
        return "wiki".to_string();
    }
    if has_url {
        return "web".to_string();
    }
    if reference_body.trim().is_empty() {
        return "note".to_string();
    }
    "other".to_string()
}

fn citation_family_for_reference(
    primary_template_title: Option<&str>,
    source_type: &str,
    reference_group: Option<&str>,
) -> String {
    if let Some(template_title) = primary_template_title {
        return template_title.to_string();
    }
    if reference_group.is_some() || source_type == "note" {
        return "note".to_string();
    }
    if source_type == "web" {
        return "bare-url".to_string();
    }
    "<ref>".to_string()
}

fn source_origin_for_reference(source_domain: &str, source_type: &str) -> String {
    if source_domain.ends_with("remilia.org") {
        return "first-party".to_string();
    }
    if source_type == "wiki" {
        return "wiki".to_string();
    }
    if source_domain.is_empty() {
        return "unknown".to_string();
    }
    "external".to_string()
}

fn build_reference_summary_hint(
    reference_title: &str,
    source_container: &str,
    source_author: &str,
    source_domain: &str,
    source_authority: &str,
    primary_template_title: Option<&str>,
    reference_name: Option<&str>,
) -> String {
    if !reference_title.is_empty() && !source_container.is_empty() {
        return format!("{reference_title} ({source_container})");
    }
    if !reference_title.is_empty() {
        return reference_title.to_string();
    }
    if !source_container.is_empty() && !source_author.is_empty() {
        return format!("{source_container} ({source_author})");
    }
    if !source_container.is_empty() {
        return source_container.to_string();
    }
    if !source_author.is_empty() {
        return source_author.to_string();
    }
    if !source_domain.is_empty() {
        return source_domain.to_string();
    }
    if !source_authority.is_empty() {
        return source_authority.to_string();
    }
    if let Some(template_title) = primary_template_title {
        return template_title.to_string();
    }
    if let Some(name) = reference_name {
        return format!("Named reference {name}");
    }
    String::new()
}

fn build_reference_citation_profile(
    source_type: &str,
    source_origin: &str,
    citation_family: &str,
    source_domain: &str,
    authority_kind: &str,
    source_authority: &str,
) -> String {
    if !source_domain.is_empty()
        && matches!(source_type, "web" | "news" | "social" | "video" | "wiki")
    {
        if source_origin == "first-party" {
            return format!("first-party {source_type} / {source_domain}");
        }
        return format!("{source_type} / {source_domain}");
    }
    if !source_authority.is_empty() && matches!(authority_kind, "container" | "author") {
        if source_origin == "first-party" {
            return format!("first-party {source_type} / {source_authority}");
        }
        return format!("{source_type} / {source_authority}");
    }
    if citation_family != "<ref>" && !citation_family.is_empty() {
        return format!("{source_type} / {citation_family}");
    }
    source_type.to_string()
}

fn extract_first_url(value: &str) -> Option<String> {
    for (start, _) in value.char_indices() {
        let rest = &value[start..];
        let starts_http = rest.starts_with("http://");
        let starts_https = rest.starts_with("https://");
        let starts_protocol_relative = rest.starts_with("//");
        if !(starts_http || starts_https || starts_protocol_relative) {
            continue;
        }

        let mut end = value.len();
        for (offset, ch) in rest.char_indices() {
            if ch.is_whitespace() || matches!(ch, '|' | '}' | ']' | '<' | '"' | '\'') {
                end = start + offset;
                break;
            }
        }
        let candidate = normalize_spaces(&value[start..end]);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }
    None
}

fn normalize_source_domain(url: &str) -> Option<String> {
    let candidate = if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.to_string()
    };
    let parsed = Url::parse(&candidate).ok()?;
    let host = parsed
        .host_str()?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    if host.is_empty() { None } else { Some(host) }
}

fn is_social_domain(domain: &str) -> bool {
    matches!(
        domain,
        "twitter.com"
            | "x.com"
            | "farcaster.xyz"
            | "instagram.com"
            | "tiktok.com"
            | "mastodon.social"
    )
}

fn is_video_domain(domain: &str) -> bool {
    matches!(
        domain,
        "youtube.com" | "youtu.be" | "vimeo.com" | "twitch.tv"
    )
}

fn is_wiki_domain(domain: &str) -> bool {
    domain.ends_with(".wikipedia.org")
        || domain.ends_with(".wiktionary.org")
        || domain.ends_with(".wikimedia.org")
        || domain.ends_with(".miraheze.org")
        || domain.ends_with(".fandom.com")
        || domain.starts_with("wiki.")
}

fn is_media_option(value: &str) -> bool {
    let normalized = normalize_spaces(value).to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }
    if matches!(
        normalized.as_str(),
        "thumb"
            | "thumbnail"
            | "frame"
            | "framed"
            | "frameless"
            | "border"
            | "right"
            | "left"
            | "center"
            | "none"
            | "baseline"
            | "sub"
            | "super"
            | "top"
            | "text-top"
            | "middle"
            | "bottom"
    ) {
        return true;
    }
    if normalized.ends_with("px")
        || normalized.starts_with("upright")
        || normalized.starts_with("alt=")
        || normalized.starts_with("link=")
        || normalized.starts_with("page=")
        || normalized.starts_with("class=")
        || normalized.starts_with("lang=")
        || normalized.starts_with("start=")
        || normalized.starts_with("end=")
    {
        return true;
    }
    false
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
    if trimmed.eq_ignore_ascii_case("Image") {
        return Some(Namespace::File.as_str());
    }
    if trimmed.eq_ignore_ascii_case("User") {
        return Some(Namespace::User.as_str());
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
    if let Some(record) = load_page_record_exact(connection, title)? {
        return Ok(Some(record));
    }
    let resolved = resolve_alias_title(connection, title, 6)?;
    if resolved.eq_ignore_ascii_case(title) {
        return Ok(None);
    }
    load_page_record_exact(connection, &resolved)
}

fn load_page_record_exact(
    connection: &Connection,
    title: &str,
) -> Result<Option<IndexedPageRecord>> {
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

fn resolve_alias_title(connection: &Connection, title: &str, max_hops: usize) -> Result<String> {
    let mut current = normalize_query_title(title);
    if current.is_empty() {
        return Ok(current);
    }
    if !table_exists(connection, "indexed_page_aliases")? {
        return Ok(current);
    }
    let mut seen = BTreeSet::new();
    for _ in 0..max_hops.max(1) {
        let normalized = normalize_query_title(&current);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            break;
        }
        let mut statement = connection
            .prepare(
                "SELECT canonical_title
                 FROM indexed_page_aliases
                 WHERE lower(alias_title) = lower(?1)
                 LIMIT 1",
            )
            .context("failed to prepare alias resolution query")?;
        let mut rows = statement
            .query([normalized.as_str()])
            .context("failed to run alias resolution query")?;
        let Some(row) = rows.next().context("failed to read alias resolution row")? else {
            return Ok(normalized);
        };
        let canonical: String = row
            .get(0)
            .context("failed to decode alias canonical title")?;
        if canonical.eq_ignore_ascii_case(&normalized) {
            return Ok(normalized);
        }
        current = canonical;
    }
    Ok(current)
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

fn open_indexed_connection(paths: &ResolvedPaths) -> Result<Option<Connection>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }
    let connection = open_initialized_database_connection(&paths.db_path)?;
    if !has_populated_local_index(&connection)? {
        return Ok(None);
    }
    Ok(Some(connection))
}

fn has_populated_local_index(connection: &Connection) -> Result<bool> {
    if !table_exists(connection, "indexed_pages")? || !table_exists(connection, "indexed_links")? {
        return Ok(false);
    }
    Ok(count_query(connection, "SELECT COUNT(*) FROM indexed_pages")? > 0)
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

fn rebuild_fts_index(connection: &Connection) -> Result<()> {
    if fts_table_exists(connection, "indexed_pages_fts") {
        connection
            .execute_batch("INSERT INTO indexed_pages_fts(indexed_pages_fts) VALUES('rebuild')")
            .context("failed to rebuild indexed_pages_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_chunks_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_chunks_fts(indexed_page_chunks_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_chunks_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_sections_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_sections_fts(indexed_page_sections_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_sections_fts")?;
    }
    if fts_table_exists(connection, "indexed_template_examples_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_template_examples_fts(indexed_template_examples_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_template_examples_fts")?;
    }
    if fts_table_exists(connection, "indexed_module_invocations_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_module_invocations_fts(indexed_module_invocations_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_module_invocations_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_references_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_references_fts(indexed_page_references_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_references_fts")?;
    }
    if fts_table_exists(connection, "indexed_reference_authorities_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_reference_authorities_fts(indexed_reference_authorities_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_reference_authorities_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_media_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_media_fts(indexed_page_media_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_media_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_semantics_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_semantics_fts(indexed_page_semantics_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_semantics_fts")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use super::{
        ActiveTemplateCatalogLookup, AuthoringKnowledgePack, AuthoringKnowledgePackOptions,
        BrokenLinkIssue, LocalChunkAcrossRetrieval, LocalChunkRetrieval, TemplateReferenceLookup,
        build_authoring_knowledge_pack, build_local_context, extract_media_records,
        extract_module_invocations, extract_reference_records, extract_template_invocations,
        extract_wikilinks, load_stored_index_stats, query_active_template_catalog, query_backlinks,
        query_empty_categories, query_orphans, query_search_local, query_template_reference,
        rebuild_index, retrieve_local_context_chunks, retrieve_local_context_chunks_across_pages,
        run_validation_checks,
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
            parser_config_path: project_root
                .join(".wikitool")
                .join(crate::runtime::PARSER_CONFIG_FILENAME),
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
            "Lead paragraph <ref name=\"alpha\">{{Cite web|title=Alpha Source|website=Remilia}}</ref>\n{{Infobox person|name=Alpha|birth_date={{Birth date|2000|1|1}}}}\n[[Image:Alpha.png|thumb|Alpha portrait]]\n== History ==\n[[Beta]] [[Module:Navbar]] [[Category:People]]",
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
        assert_eq!(context.section_summaries.len(), 2);
        assert_eq!(context.section_summaries[0].section_heading, None);
        assert_eq!(
            context.section_summaries[1].section_heading.as_deref(),
            Some("History")
        );
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
        assert_eq!(context.references.len(), 1);
        assert_eq!(
            context.references[0].reference_name.as_deref(),
            Some("alpha")
        );
        assert!(
            context.references[0]
                .template_titles
                .contains(&"Template:Cite web".to_string())
        );
        assert_eq!(context.media.len(), 1);
        assert_eq!(context.media[0].file_title, "File:Alpha.png");
        assert_eq!(context.media[0].caption_text, "Alpha portrait");
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
    fn build_local_context_rejects_indexed_path_escape() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);
        fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
        fs::create_dir_all(&paths.state_dir).expect("create state");

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Alpha body",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let connection =
            crate::schema::open_initialized_database_connection(&paths.db_path).expect("open db");
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .expect("disable foreign keys");
        connection
            .execute(
                "UPDATE indexed_pages SET relative_path = ?1 WHERE title = 'Alpha'",
                ["../outside.txt"],
            )
            .expect("tamper indexed path");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");

        let error = build_local_context(&paths, "Alpha").expect_err("must reject escaped path");
        assert!(
            error
                .to_string()
                .contains("path escapes scoped runtime directories")
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
    fn extract_module_invocations_captures_functions_and_parameter_keys() {
        let content = "{{#invoke:Infobox person|render|name=Alpha|occupation=Archivist|2020}} {{#invoke:Infobox person|render|name=Alpha|occupation=Archivist|2020}}";
        let invocations = extract_module_invocations(content);

        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].module_title, "Module:Infobox person");
        assert_eq!(invocations[0].function_name, "render");
        assert_eq!(
            invocations[0].parameter_keys,
            vec![
                "$1".to_string(),
                "name".to_string(),
                "occupation".to_string()
            ]
        );
        assert!(
            invocations[0]
                .raw_wikitext
                .contains("#invoke:Infobox person")
        );
    }

    #[test]
    fn extract_reference_records_parses_named_refs_and_template_summaries() {
        let content = "Lead <ref name=\"alpha\">{{Cite web|title=Alpha Source|url=https://remilia.org/alpha|website=Remilia|author=Jane Example|date=2025-01-01|doi=10.1234/Alpha-01}}</ref> tail <ref group=\"note\" name=\"reuse\" />";
        let references = extract_reference_records(content);

        assert_eq!(references.len(), 2);
        assert_eq!(references[0].reference_name.as_deref(), Some("alpha"));
        assert_eq!(
            references[0].template_titles,
            vec!["Template:Cite web".to_string()]
        );
        assert_eq!(references[0].citation_family, "Template:Cite web");
        assert_eq!(
            references[0].primary_template_title.as_deref(),
            Some("Template:Cite web")
        );
        assert_eq!(references[0].source_type, "web");
        assert_eq!(references[0].source_origin, "first-party");
        assert_eq!(references[0].source_family, "first-party-web");
        assert_eq!(references[0].authority_kind, "domain");
        assert_eq!(references[0].source_authority, "remilia.org");
        assert_eq!(references[0].reference_title, "Alpha Source");
        assert_eq!(references[0].source_container, "Remilia");
        assert_eq!(references[0].source_author, "Jane Example");
        assert_eq!(references[0].source_domain, "remilia.org");
        assert_eq!(references[0].source_date, "2025-01-01");
        assert_eq!(references[0].canonical_url, "https://remilia.org/alpha");
        assert!(
            references[0]
                .identifier_entries
                .iter()
                .any(|entry| entry == "doi:10.1234/alpha-01")
        );
        assert!(
            references[0]
                .source_urls
                .iter()
                .any(|url| url == "https://remilia.org/alpha")
        );
        assert!(references[0].citation_profile.contains("web"));
        assert!(references[0].citation_profile.contains("remilia.org"));
        assert!(
            references[0]
                .retrieval_signals
                .iter()
                .any(|flag| flag == "first-party")
        );
        assert!(references[0].summary_text.contains("Alpha Source"));
        assert_eq!(references[1].reference_name.as_deref(), Some("reuse"));
        assert_eq!(references[1].reference_group.as_deref(), Some("note"));
        assert_eq!(references[1].summary_text, "reuse");
    }

    #[test]
    fn section_authoring_bias_prefers_content_sections_over_reference_tail() {
        let history_score = super::retrieval::section_authoring_bias(
            Some("History"),
            "Alpha biography summary with useful prose.",
        );
        let references_score = super::retrieval::section_authoring_bias(
            Some("References"),
            "{{Reflist}}\n[[Category:Test]]",
        );

        assert!(history_score > references_score);
        assert!(references_score < 0);
    }

    #[test]
    fn extract_media_records_parses_inline_and_gallery_entries() {
        let content = "[[Image:Alpha.png|thumb|Alpha portrait]]\n<gallery mode=\"packed\">\nFile:Beta.jpg|Beta gallery caption\n</gallery>";
        let media = extract_media_records(content);

        assert_eq!(media.len(), 2);
        assert_eq!(media[0].file_title, "File:Alpha.png");
        assert_eq!(media[0].media_kind, "inline");
        assert_eq!(media[0].caption_text, "Alpha portrait");
        assert_eq!(media[1].file_title, "File:Beta.jpg");
        assert_eq!(media[1].media_kind, "gallery");
        assert!(media[1].caption_text.contains("Beta gallery caption"));
        assert!(
            media[1]
                .options
                .iter()
                .any(|option| option == "mode=packed")
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
        assert!(report.retrieval_mode == "fts" || report.retrieval_mode == "like");
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
    fn retrieve_local_context_chunks_across_pages_uses_hybrid_term_expansion() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths
                .wiki_content_dir
                .join("Main")
                .join("Alpha_Beacon.wiki"),
            "Lead alpha marker.\n== History ==\nThe beacon emits a steady signal for retrieval tests.",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Noise.wiki"),
            "Alpha beacon phrase never appears here, only unrelated noise.",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let retrieval =
            retrieve_local_context_chunks_across_pages(&paths, "Alpha Beacon", 3, 180, 2, true)
                .expect("across-pages retrieval");
        let report = match retrieval {
            LocalChunkAcrossRetrieval::Found(report) => report,
            other => panic!("expected found report, got {other:?}"),
        };
        assert!(report.retrieval_mode.contains("hybrid"));
        assert!(
            report
                .chunks
                .iter()
                .any(|chunk| chunk.source_title == "Alpha Beacon")
        );
    }

    #[test]
    fn retrieve_local_context_chunks_across_pages_uses_semantic_page_profiles() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths
                .wiki_content_dir
                .join("Main")
                .join("Alpha_Beacon.wiki"),
            "Lead prose without the page title words.\n== History ==\nThe hidden signal stays buried in ordinary text.",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Noise.wiki"),
            "Noise page with unrelated prose only.",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let retrieval =
            retrieve_local_context_chunks_across_pages(&paths, "Alpha Beacon", 3, 180, 2, true)
                .expect("across-pages retrieval");
        let report = match retrieval {
            LocalChunkAcrossRetrieval::Found(report) => report,
            other => panic!("expected found report, got {other:?}"),
        };
        assert!(report.retrieval_mode.contains("semantic"));
        assert!(report.retrieval_mode.contains("seed-pages"));
        assert!(
            report
                .chunks
                .iter()
                .any(|chunk| chunk.source_title == "Alpha Beacon")
        );
    }

    #[test]
    fn retrieve_local_context_chunks_across_pages_uses_reference_authority_and_identifier_hits() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "Lead prose without publisher words.\n<ref>{{Cite book|title=Alpha Source|publisher=Remilia Press|isbn=978-1-4028-9462-6}}</ref>",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Noise.wiki"),
            "Noise page with unrelated prose only.",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let authority_retrieval =
            retrieve_local_context_chunks_across_pages(&paths, "Remilia Press", 3, 180, 2, true)
                .expect("authority retrieval");
        let authority_report = match authority_retrieval {
            LocalChunkAcrossRetrieval::Found(report) => report,
            other => panic!("expected found report, got {other:?}"),
        };
        assert!(authority_report.retrieval_mode.contains("authority"));
        assert!(authority_report.retrieval_mode.contains("seed-pages"));
        assert!(
            authority_report
                .chunks
                .iter()
                .any(|chunk| chunk.source_title == "Alpha")
        );

        let identifier_retrieval =
            retrieve_local_context_chunks_across_pages(&paths, "9781402894626", 3, 180, 2, true)
                .expect("identifier retrieval");
        let identifier_report = match identifier_retrieval {
            LocalChunkAcrossRetrieval::Found(report) => report,
            other => panic!("expected found report, got {other:?}"),
        };
        assert!(identifier_report.retrieval_mode.contains("identifier"));
        assert!(identifier_report.retrieval_mode.contains("seed-pages"));
        assert!(
            identifier_report
                .chunks
                .iter()
                .any(|chunk| chunk.source_title == "Alpha")
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
            "{{Infobox person|name=Alpha|born=2020}}\n'''Alpha''' works with [[Beta]] and [[Gamma]].<ref>{{Cite web|title=Alpha Source|website=Remilia}}</ref>\n[[Image:Alpha.png|thumb|Alpha portrait]]\n[[Category:People]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "{{Infobox organization|name=Beta Org|founder=Alpha}}\n'''Beta''' references [[Alpha]] and [[Gamma]].<ref>{{Cite book|title=Beta Book|publisher=Remilia Press}}</ref>\n[[File:Beta.jpg|thumb|Beta portrait]]\n[[Category:Organizations]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
            "{{Navbox|name=Gamma nav|list1=[[Alpha]]}}\n'''Gamma''' is linked with [[Alpha]].<ref name=\"gamma-source\" />\n[[Category:People]]",
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
            docs_profile: crate::knowledge::status::DEFAULT_DOCS_PROFILE.to_string(),
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
        assert!(report.query_terms.contains(&"Alpha".to_string()));
        assert_eq!(report.pack_token_budget, 420);
        assert!(report.pack_token_estimate_total >= report.token_estimate_total);
        assert!(report.inventory.indexed_pages_total >= 3);
        assert!(report.inventory.reference_rows_total >= 3);
        assert!(report.inventory.media_rows_total >= 2);
        assert!(!report.related_pages.is_empty());
        assert!(
            report
                .suggested_links
                .iter()
                .any(|entry| entry.title == "Alpha")
        );
        assert!(
            report
                .suggested_templates
                .iter()
                .any(|entry| entry.template_title == "Template:Infobox person")
        );
        assert!(
            report
                .suggested_templates
                .iter()
                .any(|entry| !entry.example_invocations.is_empty())
        );
        assert!(report.suggested_references.iter().any(|entry| {
            entry.citation_family == "Template:Cite web"
                && entry.source_type == "web"
                && entry
                    .common_retrieval_signals
                    .iter()
                    .any(|signal| signal == "citation-template")
        }));
        assert!(
            report
                .suggested_media
                .iter()
                .any(|entry| entry.file_title == "File:Alpha.png")
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
                .iter()
                .any(|entry| entry.template_title == "Template:Infobox person")
        );
        assert!(report.retrieval_mode.contains("hybrid"));
        assert!(report.retrieval_mode.contains("across"));
        assert!(report.token_estimate_total <= 420);
    }

    #[test]
    fn build_authoring_knowledge_pack_uses_template_matches_for_related_pages() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "{{Infobox person|name=Alpha|occupation=Archivist}}\n'''Alpha''' chronicle text with SaffronSignal authoring detail.",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "{{Infobox location|name=Beta}}\n'''Beta''' location text with unrelated detail.",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let options = AuthoringKnowledgePackOptions {
            related_page_limit: 4,
            chunk_limit: 4,
            token_budget: 240,
            max_pages: 2,
            link_limit: 4,
            category_limit: 4,
            template_limit: 4,
            docs_profile: crate::knowledge::status::DEFAULT_DOCS_PROFILE.to_string(),
            diversify: true,
        };
        let report = build_authoring_knowledge_pack(
            &paths,
            Some("Unmatched Draft Topic"),
            Some("{{Infobox person|name=Draft|occupation=Archivist}}\nDraft body without direct links."),
            &options,
        )
        .expect("authoring pack");
        let report = match report {
            AuthoringKnowledgePack::Found(report) => *report,
            other => panic!("expected found authoring pack, got {other:?}"),
        };

        assert!(
            report
                .related_pages
                .iter()
                .any(|entry| entry.title == "Alpha" && entry.source.contains("template-match"))
        );
        assert!(
            report
                .chunks
                .iter()
                .any(|chunk| chunk.source_title == "Alpha")
        );
        assert!(report.retrieval_mode.contains("seed-pages"));
    }

    #[test]
    fn template_catalog_and_reference_include_examples_and_implementation_context() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "{{Infobox person|name=Alpha|born=2020}}\n'''Alpha''' page.",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "{{Infobox person|name=Beta|occupation=Archivist}}\n'''Beta''' page.",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Template_Infobox_person.wiki"),
            "Template lead text.\n{{#invoke:Infobox person|render}}\n== Parameters ==\nUse |name= and |occupation=.",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Module_Infobox_person.wiki"),
            "return { render = function() end }",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Template_Infobox_person___doc.wiki"),
            "Documentation lead.\n== Usage ==\nUse the template on biographies.",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("_redirects")
                .join("Template_Infobox_human.wiki"),
            "#REDIRECT [[Template:Infobox person]]",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let catalog = query_active_template_catalog(&paths, 10).expect("catalog query");
        let catalog = match catalog {
            ActiveTemplateCatalogLookup::Found(catalog) => catalog,
            other => panic!("expected template catalog, got {other:?}"),
        };
        assert!(catalog.active_template_count >= 1);
        let infobox = catalog
            .templates
            .iter()
            .find(|template| template.template_title == "Template:Infobox person")
            .expect("infobox person in catalog");
        assert!(
            infobox
                .aliases
                .contains(&"Template:Infobox human".to_string())
        );
        assert!(infobox.parameter_stats.iter().any(|stat| {
            stat.key == "name"
                && stat.usage_count >= 2
                && stat.example_values.iter().any(|value| value == "Alpha")
        }));
        assert!(!infobox.example_invocations.is_empty());
        assert!(
            infobox
                .implementation_titles
                .iter()
                .any(|title| title == "Module:Infobox person")
        );

        let reference =
            query_template_reference(&paths, "Infobox person").expect("template reference query");
        let reference = match reference {
            TemplateReferenceLookup::Found(reference) => *reference,
            other => panic!("expected template reference, got {other:?}"),
        };
        assert_eq!(reference.template.template_title, "Template:Infobox person");
        assert!(
            reference
                .implementation_pages
                .iter()
                .any(|page| page.role == "module" && page.page_title == "Module:Infobox person")
        );
        assert!(
            reference
                .implementation_pages
                .iter()
                .any(|page| page.role == "documentation")
        );
        assert!(
            reference
                .implementation_sections
                .iter()
                .any(|section| section.section_heading.as_deref() == Some("Parameters"))
        );
        assert!(!reference.implementation_chunks.is_empty());
    }

    #[test]
    fn build_authoring_knowledge_pack_bridges_templates_modules_and_docs() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create project root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "{{Infobox person|name=Alpha|occupation=Archivist}}\n'''Alpha''' article body with [[Beta]].",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "'''Beta''' article body linked from [[Alpha]].",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Template_Infobox_person.wiki"),
            "Template lead text.\n{{#invoke:Infobox person|render|name=Example|occupation=Archivist}}\n== Parameters ==\nUse |name= and |occupation=.",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Module_Infobox_person.wiki"),
            "return { render = function(frame) return frame.args.name end }",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Template_Infobox_person___doc.wiki"),
            "Documentation lead.\n== Usage ==\nUse {{Infobox person}} for biographies.",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let bundle_path = project_root.join("authoring_docs_bundle.json");
        write_file(
            &bundle_path,
            r#"{
  "schema_version": 1,
  "generated_at_unix": 1739000000,
  "source": "authoring_bridge_test",
  "technical": [
    {
      "doc_type": "manual",
      "pages": [
        {
          "page_title": "Manual:Scribunto",
          "local_path": "manual/Scribunto.md",
          "content": "Scribunto supports {{#invoke:Infobox person|render|name=Alpha}} for Lua-backed templates. Template parameters should map cleanly to module functions."
        }
      ]
    }
  ]
}"#,
        );
        crate::docs::import_docs_bundle(&paths, &bundle_path).expect("import docs bundle");
        let connection =
            crate::schema::open_initialized_database_connection(&paths.db_path).expect("open db");
        connection
            .execute(
                "UPDATE docs_corpora SET source_profile = 'remilia-mw-1.44'",
                [],
            )
            .expect("set docs profile");

        let report = build_authoring_knowledge_pack(
            &paths,
            Some("Alpha"),
            None,
            &AuthoringKnowledgePackOptions {
                related_page_limit: 6,
                chunk_limit: 6,
                token_budget: 420,
                max_pages: 4,
                link_limit: 8,
                category_limit: 4,
                template_limit: 6,
                docs_profile: crate::knowledge::status::DEFAULT_DOCS_PROFILE.to_string(),
                diversify: true,
            },
        )
        .expect("authoring pack");
        let report = match report {
            AuthoringKnowledgePack::Found(report) => *report,
            other => panic!("expected found authoring pack, got {other:?}"),
        };

        assert!(
            report
                .template_references
                .iter()
                .any(|reference| reference.template.template_title == "Template:Infobox person")
        );
        assert!(
            report
                .module_patterns
                .iter()
                .any(|module| module.module_title == "Module:Infobox person")
        );
        let docs_context = report.docs_context.expect("docs context must exist");
        assert!(
            docs_context
                .queries
                .iter()
                .any(|query| query == "Scribunto #invoke")
        );
        assert!(
            docs_context
                .pages
                .iter()
                .any(|page| page.page_title == "Manual:Scribunto")
        );
        assert!(report.retrieval_mode.contains("template-guides"));
        assert!(report.retrieval_mode.contains("module-patterns"));
        assert!(report.retrieval_mode.contains("docs-bridge"));
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

    #[test]
    fn extract_first_url_handles_multibyte_prefix_text() {
        let value = "👽 recap https://example.org/path?query=1|rest";
        assert_eq!(
            super::extract_first_url(value),
            Some("https://example.org/path?query=1".to_string())
        );
    }
}
