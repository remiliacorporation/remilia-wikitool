use anyhow::Result;
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::site::{
    PublicationPolicyIdentity, SiteAdapter, load_site_adapter, publication_policy_identity,
};

use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct AdapterArgs {
    #[command(subcommand)]
    command: AdapterSubcommand,
}

#[derive(Debug, Subcommand)]
enum AdapterSubcommand {
    #[command(about = "Inspect the explicitly configured site adapter and publication identity")]
    Inspect(AdapterInspectArgs),
}

#[derive(Debug, Args)]
struct AdapterInspectArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct AdapterInspectReport<'a> {
    schema_version: &'static str,
    adapter: &'a SiteAdapter,
    publication_identity: &'a PublicationPolicyIdentity,
}

pub(crate) fn run_adapter(runtime: &RuntimeOptions, args: AdapterArgs) -> Result<()> {
    match args.command {
        AdapterSubcommand::Inspect(args) => run_adapter_inspect(runtime, args),
    }
}

fn run_adapter_inspect(runtime: &RuntimeOptions, args: AdapterInspectArgs) -> Result<()> {
    let (paths, _config) = resolve_runtime_with_config(runtime)?;
    let adapter = load_site_adapter(&paths)?;
    let publication_identity = publication_policy_identity(&paths)?;
    let report = AdapterInspectReport {
        schema_version: "adapter_inspect_v1",
        adapter: &adapter,
        publication_identity: &publication_identity,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("adapter inspect");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("adapter_id: {}", adapter.adapter_id);
    println!("docs_profile: {}", adapter.docs_profile);
    println!(
        "publication_policy_sha256: {}",
        publication_identity.policy_sha256
    );
    println!("source_document_count: {}", adapter.source_documents.len());
    for source in &adapter.source_documents {
        println!(
            "source_document: path={} sha256={}",
            source.relative_path, source.content_hash
        );
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
