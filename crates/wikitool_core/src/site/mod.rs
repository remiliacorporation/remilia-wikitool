pub mod adapter;
pub mod capabilities;
mod namespaces;
pub mod surface;
pub mod template_catalog;
pub mod template_contract;
pub mod template_data;
pub mod template_engineering;
pub mod template_migration;

pub use adapter::model::{
    AdapterSourceDocument, AuthoringRules, CategoryRules, CitationRules, CitationTemplateRule,
    InfoboxPreference, LintRuleLevel, LintRules, SiteAdapter, SourceReviewRule,
    TemplateCatalogSummary, TemplateRules,
};
pub use adapter::{
    PublicationPolicyIdentity, load_site_adapter, load_site_adapter_with_config,
    publication_policy_identity, publication_policy_identity_with_config,
    resolve_docs_profile_with_config, resolve_project_owned_adapter_path,
    site_adapter_resource_paths,
};
pub use capabilities::{
    ExtensionInfo, MagicWordInfo, NamespaceInfo, WikiCapabilityManifest,
    fetch_remote_wiki_capabilities, load_latest_wiki_capabilities,
    load_wiki_capabilities_with_config, sync_wiki_capabilities_with_config,
};
pub use namespaces::discover_custom_namespaces;
pub use surface::{
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
pub use template_catalog::{
    TemplateCatalog, TemplateCatalogEntry, TemplateCatalogEntryLookup, TemplateCatalogExample,
    TemplateCatalogParameter, build_template_catalog_with_adapter, find_template_catalog_entry,
    load_latest_template_catalog, load_template_catalog, sync_template_catalog_with_adapter,
};
pub use template_contract::{
    TEMPLATE_ENGINEERING_CONTRACT_SCHEMA, TemplateContractAssessment, TemplateContractExample,
    TemplateContractFinding, TemplateContractImplementation, TemplateContractParameter,
    TemplateDependencyContractAssessment, TemplateEngineeringContract, TemplateParameterChange,
    TemplateRenderFixture, TemplateRenderFixtureBundle, assess_template_engineering_contract,
    capture_template_engineering_contract, parse_template_engineering_contract,
    render_template_scaffold,
};
pub use template_data::{TemplateDataParameter, TemplateDataRecord};
pub use template_engineering::{
    MissingTemplateDependency, ModuleDependencyNode, RuntimeProvidedDependency,
    TemplateDependencyClosure, TemplateDependencyEdge, TemplateDependencyFile,
    TemplateDependencyNode, TemplateRuntimeDependencyContext, UnresolvedTemplateDependency,
    build_template_dependency_closure, build_template_dependency_closure_with_capabilities,
};
