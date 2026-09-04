use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn plan(root: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_wikitool"))
        .arg("--project-root")
        .arg(root)
        .args([
            "templates",
            "migration-plan",
            "migration.json",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn plan_binds_full_current_inventory_without_catalog_or_rewrites() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".wikitool")).unwrap();
    fs::create_dir_all(root.path().join("wiki_content/Main")).unwrap();
    fs::write(
        root.path().join("migration.json"),
        r#"{
        "schema":"template_migration_spec_v1", "from_template":"Old", "to_template":"New",
        "title_case":"first_letter", "parameter_renames":{"old":"new"}
    }"#,
    )
    .unwrap();
    let source = "Start {{Old | old = value}} end";
    let article = root.path().join("wiki_content/Main/Article.wiki");
    fs::write(&article, source).unwrap();
    fs::write(
        root.path().join("wiki_content/Main/Unrelated.wiki"),
        "Other",
    )
    .unwrap();
    let first = plan(root.path());
    assert_eq!(first["scanned_files"], 2);
    assert_eq!(first["affected_files"], 1);
    assert_eq!(first["mechanical_patch_count"], 2);
    assert_eq!(first["retirement_ready"], false);
    assert_eq!(first["files"][0]["invocations"][0]["start_byte"], 6);
    assert_eq!(fs::read_to_string(&article).unwrap(), source);
    assert_eq!(first["plan_id"], plan(root.path())["plan_id"]);
    fs::write(
        root.path().join("wiki_content/Main/Unrelated.wiki"),
        "Changed",
    )
    .unwrap();
    assert_ne!(first["plan_id"], plan(root.path())["plan_id"]);
    assert!(!root.path().join(".wikitool/sync/sync.sqlite3").exists());
    assert!(!root.path().join(".wikitool/data/wikitool.db").exists());
}
