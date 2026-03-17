use anyhow::Result;
use clap::{Args, Subcommand};
use wikitool_core::profile::{
    WikiCapabilityManifest, load_wiki_capabilities_with_config, sync_wiki_capabilities_with_config,
};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::query_cli::normalize_output;
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct WikiArgs {
    #[command(subcommand)]
    command: WikiSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiSubcommand {
    #[command(about = "Sync and inspect live wiki capability manifests")]
    Capabilities(WikiCapabilitiesArgs),
}

#[derive(Debug, Args)]
pub(crate) struct WikiCapabilitiesArgs {
    #[command(subcommand)]
    command: WikiCapabilitiesSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiCapabilitiesSubcommand {
    #[command(about = "Fetch and store the current live wiki capability manifest")]
    Sync(WikiCapabilitiesFormatArgs),
    #[command(about = "Show the last stored wiki capability manifest")]
    Show(WikiCapabilitiesFormatArgs),
}

#[derive(Debug, Args)]
struct WikiCapabilitiesFormatArgs {
    #[arg(long, default_value = "text", value_name = "FORMAT")]
    format: String,
}

pub(crate) fn run_wiki(runtime: &RuntimeOptions, args: WikiArgs) -> Result<()> {
    match args.command {
        WikiSubcommand::Capabilities(args) => run_wiki_capabilities(runtime, args),
    }
}

fn run_wiki_capabilities(runtime: &RuntimeOptions, args: WikiCapabilitiesArgs) -> Result<()> {
    match args.command {
        WikiCapabilitiesSubcommand::Sync(args) => run_wiki_capabilities_sync(runtime, &args.format),
        WikiCapabilitiesSubcommand::Show(args) => run_wiki_capabilities_show(runtime, &args.format),
    }
}

fn run_wiki_capabilities_sync(runtime: &RuntimeOptions, format: &str) -> Result<()> {
    let output_format = normalize_output(format)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let manifest = sync_wiki_capabilities_with_config(&paths, &config)?;

    if output_format == "json" {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
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

fn run_wiki_capabilities_show(runtime: &RuntimeOptions, format: &str) -> Result<()> {
    let output_format = normalize_output(format)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let manifest = load_wiki_capabilities_with_config(&paths, &config)?.ok_or_else(|| {
        anyhow::anyhow!(
            "wiki capability manifest is missing; run `wikitool wiki capabilities sync`"
        )
    })?;

    if output_format == "json" {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
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

fn print_manifest(manifest: &WikiCapabilityManifest) {
    println!("wiki_id: {}", manifest.wiki_id);
    println!("wiki_url: {}", manifest.wiki_url);
    println!("api_url: {}", manifest.api_url);
    println!(
        "rest_url: {}",
        manifest.rest_url.as_deref().unwrap_or("<none>")
    );
    println!("article_path: {}", manifest.article_path);
    println!(
        "mediawiki_version: {}",
        manifest.mediawiki_version.as_deref().unwrap_or("<unknown>")
    );
    println!("namespace_count: {}", manifest.namespaces.len());
    println!("extension_count: {}", manifest.extensions.len());
    println!(
        "parser_extension_tag_count: {}",
        manifest.parser_extension_tags.len()
    );
    println!(
        "parser_function_hook_count: {}",
        manifest.parser_function_hooks.len()
    );
    println!("special_page_count: {}", manifest.special_pages.len());
    println!(
        "search_backend_hint: {}",
        manifest.search_backend_hint.as_deref().unwrap_or("<none>")
    );
    println!(
        "supports_parse_api_html: {}",
        format_flag(manifest.supports_parse_api_html)
    );
    println!(
        "supports_rest_html: {}",
        format_flag(manifest.supports_rest_html)
    );
    if let Some(value) = manifest.rest_html_path_template.as_deref() {
        println!("rest_html_path_template: {value}");
    }
    println!(
        "has_visual_editor: {}",
        format_flag(manifest.has_visual_editor)
    );
    println!(
        "has_templatedata: {}",
        format_flag(manifest.has_templatedata)
    );
    println!("has_citoid: {}", format_flag(manifest.has_citoid));
    println!("has_cargo: {}", format_flag(manifest.has_cargo));
    println!("has_page_forms: {}", format_flag(manifest.has_page_forms));
    println!(
        "has_short_description: {}",
        format_flag(manifest.has_short_description)
    );
    println!("has_scribunto: {}", format_flag(manifest.has_scribunto));
    println!(
        "has_timed_media_handler: {}",
        format_flag(manifest.has_timed_media_handler)
    );
    println!("refreshed_at: {}", manifest.refreshed_at);
}

fn format_flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}
