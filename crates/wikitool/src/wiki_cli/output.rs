use wikitool_core::profile::{
    ProfileOverlay, TemplateCatalogSummary, WikiCapabilityManifest, WikiProfileSnapshot,
};
pub(super) fn print_manifest(manifest: &WikiCapabilityManifest) {
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

pub(super) fn print_profile_snapshot(snapshot: &WikiProfileSnapshot) {
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

pub(super) fn print_overlay(overlay: &ProfileOverlay) {
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

pub(super) fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "<none>".to_string()
    } else {
        values.join(", ")
    }
}

fn format_flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}
