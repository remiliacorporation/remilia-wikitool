use anyhow::Result;
use wikitool_core::profile::{
    load_wiki_capabilities_with_config, sync_wiki_capabilities_with_config,
};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::print_manifest;
use super::summary::summarize_capability_manifest;
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
    }
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
