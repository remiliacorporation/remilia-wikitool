use std::collections::{BTreeMap, BTreeSet};

pub(crate) const INDEX_CHUNK_WORD_TARGET: usize = 96;
pub(crate) const CONTEXT_CHUNK_LIMIT: usize = 8;
pub(crate) const CONTEXT_TOKEN_BUDGET: usize = 720;
pub(crate) const TEMPLATE_INVOCATION_LIMIT: usize = 24;
pub(crate) const NO_PARAMETER_KEYS_SENTINEL: &str = "__none__";
pub(crate) const CHUNK_CANDIDATE_MULTIPLIER_SINGLE: usize = 6;
pub(crate) const CHUNK_CANDIDATE_MULTIPLIER_ACROSS: usize = 10;
pub(crate) const CHUNK_LEXICAL_SIMILARITY_THRESHOLD: f32 = 0.86;
pub(crate) const AUTHORING_TEMPLATE_KEY_LIMIT: usize = 12;
pub(crate) const TEMPLATE_REFERENCE_EXAMPLE_LIMIT: usize = 3;
pub(crate) const TEMPLATE_IMPLEMENTATION_PAGE_LIMIT: usize = 6;
pub(crate) const TEMPLATE_PARAMETER_VALUE_LIMIT: usize = 3;
pub(crate) const MODULE_REFERENCE_EXAMPLE_LIMIT: usize = 4;
pub(crate) const AUTHORING_MODULE_FUNCTION_LIMIT: usize = 8;
pub(crate) const AUTHORING_TEMPLATE_REFERENCE_LIMIT: usize = 4;
pub(crate) const AUTHORING_MODULE_PATTERN_LIMIT: usize = 6;
pub(crate) const AUTHORING_REFERENCE_LIMIT: usize = 8;
pub(crate) const AUTHORING_REFERENCE_EXAMPLE_LIMIT: usize = 3;
pub(crate) const AUTHORING_REFERENCE_AUTHORITY_LIMIT: usize = 8;
pub(crate) const AUTHORING_REFERENCE_DOMAIN_LIMIT: usize = 6;
pub(crate) const AUTHORING_REFERENCE_FLAG_LIMIT: usize = 8;
pub(crate) const AUTHORING_REFERENCE_IDENTIFIER_LIMIT: usize = 8;
pub(crate) const AUTHORING_MEDIA_LIMIT: usize = 8;
pub(crate) const AUTHORING_MEDIA_EXAMPLE_LIMIT: usize = 3;
pub(crate) const AUTHORING_PAGE_SUMMARY_WORD_LIMIT: usize = 36;
pub(crate) const AUTHORING_QUERY_EXPANSION_LIMIT: usize = 8;
pub(crate) const AUTHORING_SECTION_LIMIT: usize = 24;
pub(crate) const AUTHORING_SUGGESTION_EVIDENCE_LIMIT: usize = 4;
pub(crate) const AUTHORING_SEED_CHUNKS_PER_PAGE: usize = 2;
pub(crate) const CONTEXT_REFERENCE_LIMIT: usize = 12;
pub(crate) const CONTEXT_MEDIA_LIMIT: usize = 12;
pub(crate) const NO_STRING_LIST_SENTINEL: &str = "__none__";

pub(crate) fn candidate_limit(limit: usize, multiplier: usize) -> usize {
    limit
        .saturating_mul(multiplier.max(1))
        .clamp(limit.max(1), 512)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedLink {
    pub(crate) target_title: String,
    pub(crate) target_namespace: String,
    pub(crate) is_category_membership: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedPageRecord {
    pub(crate) title: String,
    pub(crate) namespace: String,
    pub(crate) is_redirect: bool,
    pub(crate) redirect_target: Option<String>,
    pub(crate) relative_path: String,
    pub(crate) bytes: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedLinkRow {
    pub(crate) target_title: String,
    pub(crate) target_namespace: String,
    pub(crate) is_category_membership: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedContextChunkRow {
    pub(crate) section_heading: Option<String>,
    pub(crate) token_estimate: usize,
    pub(crate) chunk_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedSectionRecord {
    pub(crate) section_heading: Option<String>,
    pub(crate) section_level: u8,
    pub(crate) summary_text: String,
    pub(crate) section_text: String,
    pub(crate) token_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedReferenceRecord {
    pub(crate) section_heading: Option<String>,
    pub(crate) reference_name: Option<String>,
    pub(crate) reference_group: Option<String>,
    pub(crate) citation_profile: String,
    pub(crate) citation_family: String,
    pub(crate) primary_template_title: Option<String>,
    pub(crate) source_type: String,
    pub(crate) source_origin: String,
    pub(crate) source_family: String,
    pub(crate) authority_kind: String,
    pub(crate) source_authority: String,
    pub(crate) reference_title: String,
    pub(crate) source_container: String,
    pub(crate) source_author: String,
    pub(crate) source_domain: String,
    pub(crate) source_date: String,
    pub(crate) canonical_url: String,
    pub(crate) identifier_keys: Vec<String>,
    pub(crate) identifier_entries: Vec<String>,
    pub(crate) source_urls: Vec<String>,
    pub(crate) retrieval_signals: Vec<String>,
    pub(crate) summary_text: String,
    pub(crate) reference_wikitext: String,
    pub(crate) template_titles: Vec<String>,
    pub(crate) link_titles: Vec<String>,
    pub(crate) token_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedSemanticProfileRecord {
    pub(crate) source_title: String,
    pub(crate) source_namespace: String,
    pub(crate) summary_text: String,
    pub(crate) section_headings: Vec<String>,
    pub(crate) category_titles: Vec<String>,
    pub(crate) template_titles: Vec<String>,
    pub(crate) template_parameter_keys: Vec<String>,
    pub(crate) link_titles: Vec<String>,
    pub(crate) reference_titles: Vec<String>,
    pub(crate) reference_containers: Vec<String>,
    pub(crate) reference_domains: Vec<String>,
    pub(crate) reference_source_families: Vec<String>,
    pub(crate) reference_authorities: Vec<String>,
    pub(crate) reference_identifiers: Vec<String>,
    pub(crate) media_titles: Vec<String>,
    pub(crate) media_captions: Vec<String>,
    pub(crate) template_implementation_titles: Vec<String>,
    pub(crate) semantic_text: String,
    pub(crate) token_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedMediaRecord {
    pub(crate) section_heading: Option<String>,
    pub(crate) file_title: String,
    pub(crate) media_kind: String,
    pub(crate) caption_text: String,
    pub(crate) options: Vec<String>,
    pub(crate) token_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct RetrievedChunkCandidate {
    pub(crate) chunk: crate::knowledge::retrieval::RetrievedChunk,
    pub(crate) lexical_signature: String,
    pub(crate) lexical_terms: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedTemplateInvocation {
    pub(crate) template_title: String,
    pub(crate) parameter_keys: Vec<String>,
    pub(crate) raw_wikitext: String,
    pub(crate) token_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedModuleInvocation {
    pub(crate) module_title: String,
    pub(crate) function_name: String,
    pub(crate) parameter_keys: Vec<String>,
    pub(crate) raw_wikitext: String,
    pub(crate) token_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedPageArtifacts {
    pub(crate) section_records: Vec<IndexedSectionRecord>,
    pub(crate) context_chunks: Vec<ArticleContextChunkRow>,
    pub(crate) template_invocations: Vec<ParsedTemplateInvocation>,
    pub(crate) module_invocations: Vec<ParsedModuleInvocation>,
    pub(crate) references: Vec<IndexedReferenceRecord>,
    pub(crate) media: Vec<IndexedMediaRecord>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TemplateImplementationSeed {
    pub(crate) template_dependencies: Vec<String>,
    pub(crate) module_dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedIdentifierEntry {
    pub(crate) key: String,
    pub(crate) value: String,
    pub(crate) normalized_value: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ChunkRetrievalPlan {
    pub(crate) limit: usize,
    pub(crate) token_budget: usize,
    pub(crate) max_pages: usize,
    pub(crate) diversify: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ChunkRerankSignals {
    pub(crate) related_page_weights: BTreeMap<String, usize>,
    pub(crate) template_page_weights: BTreeMap<String, usize>,
    pub(crate) semantic_page_weights: BTreeMap<String, usize>,
    pub(crate) authority_page_weights: BTreeMap<String, usize>,
    pub(crate) identifier_page_weights: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticPageHit {
    pub(crate) title: String,
    pub(crate) retrieval_weight: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ArticleContextChunkRow {
    pub(crate) section_heading: Option<String>,
    pub(crate) chunk_text: String,
    pub(crate) token_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedContentSection {
    pub(crate) section_heading: Option<String>,
    pub(crate) section_level: u8,
    pub(crate) section_text: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ReferenceTemplateDetails {
    pub(crate) template_title: String,
    pub(crate) named_params: BTreeMap<String, String>,
    pub(crate) positional_params: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ReferenceAnalysis {
    pub(crate) citation_profile: String,
    pub(crate) citation_family: String,
    pub(crate) primary_template_title: Option<String>,
    pub(crate) source_type: String,
    pub(crate) source_origin: String,
    pub(crate) source_family: String,
    pub(crate) authority_kind: String,
    pub(crate) source_authority: String,
    pub(crate) reference_title: String,
    pub(crate) source_container: String,
    pub(crate) source_author: String,
    pub(crate) source_domain: String,
    pub(crate) source_date: String,
    pub(crate) canonical_url: String,
    pub(crate) identifier_keys: Vec<String>,
    pub(crate) identifier_entries: Vec<String>,
    pub(crate) source_urls: Vec<String>,
    pub(crate) retrieval_signals: Vec<String>,
    pub(crate) summary_hint: String,
}







