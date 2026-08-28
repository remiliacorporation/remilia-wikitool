use std::fs;
use std::path::Path;

use tempfile::tempdir;

use crate::catalog::content_index::rebuild_index;
use crate::filesystem::ScanOptions;
use crate::runtime::{ResolvedPaths, ValueSource};

use super::{
    TemplateCatalogEntryLookup, build_template_catalog_with_adapter, find_template_catalog_entry,
};
use crate::site::{
    AuthoringRules, CategoryRules, CitationRules, InfoboxPreference, LintRules, SiteAdapter,
    TemplateRules,
};

fn paths(project_root: &Path) -> ResolvedPaths {
    let state_dir = project_root.join(".wikitool");
    let data_dir = state_dir.join("data");
    fs::create_dir_all(project_root.join("wiki_content/Main")).expect("wiki content");
    fs::create_dir_all(project_root.join("templates")).expect("templates");
    fs::create_dir_all(&data_dir).expect("data");
    ResolvedPaths {
        project_root: project_root.to_path_buf(),
        wiki_content_dir: project_root.join("wiki_content"),
        templates_dir: project_root.join("templates"),
        state_dir,
        data_dir: data_dir.clone(),
        db_path: data_dir.join("wikitool.db"),
        config_path: project_root.join(".wikitool/config.toml"),
        parser_config_path: project_root.join(".wikitool/parser-config.json"),
        root_source: ValueSource::Default,
        data_source: ValueSource::Default,
        config_source: ValueSource::Default,
    }
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write file");
}

fn test_adapter() -> SiteAdapter {
    SiteAdapter {
        schema_version: "resolved_site_adapter_v1".to_string(),
        adapter_id: "test".to_string(),
        docs_profile: "mw-1.44-authoring".to_string(),
        source_documents: Vec::new(),
        authoring: AuthoringRules {
            require_short_description: false,
            short_description_forms: Vec::new(),
            require_article_quality_banner: false,
            article_quality_template: None,
            article_quality_default_state: None,
            required_appendix_sections: Vec::new(),
            references_template: None,
            prefer_sentence_case_headings: false,
            prefer_wikitext_only: true,
            forbid_markdown: true,
            require_straight_quotes: false,
        },
        citations: CitationRules {
            preferred_templates: Vec::new(),
            use_named_references: false,
            leave_archive_fields_blank: false,
            source_review_rules: Vec::new(),
        },
        templates: TemplateRules {
            infobox_preferences: vec![InfoboxPreference {
                subject_type: "person".to_string(),
                template_title: "Template:Infobox person".to_string(),
            }],
        },
        categories: CategoryRules {
            preferred_categories: Vec::new(),
        },
        lint: LintRules {
            forbid_curly_quotes: false,
            forbid_placeholder_fragments: Vec::new(),
            proper_nouns: Vec::new(),
            citation_needed: crate::site::LintRuleLevel::Ignore,
        },
        extension_contracts: Vec::new(),
        resolved_at: "1".to_string(),
    }
}

#[test]
fn template_catalog_fuses_local_docs_templatedata_and_usage() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);

    write_file(
        &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
        "{{Infobox person|name=Alpha|occupation=Writer|birth date=2000}}\n'''Alpha''' is a page.",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Infobox_person.wiki"),
        r#"<includeonly>{{#invoke:Infobox|render|name={{{name|}}}|occupation={{{occupation|}}}|birth_date={{{birth_date|}}}}}</includeonly><noinclude>
<syntaxhighlight lang="wikitext">
{{Infobox person
| name = Example
| occupation = Writer
| birth_date = 2000
}}
</syntaxhighlight>
<templatedata>
{
  "description": "Infobox for biographical articles.",
  "params": {
    "name": {"label": "Name", "required": true, "example": "Alpha", "default": "Unknown"},
    "occupation": {"label": "Occupation", "suggested": true, "suggestedvalues": ["Writer", "Artist"], "autovalue": "Writer"},
    "birth_date": {"label": "Birth date", "suggested": true}
  }
}
</templatedata>
</noinclude>"#,
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Infobox_person___doc.wiki"),
        "Documentation lead.\n<syntaxhighlight lang=\"wikitext\">\n{{Infobox person|name=Doc example}}\n</syntaxhighlight>",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Module_Infobox.lua"),
        "return {}",
    );
    write_file(
        &paths
            .templates_dir
            .join("redirects")
            .join("Template_Infobox_human.wikitext"),
        "#REDIRECT [[Template:Infobox person]]",
    );

    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");
    let catalog = build_template_catalog_with_adapter(&paths, &test_adapter()).expect("catalog");
    assert!(catalog.usage_index_ready);
    assert_eq!(catalog.template_count, 1);
    let entry = &catalog.entries[0];
    assert_eq!(entry.template_title, "Template:Infobox person");
    assert!(
        entry
            .redirect_aliases
            .contains(&"Template:Infobox human".to_string())
    );
    let name = entry
        .parameters
        .iter()
        .find(|param| param.name == "name")
        .expect("name parameter");
    assert!(name.required);
    assert_eq!(name.example.as_deref(), Some("Alpha"));
    assert_eq!(name.default_value.as_deref(), Some("Unknown"));
    assert!(name.suggested_values.is_empty());
    assert!(name.auto_value.is_none());
    let occupation = entry
        .parameters
        .iter()
        .find(|param| param.name == "occupation")
        .expect("occupation parameter");
    assert_eq!(
        occupation.suggested_values,
        vec!["Writer".to_string(), "Artist".to_string()]
    );
    assert_eq!(occupation.auto_value.as_deref(), Some("Writer"));
    let birth_date = entry
        .parameters
        .iter()
        .find(|param| param.name == "birth_date")
        .expect("birth_date parameter");
    assert!(birth_date.aliases.contains(&"birth date".to_string()));
    assert!(
        birth_date
            .observed_names
            .contains(&"birth date".to_string())
    );
    assert!(birth_date.usage_count >= 1);
    assert!(
        entry
            .examples
            .iter()
            .any(|example| example.source_kind == "documentation")
    );
    assert!(
        entry
            .documentation_titles
            .iter()
            .any(|title| title == "Template:Infobox person/doc")
    );
    assert!(entry.module_titles.contains(&"Module:Infobox".to_string()));
    assert!(
        entry
            .recommendation_tags
            .contains(&"preferred_infobox_template".to_string())
    );
}

#[test]
fn template_catalog_lookup_matches_aliases() {
    let catalog = super::TemplateCatalog {
        schema_version: "v1".to_string(),
        site_adapter_id: "remilia".to_string(),
        refreshed_at: "1".to_string(),
        template_count: 1,
        templatedata_count: 0,
        redirect_alias_count: 1,
        usage_index_ready: false,
        entries: vec![super::TemplateCatalogEntry {
            template_title: "Template:Infobox person".to_string(),
            relative_path: "templates/infobox/Template_Infobox_person.wiki".to_string(),
            category: "infobox".to_string(),
            summary_text: None,
            templatedata: None,
            redirect_aliases: vec!["Template:Infobox human".to_string()],
            usage_aliases: Vec::new(),
            usage_count: 0,
            distinct_page_count: 0,
            example_pages: Vec::new(),
            documentation_titles: Vec::new(),
            implementation_titles: Vec::new(),
            implementation_preview: None,
            module_titles: Vec::new(),
            declared_parameter_keys: Vec::new(),
            parameters: Vec::new(),
            examples: Vec::new(),
            recommendation_tags: Vec::new(),
        }],
    };

    match find_template_catalog_entry(&catalog, "Template:Infobox human") {
        TemplateCatalogEntryLookup::Found(entry) => {
            assert_eq!(entry.template_title, "Template:Infobox person");
        }
        other => panic!("expected alias match, got {other:?}"),
    }
}
