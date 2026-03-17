use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::wiki_capabilities::WikiCapabilityManifest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileSourceDocument {
    pub relative_path: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CitationTemplateRule {
    pub family: String,
    pub template_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnreliableSourceRule {
    pub label: String,
    pub matcher: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InfoboxPreference {
    pub subject_type: String,
    pub template_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringRules {
    pub require_short_description: bool,
    pub short_description_forms: Vec<String>,
    pub require_article_quality_banner: bool,
    pub article_quality_template: Option<String>,
    pub article_quality_default_state: Option<String>,
    pub required_appendix_sections: Vec<String>,
    pub references_template: Option<String>,
    pub prefer_sentence_case_headings: bool,
    pub prefer_wikitext_only: bool,
    pub forbid_markdown: bool,
    pub require_straight_quotes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CitationRules {
    pub preferred_templates: Vec<CitationTemplateRule>,
    pub use_named_references: bool,
    pub leave_archive_fields_blank: bool,
    pub unreliable_sources: Vec<UnreliableSourceRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemiliaRules {
    pub default_parent_group: Option<String>,
    pub preferred_group_field: Option<String>,
    pub avoid_group_fields: Vec<String>,
    pub infobox_preferences: Vec<InfoboxPreference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CategoryRules {
    pub preferred_categories: Vec<String>,
    pub min_per_article: usize,
    pub max_per_article: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LintRules {
    pub banned_phrases: Vec<String>,
    pub watchlist_terms: Vec<String>,
    pub forbid_curly_quotes: bool,
    pub forbid_placeholder_fragments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoldenSetRules {
    pub article_corpus_available: bool,
    pub source_documents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileOverlay {
    pub schema_version: String,
    pub profile_id: String,
    pub base_profile_id: String,
    pub docs_profile: String,
    pub source_documents: Vec<ProfileSourceDocument>,
    pub authoring: AuthoringRules,
    pub citations: CitationRules,
    pub remilia: RemiliaRules,
    pub categories: CategoryRules,
    pub lint: LintRules,
    pub golden_set: GoldenSetRules,
    pub refreshed_at: String,
}

impl ProfileOverlay {
    pub fn recommended_template_titles(&self) -> Vec<String> {
        let mut titles = BTreeSet::new();
        if let Some(value) = self.authoring.article_quality_template.as_deref() {
            titles.insert(value.to_string());
        }
        if let Some(value) = self.authoring.references_template.as_deref() {
            titles.insert(value.to_string());
        }
        for rule in &self.citations.preferred_templates {
            titles.insert(rule.template_title.clone());
        }
        for preference in &self.remilia.infobox_preferences {
            titles.insert(preference.template_title.clone());
        }
        titles.into_iter().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateCatalogSummary {
    pub profile_id: String,
    pub template_count: usize,
    pub templatedata_count: usize,
    pub redirect_alias_count: usize,
    pub usage_index_ready: bool,
    pub recommended_template_titles: Vec<String>,
    pub refreshed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WikiProfileSnapshot {
    pub base_profile_id: String,
    pub overlay: ProfileOverlay,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<WikiCapabilityManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_catalog: Option<TemplateCatalogSummary>,
}
