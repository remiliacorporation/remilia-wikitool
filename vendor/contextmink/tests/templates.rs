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

#[test]
fn launcher_template_matches_repo_launcher() {
    let repo_launcher = include_str!("../scripts/contextmink");
    let template_launcher = include_str!("../templates/scripts/contextmink");

    assert_eq!(
        repo_launcher, template_launcher,
        "the installed launcher template must match scripts/contextmink"
    );
}

#[test]
fn launcher_finds_cargo_outside_non_login_path() {
    let launcher = include_str!("../templates/scripts/contextmink");

    assert!(launcher.contains("find_cargo()"));
    assert!(launcher.contains("\"$home_dir/.cargo/bin/cargo\""));
    assert!(launcher.contains("\"$home_dir/.cargo/bin/cargo.exe\""));
    assert!(launcher.contains("bash -lc 'command -v cargo'"));
    assert!(launcher.contains("cargo_bin=\"$(find_cargo || true)\""));
    assert!(launcher.contains("\"$cargo_bin\" build --quiet --release"));
}
