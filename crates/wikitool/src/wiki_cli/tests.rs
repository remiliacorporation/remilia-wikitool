use super::WikiJsonView;
use super::summary::{summarize_capability_manifest, summarize_profile_snapshot};
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
            docs_profile: "remilia-wiki".to_string(),
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
                proper_nouns: vec!["Webring".to_string()],
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
fn wiki_json_view_exposes_summary_and_full_names() {
    assert_eq!(WikiJsonView::Summary.as_str(), "summary");
    assert_eq!(WikiJsonView::Full.as_str(), "full");
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
