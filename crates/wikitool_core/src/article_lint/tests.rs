use std::fs;
use std::path::Path;

use tempfile::tempdir;

use crate::filesystem::ScanOptions;
use crate::knowledge::content_index::rebuild_index;
use crate::runtime::{ResolvedPaths, ValueSource};

use super::*;

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
        "**Never use:**\n- \"stands as\"\n### No placeholder content\n- Never output: `INSERT_SOURCE_URL`\n### No system artifacts\n- Never output: `contentReference[oaicite:0]`\nStraight quotes only",
    );
    write_file(
        &paths
            .project_root
            .join("tools/wikitool/ai-pack/writing_context/writing_guide.md"),
        "raw MediaWiki wikitext\nNever output Markdown\nUse 2-4 categories per article\n[[Category:Remilia]]\n{{Article quality|unverified}}\nparent_group = Remilia\n### Citation templates\n```wikitext\n{{Cite web|url=}}\n```\n## 6. Infobox selection\n| Subject type | Infobox |\n|---|---|\n| NFT Collection | `{{Infobox NFT collection}}` |\n",
    );
}

fn write_common_templates(paths: &ResolvedPaths) {
    write_file(
        &paths
            .templates_dir
            .join("misc")
            .join("Template_Article_quality.wiki"),
        "<includeonly>{{{1|unverified}}}</includeonly>",
    );
    write_file(
        &paths
            .templates_dir
            .join("misc")
            .join("Template_Reflist.wiki"),
        "<references />",
    );
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Infobox_NFT_collection.wiki"),
        "<includeonly>{{{name|}}} {{{parent_group|}}}</includeonly>",
    );
}

fn write_capability_manifest(paths: &ResolvedPaths, manifest: &WikiCapabilityManifest) {
    let connection = open_initialized_database_connection(&paths.db_path).expect("db");
    connection
        .execute(
            "INSERT INTO knowledge_artifacts (
                    artifact_key,
                    artifact_kind,
                    profile,
                    schema_generation,
                    built_at_unix,
                    row_count,
                    metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                "wiki_capabilities:test",
                "wiki_capabilities",
                Some("wiki.remilia.org"),
                KNOWLEDGE_GENERATION,
                1i64,
                1i64,
                serde_json::to_string(manifest).expect("manifest json"),
            ],
        )
        .expect("insert manifest");
}

fn wiki_capability_manifest(
    parser_extension_tags: Vec<String>,
    parser_function_hooks: Vec<String>,
    has_scribunto: bool,
) -> WikiCapabilityManifest {
    WikiCapabilityManifest {
        schema_version: "wiki_capabilities_v1".to_string(),
        wiki_id: "wiki.remilia.org".to_string(),
        wiki_url: "https://wiki.remilia.org".to_string(),
        api_url: "https://wiki.remilia.org/api.php".to_string(),
        rest_url: None,
        article_path: "/$1".to_string(),
        mediawiki_version: Some("1.44.3".to_string()),
        namespaces: Vec::new(),
        extensions: Vec::new(),
        parser_extension_tags,
        parser_function_hooks,
        special_pages: Vec::new(),
        search_backend_hint: None,
        has_visual_editor: false,
        has_templatedata: false,
        has_citoid: false,
        has_cargo: false,
        has_page_forms: false,
        has_short_description: true,
        has_scribunto,
        has_timed_media_handler: false,
        supports_parse_api_html: true,
        supports_rest_html: false,
        rest_html_path_template: None,
        refreshed_at: "1".to_string(),
    }
}

fn has_rule(report: &ArticleLintReport, rule_id: &str) -> bool {
    report.issues.iter().any(|issue| issue.rule_id == rule_id)
}

#[test]
fn detects_markdown_heading_and_applies_safe_fix() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n## History\nText.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "structure.markdown_heading"));

    let fixed = fix_article(&paths, &article_path, ArticleFixApplyMode::Safe).expect("safe fix");
    assert!(fixed.changed);
    let content = fs::read_to_string(&article_path).expect("read article");
    assert!(content.contains("== History =="));
}

#[test]
fn detects_raw_wikitext_balance_errors_inside_references() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\nAlpha cites a malformed source.<ref>{{Cite web|url=https://example.com|title=Example}</ref>\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(has_rule(&report, "wikitext.unclosed_template"));
}

#[test]
fn detects_sentence_case_heading() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n== Early Life ==\nText.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "style.sentence_case_heading"));
}

#[test]
fn accepts_tabber_separator_lines_as_extension_markup() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page.\n\n<tabber>\n|-|First tab=\nText.\n|-|Second tab=\nMore text.\n</tabber>\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(!has_rule(&report, "structure.malformed_heading"));
}

#[test]
fn accepts_template_parameter_lines_that_end_with_equals() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{Infobox subject\n| name = Alpha\n| image =\n| type = Test\n}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(!has_rule(&report, "structure.malformed_heading"));
}

#[test]
fn detects_invalid_extension_block_shapes() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page.\n\n<tabber>\n|-|=\nText.\n</tabber>\n\n<gallery>\nNot a file line\n</gallery>\n\n<DPL>\n</DPL>\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(has_rule(&report, "extension.tabber_empty_label"));
    assert!(has_rule(&report, "extension.gallery_empty"));
    assert!(has_rule(&report, "extension.dpl_empty"));
}

#[test]
fn detects_unavailable_module_functions_from_local_lua_exports() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_file(
        &paths.templates_dir.join("misc").join("Module_Chart.lua"),
        "local p = {}\nfunction p.bar(frame) return '' end\np.line = function(frame) return '' end\nreturn p\n",
    );
    write_capability_manifest(
        &paths,
        &wiki_capability_manifest(Vec::new(), vec!["invoke".to_string()], true),
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{#invoke:Chart|scatter|data=1:2}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(has_rule(&report, "module.unavailable_function"));
}

#[test]
fn detects_d3chart_semantic_contract_errors() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_file(
        &paths.templates_dir.join("misc").join("Module_D3Chart.lua"),
        "local p = {}\nfunction p.bar(frame) return '' end\nfunction p.scatter(frame) return '' end\nfunction p.chart(frame) return '' end\nreturn p\n",
    );
    write_capability_manifest(
        &paths,
        &wiki_capability_manifest(Vec::new(), vec!["invoke".to_string()], true),
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{#invoke:D3Chart|bar|title=No data}}\n{{#invoke:D3Chart|scatter|data=badpair}}\n{{#invoke:D3Chart|chart|type=heatmap|data=A:1}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(has_rule(&report, "module.d3chart_missing_data_source"));
    assert!(has_rule(&report, "module.d3chart_invalid_data"));
    assert!(has_rule(&report, "module.d3chart_unknown_type"));
}

#[test]
fn lints_state_draft_with_explicit_title_override() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.state_dir.join("drafts").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a draft.\n\n== References ==\n{{Reflist}}\n",
    );

    let report =
        lint_article_with_title(&paths, &article_path, Some("Category:Alpha")).expect("lint");

    assert_eq!(report.relative_path, ".wikitool/drafts/Alpha.wiki");
    assert_eq!(report.title, "Category:Alpha");
    assert_eq!(report.namespace, "Category");
}

#[test]
fn safe_fix_preserves_state_draft_title_override() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.state_dir.join("drafts").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n\n'''Alpha''' is a draft.\n\n== References ==\n{{Reflist}}\n",
    );

    let fixed = fix_article_with_title(
        &paths,
        &article_path,
        ArticleFixApplyMode::Safe,
        Some("Draft Alpha"),
    )
    .expect("safe fix");

    assert!(fixed.changed);
    assert_eq!(fixed.title, "Draft Alpha");
    assert_eq!(fixed.remaining_report.title, "Draft Alpha");
}

#[test]
fn detects_missing_short_description() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{Article quality|unverified}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "structure.require_short_description"));
}

#[test]
fn inserts_missing_article_quality_banner_with_safe_fix() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let fixed = fix_article(&paths, &article_path, ArticleFixApplyMode::Safe).expect("safe fix");
    assert!(fixed.changed);
    let content = fs::read_to_string(&article_path).expect("read article");
    assert!(content.contains("{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n"));
}

#[test]
fn detects_missing_reflist_and_applies_safe_fix() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page.<ref>{{Cite web|title=Source}}</ref>\n\n== References ==\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "structure.require_reflist"));

    let fixed = fix_article(&paths, &article_path, ArticleFixApplyMode::Safe).expect("safe fix");
    assert!(fixed.changed);
    let content = fs::read_to_string(&article_path).expect("read article");
    assert!(content.contains("== References ==\n{{Reflist}}\n"));
}

#[test]
fn inserts_reflist_before_reference_section_trailing_categories() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page.<ref>{{Cite web|title=Source}}</ref>\n\n== References ==\n[[Category:Ideas and Concepts]]\n",
    );

    let fixed = fix_article(&paths, &article_path, ArticleFixApplyMode::Safe).expect("safe fix");
    assert!(fixed.changed);
    let content = fs::read_to_string(&article_path).expect("read article");
    assert!(content.contains("== References ==\n{{Reflist}}\n[[Category:Ideas and Concepts]]"));
}

#[test]
fn detects_citation_after_punctuation_and_applies_safe_fix() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page<ref>{{Cite web|title=Source}}</ref>.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "citation.after_punctuation"));

    let fixed = fix_article(&paths, &article_path, ArticleFixApplyMode::Safe).expect("safe fix");
    assert!(fixed.changed);
    let content = fs::read_to_string(&article_path).expect("read article");
    assert!(content.contains("page.<ref>{{Cite web|title=Source}}</ref>"));
}

#[test]
fn clustered_citations_move_punctuation_before_the_whole_cluster() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page<ref name=\"a\">{{Cite web|title=Source A}}</ref><ref name=\"b\">{{Cite web|title=Source B}}</ref>.\n\n== References ==\n{{Reflist}}\n",
    );

    let fixed = fix_article(&paths, &article_path, ArticleFixApplyMode::Safe).expect("safe fix");
    assert!(fixed.changed);
    let content = fs::read_to_string(&article_path).expect("read article");
    assert!(content.contains("page.<ref name=\"a\">{{Cite web|title=Source A}}</ref><ref name=\"b\">{{Cite web|title=Source B}}</ref>"));
    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(!has_rule(&report, "citation.after_punctuation"));
}

#[test]
fn detects_remilia_parent_group_rule() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths
        .wiki_content_dir
        .join("Main")
        .join("Milady_Maker.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{Infobox NFT collection\n| name = Milady Maker\n| creator = Remilia\n}}\n\n'''Milady Maker''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "profile.remilia_parent_group"));
}

#[test]
fn rejects_citation_needed_templates() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page. {{Citation needed}}\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "profile.no_citation_needed"));
}

#[test]
fn detects_red_links_in_see_also() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_file(
        &paths.wiki_content_dir.join("Main").join("Existing.wiki"),
        "{{SHORTDESC:Existing}}\n{{Article quality|unverified}}\n\n'''Existing''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page.\n\n== See also ==\n* [[Existing]]\n* [[Missing]]\n\n== References ==\n{{Reflist}}\n",
    );
    rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "integration.red_link_in_see_also"));
}

#[test]
fn detects_unavailable_templates_against_local_catalog() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{Mystery box|value=1}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "template.unavailable"));
}

#[test]
fn detects_unknown_parameters_for_templatedata_backed_templates() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_file(
        &paths
            .templates_dir
            .join("infobox")
            .join("Template_Profile_box.wiki"),
        r#"<includeonly>{{{name|}}} {{{image|}}}</includeonly><noinclude>
<templatedata>
{
  "description": "Profile box",
  "params": {
    "name": {"label": "Name"},
    "image": {"aliases": ["photo"]}
  }
}
</templatedata>
</noinclude>"#,
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{Profile box|name=Alpha|photo=Alpha.png|made_up=1}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(has_rule(&report, "template.unknown_parameter"));
}

#[test]
fn detects_unavailable_modules_for_direct_invoke() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_capability_manifest(
        &paths,
        &wiki_capability_manifest(Vec::new(), vec!["invoke".to_string()], true),
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{#invoke:Missing|render|name=Alpha}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(has_rule(&report, "module.unavailable"));
}

#[test]
fn accepts_direct_invoke_for_local_module() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_file(
        &paths.templates_dir.join("misc").join("Module_Profile.lua"),
        "return { render = function(frame) return frame.args.name or '' end }\n",
    );
    write_capability_manifest(
        &paths,
        &wiki_capability_manifest(Vec::new(), vec!["invoke".to_string()], true),
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{#invoke:Profile|render|name=Alpha}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(!has_rule(&report, "module.unavailable"));
    assert!(!has_rule(&report, "capability.scribunto_unsupported"));
}

#[test]
fn detects_invoke_when_scribunto_is_not_available() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_file(
        &paths.templates_dir.join("misc").join("Module_Profile.lua"),
        "return { render = function(frame) return frame.args.name or '' end }\n",
    );
    write_capability_manifest(
        &paths,
        &wiki_capability_manifest(Vec::new(), Vec::new(), false),
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{#invoke:Profile|render|name=Alpha}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(has_rule(&report, "capability.scribunto_unsupported"));
}

#[test]
fn accepts_templatestyles_for_local_asset() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_file(
        &paths
            .templates_dir
            .join("misc")
            .join("Template_Profile")
            .join("style.css.wiki"),
        ".profile { display: block; }\n",
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n<templatestyles src=\"profile/style.css\" />\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(!has_rule(
        &report,
        "asset.templatestyles_unavailable_source"
    ));
}

#[test]
fn detects_unavailable_templatestyles_source() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n<templatestyles src=\"Missing/style.css\" />\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(has_rule(&report, "asset.templatestyles_unavailable_source"));
}

#[test]
fn detects_templatestyles_missing_src() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n<templatestyles />\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");

    assert!(has_rule(&report, "asset.templatestyles_missing_src"));
}

#[test]
fn detects_unsupported_extension_tags_from_capabilities() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_capability_manifest(
        &paths,
        &WikiCapabilityManifest {
            schema_version: "wiki_capabilities_v1".to_string(),
            wiki_id: "wiki.remilia.org".to_string(),
            wiki_url: "https://wiki.remilia.org".to_string(),
            api_url: "https://wiki.remilia.org/api.php".to_string(),
            rest_url: None,
            article_path: "/$1".to_string(),
            mediawiki_version: Some("1.44.3".to_string()),
            namespaces: Vec::new(),
            extensions: Vec::new(),
            parser_extension_tags: vec!["math".to_string()],
            parser_function_hooks: Vec::new(),
            special_pages: Vec::new(),
            search_backend_hint: None,
            has_visual_editor: false,
            has_templatedata: false,
            has_citoid: false,
            has_cargo: false,
            has_page_forms: false,
            has_short_description: true,
            has_scribunto: false,
            has_timed_media_handler: false,
            supports_parse_api_html: true,
            supports_rest_html: false,
            rest_html_path_template: None,
            refreshed_at: "1".to_string(),
        },
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n<tabber>\n|-|One=Alpha\n</tabber>\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "capability.unsupported_extension_tag"));
}

#[test]
fn detects_suspicious_html_tags_even_when_they_are_not_known_extensions() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let paths = paths(&project_root);
    write_instruction_sources(&paths);
    write_common_templates(&paths);
    write_capability_manifest(
        &paths,
        &WikiCapabilityManifest {
            schema_version: "wiki_capabilities_v1".to_string(),
            wiki_id: "wiki.remilia.org".to_string(),
            wiki_url: "https://wiki.remilia.org".to_string(),
            api_url: "https://wiki.remilia.org/api.php".to_string(),
            rest_url: None,
            article_path: "/$1".to_string(),
            mediawiki_version: Some("1.44.3".to_string()),
            namespaces: Vec::new(),
            extensions: Vec::new(),
            parser_extension_tags: vec!["<ref>".to_string(), "<references>".to_string()],
            parser_function_hooks: Vec::new(),
            special_pages: Vec::new(),
            search_backend_hint: None,
            has_visual_editor: false,
            has_templatedata: false,
            has_citoid: false,
            has_cargo: false,
            has_page_forms: false,
            has_short_description: true,
            has_scribunto: false,
            has_timed_media_handler: false,
            supports_parse_api_html: true,
            supports_rest_html: false,
            rest_html_path_template: None,
            refreshed_at: "1".to_string(),
        },
    );
    let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
    write_file(
        &article_path,
        "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n<blink>Alpha</blink>\n\n== References ==\n{{Reflist}}\n",
    );

    let report = lint_article(&paths, &article_path).expect("lint");
    assert!(has_rule(&report, "capability.unsupported_extension_tag"));
}
