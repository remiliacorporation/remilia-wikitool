use serde::Serialize;
use wikitool_core::profile::{
    ProfileOverlay, TemplateCatalogSummary, WikiCapabilityManifest, WikiProfileSnapshot,
};
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
pub(super) struct WikiProfileSnapshotSummary<'a> {
    base_profile_id: &'a str,
    overlay: ProfileOverlaySummary<'a>,
    pub(super) capabilities: Option<WikiCapabilityManifestSummary<'a>>,
    template_catalog: Option<TemplateCatalogSummaryView<'a>>,
}

#[derive(Debug, Serialize)]
pub(super) struct RemoteWikiProfileReport<'a> {
    pub(super) schema_version: &'a str,
    pub(super) profile_scope: &'a str,
    pub(super) source_url: &'a str,
    pub(super) storage: &'a str,
    pub(super) target_compatibility_note: &'a str,
    pub(super) capabilities: &'a WikiCapabilityManifest,
}

#[derive(Debug, Serialize)]
pub(super) struct RemoteWikiProfileSummary<'a> {
    pub(super) schema_version: &'a str,
    pub(super) profile_scope: &'a str,
    pub(super) source_url: &'a str,
    pub(super) storage: &'a str,
    pub(super) target_compatibility_note: &'a str,
    pub(super) capabilities: WikiCapabilityManifestSummary<'a>,
}

#[derive(Debug, Serialize)]
pub(super) struct ProfileOverlaySummary<'a> {
    pub(super) schema_version: &'a str,
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
pub(super) struct ProfileAuthoringSummary<'a> {
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
pub(super) struct ProfileCitationSummary<'a> {
    preferred_templates: &'a [wikitool_core::profile::CitationTemplateRule],
    use_named_references: bool,
    leave_archive_fields_blank: bool,
    unreliable_source_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ProfileRemiliaSummary<'a> {
    default_parent_group: Option<&'a str>,
    preferred_group_field: Option<&'a str>,
    avoid_group_fields: &'a [String],
    infobox_preferences: &'a [wikitool_core::profile::InfoboxPreference],
}

#[derive(Debug, Serialize)]
pub(super) struct ProfileCategorySummary<'a> {
    preferred_categories: &'a [String],
    min_per_article: usize,
    max_per_article: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ProfileLintSummary {
    banned_phrase_count: usize,
    watchlist_term_count: usize,
    forbid_curly_quotes: bool,
    forbid_placeholder_fragment_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ProfileGoldenSetSummary<'a> {
    article_corpus_available: bool,
    source_document_count: usize,
    source_documents: &'a [String],
}

#[derive(Debug, Serialize)]
pub(super) struct TemplateCatalogSummaryView<'a> {
    profile_id: &'a str,
    template_count: usize,
    templatedata_count: usize,
    redirect_alias_count: usize,
    usage_index_ready: bool,
    profile_template_titles: &'a [String],
    refreshed_at: &'a str,
}

pub(super) fn summarize_capability_manifest<'a>(
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

pub(super) fn summarize_profile_snapshot<'a>(
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

pub(super) fn summarize_remote_profile_report<'a>(
    report: &'a RemoteWikiProfileReport<'a>,
) -> RemoteWikiProfileSummary<'a> {
    RemoteWikiProfileSummary {
        schema_version: report.schema_version,
        profile_scope: report.profile_scope,
        source_url: report.source_url,
        storage: report.storage,
        target_compatibility_note: report.target_compatibility_note,
        capabilities: summarize_capability_manifest(report.capabilities),
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
