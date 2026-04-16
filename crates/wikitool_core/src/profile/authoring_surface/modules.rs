use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::content_store::parsing::normalize_spaces;
use crate::filesystem::{ScanOptions, scan_files};
use crate::knowledge::templates::normalize_module_lookup_title;
use crate::runtime::ResolvedPaths;

use super::super::template_catalog::TemplateCatalog;
use super::super::wiki_capabilities::WikiCapabilityManifest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringModuleSurface {
    pub module_title: String,
    pub relative_path: Option<String>,
    pub is_redirect: bool,
    pub redirect_target: Option<String>,
    pub sources: Vec<String>,
    pub used_by_templates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalModuleRecord {
    pub(super) module_title: String,
    pub(super) relative_path: String,
    pub(super) is_redirect: bool,
    pub(super) redirect_target: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ModuleSurfaceAccumulator {
    module_title: String,
    relative_path: Option<String>,
    is_redirect: bool,
    redirect_target: Option<String>,
    sources: BTreeSet<String>,
    used_by_templates: BTreeSet<String>,
}

pub fn normalize_module_title(value: &str) -> String {
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return String::new();
    }
    normalize_module_lookup_title(&normalized)
}

pub fn scan_local_module_titles(paths: &ResolvedPaths) -> Result<BTreeSet<String>> {
    Ok(scan_local_modules(paths)?
        .into_values()
        .map(|module| module.module_title)
        .collect())
}

pub fn supports_invoke_function(capabilities: &WikiCapabilityManifest) -> bool {
    capabilities.has_scribunto
        || capabilities
            .parser_function_hooks
            .iter()
            .any(|hook| hook.eq_ignore_ascii_case("invoke"))
}

pub(super) fn build_module_surfaces(
    catalog: Option<&TemplateCatalog>,
    local_modules: Option<&BTreeMap<String, LocalModuleRecord>>,
    limit: usize,
) -> Vec<AuthoringModuleSurface> {
    let mut modules = BTreeMap::<String, ModuleSurfaceAccumulator>::new();
    if let Some(local_modules) = local_modules {
        for module in local_modules.values() {
            let key = normalize_module_title(&module.module_title);
            if key.is_empty() || is_module_asset_title(&key) {
                continue;
            }
            let entry = modules
                .entry(key.clone())
                .or_insert_with(|| ModuleSurfaceAccumulator {
                    module_title: key,
                    ..ModuleSurfaceAccumulator::default()
                });
            entry.module_title = module.module_title.clone();
            entry.relative_path = Some(module.relative_path.clone());
            entry.is_redirect = module.is_redirect;
            entry.redirect_target = module.redirect_target.clone();
            entry.sources.insert("local_module_file".to_string());
        }
    }
    if let Some(catalog) = catalog {
        for entry in &catalog.entries {
            for module_title in &entry.module_titles {
                let key = normalize_module_title(module_title);
                if key.is_empty() || is_module_asset_title(&key) {
                    continue;
                }
                let module =
                    modules
                        .entry(key.clone())
                        .or_insert_with(|| ModuleSurfaceAccumulator {
                            module_title: key,
                            ..ModuleSurfaceAccumulator::default()
                        });
                module
                    .sources
                    .insert("template_catalog_reference".to_string());
                module
                    .used_by_templates
                    .insert(entry.template_title.clone());
            }
        }
    }
    let mut out = modules
        .into_values()
        .map(|module| AuthoringModuleSurface {
            module_title: module.module_title,
            relative_path: module.relative_path,
            is_redirect: module.is_redirect,
            redirect_target: module.redirect_target,
            sources: module.sources.into_iter().collect(),
            used_by_templates: module.used_by_templates.into_iter().collect(),
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| {
        right
            .used_by_templates
            .len()
            .cmp(&left.used_by_templates.len())
            .then_with(|| left.module_title.cmp(&right.module_title))
    });
    out.truncate(limit);
    out
}

pub(super) fn count_distinct_modules(
    catalog: Option<&TemplateCatalog>,
    local_modules: Option<&BTreeMap<String, LocalModuleRecord>>,
) -> usize {
    let mut modules = BTreeSet::new();
    if let Some(local_modules) = local_modules {
        for title in local_modules.keys() {
            if !is_module_asset_title(title) {
                modules.insert(title.clone());
            }
        }
    }
    if let Some(catalog) = catalog {
        for entry in &catalog.entries {
            for module_title in &entry.module_titles {
                let normalized = normalize_module_title(module_title);
                if !normalized.is_empty() && !is_module_asset_title(&normalized) {
                    modules.insert(normalized);
                }
            }
        }
    }
    modules.len()
}

pub(super) fn scan_local_modules(
    paths: &ResolvedPaths,
) -> Result<BTreeMap<String, LocalModuleRecord>> {
    let files = scan_files(
        paths,
        &ScanOptions {
            include_content: false,
            include_templates: true,
            custom_content_folders: Vec::new(),
        },
    )?;
    let mut modules = BTreeMap::new();
    for file in files {
        if file.namespace != "Module" {
            continue;
        }
        let normalized = normalize_module_title(&file.title);
        if normalized.is_empty() || is_module_asset_title(&normalized) {
            continue;
        }
        modules.insert(
            normalized,
            LocalModuleRecord {
                module_title: file.title,
                relative_path: file.relative_path,
                is_redirect: file.is_redirect,
                redirect_target: file.redirect_target,
            },
        );
    }
    Ok(modules)
}

fn is_module_asset_title(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    lower.ends_with(".css") || lower.ends_with(".js")
}
