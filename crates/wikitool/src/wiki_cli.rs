use anyhow::Result;
use clap::{Args, Subcommand};
use serde::Serialize;
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
    #[arg(long, default_value = "summary", value_name = "VIEW")]
    view: String,
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
        WikiCapabilitiesSubcommand::Sync(args) => {
            run_wiki_capabilities_sync(runtime, &args.format, &args.view)
        }
        WikiCapabilitiesSubcommand::Show(args) => {
            run_wiki_capabilities_show(runtime, &args.format, &args.view)
        }
    }
}

fn run_wiki_capabilities_sync(runtime: &RuntimeOptions, format: &str, view: &str) -> Result<()> {
    let output_format = normalize_output(format)?;
    let output_view = normalize_view(view)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let manifest = sync_wiki_capabilities_with_config(&paths, &config)?;

    if output_format == "json" {
        if output_view == "full" {
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

fn run_wiki_capabilities_show(runtime: &RuntimeOptions, format: &str, view: &str) -> Result<()> {
    let output_format = normalize_output(format)?;
    let output_view = normalize_view(view)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let manifest = load_wiki_capabilities_with_config(&paths, &config)?.ok_or_else(|| {
        anyhow::anyhow!(
            "wiki capability manifest is missing; run `wikitool wiki capabilities sync`"
        )
    })?;

    if output_format == "json" {
        if output_view == "full" {
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

fn run_wiki_profile(runtime: &RuntimeOptions, args: WikiProfileArgs) -> Result<()> {
    match args.command {
        WikiProfileSubcommand::Sync(args) => {
            run_wiki_profile_sync(runtime, &args.format, &args.view)
        }
        WikiProfileSubcommand::Show(args) => {
            run_wiki_profile_show(runtime, &args.format, &args.view)
        }
    }
}

fn run_wiki_profile_sync(runtime: &RuntimeOptions, format: &str, view: &str) -> Result<()> {
    let output_format = normalize_output(format)?;
    let output_view = normalize_view(view)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let snapshot = sync_wiki_profile_with_config(&paths, &config)?;

    if output_format == "json" {
        if output_view == "full" {
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

fn run_wiki_profile_show(runtime: &RuntimeOptions, format: &str, view: &str) -> Result<()> {
    let output_format = normalize_output(format)?;
    let output_view = normalize_view(view)?;
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let snapshot = load_wiki_profile_with_config(&paths, &config)?;

    if output_format == "json" {
        if output_view == "full" {
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
        "template_catalog.profile_templates: {}",
        if summary.profile_template_titles.is_empty() {
            "<none>".to_string()
        } else {
            summary.profile_template_titles.join(", ")
        }
    );
    println!("template_catalog.refreshed_at: {}", summary.refreshed_at);
}

fn normalize_view(value: &str) -> Result<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "summary" => Ok("summary"),
        "full" => Ok("full"),
        _ => anyhow::bail!("unsupported view: {value} (expected summary|full)"),
    }
}

#[derive(Debug, Serialize)]
struct WikiCapabilityManifestSummary<'a> {
    schema_version: &'a str,
    wiki_id: &'a str,
    wiki_url: &'a str,
    api_url: &'a str,
    rest_url: Option<&'a str>,
    article_path: &'a str,
    mediawiki_version: Option<&'a str>,
    namespace_count: usize,
    extension_count: usize,
    parser_extension_tag_count: usize,
    parser_function_hook_count: usize,
    special_page_count: usize,
    search_backend_hint: Option<&'a str>,
    has_visual_editor: bool,
    has_templatedata: bool,
    has_citoid: bool,
    has_cargo: bool,
    has_page_forms: bool,
    has_short_description: bool,
    has_scribunto: bool,
    has_timed_media_handler: bool,
    supports_parse_api_html: bool,
    supports_rest_html: bool,
    rest_html_path_template: Option<&'a str>,
    refreshed_at: &'a str,
}

#[derive(Debug, Serialize)]
struct WikiProfileSnapshotSummary<'a> {
    base_profile_id: &'a str,
    overlay: ProfileOverlaySummary<'a>,
    capabilities: Option<WikiCapabilityManifestSummary<'a>>,
    template_catalog: Option<TemplateCatalogSummaryView<'a>>,
}

#[derive(Debug, Serialize)]
struct ProfileOverlaySummary<'a> {
    schema_version: &'a str,
    profile_id: &'a str,
    base_profile_id: &'a str,
    docs_profile: &'a str,
    source_document_count: usize,
    authoring: ProfileAuthoringSummary<'a>,
    citations: ProfileCitationSummary<'a>,
    remilia: ProfileRemiliaSummary<'a>,
    categories: ProfileCategorySummary<'a>,
    lint: ProfileLintSummary,
    golden_set: ProfileGoldenSetSummary<'a>,
    refreshed_at: &'a str,
}

#[derive(Debug, Serialize)]
struct ProfileAuthoringSummary<'a> {
    require_short_description: bool,
    short_description_forms: &'a [String],
    require_article_quality_banner: bool,
    article_quality_template: Option<&'a str>,
    article_quality_default_state: Option<&'a str>,
    required_appendix_sections: &'a [String],
    references_template: Option<&'a str>,
    prefer_sentence_case_headings: bool,
    prefer_wikitext_only: bool,
    forbid_markdown: bool,
    require_straight_quotes: bool,
}

#[derive(Debug, Serialize)]
struct ProfileCitationSummary<'a> {
    preferred_templates: &'a [wikitool_core::profile::CitationTemplateRule],
    use_named_references: bool,
    leave_archive_fields_blank: bool,
    unreliable_source_count: usize,
}

#[derive(Debug, Serialize)]
struct ProfileRemiliaSummary<'a> {
    default_parent_group: Option<&'a str>,
    preferred_group_field: Option<&'a str>,
    avoid_group_fields: &'a [String],
    infobox_preferences: &'a [wikitool_core::profile::InfoboxPreference],
}

#[derive(Debug, Serialize)]
struct ProfileCategorySummary<'a> {
    preferred_categories: &'a [String],
    min_per_article: usize,
    max_per_article: usize,
}

#[derive(Debug, Serialize)]
struct ProfileLintSummary {
    banned_phrase_count: usize,
    watchlist_term_count: usize,
    forbid_curly_quotes: bool,
    forbid_placeholder_fragment_count: usize,
}

#[derive(Debug, Serialize)]
struct ProfileGoldenSetSummary<'a> {
    article_corpus_available: bool,
    source_document_count: usize,
    source_documents: &'a [String],
}

#[derive(Debug, Serialize)]
struct TemplateCatalogSummaryView<'a> {
    profile_id: &'a str,
    template_count: usize,
    templatedata_count: usize,
    redirect_alias_count: usize,
    usage_index_ready: bool,
    profile_template_titles: &'a [String],
    refreshed_at: &'a str,
}

fn summarize_capability_manifest<'a>(
    manifest: &'a WikiCapabilityManifest,
) -> WikiCapabilityManifestSummary<'a> {
    WikiCapabilityManifestSummary {
        schema_version: &manifest.schema_version,
        wiki_id: &manifest.wiki_id,
        wiki_url: &manifest.wiki_url,
        api_url: &manifest.api_url,
        rest_url: manifest.rest_url.as_deref(),
        article_path: &manifest.article_path,
        mediawiki_version: manifest.mediawiki_version.as_deref(),
        namespace_count: manifest.namespaces.len(),
        extension_count: manifest.extensions.len(),
        parser_extension_tag_count: manifest.parser_extension_tags.len(),
        parser_function_hook_count: manifest.parser_function_hooks.len(),
        special_page_count: manifest.special_pages.len(),
        search_backend_hint: manifest.search_backend_hint.as_deref(),
        has_visual_editor: manifest.has_visual_editor,
        has_templatedata: manifest.has_templatedata,
        has_citoid: manifest.has_citoid,
        has_cargo: manifest.has_cargo,
        has_page_forms: manifest.has_page_forms,
        has_short_description: manifest.has_short_description,
        has_scribunto: manifest.has_scribunto,
        has_timed_media_handler: manifest.has_timed_media_handler,
        supports_parse_api_html: manifest.supports_parse_api_html,
        supports_rest_html: manifest.supports_rest_html,
        rest_html_path_template: manifest.rest_html_path_template.as_deref(),
        refreshed_at: &manifest.refreshed_at,
    }
}

fn summarize_profile_snapshot<'a>(
    snapshot: &'a WikiProfileSnapshot,
) -> WikiProfileSnapshotSummary<'a> {
    WikiProfileSnapshotSummary {
        base_profile_id: &snapshot.base_profile_id,
        overlay: summarize_overlay(&snapshot.overlay),
        capabilities: snapshot
            .capabilities
            .as_ref()
            .map(summarize_capability_manifest),
        template_catalog: snapshot
            .template_catalog
            .as_ref()
            .map(summarize_template_catalog_summary),
    }
}

fn summarize_overlay<'a>(overlay: &'a ProfileOverlay) -> ProfileOverlaySummary<'a> {
    ProfileOverlaySummary {
        schema_version: &overlay.schema_version,
        profile_id: &overlay.profile_id,
        base_profile_id: &overlay.base_profile_id,
        docs_profile: &overlay.docs_profile,
        source_document_count: overlay.source_documents.len(),
        authoring: ProfileAuthoringSummary {
            require_short_description: overlay.authoring.require_short_description,
            short_description_forms: &overlay.authoring.short_description_forms,
            require_article_quality_banner: overlay.authoring.require_article_quality_banner,
            article_quality_template: overlay.authoring.article_quality_template.as_deref(),
            article_quality_default_state: overlay
                .authoring
                .article_quality_default_state
                .as_deref(),
            required_appendix_sections: &overlay.authoring.required_appendix_sections,
            references_template: overlay.authoring.references_template.as_deref(),
            prefer_sentence_case_headings: overlay.authoring.prefer_sentence_case_headings,
            prefer_wikitext_only: overlay.authoring.prefer_wikitext_only,
            forbid_markdown: overlay.authoring.forbid_markdown,
            require_straight_quotes: overlay.authoring.require_straight_quotes,
        },
        citations: ProfileCitationSummary {
            preferred_templates: &overlay.citations.preferred_templates,
            use_named_references: overlay.citations.use_named_references,
            leave_archive_fields_blank: overlay.citations.leave_archive_fields_blank,
            unreliable_source_count: overlay.citations.unreliable_sources.len(),
        },
        remilia: ProfileRemiliaSummary {
            default_parent_group: overlay.remilia.default_parent_group.as_deref(),
            preferred_group_field: overlay.remilia.preferred_group_field.as_deref(),
            avoid_group_fields: &overlay.remilia.avoid_group_fields,
            infobox_preferences: &overlay.remilia.infobox_preferences,
        },
        categories: ProfileCategorySummary {
            preferred_categories: &overlay.categories.preferred_categories,
            min_per_article: overlay.categories.min_per_article,
            max_per_article: overlay.categories.max_per_article,
        },
        lint: ProfileLintSummary {
            banned_phrase_count: overlay.lint.banned_phrases.len(),
            watchlist_term_count: overlay.lint.watchlist_terms.len(),
            forbid_curly_quotes: overlay.lint.forbid_curly_quotes,
            forbid_placeholder_fragment_count: overlay.lint.forbid_placeholder_fragments.len(),
        },
        golden_set: ProfileGoldenSetSummary {
            article_corpus_available: overlay.golden_set.article_corpus_available,
            source_document_count: overlay.golden_set.source_documents.len(),
            source_documents: &overlay.golden_set.source_documents,
        },
        refreshed_at: &overlay.refreshed_at,
    }
}

fn summarize_template_catalog_summary<'a>(
    summary: &'a TemplateCatalogSummary,
) -> TemplateCatalogSummaryView<'a> {
    TemplateCatalogSummaryView {
        profile_id: &summary.profile_id,
        template_count: summary.template_count,
        templatedata_count: summary.templatedata_count,
        redirect_alias_count: summary.redirect_alias_count,
        usage_index_ready: summary.usage_index_ready,
        profile_template_titles: &summary.profile_template_titles,
        refreshed_at: &summary.refreshed_at,
    }
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

#[cfg(test)]
mod tests {
    use super::{normalize_view, summarize_capability_manifest, summarize_profile_snapshot};
    use wikitool_core::profile::{
        AuthoringRules, CategoryRules, CitationRules, CitationTemplateRule, GoldenSetRules,
        InfoboxPreference, LintRules, ProfileOverlay, ProfileSourceDocument, RemiliaRules,
        TemplateCatalogSummary, WikiCapabilityManifest, WikiProfileSnapshot,
    };

    fn sample_manifest() -> WikiCapabilityManifest {
        WikiCapabilityManifest {
            schema_version: "wiki_capabilities_v1".to_string(),
            wiki_id: "remilia".to_string(),
            wiki_url: "https://wiki.example".to_string(),
            api_url: "https://wiki.example/api.php".to_string(),
            rest_url: Some("https://wiki.example/rest.php".to_string()),
            article_path: "/wiki/$1".to_string(),
            mediawiki_version: Some("1.44".to_string()),
            namespaces: vec![wikitool_core::profile::NamespaceInfo {
                id: 0,
                canonical_name: Some(String::new()),
                display_name: "Main".to_string(),
            }],
            extensions: vec![wikitool_core::profile::ExtensionInfo {
                name: "Scribunto".to_string(),
                version: Some("1.0".to_string()),
                category: Some("parser".to_string()),
            }],
            parser_extension_tags: vec!["gallery".to_string()],
            parser_function_hooks: vec!["if".to_string()],
            special_pages: vec!["Version".to_string()],
            search_backend_hint: Some("cirrus".to_string()),
            has_visual_editor: false,
            has_templatedata: true,
            has_citoid: false,
            has_cargo: false,
            has_page_forms: false,
            has_short_description: true,
            has_scribunto: true,
            has_timed_media_handler: false,
            supports_parse_api_html: true,
            supports_rest_html: true,
            rest_html_path_template: Some("/rest.php/page/html/$1".to_string()),
            refreshed_at: "1739000000".to_string(),
        }
    }

    fn sample_snapshot() -> WikiProfileSnapshot {
        WikiProfileSnapshot {
            base_profile_id: "remilia-base".to_string(),
            overlay: ProfileOverlay {
                schema_version: "profile_overlay_v1".to_string(),
                profile_id: "remilia".to_string(),
                base_profile_id: "remilia-base".to_string(),
                docs_profile: "remilia-mw-1.44".to_string(),
                source_documents: vec![ProfileSourceDocument {
                    relative_path: "docs/profile.md".to_string(),
                    content_hash: "abc".to_string(),
                }],
                authoring: AuthoringRules {
                    require_short_description: true,
                    short_description_forms: vec!["SHORTDESC".to_string()],
                    require_article_quality_banner: true,
                    article_quality_template: Some("Template:Article quality".to_string()),
                    article_quality_default_state: Some("unverified".to_string()),
                    required_appendix_sections: vec!["References".to_string()],
                    references_template: Some("Template:Reflist".to_string()),
                    prefer_sentence_case_headings: true,
                    prefer_wikitext_only: true,
                    forbid_markdown: true,
                    require_straight_quotes: true,
                },
                citations: CitationRules {
                    preferred_templates: vec![CitationTemplateRule {
                        family: "web".to_string(),
                        template_title: "Template:Cite web".to_string(),
                    }],
                    use_named_references: true,
                    leave_archive_fields_blank: true,
                    unreliable_sources: Vec::new(),
                },
                remilia: RemiliaRules {
                    default_parent_group: Some("Remilia".to_string()),
                    preferred_group_field: Some("parent_group".to_string()),
                    avoid_group_fields: vec!["group".to_string()],
                    infobox_preferences: vec![InfoboxPreference {
                        subject_type: "concept".to_string(),
                        template_title: "Template:Infobox concept".to_string(),
                    }],
                },
                categories: CategoryRules {
                    preferred_categories: vec!["Category:Ideas".to_string()],
                    min_per_article: 1,
                    max_per_article: 4,
                },
                lint: LintRules {
                    banned_phrases: vec!["foo".to_string()],
                    watchlist_terms: vec!["bar".to_string()],
                    forbid_curly_quotes: true,
                    forbid_placeholder_fragments: vec!["todo".to_string()],
                },
                golden_set: GoldenSetRules {
                    article_corpus_available: true,
                    source_documents: vec!["alpha".to_string()],
                },
                refreshed_at: "1739000000".to_string(),
            },
            capabilities: Some(sample_manifest()),
            template_catalog: Some(TemplateCatalogSummary {
                profile_id: "remilia".to_string(),
                template_count: 2,
                templatedata_count: 1,
                redirect_alias_count: 1,
                usage_index_ready: true,
                profile_template_titles: vec!["Template:Infobox concept".to_string()],
                refreshed_at: "1739000000".to_string(),
            }),
        }
    }

    #[test]
    fn normalize_view_accepts_summary_and_full() {
        assert_eq!(normalize_view("summary").expect("summary"), "summary");
        assert_eq!(normalize_view("full").expect("full"), "full");
        assert!(normalize_view("other").is_err());
    }

    #[test]
    fn capability_summary_json_omits_raw_arrays() {
        let manifest = sample_manifest();
        let summary = summarize_capability_manifest(&manifest);
        let summary_json = serde_json::to_value(&summary).expect("summary json");
        let full_json = serde_json::to_value(&manifest).expect("full json");

        assert!(summary_json.get("namespaces").is_none());
        assert!(summary_json.get("extensions").is_none());
        assert!(summary_json.get("special_pages").is_none());
        assert_eq!(
            summary_json.get("namespace_count").and_then(|v| v.as_u64()),
            Some(1)
        );
        assert!(full_json.get("namespaces").is_some());
        assert!(full_json.get("extensions").is_some());
    }

    #[test]
    fn profile_summary_json_uses_profile_template_titles() {
        let snapshot = sample_snapshot();
        let summary = summarize_profile_snapshot(&snapshot);
        let summary_json = serde_json::to_value(&summary).expect("summary json");
        let full_json = serde_json::to_value(&snapshot).expect("full json");

        let template_catalog = summary_json
            .get("template_catalog")
            .and_then(|value| value.as_object())
            .expect("template catalog summary");
        assert!(template_catalog.get("profile_template_titles").is_some());
        assert!(
            template_catalog
                .get("recommended_template_titles")
                .is_none()
        );
        assert!(
            summary_json
                .get("capabilities")
                .and_then(|value| value.get("extensions"))
                .is_none()
        );
        assert!(
            full_json
                .get("capabilities")
                .and_then(|value| value.get("extensions"))
                .is_some()
        );
    }
}
