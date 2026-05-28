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

    assert_eq!(claude, agents, "shipped AGENTS.md must mirror CLAUDE.md");
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
            body.contains("same guidance body"),
            "packaged guidance must explain that both shipped filenames carry the same instructions"
        );
        assert!(
            body.contains("## Token Discipline")
                && body.contains("Agent-facing defaults are intentionally compact")
                && body.contains("--view brief")
                && body.contains("--view full"),
            "packaged guidance must preserve the compact-default/token-discipline contract"
        );
        assert!(
            body.contains("## Session Start")
                && body.contains("wikitool diff --format json")
                && body.contains("wikitool workflow session-refresh")
                && body.contains("Do not use `pull --overwrite-local`"),
            "packaged guidance must define the normal session refresh sequence"
        );
        assert!(
            !body.contains("Docs bootstrap")
                && !body.contains("WIKITOOL_CLAUDE.md")
                && !body.contains("llm_instructions")
                && !body.contains("wikitool search")
                && !body.contains("wikitool fetch")
                && !body.contains("wikitool context")
                && !body.contains("wikitool seo")
                && !body.contains("wikitool net")
                && !body.contains("agent-card")
                && !body.contains("function-card")
                && !body.contains("function-context"),
            "packaged guidance must not refer to removed setup/backcompat artifacts"
        );
        assert!(
            body.contains("those files become the packaged writing context"),
            "packaged guidance must document host writing context overlay behavior"
        );
    }
}

#[test]
fn ai_pack_readme_keeps_shipping_and_scratch_boundaries_explicit() {
    let readme = read_repo_file("ai-pack/README.md");
    assert!(
        readme.contains("writing_context/")
            && readme.contains("Do not put local experiments")
            && readme.contains("Host `writing_context/` replaces")
            && !readme.contains("llm_instructions"),
        "ai-pack README must keep packaging, writing context, and scratch-space boundaries explicit"
    );
}

#[test]
fn thin_wrappers_reference_help_and_keep_article_start_primary() {
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
            body.contains("diff --format json")
                && body.contains("workflow session-refresh")
                && body.contains("knowledge status"),
            "thin wrappers must tell agents to inspect local changes and refresh wiki state at session start"
        );
        assert!(
            !body.contains("knowledge pack"),
            "thin wrappers must not refer to the retired raw pack command"
        );
        assert!(
            body.contains("Keep agent context compact") && body.contains("--view brief"),
            "thin wrappers must preserve compact-first agent retrieval guidance"
        );
    }
    for body in [&claude_skill, &codex_skill] {
        assert!(
            body.contains("--intent new|expand|audit|refresh")
                && body.contains("knowledge contracts")
                && body.contains("--verify-live"),
            "packaged operator wrappers must stay aligned on current authoring and validation surfaces"
        );
    }
}

#[test]
fn packaged_review_wrappers_stay_aligned_on_gate_sequence() {
    for path in [
        "ai-pack/.claude/skills/review.md",
        "ai-pack/codex_skills/wikitool-content-gate/SKILL.md",
    ] {
        let body = read_repo_file(path);
        assert!(
            body.contains("Preferred gate brief: `wikitool review --format json --view brief --summary \"...\"`")
                && body.contains("Draft-first gate: `wikitool review --draft-path")
                && body.contains("--view brief")
                && body.contains("Direct draft iteration:")
                && body.contains("wikitool article promote")
                && body.contains("next_steps")
                && body.contains("wikitool validate --summary")
                && body.contains("--verify-live"),
            "{path} must stay aligned with the current content gate sequence"
        );
    }
}

#[test]
fn packaged_extension_guidance_scopes_d3charts_to_local_contract() {
    let extensions = read_repo_file("ai-pack/writing_context/extensions.md");
    assert!(
        extensions.contains("D3Charts (Remilia local contract)")
            && extensions.contains("Module:D3Chart")
            && extensions.contains("Do not add raw `<script>` tags")
            && extensions.contains("bespoke extension"),
        "extension guidance must scope D3Charts as the current local contract, not universal MediaWiki syntax"
    );
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
fn writing_context_does_not_reintroduce_retired_top_level_commands() {
    for path in markdown_files_under("ai-pack/writing_context") {
        let body = read_repo_file(&path);
        for retired in [
            "wikitool context",
            "wikitool search",
            "wikitool fetch",
            "wikitool seo",
            "wikitool net",
            "agent-card",
            "function-card",
            "function-context",
        ] {
            assert!(
                !body.contains(retired),
                "{path} must not mention retired top-level command `{retired}`"
            );
        }
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
