pub use super::model::{
    LocalChunkAcrossPagesResult, LocalChunkAcrossRetrieval, LocalChunkRetrieval,
    LocalChunkRetrievalResult, LocalContextBundle, LocalContextChunk, LocalContextHeading,
    LocalSearchHit, LocalSectionSummary, LocalTemplateInvocation, RetrievedChunk,
};
use super::prelude::*;
use crate::knowledge::authoring::push_authoring_query_term;
use crate::knowledge::model::{AuthoringPageCandidate, StubTemplateHint};
use crate::knowledge::references::{LocalMediaUsage, LocalReferenceUsage};
use crate::knowledge::templates::{
    load_template_invocation_rows_for_template, normalize_template_lookup_title,
};
use crate::title_variants::is_translation_variant;
use anyhow::bail;

mod chunks;
mod context;
mod display;
mod rerank;
mod search;

pub use chunks::{
    retrieve_local_context_chunks, retrieve_local_context_chunks_across_pages,
    retrieve_local_context_chunks_with_options,
};
pub use context::build_local_context;
pub use search::query_search_local;

pub(crate) use chunks::{
    build_related_page_weight_map, build_template_match_score_map,
    load_reference_authority_page_hits, load_reference_identifier_page_hits,
};
pub(crate) use context::{load_context_chunks_for_bundle, load_section_records_for_bundle};
pub(crate) use rerank::retrieve_reranked_chunks_across_pages;
#[cfg(test)]
pub(crate) use rerank::section_authoring_bias;
pub(crate) use search::{collapse_search_hits, query_search_fts, query_search_like};

use chunks::{
    collect_chunk_candidates_across_pages, load_seed_chunks_for_related_pages,
    select_retrieved_chunks,
};
use display::{
    best_context_preview, chunk_allowed_for_audience, sanitize_chunk_for_audience,
    sanitize_context_chunks_for_display, sanitize_main_namespace_prose,
};
use search::unsupported_translation_message;
