use std::fs;
use std::path::PathBuf;

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

fn markdown_files_under(relative_dir: &str) -> Vec<String> {
    let root = repo_root();
    let mut files = fs::read_dir(root.join(relative_dir))
        .unwrap_or_else(|error| panic!("failed to read directory {relative_dir}: {error}"))
        .map(|entry| {
            let entry = entry
                .unwrap_or_else(|error| panic!("failed to read entry in {relative_dir}: {error}"));
            entry
                .path()
                .strip_prefix(&root)
                .expect("strip repo root")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .filter(|path| path.ends_with(".md"))
        .collect::<Vec<_>>();
    files.sort();
    files
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
            body.contains("Use normal reasoning"),
            "packaged guidance must keep the normal-reasoning boundary explicit"
        );
        assert!(
            body.contains("wikitool --help") && body.contains("docs/wikitool/reference.md"),
            "packaged guidance must defer to CLI help/reference"
        );
        assert!(
            !body.contains("wiki.remilia.org/w/api.php"),
            "packaged guidance must not regress to the stale /w/api.php example"
        );
        assert!(
            body.contains("AGENTS.md") && body.contains("byte-identical"),
            "packaged guidance must keep the host overlay instruction contract explicit"
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
            body.contains("Use normal reasoning"),
            "thin wrappers must preserve the normal-reasoning boundary"
        );
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
fn packaged_skill_wrappers_stay_thin_and_do_not_reintroduce_removed_surfaces() {
    for path in markdown_files_under("ai-pack/.claude/skills") {
        let body = read_repo_file(&path);
        assert!(
            body.contains("Thin wrapper"),
            "{path} must stay a thin wrapper"
        );
        assert!(
            body.contains("docs/wikitool/reference.md"),
            "{path} must defer to the generated CLI reference"
        );
        assert!(
            !body.contains("wikitool perf") && !body.contains("perf lighthouse"),
            "{path} must not mention removed perf surfaces"
        );
    }
}

#[test]
fn local_wikitool_command_wrappers_remain_reference_backed() {
    for path in markdown_files_under(".claude/skills/wikitool") {
        let body = read_repo_file(&path);
        assert!(
            !body.contains("wikitool perf") && !body.contains("perf lighthouse"),
            "{path} must not mention removed perf surfaces"
        );
        if path.ends_with("/SKILL.md") {
            continue;
        }
        assert!(
            body.contains("docs/wikitool/reference.md"),
            "{path} must defer to the generated CLI reference"
        );
    }
}

#[test]
fn codex_skill_wrappers_remain_help_backed_and_perf_free() {
    for path in [
        "ai-pack/codex_skills/wikitool-operator/SKILL.md",
        "ai-pack/codex_skills/wikitool-content-gate/SKILL.md",
    ] {
        let body = read_repo_file(path);
        assert!(
            body.contains("wikitool --help") && body.contains("docs/wikitool/reference.md"),
            "{path} must defer to CLI help/reference"
        );
        assert!(
            !body.contains("wikitool perf") && !body.contains("perf lighthouse"),
            "{path} must not mention removed perf surfaces"
        );
    }
}
