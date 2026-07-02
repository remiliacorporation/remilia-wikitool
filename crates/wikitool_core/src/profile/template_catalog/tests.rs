use std::fs;
use std::path::Path;

use tempfile::tempdir;

use crate::filesystem::ScanOptions;
use crate::knowledge::content_index::rebuild_index;
use crate::runtime::{ResolvedPaths, ValueSource};

use super::{
    TemplateCatalogEntryLookup, build_template_catalog_with_overlay, find_template_catalog_entry,
};
use crate::profile::remilia_overlay::build_remilia_profile_overlay;

fn paths(project_root: &Path) -> ResolvedPaths {
    let state_dir = project_root.join(".wikitool");
    let data_dir = state_dir.join("data");
    fs::create_dir_all(project_root.join("wiki_content/Main")).expect("wiki content");
    fs::create_dir_all(project_root.join("templates")).expect("templates");
    fs::create_dir_all(&data_dir).expect("data");
    fs::create_dir_all(project_root.join("tools/wikitool/ai-pack/writing_context"))
        .expect("instructions");
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

fn write_instruction_sources(paths: &ResolvedPaths) {
    write_file(
        &paths
            .project_root
            .join("tools/wikitool/ai-pack/writing_context/article_structure.md"),
        "{{SHORTDESC:Example}}\n{{Article quality|unverified}}\n== References ==\n{{Reflist}}\nparent_group = Remilia",
    );
    write_file(
        &paths
            .project_root
            .join("tools/wikitool/ai-pack/writing_context/style_rules.md"),
        "**Never use:**\n- \"stands as\", \"rich tapestry\"\n### No placeholder content\n- Never output: `[Author Name]`",
    );
    write_file(
        &paths
            .project_root
            .join("tools/wikitool/ai-pack/writing_context/writing_guide.md"),
        "raw MediaWiki wikitext\nNever output Markdown\nUse 2-4 categories per article\n[[Category:Remilia]]\n{{Article quality|unverified}}\n### Citation templates\n```wikitext\n{{Cite web|url=}}\n```\n## 6. Infobox selection\n| Subject type | Infobox |\n|---|---|\n| Person | `{{Infobox person}}` |\n| NFT Collection | `{{Infobox NFT collection}}` |\n",
    );
}

#[test]
fn template_catalog_fuses_local_docs_templatedata_and_usage() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);

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
    let overlay = build_remilia_profile_overlay(&paths).expect("overlay");
    let catalog = build_template_catalog_with_overlay(&paths, &overlay).expect("catalog");
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
        profile_id: "remilia".to_string(),
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
