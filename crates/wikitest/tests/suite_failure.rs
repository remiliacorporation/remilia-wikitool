use std::fs;

use serde_json::json;
use wikitest::artifact::atomic_write_json;
use wikitest::inspection::inspect_receipt;
use wikitest::model::{RunStatus, SCENARIO_SCHEMA, SUITE_SCHEMA};
use wikitest::runner::{RunOptions, run_suite};

#[test]
fn failed_suite_replays_without_granting_missing_coverage_or_a_forged_pass() {
    let root = tempfile::tempdir().unwrap();
    let catalog = root.path().join("wikitest");
    fs::create_dir(&catalog).unwrap();
    // This executable deliberately rejects Wikitool's runner-owned flags. It
    // provides a real failed process without depending on another crate's build.
    let tool = root
        .path()
        .join(if cfg!(windows) { "tool.exe" } else { "tool" });
    fs::copy(env!("CARGO_BIN_EXE_wikitest"), &tool).unwrap();
    atomic_write_json(&catalog.join("scenario.json"), &json!({
        "schema": SCENARIO_SCHEMA, "id":"failure-control", "title":"Failure control",
        "description":"A failed command cannot demonstrate its capability.",
        "kind":"mechanical", "environment":"isolated", "timeout_ms":10000,
        "coverage":[{"capability":"unproven", "steps":["reject"]}],
        "steps":[{"action":"command", "id":"reject", "argv":["status"], "expect":{"exit_code":0}}]
    })).unwrap();
    let suite = catalog.join("suite.json");
    atomic_write_json(
        &suite,
        &json!({
            "schema": SUITE_SCHEMA, "id":"failure-suite", "title":"Failure suite",
            "required_coverage":["unproven"], "scenarios":["failure-control"]
        }),
    )
    .unwrap();
    let repository = root.path().canonicalize().unwrap();
    let options = RunOptions::new(repository.clone(), repository.join("runs"), tool);
    let mut run = run_suite(&suite, &options, true).unwrap();
    assert_eq!(
        run.receipt.status,
        RunStatus::Failed,
        "{:?}",
        run.receipt.runs
    );
    assert!(run.receipt.observed_coverage.is_empty());
    let inspection = inspect_receipt(&run.receipt_path, &repository).unwrap();
    assert!(inspection.evidence_replayed, "{:?}", inspection.checks);
    assert_eq!(inspection.source_status, "failed");

    run.receipt.status = RunStatus::Passed;
    atomic_write_json(&run.receipt_path, &run.receipt).unwrap();
    assert!(
        !inspect_receipt(&run.receipt_path, &repository)
            .unwrap()
            .evidence_replayed
    );
    run.receipt.status = RunStatus::Failed;
    run.receipt.observed_coverage.push("unproven".into());
    atomic_write_json(&run.receipt_path, &run.receipt).unwrap();
    assert!(
        !inspect_receipt(&run.receipt_path, &repository)
            .unwrap()
            .evidence_replayed
    );
}
