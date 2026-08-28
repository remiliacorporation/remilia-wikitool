use serde::Serialize;
use wikitool_core::site::WikiCapabilityManifest;

#[derive(Debug, Serialize)]
pub(super) struct WikiCapabilityManifestSummary<'a> {
    pub(super) schema_version: &'a str,
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
pub(super) struct RemoteWikiCapabilitiesReport<'a> {
    pub(super) schema_version: &'a str,
    pub(super) capability_scope: &'a str,
    pub(super) source_url: &'a str,
    pub(super) storage: &'a str,
    pub(super) target_compatibility_note: &'a str,
    pub(super) capabilities: &'a WikiCapabilityManifest,
}

#[derive(Debug, Serialize)]
pub(super) struct RemoteWikiCapabilitiesSummary<'a> {
    pub(super) schema_version: &'a str,
    pub(super) capability_scope: &'a str,
    pub(super) source_url: &'a str,
    pub(super) storage: &'a str,
    pub(super) target_compatibility_note: &'a str,
    pub(super) capabilities: WikiCapabilityManifestSummary<'a>,
}

pub(super) fn summarize_capability_manifest(
    manifest: &WikiCapabilityManifest,
) -> WikiCapabilityManifestSummary<'_> {
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

pub(super) fn summarize_remote_capabilities_report<'a>(
    report: &'a RemoteWikiCapabilitiesReport<'a>,
) -> RemoteWikiCapabilitiesSummary<'a> {
    RemoteWikiCapabilitiesSummary {
        schema_version: report.schema_version,
        capability_scope: report.capability_scope,
        source_url: report.source_url,
        storage: report.storage,
        target_compatibility_note: report.target_compatibility_note,
        capabilities: summarize_capability_manifest(report.capabilities),
    }
}
