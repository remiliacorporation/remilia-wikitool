use super::{HookVerdict, evaluate_hook_payload};
use crate::config::DestructiveGuardConfig;

const FIELD: &str = "tool_input.command";

fn workspace_config() -> DestructiveGuardConfig {
    DestructiveGuardConfig {
        recursive_delete_fragments: vec!["re_store".to_owned(), "decompilation_outputs".to_owned()],
        delete_fragments: vec!["minkwrath.sqlite".to_owned(), "wow_ghidra.gpr".to_owned()],
    }
}

fn payload(command: &str) -> String {
    serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command }
    })
    .to_string()
}

fn verdict(command: &str) -> HookVerdict {
    evaluate_hook_payload(&payload(command), FIELD, &workspace_config(), false)
}

fn assert_denied(command: &str, expect_in_message: &str) {
    match verdict(command) {
        HookVerdict::Deny { message } => assert!(
            message.contains(expect_in_message),
            "deny message for {command:?} missing {expect_in_message:?}: {message}"
        ),
        other => panic!("expected deny for {command:?}, got {other:?}"),
    }
}

fn assert_allowed(command: &str) {
    match verdict(command) {
        HookVerdict::Allow => {}
        other => panic!("expected allow for {command:?}, got {other:?}"),
    }
}

#[test]
fn git_clean_spellings_are_denied() {
    assert_denied("git clean -fdX", "git clean");
    assert_denied("cd minkwrath && git clean -fd", "git clean");
    assert_denied("git -C ghidramink clean -f", "git clean");
    assert_denied("git.exe clean -n", "git clean");
    assert_denied(
        "bash -lc 'cd /f/AI/wow_modernclient && git clean -fdX'",
        "git clean",
    );
}

#[test]
fn recursive_deletion_of_protected_fragments_is_denied() {
    assert_denied("rm -rf re_store", "protected path fragment");
    assert_denied(
        "Remove-Item -Recurse -Force decompilation_outputs/exports",
        "protected path fragment",
    );
}

#[test]
fn direct_deletion_of_protected_files_is_denied() {
    assert_denied("rm -f re_store/minkwrath.sqlite", "protected path fragment");
    assert_denied("del wow_ghidra.gpr", "protected path fragment");
}

#[test]
fn benign_commands_are_allowed() {
    assert_allowed("git status --short");
    assert_allowed("cargo test -p minkwrath-render");
    assert_allowed("rm -f scratch_probe.rs");
    // Reads and backups of protected artifacts must never be blocked.
    assert_allowed("scripts/contextmink sqlite --path re_store/minkwrath.sqlite --sql-file q.sql");
    assert_allowed("cp re_store/minkwrath.sqlite /e/backups/minkwrath.sqlite");
}

#[test]
fn empty_command_is_allowed() {
    assert_allowed("");
}

#[test]
fn override_downgrades_deny_to_warning() {
    let verdict =
        evaluate_hook_payload(&payload("git clean -fdX"), FIELD, &workspace_config(), true);
    match verdict {
        HookVerdict::AllowWithOverride { message } => {
            assert!(message.contains("git clean"), "message: {message}");
        }
        other => panic!("expected override allow, got {other:?}"),
    }
}

#[test]
fn unparseable_payloads_allow_with_note() {
    let config = workspace_config();
    for (raw, expect) in [
        ("", "empty hook payload"),
        ("   \n", "empty hook payload"),
        ("{not json", "not valid JSON"),
        (
            r#"{"tool_name": "Bash"}"#,
            "has no `tool_input.command` field",
        ),
        (r#"{"tool_input": {"command": 42}}"#, "is not a string"),
    ] {
        match evaluate_hook_payload(raw, FIELD, &config, false) {
            HookVerdict::AllowUnparsed { note } => assert!(
                note.contains(expect),
                "note for {raw:?} missing {expect:?}: {note}"
            ),
            other => panic!("expected allow-unparsed for {raw:?}, got {other:?}"),
        }
    }
}

#[test]
fn custom_command_field_path_is_honored() {
    let raw = r#"{"cmd": "git clean -fd"}"#;
    match evaluate_hook_payload(raw, "cmd", &workspace_config(), false) {
        HookVerdict::Deny { message } => assert!(message.contains("git clean")),
        other => panic!("expected deny via custom field, got {other:?}"),
    }
}
