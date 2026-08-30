use std::fs;
use std::path::{Path, PathBuf};

const PUBLIC_SKILLS: [&str; 4] = ["prose-review", "wiki-interview", "wiki-writing", "wikitool"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve wikitool repo root")
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
    let root = repo_root().join(".agents/skills").join(name);
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
        "{name} frontmatter must contain name and description"
    );
    assert!(
        frontmatter.iter().all(|line| {
            line.starts_with("name:") || line.starts_with("description:") || line.trim().is_empty()
        }),
        "{name} frontmatter contains unsupported keys"
    );
    assert!(
        skill.contains(&format!("name: {name}")),
        "{name} frontmatter must match its directory"
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
fn public_skills_are_substantive_and_complete() {
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
    assert_skill_shape("wikitool", &[]);
}

#[test]
fn generic_skill_guidance_contains_no_remilia_policy() {
    let skills_root = repo_root().join(".agents/skills");
    for path in collect_files(&skills_root) {
        let extension = path.extension().and_then(|value| value.to_str());
        if !matches!(extension, Some("md" | "yaml" | "toml")) {
            continue;
        }
        let body = fs::read_to_string(&path).expect("read skill text");
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
                "generic skill guidance leaked target-specific token {forbidden:?} in {}",
                path.display()
            );
        }
    }
}

#[test]
fn agents_is_the_only_substantive_skill_authority() {
    let root = repo_root();
    assert!(
        !root.join("agent-pack").exists(),
        "retired agent-pack source must stay absent"
    );

    for skill in PUBLIC_SKILLS {
        let adapter_root = root.join(".claude/skills").join(skill);
        let adapter_files = collect_files(&adapter_root);
        assert_eq!(
            adapter_files,
            vec![adapter_root.join("SKILL.md")],
            "Claude adapter for {skill} must remain a single-file route"
        );
        let adapter = fs::read_to_string(&adapter_files[0]).expect("read Claude adapter");
        assert!(
            adapter.contains(&format!("../../../.agents/skills/{skill}/SKILL.md")),
            "Claude adapter for {skill} must route to the canonical Agent Skill"
        );
        assert!(
            adapter.lines().count() <= 10,
            "Claude adapter for {skill} must stay thin"
        );
    }
}
