pub mod authoring_surface;
pub mod rules;
pub mod site_adapter;
pub mod template_catalog;
pub mod template_data;
pub mod wiki_capabilities;

pub use authoring_surface::{
    AuthoringAssetSurface, AuthoringExtensionSurface, AuthoringExtensionTagSurface,
    AuthoringModuleSurface, AuthoringParserFunctionSurface, AuthoringSurface,
    AuthoringSurfaceOptions, AuthoringTemplateParameterSurface, AuthoringTemplateSurface,
    ExtensionTagPolicy, build_authoring_surface, build_authoring_surface_with_config,
    known_template_parameter_keys, normalize_asset_title, normalize_module_title,
    normalize_parser_function_name, normalize_parser_tag_name, scan_local_asset_titles,
    scan_local_module_functions, scan_local_module_titles, supports_invoke_function,
    sync_authoring_surface_with_config, template_has_parameter_contract,
    unknown_template_parameter_keys,
};
pub use rules::{
    AuthoringRules, CategoryRules, CitationRules, CitationTemplateRule, InfoboxPreference,
    LintRules, ProfileSourceDocument, SiteProfile, SourceReviewRule, TemplateCatalogSummary,
    TemplateRules, WikiProfileSnapshot,
};
pub use site_adapter::{
    build_site_profile, build_site_profile_with_config, load_latest_site_profile,
    load_or_build_site_profile, load_site_profile_artifact, load_wiki_profile_with_config,
    site_adapter_resource_paths, sync_site_profile, sync_site_profile_with_config,
    sync_wiki_profile_with_config,
};
pub use template_catalog::{
    TemplateCatalog, TemplateCatalogEntry, TemplateCatalogEntryLookup, TemplateCatalogExample,
    TemplateCatalogParameter, build_template_catalog_with_profile, find_template_catalog_entry,
    load_latest_template_catalog, load_template_catalog, sync_template_catalog_with_profile,
};
pub use template_data::{TemplateDataParameter, TemplateDataRecord};
pub use wiki_capabilities::{
    ExtensionInfo, NamespaceInfo, WikiCapabilityManifest, fetch_remote_wiki_capabilities,
    load_latest_wiki_capabilities, load_wiki_capabilities_with_config,
    sync_wiki_capabilities_with_config,
};
