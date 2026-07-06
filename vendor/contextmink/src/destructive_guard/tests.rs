use super::{
    ALLOW_DESTRUCTIVE_ENV, DenyDecision, deny_destructive_argv, destructive_override_active,
    evaluate_argv,
};
use crate::config::DestructiveGuardConfig;

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn neutral_config() -> DestructiveGuardConfig {
    DestructiveGuardConfig::default()
}

fn protected_config() -> DestructiveGuardConfig {
    DestructiveGuardConfig {
        recursive_delete_fragments: vec!["protected_cache".to_owned()],
        delete_fragments: vec!["critical.sqlite".to_owned(), "project.gpr".to_owned()],
    }
}

fn denied(args: &[&str]) -> String {
    denied_with_config(args, &neutral_config())
}

fn allowed(args: &[&str]) {
    allowed_with_config(args, &neutral_config());
}

fn denied_with_config(args: &[&str], config: &DestructiveGuardConfig) -> String {
    deny_destructive_argv(&argv(args), config)
        .unwrap_or_else(|| panic!("expected deny for {args:?}, got allow"))
}

fn allowed_with_config(args: &[&str], config: &DestructiveGuardConfig) {
    if let Some(message) = deny_destructive_argv(&argv(args), config) {
        panic!("expected allow for {args:?}, got deny: {message}");
    }
}

#[test]
fn git_clean_is_denied_in_every_spelling() {
    let message = denied(&["git", "clean", "-fdX", "-e", "keep.sqlite"]);
    assert!(message.contains("git clean"), "message: {message}");
    assert!(
        message.contains("built-in destructive-command guard"),
        "message: {message}"
    );

    denied(&["git", "-C", "scratch", "clean", "-fd"]);
    denied(&[
        "git",
        "--no-pager",
        "-c",
        "core.autocrlf=false",
        "clean",
        "-n",
    ]);
    denied(&["git.exe", "clean"]);
    denied(&["C:\\Program Files\\Git\\bin\\git.exe", "clean", "-fdX"]);
    denied(&["C:\\Program Files\\Git\\cmd\\git.cmd", "clean", "-fdX"]);
    denied(&["env", "GIT_DIR=.git", "git", "clean", "-fdX"]);
}

#[test]
fn nested_shell_payloads_are_scanned() {
    let message = denied(&[
        "bash",
        "-lc",
        "cd generated_output && git clean -fdX -e keep.sqlite",
    ]);
    assert!(message.contains("git clean"), "message: {message}");

    denied(&["sh", "-c", "git clean -fd; echo done"]);
    let config = protected_config();
    denied_with_config(
        &[
            "powershell",
            "-Command",
            "Remove-Item -Recurse -Force F:\\protected_cache",
        ],
        &config,
    );
    denied_with_config(&["pwsh", "-c", "rm -rf protected_cache"], &config);
    denied_with_config(&["cmd", "/c", "rd /s /q C:\\protected_cache"], &config);
}

#[test]
fn configured_recursive_forced_deletion_of_protected_paths_is_denied() {
    let config = protected_config();
    let message = denied_with_config(&["rm", "-rf", "protected_cache"], &config);
    assert!(message.contains("protected_cache"), "message: {message}");
    denied_with_config(&["rm", "-r", "-f", "F:/work/protected_cache"], &config);
    denied_with_config(&["rm", "-fR", "generated/protected_cache"], &config);
    denied_with_config(
        &["rm", "--recursive", "--force", "protected_cache"],
        &config,
    );
    denied_with_config(&["Remove-Item", "-Recurse", "protected_cache"], &config);
    denied_with_config(&["Remove-Item", "-r", "-Force", "protected_cache"], &config);
    denied_with_config(&["rmdir", "/s", "/q", "protected_cache"], &config);
    denied_with_config(&["del", "/s", "protected_cache\\notes"], &config);
}

#[test]
fn configured_direct_deletion_of_protected_paths_is_denied() {
    let config = protected_config();
    let message = denied_with_config(&["rm", "-f", "db/critical.sqlite"], &config);
    assert!(message.contains("critical.sqlite"), "message: {message}");
    denied_with_config(&["rm", "project.gpr"], &config);
    denied_with_config(&["del", "F:\\repo\\project.gpr"], &config);
    denied_with_config(&["Remove-Item", "critical.sqlite"], &config);
}

#[test]
fn configured_path_rules_are_off_by_default() {
    allowed(&["rm", "-rf", "protected_cache"]);
    allowed(&["Remove-Item", "-Recurse", "protected_cache"]);
    allowed(&["rm", "-f", "critical.sqlite"]);
    allowed(&["del", "project.gpr"]);
}

#[test]
fn configured_path_rules_do_not_block_backups_or_read_only_inspection() {
    let config = protected_config();
    allowed_with_config(
        &[
            "robocopy",
            "F:/work/protected_cache",
            "E:/backups/protected_cache",
            "/MIR",
        ],
        &config,
    );
    allowed_with_config(
        &["cp", "db/critical.sqlite", "E:/backups/critical.sqlite"],
        &config,
    );
    allowed_with_config(
        &[
            "sqlite3",
            "db/critical.sqlite",
            "select count(*) from items;",
        ],
        &config,
    );
}

#[test]
fn configured_fragments_ignore_empty_entries() {
    let config = DestructiveGuardConfig {
        recursive_delete_fragments: vec![String::new(), "   ".to_owned()],
        delete_fragments: vec![String::new()],
    };
    allowed_with_config(&["rm", "-rf", "anything"], &config);
    allowed_with_config(&["rm", "-f", "anything.sqlite"], &config);
}

#[test]
fn ordinary_commands_stay_allowed() {
    allowed(&["git", "status"]);
    allowed(&["git", "log", "--grep=clean"]);
    allowed(&["cargo", "clean"]);
    allowed(&["rm", "-f", "C:/Temp/scratch.txt"]);
    allowed(&["rm", "-rf", "target/debug"]);
    allowed(&[]);
}

#[test]
fn break_glass_override_downgrades_deny_to_loud_allow() {
    let fatal = argv(&["git", "clean", "-fdX", "-e", "keep.sqlite"]);
    let DenyDecision::Deny { message } = evaluate_argv(&fatal, &neutral_config(), false) else {
        panic!("expected Deny without override");
    };
    assert_eq!(
        evaluate_argv(&fatal, &neutral_config(), true),
        DenyDecision::AllowWithOverride { message }
    );
    assert_eq!(
        evaluate_argv(&argv(&["git", "status"]), &neutral_config(), true),
        DenyDecision::Allow
    );
}

#[test]
fn override_env_requires_exact_one() {
    // Deliberately never set the variable to "1" in tests: the deny tests in
    // the bridge test binary run in parallel, and a transient "1" could let
    // a blocked argv actually spawn. "0" and unset are safe to exercise.
    assert!(!destructive_override_active());
    unsafe { std::env::set_var(ALLOW_DESTRUCTIVE_ENV, "0") };
    assert!(!destructive_override_active());
    unsafe { std::env::remove_var(ALLOW_DESTRUCTIVE_ENV) };
    assert!(!destructive_override_active());
}
