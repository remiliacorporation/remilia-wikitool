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

fn read_host_repo_file(relative: &str) -> Option<String> {
    let wikitool_root = repo_root();
    let host_root = wikitool_root.join("../..");
    let nested_wikitool = host_root.join("tools/wikitool").canonicalize().ok()?;
    if nested_wikitool != wikitool_root {
        return None;
    }

    Some(
        fs::read_to_string(host_root.join(relative)).unwrap_or_else(|error| {
            panic!("failed to read host repo file {relative}: {error}");
        }),
    )
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
            body.contains("knowledge-interview")
                && body.contains(".wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md")
                && body.contains("wikitool knowledge interview init")
                && body.contains("knowledge interview open-item")
                && body.contains("knowledge interview validate")
                && body.contains("knowledge article-start --brief-path")
                && body.contains("review --brief-path")
                && body.contains("intent, scope, and")
                && body.contains("well-documented")
                && body.contains("normal move")
                && body.contains("user assertions are research leads")
                && body.contains("opt-outs"),
            "packaged guidance must route human-in-loop article work through the interview faculty"
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
            body.contains("knowledge-interview")
                || body.contains("/knowledge-interview")
                || body.contains("knowledge-interview guidance"),
            "thin wrappers must route substantial authoring to the interview faculty"
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
fn knowledge_interview_skill_and_playbook_are_packaged() {
    let claude_skill = read_repo_file("ai-pack/.claude/skills/knowledge-interview.md");
    let codex_skill = read_repo_file("ai-pack/codex_skills/wikitool-knowledge-interview/SKILL.md");
    let playbook = read_repo_file("ai-pack/writing_context/interview_playbook.md");
    let codex_readme = read_repo_file("ai-pack/codex_skills/README.md");
    let writing_readme = read_repo_file("ai-pack/writing_context/README.md");

    for body in [&claude_skill, &codex_skill] {
        assert!(
            body.contains("Thin wrapper")
                && body.contains("wikitool --help")
                && body.contains("docs/wikitool/reference.md")
                && body.contains("writing_context/interview_playbook.md")
                && body.contains("wikitool knowledge interview init")
                && body.contains("open-item")
                && body.contains("knowledge interview validate")
                && body.contains("article-start")
                && body.contains("--brief-path")
                && body.contains("intent, scope, and angle")
                && body.contains("well-documented subjects")
                && body.contains("normal move")
                && body.contains(".wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md"),
            "knowledge interview wrappers must stay thin, help-backed, and ledger-aware"
        );
    }

    assert!(
        playbook.contains("Scout first")
            && playbook.contains("freeform dump")
            && playbook.contains("Read supplied materials")
            && playbook.contains("what the person actually wants written")
            && playbook.contains("well-documented subjects")
            && playbook.contains("This is framing, not a forced")
            && playbook.contains("no fixed number of rounds")
            && playbook.contains("interviewer/critic loop")
            && playbook.contains("explicit opt-out")
            && playbook.contains("mechanical link checks")
            && playbook.contains("wikitool knowledge interview init")
            && playbook.contains("knowledge interview open-item add")
            && playbook.contains("rejected-source")
            && playbook.contains("inaccessible-source")
            && playbook.contains("knowledge interview audit")
            && playbook.contains("not article prose, citation evidence, proof")
            && playbook.contains("Mechanical validation does not imply editorial"),
        "interview playbook must preserve the adaptive, evidence-bounded intake contract"
    );
    assert!(
        !playbook.contains(
            "Reach for it when your specific knowledge reaches further than public sources do"
        ),
        "interview playbook must not narrow usage to public-source gaps"
    );
    assert!(
        !playbook.contains("There is not yet a Rust `knowledge interview` command")
            && !claude_skill.contains("Do not invent a `knowledge interview` CLI command")
            && !codex_skill.contains("Do not invent a `knowledge interview` CLI command"),
        "guidance must not retain pre-CLI interview wording"
    );
    assert!(
        codex_readme.contains("wikitool-knowledge-interview")
            && writing_readme.contains("interview_playbook.md"),
        "bundle indexes must expose the interview skill and playbook"
    );

    let changelog = read_repo_file("CHANGELOG.md");
    assert!(
        changelog.contains("normal move after the article-start scout")
            && changelog.contains("Its purpose is direction")
            && changelog.contains("well-documented subject")
            && !changelog.contains("skip it when they do not"),
        "0.4.0 release notes must describe interview as direction-first, not public-source-gap-only"
    );
}

#[test]
fn host_repo_routes_knowledge_interview_for_claude_and_codex() {
    let packaged_claude = read_repo_file("ai-pack/.claude/skills/knowledge-interview.md");
    let packaged_codex =
        read_repo_file("ai-pack/codex_skills/wikitool-knowledge-interview/SKILL.md");

    assert!(
        packaged_claude.contains("# /knowledge-interview - Thin wrapper")
            && packaged_codex.contains("name: wikitool-knowledge-interview")
            && packaged_codex.contains("writing_context/interview_playbook.md"),
        "packaged Claude and Codex interview skills must both be present"
    );

    let Some(host_claude) = read_host_repo_file("CLAUDE.md") else {
        return;
    };
    let host_stub = read_host_repo_file(".claude/skills/knowledge-interview.md")
        .expect("host repo already detected");

    assert!(
        host_claude.contains("| `/knowledge-interview` | `wikitool-knowledge-interview` |")
            && host_claude.contains("interview_playbook.md"),
        "host CLAUDE.md must route the Claude skill and name the Codex equivalent"
    );
    assert!(
        host_stub.contains("tools/wikitool/ai-pack/.claude/skills/knowledge-interview.md")
            && host_stub.contains("Frontmatter (permissions) is repo-level"),
        "repo-root Claude skill must be a redirect stub to the canonical ai-pack skill"
    );
}

#[test]
fn generated_reference_documents_knowledge_interview_commands() {
    let reference = read_repo_file("docs/wikitool/reference.md");
    for heading in [
        "## knowledge interview init",
        "## knowledge interview validate",
        "## knowledge interview show",
        "## knowledge interview audit",
        "## knowledge interview open-item",
        "## knowledge interview open-item add",
        "## knowledge interview open-item list",
        "## knowledge interview open-item update",
    ] {
        assert!(
            reference.contains(heading),
            "generated reference must document `{heading}`"
        );
    }
    assert!(
        reference.contains("--brief-path <PATH>")
            && reference.contains("Optional knowledge interview brief")
            && reference.contains("Validate and include a knowledge interview brief"),
        "generated reference must document article-start/review brief-path integration"
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
fn visual_subjects_guidance_is_present_and_indexed() {
    let visual = read_repo_file("ai-pack/writing_context/visual_subjects.md");
    assert!(
        visual.contains("primary source")
            && visual.contains("describe / interpret boundary")
            && visual.contains("do_not_assert"),
        "visual_subjects.md must define the artifact-as-source and describe/interpret rules"
    );
    for index in [
        "ai-pack/CLAUDE.md",
        "ai-pack/AGENTS.md",
        "ai-pack/writing_context/README.md",
        "ai-pack/writing_context/writing_guide.md",
        "ai-pack/writing_context/article_structure.md",
    ] {
        let body = read_repo_file(index);
        assert!(
            body.contains("visual_subjects.md"),
            "{index} must reference visual_subjects.md so agents discover it"
        );
    }
    if let Some(host_claude) = read_host_repo_file("CLAUDE.md") {
        let host_agents = read_host_repo_file("AGENTS.md").expect("host repo already detected");
        assert!(
            host_claude.contains("visual_subjects.md")
                && host_agents.contains("visual_subjects.md"),
            "host CLAUDE.md and AGENTS.md writing-guidelines tables must list visual_subjects.md"
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
        "ai-pack/codex_skills/wikitool-knowledge-interview/SKILL.md",
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

#[test]
fn article_quality_guidance_uses_review_state_semantics() {
    let writing_guide = read_repo_file("ai-pack/writing_context/writing_guide.md");
    let article_structure = read_repo_file("ai-pack/writing_context/article_structure.md");
    let host_template_guidance =
        read_host_repo_file("templates/message/Module_Message.lua").map(|message_module| {
            let template_docs =
                read_host_repo_file("templates/message/Template_Article_quality.wiki")
                    .expect("host repo already detected");
            (message_module, template_docs)
        });
    let structure_rule = read_repo_file("crates/wikitool_core/src/article_lint/rules/structure.rs");

    for body in [&writing_guide, &article_structure] {
        assert!(
            !body.contains("Risk of hallucination")
                && !body.contains("generated by AI")
                && !body.contains("AI-generated article"),
            "article quality guidance must not describe review states as AI authorship labels"
        );
    }
    if let Some((message_module, template_docs)) = &host_template_guidance {
        for body in [message_module, template_docs] {
            assert!(
                !body.contains("Risk of hallucination")
                    && !body.contains("generated by AI")
                    && !body.contains("AI-generated article"),
                "article quality guidance must not describe review states as AI authorship labels"
            );
        }
        assert!(
            message_module.contains("reviewed for factual accuracy")
                && template_docs.contains("Hidden marker"),
            "host article quality templates must describe editorial review states"
        );
    }
    assert!(
        writing_guide.contains("Preserve an existing `wip` or `verified` state")
            && article_structure.contains("Preserve existing `wip` or `verified`"),
        "packaged article quality guidance must describe editorial review states"
    );
    assert!(
        !structure_rule.contains("structure.article_quality_state")
            && structure_rule.contains("structure.require_article_quality_banner")
            && structure_rule.contains("{{Article quality|unverified}}"),
        "article lint must require the banner without normalizing intentional review states"
    );
}
