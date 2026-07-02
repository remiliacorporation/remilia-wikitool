#[test]
fn instruction_templates_are_policy_equivalent() {
    let codex = include_str!("../templates/AGENTS.contextmink.md");
    let claude = include_str!("../templates/CLAUDE.contextmink.md");

    assert_eq!(
        codex, claude,
        "Codex and Claude contextmink guidance must stay equivalent"
    );
}

#[test]
fn setup_points_to_templates_instead_of_duplicating_policy() {
    let setup = include_str!("../docs/setup.md");

    assert!(setup.contains("templates/AGENTS.contextmink.md"));
    assert!(setup.contains("templates/CLAUDE.contextmink.md"));
    assert!(
        !setup.contains("Do not route everything through `contextmink`."),
        "setup.md should point to templates instead of duplicating snippet prose"
    );
}
