use std::path::PathBuf;

use super::claude_hook_settings;

fn command_for(settings: &serde_json::Value, matcher: &str) -> String {
    settings["hooks"]["PreToolUse"]
        .as_array()
        .expect("PreToolUse hook array")
        .iter()
        .find(|entry| entry["matcher"] == matcher)
        .unwrap_or_else(|| panic!("missing {matcher} hook"))["hooks"][0]["command"]
        .as_str()
        .expect("hook command")
        .to_owned()
}

#[test]
fn claude_snippet_uses_command_string_not_args() {
    let settings = claude_hook_settings(
        &PathBuf::from("F:/AI/wow_modernclient/tools/contextmink/target/release/contextmink.exe"),
        Some(&PathBuf::from("F:/AI/wow_modernclient/.contextmink.toml")),
        false,
        &["Bash".to_owned()],
        "tool_input.command",
    );

    let hook = &settings["hooks"]["PreToolUse"][0]["hooks"][0];
    assert_eq!(hook["type"], "command");
    assert!(hook.get("args").is_none());
}

#[test]
fn windows_paths_are_bash_safe() {
    let settings = claude_hook_settings(
        &PathBuf::from(r"F:\AI\wow_modernclient\tools\contextmink\target\release\contextmink.exe"),
        Some(&PathBuf::from(r"F:\AI\wow_modernclient\.contextmink.toml")),
        false,
        &["Bash".to_owned()],
        "tool_input.command",
    );

    let command = command_for(&settings, "Bash");
    assert!(
        command.contains("F:/AI/wow_modernclient/tools/contextmink/target/release/contextmink.exe")
    );
    assert!(command.contains("--config F:/AI/wow_modernclient/.contextmink.toml"));
    assert!(
        !command.contains('\\'),
        "raw backslashes in a Claude Bash hook command are shell escapes: {command}"
    );
}

#[test]
fn paths_with_spaces_are_shell_quoted_per_matcher() {
    let settings = claude_hook_settings(
        &PathBuf::from("C:/Program Files/contextmink/contextmink.exe"),
        Some(&PathBuf::from("C:/Users/Onno/My Repo/.contextmink.toml")),
        false,
        &["Bash".to_owned(), "PowerShell".to_owned()],
        "tool_input.command",
    );

    assert_eq!(
        command_for(&settings, "Bash"),
        "'C:/Program Files/contextmink/contextmink.exe' hook-guard --config 'C:/Users/Onno/My Repo/.contextmink.toml'"
    );
    assert_eq!(
        command_for(&settings, "PowerShell"),
        "& 'C:/Program Files/contextmink/contextmink.exe' hook-guard --config 'C:/Users/Onno/My Repo/.contextmink.toml'"
    );
}

#[test]
fn custom_command_field_and_no_config_are_emitted() {
    let settings = claude_hook_settings(
        &PathBuf::from("/opt/contextmink/contextmink"),
        None,
        true,
        &["Bash".to_owned()],
        "tool.payload.command",
    );

    assert_eq!(
        command_for(&settings, "Bash"),
        "/opt/contextmink/contextmink hook-guard --no-config --command-field tool.payload.command"
    );
}
