use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalResearchPolicy {
    Fallback,
    Always,
    Off,
}

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
    pub score: u32,
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
    pub summary: Option<String>,
    pub evidence: Vec<EvidenceRef>,
    pub candidate_facts: Vec<String>,
    pub external_sources_shortlist: Vec<String>,
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
pub struct LocalIntegrationLane {
    pub comparable_pages: Vec<String>,
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
pub struct ArticleStartOptions {
    pub related_page_limit: usize,
    pub external_policy: ExternalResearchPolicy,
    pub docs_profile: String,
    pub profile_id: Option<String>,
    pub include_raw_pack_ref: bool,
    pub diversify: bool,
}

impl Default for ArticleStartOptions {
    fn default() -> Self {
        Self {
            related_page_limit: 18,
            external_policy: ExternalResearchPolicy::Fallback,
            docs_profile: "remilia-mw-1.44".to_string(),
            profile_id: None,
            include_raw_pack_ref: true,
            diversify: true,
        }
    }
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
    pub raw_pack_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArticleStart {
    IndexMissing,
    QueryMissing,
    Found(Box<ArticleStartResult>),
}
