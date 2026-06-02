use super::draft::{
    DraftReviewSelection, review_draft_selection_from_args, validate_draft_review_path,
};
use super::next_steps::build_review_next_steps;
use super::*;
use crate::{briefs::BriefView, cli_support::OutputFormat};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use wikitool_core::runtime::{ResolvedPaths, ValueSource};

fn review_args() -> ReviewArgs {
    ReviewArgs {
        format: OutputFormat::Json,
        view: BriefView::Brief,
        strict: false,
        templates: false,
        categories: false,
        titles: Vec::new(),
        paths: Vec::new(),
        draft_paths: Vec::new(),
        brief_path: None,
        brief_stale_days: 45,
        titles_file: None,
        summary: "test".to_string(),
    }
}

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
            "wikitool-review-cli-{label}-{}-{unique}",
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
fn review_draft_selection_requires_exactly_one_title() {
    let mut args = review_args();
    args.draft_paths
        .push(PathBuf::from(".wikitool/drafts/Cheetah.wiki"));

    let error = review_draft_selection_from_args(&args).unwrap_err();

    assert!(error.to_string().contains("requires exactly one --title"));
}

#[test]
fn review_draft_selection_rejects_sync_path_mix() {
    let mut args = review_args();
    args.titles.push("Cheetah".to_string());
    args.draft_paths
        .push(PathBuf::from(".wikitool/drafts/Cheetah.wiki"));
    args.paths
        .push(PathBuf::from("wiki_content/Main/Cheetah.wiki"));

    let error = review_draft_selection_from_args(&args).unwrap_err();

    assert!(error.to_string().contains("cannot be combined"));
}

#[test]
fn review_draft_selection_accepts_one_draft_and_title() {
    let mut args = review_args();
    args.titles.push("Cheetah".to_string());
    args.draft_paths
        .push(PathBuf::from(".wikitool/drafts/Cheetah.wiki"));

    let selection = review_draft_selection_from_args(&args)
        .expect("draft selection")
        .expect("present");

    assert_eq!(selection.title, "Cheetah");
    assert_eq!(
        selection.path,
        PathBuf::from(".wikitool/drafts/Cheetah.wiki")
    );
}

#[test]
fn review_draft_path_requires_drafts_subdirectory() {
    let temp = TestDir::new("draft-subdir");
    let paths = test_paths(&temp.path);
    let non_draft_state_path = paths.state_dir.join("data").join("Cheetah.wiki");
    fs::write(&non_draft_state_path, "Text.").expect("state file");

    let error = validate_draft_review_path(&paths, &non_draft_state_path)
        .expect_err("must reject non-draft state path");

    assert!(error.to_string().contains("canonical draft directory"));
}

#[test]
fn review_next_steps_are_empty_for_sync_reviews() {
    let temp = TestDir::new("sync-next");
    let paths = test_paths(&temp.path);

    let steps = build_review_next_steps(&paths, None, "Summary", None).expect("next steps");

    assert!(steps.is_empty());
}

#[test]
fn review_next_steps_guide_draft_promotion_and_push_dry_run() {
    let temp = TestDir::new("draft-next");
    let paths = test_paths(&temp.path);
    let selection = DraftReviewSelection {
        title: "Cheetah".to_string(),
        path: PathBuf::from(".wikitool/drafts/Cheetah.wiki"),
    };

    let steps = build_review_next_steps(&paths, Some(&selection), "Draft review", None)
        .expect("next steps");

    assert_eq!(steps.len(), 6);
    assert_eq!(steps[0].kind, "lint_draft");
    assert_eq!(
        steps[0].command.as_ref().expect("lint command").argv,
        vec![
            "wikitool",
            "article",
            "lint",
            ".wikitool/drafts/Cheetah.wiki",
            "--title",
            "Cheetah",
            "--format",
            "json"
        ]
    );
    let promote = steps
        .iter()
        .find(|step| step.kind == "promote_draft")
        .expect("promote step");
    assert_eq!(
        promote.target_path.as_deref(),
        Some("wiki_content/Main/Cheetah.wiki")
    );
    assert_eq!(
        promote.command.as_ref().expect("promote command").argv,
        vec![
            "wikitool",
            "article",
            "promote",
            ".wikitool/drafts/Cheetah.wiki",
            "--title",
            "Cheetah",
            "--format",
            "json"
        ]
    );
    let push = steps
        .iter()
        .find(|step| step.kind == "push_dry_run")
        .and_then(|step| step.command.as_ref())
        .expect("push command");
    assert_eq!(
        push.argv,
        vec![
            "wikitool",
            "push",
            "--dry-run",
            "--path",
            "wiki_content/Main/Cheetah.wiki",
            "--summary",
            "Draft review",
            "--format",
            "json"
        ]
    );
}

#[test]
fn review_next_steps_preserve_interview_brief_path() {
    let temp = TestDir::new("draft-brief-next");
    let paths = test_paths(&temp.path);
    let selection = DraftReviewSelection {
        title: "Cheetah".to_string(),
        path: PathBuf::from(".wikitool/drafts/Cheetah.wiki"),
    };
    let brief_path = ".wikitool/interviews/Cheetah/20260601T172430Z.brief.md";

    let steps = build_review_next_steps(&paths, Some(&selection), "Draft review", Some(brief_path))
        .expect("next steps");

    let review_draft = steps
        .iter()
        .find(|step| step.kind == "review_draft")
        .and_then(|step| step.command.as_ref())
        .expect("review draft command");
    assert!(
        review_draft
            .argv
            .windows(2)
            .any(|pair| pair == ["--brief-path", brief_path])
    );

    let review_promoted = steps
        .iter()
        .find(|step| step.kind == "review_promoted_page")
        .and_then(|step| step.command.as_ref())
        .expect("review promoted command");
    assert!(
        review_promoted
            .argv
            .windows(2)
            .any(|pair| pair == ["--brief-path", brief_path])
    );
}
