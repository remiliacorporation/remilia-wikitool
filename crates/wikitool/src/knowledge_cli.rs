use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::docs::{
    DocsImportProfileOptions, DocsImportProfileReport, import_docs_profile_with_config,
};
use wikitool_core::filesystem::{ScanOptions, validate_scoped_path};
use wikitool_core::knowledge::authoring::{
    AuthoringKnowledgePack, AuthoringKnowledgePackOptions, AuthoringKnowledgePackResult,
    build_authoring_knowledge_pack,
};
use wikitool_core::knowledge::content_index::{RebuildReport, rebuild_index};
use wikitool_core::knowledge::status::{
    DEFAULT_DOCS_PROFILE, KnowledgeReadinessLevel, KnowledgeStatusReport, knowledge_status,
};
use wikitool_core::runtime::{ResolvedPaths, ensure_runtime_ready_for_sync, inspect_runtime};

use crate::cli_support::{
    collapse_whitespace, format_flag, normalize_option, normalize_path,
    print_database_schema_status, print_scan_stats, resolve_runtime_paths,
    resolve_runtime_with_config,
};
use crate::knowledge_inspect_cli;
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct KnowledgeArgs {
    #[command(subcommand)]
    command: KnowledgeSubcommand,
}

#[derive(Debug, Subcommand)]
enum KnowledgeSubcommand {
    #[command(about = "Rebuild the local content knowledge index")]
    Build(KnowledgeBuildArgs),
    #[command(about = "Build content knowledge and hydrate a docs profile")]
    Warm(KnowledgeWarmArgs),
    #[command(about = "Report knowledge readiness and degradations")]
    Status(KnowledgeStatusArgs),
    #[command(about = "Assemble the authoring knowledge pack")]
    Pack(KnowledgePackArgs),
    #[command(about = "Inspect indexed knowledge structures directly")]
    Inspect(knowledge_inspect_cli::KnowledgeInspectArgs),
}

#[derive(Debug, Args)]
pub(crate) struct KnowledgeBuildArgs {
    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: String,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct KnowledgeWarmArgs {
    #[arg(
        long,
        default_value = DEFAULT_DOCS_PROFILE,
        value_name = "PROFILE",
        help = "Docs profile to hydrate during warmup"
    )]
    pub(crate) docs_profile: String,
    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: String,
}

#[derive(Debug, Args)]
pub(crate) struct KnowledgeStatusArgs {
    #[arg(
        long,
        default_value = DEFAULT_DOCS_PROFILE,
        value_name = "PROFILE",
        help = "Docs profile to assess for authoring readiness"
    )]
    docs_profile: String,
    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: String,
}

#[derive(Debug, Args)]
pub(crate) struct KnowledgePackArgs {
    #[arg(
        value_name = "TOPIC",
        help = "Primary article topic/title for retrieval"
    )]
    topic: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional stub wikitext file used for link/template hint extraction"
    )]
    stub_path: Option<PathBuf>,
    #[arg(
        long,
        default_value_t = 18,
        value_name = "N",
        help = "Maximum related pages in the pack"
    )]
    related_limit: usize,
    #[arg(
        long,
        default_value_t = 10,
        value_name = "N",
        help = "Maximum retrieved context chunks"
    )]
    chunk_limit: usize,
    #[arg(
        long,
        default_value_t = 1200,
        value_name = "TOKENS",
        help = "Token budget across retrieved chunks"
    )]
    token_budget: usize,
    #[arg(
        long,
        default_value_t = 8,
        value_name = "N",
        help = "Maximum distinct source pages in chunk retrieval"
    )]
    max_pages: usize,
    #[arg(
        long,
        default_value_t = 18,
        value_name = "N",
        help = "Maximum internal link suggestions"
    )]
    link_limit: usize,
    #[arg(
        long,
        default_value_t = 8,
        value_name = "N",
        help = "Maximum category suggestions"
    )]
    category_limit: usize,
    #[arg(
        long,
        default_value_t = 16,
        value_name = "N",
        help = "Maximum template summaries"
    )]
    template_limit: usize,
    #[arg(
        long,
        default_value = DEFAULT_DOCS_PROFILE,
        value_name = "PROFILE",
        help = "Docs profile to use for bridged authoring retrieval"
    )]
    docs_profile: String,
    #[arg(
        long,
        default_value = "json",
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: String,
    #[arg(long, help = "Enable lexical chunk de-duplication and diversification")]
    diversify: bool,
    #[arg(
        long,
        help = "Disable lexical chunk de-duplication and diversification"
    )]
    no_diversify: bool,
}

#[derive(Debug, Serialize)]
struct KnowledgeBuildReport {
    rebuild: RebuildReport,
    status: KnowledgeStatusReport,
}

#[derive(Debug, Serialize)]
struct KnowledgeWarmReport {
    rebuild: RebuildReport,
    docs: DocsImportProfileReport,
    status: KnowledgeStatusReport,
}

#[derive(Debug, Serialize)]
struct KnowledgePackOutput {
    docs_profile_requested: String,
    readiness: KnowledgeReadinessLevel,
    degradations: Vec<String>,
    knowledge_generation: String,
    result: KnowledgePackPayload,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum KnowledgePackPayload {
    IndexMissing,
    QueryMissing,
    Found(Box<AuthoringKnowledgePackResult>),
}

pub(crate) fn run_knowledge(runtime: &RuntimeOptions, args: KnowledgeArgs) -> Result<()> {
    match args.command {
        KnowledgeSubcommand::Build(args) => run_knowledge_build(runtime, args),
        KnowledgeSubcommand::Warm(args) => run_knowledge_warm(runtime, args),
        KnowledgeSubcommand::Status(args) => run_knowledge_status(runtime, args),
        KnowledgeSubcommand::Pack(args) => run_knowledge_pack(runtime, args),
        KnowledgeSubcommand::Inspect(args) => {
            knowledge_inspect_cli::run_knowledge_inspect(runtime, args)
        }
    }
}

pub(crate) fn run_knowledge_warm(runtime: &RuntimeOptions, args: KnowledgeWarmArgs) -> Result<()> {
    let format = normalize_format(&args.format)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let rebuild = rebuild_knowledge_index(&paths)?;
    let docs = import_docs_profile_with_config(
        &paths,
        &DocsImportProfileOptions {
            profile: args.docs_profile.clone(),
            ..DocsImportProfileOptions::default()
        },
        &config,
    )?;
    let status = knowledge_status(&paths, &args.docs_profile)?;
    let report = KnowledgeWarmReport {
        rebuild,
        docs,
        status,
    };

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("knowledge warm");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "docs_profile_requested: {}",
        report.status.docs_profile_requested
    );
    println!(
        "knowledge_generation: {}",
        report.status.knowledge_generation
    );
    println!("rebuild.inserted_rows: {}", report.rebuild.inserted_rows);
    println!("rebuild.inserted_links: {}", report.rebuild.inserted_links);
    println!("docs.imported_corpora: {}", report.docs.imported_corpora);
    println!("docs.imported_pages: {}", report.docs.imported_pages);
    print_scan_stats("scan", &report.rebuild.scan);
    print_knowledge_status("knowledge", &report.status);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_knowledge_build(runtime: &RuntimeOptions, args: KnowledgeBuildArgs) -> Result<()> {
    let format = normalize_format(&args.format)?;
    let paths = resolve_runtime_paths(runtime)?;
    let rebuild = rebuild_knowledge_index(&paths)?;
    let status = knowledge_status(&paths, DEFAULT_DOCS_PROFILE)?;
    let report = KnowledgeBuildReport { rebuild, status };

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("knowledge build");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "docs_profile_requested: {}",
        report.status.docs_profile_requested
    );
    println!(
        "knowledge_generation: {}",
        report.status.knowledge_generation
    );
    println!("rebuild.inserted_rows: {}", report.rebuild.inserted_rows);
    println!("rebuild.inserted_links: {}", report.rebuild.inserted_links);
    print_scan_stats("scan", &report.rebuild.scan);
    print_knowledge_status("knowledge", &report.status);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_knowledge_status(runtime: &RuntimeOptions, args: KnowledgeStatusArgs) -> Result<()> {
    let format = normalize_format(&args.format)?;
    let paths = resolve_runtime_paths(runtime)?;
    let status = knowledge_status(&paths, &args.docs_profile)?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    println!("knowledge status");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_knowledge_status("knowledge", &status);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_knowledge_pack(runtime: &RuntimeOptions, args: KnowledgePackArgs) -> Result<()> {
    if args.related_limit == 0 {
        bail!("knowledge pack requires --related-limit >= 1");
    }
    if args.chunk_limit == 0 {
        bail!("knowledge pack requires --chunk-limit >= 1");
    }
    if args.token_budget == 0 {
        bail!("knowledge pack requires --token-budget >= 1");
    }
    if args.max_pages == 0 {
        bail!("knowledge pack requires --max-pages >= 1");
    }
    if args.link_limit == 0 {
        bail!("knowledge pack requires --link-limit >= 1");
    }
    if args.category_limit == 0 {
        bail!("knowledge pack requires --category-limit >= 1");
    }
    if args.template_limit == 0 {
        bail!("knowledge pack requires --template-limit >= 1");
    }
    if args.diversify && args.no_diversify {
        bail!("cannot use --diversify and --no-diversify together");
    }

    let format = normalize_format(&args.format)?;
    let use_diversify = !args.no_diversify;
    let paths = resolve_runtime_paths(runtime)?;
    let topic = normalize_option(args.topic.as_deref())
        .or_else(|| derive_topic_from_stub_path(args.stub_path.as_deref()));
    let stub_content = load_knowledge_stub_content(&paths, args.stub_path.as_deref())?;
    let pack = build_authoring_knowledge_pack(
        &paths,
        topic.as_deref(),
        stub_content.as_deref(),
        &AuthoringKnowledgePackOptions {
            related_page_limit: args.related_limit,
            chunk_limit: args.chunk_limit,
            token_budget: args.token_budget,
            max_pages: args.max_pages,
            link_limit: args.link_limit,
            category_limit: args.category_limit,
            template_limit: args.template_limit,
            docs_profile: args.docs_profile.clone(),
            diversify: use_diversify,
        },
    )?;
    let status = knowledge_status(&paths, &args.docs_profile)?;
    let output = KnowledgePackOutput {
        docs_profile_requested: status.docs_profile_requested.clone(),
        readiness: status.readiness.clone(),
        degradations: status.degradations.clone(),
        knowledge_generation: status.knowledge_generation.clone(),
        result: match pack {
            AuthoringKnowledgePack::IndexMissing => KnowledgePackPayload::IndexMissing,
            AuthoringKnowledgePack::QueryMissing => KnowledgePackPayload::QueryMissing,
            AuthoringKnowledgePack::Found(report) => KnowledgePackPayload::Found(report),
        },
    };

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("knowledge pack");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "topic: {}",
        topic.as_deref().unwrap_or("<derived-from-stub>")
    );
    println!("docs_profile_requested: {}", output.docs_profile_requested);
    println!("knowledge_generation: {}", output.knowledge_generation);
    println!("readiness: {}", format_readiness(&output.readiness));
    println!("degradations: {}", format_list(&output.degradations));
    match output.result {
        KnowledgePackPayload::IndexMissing => {
            bail!(
                "knowledge pack requires a built knowledge index; run `wikitool knowledge build`"
            );
        }
        KnowledgePackPayload::QueryMissing => {
            bail!(
                "knowledge pack requires a topic or a stub with at least one resolvable wikilink"
            );
        }
        KnowledgePackPayload::Found(report) => {
            println!("pack.query: {}", report.query);
            println!("pack.query_terms: {}", format_list(&report.query_terms));
            println!("pack.related_pages.count: {}", report.related_pages.len());
            for page in report.related_pages.iter().take(8) {
                println!(
                    "pack.related_page: {} (namespace={} source={} retrieval_weight={})",
                    page.title, page.namespace, page.source, page.retrieval_weight
                );
            }
            println!(
                "pack.suggested_links.count: {}",
                report.suggested_links.len()
            );
            println!(
                "pack.suggested_categories.count: {}",
                report.suggested_categories.len()
            );
            println!(
                "pack.suggested_templates.count: {}",
                report.suggested_templates.len()
            );
            println!(
                "pack.suggested_references.count: {}",
                report.suggested_references.len()
            );
            println!(
                "pack.suggested_media.count: {}",
                report.suggested_media.len()
            );
            println!(
                "pack.template_references.count: {}",
                report.template_references.len()
            );
            println!(
                "pack.module_patterns.count: {}",
                report.module_patterns.len()
            );
            println!(
                "pack.docs_context.count: {}",
                report
                    .docs_context
                    .as_ref()
                    .map(|context| {
                        context.pages.len()
                            + context.sections.len()
                            + context.symbols.len()
                            + context.examples.len()
                    })
                    .unwrap_or(0)
            );
            println!("pack.retrieval_mode: {}", report.retrieval_mode);
            println!("pack.chunks.count: {}", report.chunks.len());
            println!(
                "pack.token_estimate_total: {}",
                report.pack_token_estimate_total
            );
            for chunk in report.chunks.iter().take(8) {
                println!(
                    "pack.chunk: source={} section={} tokens={} text={}",
                    chunk.source_title,
                    chunk.section_heading.as_deref().unwrap_or("<lead>"),
                    chunk.token_estimate,
                    chunk.chunk_text
                );
            }
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn rebuild_knowledge_index(paths: &ResolvedPaths) -> Result<RebuildReport> {
    let status = inspect_runtime(paths)?;
    ensure_runtime_ready_for_sync(paths, &status)?;
    rebuild_index(paths, &ScanOptions::default())
}

fn print_knowledge_status(prefix: &str, status: &KnowledgeStatusReport) {
    println!(
        "{prefix}.docs_profile_requested: {}",
        status.docs_profile_requested
    );
    println!(
        "{prefix}.readiness: {}",
        format_readiness(&status.readiness)
    );
    println!(
        "{prefix}.degradations: {}",
        format_list(&status.degradations)
    );
    println!(
        "{prefix}.knowledge_generation: {}",
        status.knowledge_generation
    );
    println!("{prefix}.db_exists: {}", format_flag(status.db_exists));
    println!(
        "{prefix}.content_index_ready: {}",
        format_flag(status.content_index_ready)
    );
    println!(
        "{prefix}.docs_profile_ready: {}",
        format_flag(status.docs_profile_ready)
    );
    println!("{prefix}.index_rows: {}", status.index_rows);
    println!(
        "{prefix}.docs_profile_corpora: {}",
        status.docs_profile_corpora
    );
    if let Some(artifact) = &status.content_index_artifact {
        println!(
            "{prefix}.content_index_artifact: key={} rows={} built_at_unix={}",
            artifact.artifact_key, artifact.row_count, artifact.built_at_unix
        );
    } else {
        println!("{prefix}.content_index_artifact: <missing>");
    }
    if let Some(artifact) = &status.docs_profile_artifact {
        println!(
            "{prefix}.docs_profile_artifact: key={} rows={} built_at_unix={}",
            artifact.artifact_key, artifact.row_count, artifact.built_at_unix
        );
    } else {
        println!("{prefix}.docs_profile_artifact: <missing>");
    }
}

fn format_readiness(value: &KnowledgeReadinessLevel) -> &'static str {
    match value {
        KnowledgeReadinessLevel::NotReady => "not_ready",
        KnowledgeReadinessLevel::ContentReady => "content_ready",
        KnowledgeReadinessLevel::AuthoringReady => "authoring_ready",
    }
}

fn format_list(values: &[String]) -> String {
    if values.is_empty() {
        "<none>".to_string()
    } else {
        values.join(", ")
    }
}

fn normalize_format(value: &str) -> Result<String> {
    let format = value.trim().to_ascii_lowercase();
    if format != "text" && format != "json" {
        bail!("unsupported format: {} (expected text|json)", value);
    }
    Ok(format)
}

fn load_knowledge_stub_content(
    paths: &ResolvedPaths,
    stub_path: Option<&Path>,
) -> Result<Option<String>> {
    let Some(path) = stub_path else {
        return Ok(None);
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    validate_scoped_path(paths, &absolute)?;
    let content = fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read {}", normalize_path(&absolute)))?;
    Ok(Some(content))
}

fn derive_topic_from_stub_path(path: Option<&Path>) -> Option<String> {
    let path = path?;
    let stem = path.file_stem()?.to_string_lossy();
    let normalized = collapse_whitespace(&stem.replace('_', " "));
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::derive_topic_from_stub_path;
    use std::path::Path;

    #[test]
    fn derive_topic_from_stub_path_normalizes_filename() {
        assert_eq!(
            derive_topic_from_stub_path(Some(Path::new("drafts/Remilia_Corporation.md"))),
            Some("Remilia Corporation".to_string())
        );
    }

    #[test]
    fn derive_topic_from_stub_path_rejects_blank_stem() {
        assert_eq!(
            derive_topic_from_stub_path(Some(Path::new("drafts/___.md"))),
            None
        );
    }
}
