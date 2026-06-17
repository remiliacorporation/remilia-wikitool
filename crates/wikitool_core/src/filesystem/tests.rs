use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use super::{
    Namespace, ScanOptions, case_safe_title_relative_path, content_path_to_title,
    relative_path_to_title, scan_files, scan_stats, template_path_to_title, title_to_relative_path,
    validate_scoped_path,
};
use crate::runtime::{ResolvedPaths, ValueSource};
use tempfile::tempdir;

fn paths(root: &str) -> ResolvedPaths {
    let project_root = PathBuf::from(root);
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
        project_root,
        root_source: ValueSource::Flag,
        data_source: ValueSource::Default,
        config_source: ValueSource::Default,
    }
}

fn paths_with_db(temp: &tempfile::TempDir) -> ResolvedPaths {
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(project_root.join(".wikitool/data")).expect("create data dir");
    let paths = ResolvedPaths {
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
        project_root,
        root_source: ValueSource::Flag,
        data_source: ValueSource::Default,
        config_source: ValueSource::Default,
    };
    crate::schema::ensure_database_schema(&paths).expect("schema");
    paths
}

#[test]
fn mapping_roundtrip_content_and_templates() {
    let temp = tempdir().expect("tempdir");
    let paths = paths_with_db(&temp);

    let cases = [
        ("Alpha", false, "wiki_content/Main/Alpha.wiki"),
        ("Category:Test", false, "wiki_content/Category/Test.wiki"),
        (
            "Template:Infobox person",
            false,
            "templates/infobox/Template_Infobox_person.wiki",
        ),
        (
            "Module:Navbar/styles.css",
            false,
            "templates/navbox/Module_Navbar_styles.css",
        ),
        (
            "MediaWiki:Common.css",
            false,
            "templates/mediawiki/Common.css",
        ),
        (
            "Template:Infobox person",
            true,
            "templates/infobox/_redirects/Template_Infobox_person.wiki",
        ),
    ];

    for (title, redirect, expected) in cases {
        let relative = title_to_relative_path(&paths, title, redirect).expect("relative");
        assert_eq!(
            relative, expected,
            "failed for title={title} redirect={redirect}"
        );
        let parsed = relative_path_to_title(&paths, &relative).expect("title");
        if title == "MediaWiki:Common.css" {
            assert_eq!(parsed, "MediaWiki:Common.css");
        } else {
            assert_eq!(parsed, title);
        }
    }
}

#[test]
fn custom_namespace_uses_configured_name_folder_mapping() {
    let temp = tempdir().expect("tempdir");
    let paths = paths_with_db(&temp);
    fs::create_dir_all(&paths.wiki_content_dir).expect("content dir");
    fs::write(
        &paths.config_path,
        r#"
[wiki]

[[wiki.custom_namespaces]]
name = "Lore"
id = 3000
folder = "LorePages"
"#,
    )
    .expect("write config");
    fs::create_dir_all(paths.wiki_content_dir.join("LorePages")).expect("lore pages dir");

    let relative = title_to_relative_path(&paths, "Lore:Chronicle", false).expect("relative");
    assert_eq!(relative, "wiki_content/LorePages/Chronicle.wiki");
    let parsed = relative_path_to_title(&paths, &relative).expect("title");
    assert_eq!(parsed, "Lore:Chronicle");

    fs::write(
        paths
            .wiki_content_dir
            .join("LorePages")
            .join("Chronicle.wiki"),
        "Lore content",
    )
    .expect("write custom namespace page");
    let files = scan_files(&paths, &ScanOptions::default()).expect("scan files");
    let scanned = files
        .iter()
        .find(|file| file.title == "Lore:Chronicle")
        .expect("custom namespace page must be scanned");
    assert_eq!(scanned.namespace, "Lore");
}

#[test]
fn windows_separator_content_parse() {
    let title = content_path_to_title("Category\\_redirects\\Category_Test.wiki");
    assert_eq!(title, "Category:Category Test");
}

#[test]
fn windows_separator_template_parse() {
    let title = template_path_to_title("navbox\\Module_Navbar\\configuration.lua");
    assert_eq!(title, "Module:Navbar/configuration");
}

#[test]
fn case_safe_paths_decode_exact_mediawiki_title() {
    let relative = case_safe_title_relative_path(
        "templates/quotation/Template_Quote_box.wiki",
        "Template:Quote box",
    );
    assert!(relative.contains("__mwtitle_"));
    assert_eq!(
        template_path_to_title(
            relative
                .strip_prefix("templates/")
                .expect("template relative path")
        ),
        "Template:Quote box"
    );

    let relative = case_safe_title_relative_path(
        "wiki_content/Main/I_Long_For_Network_Spirituality.wiki",
        "I Long For Network Spirituality",
    );
    assert_eq!(
        content_path_to_title(
            relative
                .strip_prefix("wiki_content/")
                .expect("content relative path")
        ),
        "I Long For Network Spirituality"
    );
}

#[test]
fn scoped_path_validation_blocks_escaping_path() {
    let paths = paths("/workspace/project");
    let unsafe_path = PathBuf::from("/workspace/secrets/token.txt");
    let error = validate_scoped_path(&paths, &unsafe_path).expect_err("must fail");
    assert!(
        error
            .to_string()
            .contains("path escapes scoped runtime directories")
    );
}

#[cfg(unix)]
#[test]
fn scoped_path_validation_blocks_symlink_escape() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    let outside_root = temp.path().join("outside");
    fs::create_dir_all(project_root.join("wiki_content").join("Main")).expect("content dir");
    fs::create_dir_all(project_root.join("templates")).expect("templates dir");
    fs::create_dir_all(project_root.join(".wikitool")).expect("state dir");
    fs::create_dir_all(&outside_root).expect("outside dir");
    symlink(
        &outside_root,
        project_root
            .join("wiki_content")
            .join("Main")
            .join("escape"),
    )
    .expect("symlink");

    let paths = paths(project_root.to_str().expect("utf8 root"));
    let candidate = project_root
        .join("wiki_content")
        .join("Main")
        .join("escape")
        .join("secret.wiki");
    let error = validate_scoped_path(&paths, &candidate).expect_err("must fail");
    assert!(
        error
            .to_string()
            .contains("path escapes scoped runtime directories")
    );
}

#[test]
fn scan_stats_on_fixture_corpus() {
    let temp = tempdir().expect("tempdir");
    let project_root = temp.path().to_path_buf();

    fs::create_dir_all(project_root.join("wiki_content").join("Main")).expect("content main dir");
    fs::create_dir_all(project_root.join("wiki_content").join("Category"))
        .expect("content category dir");
    fs::create_dir_all(
        project_root
            .join("custom")
            .join("templates")
            .join("infobox"),
    )
    .expect("template infobox dir");
    fs::create_dir_all(
        project_root
            .join("custom")
            .join("templates")
            .join("infobox")
            .join("_redirects"),
    )
    .expect("template redirects dir");
    fs::create_dir_all(
        project_root
            .join("custom")
            .join("templates")
            .join("navbox")
            .join("Module_Navbar"),
    )
    .expect("module navbox dir");

    fs::write(
        project_root
            .join("wiki_content")
            .join("Main")
            .join("Alpha.wiki"),
        "'''Alpha''' content",
    )
    .expect("write alpha");
    fs::write(
        project_root
            .join("wiki_content")
            .join("Category")
            .join("Category_Test.wiki"),
        "[[Category:Root]]",
    )
    .expect("write category");
    fs::write(
        project_root
            .join("custom")
            .join("templates")
            .join("infobox")
            .join("Template_Infobox_test.wiki"),
        "{{Infobox test}}",
    )
    .expect("write template");
    fs::write(
        project_root
            .join("custom")
            .join("templates")
            .join("infobox")
            .join("_redirects")
            .join("Template_Infobox_legacy.wiki"),
        "#REDIRECT [[Template:Infobox test]]",
    )
    .expect("write template redirect");
    fs::write(
        project_root
            .join("custom")
            .join("templates")
            .join("navbox")
            .join("Module_Navbar.lua"),
        "return {}",
    )
    .expect("write module");
    fs::write(
        project_root
            .join("custom")
            .join("templates")
            .join("navbox")
            .join("Module_Navbar")
            .join("configuration.lua"),
        "return {}",
    )
    .expect("write module subpage");

    let paths = ResolvedPaths {
        wiki_content_dir: project_root.join("wiki_content"),
        templates_dir: project_root.join("custom").join("templates"),
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
        project_root,
        root_source: ValueSource::Flag,
        data_source: ValueSource::Default,
        config_source: ValueSource::Default,
    };

    let stats = scan_stats(&paths, &ScanOptions::default()).expect("stats");
    assert_eq!(stats.total_files, 6);
    assert_eq!(stats.content_files, 2);
    assert_eq!(stats.template_files, 4);
    assert_eq!(
        stats.by_namespace,
        BTreeMap::from([
            (Namespace::Category.as_str().to_string(), 1),
            (Namespace::Main.as_str().to_string(), 1),
            (Namespace::Module.as_str().to_string(), 2),
            (Namespace::Template.as_str().to_string(), 2),
        ])
    );
}
