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

fn host_root() -> Option<PathBuf> {
    let wikitool_root = repo_root();
    let candidate = wikitool_root.join("../..").canonicalize().ok()?;
    let nested = candidate.join("tools/wikitool").canonicalize().ok()?;
    (nested == wikitool_root).then_some(candidate)
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
    let root = repo_root().join("ai-pack/codex_skills").join(name);
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
fn generic_ai_pack_contains_no_remilia_policy() {
    let ai_pack = repo_root().join("ai-pack");
    for path in collect_files(&ai_pack) {
        let extension = path.extension().and_then(|value| value.to_str());
        if !matches!(extension, Some("md" | "yaml" | "toml")) {
            continue;
        }
        let body = fs::read_to_string(&path).expect("read AI pack text");
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
                "generic AI pack leaked target-specific token {forbidden:?} in {}",
                path.display()
            );
        }
    }
    assert!(
        !ai_pack.join("writing_context").exists(),
        "retired binary-adjacent writing_context must stay removed"
    );
}

#[test]
fn claude_entrypoints_route_to_canonical_skills() {
    for (wrapper, canonical) in [
        ("wikitool.md", "codex_skills/wikitool-operator/SKILL.md"),
        ("wiki-writing.md", "codex_skills/wiki-writing/SKILL.md"),
        ("prose-review.md", "codex_skills/prose-review/SKILL.md"),
        ("wiki-interview.md", "codex_skills/wiki-interview/SKILL.md"),
    ] {
        let body = read_repo_file(&format!("ai-pack/.claude/skills/{wrapper}"));
        assert!(
            body.contains(canonical),
            "Claude wrapper {wrapper} must route to {canonical}"
        );
    }
}

#[test]
fn authoring_and_review_boundaries_are_explicit() {
    let author = read_repo_file("ai-pack/codex_skills/wiki-writing/SKILL.md");
    let review = read_repo_file("ai-pack/codex_skills/prose-review/SKILL.md");
    let interview = read_repo_file("ai-pack/codex_skills/wiki-interview/SKILL.md");
    let operator = read_repo_file("ai-pack/codex_skills/wikitool-operator/SKILL.md");

    assert!(author.contains("claim-source map") && author.contains("no inspected source document"));
    assert!(
        author.contains("Use the `prose-review` skill") && author.contains("exact final prose")
    );
    assert!(review.contains("Would someone") && review.contains("findings first"));
    assert!(review.contains("## Independence") && review.contains("P1 — block"));
    assert!(review.contains("must not create an acceptance ledger entry"));
    assert!(interview.contains("Do not read a canned questionnaire"));
    assert!(
        interview.contains("neutral ledger") && interview.contains("not automatic publication")
    );
    assert!(operator.contains("self-reported, unauthenticated claim"));
}

#[test]
fn acceptance_code_describes_a_ledger_not_identity_proof() {
    let acceptance = read_repo_file("crates/wikitool_core/src/article_acceptance.rs");
    assert!(acceptance.contains("article_acceptance_ledger_v1"));
    assert!(acceptance.contains("self_reported_unverified"));
    assert!(acceptance.contains("accepted_for_main_namespace_promotion"));
    assert!(!acceptance.contains("EDITORIAL_QUALITY_ATTESTATION"));
    assert!(!acceptance.contains("human_judged_article_specific"));
}

#[test]
fn article_start_has_no_embedded_editorial_prompt_contract() {
    let model = read_repo_file("crates/wikitool_core/src/authoring/model.rs");
    let builder = read_repo_file("crates/wikitool_core/src/authoring/article_start.rs");
    for forbidden in [
        "ArticleAuthoringContract",
        "RecommendedAction",
        "suggested_question",
        "next_actions",
        "synthetic_phrase_prompts",
        "discouraged_relationship",
    ] {
        assert!(!model.contains(forbidden), "model leaked {forbidden}");
        assert!(!builder.contains(forbidden), "builder leaked {forbidden}");
    }
}

#[test]
fn site_adapter_is_explicit_and_host_owned() {
    let integration = read_repo_file("ai-pack/integration/site_adapters.md");
    assert!(integration.contains("mediawiki-generic"));
    assert!(integration.contains("Unknown fields are rejected"));
    assert!(integration.contains("routing signals, not universal bans"));

    let Some(host) = host_root() else {
        return;
    };
    let profile = fs::read_to_string(host.join("wikitool_adapter/profile.toml"))
        .expect("host must own an explicit site adapter");
    assert!(profile.contains("profile_id = \"remilia-wiki\""));
    assert!(profile.contains("host = \"wikipedia.org\""));
    assert!(host.join("wikitool_adapter/editorial.md").is_file());
    assert!(host.join("wikitool_adapter/extensions.md").is_file());
}

#[test]
fn generated_reference_documents_interview_and_acceptance_commands() {
    let reference = read_repo_file("docs/wikitool/reference.md");
    for heading in [
        "## article accept",
        "## article promote",
        "## knowledge interview init",
        "## knowledge interview validate",
        "## knowledge interview open-item",
    ] {
        assert!(
            reference.contains(heading),
            "missing generated heading {heading}"
        );
    }
}

#[test]
fn prose_review_eval_inputs_and_expectations_are_isolated_and_aligned() {
    let cases: serde_json::Value =
        serde_json::from_str(&read_repo_file("testbench/prose_review_cases.json"))
            .expect("parse prose review cases");
    let expectations: serde_json::Value =
        serde_json::from_str(&read_repo_file("testbench/prose_review_expectations.json"))
            .expect("parse prose review expectations");

    let case_entries = cases["cases"].as_array().expect("case array");
    let expectation_entries = expectations["expectations"]
        .as_array()
        .expect("expectation array");
    let case_ids = case_entries
        .iter()
        .map(|entry| entry["id"].as_str().expect("case id"))
        .collect::<Vec<_>>();
    let expectation_ids = expectation_entries
        .iter()
        .map(|entry| entry["id"].as_str().expect("expectation id"))
        .collect::<Vec<_>>();

    assert_eq!(case_ids, expectation_ids, "review fixture ids must align");
    assert!(
        case_ids.len() >= 3,
        "review eval needs positive and negative controls"
    );
    for case in case_entries {
        assert!(case.get("required_findings").is_none());
        assert!(case.get("forbidden_findings").is_none());
        assert!(case["article_wikitext"].as_str().is_some());
        assert!(
            case["inspected_sources"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        );
    }
}
