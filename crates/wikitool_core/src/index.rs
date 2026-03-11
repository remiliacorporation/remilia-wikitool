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
    let files = scan_files(paths, options)?;
    let scan = summarize_files(&files);
    let mut connection = open_initialized_database_connection(&paths.db_path)?;
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

    let mut alias_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_page_aliases (
                alias_title,
                canonical_title,
                canonical_namespace,
                source_relative_path
            ) VALUES (?1, ?2, ?3, ?4)",
        )
        .context("failed to prepare indexed_page_aliases insert")?;

    let mut section_statement = transaction
        .prepare(
            "INSERT INTO indexed_page_sections (
                source_relative_path,
                section_index,
                source_title,
                source_namespace,
                section_heading,
                section_level,
                summary_text,
                section_text,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .context("failed to prepare indexed_page_sections insert")?;

    let mut template_example_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_template_examples (
                template_title,
                source_relative_path,
                source_title,
                invocation_index,
                example_wikitext,
                parameter_keys,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .context("failed to prepare indexed_template_examples insert")?;

    let mut module_invocation_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_module_invocations (
                source_relative_path,
                invocation_index,
                source_title,
                source_namespace,
                module_title,
                function_name,
                parameter_keys,
                invocation_wikitext,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .context("failed to prepare indexed_module_invocations insert")?;

    let mut reference_statement = transaction
        .prepare(
            "INSERT INTO indexed_page_references (
                source_relative_path,
                reference_index,
                source_title,
                source_namespace,
                section_heading,
                reference_name,
                reference_group,
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
                summary_text,
                reference_wikitext,
                template_titles,
                link_titles,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)",
        )
        .context("failed to prepare indexed_page_references insert")?;

    let mut reference_authority_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_reference_authorities (
                source_relative_path,
                reference_index,
                source_title,
                source_namespace,
                section_heading,
                citation_profile,
                citation_family,
                source_type,
                source_origin,
                source_family,
                authority_kind,
                authority_key,
                authority_label,
                primary_template_title,
                source_domain,
                source_container,
                source_author,
                identifier_keys,
                summary_text,
                retrieval_text
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        )
        .context("failed to prepare indexed_reference_authorities insert")?;

    let mut reference_identifier_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_reference_identifiers (
                source_relative_path,
                reference_index,
                source_title,
                source_namespace,
                section_heading,
                citation_profile,
                citation_family,
                source_type,
                source_origin,
                source_family,
                authority_key,
                authority_label,
                identifier_key,
                identifier_value,
                normalized_value,
                summary_text
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )
        .context("failed to prepare indexed_reference_identifiers insert")?;

    let mut media_statement = transaction
        .prepare(
            "INSERT INTO indexed_page_media (
                source_relative_path,
                media_index,
                source_title,
                source_namespace,
                section_heading,
                file_title,
                media_kind,
                caption_text,
                options_text,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .context("failed to prepare indexed_page_media insert")?;

    let mut semantic_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_page_semantics (
                source_relative_path,
                source_title,
                source_namespace,
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
                semantic_text,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        )
        .context("failed to prepare indexed_page_semantics insert")?;

    let mut template_implementation_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_template_implementation_pages (
                template_title,
                implementation_page_title,
                implementation_namespace,
                source_relative_path,
                role
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .context("failed to prepare indexed_template_implementation_pages insert")?;

    let mut inserted_rows = 0usize;
    let mut inserted_links = 0usize;
    let mut template_implementation_seeds = BTreeMap::<String, TemplateImplementationSeed>::new();
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
        let links = extract_wikilinks(&content);
        for link in &links {
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

        if file.is_redirect
            && let Some(target) = file.redirect_target.as_deref()
            && let Some((canonical_title, canonical_namespace)) =
                normalize_title_and_namespace(target)
        {
            alias_statement
                .execute(params![
                    file.title,
                    canonical_title,
                    canonical_namespace,
                    file.relative_path,
                ])
                .with_context(|| format!("failed to insert alias for {}", file.relative_path))?;
        }

        let artifacts = extract_page_artifacts(&content);
        maybe_record_template_implementation_seed(
            &mut template_implementation_seeds,
            file,
            &artifacts,
        );
        let semantic_profile = build_page_semantic_profile(file, &links, &artifacts);
        for (section_index, section) in artifacts.section_records.iter().enumerate() {
            section_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(section_index).context("section index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    section.section_heading.as_deref(),
                    i64::from(section.section_level),
                    section.summary_text,
                    section.section_text,
                    i64::try_from(section.token_estimate)
                        .context("section token estimate does not fit into i64")?,
                ])
                .with_context(|| format!("failed to insert sections for {}", file.relative_path))?;
        }

        for (chunk_index, chunk) in artifacts.context_chunks.iter().enumerate() {
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

        for (reference_index, reference) in artifacts.references.iter().enumerate() {
            reference_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(reference_index)
                        .context("reference index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    reference.section_heading.as_deref(),
                    reference.reference_name.as_deref(),
                    reference.reference_group.as_deref(),
                    reference.citation_profile.as_str(),
                    reference.citation_family.as_str(),
                    reference
                        .primary_template_title
                        .as_deref()
                        .unwrap_or_default(),
                    reference.source_type.as_str(),
                    reference.source_origin.as_str(),
                    reference.source_family.as_str(),
                    reference.authority_kind.as_str(),
                    reference.source_authority.as_str(),
                    reference.reference_title.as_str(),
                    reference.source_container.as_str(),
                    reference.source_author.as_str(),
                    reference.source_domain.as_str(),
                    reference.source_date.as_str(),
                    reference.canonical_url.as_str(),
                    serialize_string_list(&reference.identifier_keys),
                    serialize_string_list(&reference.identifier_entries),
                    serialize_string_list(&reference.source_urls),
                    serialize_string_list(&reference.retrieval_signals),
                    reference.summary_text.as_str(),
                    reference.reference_wikitext.as_str(),
                    serialize_string_list(&reference.template_titles),
                    serialize_string_list(&reference.link_titles),
                    i64::try_from(reference.token_estimate)
                        .context("reference token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!("failed to insert reference rows for {}", file.relative_path)
                })?;

            let authority_key = build_reference_authority_key(
                &reference.authority_kind,
                &reference.source_authority,
            );
            let authority_label = normalize_spaces(&reference.source_authority);
            let authority_identifier_keys = serialize_string_list(&reference.identifier_keys);
            let authority_retrieval_text = build_reference_authority_retrieval_text(reference);
            reference_authority_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(reference_index)
                        .context("reference index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    reference.section_heading.as_deref(),
                    reference.citation_profile.as_str(),
                    reference.citation_family.as_str(),
                    reference.source_type.as_str(),
                    reference.source_origin.as_str(),
                    reference.source_family.as_str(),
                    reference.authority_kind.as_str(),
                    authority_key.as_str(),
                    authority_label.as_str(),
                    reference
                        .primary_template_title
                        .as_deref()
                        .unwrap_or_default(),
                    reference.source_domain.as_str(),
                    reference.source_container.as_str(),
                    reference.source_author.as_str(),
                    authority_identifier_keys.as_str(),
                    reference.summary_text.as_str(),
                    authority_retrieval_text.as_str(),
                ])
                .with_context(|| {
                    format!(
                        "failed to insert reference authority row for {}",
                        file.relative_path
                    )
                })?;

            for entry in parse_identifier_entries(&reference.identifier_entries) {
                reference_identifier_statement
                    .execute(params![
                        file.relative_path,
                        i64::try_from(reference_index)
                            .context("reference index does not fit into i64")?,
                        file.title,
                        file.namespace,
                        reference.section_heading.as_deref(),
                        reference.citation_profile.as_str(),
                        reference.citation_family.as_str(),
                        reference.source_type.as_str(),
                        reference.source_origin.as_str(),
                        reference.source_family.as_str(),
                        authority_key.as_str(),
                        authority_label.as_str(),
                        entry.key.as_str(),
                        entry.value.as_str(),
                        entry.normalized_value.as_str(),
                        reference.summary_text.as_str(),
                    ])
                    .with_context(|| {
                        format!(
                            "failed to insert reference identifier row for {}",
                            file.relative_path
                        )
                    })?;
            }
        }

        for (media_index, media) in artifacts.media.iter().enumerate() {
            media_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(media_index).context("media index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    media.section_heading.as_deref(),
                    media.file_title.as_str(),
                    media.media_kind.as_str(),
                    media.caption_text.as_str(),
                    serialize_string_list(&media.options),
                    i64::try_from(media.token_estimate)
                        .context("media token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!("failed to insert media rows for {}", file.relative_path)
                })?;
        }

        semantic_statement
            .execute(params![
                file.relative_path,
                semantic_profile.source_title.as_str(),
                semantic_profile.source_namespace.as_str(),
                semantic_profile.summary_text.as_str(),
                serialize_string_list(&semantic_profile.section_headings),
                serialize_string_list(&semantic_profile.category_titles),
                serialize_string_list(&semantic_profile.template_titles),
                serialize_string_list(&semantic_profile.template_parameter_keys),
                serialize_string_list(&semantic_profile.link_titles),
                serialize_string_list(&semantic_profile.reference_titles),
                serialize_string_list(&semantic_profile.reference_containers),
                serialize_string_list(&semantic_profile.reference_domains),
                serialize_string_list(&semantic_profile.reference_source_families),
                serialize_string_list(&semantic_profile.reference_authorities),
                serialize_string_list(&semantic_profile.reference_identifiers),
                serialize_string_list(&semantic_profile.media_titles),
                serialize_string_list(&semantic_profile.media_captions),
                serialize_string_list(&semantic_profile.template_implementation_titles),
                semantic_profile.semantic_text.as_str(),
                i64::try_from(semantic_profile.token_estimate)
                    .context("semantic profile token estimate does not fit into i64")?,
            ])
            .with_context(|| {
                format!(
                    "failed to insert semantic profile for {}",
                    file.relative_path
                )
            })?;

        let mut seen_signatures = BTreeSet::new();
        for (invocation_index, invocation) in artifacts.template_invocations.into_iter().enumerate()
        {
            let parameter_keys = canonical_parameter_key_list(&invocation.parameter_keys);
            let signature = format!("{}|{}", invocation.template_title, parameter_keys);
            if !seen_signatures.insert(signature) {
                template_example_statement
                    .execute(params![
                        invocation.template_title,
                        file.relative_path,
                        file.title,
                        i64::try_from(invocation_index)
                            .context("invocation index does not fit into i64")?,
                        invocation.raw_wikitext,
                        parameter_keys,
                        i64::try_from(invocation.token_estimate)
                            .context("invocation token estimate does not fit into i64")?,
                    ])
                    .with_context(|| {
                        format!(
                            "failed to insert template example for {}",
                            file.relative_path
                        )
                    })?;
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
            template_example_statement
                .execute(params![
                    invocation.template_title,
                    file.relative_path,
                    file.title,
                    i64::try_from(invocation_index)
                        .context("invocation index does not fit into i64")?,
                    invocation.raw_wikitext,
                    parameter_keys,
                    i64::try_from(invocation.token_estimate)
                        .context("invocation token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!(
                        "failed to insert template example for {}",
                        file.relative_path
                    )
                })?;
        }
        for (invocation_index, invocation) in artifacts.module_invocations.into_iter().enumerate() {
            module_invocation_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(invocation_index)
                        .context("module invocation index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    invocation.module_title,
                    invocation.function_name,
                    canonical_parameter_key_list(&invocation.parameter_keys),
                    invocation.raw_wikitext,
                    i64::try_from(invocation.token_estimate)
                        .context("module invocation token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!(
                        "failed to insert module invocations for {}",
                        file.relative_path
                    )
                })?;
        }
    }
    persist_template_implementation_pages(
        &mut template_implementation_statement,
        &files,
        &template_implementation_seeds,
    )?;
    drop(template_implementation_statement);
    drop(media_statement);
    drop(semantic_statement);
    drop(reference_identifier_statement);
    drop(reference_authority_statement);
    drop(reference_statement);
    drop(module_invocation_statement);
    drop(template_example_statement);
    drop(section_statement);
    drop(alias_statement);
    drop(template_invocation_statement);
    drop(chunk_statement);
    drop(link_statement);
    drop(page_statement);

    transaction
        .commit()
        .context("failed to commit index rebuild transaction")?;

    // Rebuild FTS5 index if the virtual table exists from schema bootstrap.
    rebuild_fts_index(&connection)?;
    record_content_index_artifact(
        &connection,
        inserted_rows,
        &json!({
            "inserted_rows": inserted_rows,
            "inserted_links": inserted_links,
            "scan_total_files": scan.total_files,
            "scan_content_files": scan.content_files,
            "scan_template_files": scan.template_files,
            "scan_redirects": scan.redirects,
            "namespaces": scan.by_namespace.clone(),
        })
        .to_string(),
    )?;

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

    let connection = open_initialized_database_connection(&paths.db_path)?;
    if !has_populated_local_index(&connection)? {
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
    let link_rows = load_outgoing_link_rows(&connection, &page.relative_path)?;
    let backlinks = query_backlinks_for_connection(&connection, &page.title)?;
    let mut content = None;

    let section_rows =
        if let Some(rows) = load_section_records_for_bundle(&connection, &page.relative_path)? {
            rows
        } else {
            let loaded = load_page_content(paths, &page.relative_path)?;
            let rows = extract_section_records(&loaded);
            content = Some(loaded);
            rows
        };
    let sections = section_rows
        .iter()
        .filter_map(|section| {
            let heading = section.section_heading.as_ref()?.clone();
            Some(LocalContextHeading {
                level: section.section_level,
                heading,
            })
        })
        .take(AUTHORING_SECTION_LIMIT)
        .collect::<Vec<_>>();
    let section_summaries = section_rows
        .iter()
        .take(AUTHORING_SECTION_LIMIT)
        .map(|section| LocalSectionSummary {
            section_heading: section.section_heading.clone(),
            section_level: section.section_level,
            summary_text: section.summary_text.clone(),
            token_estimate: section.token_estimate,
        })
        .collect::<Vec<_>>();
    let word_count = section_rows
        .iter()
        .map(|section| count_words(&section.section_text))
        .sum::<usize>();
    let content_preview = section_rows
        .iter()
        .find_map(|section| {
            let summary = normalize_spaces(&section.summary_text);
            if summary.is_empty() {
                None
            } else {
                Some(summary)
            }
        })
        .unwrap_or_else(|| {
            let loaded = content.get_or_insert(String::new());
            make_content_preview(loaded, 280)
        });
    let context_chunks =
        match load_context_chunks_for_bundle(&connection, &page.relative_path, content.as_deref())?
        {
            Some(chunks) => chunks,
            None => {
                let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
                fallback_context_chunks_from_content(loaded)
            }
        };
    let context_tokens_estimate = context_chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();
    let template_invocations =
        match load_template_invocations_for_bundle(&connection, &page.relative_path)? {
            Some(invocations) => invocations,
            None => {
                let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
                summarize_template_invocations(
                    extract_template_invocations(loaded),
                    TEMPLATE_INVOCATION_LIMIT,
                )
            }
        };
    let references = match load_references_for_bundle(&connection, &page.relative_path)? {
        Some(references) => references,
        None => {
            let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
            extract_reference_records(loaded)
        }
    };
    let media = match load_media_for_bundle(&connection, &page.relative_path)? {
        Some(media) => media,
        None => {
            let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
            extract_media_records(loaded)
        }
    };

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
        word_count,
        content_preview,
        sections,
        section_summaries,
        context_chunks,
        context_tokens_estimate,
        outgoing_links: outgoing_set.into_iter().collect(),
        backlinks,
        categories: category_set.into_iter().collect(),
        templates: template_set.into_iter().collect(),
        modules: module_set.into_iter().collect(),
        template_invocations,
        references,
        media,
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
    let query_terms = expand_retrieval_query_terms(&normalized_query);
    let semantic_page_hits =
        load_semantic_page_hits(&connection, &query_terms, max_pages.max(limit).max(1))?;
    let authority_page_hits =
        load_reference_authority_page_hits(&connection, &query_terms, max_pages.max(limit).max(1))?;
    let identifier_page_hits = load_reference_identifier_page_hits(
        &connection,
        &query_terms,
        max_pages.max(limit).max(1),
    )?;
    let mut seed_pages = BTreeSet::new();
    let mut related_page_titles = Vec::new();
    for title in semantic_page_hits
        .iter()
        .chain(authority_page_hits.iter())
        .chain(identifier_page_hits.iter())
        .map(|hit| hit.title.clone())
    {
        if seed_pages.insert(title.to_ascii_lowercase()) {
            related_page_titles.push(title);
        }
    }
    let report = retrieve_reranked_chunks_across_pages(
        &connection,
        paths,
        &normalized_query,
        &query_terms,
        ChunkRetrievalPlan {
            limit,
            token_budget,
            max_pages,
            diversify,
        },
        &related_page_titles,
        ChunkRerankSignals {
            semantic_page_weights: build_semantic_page_weight_map(&semantic_page_hits),
            authority_page_weights: build_authority_page_weight_map(&authority_page_hits),
            identifier_page_weights: build_identifier_page_weight_map(&identifier_page_hits),
            ..ChunkRerankSignals::default()
        },
    )?;
    Ok(LocalChunkAcrossRetrieval::Found(report))
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

    let topic = if !normalized_topic.is_empty() {
        normalized_topic
    } else if let Some(first_link) = stub_link_titles.first() {
        first_link.clone()
    } else {
        String::new()
    };
    if topic.is_empty() {
        return Ok(AuthoringKnowledgePack::QueryMissing);
    }

    let related_limit = options.related_page_limit.max(1);
    let chunk_limit = options.chunk_limit.max(1);
    let token_budget = options.token_budget.max(1);
    let max_pages = options.max_pages.max(1);
    let link_limit = options.link_limit.max(1);
    let category_limit = options.category_limit.max(1);
    let template_limit = options.template_limit.max(1);

    let query_terms = expand_authoring_query_terms(&topic, &stub_link_titles);
    if query_terms.is_empty() {
        return Ok(AuthoringKnowledgePack::QueryMissing);
    }
    let query = query_terms[0].clone();
    let template_page_weights = build_template_match_score_map(&connection, &stub_template_titles)?;
    let semantic_page_hits = load_semantic_page_hits(&connection, &query_terms, related_limit)?;
    let authority_page_hits =
        load_reference_authority_page_hits(&connection, &query_terms, related_limit)?;
    let identifier_page_hits =
        load_reference_identifier_page_hits(&connection, &query_terms, related_limit)?;
    let semantic_page_weights = build_semantic_page_weight_map(&semantic_page_hits);
    let authority_page_weights = build_authority_page_weight_map(&authority_page_hits);
    let identifier_page_weights = build_identifier_page_weight_map(&identifier_page_hits);

    let related_pages = collect_related_pages_for_authoring(
        &connection,
        AuthoringRelatedPageInputs {
            stub_link_titles: &stub_link_titles,
            query_terms: &query_terms,
            limit: related_limit,
            template_page_scores: &template_page_weights,
            semantic_page_hits: &semantic_page_hits,
            authority_page_hits: &authority_page_hits,
            identifier_page_hits: &identifier_page_hits,
        },
    )?;

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
    let related_page_weights = build_related_page_weight_map(&related_pages, &stub_existing_links);
    let chunk_report = retrieve_reranked_chunks_across_pages(
        &connection,
        paths,
        &query,
        &query_terms,
        ChunkRetrievalPlan {
            limit: chunk_limit,
            token_budget,
            max_pages,
            diversify: options.diversify,
        },
        &related_pages
            .iter()
            .map(|page| page.title.clone())
            .collect::<Vec<_>>(),
        ChunkRerankSignals {
            related_page_weights,
            template_page_weights,
            semantic_page_weights,
            authority_page_weights,
            identifier_page_weights,
        },
    )?;
    let mut retrieval_mode = chunk_report.retrieval_mode;
    let chunks = chunk_report.chunks;
    let token_estimate_total = chunk_report.token_estimate_total;

    let mut source_titles = Vec::new();
    let mut seen_source_titles = BTreeSet::new();
    for page in &related_pages {
        if seen_source_titles.insert(page.title.to_ascii_lowercase()) {
            source_titles.push(page.title.clone());
        }
    }
    for chunk in &chunks {
        if seen_source_titles.insert(chunk.source_title.to_ascii_lowercase()) {
            source_titles.push(chunk.source_title.clone());
        }
    }
    for link in &stub_existing_links {
        if seen_source_titles.insert(link.to_ascii_lowercase()) {
            source_titles.push(link.clone());
        }
    }

    let mut suggested_links =
        query_suggested_main_links_for_sources(&connection, &source_titles, link_limit)?;
    let mut seen_suggested_links = suggested_links
        .iter()
        .map(|suggestion| suggestion.title.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    for page in &related_pages {
        if suggested_links.len() >= link_limit {
            break;
        }
        if page.namespace == Namespace::Main.as_str()
            && !page.is_redirect
            && seen_suggested_links.insert(page.title.to_ascii_lowercase())
        {
            suggested_links.push(AuthoringSuggestion {
                title: page.title.clone(),
                support_count: 1,
                evidence_titles: vec![page.title.clone()],
            });
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
            suggested_links.push(AuthoringSuggestion {
                title: chunk.source_title.clone(),
                support_count: 1,
                evidence_titles: vec![chunk.source_title.clone()],
            });
        }
    }
    suggested_links.truncate(link_limit);

    let suggested_categories =
        query_suggested_categories_for_sources(&connection, &source_titles, category_limit)?;
    let suggested_templates =
        summarize_template_usage_for_sources(&connection, Some(&source_titles), template_limit)?;
    let suggested_references = summarize_reference_usage_for_sources(
        &connection,
        &source_titles,
        AUTHORING_REFERENCE_LIMIT,
    )?;
    let suggested_media =
        summarize_media_usage_for_sources(&connection, &source_titles, AUTHORING_MEDIA_LIMIT)?;
    let template_baseline =
        summarize_template_usage_for_sources(&connection, None, template_limit)?;
    let template_reference_titles = collect_authoring_template_reference_titles(
        &stub_detected_templates,
        &suggested_templates,
        &template_baseline,
        AUTHORING_TEMPLATE_REFERENCE_LIMIT,
    );
    let template_references = load_authoring_template_references(
        &connection,
        &template_reference_titles,
        AUTHORING_TEMPLATE_REFERENCE_LIMIT,
    )?;
    let module_patterns = build_authoring_module_patterns(
        &connection,
        &source_titles,
        &template_references,
        AUTHORING_MODULE_PATTERN_LIMIT,
    )?;
    let docs_context = crate::knowledge::docs_bridge::build_authoring_docs_context(
        paths,
        &topic,
        &query_terms,
        &template_references,
        &module_patterns,
        &options.docs_profile,
    )?;
    if !template_references.is_empty() {
        retrieval_mode.push_str("+template-guides");
    }
    if !module_patterns.is_empty() {
        retrieval_mode.push_str("+module-patterns");
    }
    if docs_context.is_some() {
        retrieval_mode.push_str("+docs-bridge");
    }

    let inventory = load_authoring_inventory(&connection)?;
    let pack_token_estimate_total = estimate_authoring_pack_total(AuthoringPackEstimateInputs {
        related_pages: &related_pages,
        suggested_links: &suggested_links,
        suggested_categories: &suggested_categories,
        suggested_templates: &suggested_templates,
        suggested_references: &suggested_references,
        suggested_media: &suggested_media,
        template_baseline: &template_baseline,
        template_references: &template_references,
        module_patterns: &module_patterns,
        docs_context: docs_context.as_ref(),
        stub_detected_templates: &stub_detected_templates,
        chunks: &chunks,
    });

    Ok(AuthoringKnowledgePack::Found(Box::new(
        AuthoringKnowledgePackResult {
            topic,
            query,
            query_terms,
            inventory,
            pack_token_budget: token_budget,
            pack_token_estimate_total,
            related_pages,
            suggested_links,
            suggested_categories,
            suggested_templates,
            suggested_references,
            suggested_media,
            template_baseline,
            template_references,
            module_patterns,
            docs_context,
            stub_existing_links,
            stub_missing_links,
            stub_detected_templates,
            retrieval_mode,
            chunks,
            token_estimate_total,
        },
    )))
}

pub fn query_active_template_catalog(
    paths: &ResolvedPaths,
    limit: usize,
) -> Result<ActiveTemplateCatalogLookup> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(ActiveTemplateCatalogLookup::IndexMissing),
    };
    let active_template_count = if table_exists(&connection, "indexed_template_invocations")? {
        count_query(
            &connection,
            "SELECT COUNT(DISTINCT template_title) FROM indexed_template_invocations",
        )
        .context("failed to count active templates")?
    } else {
        0
    };
    let templates = summarize_template_usage_for_sources(&connection, None, limit.max(1))?;
    Ok(ActiveTemplateCatalogLookup::Found(ActiveTemplateCatalog {
        active_template_count,
        templates,
    }))
}

pub fn query_template_reference(
    paths: &ResolvedPaths,
    template_title: &str,
) -> Result<TemplateReferenceLookup> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(TemplateReferenceLookup::IndexMissing),
    };

    let normalized_template = normalize_template_lookup_title(template_title);
    if normalized_template.is_empty() {
        return Ok(TemplateReferenceLookup::TemplateMissing {
            template_title: template_title.trim().to_string(),
        });
    }

    let Some(reference) =
        load_template_reference_for_connection(&connection, &normalized_template)?
    else {
        return Ok(TemplateReferenceLookup::TemplateMissing {
            template_title: normalized_template,
        });
    };

    Ok(TemplateReferenceLookup::Found(Box::new(reference)))
}

fn load_template_reference_for_connection(
    connection: &Connection,
    template_title: &str,
) -> Result<Option<TemplateReference>> {
    let Some(template) = load_template_usage_summary_for_connection(connection, template_title)?
    else {
        return Ok(None);
    };

    let implementation_records =
        load_template_implementation_pages_for_connection(connection, template_title)?;
    let mut implementation_pages = Vec::new();
    let mut implementation_sections = Vec::new();
    let mut implementation_chunks = Vec::new();
    for page in implementation_records
        .into_iter()
        .take(TEMPLATE_IMPLEMENTATION_PAGE_LIMIT)
    {
        let section_summaries = load_section_records_for_bundle(connection, &page.relative_path)?
            .unwrap_or_default()
            .into_iter()
            .map(|section| LocalSectionSummary {
                section_heading: section.section_heading,
                section_level: section.section_level,
                summary_text: section.summary_text,
                token_estimate: section.token_estimate,
            })
            .collect::<Vec<_>>();
        let context_chunks = load_context_chunks_for_bundle(connection, &page.relative_path, None)?
            .unwrap_or_default();
        implementation_sections.extend(section_summaries.iter().cloned());
        implementation_chunks.extend(context_chunks.iter().cloned());
        implementation_pages.push(TemplateImplementationPage {
            page_title: page.title.clone(),
            namespace: page.namespace.clone(),
            role: page.role,
            summary_text: load_page_summary_for_connection(connection, &page.relative_path)?,
            section_summaries,
            context_chunks,
        });
    }

    Ok(Some(TemplateReference {
        template,
        implementation_pages,
        implementation_sections,
        implementation_chunks,
    }))
}

fn collect_authoring_template_reference_titles(
    stub_detected_templates: &[StubTemplateHint],
    suggested_templates: &[TemplateUsageSummary],
    template_baseline: &[TemplateUsageSummary],
    limit: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    for title in stub_detected_templates
        .iter()
        .map(|template| template.template_title.clone())
        .chain(
            suggested_templates
                .iter()
                .map(|template| template.template_title.clone()),
        )
        .chain(
            template_baseline
                .iter()
                .map(|template| template.template_title.clone()),
        )
    {
        let normalized = normalize_template_lookup_title(&title);
        if normalized.is_empty() || !seen.insert(normalized.to_ascii_lowercase()) {
            continue;
        }
        out.push(normalized);
        if out.len() >= limit {
            break;
        }
    }

    out
}

fn load_authoring_template_references(
    connection: &Connection,
    template_titles: &[String],
    limit: usize,
) -> Result<Vec<TemplateReference>> {
    let mut out = Vec::new();
    for template_title in template_titles.iter().take(limit) {
        if let Some(reference) = load_template_reference_for_connection(connection, template_title)?
        {
            out.push(reference);
        }
    }
    Ok(out)
}

fn build_authoring_module_patterns(
    connection: &Connection,
    source_titles: &[String],
    template_references: &[TemplateReference],
    limit: usize,
) -> Result<Vec<ModuleUsageSummary>> {
    let mut out = summarize_module_usage_for_sources(connection, source_titles, limit)?;
    let mut seen = out
        .iter()
        .map(|summary| summary.module_title.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();

    let referenced_modules = template_references
        .iter()
        .flat_map(|reference| reference.implementation_pages.iter())
        .filter(|page| page.role == "module")
        .map(|page| page.page_title.clone())
        .collect::<Vec<_>>();

    for module_title in referenced_modules {
        if out.len() >= limit {
            break;
        }
        let normalized = normalize_module_lookup_title(&module_title);
        if normalized.is_empty() || !seen.insert(normalized.to_ascii_lowercase()) {
            continue;
        }
        if let Some(summary) = load_module_usage_summary_for_connection(connection, &normalized)? {
            out.push(summary);
        }
    }

    out.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| right.distinct_page_count.cmp(&left.distinct_page_count))
            .then_with(|| left.module_title.cmp(&right.module_title))
    });
    out.truncate(limit);
    Ok(out)
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

fn load_context_chunks_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
    content: Option<&str>,
) -> Result<Option<Vec<LocalContextChunk>>> {
    if table_exists(connection, "indexed_page_chunks")? {
        let db_rows = load_indexed_context_chunks_for_connection(
            connection,
            source_relative_path,
            CONTEXT_CHUNK_LIMIT,
            CONTEXT_TOKEN_BUDGET,
        )?;
        if !db_rows.is_empty() {
            return Ok(Some(db_rows));
        }
    }
    Ok(content.map(fallback_context_chunks_from_content))
}

fn load_template_invocations_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<LocalTemplateInvocation>>> {
    if table_exists(connection, "indexed_template_invocations")? {
        let db_rows = load_indexed_template_invocations_for_connection(
            connection,
            source_relative_path,
            TEMPLATE_INVOCATION_LIMIT,
        )?;
        if !db_rows.is_empty() {
            return Ok(Some(db_rows));
        }
    }
    Ok(None)
}

fn load_references_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<LocalReferenceUsage>>> {
    if !table_exists(connection, "indexed_page_references")? {
        return Ok(None);
    }
    let rows = load_indexed_reference_rows_for_connection(
        connection,
        source_relative_path,
        CONTEXT_REFERENCE_LIMIT,
    )?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rows))
    }
}

fn load_media_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<LocalMediaUsage>>> {
    if !table_exists(connection, "indexed_page_media")? {
        return Ok(None);
    }
    let rows = load_indexed_media_rows_for_connection(
        connection,
        source_relative_path,
        CONTEXT_MEDIA_LIMIT,
    )?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rows))
    }
}

fn load_section_records_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<IndexedSectionRecord>>> {
    if !table_exists(connection, "indexed_page_sections")? {
        return Ok(None);
    }
    let rows = load_indexed_section_rows_for_connection(
        connection,
        source_relative_path,
        AUTHORING_SECTION_LIMIT,
    )?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rows))
    }
}

fn fallback_context_chunks_from_content(content: &str) -> Vec<LocalContextChunk> {
    let fallback_rows = chunk_article_context(content);
    apply_context_chunk_budget(
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
    )
}

fn load_indexed_section_rows_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    limit: usize,
) -> Result<Vec<IndexedSectionRecord>> {
    let limit_i64 = i64::try_from(limit).context("section limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT section_heading, section_level, summary_text, section_text, token_estimate
             FROM indexed_page_sections
             WHERE source_relative_path = ?1
             ORDER BY section_index ASC
             LIMIT ?2",
        )
        .context("failed to prepare indexed_page_sections query")?;
    let rows = statement
        .query_map(params![source_relative_path, limit_i64], |row| {
            let section_level_i64: i64 = row.get(1)?;
            let token_estimate_i64: i64 = row.get(4)?;
            Ok(IndexedSectionRecord {
                section_heading: row.get(0)?,
                section_level: u8::try_from(section_level_i64).unwrap_or(1),
                summary_text: row.get(2)?,
                section_text: row.get(3)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run indexed_page_sections query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode indexed_page_sections row")?);
    }
    Ok(out)
}

fn load_page_content(paths: &ResolvedPaths, source_relative_path: &str) -> Result<String> {
    let absolute = absolute_path_from_relative(paths, source_relative_path);
    validate_scoped_path(paths, &absolute)?;
    fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))
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

    let absolute = absolute_path_from_relative(paths, source_relative_path);
    validate_scoped_path(paths, &absolute)?;
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))?;
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

fn expand_retrieval_query_terms(query: &str) -> Vec<String> {
    let normalized = normalize_spaces(&query.replace('_', " "));
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    push_authoring_query_term(&mut out, &mut seen, &normalized);
    if let Some((_, body)) = normalized.split_once(':') {
        push_authoring_query_term(&mut out, &mut seen, body);
    }
    for token in normalized.split_whitespace() {
        if token.len() >= 4 {
            push_authoring_query_term(&mut out, &mut seen, token);
        }
    }
    out
}

fn collect_chunk_candidates_across_pages(
    connection: &Connection,
    paths: &ResolvedPaths,
    query_terms: &[String],
    candidate_cap: usize,
) -> Result<(Vec<RetrievedChunk>, String)> {
    let mut candidates = Vec::new();
    let mut modes = BTreeSet::new();

    if table_exists(connection, "indexed_page_chunks")? {
        let has_fts = fts_table_exists(connection, "indexed_page_chunks_fts");
        for term in query_terms {
            if has_fts {
                let hits =
                    query_chunks_fts_across_pages_for_connection(connection, term, candidate_cap)?;
                if !hits.is_empty() {
                    modes.insert("fts");
                    candidates.extend(hits);
                    continue;
                }
            }
            let hits =
                query_chunks_like_across_pages_for_connection(connection, term, candidate_cap)?;
            if !hits.is_empty() {
                modes.insert("like");
                candidates.extend(hits);
            }
        }
    } else {
        for term in query_terms {
            let hits = query_chunks_scan_across_pages(paths, term, candidate_cap)?;
            if !hits.is_empty() {
                modes.insert("scan");
                candidates.extend(hits);
            }
        }
    }

    let retrieval_mode = if modes.is_empty() {
        "hybrid-rerank-across".to_string()
    } else {
        format!(
            "hybrid-{}-rerank-across",
            modes.into_iter().collect::<Vec<_>>().join("+")
        )
    };
    Ok((candidates, retrieval_mode))
}

fn build_related_page_weight_map(
    related_pages: &[AuthoringPageCandidate],
    seed_titles: &[String],
) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::<String, usize>::new();
    for page in related_pages {
        out.insert(
            page.title.to_ascii_lowercase(),
            page.retrieval_weight.clamp(1, 240),
        );
    }
    for title in seed_titles {
        out.entry(title.to_ascii_lowercase()).or_insert(160);
    }
    out
}

fn build_template_match_score_map(
    connection: &Connection,
    stub_templates: &[StubTemplateHint],
) -> Result<BTreeMap<String, usize>> {
    if stub_templates.is_empty() || !table_exists(connection, "indexed_template_invocations")? {
        return Ok(BTreeMap::new());
    }

    let mut out = BTreeMap::<String, usize>::new();
    for hint in stub_templates {
        let template_title = normalize_template_lookup_title(&hint.template_title);
        if template_title.is_empty() {
            continue;
        }
        let stub_keys = hint
            .parameter_keys
            .iter()
            .map(|key| normalize_template_parameter_key(key))
            .collect::<BTreeSet<_>>();
        for (source_title, parameter_keys_serialized) in
            load_template_invocation_rows_for_template(connection, &template_title)?
        {
            let page_key = source_title.to_ascii_lowercase();
            let invocation_keys = parse_parameter_key_list(&parameter_keys_serialized)
                .into_iter()
                .map(|key| normalize_template_parameter_key(&key))
                .collect::<BTreeSet<_>>();
            let overlap = if stub_keys.is_empty() {
                0
            } else {
                stub_keys.intersection(&invocation_keys).count()
            };
            let mut score = 72usize;
            if overlap > 0 {
                score = score.saturating_add(overlap.saturating_mul(18));
            }
            if !stub_keys.is_empty() && overlap >= stub_keys.len().min(3) {
                score = score.saturating_add(24);
            }
            let entry = out.entry(page_key).or_insert(0);
            *entry = (*entry).saturating_add(score);
        }
    }
    Ok(out)
}

fn query_page_records_from_reference_authorities_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_reference_authorities")? {
        return Ok(Vec::new());
    }

    let normalized = normalize_spaces(&query.replace('_', " "));
    if normalized.is_empty() {
        return Ok(Vec::new());
    }
    let limit_i64 =
        i64::try_from(limit).context("reference authority query limit does not fit into i64")?;
    if fts_table_exists(connection, "indexed_reference_authorities_fts") {
        let fts_query = format!("\"{}\" *", normalized);
        let mut statement = connection
            .prepare(
                "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
                 FROM indexed_reference_authorities_fts fts
                 JOIN indexed_reference_authorities a ON a.rowid = fts.rowid
                 JOIN indexed_pages p ON p.relative_path = a.source_relative_path
                 WHERE indexed_reference_authorities_fts MATCH ?1
                 GROUP BY p.relative_path
                 ORDER BY COUNT(*) DESC, p.title ASC
                 LIMIT ?2",
            )
            .context("failed to prepare reference authority FTS query")?;
        let rows = statement
            .query_map(params![fts_query, limit_i64], decode_page_record_row)
            .context("failed to run reference authority FTS query")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("failed to decode reference authority FTS row")?);
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    let wildcard = format!("%{normalized}%");
    let prefix = format!("{normalized}%");
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_reference_authorities a
             JOIN indexed_pages p ON p.relative_path = a.source_relative_path
             WHERE lower(a.authority_label) LIKE lower(?1)
                OR lower(a.retrieval_text) LIKE lower(?1)
                OR lower(a.source_family) LIKE lower(?1)
                OR lower(a.source_domain) LIKE lower(?1)
                OR lower(a.source_container) LIKE lower(?1)
                OR lower(a.source_author) LIKE lower(?1)
             GROUP BY p.relative_path
             ORDER BY
               CASE
                 WHEN lower(a.authority_label) = lower(?2) THEN 0
                 WHEN lower(a.authority_label) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               COUNT(*) DESC,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare reference authority LIKE query")?;
    let rows = statement
        .query_map(
            params![wildcard, normalized, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run reference authority LIKE query")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode reference authority LIKE row")?);
    }
    Ok(out)
}

fn load_reference_authority_page_hits(
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
        let base_weight = 200usize
            .saturating_sub(query_index.saturating_mul(16))
            .max(36);
        for (rank, page) in query_page_records_from_reference_authorities_for_connection(
            connection,
            term,
            search_limit,
        )?
        .into_iter()
        .enumerate()
        {
            let key = page.title.to_ascii_lowercase();
            titles.entry(key.clone()).or_insert(page.title);
            let weight = base_weight.saturating_sub(rank.saturating_mul(12)).max(18);
            let entry = weights.entry(key).or_insert(0);
            *entry = entry.saturating_add(weight);
        }
    }

    materialize_page_hits(weights, titles, limit)
}

fn query_page_records_from_reference_identifiers_for_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexedPageRecord>> {
    if limit == 0 || !table_exists(connection, "indexed_reference_identifiers")? {
        return Ok(Vec::new());
    }

    let normalized_query = normalize_reference_identifier_search_term(query);
    if normalized_query.is_empty() {
        return Ok(Vec::new());
    }
    let limit_i64 =
        i64::try_from(limit).context("reference identifier query limit does not fit into i64")?;
    let wildcard = format!("%{normalized_query}%");
    let prefix = format!("{normalized_query}%");
    let mut statement = connection
        .prepare(
            "SELECT p.title, p.namespace, p.is_redirect, p.redirect_target, p.relative_path, p.bytes
             FROM indexed_reference_identifiers i
             JOIN indexed_pages p ON p.relative_path = i.source_relative_path
             WHERE lower(i.normalized_value) = lower(?1)
                OR lower(i.normalized_value) LIKE lower(?2)
                OR lower(i.identifier_value) LIKE lower(?2)
             GROUP BY p.relative_path
             ORDER BY
               CASE
                 WHEN lower(i.normalized_value) = lower(?1) THEN 0
                 WHEN lower(i.normalized_value) LIKE lower(?3) THEN 1
                 ELSE 2
               END,
               COUNT(*) DESC,
               p.title ASC
             LIMIT ?4",
        )
        .context("failed to prepare reference identifier query")?;
    let rows = statement
        .query_map(
            params![normalized_query, wildcard, prefix, limit_i64],
            decode_page_record_row,
        )
        .context("failed to run reference identifier query")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode reference identifier row")?);
    }
    Ok(out)
}

fn load_reference_identifier_page_hits(
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
        let base_weight = 240usize
            .saturating_sub(query_index.saturating_mul(20))
            .max(48);
        for (rank, page) in query_page_records_from_reference_identifiers_for_connection(
            connection,
            term,
            search_limit,
        )?
        .into_iter()
        .enumerate()
        {
            let key = page.title.to_ascii_lowercase();
            titles.entry(key.clone()).or_insert(page.title);
            let weight = base_weight.saturating_sub(rank.saturating_mul(16)).max(24);
            let entry = weights.entry(key).or_insert(0);
            *entry = entry.saturating_add(weight);
        }
    }

    materialize_page_hits(weights, titles, limit)
}

fn normalize_reference_identifier_search_term(query: &str) -> String {
    let normalized = normalize_spaces(&query.replace('_', " "));
    if normalized.is_empty() {
        return String::new();
    }
    if let Some((key, value)) = normalized.split_once(':') {
        let key = normalize_template_parameter_key(key);
        if !key.is_empty() {
            let normalized_value = normalize_reference_identifier_value(&key, value);
            if !normalized_value.is_empty() {
                return normalized_value;
            }
        }
    }

    normalize_reference_identifier_token(&normalized, true)
}

fn load_seed_chunks_for_related_pages(
    connection: &Connection,
    related_page_titles: &[String],
    per_page_limit: usize,
) -> Result<Vec<RetrievedChunk>> {
    if related_page_titles.is_empty()
        || per_page_limit == 0
        || !table_exists(connection, "indexed_page_chunks")?
    {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for title in related_page_titles {
        let Some(page) = load_page_record(connection, title)? else {
            continue;
        };
        let rows = load_indexed_context_chunks_for_connection(
            connection,
            &page.relative_path,
            per_page_limit,
            usize::MAX,
        )?;
        for chunk in rows {
            out.push(RetrievedChunk {
                source_title: page.title.clone(),
                source_namespace: page.namespace.clone(),
                source_relative_path: page.relative_path.clone(),
                section_heading: chunk.section_heading,
                token_estimate: chunk.token_estimate,
                chunk_text: chunk.chunk_text,
            });
        }
    }
    Ok(out)
}

fn section_authoring_bias(section_heading: Option<&str>, chunk_text: &str) -> i64 {
    let heading = section_heading.unwrap_or_default().to_ascii_lowercase();
    let text = chunk_text.to_ascii_lowercase();

    let mut score = if heading.is_empty() { 32 } else { 0 };
    for low_signal in [
        "references",
        "notes",
        "external links",
        "further reading",
        "bibliography",
        "gallery",
        "see also",
    ] {
        if heading.contains(low_signal) {
            score -= 120;
        }
    }
    for high_signal in [
        "history",
        "background",
        "overview",
        "biography",
        "profile",
        "works",
        "career",
        "philosophy",
    ] {
        if heading.contains(high_signal) {
            score += 24;
        }
    }
    if text.contains("{{reflist") || text.contains("[[category:") {
        score -= 120;
    }
    score
}

fn rerank_retrieved_chunks(
    candidates: Vec<RetrievedChunk>,
    query: &str,
    query_terms: &[String],
    signals: &ChunkRerankSignals,
) -> Vec<RetrievedChunk> {
    let normalized_query = query.to_ascii_lowercase();
    let mut deduped = BTreeMap::<String, RetrievedChunk>::new();
    for chunk in candidates {
        let key = format!(
            "{}\u{1f}{}\u{1f}{}",
            chunk.source_relative_path,
            chunk.section_heading.as_deref().unwrap_or_default(),
            chunk.chunk_text
        );
        deduped.entry(key).or_insert(chunk);
    }

    let mut scored = deduped
        .into_values()
        .map(|chunk| {
            let mut score = 0i64;
            let title = chunk.source_title.to_ascii_lowercase();
            let section = chunk
                .section_heading
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let text = chunk.chunk_text.to_ascii_lowercase();

            if !normalized_query.is_empty() {
                if title == normalized_query {
                    score += 220;
                } else if title.contains(&normalized_query) {
                    score += 140;
                }
                if section.contains(&normalized_query) {
                    score += 90;
                }
                if text.contains(&normalized_query) {
                    score += 120;
                }
            }

            let mut coverage = 0usize;
            for (index, term) in query_terms.iter().enumerate() {
                let term = term.to_ascii_lowercase();
                if term.is_empty() {
                    continue;
                }
                let weight = 36usize.saturating_sub(index.saturating_mul(4)).max(8);
                let mut matched = false;
                if title == term {
                    score += i64::try_from(weight.saturating_mul(4)).unwrap_or(0);
                    matched = true;
                } else if title.contains(&term) {
                    score += i64::try_from(weight.saturating_mul(2)).unwrap_or(0);
                    matched = true;
                }
                if section.contains(&term) {
                    score += i64::try_from(weight.saturating_add(24)).unwrap_or(0);
                    matched = true;
                }
                if text.contains(&term) {
                    score += i64::try_from(weight.saturating_add(12)).unwrap_or(0);
                    matched = true;
                }
                if matched {
                    coverage = coverage.saturating_add(1);
                }
            }
            score += i64::try_from(coverage.saturating_mul(28)).unwrap_or(0);
            if !query_terms.is_empty() && coverage >= query_terms.len().min(3) {
                score += 60;
            }
            if chunk.source_namespace == Namespace::Main.as_str() {
                score += 18;
            } else {
                score -= 20;
            }
            score += i64::try_from(
                signals
                    .related_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score += i64::try_from(
                signals
                    .template_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score += i64::try_from(
                signals
                    .authority_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score += i64::try_from(
                signals
                    .identifier_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score += i64::try_from(
                signals
                    .semantic_page_weights
                    .get(&chunk.source_title.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )
            .unwrap_or(0);
            score +=
                i64::try_from(48usize.saturating_sub(chunk.token_estimate.min(48))).unwrap_or(0);
            score += section_authoring_bias(chunk.section_heading.as_deref(), &chunk.chunk_text);
            (score, chunk)
        })
        .collect::<Vec<_>>();

    scored.sort_by(|(left_score, left_chunk), (right_score, right_chunk)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_chunk.source_title.cmp(&right_chunk.source_title))
            .then_with(|| left_chunk.section_heading.cmp(&right_chunk.section_heading))
            .then_with(|| left_chunk.chunk_text.cmp(&right_chunk.chunk_text))
    });
    scored.into_iter().map(|(_, chunk)| chunk).collect()
}

fn retrieve_reranked_chunks_across_pages(
    connection: &Connection,
    paths: &ResolvedPaths,
    query: &str,
    query_terms: &[String],
    plan: ChunkRetrievalPlan,
    related_page_titles: &[String],
    signals: ChunkRerankSignals,
) -> Result<LocalChunkAcrossPagesResult> {
    let max_chunks = plan.limit.max(1);
    let max_tokens = plan.token_budget.max(1);
    let capped_max_pages = plan.max_pages.max(1);
    let candidate_cap = candidate_limit(
        max_chunks.saturating_mul(query_terms.len().max(1)),
        CHUNK_CANDIDATE_MULTIPLIER_ACROSS,
    );
    let (mut candidates, retrieval_mode) =
        collect_chunk_candidates_across_pages(connection, paths, query_terms, candidate_cap)?;
    candidates.extend(load_seed_chunks_for_related_pages(
        connection,
        related_page_titles,
        AUTHORING_SEED_CHUNKS_PER_PAGE,
    )?);
    let reranked = rerank_retrieved_chunks(candidates, query, query_terms, &signals);
    let chunks = select_retrieved_chunks(
        reranked,
        max_chunks,
        max_tokens,
        plan.diversify,
        Some(capped_max_pages),
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

    let mut retrieval_mode = retrieval_mode;
    if !signals.semantic_page_weights.is_empty() {
        retrieval_mode = format!("{retrieval_mode}+semantic");
    }
    if !signals.authority_page_weights.is_empty() {
        retrieval_mode = format!("{retrieval_mode}+authority");
    }
    if !signals.identifier_page_weights.is_empty() {
        retrieval_mode = format!("{retrieval_mode}+identifier");
    }

    Ok(LocalChunkAcrossPagesResult {
        query: query.to_string(),
        retrieval_mode: if related_page_titles.is_empty() {
            retrieval_mode
        } else {
            format!("{retrieval_mode}+seed-pages")
        },
        max_pages: capped_max_pages,
        source_page_count,
        chunks,
        token_estimate_total,
    })
}

fn analyze_stub_hints(stub_content: Option<&str>) -> (Vec<String>, Vec<StubTemplateHint>) {
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

    let mut templates = BTreeMap::<String, BTreeSet<String>>::new();
    for invocation in extract_template_invocations(content) {
        let entry = templates.entry(invocation.template_title).or_default();
        for key in invocation.parameter_keys {
            entry.insert(key);
        }
    }

    (
        links.into_iter().collect(),
        templates
            .into_iter()
            .map(|(template_title, parameter_keys)| StubTemplateHint {
                template_title,
                parameter_keys: parameter_keys.into_iter().collect(),
            })
            .collect(),
    )
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

#[derive(Default)]
struct AuthoringPageAccumulator {
    title: String,
    namespace: String,
    is_redirect: bool,
    relative_path: String,
    score: usize,
    sources: BTreeSet<String>,
}

struct AuthoringRelatedPageInputs<'a> {
    stub_link_titles: &'a [String],
    query_terms: &'a [String],
    limit: usize,
    template_page_scores: &'a BTreeMap<String, usize>,
    semantic_page_hits: &'a [SemanticPageHit],
    authority_page_hits: &'a [SemanticPageHit],
    identifier_page_hits: &'a [SemanticPageHit],
}

fn collect_related_pages_for_authoring(
    connection: &Connection,
    inputs: AuthoringRelatedPageInputs<'_>,
) -> Result<Vec<AuthoringPageCandidate>> {
    let mut candidates = BTreeMap::<String, AuthoringPageAccumulator>::new();
    let search_limit = candidate_limit(inputs.limit.max(1), 2);

    for title in inputs.stub_link_titles {
        let normalized = normalize_query_title(title);
        if normalized.is_empty() {
            continue;
        }
        if let Some(page) = load_page_record(connection, &normalized)? {
            add_authoring_page_candidate(&mut candidates, page, "stub-link", 400);
        }
    }

    let mut ranked_template_matches = inputs.template_page_scores.iter().collect::<Vec<_>>();
    ranked_template_matches.sort_by(|(left_title, left_score), (right_title, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_title.cmp(right_title))
    });
    for (title, score) in ranked_template_matches.into_iter().take(search_limit) {
        if let Some(page) = load_page_record(connection, title)? {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "template-match",
                (*score).clamp(32, 260),
            );
        }
    }

    for semantic_hit in inputs.semantic_page_hits {
        if let Some(page) = load_page_record(connection, &semantic_hit.title)? {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "semantic-profile",
                semantic_hit.retrieval_weight.clamp(24, 260),
            );
        }
    }

    for authority_hit in inputs.authority_page_hits {
        if let Some(page) = load_page_record(connection, &authority_hit.title)? {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "source-authority",
                authority_hit.retrieval_weight.clamp(20, 240),
            );
        }
    }

    for identifier_hit in inputs.identifier_page_hits {
        if let Some(page) = load_page_record(connection, &identifier_hit.title)? {
            add_authoring_page_candidate(
                &mut candidates,
                page,
                "source-identifier",
                identifier_hit.retrieval_weight.clamp(24, 280),
            );
        }
    }

    for (query_index, term) in inputs.query_terms.iter().enumerate() {
        let title_search_score = 240usize.saturating_sub(query_index.saturating_mul(20));
        for (rank, hit) in query_local_search_for_connection(connection, term, search_limit)?
            .into_iter()
            .enumerate()
        {
            let normalized = normalize_query_title(&hit.title);
            if normalized.is_empty() {
                continue;
            }
            if let Some(page) = load_page_record(connection, &normalized)? {
                let score = title_search_score
                    .saturating_sub(rank.saturating_mul(12))
                    .max(24);
                add_authoring_page_candidate(&mut candidates, page, "title-search", score);
            }
        }

        let alias_search_score = 200usize.saturating_sub(query_index.saturating_mul(16));
        for (rank, page) in
            query_page_records_from_aliases_for_connection(connection, term, search_limit)?
                .into_iter()
                .enumerate()
        {
            let score = alias_search_score
                .saturating_sub(rank.saturating_mul(10))
                .max(20);
            add_authoring_page_candidate(&mut candidates, page, "alias-search", score);
        }

        let section_search_score = 160usize.saturating_sub(query_index.saturating_mul(12));
        for (rank, page) in
            query_page_records_from_sections_for_connection(connection, term, search_limit)?
                .into_iter()
                .enumerate()
        {
            let score = section_search_score
                .saturating_sub(rank.saturating_mul(8))
                .max(16);
            add_authoring_page_candidate(&mut candidates, page, "section-search", score);
        }
    }

    let mut ranked = candidates.into_values().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.title.cmp(&right.title))
    });
    ranked.truncate(inputs.limit);

    ranked
        .into_iter()
        .map(|candidate| {
            Ok(AuthoringPageCandidate {
                title: candidate.title,
                namespace: candidate.namespace,
                is_redirect: candidate.is_redirect,
                source: candidate.sources.into_iter().collect::<Vec<_>>().join("+"),
                retrieval_weight: candidate.score,
                summary: load_page_summary_for_connection(connection, &candidate.relative_path)?,
            })
        })
        .collect()
}

fn add_authoring_page_candidate(
    candidates: &mut BTreeMap<String, AuthoringPageAccumulator>,
    page: IndexedPageRecord,
    source: &str,
    score: usize,
) {
    let key = page.title.to_ascii_lowercase();
    let entry = candidates.entry(key).or_default();
    if entry.title.is_empty() {
        entry.title = page.title;
        entry.namespace = page.namespace;
        entry.is_redirect = page.is_redirect;
        entry.relative_path = page.relative_path;
    }
    entry.score = entry.score.saturating_add(score);
    entry.sources.insert(source.to_string());
}

fn expand_authoring_query_terms(topic: &str, stub_link_titles: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    push_authoring_query_term(&mut out, &mut seen, topic);
    if let Some((_, body)) = topic.split_once(':') {
        push_authoring_query_term(&mut out, &mut seen, body);
    }
    for token in normalize_spaces(&topic.replace('_', " ")).split_whitespace() {
        if token.len() >= 4 {
            push_authoring_query_term(&mut out, &mut seen, token);
        }
    }
    for title in stub_link_titles {
        if out.len() >= AUTHORING_QUERY_EXPANSION_LIMIT {
            break;
        }
        push_authoring_query_term(&mut out, &mut seen, title);
        if let Some((_, body)) = title.split_once(':') {
            push_authoring_query_term(&mut out, &mut seen, body);
        }
    }

    out
}

fn push_authoring_query_term(out: &mut Vec<String>, seen: &mut BTreeSet<String>, value: &str) {
    if out.len() >= AUTHORING_QUERY_EXPANSION_LIMIT {
        return;
    }
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return;
    }
    let key = normalized.to_ascii_lowercase();
    if !seen.insert(key) {
        return;
    }
    out.push(normalized);
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

#[derive(Default)]
struct SuggestionAccumulator {
    evidence_titles: BTreeSet<String>,
}

fn query_suggested_main_links_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<AuthoringSuggestion>> {
    query_suggestions_for_sources(
        connection,
        source_titles,
        limit,
        false,
        Some(Namespace::Main.as_str()),
    )
}

fn query_suggested_categories_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<AuthoringSuggestion>> {
    query_suggestions_for_sources(connection, source_titles, limit, true, None)
}

fn query_suggestions_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
    category_membership: bool,
    target_namespace: Option<&str>,
) -> Result<Vec<AuthoringSuggestion>> {
    if source_titles.is_empty() || limit == 0 || !table_exists(connection, "indexed_links")? {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "SELECT target_title, source_title
         FROM indexed_links
         WHERE source_title IN ({placeholders})
           AND is_category_membership = ?"
    );
    if target_namespace.is_some() {
        sql.push_str(" AND target_namespace = ?");
    }
    sql.push_str(" ORDER BY target_title ASC, source_title ASC");

    let mut values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    values.push(rusqlite::types::Value::from(if category_membership {
        1i64
    } else {
        0i64
    }));
    if let Some(namespace) = target_namespace {
        values.push(rusqlite::types::Value::from(namespace.to_string()));
    }

    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare suggestion query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .context("failed to run suggestion query")?;

    let mut accumulators = BTreeMap::<String, SuggestionAccumulator>::new();
    for row in rows {
        let (target_title, source_title) = row.context("failed to decode suggestion row")?;
        accumulators
            .entry(target_title)
            .or_default()
            .evidence_titles
            .insert(source_title);
    }

    let mut out = accumulators
        .into_iter()
        .map(|(title, accumulator)| AuthoringSuggestion {
            support_count: accumulator.evidence_titles.len(),
            evidence_titles: accumulator
                .evidence_titles
                .into_iter()
                .take(AUTHORING_SUGGESTION_EVIDENCE_LIMIT)
                .collect(),
            title,
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| {
        right
            .support_count
            .cmp(&left.support_count)
            .then_with(|| left.title.cmp(&right.title))
    });
    out.truncate(limit);
    Ok(out)
}

#[derive(Default)]
struct TemplateUsageAccumulator {
    usage_count: usize,
    source_pages: BTreeSet<String>,
    parameter_key_counts: BTreeMap<String, usize>,
}

#[derive(Default)]
struct ModuleUsageAccumulator {
    usage_count: usize,
    source_pages: BTreeSet<String>,
    function_counts: BTreeMap<String, usize>,
    function_parameter_examples: BTreeMap<String, BTreeSet<String>>,
    examples: Vec<ModuleInvocationExample>,
}

#[derive(Debug, Clone)]
struct IndexedModuleInvocationRow {
    source_title: String,
    source_relative_path: String,
    module_title: String,
    function_name: String,
    parameter_keys: Vec<String>,
    invocation_wikitext: String,
    token_estimate: usize,
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
    for (template_title, source_title, parameter_keys_serialized) in rows {
        let entry = template_map.entry(template_title).or_default();
        entry.usage_count = entry.usage_count.saturating_add(1);
        entry.source_pages.insert(source_title);
        for key in parse_parameter_key_list(&parameter_keys_serialized) {
            let count = entry.parameter_key_counts.entry(key).or_insert(0);
            *count = count.saturating_add(1);
        }
    }

    let mut ranked = template_map.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_title, left_acc), (right_title, right_acc)| {
        right_acc
            .usage_count
            .cmp(&left_acc.usage_count)
            .then_with(|| {
                right_acc
                    .source_pages
                    .len()
                    .cmp(&left_acc.source_pages.len())
            })
            .then_with(|| left_title.cmp(right_title))
    });
    ranked.truncate(limit);

    ranked
        .into_iter()
        .map(|(template_title, accumulator)| {
            materialize_template_usage_summary(connection, template_title, accumulator)
        })
        .collect()
}

fn load_template_invocation_rows_for_sources(
    connection: &Connection,
    source_titles: Option<&[String]>,
) -> Result<Vec<(String, String, String)>> {
    let (sql, values) = if let Some(source_titles) = source_titles {
        let placeholders = std::iter::repeat_n("?", source_titles.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT template_title, source_title, parameter_keys
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
            "SELECT template_title, source_title, parameter_keys FROM indexed_template_invocations"
                .to_string(),
            Vec::new(),
        )
    };

    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare template invocation summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .context("failed to run template invocation summary query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode template invocation summary row")?);
    }
    Ok(out)
}

fn summarize_module_usage_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<ModuleUsageSummary>> {
    if limit == 0
        || source_titles.is_empty()
        || !table_exists(connection, "indexed_module_invocations")?
    {
        return Ok(Vec::new());
    }

    let rows = load_module_invocation_rows_for_sources(connection, source_titles)?;
    let mut grouped = BTreeMap::<String, ModuleUsageAccumulator>::new();
    for row in rows {
        let entry = grouped.entry(row.module_title.clone()).or_default();
        entry.usage_count = entry.usage_count.saturating_add(1);
        entry.source_pages.insert(row.source_title.clone());
        let function_count = entry
            .function_counts
            .entry(row.function_name.clone())
            .or_insert(0);
        *function_count = function_count.saturating_add(1);
        let function_examples = entry
            .function_parameter_examples
            .entry(row.function_name.clone())
            .or_default();
        for key in &row.parameter_keys {
            function_examples.insert(key.clone());
        }
        entry.examples.push(ModuleInvocationExample {
            source_title: row.source_title,
            source_relative_path: row.source_relative_path,
            function_name: row.function_name,
            parameter_keys: row.parameter_keys,
            invocation_text: row.invocation_wikitext,
            token_estimate: row.token_estimate,
        });
    }

    let mut ranked = grouped.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_title, left_acc), (right_title, right_acc)| {
        right_acc
            .usage_count
            .cmp(&left_acc.usage_count)
            .then_with(|| {
                right_acc
                    .source_pages
                    .len()
                    .cmp(&left_acc.source_pages.len())
            })
            .then_with(|| left_title.cmp(right_title))
    });
    ranked.truncate(limit);

    Ok(ranked
        .into_iter()
        .map(|(module_title, accumulator)| {
            materialize_module_usage_summary(module_title, accumulator)
        })
        .collect())
}

fn load_module_usage_summary_for_connection(
    connection: &Connection,
    module_title: &str,
) -> Result<Option<ModuleUsageSummary>> {
    if !table_exists(connection, "indexed_module_invocations")? {
        return Ok(None);
    }

    let rows = load_module_invocation_rows_for_module(connection, module_title)?;
    if rows.is_empty() {
        return Ok(None);
    }

    let mut accumulator = ModuleUsageAccumulator::default();
    for row in rows {
        accumulator.usage_count = accumulator.usage_count.saturating_add(1);
        accumulator.source_pages.insert(row.source_title.clone());
        let function_count = accumulator
            .function_counts
            .entry(row.function_name.clone())
            .or_insert(0);
        *function_count = function_count.saturating_add(1);
        let function_examples = accumulator
            .function_parameter_examples
            .entry(row.function_name.clone())
            .or_default();
        for key in &row.parameter_keys {
            function_examples.insert(key.clone());
        }
        accumulator.examples.push(ModuleInvocationExample {
            source_title: row.source_title,
            source_relative_path: row.source_relative_path,
            function_name: row.function_name,
            parameter_keys: row.parameter_keys,
            invocation_text: row.invocation_wikitext,
            token_estimate: row.token_estimate,
        });
    }

    Ok(Some(materialize_module_usage_summary(
        normalize_module_lookup_title(module_title),
        accumulator,
    )))
}

fn load_module_invocation_rows_for_sources(
    connection: &Connection,
    source_titles: &[String],
) -> Result<Vec<IndexedModuleInvocationRow>> {
    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT source_title, source_relative_path, module_title, function_name, parameter_keys,
                invocation_wikitext, token_estimate
         FROM indexed_module_invocations
         WHERE source_title IN ({placeholders})"
    );
    let values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare module invocation summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            let token_estimate_i64: i64 = row.get(6)?;
            Ok(IndexedModuleInvocationRow {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                module_title: row.get(2)?,
                function_name: row.get(3)?,
                parameter_keys: parse_parameter_key_list(&row.get::<_, String>(4)?),
                invocation_wikitext: row.get(5)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run module invocation summary query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode module invocation summary row")?);
    }
    Ok(out)
}

fn load_module_invocation_rows_for_module(
    connection: &Connection,
    module_title: &str,
) -> Result<Vec<IndexedModuleInvocationRow>> {
    let normalized = normalize_module_lookup_title(module_title);
    if normalized.is_empty() {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            "SELECT source_title, source_relative_path, module_title, function_name, parameter_keys,
                    invocation_wikitext, token_estimate
             FROM indexed_module_invocations
             WHERE lower(module_title) = lower(?1)",
        )
        .context("failed to prepare module invocation lookup query")?;
    let rows = statement
        .query_map([normalized], |row| {
            let token_estimate_i64: i64 = row.get(6)?;
            Ok(IndexedModuleInvocationRow {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                module_title: row.get(2)?,
                function_name: row.get(3)?,
                parameter_keys: parse_parameter_key_list(&row.get::<_, String>(4)?),
                invocation_wikitext: row.get(5)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run module invocation lookup query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode module invocation lookup row")?);
    }
    Ok(out)
}

fn materialize_module_usage_summary(
    module_title: String,
    mut accumulator: ModuleUsageAccumulator,
) -> ModuleUsageSummary {
    let mut function_stats = accumulator
        .function_counts
        .into_iter()
        .map(|(function_name, usage_count)| ModuleFunctionUsage {
            example_parameter_keys: accumulator
                .function_parameter_examples
                .remove(&function_name)
                .unwrap_or_default()
                .into_iter()
                .take(AUTHORING_TEMPLATE_KEY_LIMIT)
                .collect(),
            function_name,
            usage_count,
        })
        .collect::<Vec<_>>();
    function_stats.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| left.function_name.cmp(&right.function_name))
    });
    function_stats.truncate(AUTHORING_MODULE_FUNCTION_LIMIT);

    accumulator.examples.sort_by(|left, right| {
        left.token_estimate
            .cmp(&right.token_estimate)
            .then_with(|| left.source_title.cmp(&right.source_title))
            .then_with(|| left.function_name.cmp(&right.function_name))
    });
    accumulator
        .examples
        .truncate(MODULE_REFERENCE_EXAMPLE_LIMIT);

    ModuleUsageSummary {
        module_title,
        usage_count: accumulator.usage_count,
        distinct_page_count: accumulator.source_pages.len(),
        function_stats,
        example_pages: accumulator
            .source_pages
            .iter()
            .take(MODULE_REFERENCE_EXAMPLE_LIMIT)
            .cloned()
            .collect(),
        example_invocations: accumulator.examples,
    }
}

fn load_template_usage_summary_for_connection(
    connection: &Connection,
    template_title: &str,
) -> Result<Option<TemplateUsageSummary>> {
    if !table_exists(connection, "indexed_template_invocations")? {
        return Ok(None);
    }

    let rows = load_template_invocation_rows_for_template(connection, template_title)?;
    if rows.is_empty() {
        return Ok(None);
    }

    let mut accumulator = TemplateUsageAccumulator::default();
    for (source_title, parameter_keys_serialized) in rows {
        accumulator.usage_count = accumulator.usage_count.saturating_add(1);
        accumulator.source_pages.insert(source_title);
        for key in parse_parameter_key_list(&parameter_keys_serialized) {
            let count = accumulator.parameter_key_counts.entry(key).or_insert(0);
            *count = count.saturating_add(1);
        }
    }

    materialize_template_usage_summary(connection, template_title.to_string(), accumulator)
        .map(Some)
}

fn load_template_invocation_rows_for_template(
    connection: &Connection,
    template_title: &str,
) -> Result<Vec<(String, String)>> {
    let mut statement = connection
        .prepare(
            "SELECT source_title, parameter_keys
             FROM indexed_template_invocations
             WHERE lower(template_title) = lower(?1)",
        )
        .context("failed to prepare template invocation lookup query")?;
    let rows = statement
        .query_map([template_title], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .context("failed to run template invocation lookup query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode template invocation lookup row")?);
    }
    Ok(out)
}

fn materialize_template_usage_summary(
    connection: &Connection,
    template_title: String,
    accumulator: TemplateUsageAccumulator,
) -> Result<TemplateUsageSummary> {
    let example_invocations = load_template_examples_for_connection(
        connection,
        &template_title,
        TEMPLATE_REFERENCE_EXAMPLE_LIMIT,
    )?;
    let parameter_examples = collect_template_parameter_value_examples(
        &example_invocations,
        TEMPLATE_PARAMETER_VALUE_LIMIT,
    );
    let mut parameter_stats = accumulator
        .parameter_key_counts
        .into_iter()
        .map(|(key, usage_count)| TemplateParameterUsage {
            example_values: parameter_examples.get(&key).cloned().unwrap_or_default(),
            key,
            usage_count,
        })
        .collect::<Vec<_>>();
    parameter_stats.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| left.key.cmp(&right.key))
    });
    parameter_stats.truncate(AUTHORING_TEMPLATE_KEY_LIMIT);

    Ok(TemplateUsageSummary {
        aliases: load_template_aliases(connection, &template_title)?,
        usage_count: accumulator.usage_count,
        distinct_page_count: accumulator.source_pages.len(),
        parameter_stats,
        example_pages: accumulator
            .source_pages
            .iter()
            .take(TEMPLATE_REFERENCE_EXAMPLE_LIMIT)
            .cloned()
            .collect(),
        implementation_titles: load_template_implementation_titles(connection, &template_title)?,
        implementation_preview: load_template_implementation_preview(connection, &template_title)?,
        example_invocations,
        template_title,
    })
}

fn normalize_template_lookup_title(value: &str) -> String {
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return String::new();
    }
    canonical_template_title(&normalized).unwrap_or_else(|| normalize_query_title(&normalized))
}

fn normalize_module_lookup_title(value: &str) -> String {
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return String::new();
    }
    if normalized
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Module:"))
    {
        return normalize_query_title(&normalized);
    }
    format!("Module:{normalized}")
}

fn load_page_summary_for_connection(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<String> {
    if table_exists(connection, "indexed_page_sections")? {
        let mut statement = connection
            .prepare(
                "SELECT summary_text
                 FROM indexed_page_sections
                 WHERE source_relative_path = ?1
                 ORDER BY section_index ASC
                 LIMIT 1",
            )
            .context("failed to prepare page summary query")?;
        let summary = statement
            .query_row([source_relative_path], |row| row.get::<_, String>(0))
            .optional()
            .context("failed to run page summary query")?;
        if let Some(summary) = summary {
            let normalized = normalize_spaces(&summary);
            if !normalized.is_empty() {
                return Ok(normalized);
            }
        }
    }

    if table_exists(connection, "indexed_page_chunks")? {
        let mut statement = connection
            .prepare(
                "SELECT chunk_text
                 FROM indexed_page_chunks
                 WHERE source_relative_path = ?1
                 ORDER BY chunk_index ASC
                 LIMIT 1",
            )
            .context("failed to prepare page chunk summary query")?;
        let chunk = statement
            .query_row([source_relative_path], |row| row.get::<_, String>(0))
            .optional()
            .context("failed to run page chunk summary query")?;
        if let Some(chunk) = chunk {
            let normalized = summarize_words(&chunk, AUTHORING_PAGE_SUMMARY_WORD_LIMIT);
            if !normalized.is_empty() {
                return Ok(normalized);
            }
        }
    }

    Ok(String::new())
}

fn load_template_aliases(connection: &Connection, template_title: &str) -> Result<Vec<String>> {
    if !table_exists(connection, "indexed_page_aliases")? {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            "SELECT alias_title
             FROM indexed_page_aliases
             WHERE lower(canonical_title) = lower(?1)
               AND canonical_namespace = 'Template'
             ORDER BY alias_title ASC",
        )
        .context("failed to prepare template alias query")?;
    let rows = statement
        .query_map([template_title], |row| row.get::<_, String>(0))
        .context("failed to run template alias query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode template alias row")?);
    }
    Ok(out)
}

fn load_template_implementation_preview(
    connection: &Connection,
    template_title: &str,
) -> Result<Option<String>> {
    let Some(page) = load_page_record(connection, template_title)? else {
        return Ok(None);
    };
    let summary = load_page_summary_for_connection(connection, &page.relative_path)?;
    if summary.is_empty() {
        Ok(None)
    } else {
        Ok(Some(summary))
    }
}

#[derive(Debug, Clone)]
struct TemplateImplementationRecord {
    title: String,
    namespace: String,
    relative_path: String,
    role: String,
}

fn load_template_implementation_titles(
    connection: &Connection,
    template_title: &str,
) -> Result<Vec<String>> {
    Ok(
        load_template_implementation_pages_for_connection(connection, template_title)?
            .into_iter()
            .map(|page| page.title)
            .collect(),
    )
}

fn load_template_implementation_pages_for_connection(
    connection: &Connection,
    template_title: &str,
) -> Result<Vec<TemplateImplementationRecord>> {
    if table_exists(connection, "indexed_template_implementation_pages")? {
        let mut statement = connection
            .prepare(
                "SELECT implementation_page_title, implementation_namespace, source_relative_path, role
                 FROM indexed_template_implementation_pages
                 WHERE lower(template_title) = lower(?1)
                 ORDER BY
                   CASE role
                     WHEN 'template' THEN 0
                     WHEN 'documentation' THEN 1
                     WHEN 'module' THEN 2
                     ELSE 3
                   END,
                   implementation_page_title ASC",
            )
            .context("failed to prepare template implementation page query")?;
        let rows = statement
            .query_map([template_title], |row| {
                Ok(TemplateImplementationRecord {
                    title: row.get(0)?,
                    namespace: row.get(1)?,
                    relative_path: row.get(2)?,
                    role: row.get(3)?,
                })
            })
            .context("failed to run template implementation page query")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("failed to decode template implementation page row")?);
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    let Some(page) = load_page_record(connection, template_title)? else {
        return Ok(Vec::new());
    };
    Ok(vec![TemplateImplementationRecord {
        title: page.title,
        namespace: page.namespace,
        relative_path: page.relative_path,
        role: "template".to_string(),
    }])
}

fn load_template_examples_for_connection(
    connection: &Connection,
    template_title: &str,
    limit: usize,
) -> Result<Vec<TemplateInvocationExample>> {
    if limit == 0 || !table_exists(connection, "indexed_template_examples")? {
        return Ok(Vec::new());
    }

    let limit_i64 = i64::try_from(limit).context("template example limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT source_title, source_relative_path, parameter_keys, example_wikitext, token_estimate
             FROM indexed_template_examples
             WHERE lower(template_title) = lower(?1)
             ORDER BY token_estimate ASC, source_title ASC, invocation_index ASC
             LIMIT ?2",
        )
        .context("failed to prepare template example query")?;
    let rows = statement
        .query_map(params![template_title, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(4)?;
            Ok(TemplateInvocationExample {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                parameter_keys: parse_parameter_key_list(&row.get::<_, String>(2)?),
                invocation_text: row.get(3)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run template example query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode template example row")?);
    }
    Ok(out)
}

fn collect_template_parameter_value_examples(
    examples: &[TemplateInvocationExample],
    per_key_limit: usize,
) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::<String, Vec<String>>::new();
    let mut seen = BTreeMap::<String, BTreeSet<String>>::new();
    for example in examples {
        for (key, value) in parse_template_parameter_examples(&example.invocation_text) {
            let set = seen.entry(key.clone()).or_default();
            if !set.insert(value.clone()) {
                continue;
            }
            let entry = out.entry(key).or_default();
            if entry.len() < per_key_limit {
                entry.push(value);
            }
        }
    }
    out
}

fn parse_template_parameter_examples(invocation_text: &str) -> Vec<(String, String)> {
    let Some(inner) = invocation_text
        .strip_prefix("{{")
        .and_then(|value| value.strip_suffix("}}"))
    else {
        return Vec::new();
    };
    let segments = split_template_segments(inner);
    let mut out = Vec::new();
    let mut positional_index = 1usize;
    for segment in segments.into_iter().skip(1) {
        let value = segment.trim();
        if value.is_empty() {
            continue;
        }
        let (key, raw_value) = if let Some((key, value)) = split_once_top_level_equals(value) {
            let key = normalize_template_parameter_key(&key);
            if key.is_empty() {
                continue;
            }
            (key, value)
        } else {
            let key = format!("${positional_index}");
            positional_index += 1;
            (key, value.to_string())
        };
        let sample_value = summarize_words(&flatten_markup_excerpt(&raw_value), 8);
        if sample_value.is_empty() {
            continue;
        }
        out.push((key, sample_value));
    }
    out
}

#[derive(Default)]
struct ReferenceUsageAccumulator {
    usage_count: usize,
    source_pages: BTreeSet<String>,
    template_counts: BTreeMap<String, usize>,
    link_counts: BTreeMap<String, usize>,
    domain_counts: BTreeMap<String, usize>,
    authority_counts: BTreeMap<String, usize>,
    identifier_counts: BTreeMap<String, usize>,
    identifier_entry_counts: BTreeMap<String, usize>,
    retrieval_signal_counts: BTreeMap<String, usize>,
    citation_family: String,
    source_type: String,
    source_origin: String,
    source_family: String,
    examples: Vec<ReferenceUsageExample>,
}

#[derive(Debug, Clone)]
struct IndexedReferenceUsageRow {
    source_title: String,
    source_relative_path: String,
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

fn summarize_reference_usage_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<ReferenceUsageSummary>> {
    if limit == 0
        || source_titles.is_empty()
        || !table_exists(connection, "indexed_page_references")?
    {
        return Ok(Vec::new());
    }

    let rows = load_reference_rows_for_sources(connection, source_titles)?;
    let mut grouped = BTreeMap::<String, ReferenceUsageAccumulator>::new();
    for row in rows {
        let example = ReferenceUsageExample {
            source_title: row.source_title.clone(),
            source_relative_path: row.source_relative_path.clone(),
            section_heading: row.section_heading.clone(),
            reference_name: row.reference_name.clone(),
            reference_group: row.reference_group.clone(),
            citation_family: row.citation_family.clone(),
            primary_template_title: row.primary_template_title.clone(),
            source_type: row.source_type.clone(),
            source_origin: row.source_origin.clone(),
            source_family: row.source_family.clone(),
            authority_kind: row.authority_kind.clone(),
            source_authority: row.source_authority.clone(),
            reference_title: row.reference_title.clone(),
            source_container: row.source_container.clone(),
            source_author: row.source_author.clone(),
            source_domain: row.source_domain.clone(),
            source_date: row.source_date.clone(),
            canonical_url: row.canonical_url.clone(),
            identifier_keys: row.identifier_keys.clone(),
            identifier_entries: row.identifier_entries.clone(),
            source_urls: row.source_urls.clone(),
            retrieval_signals: row.retrieval_signals.clone(),
            summary_text: row.summary_text.clone(),
            template_titles: row.template_titles.clone(),
            link_titles: row.link_titles.clone(),
            reference_wikitext: row.reference_wikitext.clone(),
            token_estimate: row.token_estimate,
        };
        let entry = grouped.entry(row.citation_profile.clone()).or_default();
        if entry.citation_family.is_empty() {
            entry.citation_family = row.citation_family.clone();
        }
        if entry.source_type.is_empty() {
            entry.source_type = row.source_type.clone();
        }
        if entry.source_origin.is_empty() {
            entry.source_origin = row.source_origin.clone();
        }
        if entry.source_family.is_empty() {
            entry.source_family = row.source_family.clone();
        }
        entry.usage_count = entry.usage_count.saturating_add(1);
        entry.source_pages.insert(row.source_title);
        for template_title in row.template_titles {
            let count = entry.template_counts.entry(template_title).or_insert(0);
            *count = count.saturating_add(1);
        }
        for link_title in row.link_titles {
            let count = entry.link_counts.entry(link_title).or_insert(0);
            *count = count.saturating_add(1);
        }
        if !row.source_domain.is_empty() {
            let count = entry.domain_counts.entry(row.source_domain).or_insert(0);
            *count = count.saturating_add(1);
        }
        if !row.source_authority.is_empty() {
            let count = entry
                .authority_counts
                .entry(row.source_authority)
                .or_insert(0);
            *count = count.saturating_add(1);
        }
        for identifier in row.identifier_keys {
            let count = entry.identifier_counts.entry(identifier).or_insert(0);
            *count = count.saturating_add(1);
        }
        for identifier_entry in row.identifier_entries {
            let count = entry
                .identifier_entry_counts
                .entry(identifier_entry)
                .or_insert(0);
            *count = count.saturating_add(1);
        }
        for retrieval_signal in row.retrieval_signals {
            let count = entry
                .retrieval_signal_counts
                .entry(retrieval_signal)
                .or_insert(0);
            *count = count.saturating_add(1);
        }
        entry.examples.push(example);
    }

    let mut ranked = grouped.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_profile, left), (right_profile, right)| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| right.source_pages.len().cmp(&left.source_pages.len()))
            .then_with(|| {
                reference_example_rank_key(right.examples.first())
                    .cmp(&reference_example_rank_key(left.examples.first()))
            })
            .then_with(|| left_profile.cmp(right_profile))
    });
    ranked.truncate(limit);

    ranked
        .into_iter()
        .map(|(citation_profile, mut accumulator)| {
            accumulator.examples.sort_by(|left, right| {
                reference_example_rank_key(Some(right))
                    .cmp(&reference_example_rank_key(Some(left)))
                    .then_with(|| left.token_estimate.cmp(&right.token_estimate))
                    .then_with(|| left.source_title.cmp(&right.source_title))
            });
            accumulator
                .examples
                .truncate(AUTHORING_REFERENCE_EXAMPLE_LIMIT);
            Ok(ReferenceUsageSummary {
                citation_profile,
                citation_family: accumulator.citation_family,
                source_type: accumulator.source_type,
                source_origin: accumulator.source_origin,
                source_family: accumulator.source_family,
                usage_count: accumulator.usage_count,
                distinct_page_count: accumulator.source_pages.len(),
                example_pages: accumulator
                    .source_pages
                    .iter()
                    .take(AUTHORING_REFERENCE_EXAMPLE_LIMIT)
                    .cloned()
                    .collect(),
                common_templates: top_counted_keys(
                    &accumulator.template_counts,
                    AUTHORING_TEMPLATE_KEY_LIMIT,
                ),
                common_links: top_counted_keys(
                    &accumulator.link_counts,
                    AUTHORING_TEMPLATE_KEY_LIMIT,
                ),
                common_domains: top_counted_keys(
                    &accumulator.domain_counts,
                    AUTHORING_REFERENCE_DOMAIN_LIMIT,
                ),
                common_authorities: top_counted_keys(
                    &accumulator.authority_counts,
                    AUTHORING_REFERENCE_AUTHORITY_LIMIT,
                ),
                common_identifier_keys: top_counted_keys(
                    &accumulator.identifier_counts,
                    AUTHORING_REFERENCE_IDENTIFIER_LIMIT,
                ),
                common_identifier_entries: top_counted_keys(
                    &accumulator.identifier_entry_counts,
                    AUTHORING_REFERENCE_IDENTIFIER_LIMIT,
                ),
                common_retrieval_signals: top_counted_keys(
                    &accumulator.retrieval_signal_counts,
                    AUTHORING_REFERENCE_FLAG_LIMIT,
                ),
                example_references: accumulator.examples,
            })
        })
        .collect()
}

fn load_reference_rows_for_sources(
    connection: &Connection,
    source_titles: &[String],
) -> Result<Vec<IndexedReferenceUsageRow>> {
    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT source_title, source_relative_path, section_heading, reference_name, reference_group,
                citation_profile, citation_family, primary_template_title, source_type, source_origin,
                source_family, authority_kind, source_authority, reference_title, source_container,
                source_author, source_domain, source_date, canonical_url, identifier_keys,
                identifier_entries, source_urls, retrieval_signals, summary_text, reference_wikitext,
                template_titles, link_titles, token_estimate
         FROM indexed_page_references
         WHERE source_title IN ({placeholders})"
    );
    let values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare reference summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            let token_estimate_i64: i64 = row.get(27)?;
            Ok(IndexedReferenceUsageRow {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                section_heading: row.get(2)?,
                reference_name: row.get(3)?,
                reference_group: row.get(4)?,
                citation_profile: row.get(5)?,
                citation_family: row.get(6)?,
                primary_template_title: normalize_non_empty_string(row.get::<_, String>(7)?),
                source_type: row.get(8)?,
                source_origin: row.get(9)?,
                source_family: row.get(10)?,
                authority_kind: row.get(11)?,
                source_authority: row.get(12)?,
                reference_title: row.get(13)?,
                source_container: row.get(14)?,
                source_author: row.get(15)?,
                source_domain: row.get(16)?,
                source_date: row.get(17)?,
                canonical_url: row.get(18)?,
                identifier_keys: parse_string_list(&row.get::<_, String>(19)?),
                identifier_entries: parse_string_list(&row.get::<_, String>(20)?),
                source_urls: parse_string_list(&row.get::<_, String>(21)?),
                retrieval_signals: parse_string_list(&row.get::<_, String>(22)?),
                summary_text: row.get(23)?,
                reference_wikitext: row.get(24)?,
                template_titles: parse_string_list(&row.get::<_, String>(25)?),
                link_titles: parse_string_list(&row.get::<_, String>(26)?),
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run reference summary query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode reference summary row")?);
    }
    Ok(out)
}

#[derive(Default)]
struct MediaUsageAccumulator {
    usage_count: usize,
    source_pages: BTreeSet<String>,
    examples: Vec<MediaUsageExample>,
}

#[derive(Debug, Clone)]
struct IndexedMediaUsageRow {
    source_title: String,
    source_relative_path: String,
    section_heading: Option<String>,
    file_title: String,
    media_kind: String,
    caption_text: String,
    options: Vec<String>,
    token_estimate: usize,
}

fn summarize_media_usage_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<MediaUsageSummary>> {
    if limit == 0 || source_titles.is_empty() || !table_exists(connection, "indexed_page_media")? {
        return Ok(Vec::new());
    }

    let rows = load_media_rows_for_sources(connection, source_titles)?;
    let mut grouped = BTreeMap::<String, MediaUsageAccumulator>::new();
    let mut file_titles = BTreeMap::<String, String>::new();
    let mut media_kinds = BTreeMap::<String, String>::new();
    for row in rows {
        let key = format!("{}\u{1f}{}", row.file_title, row.media_kind);
        file_titles.insert(key.clone(), row.file_title.clone());
        media_kinds.insert(key.clone(), row.media_kind.clone());
        let entry = grouped.entry(key).or_default();
        entry.usage_count = entry.usage_count.saturating_add(1);
        entry.source_pages.insert(row.source_title.clone());
        entry.examples.push(MediaUsageExample {
            source_title: row.source_title,
            source_relative_path: row.source_relative_path,
            section_heading: row.section_heading,
            caption_text: row.caption_text,
            options: row.options,
            token_estimate: row.token_estimate,
        });
    }

    let mut ranked = grouped.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_key, left), (right_key, right)| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| right.source_pages.len().cmp(&left.source_pages.len()))
            .then_with(|| left_key.cmp(right_key))
    });
    ranked.truncate(limit);

    ranked
        .into_iter()
        .map(|(key, mut accumulator)| {
            accumulator.examples.sort_by(|left, right| {
                let left_has_caption = !left.caption_text.is_empty();
                let right_has_caption = !right.caption_text.is_empty();
                right_has_caption
                    .cmp(&left_has_caption)
                    .then_with(|| left.token_estimate.cmp(&right.token_estimate))
                    .then_with(|| left.source_title.cmp(&right.source_title))
            });
            accumulator.examples.truncate(AUTHORING_MEDIA_EXAMPLE_LIMIT);
            Ok(MediaUsageSummary {
                file_title: file_titles.get(&key).cloned().unwrap_or_default(),
                media_kind: media_kinds
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| "inline".to_string()),
                usage_count: accumulator.usage_count,
                distinct_page_count: accumulator.source_pages.len(),
                example_pages: accumulator
                    .source_pages
                    .iter()
                    .take(AUTHORING_MEDIA_EXAMPLE_LIMIT)
                    .cloned()
                    .collect(),
                example_usages: accumulator.examples,
            })
        })
        .collect()
}

fn load_media_rows_for_sources(
    connection: &Connection,
    source_titles: &[String],
) -> Result<Vec<IndexedMediaUsageRow>> {
    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT source_title, source_relative_path, section_heading, file_title, media_kind,
                caption_text, options_text, token_estimate
         FROM indexed_page_media
         WHERE source_title IN ({placeholders})"
    );
    let values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare media summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            let token_estimate_i64: i64 = row.get(7)?;
            Ok(IndexedMediaUsageRow {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                section_heading: row.get(2)?,
                file_title: row.get(3)?,
                media_kind: row.get(4)?,
                caption_text: row.get(5)?,
                options: parse_string_list(&row.get::<_, String>(6)?),
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run media summary query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode media summary row")?);
    }
    Ok(out)
}

fn top_counted_keys(counts: &BTreeMap<String, usize>, limit: usize) -> Vec<String> {
    let mut ranked = counts
        .iter()
        .map(|(key, count)| (key.clone(), *count))
        .collect::<Vec<_>>();
    ranked.sort_by(|(left_key, left_count), (right_key, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_key.cmp(right_key))
    });
    ranked.into_iter().take(limit).map(|(key, _)| key).collect()
}

fn reference_example_rank_key(
    example: Option<&ReferenceUsageExample>,
) -> (
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
) {
    let Some(example) = example else {
        return (0, 0, 0, 0, 0, 0, 0, 0, 0);
    };
    (
        usize::from(example.primary_template_title.is_some()),
        usize::from(!example.reference_title.is_empty()),
        usize::from(!example.source_author.is_empty()),
        usize::from(!example.source_domain.is_empty()),
        usize::from(!example.source_container.is_empty() || !example.source_authority.is_empty()),
        example
            .identifier_entries
            .len()
            .max(example.identifier_keys.len()),
        example.retrieval_signals.len(),
        example.template_titles.len(),
        example.link_titles.len(),
    )
}

struct AuthoringPackEstimateInputs<'a> {
    related_pages: &'a [AuthoringPageCandidate],
    suggested_links: &'a [AuthoringSuggestion],
    suggested_categories: &'a [AuthoringSuggestion],
    suggested_templates: &'a [TemplateUsageSummary],
    suggested_references: &'a [ReferenceUsageSummary],
    suggested_media: &'a [MediaUsageSummary],
    template_baseline: &'a [TemplateUsageSummary],
    template_references: &'a [TemplateReference],
    module_patterns: &'a [ModuleUsageSummary],
    docs_context: Option<&'a AuthoringDocsContext>,
    stub_detected_templates: &'a [StubTemplateHint],
    chunks: &'a [RetrievedChunk],
}

fn estimate_authoring_pack_total(inputs: AuthoringPackEstimateInputs<'_>) -> usize {
    let page_summary_tokens = inputs
        .related_pages
        .iter()
        .map(|page| estimate_tokens(&page.summary))
        .sum::<usize>();
    let link_tokens = inputs
        .suggested_links
        .iter()
        .map(|suggestion| estimate_tokens(&suggestion.title))
        .sum::<usize>();
    let category_tokens = inputs
        .suggested_categories
        .iter()
        .map(|suggestion| estimate_tokens(&suggestion.title))
        .sum::<usize>();
    let template_tokens = inputs
        .suggested_templates
        .iter()
        .chain(inputs.template_baseline.iter())
        .map(|template| {
            estimate_tokens(&template.template_title)
                + template
                    .parameter_stats
                    .iter()
                    .map(|stat| {
                        estimate_tokens(&stat.key)
                            + stat
                                .example_values
                                .iter()
                                .map(|value| estimate_tokens(value))
                                .sum::<usize>()
                    })
                    .sum::<usize>()
                + template
                    .implementation_titles
                    .iter()
                    .map(|title| estimate_tokens(title))
                    .sum::<usize>()
                + template
                    .implementation_preview
                    .as_deref()
                    .map(estimate_tokens)
                    .unwrap_or(0)
                + template
                    .example_invocations
                    .iter()
                    .map(|example| example.token_estimate)
                    .sum::<usize>()
        })
        .sum::<usize>();
    let template_reference_tokens = inputs
        .template_references
        .iter()
        .map(|reference| {
            estimate_tokens(&reference.template.template_title)
                + reference
                    .implementation_pages
                    .iter()
                    .map(|page| {
                        estimate_tokens(&page.page_title)
                            + estimate_tokens(&page.summary_text)
                            + page
                                .context_chunks
                                .iter()
                                .map(|chunk| chunk.token_estimate)
                                .sum::<usize>()
                    })
                    .sum::<usize>()
        })
        .sum::<usize>();
    let module_tokens = inputs
        .module_patterns
        .iter()
        .map(|module| {
            estimate_tokens(&module.module_title)
                + module
                    .function_stats
                    .iter()
                    .map(|function| {
                        estimate_tokens(&function.function_name)
                            + function
                                .example_parameter_keys
                                .iter()
                                .map(|key| estimate_tokens(key))
                                .sum::<usize>()
                    })
                    .sum::<usize>()
                + module
                    .example_invocations
                    .iter()
                    .map(|example| example.token_estimate)
                    .sum::<usize>()
        })
        .sum::<usize>();
    let reference_tokens = inputs
        .suggested_references
        .iter()
        .map(|reference| {
            estimate_tokens(&reference.citation_profile)
                + estimate_tokens(&reference.citation_family)
                + estimate_tokens(&reference.source_type)
                + estimate_tokens(&reference.source_origin)
                + estimate_tokens(&reference.source_family)
                + reference
                    .common_templates
                    .iter()
                    .map(|template| estimate_tokens(template))
                    .sum::<usize>()
                + reference
                    .common_links
                    .iter()
                    .map(|title| estimate_tokens(title))
                    .sum::<usize>()
                + reference
                    .common_domains
                    .iter()
                    .map(|domain| estimate_tokens(domain))
                    .sum::<usize>()
                + reference
                    .common_authorities
                    .iter()
                    .map(|authority| estimate_tokens(authority))
                    .sum::<usize>()
                + reference
                    .common_identifier_keys
                    .iter()
                    .map(|identifier| estimate_tokens(identifier))
                    .sum::<usize>()
                + reference
                    .common_identifier_entries
                    .iter()
                    .map(|identifier| estimate_tokens(identifier))
                    .sum::<usize>()
                + reference
                    .common_retrieval_signals
                    .iter()
                    .map(|flag| estimate_tokens(flag))
                    .sum::<usize>()
                + reference
                    .example_references
                    .iter()
                    .map(|example| example.token_estimate)
                    .sum::<usize>()
        })
        .sum::<usize>();
    let media_tokens = inputs
        .suggested_media
        .iter()
        .map(|media| {
            estimate_tokens(&media.file_title)
                + media
                    .example_usages
                    .iter()
                    .map(|example| example.token_estimate)
                    .sum::<usize>()
        })
        .sum::<usize>();
    let stub_template_tokens = inputs
        .stub_detected_templates
        .iter()
        .map(|template| {
            estimate_tokens(&template.template_title)
                + template
                    .parameter_keys
                    .iter()
                    .map(|key| estimate_tokens(key))
                    .sum::<usize>()
        })
        .sum::<usize>();
    let chunk_tokens = inputs
        .chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();
    let docs_tokens = inputs
        .docs_context
        .map(|context| context.token_estimate_total)
        .unwrap_or(0);

    page_summary_tokens
        .saturating_add(link_tokens)
        .saturating_add(category_tokens)
        .saturating_add(template_tokens)
        .saturating_add(template_reference_tokens)
        .saturating_add(module_tokens)
        .saturating_add(reference_tokens)
        .saturating_add(media_tokens)
        .saturating_add(docs_tokens)
        .saturating_add(stub_template_tokens)
        .saturating_add(chunk_tokens)
}

fn load_authoring_inventory(connection: &Connection) -> Result<AuthoringInventory> {
    let indexed_pages_total = count_query(connection, "SELECT COUNT(*) FROM indexed_pages")
        .context("failed to count indexed pages for authoring inventory")?;
    let semantic_profiles_total = if table_exists(connection, "indexed_page_semantics")? {
        count_query(connection, "SELECT COUNT(*) FROM indexed_page_semantics")
            .context("failed to count semantic profiles for authoring inventory")?
    } else {
        0
    };
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
    let (module_invocation_rows_total, distinct_modules_invoked) =
        if table_exists(connection, "indexed_module_invocations")? {
            (
                count_query(
                    connection,
                    "SELECT COUNT(*) FROM indexed_module_invocations",
                )
                .context("failed to count module invocation rows for authoring inventory")?,
                count_query(
                    connection,
                    "SELECT COUNT(DISTINCT module_title) FROM indexed_module_invocations",
                )
                .context("failed to count distinct modules for authoring inventory")?,
            )
        } else {
            (0, 0)
        };
    let (reference_rows_total, distinct_reference_profiles) =
        if table_exists(connection, "indexed_page_references")? {
            (
                count_query(connection, "SELECT COUNT(*) FROM indexed_page_references")
                    .context("failed to count reference rows for authoring inventory")?,
                count_query(
                    connection,
                    "SELECT COUNT(DISTINCT citation_profile) FROM indexed_page_references",
                )
                .context("failed to count distinct reference profiles for authoring inventory")?,
            )
        } else {
            (0, 0)
        };
    let reference_authority_rows_total =
        if table_exists(connection, "indexed_reference_authorities")? {
            count_query(
                connection,
                "SELECT COUNT(*) FROM indexed_reference_authorities",
            )
            .context("failed to count reference authority rows for authoring inventory")?
        } else {
            0
        };
    let reference_identifier_rows_total =
        if table_exists(connection, "indexed_reference_identifiers")? {
            count_query(
                connection,
                "SELECT COUNT(*) FROM indexed_reference_identifiers",
            )
            .context("failed to count reference identifier rows for authoring inventory")?
        } else {
            0
        };
    let (media_rows_total, distinct_media_files) =
        if table_exists(connection, "indexed_page_media")? {
            (
                count_query(connection, "SELECT COUNT(*) FROM indexed_page_media")
                    .context("failed to count media rows for authoring inventory")?,
                count_query(
                    connection,
                    "SELECT COUNT(DISTINCT file_title) FROM indexed_page_media",
                )
                .context("failed to count distinct media files for authoring inventory")?,
            )
        } else {
            (0, 0)
        };
    let template_implementation_rows_total =
        if table_exists(connection, "indexed_template_implementation_pages")? {
            count_query(
                connection,
                "SELECT COUNT(*) FROM indexed_template_implementation_pages",
            )
            .context("failed to count template implementation rows for authoring inventory")?
        } else {
            0
        };

    Ok(AuthoringInventory {
        indexed_pages_total,
        semantic_profiles_total,
        main_pages,
        template_pages,
        indexed_links_total,
        template_invocation_rows,
        distinct_templates_invoked,
        module_invocation_rows_total,
        distinct_modules_invoked,
        reference_rows_total,
        reference_authority_rows_total,
        reference_identifier_rows_total,
        distinct_reference_profiles,
        media_rows_total,
        distinct_media_files,
        template_implementation_rows_total,
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
        let history_score = super::section_authoring_bias(
            Some("History"),
            "Alpha biography summary with useful prose.",
        );
        let references_score =
            super::section_authoring_bias(Some("References"), "{{Reflist}}\n[[Category:Test]]");

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
