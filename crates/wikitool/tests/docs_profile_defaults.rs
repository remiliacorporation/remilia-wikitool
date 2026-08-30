use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

fn run_json(project_root: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_wikitool"))
        .arg("--project-root")
        .arg(project_root)
        .args(args)
        .env("WIKITOOL_SILENT", "1")
        .output()
        .expect("run wikitool");
    assert!(
        output.status.success(),
        "wikitool failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON output")
}

fn write_site_adapter(project_root: &Path) {
    let adapter_dir = project_root.join("site-adapter");
    fs::create_dir_all(project_root.join(".wikitool")).expect("state dir");
    fs::create_dir_all(&adapter_dir).expect("adapter dir");
    fs::write(
        project_root.join(".wikitool/config.toml"),
        "[adapter]\npath = \"site-adapter/site-adapter.toml\"\n",
    )
    .expect("config");
    fs::write(
        adapter_dir.join("site-adapter.toml"),
        include_str!("../../../site_adapters/generic/site-adapter.toml").replace(
            "docs_profile = \"mw-1.44-authoring\"",
            "docs_profile = \"mw-1.44-site-authoring\"",
        ),
    )
    .expect("adapter");
}

#[test]
fn adapter_docs_profile_defaults_flow_through_catalog_and_docs_commands() {
    let generic_temp = tempdir().expect("generic tempdir");
    fs::create_dir_all(generic_temp.path().join(".wikitool")).expect("generic state dir");
    let generic_status = run_json(
        generic_temp.path(),
        &["catalog", "status", "--format", "json"],
    );
    assert_eq!(
        generic_status["docs_profile_requested"],
        "mw-1.44-authoring"
    );

    let temp = tempdir().expect("tempdir");
    write_site_adapter(temp.path());

    let status = run_json(temp.path(), &["catalog", "status", "--format", "json"]);
    assert_eq!(status["docs_profile_requested"], "mw-1.44-site-authoring");

    let explicit_status = run_json(
        temp.path(),
        &[
            "catalog",
            "status",
            "--docs-profile",
            "mw-1.44-authoring",
            "--format",
            "json",
        ],
    );
    assert_eq!(
        explicit_status["docs_profile_requested"],
        "mw-1.44-authoring"
    );

    let context = run_json(
        temp.path(),
        &["docs", "context", "hooks", "--format", "json"],
    );
    assert_eq!(context["profile"], "mw-1.44-site-authoring");

    let explicit_context = run_json(
        temp.path(),
        &[
            "docs",
            "context",
            "hooks",
            "--profile",
            "mw-1.44-authoring",
            "--format",
            "json",
        ],
    );
    assert_eq!(explicit_context["profile"], "mw-1.44-authoring");
}
