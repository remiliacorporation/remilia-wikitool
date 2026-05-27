use super::promote::run_article_promote;
use super::selection::{path_is_under_state_drafts_dir, single_state_path_title_override};
use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use wikitool_core::runtime::{ResolvedPaths, ValueSource};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "wikitool-article-cli-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp test dir");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn test_paths(project_root: &Path) -> ResolvedPaths {
    let wiki_content_dir = project_root.join("wiki_content");
    let templates_dir = project_root.join("templates");
    let state_dir = project_root.join(".wikitool");
    let data_dir = state_dir.join("data");
    fs::create_dir_all(&wiki_content_dir).expect("wiki content dir");
    fs::create_dir_all(&templates_dir).expect("templates dir");
    fs::create_dir_all(&data_dir).expect("data dir");
    ResolvedPaths {
        project_root: project_root.to_path_buf(),
        wiki_content_dir,
        templates_dir,
        state_dir: state_dir.clone(),
        data_dir: data_dir.clone(),
        db_path: data_dir.join("wikitool.db"),
        config_path: state_dir.join("config.toml"),
        parser_config_path: state_dir.join("parser-config.json"),
        root_source: ValueSource::Default,
        data_source: ValueSource::Default,
        config_source: ValueSource::Default,
    }
}

#[test]
fn state_draft_title_override_accepts_single_state_path_title() {
    let temp = TestDir::new("draft-title");
    let paths = test_paths(&temp.path);
    let draft_path = paths.state_dir.join("drafts").join("Cheetah.wiki");
    fs::create_dir_all(draft_path.parent().expect("draft parent")).expect("draft parent");
    fs::write(&draft_path, "Text.").expect("draft");
    let titles = vec!["Cheetah".to_string()];

    let override_title =
        single_state_path_title_override(&paths, Some(&draft_path), &titles, &[], None, false)
            .expect("title override");

    assert_eq!(override_title, Some("Cheetah"));
}

#[test]
fn state_title_override_rejects_non_draft_state_path() {
    let temp = TestDir::new("state-non-draft-title");
    let paths = test_paths(&temp.path);
    let state_path = paths.state_dir.join("data").join("Cheetah.wiki");
    fs::write(&state_path, "Text.").expect("state file");
    let titles = vec!["Cheetah".to_string()];

    let override_title =
        single_state_path_title_override(&paths, Some(&state_path), &titles, &[], None, false)
            .expect("title override");

    assert_eq!(override_title, None);
}

#[test]
fn state_draft_detection_requires_canonical_state_dir_spelling() {
    let temp = TestDir::new("draft-case");
    let paths = test_paths(&temp.path);
    let candidate = paths.project_root.join(".WIKITOOL").join("drafts");

    assert!(!path_is_under_state_drafts_dir(&paths, &candidate));
}

#[test]
fn article_promote_copies_state_draft_to_title_path() {
    let temp = TestDir::new("promote");
    let paths = test_paths(&temp.path);
    let draft_path = paths.state_dir.join("drafts").join("Cheetah.wiki");
    fs::create_dir_all(draft_path.parent().expect("draft parent")).expect("draft parent");
    fs::write(&draft_path, "'''Cheetah''' is a cat.").expect("draft");
    let runtime = RuntimeOptions {
        project_root: Some(temp.path.clone()),
        data_dir: None,
        config: None,
        diagnostics: false,
    };

    run_article_promote(
        &runtime,
        ArticlePromoteArgs {
            path: draft_path.clone(),
            title: "Cheetah".to_string(),
            overwrite: false,
            format: OutputFormat::Json,
        },
    )
    .expect("promote draft");

    let target_path = temp
        .path
        .join("wiki_content")
        .join("Main")
        .join("Cheetah.wiki");
    assert_eq!(
        fs::read_to_string(&target_path).expect("target"),
        "'''Cheetah''' is a cat."
    );
    assert!(draft_path.exists(), "promotion preserves the draft source");
}

#[test]
fn article_promote_refuses_existing_target_without_overwrite() {
    let temp = TestDir::new("promote-existing");
    let paths = test_paths(&temp.path);
    let draft_path = paths.state_dir.join("drafts").join("Cheetah.wiki");
    let target_path = temp
        .path
        .join("wiki_content")
        .join("Main")
        .join("Cheetah.wiki");
    fs::create_dir_all(draft_path.parent().expect("draft parent")).expect("draft parent");
    fs::create_dir_all(target_path.parent().expect("target parent")).expect("target parent");
    fs::write(&draft_path, "draft").expect("draft");
    fs::write(&target_path, "existing").expect("target");
    let runtime = RuntimeOptions {
        project_root: Some(temp.path.clone()),
        data_dir: None,
        config: None,
        diagnostics: false,
    };

    let error = run_article_promote(
        &runtime,
        ArticlePromoteArgs {
            path: draft_path,
            title: "Cheetah".to_string(),
            overwrite: false,
            format: OutputFormat::Json,
        },
    )
    .expect_err("must refuse overwrite");

    assert!(error.to_string().contains("target already exists"));
    assert_eq!(
        fs::read_to_string(&target_path).expect("target"),
        "existing"
    );
}
