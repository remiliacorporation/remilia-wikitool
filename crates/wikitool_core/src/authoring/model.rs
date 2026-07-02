use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalExistenceState {
    ExactPageExists,
    RedirectExists,
    LinkedButMissing,
    LikelyMissing,
    AmbiguousLocalCoverage,
    ExpansionCandidate,
    MergeCandidate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextSurfaceSource {
    Profile,
    Comparables,
    Both,
    ContractTraversal,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleStartIntent {
    #[default]
    New,
    Expand,
    Audit,
    Refresh,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceRef {
    pub id: String,
    pub source_kind: String,
    pub source_title: String,
    pub locator: Option<String>,
    /// Approximate chunk size in tokens. This is a size, not a relevance rank;
    /// it was previously (mis)named `score`.
    pub token_estimate: u32,
    /// Leading excerpt of the chunk so the reference is usable without a
    /// follow-up `knowledge inspect chunks` call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenQuestion {
    pub question: String,
    pub reason: String,
    pub blocking: bool,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecommendedAction {
    pub label: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequiredTemplate {
    pub template_title: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectTypeHint {
    pub subject_type: String,
    pub source: ContextSurfaceSource,
    pub supporting_pages: Vec<String>,
    pub supporting_templates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateSurfaceEntry {
    pub template_title: String,
    pub source: ContextSurfaceSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mapped_subject_type: Option<String>,
    pub supporting_pages: Vec<String>,
    /// Known parameter keys from the contract index, inlined so drafting does
    /// not require a second `templates show` round-trip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategorySurfaceEntry {
    pub category_title: String,
    pub source: ContextSurfaceSource,
    pub supporting_pages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkSurfaceEntry {
    pub page_title: String,
    pub source: ContextSurfaceSource,
    pub supporting_pages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SectionSkeleton {
    pub heading: String,
    pub rationale: String,
    pub required: bool,
    #[serde(default)]
    pub content_backed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_pages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectResearchLane {
    /// Top retrieved chunk text. For a subject with no local page this is
    /// prose from *other* pages, not facts about the subject.
    pub top_local_excerpt: Option<String>,
    pub evidence: Vec<EvidenceRef>,
    /// Verbatim chunk texts from comparable pages. Context to mine, not
    /// assertions about the subject; previously (mis)named `candidate_facts`.
    pub comparable_page_excerpts: Vec<String>,
    /// Citation template families observed locally (e.g. "cite web / web");
    /// previously (mis)named `external_sources_shortlist`, which suggested
    /// followable sources.
    pub citation_template_families: Vec<String>,
    pub ambiguity_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryTermCoverage {
    pub term: String,
    pub local_chunk_matches: usize,
    pub comparable_page_matches: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceCoverageItem {
    pub source_kind: String,
    pub source_title: String,
    pub locator: Option<String>,
    pub matched_query_terms: Vec<String>,
    pub missing_query_terms: Vec<String>,
    pub evidence_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticleEvidenceProfile {
    pub query: String,
    pub query_terms: Vec<String>,
    pub exact_local_title: Option<String>,
    pub local_title_hit_count: usize,
    pub backlink_count: usize,
    pub direct_subject_evidence: Vec<EvidenceCoverageItem>,
    pub broad_context: Vec<EvidenceCoverageItem>,
    pub comparable_pages: Vec<EvidenceCoverageItem>,
    pub live_leads_status: String,
    pub live_leads: Vec<EvidenceCoverageItem>,
    pub missing_query_terms: Vec<String>,
    pub query_term_coverage: Vec<QueryTermCoverage>,
    pub missing_evidence_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComparableOutline {
    pub title: String,
    /// The page's level-2 headings in document order.
    pub ordered_headings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalIntegrationLane {
    pub comparable_pages: Vec<String>,
    /// The closest comparable page's section sequence in document order — the
    /// strongest single structural model for a new draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closest_comparable_outline: Option<ComparableOutline>,
    pub required_templates: Vec<RequiredTemplate>,
    pub subject_type_hints: Vec<SubjectTypeHint>,
    pub available_infoboxes: Vec<TemplateSurfaceEntry>,
    pub citation_templates_seen: Vec<TemplateSurfaceEntry>,
    pub template_surface: Vec<TemplateSurfaceEntry>,
    pub categories_seen: Vec<CategorySurfaceEntry>,
    pub links_seen: Vec<LinkSurfaceEntry>,
    pub section_skeleton: Vec<SectionSkeleton>,
    pub docs_queries: Vec<String>,
    pub contract_query: String,
    pub contract_matched_query_terms: Vec<String>,
    pub contract_missing_query_terms: Vec<String>,
    pub contract_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringConstraint {
    pub level: String,
    pub rule_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticleStartResult {
    pub schema_version: String,
    pub topic: String,
    pub intent: ArticleStartIntent,
    pub local_state: LocalExistenceState,
    pub evidence_profile: ArticleEvidenceProfile,
    pub subject_research: SubjectResearchLane,
    pub local_integration: LocalIntegrationLane,
    pub constraints: Vec<AuthoringConstraint>,
    pub open_questions: Vec<OpenQuestion>,
    pub next_actions: Vec<RecommendedAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArticleStart {
    IndexMissing,
    QueryMissing,
    Found(Box<ArticleStartResult>),
}
