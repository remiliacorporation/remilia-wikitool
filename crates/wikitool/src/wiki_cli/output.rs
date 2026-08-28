use wikitool_core::site::WikiCapabilityManifest;

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
    println!("magic_word_count: {}", manifest.magic_words.len());
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
