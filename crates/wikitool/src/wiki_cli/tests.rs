use super::WikiJsonView;
use super::summary::summarize_capability_manifest;
use wikitool_core::site::WikiCapabilityManifest;

fn sample_manifest() -> WikiCapabilityManifest {
    WikiCapabilityManifest {
        schema_version: "wiki_capabilities_v1".to_string(),
        wiki_id: "example".to_string(),
        wiki_url: "https://wiki.example".to_string(),
        api_url: "https://wiki.example/api.php".to_string(),
        rest_url: Some("https://wiki.example/rest.php".to_string()),
        article_path: "/wiki/$1".to_string(),
        mediawiki_version: Some("1.44".to_string()),
        namespaces: vec![wikitool_core::site::NamespaceInfo {
            id: 0,
            canonical_name: Some(String::new()),
            display_name: "Main".to_string(),
        }],
        extensions: vec![wikitool_core::site::ExtensionInfo {
            name: "Scribunto".to_string(),
            version: Some("1.0".to_string()),
            category: Some("parser".to_string()),
        }],
        parser_extension_tags: vec!["gallery".to_string()],
        parser_function_hooks: vec!["if".to_string()],
        magic_words: Vec::new(),
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
