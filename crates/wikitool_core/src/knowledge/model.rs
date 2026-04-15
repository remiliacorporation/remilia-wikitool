use std::collections::BTreeMap;

use serde::Serialize;

use crate::filesystem::ScanStats;
use crate::knowledge::status::DEFAULT_DOCS_PROFILE;

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalSearchHit {
    pub title: String,
    pub namespace: String,
    pub is_redirect: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub translation_languages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_translation_language: Option<String>,
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
pub struct AuthoringTopicAssessment {
    pub title_exists_locally: bool,
    pub should_create_new_article: bool,
    pub exact_page: Option<LocalSearchHit>,
    pub local_title_hit_count: usize,
    pub local_title_hits: Vec<LocalSearchHit>,
    pub backlink_count: usize,
    pub backlinks: Vec<String>,
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

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ReferenceAuditFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReferenceAuditSummaryReport {
    pub reference_count: usize,
    pub distinct_page_count: usize,
    pub distinct_domain_count: usize,
    pub distinct_template_count: usize,
    pub distinct_authority_count: usize,
    pub distinct_identifier_key_count: usize,
    pub distinct_identifier_entry_count: usize,
    pub top_domains: Vec<String>,
    pub top_templates: Vec<String>,
    pub top_authorities: Vec<String>,
    pub top_identifier_keys: Vec<String>,
    pub top_identifier_entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReferenceListItem {
    pub source_title: String,
    pub source_relative_path: String,
    pub section_heading: Option<String>,
    pub reference_index: usize,
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
    pub reference_wikitext: String,
    pub template_titles: Vec<String>,
    pub link_titles: Vec<String>,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReferenceListReport {
    pub reference_count: usize,
    pub items: Vec<ReferenceListItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceDuplicateKind {
    CanonicalUrl,
    NormalizedIdentifier,
    ExactReferenceWikitext,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReferenceDuplicateGroup {
    pub kind: ReferenceDuplicateKind,
    pub match_key: String,
    pub reference_count: usize,
    pub distinct_page_count: usize,
    pub source_titles: Vec<String>,
    pub items: Vec<ReferenceListItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReferenceDuplicatesReport {
    pub duplicate_group_count: usize,
    pub duplicated_reference_count: usize,
    pub groups: Vec<ReferenceDuplicateGroup>,
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
pub struct ComparablePageHeading {
    pub source_title: String,
    pub section_heading: String,
    pub section_level: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthoringKnowledgePackResult {
    pub topic: String,
    pub query: String,
    pub query_terms: Vec<String>,
    pub topic_assessment: AuthoringTopicAssessment,
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
    pub comparable_page_headings: Vec<ComparablePageHeading>,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LivePageVerificationStatus {
    Exists,
    Missing,
    RedirectResolved,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LiveBrokenLinkVerification {
    pub source_title: String,
    pub target_title: String,
    pub live_status: LivePageVerificationStatus,
    pub resolved_title: Option<String>,
    pub page_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiveRedirectVerificationStatus {
    SourceMissing,
    ResolvesToExpectedFinal,
    ResolvesToDifferentTarget,
    SourceExistsWithoutRedirectResolution,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LiveDoubleRedirectVerification {
    pub title: String,
    pub first_target: String,
    pub final_target: String,
    pub live_status: LiveRedirectVerificationStatus,
    pub resolved_title: Option<String>,
    pub page_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LiveValidationReport {
    pub request_count: usize,
    pub broken_links: Vec<LiveBrokenLinkVerification>,
    pub double_redirects: Vec<LiveDoubleRedirectVerification>,
}
