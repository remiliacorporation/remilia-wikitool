use anyhow::Result;

use crate::runtime::ResolvedPaths;

pub use crate::index::{
    LocalChunkAcrossPagesResult, LocalChunkAcrossRetrieval, LocalChunkRetrieval,
    LocalChunkRetrievalResult, LocalContextBundle, LocalSearchHit, RetrievedChunk,
};

pub fn query_search_local(
    paths: &ResolvedPaths,
    query: &str,
    limit: usize,
) -> Result<Option<Vec<LocalSearchHit>>> {
    crate::index::query_search_local(paths, query, limit)
}

pub fn build_local_context(
    paths: &ResolvedPaths,
    title: &str,
) -> Result<Option<LocalContextBundle>> {
    crate::index::build_local_context(paths, title)
}

pub fn retrieve_local_context_chunks(
    paths: &ResolvedPaths,
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
) -> Result<LocalChunkRetrieval> {
    crate::index::retrieve_local_context_chunks(paths, title, query, limit, token_budget)
}

pub fn retrieve_local_context_chunks_with_options(
    paths: &ResolvedPaths,
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
    diversify: bool,
) -> Result<LocalChunkRetrieval> {
    crate::index::retrieve_local_context_chunks_with_options(
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
    crate::index::retrieve_local_context_chunks_across_pages(
        paths,
        query,
        limit,
        token_budget,
        max_pages,
        diversify,
    )
}
