use std::fs;
use std::path::PathBuf;

use wikitool_core::contracts::command_surface;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve wikitool repo root")
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative)).unwrap_or_else(|error| {
        panic!("failed to read {relative}: {error}");
    })
}

#[test]
fn packaged_guidance_stays_in_sync_with_current_authoring_front_door() {
    let claude = read_repo_file("ai-pack/CLAUDE.md");
    let agents = read_repo_file("ai-pack/AGENTS.md");

    assert_eq!(claude, agents, "ai-pack AGENTS.md must mirror CLAUDE.md");
    for body in [&claude, &agents] {
        assert!(
            body.contains("wikitool knowledge article-start"),
            "packaged guidance must mention article-start"
        );
        assert!(
            body.contains("wikitool --help") && body.contains("docs/wikitool/reference.md"),
            "packaged guidance must defer to CLI help/reference"
        );
        assert!(
            !body.contains("wiki.remilia.org/w/api.php"),
            "packaged guidance must not regress to the stale /w/api.php example"
        );
    }
}

#[test]
fn thin_wrappers_reference_help_and_keep_raw_pack_secondary() {
    let claude_skill = read_repo_file("ai-pack/.claude/skills/wikitool.md");
    let codex_skill = read_repo_file("ai-pack/codex_skills/wikitool-operator/SKILL.md");
    let local_skill = read_repo_file(".claude/skills/wikitool/SKILL.md");

    for body in [&claude_skill, &codex_skill, &local_skill] {
        assert!(
            body.contains("wikitool --help") && body.contains("docs/wikitool/reference.md"),
            "thin wrappers must defer to CLI help/reference"
        );
        assert!(
            body.contains("knowledge article-start"),
            "thin wrappers must point to article-start"
        );
        assert!(
            body.contains("knowledge pack"),
            "thin wrappers must still mention raw knowledge pack access"
        );
    }
}

#[test]
fn db_wrapper_hint_matches_current_public_surface() {
    let db_skill = read_repo_file(".claude/skills/wikitool/db.md");
    assert!(
        db_skill.contains("argument-hint: <stats|reset> [options]"),
        "db wrapper hint must match the live db surface"
    );
    assert!(
        !db_skill.contains("<stats|sync|migrate>"),
        "db wrapper hint must not mention removed commands"
    );
}

#[test]
fn frozen_command_surface_includes_release_research_and_dev_commands() {
    let surface = command_surface();
    for command in [
        "knowledge article-start",
        "research search",
        "research fetch",
        "release build-ai-pack",
        "release package",
        "release build-matrix",
        "dev install-git-hooks",
        "contracts snapshot",
        "contracts command-surface",
    ] {
        assert!(
            surface.iter().any(|item| item == command),
            "missing `{command}` from frozen command surface"
        );
    }
    assert!(
        !surface.iter().any(|item| item == "perf lighthouse"),
        "frozen command surface must not retain removed Lighthouse commands"
    );
}
