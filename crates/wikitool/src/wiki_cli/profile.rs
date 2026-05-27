use anyhow::Result;
use wikitool_core::profile::{
    fetch_remote_wiki_capabilities, load_wiki_profile_with_config, sync_wiki_profile_with_config,
};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::{print_manifest, print_profile_snapshot};
use super::summary::{
    RemoteWikiProfileReport, summarize_profile_snapshot, summarize_remote_profile_report,
};
use super::*;
pub(super) fn run_wiki_profile(runtime: &RuntimeOptions, args: WikiProfileArgs) -> Result<()> {
    match args.command {
        WikiProfileSubcommand::Sync(args) => run_wiki_profile_sync(runtime, args.format, args.view),
        WikiProfileSubcommand::Show(args) => run_wiki_profile_show(runtime, args.format, args.view),
        WikiProfileSubcommand::Remote(args) => run_wiki_profile_remote(runtime, args),
    }
}

fn run_wiki_profile_sync(
    runtime: &RuntimeOptions,
    format: OutputFormat,
    view: WikiJsonView,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let snapshot = sync_wiki_profile_with_config(&paths, &config)?;

    if format.is_json() {
        if view.is_full() {
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&summarize_profile_snapshot(&snapshot))?
            );
        }
        return Ok(());
    }

    println!("wiki profile sync");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_profile_snapshot(&snapshot);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_wiki_profile_show(
    runtime: &RuntimeOptions,
    format: OutputFormat,
    view: WikiJsonView,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let snapshot = load_wiki_profile_with_config(&paths, &config)?;

    if format.is_json() {
        if view.is_full() {
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&summarize_profile_snapshot(&snapshot))?
            );
        }
        return Ok(());
    }

    println!("wiki profile show");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_profile_snapshot(&snapshot);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_wiki_profile_remote(runtime: &RuntimeOptions, args: WikiRemoteProfileArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let manifest = fetch_remote_wiki_capabilities(&args.url, &config)?;
    let report = RemoteWikiProfileReport {
        schema_version: "remote_wiki_profile_v1",
        profile_scope: "remote_live_capability_probe",
        source_url: &args.url,
        storage: "not_stored",
        target_compatibility_note: "This report describes the remote wiki capability surface only; templates, modules, local files, and article lint authority still require that target wiki's own catalog or a local import.",
        capabilities: &manifest,
    };

    if args.format.is_json() {
        if args.view.is_full() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&summarize_remote_profile_report(&report))?
            );
        }
        return Ok(());
    }

    println!("wiki profile remote");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("profile_scope: {}", report.profile_scope);
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
