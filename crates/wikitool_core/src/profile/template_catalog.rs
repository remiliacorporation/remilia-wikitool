use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::content_store::parsing::{
    estimate_tokens, normalize_spaces, normalize_template_parameter_key, open_indexed_connection,
    rebuild_fts_index, serialize_string_list,
};
use crate::filesystem::{ScanOptions, scan_files};
use crate::knowledge::status::KNOWLEDGE_GENERATION;
use crate::knowledge::templates::{
    load_template_reference_for_connection, normalize_template_lookup_title,
    summarize_template_usage_for_sources,
};
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::table_exists;
use crate::support::{normalize_path, unix_timestamp};

use super::rules::{ProfileOverlay, TemplateCatalogSummary};
use super::template_data::{
    LocalTemplateExample, TemplateDataParameter, TemplateDataRecord, extract_module_references,
    extract_source_parameters, extract_summary_text, extract_template_data,
    extract_template_examples,
};
mod entry;
mod local;
mod model;
mod storage;

use entry::build_catalog_entry;
use local::load_local_templates;
use model::{
    LocalDocumentationPage, LocalTemplateMap, LocalTemplateRecord, LocalUsageAccumulator,
    TEMPLATE_CATALOG_ARTIFACT_KIND, TEMPLATE_CATALOG_SCHEMA_VERSION, TemplateAliasMap,
};
pub use model::{
    TemplateCatalog, TemplateCatalogEntry, TemplateCatalogEntryLookup, TemplateCatalogExample,
    TemplateCatalogParameter,
};
use storage::{
    decode_current_template_catalog, store_template_catalog, template_catalog_artifact_key,
};
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
        .map(|value| decode_current_template_catalog(&value))
        .transpose()
        .map(Option::flatten)
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
        .map(|value| decode_current_template_catalog(&value))
        .transpose()
        .map(Option::flatten)
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
        let mut profile_template_titles = BTreeSet::new();
        for entry in &self.entries {
            if !entry.recommendation_tags.is_empty() {
                profile_template_titles.insert(entry.template_title.clone());
            }
        }
        TemplateCatalogSummary {
            profile_id: self.profile_id.clone(),
            template_count: self.template_count,
            templatedata_count: self.templatedata_count,
            redirect_alias_count: self.redirect_alias_count,
            usage_index_ready: self.usage_index_ready,
            profile_template_titles: profile_template_titles.into_iter().collect(),
            refreshed_at: self.refreshed_at.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
