use anyhow::Result;
use wikitool_core::filesystem::title_to_relative_path;
use wikitool_core::runtime::ResolvedPaths;

use crate::cli_support::normalize_path;

use super::draft::DraftReviewSelection;
use super::{ReviewNextStep, ReviewNextStepCommand};

pub(super) fn build_review_next_steps(
    paths: &ResolvedPaths,
    draft_selection: Option<&DraftReviewSelection>,
    summary: &str,
    brief_path: Option<&str>,
) -> Result<Vec<ReviewNextStep>> {
    let Some(draft_selection) = draft_selection else {
        return Ok(Vec::new());
    };

    let draft_path = normalize_path(&draft_selection.path);
    let title = draft_selection.title.clone();
    let promote_path = title_to_relative_path(paths, &title, false)?;
    let mut review_draft_argv = vec![
        "wikitool",
        "review",
        "--draft-path",
        &draft_path,
        "--title",
        &title,
    ];
    if let Some(brief_path) = brief_path {
        review_draft_argv.extend(["--brief-path", brief_path]);
    }
    review_draft_argv.extend(["--format", "json", "--summary", summary]);

    let mut review_promoted_argv = vec!["wikitool", "review", "--path", &promote_path];
    if let Some(brief_path) = brief_path {
        review_promoted_argv.extend(["--brief-path", brief_path]);
    }
    review_promoted_argv.extend(["--format", "json", "--summary", summary]);

    Ok(vec![
        command_next_step(
            "lint_draft",
            "Lint the draft directly with the same title override.",
            vec![
                "wikitool",
                "article",
                "lint",
                &draft_path,
                "--title",
                &title,
                "--format",
                "json",
            ],
        ),
        command_next_step(
            "fix_draft",
            "Apply safe mechanical fixes to the draft before reviewing again.",
            vec![
                "wikitool",
                "article",
                "fix",
                &draft_path,
                "--title",
                &title,
                "--apply",
                "safe",
            ],
        ),
        command_next_step(
            "review_draft",
            "Rerun the draft review gate after edits or safe fixes.",
            review_draft_argv,
        ),
        ReviewNextStep {
            kind: "promote_draft",
            description: "Copy the accepted draft to the sync path before push review.".to_string(),
            command: Some(review_next_step_command(vec![
                "wikitool",
                "article",
                "promote",
                &draft_path,
                "--title",
                &title,
                "--format",
                "json",
            ])),
            target_path: Some(promote_path.clone()),
        },
        command_next_step(
            "review_promoted_page",
            "Run the normal pre-push gate after the draft is under wiki_content/.",
            review_promoted_argv,
        ),
        command_next_step(
            "push_dry_run",
            "Preview the scoped push only after the promoted review is clean.",
            vec![
                "wikitool",
                "push",
                "--dry-run",
                "--path",
                &promote_path,
                "--summary",
                summary,
                "--format",
                "json",
            ],
        ),
    ])
}

fn command_next_step(kind: &'static str, description: &str, argv: Vec<&str>) -> ReviewNextStep {
    ReviewNextStep {
        kind,
        description: description.to_string(),
        command: Some(review_next_step_command(argv)),
        target_path: None,
    }
}

fn review_next_step_command(argv: Vec<&str>) -> ReviewNextStepCommand {
    let argv = argv
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();
    ReviewNextStepCommand {
        display: display_command(&argv),
        argv,
    }
}

fn display_command(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| display_command_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_command_arg(arg: &str) -> String {
    if arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '\\' | ':'))
    {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
}
