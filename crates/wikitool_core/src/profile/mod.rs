pub mod authoring_surface;
pub mod remilia_overlay;
pub mod rules;
pub mod template_catalog;
pub mod template_data;
pub mod wiki_capabilities;

pub use authoring_surface::{
    AuthoringExtensionSurface, AuthoringExtensionTagSurface, AuthoringModuleSurface,
    AuthoringSurface, AuthoringSurfaceOptions, AuthoringTemplateParameterSurface,
    AuthoringTemplateSurface, ExtensionTagPolicy, build_authoring_surface,
    build_authoring_surface_with_config, known_template_parameter_keys, normalize_module_title,
    normalize_parser_tag_name, scan_local_module_titles, supports_invoke_function,
    sync_authoring_surface_with_config, template_has_parameter_contract,
    unknown_template_parameter_keys,
};
pub use remilia_overlay::{
    build_remilia_profile_overlay, load_latest_profile_overlay,
    load_or_build_remilia_profile_overlay, load_profile_overlay, load_wiki_profile_with_config,
    sync_remilia_profile_overlay, sync_wiki_profile_with_config,
};
pub use rules::{
    AuthoringRules, CategoryRules, CitationRules, CitationTemplateRule, GoldenSetRules,
    InfoboxPreference, LintRules, ProfileOverlay, ProfileSourceDocument, RemiliaRules,
    TemplateCatalogSummary, UnreliableSourceRule, WikiProfileSnapshot,
};
pub use template_catalog::{
    TemplateCatalog, TemplateCatalogEntry, TemplateCatalogEntryLookup, TemplateCatalogExample,
    TemplateCatalogParameter, build_template_catalog_with_overlay, find_template_catalog_entry,
    load_latest_template_catalog, load_template_catalog, sync_template_catalog_with_overlay,
};
pub use template_data::{TemplateDataParameter, TemplateDataRecord};
pub use wiki_capabilities::{
    ExtensionInfo, NamespaceInfo, WikiCapabilityManifest, load_latest_wiki_capabilities,
    load_wiki_capabilities_with_config, sync_wiki_capabilities_with_config,
};
