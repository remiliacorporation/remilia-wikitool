use anyhow::Result;
use wikitool_core::site::{
    fetch_remote_wiki_capabilities, load_wiki_capabilities_with_config,
    sync_wiki_capabilities_with_config,
};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::print_manifest;
use super::summary::{
    RemoteWikiCapabilitiesReport, summarize_capability_manifest,
    summarize_remote_capabilities_report,
};
use super::*;
pub(super) fn run_wiki_capabilities(
    runtime: &RuntimeOptions,
    args: WikiCapabilitiesArgs,
) -> Result<()> {
    match args.command {
        WikiCapabilitiesSubcommand::Sync(args) => {
            run_wiki_capabilities_sync(runtime, args.format, args.view)
        }
        WikiCapabilitiesSubcommand::Show(args) => {
            run_wiki_capabilities_show(runtime, args.format, args.view)
        }
        WikiCapabilitiesSubcommand::Remote(args) => run_wiki_capabilities_remote(runtime, args),
    }
}

fn run_wiki_capabilities_remote(
    runtime: &RuntimeOptions,
    args: WikiRemoteCapabilitiesArgs,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let manifest = fetch_remote_wiki_capabilities(&args.url, &config)?;
    let report = RemoteWikiCapabilitiesReport {
        schema_version: "remote_wiki_capabilities_v1",
        capability_scope: "remote_live_capability_probe",
        source_url: &args.url,
        storage: "not_stored",
        target_compatibility_note: "This report describes the remote MediaWiki capability surface only; adapter policy, templates, modules, local files, and article lint authority require that target's own project context.",
        capabilities: &manifest,
    };

    if args.format.is_json() {
        if args.view.is_full() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&summarize_remote_capabilities_report(&report))?
            );
        }
        return Ok(());
    }

    println!("wiki capabilities remote");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("capability_scope: {}", report.capability_scope);
    println!("source_url: {}", report.source_url);
    println!("storage: {}", report.storage);
    println!(
        "target_compatibility_note: {}",
        report.target_compatibility_note
    );
    print_manifest(&manifest);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_wiki_capabilities_sync(
    runtime: &RuntimeOptions,
    format: OutputFormat,
    view: WikiJsonView,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let manifest = sync_wiki_capabilities_with_config(&paths, &config)?;

    if format.is_json() {
        if view.is_full() {
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&summarize_capability_manifest(&manifest))?
            );
        }
        return Ok(());
    }

    println!("wiki capabilities sync");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_manifest(&manifest);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_wiki_capabilities_show(
    runtime: &RuntimeOptions,
    format: OutputFormat,
    view: WikiJsonView,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let manifest = load_wiki_capabilities_with_config(&paths, &config)?.ok_or_else(|| {
        anyhow::anyhow!(
            "wiki capability manifest is missing; run `wikitool wiki capabilities sync`"
        )
    })?;

    if format.is_json() {
        if view.is_full() {
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&summarize_capability_manifest(&manifest))?
            );
        }
        return Ok(());
    }

    println!("wiki capabilities show");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_manifest(&manifest);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
