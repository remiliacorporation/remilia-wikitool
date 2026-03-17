use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::content_store::parsing::open_indexed_connection;
use crate::filesystem::{ScanOptions, scan_files};
use crate::knowledge::status::KNOWLEDGE_GENERATION;
use crate::knowledge::templates::{
    load_template_reference_for_connection, normalize_template_lookup_title,
    summarize_template_usage_for_sources,
};
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{normalize_path, unix_timestamp};

use super::rules::{ProfileOverlay, TemplateCatalogSummary};
use super::template_data::{
    LocalTemplateExample, TemplateDataParameter, TemplateDataRecord, extract_module_references,
    extract_source_parameters, extract_summary_text, extract_template_data,
    extract_template_examples,
};

const TEMPLATE_CATALOG_ARTIFACT_KIND: &str = "template_catalog";
const TEMPLATE_CATALOG_SCHEMA_VERSION: &str = "template_catalog_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateCatalogParameter {
    pub name: String,
    pub aliases: Vec<String>,
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
struct LocalUsageAccumulator {
    usage_count: usize,
    example_values: Vec<String>,
}

#[derive(Debug, Clone)]
struct LocalDocumentationPage {
    title: String,
    relative_path: String,
    content: String,
}

type LocalTemplateMap = BTreeMap<String, LocalTemplateRecord>;
type TemplateAliasMap = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone)]
struct LocalTemplateRecord {
    template_title: String,
    relative_path: String,
    category: String,
    templatedata: Option<TemplateDataRecord>,
    summary_text: Option<String>,
    declared_parameter_keys: Vec<String>,
    documentation_pages: Vec<LocalDocumentationPage>,
    local_examples: Vec<LocalTemplateExample>,
    module_titles: Vec<String>,
}

pub fn build_template_catalog_with_overlay(
    paths: &ResolvedPaths,
    overlay: &ProfileOverlay,
) -> Result<TemplateCatalog> {
    let (local_templates, redirect_aliases) = load_local_templates(paths)?;

    let mut usage_map = BTreeMap::new();
    let mut reference_map = BTreeMap::new();
    let usage_index_ready = if let Some(connection) = open_indexed_connection(paths)? {
        for summary in summarize_template_usage_for_sources(&connection, None, usize::MAX)? {
            usage_map.insert(
                normalize_template_lookup_title(&summary.template_title),
                summary,
            );
        }
        for template_title in local_templates.keys() {
            if let Some(reference) =
                load_template_reference_for_connection(&connection, template_title)?
            {
                reference_map.insert(template_title.clone(), reference);
            }
        }
        true
    } else {
        false
    };

    let mut entries = Vec::new();
    let mut redirect_alias_count = 0usize;
    let mut templatedata_count = 0usize;
    for (normalized_title, template) in local_templates {
        let usage = usage_map.get(&normalized_title);
        let reference = reference_map.get(&normalized_title);
        let redirect_aliases_for_template = redirect_aliases
            .get(&normalized_title)
            .cloned()
            .unwrap_or_default();
        redirect_alias_count += redirect_aliases_for_template.len();
        if template.templatedata.is_some() {
            templatedata_count += 1;
        }
        entries.push(build_catalog_entry(
            template,
            usage,
            reference,
            &redirect_aliases_for_template,
            overlay,
        ));
    }
    entries.sort_by(|left, right| left.template_title.cmp(&right.template_title));

    Ok(TemplateCatalog {
        schema_version: TEMPLATE_CATALOG_SCHEMA_VERSION.to_string(),
        profile_id: overlay.profile_id.clone(),
        refreshed_at: unix_timestamp()?.to_string(),
        template_count: entries.len(),
        templatedata_count,
        redirect_alias_count,
        usage_index_ready,
        entries,
    })
}

pub fn sync_template_catalog_with_overlay(
    paths: &ResolvedPaths,
    overlay: &ProfileOverlay,
) -> Result<TemplateCatalog> {
    let catalog = build_template_catalog_with_overlay(paths, overlay)?;
    store_template_catalog(paths, &catalog)?;
    Ok(catalog)
}

pub fn load_template_catalog(
    paths: &ResolvedPaths,
    profile_id: &str,
) -> Result<Option<TemplateCatalog>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let catalog_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM knowledge_artifacts
             WHERE artifact_key = ?1",
            params![template_catalog_artifact_key(profile_id)],
            |row| row.get(0),
        )
        .optional()
        .with_context(|| format!("failed to load template catalog for {profile_id}"))?;

    catalog_json
        .map(|value| serde_json::from_str(&value).context("failed to decode template catalog"))
        .transpose()
}

pub fn load_latest_template_catalog(paths: &ResolvedPaths) -> Result<Option<TemplateCatalog>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let catalog_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM knowledge_artifacts
             WHERE artifact_kind = ?1
             ORDER BY built_at_unix DESC
             LIMIT 1",
            params![TEMPLATE_CATALOG_ARTIFACT_KIND],
            |row| row.get(0),
        )
        .optional()
        .context("failed to load latest template catalog")?;

    catalog_json
        .map(|value| serde_json::from_str(&value).context("failed to decode template catalog"))
        .transpose()
}

pub fn find_template_catalog_entry(
    catalog: &TemplateCatalog,
    template_title: &str,
) -> TemplateCatalogEntryLookup {
    let normalized = normalize_template_lookup_title(template_title);
    if normalized.is_empty() {
        return TemplateCatalogEntryLookup::TemplateMissing {
            template_title: template_title.trim().to_string(),
        };
    }

    for entry in &catalog.entries {
        if normalize_template_lookup_title(&entry.template_title) == normalized {
            return TemplateCatalogEntryLookup::Found(Box::new(entry.clone()));
        }
        if entry
            .redirect_aliases
            .iter()
            .chain(entry.usage_aliases.iter())
            .any(|alias| normalize_template_lookup_title(alias) == normalized)
        {
            return TemplateCatalogEntryLookup::Found(Box::new(entry.clone()));
        }
    }

    TemplateCatalogEntryLookup::TemplateMissing {
        template_title: normalized,
    }
}

impl TemplateCatalog {
    pub fn summary(&self) -> TemplateCatalogSummary {
        let mut recommended_template_titles = BTreeSet::new();
        for entry in &self.entries {
            if !entry.recommendation_tags.is_empty() {
                recommended_template_titles.insert(entry.template_title.clone());
            }
        }
        TemplateCatalogSummary {
            profile_id: self.profile_id.clone(),
            template_count: self.template_count,
            templatedata_count: self.templatedata_count,
            redirect_alias_count: self.redirect_alias_count,
            usage_index_ready: self.usage_index_ready,
            recommended_template_titles: recommended_template_titles.into_iter().collect(),
            refreshed_at: self.refreshed_at.clone(),
        }
    }
}

fn build_catalog_entry(
    template: LocalTemplateRecord,
    usage: Option<&crate::knowledge::templates::TemplateUsageSummary>,
    reference: Option<&crate::knowledge::templates::TemplateReference>,
    redirect_aliases: &[String],
    overlay: &ProfileOverlay,
) -> TemplateCatalogEntry {
    let usage_aliases = merge_titles(
        usage.map(|item| item.aliases.clone()).unwrap_or_default(),
        None,
    );
    let parameters = merge_parameters(
        template.templatedata.as_ref(),
        &template.declared_parameter_keys,
        usage,
    );
    let summary_text = template
        .summary_text
        .clone()
        .or_else(|| {
            template
                .templatedata
                .as_ref()
                .and_then(|item| item.description.clone())
        })
        .or_else(|| {
            usage.and_then(|item| {
                item.implementation_preview
                    .as_deref()
                    .and_then(extract_summary_text)
            })
        });
    let documentation_titles = merge_titles(
        template
            .documentation_pages
            .iter()
            .map(|page| page.title.clone())
            .collect(),
        reference.map(|item| {
            item.implementation_pages
                .iter()
                .filter(|page| page.role == "documentation")
                .map(|page| page.page_title.clone())
                .collect()
        }),
    );
    let implementation_titles = merge_titles(
        usage
            .map(|item| item.implementation_titles.clone())
            .unwrap_or_default(),
        None,
    );
    let module_titles = merge_titles(
        template.module_titles.clone(),
        Some(
            reference
                .map(|item| {
                    item.implementation_pages
                        .iter()
                        .filter(|page| page.role == "module")
                        .map(|page| page.page_title.clone())
                        .collect()
                })
                .unwrap_or_else(|| {
                    implementation_titles
                        .iter()
                        .filter(|title| title.starts_with("Module:"))
                        .cloned()
                        .collect()
                }),
        ),
    );

    TemplateCatalogEntry {
        template_title: template.template_title.clone(),
        relative_path: template.relative_path,
        category: template.category,
        summary_text,
        templatedata: template.templatedata,
        redirect_aliases: merge_titles(redirect_aliases.to_vec(), None),
        usage_aliases,
        usage_count: usage.map(|item| item.usage_count).unwrap_or(0),
        distinct_page_count: usage.map(|item| item.distinct_page_count).unwrap_or(0),
        example_pages: usage
            .map(|item| item.example_pages.clone())
            .unwrap_or_default(),
        documentation_titles,
        implementation_titles,
        implementation_preview: usage.and_then(|item| item.implementation_preview.clone()),
        module_titles,
        declared_parameter_keys: template.declared_parameter_keys,
        parameters,
        examples: merge_examples(template.local_examples, usage),
        recommendation_tags: recommendation_tags(&template.template_title, overlay),
    }
}

fn merge_parameters(
    templatedata: Option<&TemplateDataRecord>,
    declared_parameter_keys: &[String],
    usage: Option<&crate::knowledge::templates::TemplateUsageSummary>,
) -> Vec<TemplateCatalogParameter> {
    let mut templatedata_map = BTreeMap::<String, &TemplateDataParameter>::new();
    let mut alias_to_canonical = BTreeMap::<String, String>::new();
    let mut order = Vec::<String>::new();
    let mut seen = BTreeSet::new();
    if let Some(templatedata) = templatedata {
        for parameter in &templatedata.parameters {
            let canonical_name = canonical_parameter_key(&parameter.name);
            let key = canonical_name.to_ascii_lowercase();
            templatedata_map.insert(key.clone(), parameter);
            if seen.insert(key) {
                order.push(canonical_name.clone());
            }
            for alias in &parameter.aliases {
                alias_to_canonical.insert(
                    canonical_parameter_key(alias).to_ascii_lowercase(),
                    canonical_name.clone(),
                );
            }
        }
    }

    let declared_set = declared_parameter_keys
        .iter()
        .map(|item| canonical_parameter_key(item).to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    for key in declared_parameter_keys {
        let canonical = canonical_parameter_key(key);
        let lower = canonical.to_ascii_lowercase();
        if seen.insert(lower) {
            order.push(canonical);
        }
    }

    let mut usage_map = BTreeMap::<String, LocalUsageAccumulator>::new();
    if let Some(usage) = usage {
        let mut usage_keys = usage
            .parameter_stats
            .iter()
            .map(|parameter| canonical_parameter_key(&parameter.key))
            .collect::<Vec<_>>();
        usage_keys.sort();
        for key in usage_keys {
            let canonical = alias_to_canonical
                .get(&key.to_ascii_lowercase())
                .cloned()
                .unwrap_or_else(|| key.clone());
            let lower = canonical.to_ascii_lowercase();
            if seen.insert(lower) {
                order.push(canonical);
            }
        }

        for parameter in &usage.parameter_stats {
            let usage_key = canonical_parameter_key(&parameter.key);
            let canonical = alias_to_canonical
                .get(&usage_key.to_ascii_lowercase())
                .cloned()
                .unwrap_or(usage_key);
            let entry = usage_map.entry(canonical.to_ascii_lowercase()).or_default();
            entry.usage_count = entry.usage_count.saturating_add(parameter.usage_count);
            for value in &parameter.example_values {
                if !entry.example_values.iter().any(|item| item == value) {
                    entry.example_values.push(value.clone());
                }
            }
        }
    }

    let mut out = Vec::new();
    for name in order {
        let lower = name.to_ascii_lowercase();
        let templatedata_parameter = templatedata_map.get(&lower).copied();
        let usage_parameter = usage_map.get(&lower);
        let mut sources = Vec::new();
        if templatedata_parameter.is_some() {
            sources.push("templatedata".to_string());
        }
        if declared_set.contains(&lower) {
            sources.push("source".to_string());
        }
        if usage_parameter.is_some() {
            sources.push("usage".to_string());
        }

        out.push(TemplateCatalogParameter {
            name: name.clone(),
            aliases: templatedata_parameter
                .map(|item| item.aliases.clone())
                .unwrap_or_default(),
            sources,
            label: templatedata_parameter.and_then(|item| item.label.clone()),
            description: templatedata_parameter.and_then(|item| item.description.clone()),
            param_type: templatedata_parameter.and_then(|item| item.param_type.clone()),
            required: templatedata_parameter
                .map(|item| item.required)
                .unwrap_or(false),
            suggested: templatedata_parameter
                .map(|item| item.suggested)
                .unwrap_or(false),
            deprecated: templatedata_parameter
                .map(|item| item.deprecated)
                .unwrap_or(false),
            usage_count: usage_parameter.map(|item| item.usage_count).unwrap_or(0),
            example_values: usage_parameter
                .map(|item| item.example_values.clone())
                .unwrap_or_default(),
        });
    }
    out
}

fn merge_examples(
    local_examples: Vec<LocalTemplateExample>,
    usage: Option<&crate::knowledge::templates::TemplateUsageSummary>,
) -> Vec<TemplateCatalogExample> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for example in local_examples {
        let key = example.invocation_text.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(TemplateCatalogExample {
                source_kind: "documentation".to_string(),
                source_title: Some(example.source_title),
                source_relative_path: Some(example.source_relative_path),
                parameter_keys: example.parameter_keys,
                invocation_text: example.invocation_text,
                token_estimate: 0,
            });
        }
    }
    if let Some(usage) = usage {
        for example in &usage.example_invocations {
            let key = example.invocation_text.to_ascii_lowercase();
            if seen.insert(key) {
                out.push(TemplateCatalogExample {
                    source_kind: "indexed_usage".to_string(),
                    source_title: Some(example.source_title.clone()),
                    source_relative_path: Some(example.source_relative_path.clone()),
                    parameter_keys: example
                        .parameter_keys
                        .iter()
                        .map(|key| canonical_parameter_key(key))
                        .collect(),
                    invocation_text: example.invocation_text.clone(),
                    token_estimate: example.token_estimate,
                });
            }
        }
    }
    out
}

fn recommendation_tags(template_title: &str, overlay: &ProfileOverlay) -> Vec<String> {
    let mut tags = Vec::new();
    if overlay.authoring.article_quality_template.as_deref() == Some(template_title) {
        tags.push("required_quality_banner".to_string());
    }
    if overlay.authoring.references_template.as_deref() == Some(template_title) {
        tags.push("required_references_template".to_string());
    }
    if overlay
        .citations
        .preferred_templates
        .iter()
        .any(|rule| rule.template_title == template_title)
    {
        tags.push("preferred_citation_template".to_string());
    }
    if overlay
        .remilia
        .infobox_preferences
        .iter()
        .any(|rule| rule.template_title == template_title)
    {
        tags.push("preferred_infobox_template".to_string());
    }
    tags
}

fn merge_titles(left: Vec<String>, right: Option<Vec<String>>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for title in left
        .into_iter()
        .chain(right.unwrap_or_default().into_iter())
    {
        let normalized = title.to_ascii_lowercase();
        if !title.is_empty() && seen.insert(normalized) {
            out.push(title);
        }
    }
    out
}

fn load_local_templates(paths: &ResolvedPaths) -> Result<(LocalTemplateMap, TemplateAliasMap)> {
    let files = scan_files(
        paths,
        &ScanOptions {
            include_content: false,
            include_templates: true,
            custom_content_folders: Vec::new(),
        },
    )?;

    let mut local_templates = LocalTemplateMap::new();
    let mut documentation_pages = BTreeMap::<String, Vec<LocalDocumentationPage>>::new();
    let mut redirect_aliases = TemplateAliasMap::new();
    for file in files {
        if file.namespace != "Template" {
            continue;
        }
        let normalized_title = normalize_template_lookup_title(&file.title);
        if normalized_title.is_empty() {
            continue;
        }
        if file.is_redirect {
            if let Some(target) = file.redirect_target.as_deref() {
                let normalized_target = normalize_template_lookup_title(target);
                if !normalized_target.is_empty() && normalized_target != normalized_title {
                    redirect_aliases
                        .entry(normalized_target)
                        .or_default()
                        .push(file.title);
                }
            }
            continue;
        }
        let full_path = relative_path_to_path(paths, &file.relative_path);
        let content = fs::read_to_string(&full_path)
            .with_context(|| format!("failed to read {}", full_path.display()))?;
        let relative_path = file.relative_path.clone();

        if let Some((base_title, subpage)) = normalized_title.split_once('/') {
            if is_documentation_subpage(subpage) {
                documentation_pages
                    .entry(base_title.to_string())
                    .or_default()
                    .push(LocalDocumentationPage {
                        title: normalized_title,
                        relative_path,
                        content,
                    });
            }
            continue;
        }

        let templatedata = extract_template_data(&content)?;
        let declared_parameter_keys = extract_source_parameters(&content);
        let module_titles = extract_module_references(&content);
        let mut local_examples = extract_template_examples(
            &content,
            &normalized_title,
            &normalized_title,
            &file.relative_path,
            4,
        );
        let summary_text = templatedata
            .as_ref()
            .and_then(|item| item.description.clone())
            .or_else(|| extract_summary_text(&content));
        local_templates.insert(
            normalized_title.clone(),
            LocalTemplateRecord {
                template_title: normalized_title,
                relative_path: relative_path.clone(),
                category: relative_template_category(&relative_path),
                templatedata,
                summary_text,
                declared_parameter_keys,
                documentation_pages: Vec::new(),
                local_examples: std::mem::take(&mut local_examples),
                module_titles,
            },
        );
    }

    for (base_title, pages) in documentation_pages {
        if let Some(entry) = local_templates.get_mut(&base_title) {
            for page in pages {
                if entry.summary_text.is_none() {
                    entry.summary_text = extract_summary_text(&page.content);
                }
                entry.local_examples.extend(extract_template_examples(
                    &page.content,
                    &base_title,
                    &page.title,
                    &page.relative_path,
                    4,
                ));
                entry.documentation_pages.push(page);
            }
        }
    }

    Ok((local_templates, redirect_aliases))
}

fn is_documentation_subpage(subpage: &str) -> bool {
    matches!(
        subpage.to_ascii_lowercase().as_str(),
        "doc" | "documentation"
    )
}

fn relative_template_category(relative_path: &str) -> String {
    let normalized = normalize_path(relative_path);
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.first().copied() == Some("templates") && segments.len() >= 2 {
        return segments[1].to_string();
    }
    if !segments.is_empty() {
        return segments[0].to_string();
    }
    "templates".to_string()
}

fn relative_path_to_path(paths: &ResolvedPaths, relative_path: &str) -> PathBuf {
    let mut path = paths.project_root.clone();
    for segment in normalize_path(relative_path).split('/') {
        if !segment.is_empty() {
            path.push(segment);
        }
    }
    path
}

fn canonical_parameter_key(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(rest) = trimmed.strip_prefix('$')
        && !rest.is_empty()
        && rest.chars().all(|ch| ch.is_ascii_digit())
    {
        return rest.to_string();
    }
    trimmed.to_string()
}

fn store_template_catalog(paths: &ResolvedPaths, catalog: &TemplateCatalog) -> Result<()> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let metadata_json =
        serde_json::to_string_pretty(catalog).context("failed to serialize template catalog")?;
    let built_at_unix = unix_timestamp()?;
    connection
        .execute(
            "INSERT INTO knowledge_artifacts (
                artifact_key,
                artifact_kind,
                profile,
                schema_generation,
                built_at_unix,
                row_count,
                metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(artifact_key) DO UPDATE SET
                artifact_kind = excluded.artifact_kind,
                profile = excluded.profile,
                schema_generation = excluded.schema_generation,
                built_at_unix = excluded.built_at_unix,
                row_count = excluded.row_count,
                metadata_json = excluded.metadata_json",
            params![
                template_catalog_artifact_key(&catalog.profile_id),
                TEMPLATE_CATALOG_ARTIFACT_KIND,
                Some(catalog.profile_id.as_str()),
                KNOWLEDGE_GENERATION,
                i64::try_from(built_at_unix).context("artifact timestamp does not fit into i64")?,
                i64::try_from(catalog.entries.len())
                    .context("artifact row count does not fit into i64")?,
                metadata_json,
            ],
        )
        .with_context(|| {
            format!(
                "failed to store template catalog for {}",
                catalog.profile_id
            )
        })?;

    Ok(())
}

fn template_catalog_artifact_key(profile_id: &str) -> String {
    format!(
        "template_catalog:{}",
        profile_id.trim().to_ascii_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use crate::filesystem::ScanOptions;
    use crate::knowledge::content_index::rebuild_index;
    use crate::runtime::{ResolvedPaths, ValueSource};

    use super::{
        TemplateCatalogEntryLookup, build_template_catalog_with_overlay,
        find_template_catalog_entry,
    };
    use crate::profile::remilia_overlay::build_remilia_profile_overlay;

    fn paths(project_root: &Path) -> ResolvedPaths {
        let state_dir = project_root.join(".wikitool");
        let data_dir = state_dir.join("data");
        fs::create_dir_all(project_root.join("wiki_content/Main")).expect("wiki content");
        fs::create_dir_all(project_root.join("templates")).expect("templates");
        fs::create_dir_all(&data_dir).expect("data");
        fs::create_dir_all(project_root.join("tools/wikitool/ai-pack/llm_instructions"))
            .expect("instructions");
        ResolvedPaths {
            project_root: project_root.to_path_buf(),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir,
            data_dir: data_dir.clone(),
            db_path: data_dir.join("wikitool.db"),
            config_path: project_root.join(".wikitool/config.toml"),
            parser_config_path: project_root.join(".wikitool/parser-config.json"),
            root_source: ValueSource::Default,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, content).expect("write file");
    }

    fn write_instruction_sources(paths: &ResolvedPaths) {
        write_file(
            &paths
                .project_root
                .join("tools/wikitool/ai-pack/llm_instructions/article_structure.md"),
            "{{SHORTDESC:Example}}\n{{Article quality|unverified}}\n== References ==\n{{Reflist}}\nparent_group = Remilia",
        );
        write_file(
            &paths
                .project_root
                .join("tools/wikitool/ai-pack/llm_instructions/style_rules.md"),
            "**Never use:**\n- \"stands as\", \"rich tapestry\"\n### No placeholder content\n- Never output: `[Author Name]`",
        );
        write_file(
            &paths
                .project_root
                .join("tools/wikitool/ai-pack/llm_instructions/writing_guide.md"),
            "raw MediaWiki wikitext\nNever output Markdown\nUse 2-4 categories per article\n[[Category:Remilia]]\n{{Article quality|unverified}}\n### Citation templates\n```wikitext\n{{Cite web|url=}}\n```\n## 6. Infobox selection\n| Subject type | Infobox |\n|---|---|\n| Person | `{{Infobox person}}` |\n| NFT Collection | `{{Infobox NFT collection}}` |\n",
        );
    }

    #[test]
    fn template_catalog_fuses_local_docs_templatedata_and_usage() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "{{Infobox person|name=Alpha|occupation=Writer}}\n'''Alpha''' is a page.",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Template_Infobox_person.wiki"),
            r#"<includeonly>{{#invoke:Infobox|render|name={{{name|}}}|occupation={{{occupation|}}}}}</includeonly><noinclude>
<syntaxhighlight lang="wikitext">
{{Infobox person
| name = Example
| occupation = Writer
}}
</syntaxhighlight>
<templatedata>
{
  "description": "Infobox for biographical articles.",
  "params": {
    "name": {"label": "Name", "required": true},
    "occupation": {"label": "Occupation", "suggested": true}
  }
}
</templatedata>
</noinclude>"#,
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Template_Infobox_person___doc.wiki"),
            "Documentation lead.\n<syntaxhighlight lang=\"wikitext\">\n{{Infobox person|name=Doc example}}\n</syntaxhighlight>",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Module_Infobox.lua"),
            "return {}",
        );
        write_file(
            &paths
                .templates_dir
                .join("redirects")
                .join("Template_Infobox_human.wikitext"),
            "#REDIRECT [[Template:Infobox person]]",
        );

        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
        let overlay = build_remilia_profile_overlay(&paths).expect("overlay");
        let catalog = build_template_catalog_with_overlay(&paths, &overlay).expect("catalog");
        assert!(catalog.usage_index_ready);
        assert_eq!(catalog.template_count, 1);
        let entry = &catalog.entries[0];
        assert_eq!(entry.template_title, "Template:Infobox person");
        assert!(
            entry
                .redirect_aliases
                .contains(&"Template:Infobox human".to_string())
        );
        assert!(
            entry
                .parameters
                .iter()
                .any(|param| param.name == "name" && param.required)
        );
        assert!(
            entry
                .examples
                .iter()
                .any(|example| example.source_kind == "documentation")
        );
        assert!(
            entry
                .recommendation_tags
                .contains(&"preferred_infobox_template".to_string())
        );
    }

    #[test]
    fn template_catalog_lookup_matches_aliases() {
        let catalog = super::TemplateCatalog {
            schema_version: "v1".to_string(),
            profile_id: "remilia".to_string(),
            refreshed_at: "1".to_string(),
            template_count: 1,
            templatedata_count: 0,
            redirect_alias_count: 1,
            usage_index_ready: false,
            entries: vec![super::TemplateCatalogEntry {
                template_title: "Template:Infobox person".to_string(),
                relative_path: "templates/infobox/Template_Infobox_person.wiki".to_string(),
                category: "infobox".to_string(),
                summary_text: None,
                templatedata: None,
                redirect_aliases: vec!["Template:Infobox human".to_string()],
                usage_aliases: Vec::new(),
                usage_count: 0,
                distinct_page_count: 0,
                example_pages: Vec::new(),
                documentation_titles: Vec::new(),
                implementation_titles: Vec::new(),
                implementation_preview: None,
                module_titles: Vec::new(),
                declared_parameter_keys: Vec::new(),
                parameters: Vec::new(),
                examples: Vec::new(),
                recommendation_tags: Vec::new(),
            }],
        };

        match find_template_catalog_entry(&catalog, "Template:Infobox human") {
            TemplateCatalogEntryLookup::Found(entry) => {
                assert_eq!(entry.template_title, "Template:Infobox person");
            }
            other => panic!("expected alias match, got {other:?}"),
        }
    }
}
