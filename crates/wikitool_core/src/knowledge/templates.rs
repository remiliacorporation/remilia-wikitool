use anyhow::Result;

use crate::runtime::ResolvedPaths;

pub use crate::index::{
    ActiveTemplateCatalog, ActiveTemplateCatalogLookup, TemplateImplementationPage,
    TemplateInvocationExample, TemplateParameterUsage, TemplateReference, TemplateReferenceLookup,
    TemplateUsageSummary,
};

pub fn query_active_template_catalog(
    paths: &ResolvedPaths,
    limit: usize,
) -> Result<ActiveTemplateCatalogLookup> {
    crate::index::query_active_template_catalog(paths, limit)
}

pub fn query_template_reference(
    paths: &ResolvedPaths,
    template_title: &str,
) -> Result<TemplateReferenceLookup> {
    crate::index::query_template_reference(paths, template_title)
}
