use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::catalog::templates::{normalize_module_lookup_title, normalize_template_lookup_title};
use crate::config::load_config;
use crate::content_store::parsing::{extract_template_invocations, extract_transclusion_heads};
use crate::filesystem::{ScanOptions, ScannedFile, scan_files};
use crate::runtime::ResolvedPaths;
use crate::support::{compute_sha256, normalize_path};

use super::template_catalog::{
    TemplateCatalog, TemplateCatalogEntry, TemplateCatalogEntryLookup, TemplateCatalogParameter,
    find_template_catalog_entry,
};
use super::template_data::extract_module_references;
use super::{MagicWordInfo, WikiCapabilityManifest};

const TEMPLATE_DEPENDENCY_CLOSURE_SCHEMA: &str = "template_dependency_closure_v2";
const SCRIBUNTO_RUNTIME_LIBRARIES: [&str; 2] = ["libraryUtil", "strict"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateDependencyClosure {
    pub schema_version: String,
    pub site_adapter_id: String,
    pub catalog_schema_version: String,
    pub roots: Vec<String>,
    pub templates: Vec<TemplateDependencyNode>,
    pub modules: Vec<ModuleDependencyNode>,
    pub files: Vec<TemplateDependencyFile>,
    pub edges: Vec<TemplateDependencyEdge>,
    pub runtime_context: TemplateRuntimeDependencyContext,
    pub runtime_dependencies: Vec<RuntimeProvidedDependency>,
    pub missing: Vec<MissingTemplateDependency>,
    pub unresolved: Vec<UnresolvedTemplateDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateRuntimeDependencyContext {
    pub mediawiki_capabilities_available: bool,
    pub wiki_id: Option<String>,
    pub capability_schema_version: Option<String>,
    pub capability_manifest_sha256: Option<String>,
    pub capability_refreshed_at: Option<String>,
    pub magic_word_count: usize,
    pub parser_function_hook_count: usize,
    pub scribunto_available: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimeProvidedDependency {
    pub title: String,
    pub kind: String,
    pub referenced_by: String,
    pub relation: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateDependencyNode {
    pub template_title: String,
    pub relative_path: String,
    pub content_sha256: String,
    pub root: bool,
    pub summary_text: Option<String>,
    pub usage_count: usize,
    pub distinct_page_count: usize,
    pub parameters: Vec<TemplateCatalogParameter>,
    pub direct_template_dependencies: Vec<String>,
    pub direct_module_dependencies: Vec<String>,
    pub documentation_titles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleDependencyNode {
    pub module_title: String,
    pub relative_path: String,
    pub content_sha256: String,
    pub direct_module_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateDependencyFile {
    pub title: String,
    pub namespace: String,
    pub role: String,
    pub relative_path: String,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TemplateDependencyEdge {
    pub from_title: String,
    pub from_kind: String,
    pub to_title: String,
    pub to_kind: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissingTemplateDependency {
    pub title: String,
    pub kind: String,
    pub referenced_by: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnresolvedTemplateDependency {
    pub referenced_by: String,
    pub kind: String,
    pub relation: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
struct EngineeringFile {
    title: String,
    namespace: String,
    relative_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModuleReference {
    title: String,
    relation: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LuaModuleReferences {
    resolved: Vec<ModuleReference>,
    runtime_libraries: Vec<String>,
    unresolved_calls: Vec<String>,
}

#[derive(Debug)]
struct RuntimeDependencyClassifier<'a> {
    magic_words: &'a [MagicWordInfo],
    parser_function_hooks: BTreeSet<String>,
}

#[derive(Debug)]
struct RuntimeDependencyClassification {
    title: String,
    kind: &'static str,
    relation: &'static str,
    provider: &'static str,
}

impl<'a> RuntimeDependencyClassifier<'a> {
    fn new(capabilities: Option<&'a WikiCapabilityManifest>) -> Self {
        let magic_words = capabilities
            .map(|manifest| manifest.magic_words.as_slice())
            .unwrap_or_default();
        let parser_function_hooks = capabilities
            .into_iter()
            .flat_map(|manifest| manifest.parser_function_hooks.iter())
            .map(|hook| normalize_runtime_token(hook).to_ascii_lowercase())
            .filter(|hook| !hook.is_empty())
            .collect();
        Self {
            magic_words,
            parser_function_hooks,
        }
    }

    fn classify_transclusion_head(&self, head: &str) -> Option<RuntimeDependencyClassification> {
        let trimmed = head.trim();
        if trimmed
            .get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Template:"))
        {
            return None;
        }

        let token = trimmed.split_once(':').map_or(trimmed, |(token, _)| token);
        let normalized = normalize_runtime_token(token);
        if normalized.is_empty() {
            return None;
        }
        let normalized_lower = normalized.to_ascii_lowercase();
        let magic_word = self
            .magic_words
            .iter()
            .find(|word| magic_word_token_matches(word, &normalized));
        let function_alias =
            magic_word.is_some_and(|word| magic_word_has_function_alias(word, &normalized));
        let registered =
            magic_word.is_some() || self.parser_function_hooks.contains(&normalized_lower);
        let parser_function = (token.trim_start().starts_with('#') && registered)
            || function_alias
            || (trimmed.contains(':') && self.parser_function_hooks.contains(&normalized_lower));

        if parser_function {
            let title = magic_word
                .map(|word| normalize_runtime_token(&word.name))
                .filter(|title| !title.is_empty())
                .unwrap_or(normalized);
            return Some(RuntimeDependencyClassification {
                title: format!("#{}", title.trim_start_matches('#')),
                kind: "mediawiki_parser_function",
                relation: "calls_parser_function",
                provider: "mediawiki_siteinfo",
            });
        }

        magic_word.map(|word| RuntimeDependencyClassification {
            title: word.name.clone(),
            kind: "mediawiki_magic_word",
            relation: "expands_magic_word",
            provider: "mediawiki_siteinfo_magicwords",
        })
    }
}

fn normalize_runtime_token(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('#')
        .trim_end_matches(':')
        .trim()
        .replace('_', " ")
}

fn magic_word_token_matches(word: &MagicWordInfo, token: &str) -> bool {
    std::iter::once(word.name.as_str())
        .chain(word.aliases.iter().map(String::as_str))
        .map(normalize_runtime_token)
        .any(|candidate| {
            if word.case_sensitive {
                candidate == token
            } else {
                candidate.eq_ignore_ascii_case(token)
            }
        })
}

fn magic_word_has_function_alias(word: &MagicWordInfo, token: &str) -> bool {
    word.aliases.iter().any(|alias| {
        alias.trim_end().ends_with(':')
            && if word.case_sensitive {
                normalize_runtime_token(alias) == token
            } else {
                normalize_runtime_token(alias).eq_ignore_ascii_case(token)
            }
    })
}

fn unregistered_parser_function_title(head: &str) -> Option<String> {
    let token = head.trim().strip_prefix('#')?.split(':').next()?;
    let normalized = normalize_runtime_token(token);
    (!normalized.is_empty()).then(|| format!("#{normalized}"))
}

pub fn build_template_dependency_closure(
    paths: &ResolvedPaths,
    catalog: &TemplateCatalog,
    requested_roots: &[String],
    max_nodes: usize,
) -> Result<TemplateDependencyClosure> {
    let config = load_config(&paths.config_path).with_context(|| {
        format!(
            "failed to load template closure configuration from {}",
            normalize_path(&paths.config_path)
        )
    })?;
    let capabilities = super::capabilities::load_wiki_capabilities_with_config(paths, &config)?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "wiki capability manifest is missing; sync capabilities before building a template dependency closure"
            )
        })?;
    if capabilities.magic_words.is_empty() {
        bail!(
            "wiki capability manifest lacks MediaWiki magic-word data; refresh capabilities before building a template dependency closure"
        );
    }
    build_template_dependency_closure_with_capabilities(
        paths,
        catalog,
        requested_roots,
        Some(&capabilities),
        max_nodes,
    )
}

pub fn build_template_dependency_closure_with_capabilities(
    paths: &ResolvedPaths,
    catalog: &TemplateCatalog,
    requested_roots: &[String],
    capabilities: Option<&WikiCapabilityManifest>,
    max_nodes: usize,
) -> Result<TemplateDependencyClosure> {
    if requested_roots.is_empty() {
        bail!("template dependency closure requires at least one named template");
    }
    if max_nodes == 0 {
        bail!("template dependency closure requires max_nodes >= 1");
    }

    let inventory = load_engineering_inventory(paths)?;
    let runtime_context = build_runtime_dependency_context(capabilities)?;
    let runtime_classifier = RuntimeDependencyClassifier::new(capabilities);
    let mut roots = Vec::new();
    let mut template_queue = VecDeque::new();
    let mut missing = BTreeSet::new();
    for requested in requested_roots {
        match find_template_catalog_entry(catalog, requested) {
            TemplateCatalogEntryLookup::Found(entry) => {
                if !roots.contains(&entry.template_title) {
                    roots.push(entry.template_title.clone());
                    template_queue.push_back(entry.template_title.clone());
                }
            }
            TemplateCatalogEntryLookup::TemplateMissing { template_title } => {
                missing.insert(MissingTemplateDependency {
                    title: template_title,
                    kind: "template".to_string(),
                    referenced_by: "<root>".to_string(),
                    reason: "template_catalog_entry_missing".to_string(),
                });
            }
            TemplateCatalogEntryLookup::CatalogMissing => {
                bail!("template catalog is missing");
            }
        }
    }
    roots.sort();

    let root_set = roots.iter().cloned().collect::<BTreeSet<_>>();
    let mut seen_templates = BTreeSet::new();
    let mut seen_modules = BTreeSet::new();
    let mut module_queue = VecDeque::new();
    let mut templates = Vec::new();
    let mut modules = Vec::new();
    let mut files = BTreeMap::<String, TemplateDependencyFile>::new();
    let mut edges = BTreeSet::new();
    let mut runtime_dependencies = BTreeSet::new();
    let mut unresolved = BTreeSet::new();

    while let Some(template_title) = template_queue.pop_front() {
        if !seen_templates.insert(template_title.clone()) {
            continue;
        }
        enforce_node_limit(seen_templates.len(), seen_modules.len(), max_nodes)?;
        let Some(entry) = catalog_entry(catalog, &template_title) else {
            missing.insert(MissingTemplateDependency {
                title: template_title,
                kind: "template".to_string(),
                referenced_by: "<closure>".to_string(),
                reason: "template_catalog_entry_missing".to_string(),
            });
            continue;
        };
        let template_file_key = inventory_key("Template", &entry.template_title);
        let Some(file) = inventory.get(&template_file_key) else {
            missing.insert(MissingTemplateDependency {
                title: entry.template_title.clone(),
                kind: "template".to_string(),
                referenced_by: entry.template_title.clone(),
                reason: "local_implementation_file_missing".to_string(),
            });
            continue;
        };
        let content = read_engineering_file(paths, file)?;
        let content_sha256 = compute_sha256(&content);
        let runtime_source = transcluded_template_source(&content);

        let mut runtime_template_titles = BTreeSet::new();
        for head in extract_transclusion_heads(&runtime_source) {
            let Some(classification) = runtime_classifier.classify_transclusion_head(&head) else {
                if let Some(title) = unregistered_parser_function_title(&head) {
                    unresolved.insert(UnresolvedTemplateDependency {
                        referenced_by: entry.template_title.clone(),
                        kind: "mediawiki_parser_function".to_string(),
                        relation: "runtime_dependency_lookup".to_string(),
                        reason: format!("parser_function_not_in_capability_manifest:{title}"),
                    });
                }
                continue;
            };
            if !head.contains(':') && !head.starts_with('#') {
                runtime_template_titles.insert(normalize_template_lookup_title(&head));
            }
            add_runtime_dependency(
                &mut runtime_dependencies,
                &mut edges,
                &entry.template_title,
                "template",
                classification,
            );
        }

        let mut direct_template_dependencies = BTreeSet::new();
        for invocation in extract_template_invocations(&runtime_source) {
            if runtime_template_titles.contains(&invocation.template_title) {
                continue;
            }
            let dependency = resolve_template_title(catalog, &invocation.template_title)
                .unwrap_or(invocation.template_title);
            if dependency == entry.template_title {
                continue;
            }
            direct_template_dependencies.insert(dependency.clone());
            edges.insert(TemplateDependencyEdge {
                from_title: entry.template_title.clone(),
                from_kind: "template".to_string(),
                to_title: dependency.clone(),
                to_kind: "template".to_string(),
                relation: "transcludes".to_string(),
            });
            if catalog_entry(catalog, &dependency).is_some() {
                template_queue.push_back(dependency);
            } else {
                missing.insert(MissingTemplateDependency {
                    title: dependency,
                    kind: "template".to_string(),
                    referenced_by: entry.template_title.clone(),
                    reason: "template_catalog_entry_missing".to_string(),
                });
            }
        }

        let mut direct_module_dependencies = entry
            .module_titles
            .iter()
            .map(|title| normalize_module_lookup_title(title))
            .filter(|title| !title.is_empty())
            .collect::<BTreeSet<_>>();
        for title in extract_module_references(&runtime_source) {
            let normalized = normalize_module_lookup_title(&title);
            if !normalized.is_empty() {
                direct_module_dependencies.insert(normalized);
            }
        }
        for dependency in &direct_module_dependencies {
            edges.insert(TemplateDependencyEdge {
                from_title: entry.template_title.clone(),
                from_kind: "template".to_string(),
                to_title: dependency.clone(),
                to_kind: "module".to_string(),
                relation: "uses_module".to_string(),
            });
            module_queue.push_back((dependency.clone(), entry.template_title.clone()));
        }

        add_dependency_file(&mut files, file, "template", &content_sha256);
        for documentation_title in &entry.documentation_titles {
            let key = inventory_key("Template", documentation_title);
            match inventory.get(&key) {
                Some(documentation_file) => {
                    let documentation_content = read_engineering_file(paths, documentation_file)?;
                    add_dependency_file(
                        &mut files,
                        documentation_file,
                        "documentation",
                        &compute_sha256(&documentation_content),
                    );
                    edges.insert(TemplateDependencyEdge {
                        from_title: entry.template_title.clone(),
                        from_kind: "template".to_string(),
                        to_title: documentation_title.clone(),
                        to_kind: "documentation".to_string(),
                        relation: "documented_by".to_string(),
                    });
                }
                None => {
                    missing.insert(MissingTemplateDependency {
                        title: documentation_title.clone(),
                        kind: "documentation".to_string(),
                        referenced_by: entry.template_title.clone(),
                        reason: "local_documentation_file_missing".to_string(),
                    });
                }
            }
        }

        templates.push(TemplateDependencyNode {
            template_title: entry.template_title.clone(),
            relative_path: file.relative_path.clone(),
            content_sha256,
            root: root_set.contains(&entry.template_title),
            summary_text: entry.summary_text.clone(),
            usage_count: entry.usage_count,
            distinct_page_count: entry.distinct_page_count,
            parameters: entry.parameters.clone(),
            direct_template_dependencies: direct_template_dependencies.into_iter().collect(),
            direct_module_dependencies: direct_module_dependencies.into_iter().collect(),
            documentation_titles: entry.documentation_titles.clone(),
        });
    }

    while let Some((module_title, referenced_by)) = module_queue.pop_front() {
        if !seen_modules.insert(module_title.clone()) {
            continue;
        }
        enforce_node_limit(seen_templates.len(), seen_modules.len(), max_nodes)?;
        let key = inventory_key("Module", &module_title);
        let Some(file) = inventory.get(&key) else {
            missing.insert(MissingTemplateDependency {
                title: module_title,
                kind: "module".to_string(),
                referenced_by,
                reason: "local_module_file_missing".to_string(),
            });
            continue;
        };
        let content = read_engineering_file(paths, file)?;
        let content_sha256 = compute_sha256(&content);
        let references = extract_lua_module_references(&content);
        for runtime_library in references.runtime_libraries {
            add_runtime_dependency(
                &mut runtime_dependencies,
                &mut edges,
                &module_title,
                "module",
                RuntimeDependencyClassification {
                    title: runtime_library,
                    kind: "scribunto_runtime_library",
                    relation: "requires_runtime_library",
                    provider: "scribunto_runtime",
                },
            );
        }
        for detail in references.unresolved_calls {
            unresolved.insert(UnresolvedTemplateDependency {
                referenced_by: module_title.clone(),
                kind: "module".to_string(),
                relation: "dynamic_module_load".to_string(),
                reason: detail,
            });
        }
        let mut direct_dependencies = BTreeSet::new();
        for reference in references.resolved {
            if reference.title == module_title {
                continue;
            }
            direct_dependencies.insert(reference.title.clone());
            edges.insert(TemplateDependencyEdge {
                from_title: module_title.clone(),
                from_kind: "module".to_string(),
                to_title: reference.title.clone(),
                to_kind: "module".to_string(),
                relation: reference.relation,
            });
            module_queue.push_back((reference.title, module_title.clone()));
        }
        add_dependency_file(&mut files, file, "module", &content_sha256);
        modules.push(ModuleDependencyNode {
            module_title,
            relative_path: file.relative_path.clone(),
            content_sha256,
            direct_module_dependencies: direct_dependencies.into_iter().collect(),
        });
    }

    templates.sort_by(|left, right| left.template_title.cmp(&right.template_title));
    modules.sort_by(|left, right| left.module_title.cmp(&right.module_title));

    Ok(TemplateDependencyClosure {
        schema_version: TEMPLATE_DEPENDENCY_CLOSURE_SCHEMA.to_string(),
        site_adapter_id: catalog.site_adapter_id.clone(),
        catalog_schema_version: catalog.schema_version.clone(),
        roots,
        templates,
        modules,
        files: files.into_values().collect(),
        edges: edges.into_iter().collect(),
        runtime_context,
        runtime_dependencies: runtime_dependencies.into_iter().collect(),
        missing: missing.into_iter().collect(),
        unresolved: unresolved.into_iter().collect(),
    })
}

fn build_runtime_dependency_context(
    capabilities: Option<&WikiCapabilityManifest>,
) -> Result<TemplateRuntimeDependencyContext> {
    let Some(manifest) = capabilities else {
        return Ok(TemplateRuntimeDependencyContext {
            mediawiki_capabilities_available: false,
            wiki_id: None,
            capability_schema_version: None,
            capability_manifest_sha256: None,
            capability_refreshed_at: None,
            magic_word_count: 0,
            parser_function_hook_count: 0,
            scribunto_available: None,
        });
    };
    let encoded = serde_json::to_string(manifest)
        .context("failed to serialize wiki capabilities for template closure provenance")?;
    Ok(TemplateRuntimeDependencyContext {
        mediawiki_capabilities_available: true,
        wiki_id: Some(manifest.wiki_id.clone()),
        capability_schema_version: Some(manifest.schema_version.clone()),
        capability_manifest_sha256: Some(compute_sha256(&encoded)),
        capability_refreshed_at: Some(manifest.refreshed_at.clone()),
        magic_word_count: manifest.magic_words.len(),
        parser_function_hook_count: manifest.parser_function_hooks.len(),
        scribunto_available: Some(manifest.has_scribunto),
    })
}

fn add_runtime_dependency(
    runtime_dependencies: &mut BTreeSet<RuntimeProvidedDependency>,
    edges: &mut BTreeSet<TemplateDependencyEdge>,
    referenced_by: &str,
    referenced_by_kind: &str,
    classification: RuntimeDependencyClassification,
) {
    runtime_dependencies.insert(RuntimeProvidedDependency {
        title: classification.title.clone(),
        kind: classification.kind.to_string(),
        referenced_by: referenced_by.to_string(),
        relation: classification.relation.to_string(),
        provider: classification.provider.to_string(),
    });
    edges.insert(TemplateDependencyEdge {
        from_title: referenced_by.to_string(),
        from_kind: referenced_by_kind.to_string(),
        to_title: classification.title,
        to_kind: classification.kind.to_string(),
        relation: classification.relation.to_string(),
    });
}

fn load_engineering_inventory(paths: &ResolvedPaths) -> Result<BTreeMap<String, EngineeringFile>> {
    let scanned = scan_files(
        paths,
        &ScanOptions {
            include_content: false,
            include_templates: true,
            custom_content_folders: Vec::new(),
        },
    )?;
    let mut inventory = BTreeMap::new();
    for file in scanned {
        if file.is_redirect || !matches!(file.namespace.as_str(), "Template" | "Module") {
            continue;
        }
        let key = inventory_key(&file.namespace, &file.title);
        inventory.insert(key, engineering_file(file));
    }
    Ok(inventory)
}

fn engineering_file(file: ScannedFile) -> EngineeringFile {
    EngineeringFile {
        title: file.title,
        namespace: file.namespace,
        relative_path: file.relative_path,
    }
}

fn inventory_key(namespace: &str, title: &str) -> String {
    let normalized = if namespace == "Module" {
        normalize_module_lookup_title(title)
    } else {
        normalize_template_lookup_title(title)
    };
    format!(
        "{}:{}",
        namespace.to_ascii_lowercase(),
        normalized.to_ascii_lowercase()
    )
}

fn catalog_entry<'a>(
    catalog: &'a TemplateCatalog,
    template_title: &str,
) -> Option<&'a TemplateCatalogEntry> {
    let normalized = normalize_template_lookup_title(template_title);
    catalog
        .entries
        .iter()
        .find(|entry| normalize_template_lookup_title(&entry.template_title) == normalized)
}

fn resolve_template_title(catalog: &TemplateCatalog, title: &str) -> Option<String> {
    match find_template_catalog_entry(catalog, title) {
        TemplateCatalogEntryLookup::Found(entry) => Some(entry.template_title.clone()),
        TemplateCatalogEntryLookup::CatalogMissing
        | TemplateCatalogEntryLookup::TemplateMissing { .. } => None,
    }
}

fn enforce_node_limit(template_count: usize, module_count: usize, max_nodes: usize) -> Result<()> {
    let count = template_count.saturating_add(module_count);
    if count > max_nodes {
        bail!(
            "template dependency closure exceeded max_nodes={max_nodes} after resolving {count} nodes; raise the explicit limit only after inspecting the requested roots"
        );
    }
    Ok(())
}

fn add_dependency_file(
    files: &mut BTreeMap<String, TemplateDependencyFile>,
    file: &EngineeringFile,
    role: &str,
    content_sha256: &str,
) {
    files
        .entry(file.relative_path.clone())
        .or_insert_with(|| TemplateDependencyFile {
            title: file.title.clone(),
            namespace: file.namespace.clone(),
            role: role.to_string(),
            relative_path: file.relative_path.clone(),
            content_sha256: content_sha256.to_string(),
        });
}

fn read_engineering_file(paths: &ResolvedPaths, file: &EngineeringFile) -> Result<String> {
    let path = relative_path_to_path(&paths.project_root, &file.relative_path);
    fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read template dependency {}",
            normalize_path(&path)
        )
    })
}

fn relative_path_to_path(project_root: &Path, relative_path: &str) -> PathBuf {
    let mut path = project_root.to_path_buf();
    for segment in normalize_path(relative_path).split('/') {
        if !segment.is_empty() {
            path.push(segment);
        }
    }
    path
}

pub(super) fn transcluded_template_source(content: &str) -> String {
    let onlyinclude = collect_tag_bodies(content, "onlyinclude");
    if !onlyinclude.is_empty() {
        return onlyinclude.join("\n");
    }
    strip_template_nontranscluded_regions(content)
}

fn collect_tag_bodies(content: &str, tag_name: &str) -> Vec<String> {
    let lower = content.to_ascii_lowercase();
    let open_prefix = format!("<{}", tag_name.to_ascii_lowercase());
    let close = format!("</{}>", tag_name.to_ascii_lowercase());
    let mut bodies = Vec::new();
    let mut cursor = 0usize;
    while let Some(open_relative) = lower[cursor..].find(&open_prefix) {
        let open_start = cursor + open_relative;
        let Some(open_end_relative) = lower[open_start..].find('>') else {
            break;
        };
        let body_start = open_start + open_end_relative + 1;
        let Some(close_relative) = lower[body_start..].find(&close) else {
            break;
        };
        let body_end = body_start + close_relative;
        bodies.push(content[body_start..body_end].to_string());
        cursor = body_end + close.len();
    }
    bodies
}

fn strip_template_nontranscluded_regions(content: &str) -> String {
    let lower = content.to_ascii_lowercase();
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    while cursor < content.len() {
        if lower[cursor..].starts_with("<!--") {
            cursor = lower[cursor + 4..]
                .find("-->")
                .map(|offset| cursor + 4 + offset + 3)
                .unwrap_or(content.len());
            continue;
        }
        if lower[cursor..].starts_with("<noinclude") {
            let Some(open_end_relative) = lower[cursor..].find('>') else {
                break;
            };
            let open_end = cursor + open_end_relative + 1;
            if lower[cursor..open_end].trim_end().ends_with("/>") {
                cursor = open_end;
                continue;
            }
            cursor = lower[open_end..]
                .find("</noinclude>")
                .map(|offset| open_end + offset + "</noinclude>".len())
                .unwrap_or(content.len());
            continue;
        }
        if lower[cursor..].starts_with("<includeonly")
            || lower[cursor..].starts_with("</includeonly")
        {
            cursor = lower[cursor..]
                .find('>')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(content.len());
            continue;
        }
        let ch = content[cursor..]
            .chars()
            .next()
            .expect("cursor is inside content");
        out.push(ch);
        cursor += ch.len_utf8();
    }
    out
}

fn extract_lua_module_references(content: &str) -> LuaModuleReferences {
    let mut out = LuaModuleReferences::default();
    let mut seen = BTreeSet::new();
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"--") {
            if let Some(end) = skip_lua_long_bracket(bytes, cursor + 2) {
                cursor = end;
                continue;
            }
            cursor = bytes[cursor..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(bytes.len());
            continue;
        }
        if matches!(bytes[cursor], b'\'' | b'"') {
            cursor = skip_lua_string(bytes, cursor).unwrap_or(bytes.len());
            continue;
        }
        if bytes[cursor] == b'['
            && let Some(end) = skip_lua_long_bracket(bytes, cursor)
        {
            cursor = end;
            continue;
        }
        let (token, relation) = if token_at(bytes, cursor, b"require") {
            (b"require".as_slice(), "requires")
        } else if token_at(bytes, cursor, b"mw.loadData") {
            (b"mw.loadData".as_slice(), "loads_data")
        } else {
            cursor += 1;
            continue;
        };
        let mut argument = cursor + token.len();
        while argument < bytes.len() && bytes[argument].is_ascii_whitespace() {
            argument += 1;
        }
        let parenthesized = argument < bytes.len() && bytes[argument] == b'(';
        if parenthesized {
            argument += 1;
            while argument < bytes.len() && bytes[argument].is_ascii_whitespace() {
                argument += 1;
            }
        }
        let Some((value_start, value_end, call_end)) = parse_lua_string_literal(bytes, argument)
        else {
            if parenthesized {
                out.unresolved_calls
                    .push(format!("{relation}_argument_is_not_a_string_literal"));
            }
            cursor = argument.saturating_add(1);
            continue;
        };
        let raw = &content[value_start..value_end];
        let normalized = normalize_module_lookup_title(raw);
        if raw
            .trim()
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Module:"))
        {
            let signature = format!("{relation}\0{}", normalized.to_ascii_lowercase());
            if seen.insert(signature) {
                out.resolved.push(ModuleReference {
                    title: normalized,
                    relation: relation.to_string(),
                });
            }
        } else if relation == "requires" && SCRIBUNTO_RUNTIME_LIBRARIES.contains(&raw.trim()) {
            out.runtime_libraries.push(raw.trim().to_string());
        } else {
            out.unresolved_calls
                .push(format!("{relation}_literal_is_not_a_module_title:{raw}"));
        }
        cursor = call_end;
    }
    out.resolved.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.relation.cmp(&right.relation))
    });
    out.runtime_libraries.sort();
    out.runtime_libraries.dedup();
    out.unresolved_calls.sort();
    out.unresolved_calls.dedup();
    out
}

fn token_at(content: &[u8], cursor: usize, token: &[u8]) -> bool {
    if !content[cursor..].starts_with(token) {
        return false;
    }
    let before_is_identifier = cursor > 0 && is_lua_identifier_byte(content[cursor - 1]);
    let after = cursor + token.len();
    let after_is_identifier = after < content.len() && is_lua_identifier_byte(content[after]);
    !before_is_identifier && !after_is_identifier
}

fn is_lua_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn skip_lua_string(content: &[u8], start: usize) -> Option<usize> {
    let quote = *content.get(start)?;
    find_lua_string_end(content, start + 1, quote).map(|end| end + 1)
}

fn parse_lua_string_literal(content: &[u8], start: usize) -> Option<(usize, usize, usize)> {
    let first = *content.get(start)?;
    if matches!(first, b'\'' | b'"') {
        let value_start = start + 1;
        let value_end = find_lua_string_end(content, value_start, first)?;
        return Some((value_start, value_end, value_end + 1));
    }
    let (value_start, equals_count) = lua_long_bracket_open(content, start)?;
    let (value_end, call_end) = find_lua_long_bracket_end(content, value_start, equals_count)?;
    Some((value_start, value_end, call_end))
}

fn skip_lua_long_bracket(content: &[u8], start: usize) -> Option<usize> {
    let (value_start, equals_count) = lua_long_bracket_open(content, start)?;
    find_lua_long_bracket_end(content, value_start, equals_count).map(|(_, end)| end)
}

fn lua_long_bracket_open(content: &[u8], start: usize) -> Option<(usize, usize)> {
    if *content.get(start)? != b'[' {
        return None;
    }
    let mut cursor = start + 1;
    while content.get(cursor).is_some_and(|byte| *byte == b'=') {
        cursor += 1;
    }
    if *content.get(cursor)? != b'[' {
        return None;
    }
    Some((cursor + 1, cursor - start - 1))
}

fn find_lua_long_bracket_end(
    content: &[u8],
    mut cursor: usize,
    equals_count: usize,
) -> Option<(usize, usize)> {
    while cursor < content.len() {
        if content[cursor] == b']' {
            let mut closing = cursor + 1;
            let mut matched_equals = 0usize;
            while matched_equals < equals_count
                && content.get(closing).is_some_and(|byte| *byte == b'=')
            {
                matched_equals += 1;
                closing += 1;
            }
            if matched_equals == equals_count && content.get(closing) == Some(&b']') {
                return Some((cursor, closing + 1));
            }
        }
        cursor += 1;
    }
    None
}

fn find_lua_string_end(content: &[u8], mut cursor: usize, quote: u8) -> Option<usize> {
    while cursor < content.len() {
        if content[cursor] == b'\\' {
            cursor = cursor.saturating_add(2);
            continue;
        }
        if content[cursor] == quote {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        build_template_dependency_closure, build_template_dependency_closure_with_capabilities,
        extract_lua_module_references, transcluded_template_source,
    };
    use crate::runtime::{ResolvedPaths, ValueSource};
    use crate::site::{
        MagicWordInfo, TemplateCatalog, TemplateCatalogEntry, WikiCapabilityManifest,
    };

    fn paths(project_root: &Path) -> ResolvedPaths {
        let state_dir = project_root.join(".wikitool");
        let data_dir = state_dir.join("data");
        fs::create_dir_all(project_root.join("wiki_content/Main")).expect("wiki content");
        fs::create_dir_all(project_root.join("templates")).expect("templates");
        fs::create_dir_all(&data_dir).expect("data");
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
        fs::write(path, content).expect("write fixture");
    }

    fn catalog_entry(
        template_title: &str,
        relative_path: &str,
        documentation_titles: Vec<String>,
    ) -> TemplateCatalogEntry {
        TemplateCatalogEntry {
            template_title: template_title.to_string(),
            relative_path: relative_path.to_string(),
            category: "test".to_string(),
            summary_text: Some(format!("Contract for {template_title}")),
            templatedata: None,
            redirect_aliases: Vec::new(),
            usage_aliases: Vec::new(),
            usage_count: 0,
            distinct_page_count: 0,
            example_pages: Vec::new(),
            documentation_titles,
            implementation_titles: vec![template_title.to_string()],
            implementation_preview: None,
            module_titles: Vec::new(),
            declared_parameter_keys: Vec::new(),
            parameters: Vec::new(),
            examples: Vec::new(),
            recommendation_tags: Vec::new(),
        }
    }

    fn capabilities() -> WikiCapabilityManifest {
        WikiCapabilityManifest {
            schema_version: "wiki_capabilities_v2".to_string(),
            wiki_id: "test.example".to_string(),
            wiki_url: "https://test.example".to_string(),
            api_url: "https://test.example/api.php".to_string(),
            rest_url: None,
            article_path: "/wiki/$1".to_string(),
            mediawiki_version: Some("1.44.0".to_string()),
            namespaces: Vec::new(),
            extensions: Vec::new(),
            parser_extension_tags: Vec::new(),
            parser_function_hooks: vec!["if".to_string(), "invoke".to_string(), "lc".to_string()],
            magic_words: vec![
                MagicWordInfo {
                    name: "if".to_string(),
                    aliases: vec!["if".to_string()],
                    case_sensitive: false,
                },
                MagicWordInfo {
                    name: "namespace".to_string(),
                    aliases: vec!["NAMESPACE".to_string()],
                    case_sensitive: true,
                },
                MagicWordInfo {
                    name: "lc".to_string(),
                    aliases: vec!["LC:".to_string()],
                    case_sensitive: false,
                },
            ],
            special_pages: Vec::new(),
            search_backend_hint: None,
            has_visual_editor: false,
            has_templatedata: false,
            has_citoid: false,
            has_cargo: false,
            has_page_forms: false,
            has_short_description: false,
            has_scribunto: true,
            has_timed_media_handler: false,
            supports_parse_api_html: true,
            supports_rest_html: false,
            rest_html_path_template: None,
            refreshed_at: "1".to_string(),
        }
    }

    #[test]
    fn transcluded_source_excludes_documentation_examples() {
        let source = r#"<includeonly>{{Runtime helper}}</includeonly><noinclude>
{{Documentation-only example}}
<templatedata>{"description":"{{not runtime}}"}</templatedata>
</noinclude>"#;
        let transcluded = transcluded_template_source(source);
        assert!(transcluded.contains("{{Runtime helper}}"));
        assert!(!transcluded.contains("Documentation-only example"));
        assert!(!transcluded.contains("not runtime"));

        let onlyinclude = transcluded_template_source(
            "{{Outside}}<onlyinclude>{{Inside one}}</onlyinclude><onlyinclude>{{Inside two}}</onlyinclude>",
        );
        assert_eq!(onlyinclude, "{{Inside one}}\n{{Inside two}}");
    }

    #[test]
    fn lua_dependencies_are_literal_and_comment_aware() {
        let references = extract_lua_module_references(
            r#"
local args = require('Module:Arguments')
local data = mw.loadData("Module:Infobox/data")
local direct = require 'Module:Direct'
local long_data = mw.loadData [=[Module:Long data]=]
-- require('Module:Comment')
--[=[ require('Module:Long comment') ]=]
local text = "require('Module:String')"
local long_text = [=[require('Module:Long string')]=]
local dynamic = require(module_name)
local builtin = require('strict')
local utility = require('libraryUtil')
local unknown = require('notBundled')
local function_reference = require
"#,
        );
        assert_eq!(references.resolved.len(), 4);
        assert_eq!(references.resolved[0].title, "Module:Arguments");
        assert_eq!(references.resolved[0].relation, "requires");
        assert!(references.resolved.iter().any(
            |reference| reference.title == "Module:Direct" && reference.relation == "requires"
        ));
        assert!(
            references
                .resolved
                .iter()
                .any(|reference| reference.title == "Module:Infobox/data"
                    && reference.relation == "loads_data")
        );
        assert!(
            references
                .resolved
                .iter()
                .any(|reference| reference.title == "Module:Long data"
                    && reference.relation == "loads_data")
        );
        assert_eq!(
            references.runtime_libraries,
            vec!["libraryUtil".to_string(), "strict".to_string()]
        );
        assert_eq!(
            references.unresolved_calls,
            vec![
                "requires_argument_is_not_a_string_literal".to_string(),
                "requires_literal_is_not_a_module_title:notBundled".to_string(),
            ]
        );
    }

    #[test]
    fn closure_separates_runtime_primitives_from_missing_and_dynamic_dependencies() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_file(
            &paths.templates_dir.join("core/Template_Root.wiki"),
            "<includeonly>{{NAMESPACE}}{{#if:x|yes|no}}{{lc:VALUE}}{{#notRegistered:x}}{{Missing helper}}{{#invoke:Root|main}}</includeonly>",
        );
        write_file(
            &paths.templates_dir.join("core/Module_Root.lua"),
            "local strict = require('strict')\nlocal util = require('libraryUtil')\nlocal missing = require('Module:Missing')\nlocal dynamic = mw.loadData(data_name)\nreturn {}",
        );
        let catalog = TemplateCatalog {
            schema_version: "template_catalog_v4".to_string(),
            site_adapter_id: "test".to_string(),
            refreshed_at: "1".to_string(),
            template_count: 1,
            templatedata_count: 0,
            redirect_alias_count: 0,
            usage_index_ready: false,
            entries: vec![catalog_entry(
                "Template:Root",
                "templates/core/Template_Root.wiki",
                Vec::new(),
            )],
        };

        let closure = build_template_dependency_closure_with_capabilities(
            &paths,
            &catalog,
            &["Root".to_string()],
            Some(&capabilities()),
            16,
        )
        .expect("dependency closure");

        assert_eq!(closure.schema_version, "template_dependency_closure_v2");
        assert!(closure.runtime_context.mediawiki_capabilities_available);
        assert_eq!(closure.runtime_dependencies.len(), 6);
        assert!(closure.runtime_dependencies.iter().any(|dependency| {
            dependency.kind == "mediawiki_magic_word" && dependency.title == "namespace"
        }));
        assert!(closure.runtime_dependencies.iter().any(|dependency| {
            dependency.kind == "mediawiki_parser_function" && dependency.title == "#if"
        }));
        assert!(closure.runtime_dependencies.iter().any(|dependency| {
            dependency.kind == "mediawiki_parser_function" && dependency.title == "#lc"
        }));
        assert!(closure.runtime_dependencies.iter().any(|dependency| {
            dependency.kind == "scribunto_runtime_library" && dependency.title == "strict"
        }));
        assert!(closure.missing.iter().any(|dependency| {
            dependency.kind == "template" && dependency.title == "Template:Missing helper"
        }));
        assert!(closure.missing.iter().any(|dependency| {
            dependency.kind == "module" && dependency.title == "Module:Missing"
        }));
        assert!(closure.missing.iter().all(|dependency| {
            dependency.title != "Template:NAMESPACE"
                && dependency.title != "Module:strict"
                && dependency.title != "Module:libraryUtil"
        }));
        assert_eq!(closure.unresolved.len(), 2);
        assert!(closure.unresolved.iter().any(|dependency| {
            dependency.relation == "dynamic_module_load"
                && dependency.reason == "loads_data_argument_is_not_a_string_literal"
        }));
        assert!(closure.unresolved.iter().any(|dependency| {
            dependency.kind == "mediawiki_parser_function"
                && dependency.relation == "runtime_dependency_lookup"
                && dependency.reason == "parser_function_not_in_capability_manifest:#notRegistered"
        }));
    }

    #[test]
    fn named_closure_is_transitive_hash_bound_and_excludes_doc_examples() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_file(
            &paths.templates_dir.join("core/Template_Root.wiki"),
            "<includeonly>{{Helper}}{{#invoke:Root|main}}</includeonly><noinclude>{{Noise}}</noinclude>",
        );
        write_file(
            &paths.templates_dir.join("core/Template_Root___doc.wiki"),
            "{{Noise}} is documentation only.",
        );
        write_file(
            &paths.templates_dir.join("core/Template_Helper.wiki"),
            "<includeonly>helper</includeonly>",
        );
        write_file(
            &paths.templates_dir.join("core/Module_Root.lua"),
            "local args = require('Module:Arguments')\nlocal other = require(name)\nreturn {}",
        );
        write_file(
            &paths.templates_dir.join("core/Module_Arguments.lua"),
            "return {}",
        );
        let catalog = TemplateCatalog {
            schema_version: "template_catalog_v4".to_string(),
            site_adapter_id: "test".to_string(),
            refreshed_at: "1".to_string(),
            template_count: 2,
            templatedata_count: 0,
            redirect_alias_count: 0,
            usage_index_ready: false,
            entries: vec![
                catalog_entry(
                    "Template:Root",
                    "templates/core/Template_Root.wiki",
                    vec!["Template:Root/doc".to_string()],
                ),
                catalog_entry(
                    "Template:Helper",
                    "templates/core/Template_Helper.wiki",
                    Vec::new(),
                ),
            ],
        };
        let capability_manifest = capabilities();

        let closure = build_template_dependency_closure_with_capabilities(
            &paths,
            &catalog,
            &["Root".to_string()],
            Some(&capability_manifest),
            16,
        )
        .expect("dependency closure");
        assert_eq!(closure.roots, vec!["Template:Root".to_string()]);
        assert_eq!(closure.templates.len(), 2);
        assert_eq!(closure.modules.len(), 2);
        assert_eq!(closure.files.len(), 5);
        assert!(closure.missing.is_empty());
        assert_eq!(closure.unresolved.len(), 1);
        assert!(
            closure
                .edges
                .iter()
                .any(|edge| edge.from_title == "Template:Root"
                    && edge.to_title == "Template:Helper"
                    && edge.relation == "transcludes")
        );
        assert!(
            closure
                .edges
                .iter()
                .all(|edge| edge.to_title != "Template:Noise")
        );
        assert!(
            closure
                .files
                .iter()
                .all(|file| file.content_sha256.len() == 64)
        );

        let bounded_error = build_template_dependency_closure_with_capabilities(
            &paths,
            &catalog,
            &["Root".to_string()],
            Some(&capability_manifest),
            3,
        )
        .expect_err("closure must fail rather than silently truncate");
        assert!(bounded_error.to_string().contains("exceeded max_nodes=3"));

        let missing = build_template_dependency_closure_with_capabilities(
            &paths,
            &catalog,
            &["Template:Unknown".to_string()],
            Some(&capability_manifest),
            16,
        )
        .expect("missing roots remain explicit evidence");
        assert!(missing.roots.is_empty());
        assert_eq!(missing.missing.len(), 1);
        assert_eq!(missing.missing[0].title, "Template:Unknown");
        assert_eq!(missing.missing[0].referenced_by, "<root>");

        let capability_error =
            build_template_dependency_closure(&paths, &catalog, &["Root".to_string()], 16)
                .expect_err("the compatibility entrypoint must not emit an unclassified closure");
        assert!(
            capability_error
                .to_string()
                .contains("wiki capability manifest is missing")
        );
    }
}
