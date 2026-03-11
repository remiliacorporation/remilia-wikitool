use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use reqwest::Url;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde_json::json;

use crate::filesystem::{
    Namespace, ScanOptions, ScanStats, ScannedFile, scan_files, validate_scoped_path,
};
use crate::knowledge::status::record_content_index_artifact;
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{normalize_path, table_exists, unix_timestamp};

#[path = "index/authoring.rs"]
pub(crate) mod authoring;
#[path = "index/ingest.rs"]
pub(crate) mod ingest;
#[path = "index/model.rs"]
mod model;
#[path = "index/parsing.rs"]
mod parsing;
#[path = "index/references.rs"]
mod references;
#[path = "index/retrieval.rs"]
pub(crate) mod retrieval;
#[path = "index/storage.rs"]
mod storage;
#[path = "index/templates.rs"]
pub(crate) mod templates;
#[path = "index/validation.rs"]
pub(crate) mod validation;
#[cfg(test)]
#[path = "index/tests.rs"]
mod tests;

pub use crate::knowledge::authoring::{
    AuthoringDocsContext, AuthoringInventory, AuthoringKnowledgePack,
    AuthoringKnowledgePackOptions, AuthoringKnowledgePackResult, AuthoringPageCandidate,
    AuthoringSuggestion, StubTemplateHint,
};
pub use crate::knowledge::content_index::{RebuildReport, StoredIndexStats};
pub use crate::knowledge::inspect::{
    BrokenLinkIssue, DoubleRedirectIssue, ValidationReport,
};
pub use crate::knowledge::references::{
    LocalMediaUsage, LocalReferenceUsage, MediaUsageExample, MediaUsageSummary,
    ReferenceUsageExample, ReferenceUsageSummary,
};
pub use crate::knowledge::retrieval::{
    LocalChunkAcrossPagesResult, LocalChunkAcrossRetrieval, LocalChunkRetrieval,
    LocalChunkRetrievalResult, LocalContextBundle, LocalContextChunk, LocalContextHeading,
    LocalSearchHit, LocalSectionSummary, LocalTemplateInvocation, RetrievedChunk,
};
pub use crate::knowledge::templates::{
    ActiveTemplateCatalog, ActiveTemplateCatalogLookup, ModuleFunctionUsage,
    ModuleInvocationExample, ModuleUsageSummary, TemplateImplementationPage,
    TemplateInvocationExample, TemplateParameterUsage, TemplateReference,
    TemplateReferenceLookup, TemplateUsageSummary,
};

pub fn query_active_template_catalog(
    paths: &ResolvedPaths,
    limit: usize,
) -> Result<ActiveTemplateCatalogLookup> {
    crate::knowledge::templates::query_active_template_catalog(paths, limit)
}

pub fn query_template_reference(
    paths: &ResolvedPaths,
    template_title: &str,
) -> Result<TemplateReferenceLookup> {
    crate::knowledge::templates::query_template_reference(paths, template_title)
}

pub fn run_validation_checks(paths: &ResolvedPaths) -> Result<Option<ValidationReport>> {
    crate::knowledge::inspect::run_validation_checks(paths)
}

pub fn query_backlinks(paths: &ResolvedPaths, title: &str) -> Result<Option<Vec<String>>> {
    crate::knowledge::inspect::query_backlinks(paths, title)
}

pub fn query_orphans(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    crate::knowledge::inspect::query_orphans(paths)
}

pub fn query_empty_categories(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    crate::knowledge::inspect::query_empty_categories(paths)
}

pub fn rebuild_index(paths: &ResolvedPaths, options: &ScanOptions) -> Result<RebuildReport> {
    crate::knowledge::content_index::rebuild_index(paths, options)
}

pub fn load_stored_index_stats(paths: &ResolvedPaths) -> Result<Option<StoredIndexStats>> {
    crate::knowledge::content_index::load_stored_index_stats(paths)
}

pub fn build_authoring_knowledge_pack(
    paths: &ResolvedPaths,
    topic: Option<&str>,
    stub_content: Option<&str>,
    options: &AuthoringKnowledgePackOptions,
) -> Result<AuthoringKnowledgePack> {
    crate::knowledge::authoring::build_authoring_knowledge_pack(paths, topic, stub_content, options)
}

pub fn query_search_local(
    paths: &ResolvedPaths,
    query: &str,
    limit: usize,
) -> Result<Option<Vec<LocalSearchHit>>> {
    crate::knowledge::retrieval::query_search_local(paths, query, limit)
}

pub fn build_local_context(
    paths: &ResolvedPaths,
    title: &str,
) -> Result<Option<LocalContextBundle>> {
    crate::knowledge::retrieval::build_local_context(paths, title)
}

pub fn retrieve_local_context_chunks(
    paths: &ResolvedPaths,
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
) -> Result<LocalChunkRetrieval> {
    crate::knowledge::retrieval::retrieve_local_context_chunks(
        paths,
        title,
        query,
        limit,
        token_budget,
    )
}

pub fn retrieve_local_context_chunks_with_options(
    paths: &ResolvedPaths,
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
    diversify: bool,
) -> Result<LocalChunkRetrieval> {
    crate::knowledge::retrieval::retrieve_local_context_chunks_with_options(
        paths,
        title,
        query,
        limit,
        token_budget,
        diversify,
    )
}

pub fn retrieve_local_context_chunks_across_pages(
    paths: &ResolvedPaths,
    query: &str,
    limit: usize,
    token_budget: usize,
    max_pages: usize,
    diversify: bool,
) -> Result<LocalChunkAcrossRetrieval> {
    crate::knowledge::retrieval::retrieve_local_context_chunks_across_pages(
        paths,
        query,
        limit,
        token_budget,
        max_pages,
        diversify,
    )
}
