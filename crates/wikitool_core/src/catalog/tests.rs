use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::tempdir;

use crate::authoring::{
    article_scout::build_article_scout,
    contract_traversal::{
        AuthoringContractPlanOptions, build_authoring_contract_plan_for_connection,
    },
    model::{ArticleScoutIntent, ContextSurfaceSource, LocalExistenceState},
};
use crate::catalog::authoring::{
    AuthoringContextOptions, AuthoringContextOutcome, AuthoringContractProfile,
    AuthoringPayloadMode, build_authoring_context,
};
use crate::catalog::content_index::{load_stored_index_stats, rebuild_index};
use crate::catalog::inspect::{
    BrokenLinkIssue, query_backlinks, query_empty_categories, query_orphans, run_validation_checks,
};
use crate::catalog::references::{
    ReferenceAuditFilters, ReferenceDuplicateKind, inspect_reference_duplicates,
    inspect_reference_list, inspect_reference_summary,
};
use crate::catalog::retrieval::{
    LocalChunkAcrossRetrieval, LocalChunkRetrieval, build_local_context, query_search_local,
    retrieve_local_context_chunks, retrieve_local_context_chunks_across_pages,
};
use crate::catalog::templates::{
    ActiveTemplateCatalogLookup, TemplateReferenceLookup, query_active_template_catalog,
    query_template_reference,
};
use crate::content_store::parsing::{
    extract_first_url, extract_media_records, extract_module_invocations,
    extract_reference_records, extract_template_invocations, extract_wikilinks,
    extract_wikilinks_for_namespace,
};
use crate::filesystem::{Namespace, ScanOptions};
use crate::runtime::{ResolvedPaths, ValueSource};
use crate::site::{
    AuthoringRules, CategoryRules, CitationRules, CitationTemplateRule, InfoboxPreference,
    LintRules, SiteAdapter, TemplateRules, sync_template_catalog_with_adapter,
};

fn write_file(path: &Path, content: &str) {
    let parent = path.parent().expect("parent");
    fs::create_dir_all(parent).expect("create parent");
    fs::write(path, content).expect("write file");
}

fn paths(project_root: &Path) -> ResolvedPaths {
    ResolvedPaths {
        wiki_content_dir: project_root.join("wiki_content"),
        templates_dir: project_root.join("templates"),
        state_dir: project_root.join(".wikitool"),
        data_dir: project_root.join(".wikitool").join("data"),
        db_path: project_root
            .join(".wikitool")
            .join("data")
            .join("wikitool.db"),
        config_path: project_root.join(".wikitool").join("config.toml"),
        parser_config_path: project_root
            .join(".wikitool")
            .join(crate::runtime::PARSER_CONFIG_FILENAME),
        project_root: project_root.to_path_buf(),
        root_source: ValueSource::Flag,
        data_source: ValueSource::Default,
        config_source: ValueSource::Default,
    }
}

fn test_site_adapter() -> SiteAdapter {
    SiteAdapter {
        schema_version: "resolved_site_adapter_v1".to_string(),
        adapter_id: "test-adapter".to_string(),
        docs_profile: "remilia-wiki".to_string(),
        source_documents: Vec::new(),
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
            source_review_rules: Vec::new(),
        },
        templates: TemplateRules {
            infobox_preferences: vec![
                InfoboxPreference {
                    subject_type: "concept".to_string(),
                    template_title: "Template:Infobox concept".to_string(),
                },
                InfoboxPreference {
                    subject_type: "organization".to_string(),
                    template_title: "Template:Infobox organization".to_string(),
                },
            ],
        },
        categories: CategoryRules {
            preferred_categories: vec!["Category:Ideas".to_string()],
        },
        lint: LintRules {
            forbid_curly_quotes: true,
            forbid_placeholder_fragments: Vec::new(),
            proper_nouns: Vec::new(),
            citation_needed: crate::site::LintRuleLevel::Ignore,
        },
        extension_contracts: Vec::new(),
        resolved_at: "1739000000".to_string(),
    }
}

#[test]
fn extract_wikilinks_parses_titles_and_category_membership() {
    let content = "[[Alpha|label]] [[Category:People]] [[:Category:People]] [[Module:Navbar/configuration]] [[Alpha#History]] [[https://example.com]]";
    let links = extract_wikilinks(content);

    assert_eq!(links.len(), 5);
    assert_eq!(links[0].target_title, "Alpha");
    assert!(!links[0].is_category_membership);
    assert_eq!(links[1].target_title, "Category:People");
    assert!(links[1].is_category_membership);
    assert_eq!(links[2].target_title, "Category:People");
    assert!(!links[2].is_category_membership);
    assert_eq!(links[3].target_title, "Module:Navbar/configuration");
    assert_eq!(links[4].target_title, "Alpha");
}

#[test]
fn extract_wikilinks_skips_parser_placeholder_targets() {
    let content = "<DPL>\nformat = ,* [[%PAGE%|%TITLE%]]\\n,,\n</DPL> [[Alpha]]";
    let links = extract_wikilinks(content);

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target_title, "Alpha");
}

#[test]
fn namespace_aware_wikilinks_skip_non_rendered_template_and_module_regions() {
    let template_content = "[[Visible]] <!-- [[Commented]] --> <noinclude>[[Doc only]]</noinclude> <templatedata>{\"params\":{\"[[Pseudo]]\":{}}}</templatedata>";
    let links = extract_wikilinks_for_namespace(template_content, Namespace::Template.as_str());

    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target_title, "Visible");

    let module_content = "local docs = --[[ [[Not a page]] ]] return {}";
    assert!(extract_wikilinks_for_namespace(module_content, Namespace::Module.as_str()).is_empty());
}

#[test]
fn rebuild_index_persists_scan_rows() {
    let temp = tempdir().expect("tempdir");
    let project_root: PathBuf = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Alpha article",
    );
    write_file(
        &paths.wiki_content_dir.join("Category").join("Foo.wiki"),
        "#REDIRECT [[Category:Bar]]",
    );
    write_file(
        &paths
            .templates_dir
            .join("navbox")
            .join("Module_Navbar")
            .join("configuration.lua"),
        "return {}",
    );

    let report = rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
    assert!(paths.db_path.exists());
    assert!(!report.unchanged);
    assert_eq!(report.inserted_rows, 3);
    assert_eq!(report.scan.total_files, 3);
    assert_eq!(report.scan.redirects, 1);

    let stored = load_stored_index_stats(&paths)
        .expect("load stats")
        .expect("stats must exist");
    assert_eq!(stored.indexed_rows, 3);
    assert_eq!(stored.redirects, 1);
    assert_eq!(
        stored.by_namespace,
        BTreeMap::from([
            (Namespace::Category.as_str().to_string(), 1usize),
            (Namespace::Main.as_str().to_string(), 1usize),
            (Namespace::Module.as_str().to_string(), 1usize),
        ])
    );
}

#[test]
fn rebuild_index_skips_when_corpus_unchanged_and_rebuilds_on_change() {
    let temp = tempdir().expect("tempdir");
    let project_root: PathBuf = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    let alpha_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(&alpha_path, "Alpha article [[Beta]]");
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "Beta article",
    );

    let first = rebuild_index(&paths, &ScanOptions::default()).expect("first rebuild");
    assert!(!first.unchanged);
    assert_eq!(first.inserted_rows, 2);

    let second = rebuild_index(&paths, &ScanOptions::default()).expect("second rebuild");
    assert!(second.unchanged);
    assert_eq!(second.inserted_rows, first.inserted_rows);
    assert_eq!(second.inserted_links, first.inserted_links);

    write_file(&alpha_path, "Alpha article revised [[Beta]] [[Gamma]]");
    let third = rebuild_index(&paths, &ScanOptions::default()).expect("third rebuild");
    assert!(!third.unchanged);
    assert_eq!(third.inserted_rows, 2);

    fs::remove_file(&alpha_path).expect("remove alpha");
    let fourth = rebuild_index(&paths, &ScanOptions::default()).expect("fourth rebuild");
    assert!(!fourth.unchanged);
    assert_eq!(fourth.inserted_rows, 1);
}

#[test]
fn query_backlinks_orphans_and_empty_categories() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "[[Beta]] [[Category:People]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "No links here",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "[[Beta]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Category").join("People.wiki"),
        "People category",
    );
    write_file(
        &paths.wiki_content_dir.join("Category").join("Empty.wiki"),
        "Empty category",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let backlinks = query_backlinks(&paths, "Beta")
        .expect("backlinks query")
        .expect("backlinks should exist");
    assert_eq!(backlinks, vec!["Alpha".to_string(), "Gamma".to_string()]);

    let orphans = query_orphans(&paths)
        .expect("orphans query")
        .expect("orphans should exist");
    assert_eq!(orphans, vec!["Alpha".to_string(), "Gamma".to_string()]);

    let empty_categories = query_empty_categories(&paths)
        .expect("empty category query")
        .expect("empty categories should exist");
    assert_eq!(empty_categories, vec!["Category:Empty".to_string()]);
}

#[test]
fn query_search_and_context_bundle() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Lead paragraph <ref name=\"alpha\">{{Cite web|title=Alpha Source|website=Remilia}}</ref>\n{{Infobox person|name=Alpha|birth_date={{Birth date|2000|1|1}}}}\n[[Image:Alpha.png|thumb|Alpha portrait]]\n== History ==\n[[Beta]] [[Module:Navbar]] [[Category:People]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "No links here",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "[[Beta]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Category").join("People.wiki"),
        "People category",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let search = query_search_local(&paths, "be", 20)
        .expect("search query")
        .expect("search should be available");
    assert_eq!(search.len(), 1);
    assert_eq!(search[0].title, "Beta");

    let context = build_local_context(&paths, "Alpha")
        .expect("context query")
        .expect("alpha context exists");
    assert_eq!(context.title, "Alpha");
    assert_eq!(context.namespace, "Main");
    assert_eq!(context.sections.len(), 1);
    assert_eq!(context.sections[0].heading, "History");
    assert_eq!(context.section_summaries.len(), 2);
    assert_eq!(context.section_summaries[0].section_heading, None);
    assert_eq!(
        context.section_summaries[1].section_heading.as_deref(),
        Some("History")
    );
    assert_eq!(context.categories, vec!["Category:People".to_string()]);
    assert!(
        context
            .templates
            .contains(&"Template:Infobox person".to_string())
    );
    assert!(
        context
            .templates
            .contains(&"Template:Birth date".to_string())
    );
    assert_eq!(context.modules, vec!["Module:Navbar".to_string()]);
    assert_eq!(context.backlinks.len(), 0);
    assert_eq!(context.references.len(), 1);
    assert_eq!(
        context.references[0].reference_name.as_deref(),
        Some("alpha")
    );
    assert!(
        context.references[0]
            .template_titles
            .contains(&"Template:Cite web".to_string())
    );
    assert_eq!(context.media.len(), 1);
    assert_eq!(context.media[0].file_title, "File:Alpha.png");
    assert_eq!(context.media[0].caption_text, "Alpha portrait");
    let infobox_invocation = context
        .template_invocations
        .iter()
        .find(|invocation| invocation.template_title == "Template:Infobox person")
        .expect("infobox invocation");
    assert_eq!(
        infobox_invocation.parameter_keys,
        vec!["birth_date".to_string(), "name".to_string()]
    );
    let birth_date_invocation = context
        .template_invocations
        .iter()
        .find(|invocation| invocation.template_title == "Template:Birth date")
        .expect("birth date invocation");
    assert_eq!(
        birth_date_invocation.parameter_keys,
        vec!["$1".to_string(), "$2".to_string(), "$3".to_string()]
    );

    let beta_context = build_local_context(&paths, "Beta")
        .expect("beta context query")
        .expect("beta context exists");
    assert_eq!(
        beta_context.backlinks,
        vec!["Alpha".to_string(), "Gamma".to_string()]
    );
}

#[test]
fn build_local_context_rejects_indexed_path_escape() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create root");
    let paths = paths(&project_root);
    fs::create_dir_all(&paths.wiki_content_dir).expect("create wiki_content");
    fs::create_dir_all(&paths.state_dir).expect("create state");

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Alpha body",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let connection =
        crate::schema::open_initialized_database_connection(&paths.db_path).expect("open db");
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("disable foreign keys");
    connection
        .execute(
            "UPDATE indexed_pages SET relative_path = ?1 WHERE title = 'Alpha'",
            ["../outside.txt"],
        )
        .expect("tamper indexed path");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");

    let error = build_local_context(&paths, "Alpha").expect_err("must reject escaped path");
    assert!(
        error
            .to_string()
            .contains("path escapes scoped runtime directories")
    );
}

#[test]
fn extract_template_invocations_captures_nested_templates() {
    let content =
        "{{Infobox person|name=Alpha|birth_date={{Birth date|2000|1|1}}}} {{#if:foo|bar|baz}}";
    let invocations = extract_template_invocations(content);

    let infobox = invocations
        .iter()
        .find(|invocation| invocation.template_title == "Template:Infobox person")
        .expect("infobox invocation");
    assert_eq!(
        infobox.parameter_keys,
        vec!["birth_date".to_string(), "name".to_string()]
    );

    let birth_date = invocations
        .iter()
        .find(|invocation| invocation.template_title == "Template:Birth date")
        .expect("birth date invocation");
    assert_eq!(
        birth_date.parameter_keys,
        vec!["$1".to_string(), "$2".to_string(), "$3".to_string()]
    );
    assert!(
        invocations
            .iter()
            .all(|invocation| !invocation.template_title.starts_with("Template:#"))
    );
}

#[test]
fn extract_module_invocations_captures_functions_and_parameter_keys() {
    let content = "{{#invoke:Infobox person|render|name=Alpha|occupation=Archivist|2020}} {{#invoke:Infobox person|render|name=Alpha|occupation=Archivist|2020}}";
    let invocations = extract_module_invocations(content);

    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].module_title, "Module:Infobox person");
    assert_eq!(invocations[0].function_name, "render");
    assert_eq!(
        invocations[0].parameter_keys,
        vec![
            "$1".to_string(),
            "name".to_string(),
            "occupation".to_string()
        ]
    );
    assert!(
        invocations[0]
            .raw_wikitext
            .contains("#invoke:Infobox person")
    );
}

#[test]
fn extract_reference_records_parses_named_refs_and_template_summaries() {
    let content = "Lead <ref name=\"alpha\">{{Cite web|title=Alpha Source|url=https://example.org/alpha|website=Example|author=Jane Example|date=2025-01-01|doi=10.1234/Alpha-01}}</ref> tail <ref group=\"note\" name=\"reuse\" />";
    let references = extract_reference_records(content);

    assert_eq!(references.len(), 2);
    assert_eq!(references[0].reference_name.as_deref(), Some("alpha"));
    assert_eq!(
        references[0].template_titles,
        vec!["Template:Cite web".to_string()]
    );
    assert_eq!(references[0].citation_family, "Template:Cite web");
    assert_eq!(
        references[0].primary_template_title.as_deref(),
        Some("Template:Cite web")
    );
    assert_eq!(references[0].source_type, "web");
    assert_eq!(references[0].source_origin, "external");
    assert_eq!(references[0].source_family, "web");
    assert_eq!(references[0].authority_kind, "domain");
    assert_eq!(references[0].source_authority, "example.org");
    assert_eq!(references[0].reference_title, "Alpha Source");
    assert_eq!(references[0].source_container, "Example");
    assert_eq!(references[0].source_author, "Jane Example");
    assert_eq!(references[0].source_domain, "example.org");
    assert_eq!(references[0].source_date, "2025-01-01");
    assert_eq!(references[0].canonical_url, "https://example.org/alpha");
    assert!(
        references[0]
            .identifier_entries
            .iter()
            .any(|entry| entry == "doi:10.1234/alpha-01")
    );
    assert!(
        references[0]
            .source_urls
            .iter()
            .any(|url| url == "https://example.org/alpha")
    );
    assert!(references[0].citation_profile.contains("web"));
    assert!(references[0].citation_profile.contains("example.org"));
    assert!(
        !references[0]
            .retrieval_signals
            .iter()
            .any(|flag| flag == "first-party")
    );
    assert!(references[0].summary_text.contains("Alpha Source"));
    assert_eq!(references[1].reference_name.as_deref(), Some("reuse"));
    assert_eq!(references[1].reference_group.as_deref(), Some("note"));
    assert_eq!(references[1].summary_text, "reuse");
}

#[test]
fn section_authoring_bias_prefers_content_sections_over_reference_tail() {
    let history_score = crate::catalog::retrieval::section_authoring_bias(
        Some("History"),
        "Alpha biography summary with useful prose.",
    );
    let references_score = crate::catalog::retrieval::section_authoring_bias(
        Some("References"),
        "{{Reflist}}\n[[Category:Test]]",
    );

    assert!(history_score > references_score);
    assert!(references_score < 0);
}

#[test]
fn extract_media_records_parses_inline_and_gallery_entries() {
    let content = "[[Image:Alpha.png|thumb|Alpha portrait]]\n<gallery mode=\"packed\">\nFile:Beta.jpg|Beta gallery caption\n</gallery>";
    let media = extract_media_records(content);

    assert_eq!(media.len(), 2);
    assert_eq!(media[0].file_title, "File:Alpha.png");
    assert_eq!(media[0].media_kind, "inline");
    assert_eq!(media[0].caption_text, "Alpha portrait");
    assert_eq!(media[1].file_title, "File:Beta.jpg");
    assert_eq!(media[1].media_kind, "gallery");
    assert!(media[1].caption_text.contains("Beta gallery caption"));
    assert!(
        media[1]
            .options
            .iter()
            .any(|option| option == "mode=packed")
    );
}

#[test]
fn retrieve_local_context_chunks_returns_index_missing_when_not_built() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    let retrieval = retrieve_local_context_chunks(&paths, "Alpha", None, 4, 200)
        .expect("retrieve chunks without index");
    assert_eq!(retrieval, LocalChunkRetrieval::IndexMissing);
}

#[test]
fn retrieve_local_context_chunks_supports_query_and_budget() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Lead paragraph with CinderSignal marker and extra tokens for chunking.\n== History ==\nThis section carries CinderSignal data for retrieval testing and deterministic filtering.",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let retrieval = retrieve_local_context_chunks(&paths, "Alpha", Some("CinderSignal"), 3, 80)
        .expect("retrieve chunks with query");
    let report = match retrieval {
        LocalChunkRetrieval::Found(report) => report,
        other => panic!("expected found report, got {other:?}"),
    };
    assert_eq!(report.title, "Alpha");
    assert_eq!(report.query.as_deref(), Some("CinderSignal"));
    assert!(report.retrieval_mode == "fts" || report.retrieval_mode == "like");
    assert!(!report.chunks.is_empty());
    assert!(report.token_estimate_total <= 80);
    assert!(
        report
            .chunks
            .iter()
            .all(|chunk| chunk.chunk_text.contains("CinderSignal"))
    );
}

#[test]
fn retrieve_local_context_chunks_across_pages_requires_query() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);
    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Alpha chunk body",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let retrieval = retrieve_local_context_chunks_across_pages(&paths, " ", 4, 200, 2, true)
        .expect("across-pages retrieval");
    assert_eq!(retrieval, LocalChunkAcrossRetrieval::QueryMissing);
}

#[test]
fn retrieve_local_context_chunks_across_pages_returns_multi_source_chunks() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Lead AlphaSignal signal chunk one.\n== A ==\nAlphaSignal chunk two with overlap.",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "Lead AlphaSignal beta chunk one.\n== B ==\nAlphaSignal beta chunk two with overlap.",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "Lead AlphaSignal gamma chunk one.\n== C ==\nAlphaSignal gamma chunk two with overlap.",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let retrieval =
        retrieve_local_context_chunks_across_pages(&paths, "AlphaSignal", 4, 140, 2, true)
            .expect("across-pages retrieval");
    let report = match retrieval {
        LocalChunkAcrossRetrieval::Found(report) => report,
        other => panic!("expected found report, got {other:?}"),
    };
    assert!(report.retrieval_mode.contains("across"));
    assert!(report.source_page_count <= 2);
    assert!(report.token_estimate_total <= 140);
    assert!(!report.chunks.is_empty());
    let unique_sources = report
        .chunks
        .iter()
        .map(|chunk| chunk.source_relative_path.as_str())
        .collect::<BTreeSet<_>>();
    assert!(unique_sources.len() <= 2);
    assert!(
        report
            .chunks
            .iter()
            .all(|chunk| chunk.chunk_text.contains("AlphaSignal"))
    );
}

#[test]
fn retrieve_local_context_chunks_across_pages_uses_hybrid_term_expansion() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Alpha_Beacon.wiki"),
        "Lead alpha marker.\n== History ==\nThe beacon emits a steady signal for retrieval tests.",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Noise.wiki"),
        "Alpha beacon phrase never appears here, only unrelated noise.",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let retrieval =
        retrieve_local_context_chunks_across_pages(&paths, "Alpha Beacon", 3, 180, 2, true)
            .expect("across-pages retrieval");
    let report = match retrieval {
        LocalChunkAcrossRetrieval::Found(report) => report,
        other => panic!("expected found report, got {other:?}"),
    };
    assert!(report.retrieval_mode.contains("hybrid"));
    assert!(
        report
            .chunks
            .iter()
            .any(|chunk| chunk.source_title == "Alpha Beacon")
    );
}

#[test]
fn retrieve_local_context_chunks_across_pages_uses_semantic_page_profiles() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Alpha_Beacon.wiki"),
        "Lead prose without the page title words.\n== History ==\nThe hidden signal stays buried in ordinary text.",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Noise.wiki"),
        "Noise page with unrelated prose only.",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let retrieval =
        retrieve_local_context_chunks_across_pages(&paths, "Alpha Beacon", 3, 180, 2, true)
            .expect("across-pages retrieval");
    let report = match retrieval {
        LocalChunkAcrossRetrieval::Found(report) => report,
        other => panic!("expected found report, got {other:?}"),
    };
    assert!(report.retrieval_mode.contains("term-profile"));
    assert!(report.retrieval_mode.contains("seed-pages"));
    assert!(
        report
            .chunks
            .iter()
            .any(|chunk| chunk.source_title == "Alpha Beacon")
    );
}

#[test]
fn retrieve_local_context_chunks_across_pages_uses_reference_authority_and_identifier_hits() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Lead prose without publisher words.\n<ref>{{Cite book|title=Alpha Source|publisher=Remilia Press|isbn=978-1-4028-9462-6}}</ref>",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Noise.wiki"),
        "Noise page with unrelated prose only.",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let authority_retrieval =
        retrieve_local_context_chunks_across_pages(&paths, "Remilia Press", 3, 180, 2, true)
            .expect("authority retrieval");
    let authority_report = match authority_retrieval {
        LocalChunkAcrossRetrieval::Found(report) => report,
        other => panic!("expected found report, got {other:?}"),
    };
    assert!(authority_report.retrieval_mode.contains("authority"));
    assert!(authority_report.retrieval_mode.contains("seed-pages"));
    assert!(
        authority_report
            .chunks
            .iter()
            .any(|chunk| chunk.source_title == "Alpha")
    );

    let identifier_retrieval =
        retrieve_local_context_chunks_across_pages(&paths, "9781402894626", 3, 180, 2, true)
            .expect("identifier retrieval");
    let identifier_report = match identifier_retrieval {
        LocalChunkAcrossRetrieval::Found(report) => report,
        other => panic!("expected found report, got {other:?}"),
    };
    assert!(identifier_report.retrieval_mode.contains("identifier"));
    assert!(identifier_report.retrieval_mode.contains("seed-pages"));
    assert!(
        identifier_report
            .chunks
            .iter()
            .any(|chunk| chunk.source_title == "Alpha")
    );
}

#[test]
fn build_authoring_context_requires_topic_or_stub_signal() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);
    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Alpha body text",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = build_authoring_context(&paths, None, None, &AuthoringContextOptions::default())
        .expect("authoring pack");
    assert_eq!(report, AuthoringContextOutcome::QueryMissing);
}

#[test]
fn build_authoring_context_collects_templates_links_and_chunks() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "{{Infobox person|name=Alpha|born=2020}}\n'''Alpha''' works with [[Beta]] and [[Gamma]].<ref>{{Cite web|title=Alpha Source|website=Remilia}}</ref>\n[[Image:Alpha.png|thumb|Alpha portrait]]\n[[Category:People]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "{{Infobox organization|name=Beta Org|founder=Alpha}}\n'''Beta''' references [[Alpha]] and [[Gamma]].<ref>{{Cite book|title=Beta Book|publisher=Remilia Press}}</ref>\n[[File:Beta.jpg|thumb|Beta portrait]]\n[[Category:Organizations]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "{{Navbox|name=Gamma nav|list1=[[Alpha]]}}\n'''Gamma''' is linked with [[Alpha]].<ref name=\"gamma-source\" />\n[[Category:People]]",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let options = AuthoringContextOptions {
        related_page_limit: 6,
        chunk_limit: 6,
        token_budget: 420,
        max_pages: 4,
        link_limit: 8,
        category_limit: 4,
        template_limit: 6,
        docs_profile: crate::catalog::status::DEFAULT_DOCS_PROFILE.to_string(),
        diversify: true,
        payload_mode: AuthoringPayloadMode::Compact,
        contract_profile: AuthoringContractProfile::Author,
        contract_query: None,
    };
    let report = build_authoring_context(
        &paths,
        Some("Alpha"),
        Some("{{Infobox person|name=Draft}}\nDraft body with [[Alpha]] and [[Missing Page]].\n<DPL>\nformat = ,* [[%PAGE%|%TITLE%]]\\n,,\n</DPL>"),
        &options,
    )
    .expect("authoring pack");

    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };
    assert_eq!(report.topic, "Alpha");
    assert_eq!(report.query, "Alpha");
    assert!(report.query_terms.contains(&"Alpha".to_string()));
    assert!(report.topic_assessment.title_exists_locally);
    assert!(!report.topic_assessment.should_create_new_article);
    assert_eq!(report.pack_token_budget, 420);
    assert!(report.pack_token_estimate_total >= report.token_estimate_total);
    assert_eq!(report.payload_mode, AuthoringPayloadMode::Compact);
    assert_eq!(
        report.context_summary.subject_context.related_page_count,
        report.related_pages.len()
    );
    assert!(
        report
            .context_summary
            .wiki_contract_context
            .template_contracts
            .iter()
            .any(|entry| entry.template_title == "Template:Infobox person")
    );
    assert!(
        !report
            .context_summary
            .wiki_contract_context
            .omitted_detail
            .is_empty()
    );
    assert!(
        report
            .template_references
            .iter()
            .all(|reference| reference.implementation_chunks.is_empty())
    );
    assert!(!report.related_pages.is_empty());
    assert!(
        report
            .suggested_links
            .iter()
            .any(|entry| entry.title == "Alpha")
    );
    assert!(
        report
            .suggested_templates
            .iter()
            .any(|entry| entry.template_title == "Template:Infobox person")
    );
    assert!(
        report
            .suggested_templates
            .iter()
            .any(|entry| !entry.example_invocations.is_empty())
    );
    assert!(report.suggested_references.iter().any(|entry| {
        entry.citation_family == "Template:Cite web"
            && entry.source_type == "web"
            && entry
                .common_retrieval_signals
                .iter()
                .any(|signal| signal == "citation-template")
    }));
    assert!(!report.template_baseline.is_empty());
    assert!(report.stub_existing_links.contains(&"Alpha".to_string()));
    assert!(!report.query_terms.contains(&"%PAGE%".to_string()));
    assert!(!report.stub_missing_links.contains(&"%PAGE%".to_string()));
    assert!(
        report
            .stub_missing_links
            .contains(&"Missing Page".to_string())
    );
    assert!(
        report
            .stub_detected_templates
            .iter()
            .any(|entry| entry.template_title == "Template:Infobox person")
    );
    assert!(report.retrieval_mode.contains("hybrid"));
    assert!(report.retrieval_mode.contains("across"));
    assert!(report.token_estimate_total <= 420);
}

#[test]
fn build_authoring_context_reports_article_gap_signals() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Hyperrealtime.wiki"),
        "'''Hyperrealtime''' mentions [[Miladychan]] in the context of chen2 revival.",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Tim_Clancy.wiki"),
        "'''Tim Clancy''' created [[Miladychan]] for distributed chat experiments.",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = build_authoring_context(
        &paths,
        Some("Miladychan"),
        None,
        &AuthoringContextOptions::default(),
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };

    assert!(!report.topic_assessment.title_exists_locally);
    assert!(report.topic_assessment.should_create_new_article);
    assert!(report.topic_assessment.exact_page.is_none());
    assert_eq!(report.topic_assessment.local_title_hit_count, 0);
    assert_eq!(report.topic_assessment.backlink_count, 2);
    assert_eq!(
        report.topic_assessment.backlinks,
        vec!["Hyperrealtime".to_string(), "Tim Clancy".to_string()]
    );
    assert!(report.docs_context.is_none());
    assert!(!report.retrieval_mode.contains("docs-bridge"));
}

#[test]
fn build_authoring_context_uses_template_matches_for_related_pages() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "{{Infobox person|name=Alpha|occupation=Archivist}}\n'''Alpha''' chronicle text with SaffronSignal authoring detail.",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "{{Infobox location|name=Beta}}\n'''Beta''' location text with unrelated detail.",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let options = AuthoringContextOptions {
        related_page_limit: 4,
        chunk_limit: 4,
        token_budget: 240,
        max_pages: 2,
        link_limit: 4,
        category_limit: 4,
        template_limit: 4,
        docs_profile: crate::catalog::status::DEFAULT_DOCS_PROFILE.to_string(),
        diversify: true,
        payload_mode: AuthoringPayloadMode::Compact,
        contract_profile: AuthoringContractProfile::Author,
        contract_query: None,
    };
    let report = build_authoring_context(
        &paths,
        Some("Unmatched Draft Topic"),
        Some(
            "{{Infobox person|name=Draft|occupation=Archivist}}\nDraft body without direct links.",
        ),
        &options,
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };

    assert!(
        report
            .related_pages
            .iter()
            .any(|entry| entry.title == "Alpha" && entry.source.contains("template-match"))
    );
    assert!(
        report
            .chunks
            .iter()
            .any(|chunk| chunk.source_title == "Alpha")
    );
    assert!(report.retrieval_mode.contains("seed-pages"));
}

#[test]
fn build_authoring_context_filters_redirect_category_and_fragment_noise() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "'''Alpha''' is the coherent NoiseSignal article with grounded history and context.",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("NoiseSignal.wiki"),
        "#REDIRECT [[Alpha]]",
    );
    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("NoiseSignal___ko.wiki"),
        "'''NoiseSignal''' 한국어 번역 문서.",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "NoiseSignal 2025}}</ref>",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "{{SHORTDESC:NoiseSignal metadata}}\n{{Infobox organization|name=NoiseSignal}}",
    );
    write_file(
        &paths
            .wiki_content_dir
            .join("Category")
            .join("NoiseSignal.wiki"),
        "Category landing text for NoiseSignal.",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = build_authoring_context(
        &paths,
        Some("NoiseSignal"),
        None,
        &AuthoringContextOptions {
            related_page_limit: 6,
            chunk_limit: 4,
            token_budget: 240,
            max_pages: 3,
            link_limit: 4,
            category_limit: 4,
            template_limit: 4,
            docs_profile: crate::catalog::status::DEFAULT_DOCS_PROFILE.to_string(),
            diversify: true,
            payload_mode: AuthoringPayloadMode::Compact,
            contract_profile: AuthoringContractProfile::Author,
            contract_query: None,
        },
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };

    assert!(report.retrieval_mode.contains("authoring-curated"));
    assert!(
        report
            .related_pages
            .iter()
            .all(|entry| entry.namespace == Namespace::Main.as_str() && !entry.is_redirect)
    );
    assert!(report.related_pages.iter().all(|entry| {
        entry.title != "Category:NoiseSignal"
            && entry.title != "NoiseSignal"
            && entry.title != "NoiseSignal/ko"
    }));
    assert!(report.chunks.iter().all(|chunk| {
        chunk.source_namespace == Namespace::Main.as_str()
            && !chunk.chunk_text.contains("2025}}</ref>")
            && !chunk.chunk_text.starts_with("{{")
    }));
    assert!(
        report
            .chunks
            .iter()
            .all(|chunk| chunk.source_title != "Gamma")
    );
    assert!(
        report
            .chunks
            .iter()
            .any(|chunk| chunk.source_title == "Alpha")
    );
}

#[test]
fn template_catalog_and_reference_include_examples_and_implementation_context() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "{{Infobox person|name=Alpha|born=2020}}\n'''Alpha''' page.",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "{{Infobox person|name=Beta|occupation=Archivist}}\n'''Beta''' page.",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Infobox_person.wiki"),
        "Template lead text.\n{{#invoke:Infobox person|render}}\n== Parameters ==\nUse |name= and |occupation=.",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Module_Infobox_person.wiki"),
        "return { render = function() end }",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Infobox_person___doc.wiki"),
        "Documentation lead.\n== Usage ==\nUse the template on biographies.",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("_redirects")
            .join("Template_Infobox_human.wiki"),
        "#REDIRECT [[Template:Infobox person]]",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let catalog = query_active_template_catalog(&paths, 10).expect("catalog query");
    let catalog = match catalog {
        ActiveTemplateCatalogLookup::Found(catalog) => catalog,
        other => panic!("expected template catalog, got {other:?}"),
    };
    assert!(catalog.active_template_count >= 1);
    let infobox = catalog
        .templates
        .iter()
        .find(|template| template.template_title == "Template:Infobox person")
        .expect("infobox person in catalog");
    assert!(
        infobox
            .aliases
            .contains(&"Template:Infobox human".to_string())
    );
    assert!(infobox.parameter_stats.iter().any(|stat| {
        stat.key == "name"
            && stat.usage_count >= 2
            && stat.example_values.iter().any(|value| value == "Alpha")
    }));
    assert!(!infobox.example_invocations.is_empty());
    assert!(
        infobox
            .implementation_titles
            .iter()
            .any(|title| title == "Module:Infobox person")
    );

    let reference =
        query_template_reference(&paths, "Infobox person").expect("template reference query");
    let reference = match reference {
        TemplateReferenceLookup::Found(reference) => *reference,
        other => panic!("expected template reference, got {other:?}"),
    };
    assert_eq!(reference.template.template_title, "Template:Infobox person");
    assert!(
        reference
            .implementation_pages
            .iter()
            .any(|page| page.role == "module" && page.page_title == "Module:Infobox person")
    );
    assert!(
        reference
            .implementation_pages
            .iter()
            .any(|page| page.role == "documentation")
    );
    assert!(
        reference
            .implementation_sections
            .iter()
            .any(|section| section.section_heading.as_deref() == Some("Parameters"))
    );
    assert!(!reference.implementation_chunks.is_empty());
}

#[test]
fn template_reference_preserves_dynamic_parameter_invocation_examples() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);
    let invocation = concat!(
        "{{Version label|name=",
        "{{{min_name|}}}",
        "|build=",
        "{{{min_build|}}}",
        "}}",
    );
    let range_source = concat!(
        "{{#if:has minimum|",
        "{{Version label|name=",
        "{{{min_name|}}}",
        "|build=",
        "{{{min_build|}}}",
        "}}",
        "}}",
    );

    write_file(
        &paths
            .templates_dir
            .join("misc")
            .join("Template_Version_label.wiki"),
        "Formats one software version label.",
    );
    write_file(
        &paths
            .templates_dir
            .join("misc")
            .join("Template_Version_range.wiki"),
        range_source,
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let reference =
        query_template_reference(&paths, "Version label").expect("template reference query");
    let reference = match reference {
        TemplateReferenceLookup::Found(reference) => *reference,
        other => panic!("expected template reference, got {other:?}"),
    };
    assert_eq!(reference.template.usage_count, 1);
    assert_eq!(reference.template.example_invocations.len(), 1);
    assert_eq!(
        reference.template.example_invocations[0].invocation_text,
        invocation
    );
    assert_eq!(
        reference.template.example_invocations[0].parameter_keys,
        vec!["build", "name"]
    );
    assert!(
        reference
            .template
            .parameter_stats
            .iter()
            .filter(|parameter| matches!(parameter.key.as_str(), "build" | "name"))
            .all(|parameter| parameter.example_values.is_empty())
    );

    let catalog =
        sync_template_catalog_with_adapter(&paths, &test_site_adapter()).expect("template catalog");
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.template_title == "Template:Version label")
        .expect("version label catalog entry");
    let indexed_example = entry
        .examples
        .iter()
        .find(|example| example.source_kind == "indexed_usage")
        .expect("indexed version label example");
    assert_eq!(indexed_example.invocation_text, invocation);
    assert!(
        entry
            .parameters
            .iter()
            .filter(|parameter| matches!(parameter.name.as_str(), "build" | "name"))
            .all(|parameter| parameter.example_values.is_empty())
    );
}

#[test]
fn authoring_contract_plan_uses_indexed_template_module_graph() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "{{Infobox person|name=Alpha|occupation=Archivist}}\n'''Alpha''' page.",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Infobox_person.wiki"),
        "Template lead text.\n{{#invoke:Infobox person|render}}\n== Parameters ==\nUse |name= and |occupation=.",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Module_Infobox_person.wiki"),
        "return { render = function(frame) return frame.args.name end }",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
    sync_template_catalog_with_adapter(&paths, &test_site_adapter()).expect("sync catalog");

    let connection =
        crate::schema::open_initialized_database_connection(&paths.db_path).expect("open db");
    let plan = build_authoring_contract_plan_for_connection(
        &connection,
        "test-adapter",
        &AuthoringContractPlanOptions {
            query: "person infobox".to_string(),
            query_terms: vec!["person".to_string(), "infobox".to_string()],
            limit: 8,
            token_budget: 500,
            profile: AuthoringContractProfile::Author,
            ..AuthoringContractPlanOptions::default()
        },
        None,
    )
    .expect("contract plan");

    assert!(
        plan.selected_contracts
            .iter()
            .any(|contract| contract.title == "Template:Infobox person"
                && contract.contract_kind == "template"
                && contract.parameter_keys.iter().any(|key| key == "name"))
    );
    assert!(
        plan.contract_edges
            .iter()
            .any(|edge| edge.from_title == "Template:Infobox person"
                && edge.to_title == "Module:Infobox person"
                && edge.relation == "implemented_by")
    );

    let partial_plan = build_authoring_contract_plan_for_connection(
        &connection,
        "test-adapter",
        &AuthoringContractPlanOptions {
            query: "animal infobox".to_string(),
            query_terms: vec!["animal".to_string(), "infobox".to_string()],
            limit: 4,
            token_budget: 300,
            profile: AuthoringContractProfile::Author,
            ..AuthoringContractPlanOptions::default()
        },
        None,
    )
    .expect("partial contract plan");
    assert!(
        partial_plan
            .matched_query_terms
            .contains(&"infobox".to_string())
    );
    assert_eq!(partial_plan.missing_query_terms, vec!["animal".to_string()]);
    assert!(partial_plan.warnings.iter().any(|warning| {
        warning.contains("No selected contract matched these query terms: animal")
    }));
}

#[test]
fn authoring_contract_index_preserves_case_distinct_template_titles() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths
            .templates_dir
            .join("misc")
            .join("Template_Block_Indent.wiki"),
        "Uppercase internal title.",
    );
    write_file(
        &paths
            .templates_dir
            .join("list")
            .join("Template_Block_indent.wiki"),
        "Lowercase internal title.",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
    sync_template_catalog_with_adapter(&paths, &test_site_adapter()).expect("sync catalog");

    let connection =
        crate::schema::open_initialized_database_connection(&paths.db_path).expect("open db");
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM authoring_contracts
             WHERE site_adapter_id = ?1
               AND contract_kind = 'template'
               AND title IN ('Template:Block Indent', 'Template:Block indent')",
            ["test-adapter"],
            |row| row.get(0),
        )
        .expect("count case-distinct contract titles");

    assert_eq!(count, 2);
}

#[test]
fn build_authoring_context_uses_explicit_contract_query_for_graph_traversal() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "{{Infobox species|name=Alpha|conservation_status=Least concern}}\n'''Alpha''' page.",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Infobox_species.wiki"),
        "Species infobox template.\n{{#invoke:Infobox species|render}}\n== Parameters ==\nUse |name=, |taxon=, and |conservation_status=.",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Module_Infobox_species.wiki"),
        "return { render = function(frame) return frame.args.conservation_status end }",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = build_authoring_context(
        &paths,
        Some("Cheetah"),
        None,
        &AuthoringContextOptions {
            related_page_limit: 4,
            chunk_limit: 4,
            token_budget: 280,
            max_pages: 2,
            link_limit: 4,
            category_limit: 4,
            template_limit: 4,
            docs_profile: crate::catalog::status::DEFAULT_DOCS_PROFILE.to_string(),
            diversify: true,
            payload_mode: AuthoringPayloadMode::Compact,
            contract_profile: AuthoringContractProfile::Author,
            contract_query: Some("species infobox conservation".to_string()),
        },
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };
    let traversal = &report.context_summary.wiki_contract_context.traversal_plan;

    assert_eq!(report.query, "Cheetah");
    assert_eq!(traversal.query, "species infobox conservation");
    let species = traversal
        .selected_contracts
        .iter()
        .find(|contract| contract.title == "Template:Infobox species")
        .expect("species infobox contract selected");
    assert!(species.parameter_keys.iter().any(|key| key == "name"));
    assert!(
        species
            .parameter_keys
            .iter()
            .any(|key| key == "conservation_status")
    );
    assert!(
        species
            .selection_reasons
            .iter()
            .any(|reason| reason.signal == "title_match" || reason.signal == "parameter_match")
    );
}

#[test]
fn build_authoring_context_bridges_templates_modules_and_docs() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "{{Infobox person|name=Alpha|occupation=Archivist}}\n'''Alpha''' article body with [[Beta]].",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "'''Beta''' article body linked from [[Alpha]].",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Infobox_person.wiki"),
        "Template lead text.\n{{#invoke:Infobox person|render|name=Example|occupation=Archivist}}\n== Parameters ==\nUse |name= and |occupation=.",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Module_Infobox_person.wiki"),
        "return { render = function(frame) return frame.args.name end }",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Infobox_person___doc.wiki"),
        "Documentation lead.\n== Usage ==\nUse {{Infobox person}} for biographies.",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let bundle_path = project_root.join("authoring_docs_bundle.json");
    write_file(
        &bundle_path,
        r#"{
  "schema_version": 1,
  "generated_at_unix": 1739000000,
  "source": "authoring_bridge_test",
  "technical": [
    {
      "doc_type": "manual",
      "pages": [
        {
          "page_title": "Manual:Scribunto",
          "local_path": "manual/Scribunto.md",
          "content": "Scribunto supports {{#invoke:Infobox person|render|name=Alpha}} for Lua-backed templates. Template parameters should map cleanly to module functions."
        }
      ]
    }
  ]
}"#,
    );
    crate::docs::import_docs_bundle(&paths, &bundle_path).expect("import docs bundle");
    let connection =
        crate::schema::open_initialized_database_connection(&paths.db_path).expect("open db");
    connection
        .execute(
            "UPDATE docs_corpora SET source_profile = 'mw-1.44-authoring'",
            [],
        )
        .expect("set docs profile");

    let report = build_authoring_context(
        &paths,
        Some("Alpha"),
        Some("{{Infobox person|name=Draft|occupation=Archivist}}\nDraft prose."),
        &AuthoringContextOptions {
            related_page_limit: 6,
            chunk_limit: 6,
            token_budget: 420,
            max_pages: 4,
            link_limit: 8,
            category_limit: 4,
            template_limit: 6,
            docs_profile: crate::catalog::status::DEFAULT_DOCS_PROFILE.to_string(),
            diversify: true,
            payload_mode: AuthoringPayloadMode::Full,
            contract_profile: AuthoringContractProfile::Implementation,
            contract_query: None,
        },
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };

    assert!(
        report
            .template_references
            .iter()
            .any(|reference| reference.template.template_title == "Template:Infobox person")
    );
    assert!(
        report
            .module_patterns
            .iter()
            .any(|module| module.module_title == "Module:Infobox person")
    );
    let docs_context = report.docs_context.expect("docs context must exist");
    assert!(
        docs_context
            .queries
            .iter()
            .any(|query| query == "Scribunto #invoke")
    );
    assert!(
        docs_context
            .pages
            .iter()
            .any(|page| page.page_title == "Manual:Scribunto")
    );
    assert!(report.retrieval_mode.contains("template-guides"));
    assert!(report.retrieval_mode.contains("module-patterns"));
    assert!(report.retrieval_mode.contains("docs-bridge"));
}

#[test]
fn inspect_reference_summary_and_list_support_selection_and_filters() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Alpha<ref>{{Cite web|title=Alpha Source|url=https://remilia.org/a|website=Remilia|doi=10.1234/alpha}}</ref>\n\
         Alpha reuse<ref>{{Cite web|title=Shared ID|url=https://mirror.example/shared|website=Mirror|doi=10.5555/shared}}</ref>",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "Beta<ref>{{Cite web|title=Beta Copy|url=https://remilia.org/a|website=Remilia}}</ref>\n\
         Beta reuse<ref>{{Cite web|title=Identifier Copy|url=https://elsewhere.example/other|website=Elsewhere|doi=10.5555/shared}}</ref>",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "Gamma<ref>{{Cite web|title=Gamma Similar|url=https://remilia.org/another|website=Remilia}}</ref>",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let summary = inspect_reference_summary(&paths, &[], &ReferenceAuditFilters::default())
        .expect("reference summary")
        .expect("summary report");
    assert_eq!(summary.reference_count, 5);
    assert_eq!(summary.distinct_page_count, 3);
    assert!(
        summary
            .top_domains
            .iter()
            .any(|domain| domain == "remilia.org")
    );

    let selected = inspect_reference_list(
        &paths,
        &["Alpha".to_string()],
        &ReferenceAuditFilters::default(),
    )
    .expect("selected list")
    .expect("selected report");
    assert_eq!(selected.reference_count, 2);
    assert!(
        selected
            .items
            .iter()
            .all(|item| item.source_title == "Alpha")
    );

    let filtered = inspect_reference_list(
        &paths,
        &[],
        &ReferenceAuditFilters {
            domain: Some("remilia.org".to_string()),
            ..ReferenceAuditFilters::default()
        },
    )
    .expect("domain filtered list")
    .expect("filtered report");
    assert_eq!(filtered.reference_count, 3);
    assert!(
        filtered
            .items
            .iter()
            .all(|item| item.source_domain == "remilia.org")
    );

    let identifier_filtered = inspect_reference_list(
        &paths,
        &[],
        &ReferenceAuditFilters {
            identifier_key: Some("doi".to_string()),
            identifier: Some("10.5555/shared".to_string()),
            ..ReferenceAuditFilters::default()
        },
    )
    .expect("identifier filtered list")
    .expect("identifier report");
    assert_eq!(identifier_filtered.reference_count, 2);
    assert!(identifier_filtered.items.iter().all(|item| {
        item.identifier_entries
            .iter()
            .any(|entry| entry == "doi:10.5555/shared")
    }));
}

#[test]
fn inspect_reference_duplicates_uses_strong_keys_only() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "Alpha<ref>{{Cite web|title=Alpha Source|url=https://remilia.org/a|website=Remilia|doi=10.1234/alpha}}</ref>\n\
         Alpha reuse<ref>{{Cite web|title=Shared ID|url=https://mirror.example/shared|website=Mirror|doi=10.5555/shared}}</ref>",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "Beta<ref>{{Cite web|title=Beta Copy|url=https://remilia.org/a|website=Remilia}}</ref>\n\
         Beta reuse<ref>{{Cite web|title=Identifier Copy|url=https://elsewhere.example/other|website=Elsewhere|doi=10.5555/shared}}</ref>",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Delta.wiki"),
        "Delta<ref>{{Cite web|title=Alpha Source|url=https://remilia.org/a|website=Remilia|doi=10.1234/alpha}}</ref>",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "Gamma<ref>{{Cite web|title=Gamma Similar|url=https://remilia.org/another|website=Remilia}}</ref>",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let duplicates = inspect_reference_duplicates(&paths, &[], &ReferenceAuditFilters::default())
        .expect("duplicate report")
        .expect("duplicates");
    assert!(
        duplicates
            .groups
            .iter()
            .any(|group| group.kind == ReferenceDuplicateKind::CanonicalUrl
                && group.match_key == "https://remilia.org/a"
                && group.reference_count >= 3)
    );
    assert!(duplicates.groups.iter().any(|group| group.kind
        == ReferenceDuplicateKind::NormalizedIdentifier
        && group.match_key == "doi:10.5555/shared"
        && group.reference_count == 2));
    assert!(duplicates.groups.iter().any(|group| group.kind
        == ReferenceDuplicateKind::ExactReferenceWikitext
        && group.reference_count >= 2));
    assert!(
        duplicates
            .groups
            .iter()
            .all(|group| { group.items.iter().all(|item| item.source_title != "Gamma") })
    );
}

#[test]
fn build_local_context_prefers_prose_over_leading_metadata() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "{{SHORTDESC:Metadata only}}\n{{Infobox person|name=Alpha}}\n'''Alpha''' is the first prose sentence with grounded context.\n== History ==\nAlpha history sentence.",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let context = build_local_context(&paths, "Alpha")
        .expect("context query")
        .expect("alpha context exists");
    assert!(context.content_preview.starts_with("'''Alpha'''"));
    assert!(!context.context_chunks.is_empty());
    assert!(
        context
            .context_chunks
            .iter()
            .all(|chunk| !chunk.chunk_text.starts_with("{{SHORTDESC"))
    );
}

#[test]
fn build_article_scout_uses_neutral_surfaces_without_forced_type() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Hyperstition_concept.wiki"),
        "{{Infobox concept|name=Hyperstition concept}}\n'''Hyperstition concept''' is a concept page.\n== Philosophy ==\nHyperstition concept grounding text with SignalThread.\n<ref>{{Cite web|title=Concept Source|website=Remilia}}</ref>\n[[Category:Ideas]]",
    );
    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Hyperstition_collective.wiki"),
        "{{Infobox organization|name=Hyperstition Collective}}\n'''Hyperstition Collective''' is an organization.\n== History ==\nHyperstition group context.\n[[Category:Organizations]]",
    );
    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Hyperstition_art.wiki"),
        "{{Infobox artwork|name=Hyperstition art}}\n'''Hyperstition art''' is an artwork.\n== Background ==\nHyperstition artwork context.",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = build_authoring_context(
        &paths,
        Some("Hyperstition"),
        None,
        &AuthoringContextOptions::default(),
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };
    let article_scout = build_article_scout(&report, &test_site_adapter(), ArticleScoutIntent::New);
    let serialized = serde_json::to_string(&article_scout).expect("serialize article start");

    assert_eq!(article_scout.schema_version, "article_scout_v1");
    assert!(!serialized.contains("authoring_contract"));
    assert!(!serialized.contains("next_actions"));
    assert!(!serialized.contains("open_questions"));
    assert_eq!(article_scout.intent, ArticleScoutIntent::New);
    assert!(!serialized.contains("\"article_type\""));
    assert!(!serialized.contains("confidence"));
    assert!(
        article_scout
            .context_profile
            .query_terms
            .contains(&"hyperstition".to_string())
    );
    assert!(
        article_scout
            .context_profile
            .subject_context
            .iter()
            .any(|item| item.source_kind == "local_chunk")
    );
    assert!(article_scout.context_profile.missing_query_terms.is_empty());
    assert_eq!(
        article_scout.local_integration.required_templates[0].template_title,
        "Template:Article quality"
    );
    assert!(
        article_scout
            .local_integration
            .subject_type_hints
            .is_empty()
    );
    let infobox_titles = article_scout
        .local_integration
        .available_infoboxes
        .iter()
        .map(|entry| entry.template_title.clone())
        .collect::<Vec<_>>();
    let mut sorted_infobox_titles = infobox_titles.clone();
    sorted_infobox_titles.sort();
    assert_eq!(infobox_titles, sorted_infobox_titles);
    assert_eq!(
        infobox_titles,
        vec![
            "Template:Infobox concept".to_string(),
            "Template:Infobox organization".to_string(),
        ]
    );
    assert!(
        article_scout
            .local_integration
            .available_infoboxes
            .iter()
            .all(|entry| {
                entry.source == ContextSurfaceSource::Adapter && entry.supporting_pages.is_empty()
            })
    );
}

#[test]
fn build_article_scout_scopes_existing_page_comparables_to_its_infobox_type() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Aster_Vale.wiki"),
        "{{Infobox person|name=Aster Vale}}\n'''Aster Vale''' is a writer.\n== Biography ==\nAster Vale wrote essays.\n[[Category:People]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Morgan_Reed.wiki"),
        "{{Infobox person|name=Morgan Reed}}\n'''Morgan Reed''' is an editor who worked with Aster Vale.\n== Career ==\nReed edited Vale's essays.\n[[Category:People]]",
    );
    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Aster_Vale_launch.wiki"),
        "{{Infobox event|name=Aster Vale launch}}\nThe '''Aster Vale launch''' was an event.\n== Program ==\nAster Vale spoke.\n[[Category:Events]]",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = build_authoring_context(
        &paths,
        Some("Aster Vale"),
        None,
        &AuthoringContextOptions::default(),
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };
    let article_scout =
        build_article_scout(&report, &test_site_adapter(), ArticleScoutIntent::Audit);

    assert!(
        article_scout
            .local_integration
            .comparable_pages
            .iter()
            .all(|title| title != "Aster Vale launch")
    );
    assert!(
        article_scout
            .local_integration
            .available_infoboxes
            .iter()
            .all(|entry| entry.template_title == "Template:Infobox person")
    );
    assert!(
        article_scout
            .local_integration
            .observed_categories
            .iter()
            .all(|entry| entry.category_title != "Category:Events")
    );
    assert!(
        article_scout
            .local_integration
            .section_candidates
            .iter()
            .all(|entry| entry.heading != "Program")
    );
}

#[test]
fn build_article_scout_marks_empty_local_evidence_without_forcing_comparables() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "'''Alpha''' is an unrelated local page.\n== History ==\nAlpha context.",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = build_authoring_context(
        &paths,
        Some("Cheetah"),
        None,
        &AuthoringContextOptions::default(),
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };
    let article_scout = build_article_scout(&report, &test_site_adapter(), ArticleScoutIntent::New);

    assert_eq!(
        article_scout.context_profile.missing_query_terms,
        vec!["cheetah".to_string()]
    );
    assert!(article_scout.local_integration.comparable_pages.is_empty());
    assert_eq!(
        article_scout
            .local_integration
            .section_candidates
            .iter()
            .map(|section| section.heading.as_str())
            .collect::<Vec<_>>(),
        vec!["References"]
    );
}

#[test]
fn build_article_scout_does_not_treat_semantic_neighbors_as_new_article_peers() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Cheetah_Collective.wiki"),
        "{{Infobox organization|name=Cheetah Collective}}\n'''Cheetah Collective''' mentions [[Cheetah]].\n== History ==\nThe collective has a history.\n[[Category:Organizations]]",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = build_authoring_context(
        &paths,
        Some("Cheetah"),
        None,
        &AuthoringContextOptions::default(),
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };
    let article_scout = build_article_scout(&report, &test_site_adapter(), ArticleScoutIntent::New);

    assert_eq!(
        article_scout.local_state,
        LocalExistenceState::LinkedButMissing
    );
    assert!(article_scout.local_integration.comparable_pages.is_empty());
    assert!(
        article_scout
            .local_integration
            .subject_type_hints
            .is_empty()
    );
    assert!(
        article_scout
            .local_integration
            .observed_categories
            .is_empty()
    );
}

#[test]
fn build_article_scout_does_not_confuse_an_unrelated_missing_stub_link_with_the_topic() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "'''Alpha''' is an unrelated local page.\n",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = build_authoring_context(
        &paths,
        Some("Cheetah"),
        Some("A draft mentions [[Unrelated missing page]]."),
        &AuthoringContextOptions::default(),
    )
    .expect("authoring pack");
    let report = match report {
        AuthoringContextOutcome::Found(report) => *report,
        other => panic!("expected found authoring pack, got {other:?}"),
    };
    let article_scout = build_article_scout(&report, &test_site_adapter(), ArticleScoutIntent::New);

    assert_eq!(
        article_scout.local_state,
        LocalExistenceState::LikelyMissing
    );
}

#[test]
fn translation_variants_are_discovery_only_and_do_not_pollute_editing_context() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Network_spirituality.wiki"),
        "{{Infobox concept|name=Network spirituality}}\n'''Network spirituality''' anchors the BeaconSignal concept.\n== History ==\nBeaconSignal history text with grounded prose.",
    );
    write_file(
        &paths
            .wiki_content_dir
            .join("Main")
            .join("Network_spirituality___ko.wiki"),
        "{{Infobox concept|name=Network spirituality}}\n'''Network spirituality''' 한국어 BeaconSignal 문서.\n== References ==\n{{Reflist}}",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let search = query_search_local(&paths, "Network spirituality/ko", 10)
        .expect("search query")
        .expect("search should be available");
    assert_eq!(search.len(), 1);
    assert_eq!(search[0].title, "Network spirituality");
    assert_eq!(
        search[0].matched_translation_language.as_deref(),
        Some("ko")
    );
    assert_eq!(search[0].translation_languages, vec!["ko".to_string()]);

    let context_error =
        build_local_context(&paths, "Network spirituality/ko").expect_err("translation context");
    assert!(context_error.to_string().contains("discovery-only"));

    let chunk_error = crate::catalog::retrieval::retrieve_local_context_chunks_with_options(
        &paths,
        "Network spirituality/ko",
        None,
        4,
        240,
        true,
    )
    .expect_err("translation chunks");
    assert!(chunk_error.to_string().contains("discovery-only"));

    let pack_error = build_authoring_context(
        &paths,
        Some("Network spirituality/ko"),
        None,
        &AuthoringContextOptions::default(),
    )
    .expect_err("translation pack");
    assert!(pack_error.to_string().contains("discovery-only"));

    let retrieval =
        retrieve_local_context_chunks_across_pages(&paths, "BeaconSignal", 4, 240, 4, true)
            .expect("across-pages retrieval");
    let report = match retrieval {
        LocalChunkAcrossRetrieval::Found(report) => report,
        other => panic!("expected found report, got {other:?}"),
    };
    assert!(
        report
            .chunks
            .iter()
            .all(|chunk| chunk.source_title != "Network spirituality/ko")
    );
    assert!(
        report
            .chunks
            .iter()
            .all(|chunk| chunk.section_heading.as_deref() != Some("References"))
    );

    let catalog = query_active_template_catalog(&paths, 10).expect("catalog query");
    let catalog = match catalog {
        ActiveTemplateCatalogLookup::Found(catalog) => catalog,
        other => panic!("expected catalog, got {other:?}"),
    };
    let infobox = catalog
        .templates
        .iter()
        .find(|template| template.template_title == "Template:Infobox concept")
        .expect("infobox concept");
    assert!(
        infobox
            .example_invocations
            .iter()
            .all(|example| example.source_title != "Network spirituality/ko")
    );
}

#[test]
fn validation_checks_report_expected_issues() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "[[Beta]] [[MissingTarget]] [[Category:People]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
        "#REDIRECT [[Gamma]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("Gamma.wiki"),
        "#REDIRECT [[Delta]]",
    );
    write_file(
        &paths.wiki_content_dir.join("Main").join("NoCategory.wiki"),
        "Standalone page",
    );
    write_file(
        &paths.wiki_content_dir.join("Category").join("People.wiki"),
        "People category",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
    let report = run_validation_checks(&paths)
        .expect("validate query")
        .expect("validation should be available");

    assert_eq!(report.broken_links.len(), 2);
    assert!(report.broken_links.contains(&BrokenLinkIssue {
        source_title: "Alpha".to_string(),
        target_title: "MissingTarget".to_string(),
    }));
    assert!(report.broken_links.contains(&BrokenLinkIssue {
        source_title: "Gamma".to_string(),
        target_title: "Delta".to_string(),
    }));
    assert_eq!(report.double_redirects.len(), 1);
    assert_eq!(report.double_redirects[0].title, "Beta");
    assert!(
        report
            .uncategorized_pages
            .contains(&"NoCategory".to_string())
    );
    assert!(report.orphan_pages.contains(&"Alpha".to_string()));
}

#[test]
fn load_stored_index_stats_returns_none_when_db_is_missing() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("create project root");
    let paths = paths(&project_root);

    let stored = load_stored_index_stats(&paths).expect("load stats");
    assert!(stored.is_none());
}

#[test]
fn extract_first_url_handles_multibyte_prefix_text() {
    let value = "?? recap https://example.org/path?query=1|rest";
    assert_eq!(
        extract_first_url(value),
        Some("https://example.org/path?query=1".to_string())
    );
}
