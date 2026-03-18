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
    pub required_templates: Vec<RequiredTemplate>,
    pub subject_type_hints: Vec<SubjectTypeHint>,
    pub available_infoboxes: Vec<TemplateSurfaceEntry>,
    pub citation_templates_seen: Vec<TemplateSurfaceEntry>,
    pub template_surface: Vec<TemplateSurfaceEntry>,
    pub categories_seen: Vec<CategorySurfaceEntry>,
    pub links_seen: Vec<LinkSurfaceEntry>,
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
