use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::LocalTemplateExample;
use super::TemplateDataRecord;

pub(super) const TEMPLATE_CATALOG_ARTIFACT_KIND: &str = "template_catalog";
pub(super) const TEMPLATE_CATALOG_SCHEMA_VERSION: &str = "template_catalog_v2";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateCatalogParameter {
    pub name: String,
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_names: Vec<String>,
    pub sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param_type: Option<String>,
    pub required: bool,
    pub suggested: bool,
    pub deprecated: bool,
    pub usage_count: usize,
    pub example_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateCatalogExample {
    pub source_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_relative_path: Option<String>,
    pub invocation_text: String,
    pub parameter_keys: Vec<String>,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateCatalogEntry {
    pub template_title: String,
    pub relative_path: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub templatedata: Option<TemplateDataRecord>,
    pub redirect_aliases: Vec<String>,
    pub usage_aliases: Vec<String>,
    pub usage_count: usize,
    pub distinct_page_count: usize,
    pub example_pages: Vec<String>,
    pub documentation_titles: Vec<String>,
    pub implementation_titles: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementation_preview: Option<String>,
    pub module_titles: Vec<String>,
    pub declared_parameter_keys: Vec<String>,
    pub parameters: Vec<TemplateCatalogParameter>,
    pub examples: Vec<TemplateCatalogExample>,
    pub recommendation_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateCatalog {
    pub schema_version: String,
    pub profile_id: String,
    pub refreshed_at: String,
    pub template_count: usize,
    pub templatedata_count: usize,
    pub redirect_alias_count: usize,
    pub usage_index_ready: bool,
    pub entries: Vec<TemplateCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TemplateCatalogEntryLookup {
    CatalogMissing,
    TemplateMissing { template_title: String },
    Found(Box<TemplateCatalogEntry>),
}

#[derive(Debug, Clone, Default)]
pub(super) struct LocalUsageAccumulator {
    pub(super) usage_count: usize,
    pub(super) example_values: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct LocalDocumentationPage {
    pub(super) title: String,
    pub(super) relative_path: String,
    pub(super) content: String,
}

pub(super) type LocalTemplateMap = BTreeMap<String, LocalTemplateRecord>;
pub(super) type TemplateAliasMap = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone)]
pub(super) struct LocalTemplateRecord {
    pub(super) template_title: String,
    pub(super) relative_path: String,
    pub(super) category: String,
    pub(super) templatedata: Option<TemplateDataRecord>,
    pub(super) summary_text: Option<String>,
    pub(super) declared_parameter_keys: Vec<String>,
    pub(super) documentation_pages: Vec<LocalDocumentationPage>,
    pub(super) local_examples: Vec<LocalTemplateExample>,
    pub(super) module_titles: Vec<String>,
}
