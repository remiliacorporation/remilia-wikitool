use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::knowledge::retrieval::{
    LocalChunkAcrossRetrieval, LocalChunkRetrieval, retrieve_local_context_chunks_across_pages,
    retrieve_local_context_chunks_with_options,
};

use crate::briefs::{BriefCommand, brief_command, brief_command_owned};
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
    view: BriefView,
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
            if view.is_full() {
                println!("{}", serde_json::to_string_pretty(&retrieval)?);
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&build_across_pages_brief(
                        query,
                        limit,
                        token_budget,
                        max_pages,
                        use_diversify,
                        &retrieval,
                    ))?
                );
            }
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
            if view.is_full() {
                println!("{}", serde_json::to_string_pretty(&retrieval)?);
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&build_single_page_brief(
                        title,
                        query.map(str::trim).filter(|value| !value.is_empty()),
                        limit,
                        token_budget,
                        use_diversify,
                        &retrieval,
                    ))?
                );
            }
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

#[derive(Debug, Serialize)]
struct ChunkBrief {
    schema_version: &'static str,
    command: &'static str,
    view: &'static str,
    status: &'static str,
    target: String,
    query: Option<String>,
    retrieval_mode: Option<String>,
    source_page_count: Option<usize>,
    chunk_count: usize,
    token_estimate_total: usize,
    limits: ChunkBriefLimits,
    chunks: Vec<ChunkCard>,
    blocking: Vec<String>,
    warnings: Vec<String>,
    next_commands: Vec<BriefCommand>,
    full_view_command: BriefCommand,
}

#[derive(Debug, Serialize)]
struct ChunkBriefLimits {
    limit: usize,
    token_budget: usize,
    max_pages: Option<usize>,
    diversify: bool,
}

#[derive(Debug, Serialize)]
struct ChunkCard {
    source_title: Option<String>,
    source_namespace: Option<String>,
    source_relative_path: Option<String>,
    section_heading: Option<String>,
    token_estimate: usize,
    text: String,
}

fn build_across_pages_brief(
    query: &str,
    limit: usize,
    token_budget: usize,
    max_pages: usize,
    diversify: bool,
    retrieval: &LocalChunkAcrossRetrieval,
) -> ChunkBrief {
    match retrieval {
        LocalChunkAcrossRetrieval::IndexMissing => ChunkBrief {
            schema_version: "wikitool_brief_v1",
            command: "knowledge inspect chunks",
            view: "brief",
            status: "index_missing",
            target: "<across-pages>".to_string(),
            query: Some(query.to_string()),
            retrieval_mode: None,
            source_page_count: None,
            chunk_count: 0,
            token_estimate_total: 0,
            limits: ChunkBriefLimits {
                limit,
                token_budget,
                max_pages: Some(max_pages),
                diversify,
            },
            chunks: Vec::new(),
            blocking: vec![
                "knowledge index is missing; run `wikitool knowledge build`".to_string(),
            ],
            warnings: Vec::new(),
            next_commands: vec![brief_command(&[
                "wikitool",
                "knowledge",
                "build",
                "--format",
                "json",
            ])],
            full_view_command: across_pages_full_command(query, limit, token_budget, max_pages),
        },
        LocalChunkAcrossRetrieval::QueryMissing => ChunkBrief {
            schema_version: "wikitool_brief_v1",
            command: "knowledge inspect chunks",
            view: "brief",
            status: "query_missing",
            target: "<across-pages>".to_string(),
            query: None,
            retrieval_mode: None,
            source_page_count: None,
            chunk_count: 0,
            token_estimate_total: 0,
            limits: ChunkBriefLimits {
                limit,
                token_budget,
                max_pages: Some(max_pages),
                diversify,
            },
            chunks: Vec::new(),
            blocking: vec!["--query is required for across-pages chunk retrieval".to_string()],
            warnings: Vec::new(),
            next_commands: Vec::new(),
            full_view_command: across_pages_full_command(query, limit, token_budget, max_pages),
        },
        LocalChunkAcrossRetrieval::Found(report) => ChunkBrief {
            schema_version: "wikitool_brief_v1",
            command: "knowledge inspect chunks",
            view: "brief",
            status: "found",
            target: "<across-pages>".to_string(),
            query: Some(report.query.clone()),
            retrieval_mode: Some(report.retrieval_mode.clone()),
            source_page_count: Some(report.source_page_count),
            chunk_count: report.chunks.len(),
            token_estimate_total: report.token_estimate_total,
            limits: ChunkBriefLimits {
                limit,
                token_budget,
                max_pages: Some(report.max_pages),
                diversify,
            },
            chunks: report
                .chunks
                .iter()
                .map(|chunk| ChunkCard {
                    source_title: Some(chunk.source_title.clone()),
                    source_namespace: Some(chunk.source_namespace.clone()),
                    source_relative_path: Some(chunk.source_relative_path.clone()),
                    section_heading: chunk.section_heading.clone(),
                    token_estimate: chunk.token_estimate,
                    text: chunk.chunk_text.clone(),
                })
                .collect(),
            blocking: Vec::new(),
            warnings: if report.chunks.is_empty() {
                vec!["no chunks matched the query within the current token budget".to_string()]
            } else {
                Vec::new()
            },
            next_commands: vec![brief_command_owned(vec![
                "wikitool".to_string(),
                "knowledge".to_string(),
                "inspect".to_string(),
                "chunks".to_string(),
                "--across-pages".to_string(),
                "--query".to_string(),
                report.query.clone(),
                "--limit".to_string(),
                (limit.saturating_mul(2)).max(limit).to_string(),
                "--token-budget".to_string(),
                (token_budget.saturating_mul(2))
                    .max(token_budget)
                    .to_string(),
                "--format".to_string(),
                "json".to_string(),
                "--view".to_string(),
                "brief".to_string(),
            ])],
            full_view_command: across_pages_full_command(
                &report.query,
                limit,
                token_budget,
                max_pages,
            ),
        },
    }
}

fn build_single_page_brief(
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
    diversify: bool,
    retrieval: &LocalChunkRetrieval,
) -> ChunkBrief {
    match retrieval {
        LocalChunkRetrieval::IndexMissing => ChunkBrief {
            schema_version: "wikitool_brief_v1",
            command: "knowledge inspect chunks",
            view: "brief",
            status: "index_missing",
            target: normalize_title_query(title),
            query: query.map(str::to_string),
            retrieval_mode: None,
            source_page_count: None,
            chunk_count: 0,
            token_estimate_total: 0,
            limits: ChunkBriefLimits {
                limit,
                token_budget,
                max_pages: None,
                diversify,
            },
            chunks: Vec::new(),
            blocking: vec![
                "knowledge index is missing; run `wikitool knowledge build`".to_string(),
            ],
            warnings: Vec::new(),
            next_commands: vec![brief_command(&[
                "wikitool",
                "knowledge",
                "build",
                "--format",
                "json",
            ])],
            full_view_command: single_page_full_command(title, query, limit, token_budget),
        },
        LocalChunkRetrieval::TitleMissing { title } => ChunkBrief {
            schema_version: "wikitool_brief_v1",
            command: "knowledge inspect chunks",
            view: "brief",
            status: "title_missing",
            target: title.clone(),
            query: query.map(str::to_string),
            retrieval_mode: None,
            source_page_count: None,
            chunk_count: 0,
            token_estimate_total: 0,
            limits: ChunkBriefLimits {
                limit,
                token_budget,
                max_pages: None,
                diversify,
            },
            chunks: Vec::new(),
            blocking: vec![format!("page not found in local index: {title}")],
            warnings: Vec::new(),
            next_commands: vec![brief_command_owned(vec![
                "wikitool".to_string(),
                "research".to_string(),
                "wiki-search".to_string(),
                title.clone(),
                "--what".to_string(),
                "title".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])],
            full_view_command: single_page_full_command(title, query, limit, token_budget),
        },
        LocalChunkRetrieval::Found(report) => ChunkBrief {
            schema_version: "wikitool_brief_v1",
            command: "knowledge inspect chunks",
            view: "brief",
            status: "found",
            target: report.title.clone(),
            query: report.query.clone(),
            retrieval_mode: Some(report.retrieval_mode.clone()),
            source_page_count: Some(1),
            chunk_count: report.chunks.len(),
            token_estimate_total: report.token_estimate_total,
            limits: ChunkBriefLimits {
                limit,
                token_budget,
                max_pages: None,
                diversify,
            },
            chunks: report
                .chunks
                .iter()
                .map(|chunk| ChunkCard {
                    source_title: Some(report.title.clone()),
                    source_namespace: Some(report.namespace.clone()),
                    source_relative_path: Some(report.relative_path.clone()),
                    section_heading: chunk.section_heading.clone(),
                    token_estimate: chunk.token_estimate,
                    text: chunk.chunk_text.clone(),
                })
                .collect(),
            blocking: Vec::new(),
            warnings: if report.chunks.is_empty() {
                vec!["no chunks matched the query within the current token budget".to_string()]
            } else {
                Vec::new()
            },
            next_commands: vec![brief_command_owned(vec![
                "wikitool".to_string(),
                "knowledge".to_string(),
                "inspect".to_string(),
                "chunks".to_string(),
                report.title.clone(),
                "--limit".to_string(),
                (limit.saturating_mul(2)).max(limit).to_string(),
                "--token-budget".to_string(),
                (token_budget.saturating_mul(2))
                    .max(token_budget)
                    .to_string(),
                "--format".to_string(),
                "json".to_string(),
                "--view".to_string(),
                "brief".to_string(),
            ])],
            full_view_command: single_page_full_command(
                &report.title,
                report.query.as_deref(),
                limit,
                token_budget,
            ),
        },
    }
}

fn across_pages_full_command(
    query: &str,
    limit: usize,
    token_budget: usize,
    max_pages: usize,
) -> BriefCommand {
    brief_command_owned(vec![
        "wikitool".to_string(),
        "knowledge".to_string(),
        "inspect".to_string(),
        "chunks".to_string(),
        "--across-pages".to_string(),
        "--query".to_string(),
        query.to_string(),
        "--limit".to_string(),
        limit.to_string(),
        "--max-pages".to_string(),
        max_pages.to_string(),
        "--token-budget".to_string(),
        token_budget.to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--view".to_string(),
        "full".to_string(),
    ])
}

fn single_page_full_command(
    title: &str,
    query: Option<&str>,
    limit: usize,
    token_budget: usize,
) -> BriefCommand {
    let mut argv = vec![
        "wikitool".to_string(),
        "knowledge".to_string(),
        "inspect".to_string(),
        "chunks".to_string(),
        title.to_string(),
    ];
    if let Some(query) = query {
        argv.push("--query".to_string());
        argv.push(query.to_string());
    }
    argv.extend([
        "--limit".to_string(),
        limit.to_string(),
        "--token-budget".to_string(),
        token_budget.to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--view".to_string(),
        "full".to_string(),
    ]);
    brief_command_owned(argv)
}
