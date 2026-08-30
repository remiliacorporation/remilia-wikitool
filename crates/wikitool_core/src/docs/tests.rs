use std::collections::BTreeMap;

use super::{
    DOCS_NAMESPACE_MANUAL, DocsApi, RemoteDocsPage, TechnicalDocType, TechnicalImportTask,
    import::collect_pages_for_technical_task,
};

#[derive(Default)]
struct MockDocsApi {
    subpages: Vec<String>,
    pages: BTreeMap<String, RemoteDocsPage>,
    default_page_content: Option<String>,
    subpage_calls: Vec<(String, i32, usize)>,
    page_calls: Vec<String>,
}

impl DocsApi for MockDocsApi {
    fn get_subpages(
        &mut self,
        prefix: &str,
        namespace: i32,
        limit: usize,
    ) -> anyhow::Result<Vec<String>> {
        self.subpage_calls
            .push((prefix.to_string(), namespace, limit));
        Ok(self.subpages.clone())
    }

    fn get_page(&mut self, title: &str) -> anyhow::Result<Option<RemoteDocsPage>> {
        self.page_calls.push(title.to_string());
        Ok(self.pages.get(title).cloned().or_else(|| {
            self.default_page_content
                .as_ref()
                .map(|content| RemoteDocsPage {
                    requested_title: title.to_string(),
                    title: title.to_string(),
                    timestamp: String::new(),
                    content: format!("{content} {title}"),
                })
        }))
    }

    fn request_count(&self) -> usize {
        self.page_calls.len() + self.subpage_calls.len()
    }
}

#[test]
fn collect_pages_for_technical_task_uses_mediawiki_namespace_and_skips_translation_variants() {
    let mut api = MockDocsApi {
        subpages: vec![
            "Manual:Hooks/PageSaveComplete/en".to_string(),
            "Manual:Hooks/PageSaveComplete".to_string(),
        ],
        pages: BTreeMap::from([
            (
                "Manual:Hooks".to_string(),
                RemoteDocsPage {
                    requested_title: "Manual:Hooks".to_string(),
                    title: "Manual:Hooks".to_string(),
                    timestamp: String::new(),
                    content: "Hooks index".to_string(),
                },
            ),
            (
                "Manual:Hooks/PageSaveComplete".to_string(),
                RemoteDocsPage {
                    requested_title: "Manual:Hooks/PageSaveComplete".to_string(),
                    title: "Manual:Hooks/PageSaveComplete".to_string(),
                    timestamp: String::new(),
                    content: "PageSaveComplete docs".to_string(),
                },
            ),
        ]),
        default_page_content: None,
        subpage_calls: Vec::new(),
        page_calls: Vec::new(),
    };
    let mut task = TechnicalImportTask {
        doc_type: TechnicalDocType::Hooks,
        page_title: None,
        include_subpages: true,
    };

    let pages = collect_pages_for_technical_task(&mut api, &mut task, 25).unwrap();

    assert_eq!(api.subpage_calls.len(), 1);
    assert_eq!(api.subpage_calls[0].0, "Manual:Hooks/");
    assert_eq!(api.subpage_calls[0].1, DOCS_NAMESPACE_MANUAL);
    assert!(
        !api.page_calls
            .iter()
            .any(|title| title == "Manual:Hooks/PageSaveComplete/en")
    );
    assert_eq!(pages.len(), 2);
    assert!(
        pages
            .iter()
            .any(|page| page.page_title == "Manual:Hooks/PageSaveComplete")
    );
}

#[test]
fn import_docs_profile_skips_installed_extension_discovery_failures() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(&project_root).expect("create project root");
    let context = crate::runtime::ResolutionContext {
        cwd: project_root.clone(),
        executable_dir: None,
    };
    let overrides = crate::runtime::PathOverrides {
        project_root: Some(project_root.clone()),
        ..crate::runtime::PathOverrides::default()
    };
    let paths = crate::runtime::resolve_paths(&context, &overrides).expect("resolve runtime");
    crate::runtime::init_layout(&paths, &crate::runtime::InitOptions::default())
        .expect("init runtime");

    let mut api = MockDocsApi {
        subpages: Vec::new(),
        pages: BTreeMap::new(),
        default_page_content: Some("Profile docs fixture".to_string()),
        subpage_calls: Vec::new(),
        page_calls: Vec::new(),
    };
    let report = super::import_docs_profile_with_api(
        &paths,
        &super::DocsImportProfileOptions {
            profile: "mw-1.44-site-authoring".to_string(),
            include_installed_extensions: false,
            include_extension_subpages: false,
            extra_extensions: Vec::new(),
            limit: 2,
        },
        &crate::config::WikiConfig::default(),
        &mut api,
    )
    .expect("profile import should degrade cleanly");

    assert_eq!(report.profile, "mw-1.44-site-authoring");
    assert!(report.imported_pages > 0);
    assert!(
        report
            .failures
            .iter()
            .any(|entry| entry.contains("installed extension discovery skipped"))
    );
    let hits = super::search_docs(
        &paths,
        "Profile docs fixture",
        &super::DocsSearchOptions {
            tier: None,
            profile: Some("mw-1.44-site-authoring".to_string()),
            limit: 5,
        },
    )
    .expect("profile import should leave docs FTS searchable");
    assert!(!hits.is_empty());
}

#[test]
fn unprofiled_bundles_are_global_without_leaking_named_profiles() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(&project_root).expect("create project root");
    let context = crate::runtime::ResolutionContext {
        cwd: project_root.clone(),
        executable_dir: None,
    };
    let overrides = crate::runtime::PathOverrides {
        project_root: Some(project_root.clone()),
        ..crate::runtime::PathOverrides::default()
    };
    let paths = crate::runtime::resolve_paths(&context, &overrides).expect("resolve runtime");
    crate::runtime::init_layout(&paths, &crate::runtime::InitOptions::default())
        .expect("init runtime");

    let bundle_path = project_root.join("docs-bundle-v1.json");
    std::fs::write(
        &bundle_path,
        r#"{
  "schema_version": 1,
  "source": "unprofiled_bundle_test",
  "extensions": [
    {
      "extension_name": "TestExtension",
      "pages": [
        {
          "page_title": "Extension:TestExtension",
          "content": "AlphaBetaToken and BetaToken configure $wgTestExtensionEnable = true;"
        }
      ]
    }
  ]
}"#,
    )
    .expect("write bundle fixture");
    let report = super::import_docs_bundle(&paths, &bundle_path).expect("import bundle");
    assert_eq!(report.imported_extensions, 1);
    assert!(report.imported_pages > 0);

    for profile in ["mw-1.44-authoring", "mw-1.44-site-authoring"] {
        let hits = super::search_docs(
            &paths,
            "BetaToken",
            &super::DocsSearchOptions {
                tier: None,
                profile: Some(profile.to_string()),
                limit: 10,
            },
        )
        .expect("search generic bundle through a named profile");
        assert!(
            hits.iter()
                .any(|hit| hit.page_title == "Extension:TestExtension")
        );
        assert!(hits.iter().all(|hit| hit.source_profile.is_empty()));

        let symbols = super::lookup_docs_symbols(
            &paths,
            "$wgTestExtensionEnable",
            &super::DocsSymbolLookupOptions {
                kind: None,
                profile: Some(profile.to_string()),
                limit: 10,
            },
        )
        .expect("lookup generic bundle symbol through a named profile");
        assert!(
            symbols
                .iter()
                .any(|symbol| symbol.symbol_name == "$wgTestExtensionEnable")
        );
        assert!(
            symbols
                .iter()
                .all(|symbol| symbol.source_profile.is_empty())
        );

        let context = super::build_docs_context(
            &paths,
            "AlphaBetaToken",
            &super::DocsContextOptions {
                profile: Some(profile.to_string()),
                limit: 10,
                token_budget: 2_000,
            },
        )
        .expect("build context from a generic bundle through a named profile");
        assert!(
            !context.pages.is_empty()
                || !context.sections.is_empty()
                || !context.symbols.is_empty()
                || !context.examples.is_empty()
        );
    }

    let mut api = MockDocsApi {
        subpages: Vec::new(),
        pages: BTreeMap::new(),
        default_page_content: Some(
            "ProfileOnlyNeedle configuration: $wgProfileOnlyFlag = true;".to_string(),
        ),
        subpage_calls: Vec::new(),
        page_calls: Vec::new(),
    };
    super::import_docs_profile_with_api(
        &paths,
        &super::DocsImportProfileOptions {
            profile: "mw-1.44-site-authoring".to_string(),
            include_installed_extensions: false,
            include_extension_subpages: false,
            extra_extensions: Vec::new(),
            limit: 2,
        },
        &crate::config::WikiConfig::default(),
        &mut api,
    )
    .expect("import named profile fixture");

    let matching_profile = super::search_docs(
        &paths,
        "ProfileOnlyNeedle",
        &super::DocsSearchOptions {
            tier: None,
            profile: Some("mw-1.44-site-authoring".to_string()),
            limit: 10,
        },
    )
    .expect("search matching profile");
    assert!(!matching_profile.is_empty());

    let other_profile = super::search_docs(
        &paths,
        "ProfileOnlyNeedle",
        &super::DocsSearchOptions {
            tier: None,
            profile: Some("mw-1.44-authoring".to_string()),
            limit: 10,
        },
    )
    .expect("search isolated profile");
    assert!(other_profile.is_empty());

    let matching_symbols = super::lookup_docs_symbols(
        &paths,
        "$wgProfileOnlyFlag",
        &super::DocsSymbolLookupOptions {
            kind: None,
            profile: Some("mw-1.44-site-authoring".to_string()),
            limit: 10,
        },
    )
    .expect("lookup matching profile symbol");
    assert!(!matching_symbols.is_empty());
    let other_symbols = super::lookup_docs_symbols(
        &paths,
        "$wgProfileOnlyFlag",
        &super::DocsSymbolLookupOptions {
            kind: None,
            profile: Some("mw-1.44-authoring".to_string()),
            limit: 10,
        },
    )
    .expect("lookup isolated profile symbol");
    assert!(other_symbols.is_empty());

    let other_context = super::build_docs_context(
        &paths,
        "ProfileOnlyNeedle",
        &super::DocsContextOptions {
            profile: Some("mw-1.44-authoring".to_string()),
            limit: 10,
            token_budget: 2_000,
        },
    )
    .expect("build isolated profile context");
    assert!(other_context.pages.is_empty());
    assert!(other_context.sections.is_empty());
    assert!(other_context.symbols.is_empty());
    assert!(other_context.examples.is_empty());
}
