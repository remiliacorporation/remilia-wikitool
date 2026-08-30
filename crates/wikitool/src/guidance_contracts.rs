use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve wikitool repo root")
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

fn collect_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn assert_skill_shape(name: &str, required_references: &[&str]) {
    let root = repo_root().join("agent-pack/skills").join(name);
    let skill = fs::read_to_string(root.join("SKILL.md")).expect("read skill");
    let lines = skill.lines().collect::<Vec<_>>();
    assert_eq!(lines.first(), Some(&"---"), "{name} needs frontmatter");
    let closing = lines
        .iter()
        .skip(1)
        .position(|line| *line == "---")
        .map(|index| index + 1)
        .expect("closing frontmatter");
    let frontmatter = &lines[1..closing];
    assert_eq!(
        frontmatter
            .iter()
            .filter(|line| line.starts_with("name:") || line.starts_with("description:"))
            .count(),
        2,
        "{name} frontmatter must contain only name and description"
    );
    assert!(
        frontmatter.iter().all(|line| {
            line.starts_with("name:") || line.starts_with("description:") || line.trim().is_empty()
        }),
        "{name} frontmatter contains unsupported keys"
    );
    assert!(
        skill.contains("## Procedure") && skill.contains("## Exit conditions"),
        "{name} must be a substantive procedure with exit conditions"
    );
    assert!(
        root.join("agents/openai.yaml").is_file(),
        "{name} must include agents/openai.yaml"
    );
    for reference in required_references {
        assert!(
            root.join("references").join(reference).is_file(),
            "{name} is missing routed reference {reference}"
        );
        assert!(
            skill.contains(reference),
            "{name} must route {reference} from SKILL.md"
        );
    }
}

#[test]
fn public_editorial_skills_are_substantive_and_complete() {
    assert_skill_shape(
        "wiki-writing",
        &[
            "evidence-to-prose.md",
            "human-notes.md",
            "mediawiki-structure.md",
        ],
    );
    assert_skill_shape(
        "prose-review",
        &["source-fidelity.md", "reader-value.md", "blp-sensitive.md"],
    );
    assert_skill_shape("wiki-interview", &["interview-ledger.md"]);
    assert_skill_shape("wikitool-operator", &[]);
}

#[test]
fn generic_agent_guidance_contains_no_remilia_policy() {
    let agent_pack = repo_root().join("agent-pack");
    for path in collect_files(&agent_pack.join("skills")) {
        let extension = path.extension().and_then(|value| value.to_str());
        if !matches!(extension, Some("md" | "yaml" | "toml")) {
            continue;
        }
        let body = fs::read_to_string(&path).expect("read agent pack text");
        let lowered = body.to_ascii_lowercase();
        for forbidden in [
            "remilia",
            "charlotte fang",
            "milady maker",
            "wiki.remilia.org",
            "d3chart",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "generic agent guidance leaked target-specific token {forbidden:?} in {}",
                path.display()
            );
        }
    }
    assert!(
        !agent_pack.join("writing_context").exists(),
        "retired binary-adjacent writing_context must stay removed"
    );
}

#[test]
fn agent_pack_has_one_harness_neutral_skill_authority() {
    let root = repo_root().join("agent-pack");
    for retired in ["AGENTS.md", "CLAUDE.md", ".claude", "codex_skills"] {
        assert!(
            !root.join(retired).exists(),
            "retired parallel agent-pack surface must stay absent: {retired}"
        );
    }

    let source_root = read_repo_file(".claude/skills/wikitool/SKILL.md");
    assert!(source_root.contains("agent-pack/skills/wikitool-operator/SKILL.md"));
    assert!(
        source_root.lines().count() <= 16,
        "source-root Claude entrypoint must stay a thin canonical route"
    );
    let source_root_dir = repo_root().join(".claude/skills/wikitool");
    let source_root_files = collect_files(&source_root_dir);
    assert_eq!(
        source_root_files,
        vec![source_root_dir.join("SKILL.md")],
        "source-root skill must not retain a parallel legacy command tree"
    );
}
