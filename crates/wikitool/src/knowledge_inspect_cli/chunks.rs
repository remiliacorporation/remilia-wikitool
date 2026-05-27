use anyhow::{Result, bail};
use wikitool_core::knowledge::retrieval::{
    LocalChunkAcrossRetrieval, LocalChunkRetrieval, retrieve_local_context_chunks_across_pages,
    retrieve_local_context_chunks_with_options,
};

use crate::cli_support::{normalize_path, normalize_title_query, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn run_inspect_chunks(
    runtime: &RuntimeOptions,
    title: Option<&str>,
    query: Option<&str>,
    across_pages: bool,
    limit: usize,
    token_budget: usize,
    max_pages: usize,
    format: OutputFormat,
    diversify: bool,
    no_diversify: bool,
) -> Result<()> {
    if limit == 0 {
        bail!("knowledge inspect chunks requires --limit >= 1");
    }
    if token_budget == 0 {
        bail!("knowledge inspect chunks requires --token-budget >= 1");
    }
    if max_pages == 0 {
        bail!("knowledge inspect chunks requires --max-pages >= 1");
    }
    if diversify && no_diversify {
        bail!("cannot use --diversify and --no-diversify together");
    }

    let use_diversify = !no_diversify;
    let paths = resolve_runtime_paths(runtime)?;

    if across_pages {
        if title.is_some() {
            bail!("omit TITLE when using --across-pages");
        }
        let query = query.unwrap_or_default().trim();
        if query.is_empty() {
            bail!("knowledge inspect chunks --across-pages requires --query");
        }
        let retrieval = retrieve_local_context_chunks_across_pages(
            &paths,
            query,
            limit,
            token_budget,
            max_pages,
            use_diversify,
        )?;
        if format.is_json() {
            println!("{}", serde_json::to_string_pretty(&retrieval)?);
            return Ok(());
        }

        println!("knowledge inspect chunks");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("target: <across-pages>");
        println!("query: {query}");
        println!("limit: {limit}");
        println!("token_budget: {token_budget}");
        println!("max_pages: {max_pages}");
        println!("diversify: {use_diversify}");
        match retrieval {
            LocalChunkAcrossRetrieval::IndexMissing => {
                println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
            }
            LocalChunkAcrossRetrieval::QueryMissing => {
                bail!("query is required for across-pages chunk retrieval");
            }
            LocalChunkAcrossRetrieval::Found(report) => {
                println!("chunks.retrieval_mode: {}", report.retrieval_mode);
                println!("chunks.count: {}", report.chunks.len());
                println!("chunks.source_page_count: {}", report.source_page_count);
                println!(
                    "chunks.tokens_estimate_total: {}",
                    report.token_estimate_total
                );
                for chunk in &report.chunks {
                    println!(
                        "chunk: source={} section={} tokens={} text={}",
                        chunk.source_title,
                        chunk.section_heading.as_deref().unwrap_or("<lead>"),
                        chunk.token_estimate,
                        chunk.chunk_text
                    );
                }
            }
        }
    } else {
        let title = title.unwrap_or_default().trim();
        if title.is_empty() {
            bail!(
                "knowledge inspect chunks requires a non-empty TITLE unless --across-pages is set"
            );
        }
        let retrieval = retrieve_local_context_chunks_with_options(
            &paths,
            title,
            query.map(str::trim).filter(|value| !value.is_empty()),
            limit,
            token_budget,
            use_diversify,
        )?;
        if format.is_json() {
            println!("{}", serde_json::to_string_pretty(&retrieval)?);
            return Ok(());
        }

        println!("knowledge inspect chunks");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("target: {}", normalize_title_query(title));
        println!("query: {}", query.unwrap_or("<none>").trim());
        println!("limit: {limit}");
        println!("token_budget: {token_budget}");
        match retrieval {
            LocalChunkRetrieval::IndexMissing => {
                println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
            }
            LocalChunkRetrieval::TitleMissing { title } => {
                bail!("page not found in local index: {title}");
            }
            LocalChunkRetrieval::Found(report) => {
                println!("chunks.retrieval_mode: {}", report.retrieval_mode);
                println!("chunks.count: {}", report.chunks.len());
                println!(
                    "chunks.tokens_estimate_total: {}",
                    report.token_estimate_total
                );
                for chunk in &report.chunks {
                    println!(
                        "chunk: section={} tokens={} text={}",
                        chunk.section_heading.as_deref().unwrap_or("<lead>"),
                        chunk.token_estimate,
                        chunk.chunk_text
                    );
                }
            }
        }
    }

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
