use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::WikiConfig;
use crate::content_store::parsing::{normalize_spaces, normalize_template_parameter_key};
use crate::filesystem::{ScanOptions, scan_files};
use crate::knowledge::templates::normalize_module_lookup_title;
use crate::runtime::ResolvedPaths;
use crate::support::now_iso8601_utc;

use super::remilia_overlay::load_or_build_remilia_profile_overlay;
use super::template_catalog::{
    TemplateCatalog, TemplateCatalogEntry, TemplateCatalogExample, TemplateCatalogParameter,
    build_template_catalog_with_overlay, load_template_catalog, sync_template_catalog_with_overlay,
};
use super::wiki_capabilities::{
    WikiCapabilityManifest, load_wiki_capabilities_with_config, sync_wiki_capabilities_with_config,
};

const AUTHORING_SURFACE_SCHEMA_VERSION: &str = "authoring_surface_v1";

pub const SOURCE_HTML_TAGS: &[&str] = &[
    "abbr",
    "b",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "dd",
    "del",
    "div",
    "dl",
    "dt",
    "em",
    "font",
    "gallery",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "includeonly",
    "ins",
    "kbd",
    "li",
    "noinclude",
    "ol",
    "onlyinclude",
    "p",
    "pre",
    "q",
    "rb",
    "rp",
    "rt",
    "rtc",
    "ruby",
    "s",
    "samp",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "th",
    "thead",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "wbr",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoringSurfaceOptions {
    pub template_limit: usize,
    pub template_example_limit: usize,
    pub module_limit: usize,
    pub extension_limit: usize,
    pub extension_tag_limit: usize,
}

impl Default for AuthoringSurfaceOptions {
    fn default() -> Self {
        Self {
            template_limit: 64,
            template_example_limit: 2,
            module_limit: 128,
            extension_limit: 128,
            extension_tag_limit: 128,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringSurface {
    pub schema_version: String,
    pub profile_id: String,
    pub generated_at: String,
    pub wiki_id: Option<String>,
    pub wiki_url: Option<String>,
    pub capabilities_refreshed_at: Option<String>,
    pub template_catalog_refreshed_at: Option<String>,
    pub template_source: String,
    pub template_count_total: usize,
    pub template_count_returned: usize,
    pub module_count_total: usize,
    pub module_count_returned: usize,
    pub extension_count_total: usize,
    pub extension_count_returned: usize,
    pub extension_tag_count_total: usize,
    pub extension_tag_count_returned: usize,
    pub templates: Vec<AuthoringTemplateSurface>,
    pub modules: Vec<AuthoringModuleSurface>,
    pub extensions: Vec<AuthoringExtensionSurface>,
    pub extension_tags: Vec<AuthoringExtensionTagSurface>,
    pub source_html_tags: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringTemplateSurface {
    pub template_title: String,
    pub category: String,
    pub summary_text: Option<String>,
    pub has_templatedata: bool,
    pub redirect_aliases: Vec<String>,
    pub usage_aliases: Vec<String>,
    pub usage_count: usize,
    pub distinct_page_count: usize,
    pub documentation_titles: Vec<String>,
    pub implementation_titles: Vec<String>,
    pub module_titles: Vec<String>,
    pub recommendation_tags: Vec<String>,
    pub declared_parameter_keys: Vec<String>,
    pub parameter_count: usize,
    pub parameters: Vec<AuthoringTemplateParameterSurface>,
    pub examples: Vec<TemplateCatalogExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringTemplateParameterSurface {
    pub name: String,
    pub aliases: Vec<String>,
    pub observed_names: Vec<String>,
    pub sources: Vec<String>,
    pub label: Option<String>,
    pub description: Option<String>,
    pub param_type: Option<String>,
    pub required: bool,
    pub suggested: bool,
    pub deprecated: bool,
    pub usage_count: usize,
    pub example_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringModuleSurface {
    pub module_title: String,
    pub relative_path: Option<String>,
    pub is_redirect: bool,
    pub redirect_target: Option<String>,
    pub sources: Vec<String>,
    pub used_by_templates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringExtensionSurface {
    pub name: String,
    pub version: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringExtensionTagSurface {
    pub tag_name: String,
    pub paired_syntax: String,
    pub self_closing_syntax: String,
    pub source: String,
    pub docs_query: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionTagPolicy {
    supported_extension_tags: BTreeSet<String>,
    source_html_tags: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalModuleRecord {
    module_title: String,
    relative_path: String,
    is_redirect: bool,
    redirect_target: Option<String>,
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

impl ExtensionTagPolicy {
    pub fn from_capabilities(capabilities: &WikiCapabilityManifest) -> Self {
        let supported_extension_tags = capabilities
            .parser_extension_tags
            .iter()
            .map(|tag| normalize_parser_tag_name(tag))
            .filter(|tag| !tag.is_empty())
            .collect::<BTreeSet<_>>();
        let source_html_tags = SOURCE_HTML_TAGS
            .iter()
            .map(|tag| normalize_parser_tag_name(tag))
            .collect::<BTreeSet<_>>();
        Self {
            supported_extension_tags,
            source_html_tags,
        }
    }

    pub fn supports_source_tag(&self, tag: &str) -> bool {
        let normalized = normalize_parser_tag_name(tag);
        self.supported_extension_tags.contains(&normalized)
            || self.source_html_tags.contains(&normalized)
    }
}

pub fn build_authoring_surface_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
    options: AuthoringSurfaceOptions,
) -> Result<AuthoringSurface> {
    let overlay = load_or_build_remilia_profile_overlay(paths)?;
    let capabilities = load_wiki_capabilities_with_config(paths, config)?;
    let catalog = match load_template_catalog(paths, &overlay.profile_id)? {
        Some(catalog) => Some(catalog),
        None => Some(build_template_catalog_with_overlay(paths, &overlay)?),
    };
    let local_modules = scan_local_modules(paths)?;
    Ok(build_authoring_surface_from_parts(
        &overlay.profile_id,
        capabilities.as_ref(),
        catalog.as_ref(),
        Some(&local_modules),
        options,
    ))
}

pub fn sync_authoring_surface_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
    options: AuthoringSurfaceOptions,
) -> Result<AuthoringSurface> {
    let overlay = load_or_build_remilia_profile_overlay(paths)?;
    let capabilities = sync_wiki_capabilities_with_config(paths, config)?;
    let catalog = sync_template_catalog_with_overlay(paths, &overlay)?;
    let local_modules = scan_local_modules(paths)?;
    Ok(build_authoring_surface_from_parts(
        &overlay.profile_id,
        Some(&capabilities),
        Some(&catalog),
        Some(&local_modules),
        options,
    ))
}

pub fn build_authoring_surface(
    profile_id: &str,
    capabilities: Option<&WikiCapabilityManifest>,
    catalog: Option<&TemplateCatalog>,
    options: AuthoringSurfaceOptions,
) -> AuthoringSurface {
    build_authoring_surface_from_parts(profile_id, capabilities, catalog, None, options)
}

fn build_authoring_surface_from_parts(
    profile_id: &str,
    capabilities: Option<&WikiCapabilityManifest>,
    catalog: Option<&TemplateCatalog>,
    local_modules: Option<&BTreeMap<String, LocalModuleRecord>>,
    options: AuthoringSurfaceOptions,
) -> AuthoringSurface {
    let mut warnings = Vec::new();
    if capabilities.is_none() {
        warnings.push(
            "wiki capability manifest is missing; run `wikitool wiki capabilities sync`"
                .to_string(),
        );
    }
    if catalog.is_none() {
        warnings.push(
            "template catalog is missing; run `wikitool templates catalog build`".to_string(),
        );
    }
    if let Some(catalog) = catalog
        && !catalog.usage_index_ready
    {
        warnings.push(
            "template usage counts/examples are incomplete because the local content index is missing"
                .to_string(),
        );
    }

    let templates = catalog
        .map(|catalog| build_template_surfaces(catalog, options))
        .unwrap_or_default();
    let modules = build_module_surfaces(catalog, local_modules, options.module_limit);
    let extensions = capabilities
        .map(|capabilities| build_extension_surfaces(capabilities, options.extension_limit))
        .unwrap_or_default();
    let extension_tags = capabilities
        .map(|capabilities| build_extension_tag_surfaces(capabilities, options.extension_tag_limit))
        .unwrap_or_default();

    AuthoringSurface {
        schema_version: AUTHORING_SURFACE_SCHEMA_VERSION.to_string(),
        profile_id: profile_id.to_string(),
        generated_at: now_iso8601_utc(),
        wiki_id: capabilities.map(|manifest| manifest.wiki_id.clone()),
        wiki_url: capabilities.map(|manifest| manifest.wiki_url.clone()),
        capabilities_refreshed_at: capabilities.map(|manifest| manifest.refreshed_at.clone()),
        template_catalog_refreshed_at: catalog.map(|catalog| catalog.refreshed_at.clone()),
        template_source:
            "local template source, local TemplateData, local usage index, and profile overlay"
                .to_string(),
        template_count_total: catalog.map(|catalog| catalog.entries.len()).unwrap_or(0),
        template_count_returned: templates.len(),
        module_count_total: count_distinct_modules(catalog, local_modules),
        module_count_returned: modules.len(),
        extension_count_total: capabilities
            .map(|manifest| manifest.extensions.len())
            .unwrap_or(0),
        extension_count_returned: extensions.len(),
        extension_tag_count_total: capabilities
            .map(|manifest| manifest.parser_extension_tags.len())
            .unwrap_or(0),
        extension_tag_count_returned: extension_tags.len(),
        templates,
        modules,
        extensions,
        extension_tags,
        source_html_tags: SOURCE_HTML_TAGS
            .iter()
            .map(|tag| (*tag).to_string())
            .collect(),
        warnings,
    }
}

pub fn normalize_parser_tag_name(tag: &str) -> String {
    tag.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim_start_matches('/')
        .trim_end_matches('/')
        .trim()
        .to_ascii_lowercase()
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

pub fn known_template_parameter_keys(entry: &TemplateCatalogEntry) -> BTreeSet<String> {
    let mut known = BTreeSet::new();
    for key in &entry.declared_parameter_keys {
        insert_template_parameter_key(&mut known, key);
    }
    for parameter in &entry.parameters {
        insert_template_parameter_key(&mut known, &parameter.name);
        for alias in &parameter.aliases {
            insert_template_parameter_key(&mut known, alias);
        }
        for observed in &parameter.observed_names {
            insert_template_parameter_key(&mut known, observed);
        }
    }
    known
}

pub fn template_has_parameter_contract(entry: &TemplateCatalogEntry) -> bool {
    entry.templatedata.is_some() && !entry.parameters.is_empty()
}

pub fn unknown_template_parameter_keys(
    entry: &TemplateCatalogEntry,
    parameter_keys: &[String],
) -> Vec<String> {
    if !template_has_parameter_contract(entry) {
        return Vec::new();
    }
    let known = known_template_parameter_keys(entry);
    let mut unknown = Vec::new();
    for key in parameter_keys {
        if key.starts_with('$') {
            continue;
        }
        let normalized = normalize_template_parameter_key(key);
        if normalized.is_empty() || normalized.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        if !known.contains(&normalized) {
            unknown.push(normalized);
        }
    }
    unknown.sort();
    unknown.dedup();
    unknown
}

fn insert_template_parameter_key(out: &mut BTreeSet<String>, key: &str) {
    let normalized = normalize_template_parameter_key(key);
    if !normalized.is_empty() {
        out.insert(normalized);
    }
}

fn build_template_surfaces(
    catalog: &TemplateCatalog,
    options: AuthoringSurfaceOptions,
) -> Vec<AuthoringTemplateSurface> {
    let mut entries = catalog.entries.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then(left.template_title.cmp(&right.template_title))
    });
    entries
        .into_iter()
        .take(options.template_limit)
        .map(|entry| build_template_surface(entry, options.template_example_limit))
        .collect()
}

fn build_template_surface(
    entry: &TemplateCatalogEntry,
    example_limit: usize,
) -> AuthoringTemplateSurface {
    AuthoringTemplateSurface {
        template_title: entry.template_title.clone(),
        category: entry.category.clone(),
        summary_text: entry.summary_text.clone(),
        has_templatedata: entry.templatedata.is_some(),
        redirect_aliases: entry.redirect_aliases.clone(),
        usage_aliases: entry.usage_aliases.clone(),
        usage_count: entry.usage_count,
        distinct_page_count: entry.distinct_page_count,
        documentation_titles: entry.documentation_titles.clone(),
        implementation_titles: entry.implementation_titles.clone(),
        module_titles: entry.module_titles.clone(),
        recommendation_tags: entry.recommendation_tags.clone(),
        declared_parameter_keys: entry.declared_parameter_keys.clone(),
        parameter_count: entry.parameters.len(),
        parameters: entry
            .parameters
            .iter()
            .map(build_parameter_surface)
            .collect(),
        examples: entry.examples.iter().take(example_limit).cloned().collect(),
    }
}

fn build_parameter_surface(
    parameter: &TemplateCatalogParameter,
) -> AuthoringTemplateParameterSurface {
    AuthoringTemplateParameterSurface {
        name: parameter.name.clone(),
        aliases: parameter.aliases.clone(),
        observed_names: parameter.observed_names.clone(),
        sources: parameter.sources.clone(),
        label: parameter.label.clone(),
        description: parameter.description.clone(),
        param_type: parameter.param_type.clone(),
        required: parameter.required,
        suggested: parameter.suggested,
        deprecated: parameter.deprecated,
        usage_count: parameter.usage_count,
        example_values: parameter.example_values.clone(),
    }
}

fn build_module_surfaces(
    catalog: Option<&TemplateCatalog>,
    local_modules: Option<&BTreeMap<String, LocalModuleRecord>>,
    limit: usize,
) -> Vec<AuthoringModuleSurface> {
    let mut modules = BTreeMap::<String, ModuleSurfaceAccumulator>::new();
    if let Some(local_modules) = local_modules {
        for module in local_modules.values() {
            let key = normalize_module_title(&module.module_title);
            if key.is_empty() {
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
                if key.is_empty() {
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

fn count_distinct_modules(
    catalog: Option<&TemplateCatalog>,
    local_modules: Option<&BTreeMap<String, LocalModuleRecord>>,
) -> usize {
    let mut modules = BTreeSet::new();
    if let Some(local_modules) = local_modules {
        modules.extend(local_modules.keys().cloned());
    }
    if let Some(catalog) = catalog {
        for entry in &catalog.entries {
            for module_title in &entry.module_titles {
                let normalized = normalize_module_title(module_title);
                if !normalized.is_empty() {
                    modules.insert(normalized);
                }
            }
        }
    }
    modules.len()
}

fn scan_local_modules(paths: &ResolvedPaths) -> Result<BTreeMap<String, LocalModuleRecord>> {
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
        if normalized.is_empty() {
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

fn build_extension_surfaces(
    capabilities: &WikiCapabilityManifest,
    limit: usize,
) -> Vec<AuthoringExtensionSurface> {
    capabilities
        .extensions
        .iter()
        .take(limit)
        .map(|extension| AuthoringExtensionSurface {
            name: extension.name.clone(),
            version: extension.version.clone(),
            category: extension.category.clone(),
        })
        .collect()
}

fn build_extension_tag_surfaces(
    capabilities: &WikiCapabilityManifest,
    limit: usize,
) -> Vec<AuthoringExtensionTagSurface> {
    capabilities
        .parser_extension_tags
        .iter()
        .map(|tag| normalize_parser_tag_name(tag))
        .filter(|tag| !tag.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(limit)
        .map(|tag| AuthoringExtensionTagSurface {
            tag_name: tag.clone(),
            paired_syntax: format!("<{tag}>...</{tag}>"),
            self_closing_syntax: format!("<{tag} />"),
            source: "live wiki capability manifest".to_string(),
            docs_query: format!("{tag} extension tag"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{
        NamespaceInfo, TemplateCatalogEntry, TemplateCatalogParameter, TemplateDataRecord,
    };

    fn sample_entry() -> TemplateCatalogEntry {
        TemplateCatalogEntry {
            template_title: "Template:Infobox person".to_string(),
            relative_path: "templates/infobox/person.wiki".to_string(),
            category: "infobox".to_string(),
            summary_text: Some("Person infobox".to_string()),
            templatedata: Some(TemplateDataRecord {
                description: Some("Person".to_string()),
                format: None,
                parameters: Vec::new(),
            }),
            redirect_aliases: Vec::new(),
            usage_aliases: Vec::new(),
            usage_count: 3,
            distinct_page_count: 2,
            example_pages: Vec::new(),
            documentation_titles: Vec::new(),
            implementation_titles: Vec::new(),
            implementation_preview: None,
            module_titles: vec!["Module:Infobox person".to_string()],
            declared_parameter_keys: vec!["birth_date".to_string()],
            parameters: vec![TemplateCatalogParameter {
                name: "name".to_string(),
                aliases: vec!["title".to_string()],
                observed_names: vec!["occupation".to_string()],
                sources: vec!["templatedata".to_string()],
                label: None,
                description: None,
                param_type: None,
                required: false,
                suggested: false,
                deprecated: false,
                usage_count: 2,
                example_values: Vec::new(),
            }],
            examples: Vec::new(),
            recommendation_tags: Vec::new(),
        }
    }

    #[test]
    fn unknown_template_parameters_use_templatedata_contracts() {
        let entry = sample_entry();
        let unknown = unknown_template_parameter_keys(
            &entry,
            &[
                "name".to_string(),
                "title".to_string(),
                "occupation".to_string(),
                "birth date".to_string(),
                "made_up".to_string(),
                "$1".to_string(),
            ],
        );

        assert_eq!(unknown, vec!["made up"]);
    }

    #[test]
    fn extension_tag_policy_combines_live_tags_and_source_html() {
        let manifest = WikiCapabilityManifest {
            schema_version: "wiki_capabilities_v1".to_string(),
            wiki_id: "example.org".to_string(),
            wiki_url: "https://example.org".to_string(),
            api_url: "https://example.org/api.php".to_string(),
            rest_url: None,
            article_path: "/wiki/$1".to_string(),
            mediawiki_version: None,
            namespaces: vec![NamespaceInfo {
                id: 0,
                canonical_name: None,
                display_name: "Main".to_string(),
            }],
            extensions: Vec::new(),
            parser_extension_tags: vec!["<math>".to_string()],
            parser_function_hooks: Vec::new(),
            special_pages: Vec::new(),
            search_backend_hint: None,
            has_visual_editor: false,
            has_templatedata: false,
            has_citoid: false,
            has_cargo: false,
            has_page_forms: false,
            has_short_description: false,
            has_scribunto: false,
            has_timed_media_handler: false,
            supports_parse_api_html: false,
            supports_rest_html: false,
            rest_html_path_template: None,
            refreshed_at: "2026-04-16T00:00:00Z".to_string(),
        };
        let policy = ExtensionTagPolicy::from_capabilities(&manifest);

        assert!(policy.supports_source_tag("math"));
        assert!(policy.supports_source_tag("<span>"));
        assert!(!policy.supports_source_tag("unknown"));
    }

    #[test]
    fn authoring_surface_combines_local_and_template_referenced_modules() {
        let catalog = TemplateCatalog {
            schema_version: "template_catalog_v2".to_string(),
            profile_id: "remilia".to_string(),
            refreshed_at: "1".to_string(),
            template_count: 1,
            templatedata_count: 1,
            redirect_alias_count: 0,
            usage_index_ready: true,
            entries: vec![sample_entry()],
        };
        let mut local_modules = BTreeMap::new();
        local_modules.insert(
            "Module:Sidebar".to_string(),
            LocalModuleRecord {
                module_title: "Module:Sidebar".to_string(),
                relative_path: "templates/sidebar/Module_Sidebar.lua".to_string(),
                is_redirect: false,
                redirect_target: None,
            },
        );

        let surface = build_authoring_surface_from_parts(
            "remilia",
            None,
            Some(&catalog),
            Some(&local_modules),
            AuthoringSurfaceOptions::default(),
        );

        assert_eq!(surface.module_count_total, 2);
        assert!(
            surface
                .modules
                .iter()
                .any(|module| module.module_title == "Module:Sidebar"
                    && module.sources == vec!["local_module_file".to_string()])
        );
        assert!(
            surface
                .modules
                .iter()
                .any(|module| module.module_title == "Module:Infobox person"
                    && module.used_by_templates == vec!["Template:Infobox person".to_string()])
        );
    }
}
