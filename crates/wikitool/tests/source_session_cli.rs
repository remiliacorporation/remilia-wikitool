use std::io::Write;
use std::process::{Command, Output, Stdio};

use tempfile::tempdir;

#[test]
fn import_rejects_literal_cookie_argv_without_echoing_it() {
    let project = initialized_project();
    let literal = "session_cookie=literal-argv-sentinel";

    let output = Command::new(env!("CARGO_BIN_EXE_wikitool"))
        .args([
            "source",
            "session",
            "import",
            "https://example.invalid/source",
            "--project-root",
        ])
        .arg(project.path())
        .args(["--cookies", literal, "--format", "json"])
        .output()
        .expect("run source session import");
    let rendered = combined_output(&output);

    assert!(!output.status.success(), "literal cookie argv was accepted");
    assert!(
        rendered.contains("existing regular file"),
        "rejection did not explain the safe input contract"
    );
    assert!(
        !rendered.contains(literal) && !rendered.contains("literal-argv-sentinel"),
        "rejection echoed cookie material"
    );
}

#[test]
fn import_accepts_stdin_without_printing_cookie_values() {
    let project = initialized_project();
    let cookie = "session_cookie=stdin-value-sentinel";
    let mut child = Command::new(env!("CARGO_BIN_EXE_wikitool"))
        .args([
            "source",
            "session",
            "import",
            "https://example.invalid/source",
            "--project-root",
        ])
        .arg(project.path())
        .args(["--cookies", "-", "--format", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn source session import");
    child
        .stdin
        .take()
        .expect("open child stdin")
        .write_all(cookie.as_bytes())
        .expect("write cookie fixture to stdin");
    let output = child.wait_with_output().expect("wait for session import");
    let rendered = combined_output(&output);

    assert!(output.status.success(), "stdin session import failed");
    assert!(
        !rendered.contains(cookie) && !rendered.contains("stdin-value-sentinel"),
        "session import output printed cookie material"
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse masked import summary");
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["session"]["cookie_names"][0], "session_cookie");
}

#[test]
fn import_help_exposes_only_file_or_stdin_inputs() {
    let output = Command::new(env!("CARGO_BIN_EXE_wikitool"))
        .args(["source", "session", "import", "--help"])
        .output()
        .expect("run source session import help");
    let rendered = combined_output(&output);

    assert!(output.status.success(), "session import help failed");
    assert!(rendered.contains("existing regular"));
    assert!(rendered.contains("non-symlink file"));
    assert!(rendered.contains("literal values are rejected"));
    assert!(!rendered.contains("COOKIE_HEADER"));
    assert!(!rendered.contains("literal Cookie header"));
}

fn initialized_project() -> tempfile::TempDir {
    let project = tempdir().expect("create isolated project");
    let output = Command::new(env!("CARGO_BIN_EXE_wikitool"))
        .args(["init", "--project-root"])
        .arg(project.path())
        .args([
            "--api-url",
            "https://example.invalid/api.php",
            "--no-network",
        ])
        .output()
        .expect("initialize isolated project");
    assert!(output.status.success(), "isolated project init failed");
    project
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
