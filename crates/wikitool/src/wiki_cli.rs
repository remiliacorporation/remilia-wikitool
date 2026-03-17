use anyhow::Result;
use clap::{Args, Subcommand};
use wikitool_core::profile::{
    ProfileOverlay, TemplateCatalogSummary, WikiCapabilityManifest, WikiProfileSnapshot,
    load_or_build_remilia_profile_overlay, load_wiki_capabilities_with_config,
    load_wiki_profile_with_config, sync_wiki_capabilities_with_config,
    sync_wiki_profile_with_config,
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
    #[command(about = "Show the combined live/profile-aware wiki surface")]
    Profile(WikiProfileArgs),
    #[command(about = "Show the structured local editorial rules overlay")]
    Rules(WikiRulesArgs),
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

#[derive(Debug, Args)]
pub(crate) struct WikiProfileArgs {
    #[command(subcommand)]
    command: WikiProfileSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiProfileSubcommand {
    #[command(about = "Refresh the local rules overlay and live capability snapshot")]
    Sync(WikiCapabilitiesFormatArgs),
    #[command(about = "Show the current combined profile snapshot")]
    Show(WikiCapabilitiesFormatArgs),
}

#[derive(Debug, Args)]
pub(crate) struct WikiRulesArgs {
    #[command(subcommand)]
    command: WikiRulesSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiRulesSubcommand {
    #[command(about = "Show the current Remilia rules overlay")]
    Show(WikiCapabilitiesFormatArgs),
}

pub(crate) fn run_wiki(runtime: &RuntimeOptions, args: WikiArgs) -> Result<()> {
    match args.command {
        WikiSubcommand::Capabilities(args) => run_wiki_capabilities(runtime, args),
        WikiSubcommand::Profile(args) => run_wiki_profile(runtime, args),
        WikiSubcommand::Rules(args) => run_wiki_rules(runtime, args),
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

fn run_wiki_profile(runtime: &RuntimeOptions, args: WikiProfileArgs) -> Result<()> {
    match args.command {
        WikiProfileSubcommand::Sync(args) => run_wiki_profile_sync(runtime, &args.format),
        WikiProfileSubcommand::Show(args) => run_wiki_profile_show(runtime, &args.format),
    }
}

fn run_wiki_profile_sync(runtime: &RuntimeOptions, format: &str) -> Result<()> {
    let output_format = normalize_output(format)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let snapshot = sync_wiki_profile_with_config(&paths, &config)?;

    if output_format == "json" {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
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

fn run_wiki_profile_show(runtime: &RuntimeOptions, format: &str) -> Result<()> {
    let output_format = normalize_output(format)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let snapshot = load_wiki_profile_with_config(&paths, &config)?;

    if output_format == "json" {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
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

fn run_wiki_rules(runtime: &RuntimeOptions, args: WikiRulesArgs) -> Result<()> {
    match args.command {
        WikiRulesSubcommand::Show(args) => run_wiki_rules_show(runtime, &args.format),
    }
}

fn run_wiki_rules_show(runtime: &RuntimeOptions, format: &str) -> Result<()> {
    let output_format = normalize_output(format)?;
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    let overlay = load_or_build_remilia_profile_overlay(&paths)?;

    if output_format == "json" {
        println!("{}", serde_json::to_string_pretty(&overlay)?);
        return Ok(());
    }

    println!("wiki rules show");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_overlay(&overlay);
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

fn print_profile_snapshot(snapshot: &WikiProfileSnapshot) {
    println!("profile_id: {}", snapshot.overlay.profile_id);
    println!("base_profile_id: {}", snapshot.base_profile_id);
    println!("docs_profile: {}", snapshot.overlay.docs_profile);
    if let Some(manifest) = snapshot.capabilities.as_ref() {
        println!("capabilities.wiki_id: {}", manifest.wiki_id);
        println!(
            "capabilities.mediawiki_version: {}",
            manifest.mediawiki_version.as_deref().unwrap_or("<unknown>")
        );
        println!(
            "capabilities.extension_count: {}",
            manifest.extensions.len()
        );
        println!(
            "capabilities.search_backend_hint: {}",
            manifest.search_backend_hint.as_deref().unwrap_or("<none>")
        );
    } else {
        println!("capabilities: <none>");
    }
    if let Some(summary) = snapshot.template_catalog.as_ref() {
        print_template_catalog_summary(summary);
    } else {
        println!("template_catalog: <none>");
    }
    print_overlay(&snapshot.overlay);
}

fn print_template_catalog_summary(summary: &TemplateCatalogSummary) {
    println!("template_catalog.profile_id: {}", summary.profile_id);
    println!(
        "template_catalog.template_count: {}",
        summary.template_count
    );
    println!(
        "template_catalog.templatedata_count: {}",
        summary.templatedata_count
    );
    println!(
        "template_catalog.redirect_alias_count: {}",
        summary.redirect_alias_count
    );
    println!(
        "template_catalog.usage_index_ready: {}",
        format_flag(summary.usage_index_ready)
    );
    println!(
        "template_catalog.recommended_templates: {}",
        if summary.recommended_template_titles.is_empty() {
            "<none>".to_string()
        } else {
            summary.recommended_template_titles.join(", ")
        }
    );
    println!("template_catalog.refreshed_at: {}", summary.refreshed_at);
}

fn print_overlay(overlay: &ProfileOverlay) {
    println!(
        "rules.source_document_count: {}",
        overlay.source_documents.len()
    );
    println!(
        "rules.require_short_description: {}",
        format_flag(overlay.authoring.require_short_description)
    );
    println!(
        "rules.require_article_quality_banner: {}",
        format_flag(overlay.authoring.require_article_quality_banner)
    );
    println!(
        "rules.article_quality_template: {}",
        overlay
            .authoring
            .article_quality_template
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "rules.references_template: {}",
        overlay
            .authoring
            .references_template
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "rules.prefer_sentence_case_headings: {}",
        format_flag(overlay.authoring.prefer_sentence_case_headings)
    );
    println!(
        "rules.prefer_wikitext_only: {}",
        format_flag(overlay.authoring.prefer_wikitext_only)
    );
    println!(
        "rules.require_straight_quotes: {}",
        format_flag(overlay.authoring.require_straight_quotes)
    );
    println!(
        "rules.preferred_citation_templates: {}",
        join_or_none(
            &overlay
                .citations
                .preferred_templates
                .iter()
                .map(|rule| rule.template_title.clone())
                .collect::<Vec<_>>()
        )
    );
    println!(
        "rules.preferred_infobox_templates: {}",
        join_or_none(
            &overlay
                .remilia
                .infobox_preferences
                .iter()
                .map(|rule| rule.template_title.clone())
                .collect::<Vec<_>>()
        )
    );
    println!(
        "rules.default_parent_group: {}",
        overlay
            .remilia
            .default_parent_group
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "rules.preferred_categories: {}",
        join_or_none(&overlay.categories.preferred_categories)
    );
    println!(
        "rules.banned_phrase_count: {}",
        overlay.lint.banned_phrases.len()
    );
    println!(
        "rules.unreliable_source_count: {}",
        overlay.citations.unreliable_sources.len()
    );
    println!(
        "rules.article_corpus_available: {}",
        format_flag(overlay.golden_set.article_corpus_available)
    );
    println!("rules.refreshed_at: {}", overlay.refreshed_at);
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "<none>".to_string()
    } else {
        values.join(", ")
    }
}

fn format_flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}
