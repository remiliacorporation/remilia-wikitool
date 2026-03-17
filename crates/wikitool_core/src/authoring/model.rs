use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleType {
    Person,
    Organization,
    Website,
    Concept,
    Event,
    Work,
    Collection,
    Unknown,
}

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
pub struct TemplateRecommendation {
    pub template_title: String,
    pub rationale: String,
    pub confidence: u8,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategoryRecommendation {
    pub category_title: String,
    pub rationale: String,
    pub confidence: u8,
    pub evidence_titles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkRecommendation {
    pub page_title: String,
    pub rationale: String,
    pub confidence: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SectionSkeleton {
    pub heading: String,
    pub rationale: String,
    pub required: bool,
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
pub struct LocalIntegrationLane {
    pub comparable_pages: Vec<String>,
    pub infobox: Option<TemplateRecommendation>,
    pub citation_families: Vec<TemplateRecommendation>,
    pub template_recommendations: Vec<TemplateRecommendation>,
    pub category_candidates: Vec<CategoryRecommendation>,
    pub link_candidates: Vec<LinkRecommendation>,
    pub section_skeleton: Vec<SectionSkeleton>,
    pub docs_queries: Vec<String>,
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
    pub article_type: ArticleType,
    pub local_state: LocalExistenceState,
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
