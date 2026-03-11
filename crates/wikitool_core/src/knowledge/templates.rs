use anyhow::Result;

use crate::runtime::ResolvedPaths;

pub use super::model::{
    ActiveTemplateCatalog, ActiveTemplateCatalogLookup, ModuleFunctionUsage,
    ModuleInvocationExample, ModuleUsageSummary, TemplateImplementationPage,
    TemplateInvocationExample, TemplateParameterUsage, TemplateReference, TemplateReferenceLookup,
    TemplateUsageSummary,
};

pub fn query_active_template_catalog(
    paths: &ResolvedPaths,
    limit: usize,
) -> Result<ActiveTemplateCatalogLookup> {
    crate::index::templates::query_active_template_catalog(paths, limit)
}

pub fn query_template_reference(
    paths: &ResolvedPaths,
    template_title: &str,
) -> Result<TemplateReferenceLookup> {
    crate::index::templates::query_template_reference(paths, template_title)
}
