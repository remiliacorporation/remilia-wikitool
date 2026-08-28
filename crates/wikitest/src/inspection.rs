use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::artifact::{resolve_output_path, sha256_bytes, sha256_file};
use crate::identity::resolve_recorded_identity_path;
use crate::mediawiki::{MediaWikiObservation, evaluate_expectation};
use crate::model::{
    ArtifactIdentity, AssertionReceipt, RECEIPT_SCHEMA, REQUIREMENT_OBSERVATION_SCHEMA,
    RequirementObservation, RunReceipt, RunStatus, SUITE_RECEIPT_SCHEMA, ScenarioManifest,
    ScenarioStep, StepReceipt, SuiteManifest, SuiteReceipt,
};
use crate::process::CapturedStream;
use crate::prose::{
    operational_participant_export_root, prose_coverage_status, prose_suite_catalog_locator,
    resolve_prose_suite_child_receipt, review_assignment_projection, verify_current_receipt,
};
use crate::prose_model::{
    AUTHOR_REQUEST_SCHEMA, AuthorRequest, PROSE_RECEIPT_SCHEMA, PROSE_SUITE_RECEIPT_SCHEMA,
    ParticipantExport, ProseAssignment, ProseCoverageStatus, ProseMode, ProseReceipt,
    ProseRunStatus, ProseSuite, ProseSuiteReceipt, ProseSuiteStatus, REVIEW_REQUEST_SCHEMA,
    REVIEW_SUBMISSION_SCHEMA, ReviewDisposition, ReviewPacketBinding, ReviewRequest,
    ReviewSubmission,
};
use crate::runner::{
    evaluate_file_assertion, evaluate_json_captures, evaluate_output_assertions,
    successful_coverage,
};

pub const INSPECTION_SCHEMA: &str = "wikitest.receipt-inspection.v2";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptAuthenticity {
    Unanchored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptInspection {
    pub schema: String,
    pub source_schema: String,
    pub source_status: String,
    pub evidence_replayed: bool,
    pub authenticity: ReceiptAuthenticity,
    pub checks: Vec<InspectionCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

pub fn inspect_receipt(path: &Path, repository: &Path) -> Result<ReceiptInspection> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect receipt {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "receipt must be a non-symlink plain file: {}",
            path.display()
        );
    }
    let bytes = fs::read(path)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid receipt JSON {}", path.display()))?;
    let schema = value
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .context("receipt has no string schema")?;
    match schema {
        RECEIPT_SCHEMA => inspect_run(
            path,
            repository,
            serde_json::from_slice(&bytes).context("invalid run receipt")?,
        ),
        SUITE_RECEIPT_SCHEMA => inspect_suite(
            path,
            repository,
            serde_json::from_slice(&bytes).context("invalid suite receipt")?,
        ),
        PROSE_RECEIPT_SCHEMA => inspect_prose(
            path,
            repository,
            serde_json::from_slice(&bytes).context("invalid prose receipt")?,
        ),
        PROSE_SUITE_RECEIPT_SCHEMA => inspect_prose_suite(
            path,
            repository,
            serde_json::from_slice(&bytes).context("invalid prose suite receipt")?,
        ),
        _ => bail!("unsupported receipt schema '{schema}'"),
    }
}

fn inspect_run(path: &Path, repository: &Path, receipt: RunReceipt) -> Result<ReceiptInspection> {
    let root = path.parent().context("receipt path has no parent")?;
    let mut checks = Vec::new();
    let scenario_input = receipt
        .inputs
        .iter()
        .find(|artifact| artifact.locator == "inputs/scenario.json");
    checks.push(InspectionCheck {
        name: "scenario_input_bound".to_owned(),
        passed: scenario_input.is_some_and(|artifact| artifact.sha256 == receipt.scenario.sha256),
        detail: scenario_input.map_or_else(
            || "inputs/scenario.json is missing".to_owned(),
            |artifact| {
                format!(
                    "receipt scenario digest {}, input digest {}",
                    receipt.scenario.sha256, artifact.sha256
                )
            },
        ),
    });
    let scenario_manifest = scenario_input.and_then(|artifact| {
        resolve_output_path(root, &artifact.locator)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ScenarioManifest>(&bytes).ok())
    });
    checks.push(InspectionCheck {
        name: "scenario_manifest_valid".to_owned(),
        passed: scenario_manifest
            .as_ref()
            .is_some_and(|manifest| manifest.validate().is_ok()),
        detail: "retained scenario manifest must satisfy the current strict schema".to_owned(),
    });
    checks.push(InspectionCheck {
        name: "scenario_identity".to_owned(),
        passed: scenario_manifest.as_ref().is_some_and(|manifest| {
            manifest.id == receipt.scenario.id
                && manifest.title == receipt.scenario.title
                && manifest.kind == receipt.scenario.kind
                && manifest.environment == receipt.scenario.environment
                && manifest.coverage == receipt.scenario.coverage
        }),
        detail: "receipt identity must equal the retained strict scenario manifest".to_owned(),
    });
    for artifact in &receipt.inputs {
        checks.push(verify_artifact(root, artifact, "input"));
    }
    for requirement in &receipt.requirements {
        checks.push(verify_artifact(
            root,
            &requirement.observation,
            "requirement observation",
        ));
    }
    for step in &receipt.steps {
        if let Some(artifact) = &step.copied {
            checks.push(verify_artifact(root, artifact, "copied output"));
        }
        if let Some(output) = &step.stdout {
            checks.push(verify_output(root, output, "stdout"));
        }
        if let Some(output) = &step.stderr {
            checks.push(verify_output(root, output, "stderr"));
        }
        if let Some(artifact) = &step.observation {
            checks.push(verify_artifact(root, artifact, "step observation"));
        }
        for assertion in &step.assertions {
            if let Some(artifact) = assertion
                .file_evidence
                .as_ref()
                .and_then(|evidence| evidence.artifact.as_ref())
            {
                checks.push(verify_artifact(root, artifact, "file assertion evidence"));
            }
        }
    }
    checks.push(binary_identity_check(
        repository,
        &receipt.driver,
        "driver_identity",
    ));
    checks.push(binary_identity_check(
        repository,
        &receipt.tool,
        "tool_identity",
    ));
    let observed_truncation = receipt.steps.iter().any(|step| {
        step.stdout.as_ref().is_some_and(|output| output.truncated)
            || step.stderr.as_ref().is_some_and(|output| output.truncated)
    });
    checks.push(InspectionCheck {
        name: "completeness_state".to_owned(),
        passed: receipt.output_truncated == observed_truncation
            && receipt.complete == !observed_truncation,
        detail: format!(
            "complete={}, output_truncated={}, observed_truncation={observed_truncation}",
            receipt.complete, receipt.output_truncated
        ),
    });
    let step_closure = scenario_manifest
        .as_ref()
        .is_some_and(|manifest| steps_form_valid_prefix(manifest, &receipt));
    checks.push(InspectionCheck {
        name: "step_closure".to_owned(),
        passed: step_closure,
        detail: format!(
            "retained manifest has {} step(s); receipt records {}",
            scenario_manifest
                .as_ref()
                .map_or(0, |manifest| manifest.steps.len()),
            receipt.steps.len()
        ),
    });
    let requirement_closure = scenario_manifest
        .as_ref()
        .is_some_and(|manifest| requirements_match(root, manifest, &receipt));
    checks.push(InspectionCheck {
        name: "requirement_closure".to_owned(),
        passed: requirement_closure,
        detail: format!(
            "retained manifest has {} requirement(s); receipt records {}",
            scenario_manifest
                .as_ref()
                .map_or(0, |manifest| manifest.requirements.len()),
            receipt.requirements.len()
        ),
    });
    let replayed = scenario_manifest.as_ref().is_some_and(|manifest| {
        receipt
            .steps
            .iter()
            .zip(&manifest.steps)
            .all(|(recorded, declared)| replay_step(root, declared, recorded).unwrap_or(false))
    });
    checks.push(InspectionCheck {
        name: "assertions_replayed".to_owned(),
        passed: replayed,
        detail:
            "every recorded assertion must be recomputable from retained immutable observations"
                .to_owned(),
    });
    let status_consistent = match receipt.status {
        RunStatus::Passed => {
            requirement_closure
                && step_closure
                && replayed
                && !receipt.steps.is_empty()
                && receipt
                    .steps
                    .iter()
                    .all(|step| step.status == RunStatus::Passed)
                && receipt
                    .requirements
                    .iter()
                    .all(|requirement| requirement.passed)
                && receipt.failure.is_none()
        }
        RunStatus::Failed => {
            requirement_closure
                && step_closure
                && replayed
                && (receipt
                    .steps
                    .iter()
                    .any(|step| step.status == RunStatus::Failed)
                    || receipt.requirements.iter().any(|requirement| {
                        !requirement.passed
                            && requirement.disposition == crate::model::MissingDisposition::Fail
                    }))
        }
        RunStatus::Skipped => {
            requirement_closure
                && step_closure
                && receipt.steps.is_empty()
                && receipt.requirements.iter().any(|requirement| {
                    !requirement.passed
                        && requirement.disposition == crate::model::MissingDisposition::Skip
                })
        }
        RunStatus::Error => step_closure && receipt.failure.is_some(),
    };
    checks.push(InspectionCheck {
        name: "status_consistent".to_owned(),
        passed: status_consistent,
        detail: format!("source status is {:?}", receipt.status),
    });
    let evidence_replayed = checks.iter().all(|check| check.passed);
    Ok(ReceiptInspection {
        schema: INSPECTION_SCHEMA.to_owned(),
        source_schema: RECEIPT_SCHEMA.to_owned(),
        source_status: enum_name(receipt.status),
        evidence_replayed,
        authenticity: ReceiptAuthenticity::Unanchored,
        checks,
    })
}

fn requirements_match(root: &Path, manifest: &ScenarioManifest, receipt: &RunReceipt) -> bool {
    manifest.requirements.len() == receipt.requirements.len()
        && manifest
            .requirements
            .iter()
            .zip(&receipt.requirements)
            .enumerate()
            .all(|(index, (declared, recorded))| match declared {
                crate::model::Requirement::PathExists { path, on_missing } => {
                    let expected_locator = format!("requirements/{index:04}-path-exists.json");
                    if recorded.kind != "path_exists"
                        || recorded.disposition != *on_missing
                        || recorded.observation.locator != expected_locator
                    {
                        return false;
                    }
                    let observation_path =
                        match resolve_output_path(root, &recorded.observation.locator) {
                            Ok(path) => path,
                            Err(_) => return false,
                        };
                    let bytes = match fs::read(&observation_path) {
                        Ok(bytes) => bytes,
                        Err(_) => return false,
                    };
                    if bytes.len() as u64 != recorded.observation.bytes
                        || sha256_bytes(&bytes) != recorded.observation.sha256
                    {
                        return false;
                    }
                    let observation = match serde_json::from_slice::<RequirementObservation>(&bytes)
                    {
                        Ok(observation) => observation,
                        Err(_) => return false,
                    };
                    observation.schema == REQUIREMENT_OBSERVATION_SCHEMA
                        && observation.kind == "path_exists"
                        && observation.declared_path == *path
                        && !observation.expanded_path.trim().is_empty()
                        && observation.exists == recorded.passed
                        && if observation.exists {
                            observation
                                .path_kind
                                .as_deref()
                                .is_some_and(|kind| matches!(kind, "file" | "directory" | "other"))
                                && recorded.detail
                                    == format!("path exists: {}", observation.expanded_path)
                        } else {
                            observation.path_kind.is_none()
                                && recorded.detail
                                    == format!("path is missing: {}", observation.expanded_path)
                        }
                }
            })
}

fn steps_form_valid_prefix(manifest: &ScenarioManifest, receipt: &RunReceipt) -> bool {
    if receipt.steps.len() > manifest.steps.len() {
        return false;
    }
    if !receipt
        .steps
        .iter()
        .zip(&manifest.steps)
        .all(|(recorded, declared)| {
            recorded.id == declared.id()
                && recorded.action == step_action(declared)
                && match declared {
                    ScenarioStep::Command { argv, .. } => recorded.argv == *argv,
                    _ => recorded.argv.is_empty(),
                }
        })
    {
        return false;
    }
    match receipt.status {
        RunStatus::Passed => receipt.steps.len() == manifest.steps.len(),
        RunStatus::Skipped => receipt.steps.is_empty(),
        RunStatus::Failed => {
            receipt.steps.is_empty()
                || (receipt.steps[..receipt.steps.len().saturating_sub(1)]
                    .iter()
                    .all(|step| step.status == RunStatus::Passed)
                    && receipt
                        .steps
                        .last()
                        .is_some_and(|step| step.status == RunStatus::Failed))
        }
        RunStatus::Error => receipt.steps.is_empty() || receipt.steps.len() <= manifest.steps.len(),
    }
}

fn suite_membership_matches(manifest: &SuiteManifest, receipt: &SuiteReceipt) -> bool {
    manifest.scenarios
        == receipt
            .runs
            .iter()
            .map(|run| run.scenario.clone())
            .collect::<Vec<_>>()
}

fn replay_step(root: &Path, declared: &ScenarioStep, recorded: &StepReceipt) -> Result<bool> {
    let (replayed, replayed_captures) = match declared {
        ScenarioStep::Copy { sha256, target, .. } => {
            let artifact = recorded
                .copied
                .as_ref()
                .context("copy step omitted retained output")?;
            (
                vec![AssertionReceipt {
                    target: target.clone(),
                    assertion: "copied_sha256".to_owned(),
                    passed: artifact.sha256 == *sha256,
                    detail: artifact.sha256.clone(),
                    file_evidence: None,
                }],
                Vec::new(),
            )
        }
        ScenarioStep::Command {
            id,
            captures,
            expect,
            ..
        } => replay_command_assertions(root, id, captures, expect, recorded)?,
        ScenarioStep::MediaWikiUpdate { page, .. } => {
            let observation = read_mediawiki_observation(root, recorded)?;
            let observed = observation
                .pages
                .iter()
                .find(|item| item.title == page.title);
            (
                vec![AssertionReceipt {
                    target: format!("mediawiki.page:{}", page.title),
                    assertion: "fixture_page_updated".to_owned(),
                    passed: observed == Some(page),
                    detail: format!("revision_id={}", page.revision_id),
                    file_evidence: None,
                }],
                Vec::new(),
            )
        }
        ScenarioStep::MediaWikiAssert { expect, .. } => (
            evaluate_expectation(&read_mediawiki_observation(root, recorded)?, expect),
            Vec::new(),
        ),
    };
    Ok(replayed_captures == recorded.captures
        && assertion_outcomes_match(&replayed, &recorded.assertions)
        && recorded.status
            == if replayed.iter().all(|assertion| assertion.passed) {
                RunStatus::Passed
            } else {
                RunStatus::Failed
            })
}

fn replay_command_assertions(
    root: &Path,
    step_id: &str,
    captures: &[crate::model::JsonScalarCapture],
    expect: &crate::model::CommandExpectation,
    recorded: &StepReceipt,
) -> Result<(
    Vec<AssertionReceipt>,
    Vec<crate::model::JsonScalarCaptureReceipt>,
)> {
    let stdout = captured_stream(
        root,
        recorded.stdout.as_ref().context("command omitted stdout")?,
    )?;
    let stderr = captured_stream(
        root,
        recorded.stderr.as_ref().context("command omitted stderr")?,
    )?;
    let mut assertions = vec![AssertionReceipt {
        target: "process".to_owned(),
        assertion: "exit_code".to_owned(),
        passed: recorded.exit_code == Some(expect.exit_code),
        detail: String::new(),
        file_evidence: None,
    }];
    assertions.push(AssertionReceipt {
        target: "process".to_owned(),
        assertion: "within_timeout".to_owned(),
        passed: recorded.timed_out == Some(false),
        detail: String::new(),
        file_evidence: None,
    });
    assertions.extend(evaluate_output_assertions(
        "stdout",
        &stdout,
        &expect.stdout,
    ));
    assertions.extend(evaluate_output_assertions(
        "stderr",
        &stderr,
        &expect.stderr,
    ));
    let (replayed_captures, capture_assertions) =
        evaluate_json_captures(step_id, captures, &stdout);
    assertions.extend(capture_assertions);
    let file_start = assertions.len();
    for (offset, expected) in expect.files.iter().enumerate() {
        let evidence = recorded
            .assertions
            .get(file_start + offset)
            .and_then(|assertion| assertion.file_evidence.clone())
            .context("file assertion omitted retained observation")?;
        let bytes = evidence
            .artifact
            .as_ref()
            .map(|artifact| read_artifact(root, artifact))
            .transpose()?;
        assertions.push(evaluate_file_assertion(
            expected,
            evidence,
            bytes.as_deref(),
        )?);
    }
    if stdout.truncated || stderr.truncated {
        assertions.push(AssertionReceipt {
            target: "process".to_owned(),
            assertion: "output_complete".to_owned(),
            passed: false,
            detail: String::new(),
            file_evidence: None,
        });
    }
    Ok((assertions, replayed_captures))
}

fn assertion_outcomes_match(expected: &[AssertionReceipt], recorded: &[AssertionReceipt]) -> bool {
    expected.len() == recorded.len()
        && expected.iter().zip(recorded).all(|(expected, recorded)| {
            expected.target == recorded.target
                && expected.assertion == recorded.assertion
                && expected.passed == recorded.passed
        })
}

fn captured_stream(root: &Path, artifact: &crate::model::OutputArtifact) -> Result<CapturedStream> {
    let path = resolve_output_path(root, &artifact.locator)?;
    let bytes = fs::read(path)?;
    Ok(CapturedStream {
        sha256: artifact.sha256.clone(),
        stored_sha256: sha256_bytes(&bytes),
        observed_bytes: artifact.observed_bytes,
        stored_bytes: bytes.len() as u64,
        truncated: artifact.truncated,
        bytes,
    })
}

fn read_artifact(root: &Path, artifact: &ArtifactIdentity) -> Result<Vec<u8>> {
    fs::read(resolve_output_path(root, &artifact.locator)?)
        .with_context(|| format!("failed to read retained artifact {}", artifact.locator))
}

fn read_mediawiki_observation(root: &Path, recorded: &StepReceipt) -> Result<MediaWikiObservation> {
    let artifact = recorded
        .observation
        .as_ref()
        .context("MediaWiki step omitted retained observation")?;
    serde_json::from_slice(&read_artifact(root, artifact)?).context("invalid MediaWiki observation")
}

fn step_action(step: &ScenarioStep) -> &'static str {
    match step {
        ScenarioStep::Copy { .. } => "copy",
        ScenarioStep::Command { .. } => "command",
        ScenarioStep::MediaWikiUpdate { .. } => "mediawiki_update",
        ScenarioStep::MediaWikiAssert { .. } => "mediawiki_assert",
    }
}

fn inspect_suite(
    path: &Path,
    repository: &Path,
    receipt: SuiteReceipt,
) -> Result<ReceiptInspection> {
    let root = path.parent().context("receipt path has no parent")?;
    let artifact_root = root.parent().context("suite run has no artifact root")?;
    let mut checks = Vec::new();
    let suite_input = root.join("inputs/suite.json");
    let suite_manifest = fs::read(&suite_input)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SuiteManifest>(&bytes).ok());
    checks.push(match sha256_file(&suite_input) {
        Ok((digest, _)) => InspectionCheck {
            name: "suite_input_bound".to_owned(),
            passed: digest == receipt.suite.sha256,
            detail: format!("expected {}, observed {digest}", receipt.suite.sha256),
        },
        Err(error) => InspectionCheck {
            name: "suite_input_bound".to_owned(),
            passed: false,
            detail: format!("{error:#}"),
        },
    });
    checks.push(InspectionCheck {
        name: "suite_manifest_valid".to_owned(),
        passed: suite_manifest
            .as_ref()
            .is_some_and(|manifest| manifest.validate().is_ok()),
        detail: "retained suite manifest must satisfy current strict invariants".to_owned(),
    });
    checks.push(InspectionCheck {
        name: "suite_identity".to_owned(),
        passed: suite_manifest.as_ref().is_some_and(|manifest| {
            manifest.id == receipt.suite.id
                && manifest.title == receipt.suite.title
                && manifest.required_coverage == receipt.required_coverage
        }),
        detail: "receipt identity must equal the retained strict suite manifest".to_owned(),
    });
    checks.push(InspectionCheck {
        name: "suite_membership".to_owned(),
        passed: suite_manifest
            .as_ref()
            .is_some_and(|manifest| suite_membership_matches(manifest, &receipt)),
        detail: "receipt children must exactly match retained suite membership and order"
            .to_owned(),
    });
    let mut child_complete = true;
    let mut observed_coverage = BTreeSet::new();
    for run in &receipt.runs {
        let Some(locator) = &run.receipt_locator else {
            child_complete = false;
            checks.push(InspectionCheck {
                name: format!("child_receipt:{}", run.scenario),
                passed: false,
                detail: run
                    .error
                    .clone()
                    .unwrap_or_else(|| "child receipt locator is missing".to_owned()),
            });
            continue;
        };
        let child_path = match resolve_output_path(artifact_root, locator) {
            Ok(path) => path,
            Err(error) => {
                child_complete = false;
                checks.push(InspectionCheck {
                    name: format!("child_receipt:{}", run.scenario),
                    passed: false,
                    detail: format!("{error:#}"),
                });
                continue;
            }
        };
        let check = match sha256_file(&child_path) {
            Ok((digest, _)) => {
                let parsed = fs::read(&child_path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<RunReceipt>(&bytes).ok());
                let identity_matches = parsed.as_ref().is_some_and(|child| {
                    child.schema == RECEIPT_SCHEMA
                        && Some(child.scenario.id.as_str()) == run.scenario_id.as_deref()
                        && child.status == run.status
                });
                let child_inspection = parsed
                    .as_ref()
                    .and_then(|child| inspect_run(&child_path, repository, child.clone()).ok());
                if let Some(child) = &parsed
                    && child_inspection
                        .as_ref()
                        .is_some_and(|inspection| inspection.evidence_replayed)
                {
                    observed_coverage.extend(successful_coverage(child));
                }
                child_complete &= parsed.as_ref().is_some_and(|child| child.complete)
                    && child_inspection
                        .as_ref()
                        .is_some_and(|inspection| inspection.evidence_replayed);
                InspectionCheck {
                    name: format!("child_receipt:{}", run.scenario),
                    passed: Some(&digest) == run.receipt_sha256.as_ref()
                        && identity_matches
                        && child_inspection
                            .as_ref()
                            .is_some_and(|inspection| inspection.evidence_replayed),
                    detail: format!(
                        "expected digest {}, observed {digest}, identity_matches={identity_matches}, evidence_replayed={}",
                        run.receipt_sha256.as_deref().unwrap_or("<missing>"),
                        child_inspection
                            .as_ref()
                            .is_some_and(|inspection| inspection.evidence_replayed)
                    ),
                }
            }
            Err(error) => {
                child_complete = false;
                InspectionCheck {
                    name: format!("child_receipt:{}", run.scenario),
                    passed: false,
                    detail: format!("{error:#}"),
                }
            }
        };
        checks.push(check);
    }
    let recorded_coverage = receipt
        .observed_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let required_coverage = receipt
        .required_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let missing_coverage = !required_coverage.is_subset(&observed_coverage);
    let expected_status = if receipt
        .runs
        .iter()
        .any(|run| run.status == RunStatus::Error)
    {
        RunStatus::Error
    } else if receipt
        .runs
        .iter()
        .any(|run| run.status == RunStatus::Failed)
        || missing_coverage
        || (receipt.require_all
            && receipt
                .runs
                .iter()
                .any(|run| run.status == RunStatus::Skipped))
    {
        RunStatus::Failed
    } else {
        RunStatus::Passed
    };
    checks.push(InspectionCheck {
        name: "suite_status_consistent".to_owned(),
        passed: receipt.status == expected_status,
        detail: format!(
            "recorded {:?}, recomputed {:?}",
            receipt.status, expected_status
        ),
    });
    checks.push(InspectionCheck {
        name: "suite_coverage".to_owned(),
        passed: recorded_coverage == observed_coverage
            && required_coverage.is_subset(&observed_coverage),
        detail: format!(
            "required {:?}, recorded {:?}, observed {:?}",
            required_coverage, recorded_coverage, observed_coverage
        ),
    });
    checks.push(InspectionCheck {
        name: "suite_completeness".to_owned(),
        passed: receipt.complete == child_complete,
        detail: format!(
            "recorded {}, child receipts complete {child_complete}",
            receipt.complete
        ),
    });
    checks.push(binary_identity_check(
        repository,
        &receipt.driver,
        "driver_identity",
    ));
    let evidence_replayed = checks.iter().all(|check| check.passed);
    Ok(ReceiptInspection {
        schema: INSPECTION_SCHEMA.to_owned(),
        source_schema: SUITE_RECEIPT_SCHEMA.to_owned(),
        source_status: enum_name(receipt.status),
        evidence_replayed,
        authenticity: ReceiptAuthenticity::Unanchored,
        checks,
    })
}

fn inspect_prose(
    path: &Path,
    repository: &Path,
    receipt: ProseReceipt,
) -> Result<ReceiptInspection> {
    let root = path.parent().context("prose receipt path has no parent")?;
    let mut checks = Vec::new();
    checks.push(match verify_current_receipt(repository, root, &receipt) {
        Ok(()) => InspectionCheck {
            name: "prose_semantic_replay".to_owned(),
            passed: true,
            detail: "retained stages, redaction, oracle result, and isolated exports re-derived"
                .to_owned(),
        },
        Err(error) => InspectionCheck {
            name: "prose_semantic_replay".to_owned(),
            passed: false,
            detail: format!("{error:#}"),
        },
    });
    checks.push(verify_artifact(root, &receipt.packet, "prose packet"));
    for input in &receipt.inputs {
        checks.push(verify_artifact(root, input, "prose input"));
    }
    let assignment_input = receipt
        .inputs
        .iter()
        .find(|artifact| artifact.locator == "inputs/assignment.json");
    let assignment = assignment_input.and_then(|artifact| {
        resolve_output_path(root, &artifact.locator)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ProseAssignment>(&bytes).ok())
    });
    let authority_path = resolve_output_path(root, &receipt.authority.locator)?;
    let authority_assignment = fs::read(&authority_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ProseAssignment>(&bytes).ok());
    checks.push(InspectionCheck {
        name: "prose_assignment_identity".to_owned(),
        passed: assignment.as_ref().is_some_and(|assignment| {
            authority_assignment.as_ref().is_some_and(|authority| {
                let mut expected = authority.clone();
                expected.oracle = None;
                assignment.validate().is_ok()
                    && serde_json::to_value(assignment).ok() == serde_json::to_value(expected).ok()
                    && assignment_input.is_some_and(|artifact| {
                        receipt.public_assignment.locator == artifact.locator
                            && receipt.public_assignment.sha256 == artifact.sha256
                    })
            })
        }),
        detail: "author-visible assignment must match the receipt with holdout oracle removed"
            .to_owned(),
    });
    let authority_check = verify_artifact(root, &receipt.authority, "prose assignment authority");
    checks.push(InspectionCheck {
        name: "prose_assignment_authority".to_owned(),
        passed: authority_check.passed
            && authority_assignment.as_ref().is_some_and(|assignment| {
                assignment.validate().is_ok()
                    && assignment.id == receipt.public_assignment.id
                    && assignment.title == receipt.public_assignment.title
                    && assignment.mode == receipt.public_assignment.mode
                    && assignment.coverage == receipt.public_assignment.coverage
            }),
        detail: authority_check.detail,
    });
    let packet_valid = resolve_output_path(root, &receipt.packet.locator)
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<crate::prose_model::PacketBinding>(&bytes).ok())
        .is_some_and(|packet| {
            packet.schema == crate::prose_model::PROSE_PACKET_SCHEMA
                && packet.assignment == receipt.public_assignment
                && packet.inputs.len() == receipt.inputs.len()
                && packet
                    .inputs
                    .iter()
                    .zip(&receipt.inputs)
                    .all(|(left, right)| {
                        left.locator == right.locator
                            && left.sha256 == right.sha256
                            && left.bytes == right.bytes
                    })
        });
    checks.push(InspectionCheck {
        name: "prose_packet_binding".to_owned(),
        passed: packet_valid,
        detail: "base packet must bind the retained assignment and all prepared inputs".to_owned(),
    });

    for artifact in [
        receipt.author_request.as_ref(),
        receipt.review_packet.as_ref(),
        receipt.review_request.as_ref(),
        receipt.author.as_ref().map(|author| &author.submission),
        receipt
            .author
            .as_ref()
            .and_then(|author| author.candidate.as_ref()),
        receipt
            .author
            .as_ref()
            .and_then(|author| author.claim_map.as_ref()),
        receipt.review.as_ref().map(|review| &review.submission),
    ]
    .into_iter()
    .flatten()
    {
        checks.push(verify_artifact(root, artifact, "prose stage"));
    }
    if let Some(author) = &receipt.author {
        for observation in &author.mechanical_observations {
            checks.push(verify_output(
                root,
                &observation.stdout,
                "prose mechanical stdout",
            ));
            checks.push(verify_output(
                root,
                &observation.stderr,
                "prose mechanical stderr",
            ));
        }
    }

    let author_request = receipt.author_request.as_ref().and_then(|artifact| {
        resolve_output_path(root, &artifact.locator)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<AuthorRequest>(&bytes).ok())
    });
    if let Some(request) = &author_request {
        checks.push(verify_artifact(
            root,
            &request.submission_template,
            "author submission template",
        ));
        checks.push(verify_artifact(
            root,
            &request.claim_map_template,
            "claim map template",
        ));
    }
    checks.push(InspectionCheck {
        name: "author_request_binding".to_owned(),
        passed: match (&receipt.author_request, &author_request) {
            (Some(_), Some(request)) => {
                request.schema == AUTHOR_REQUEST_SCHEMA
                    && request.packet_sha256 == receipt.packet.sha256
                    && request.run_id == receipt.run_id
                    && request.assignment_id == receipt.participant_assignment_id
            }
            (None, None) => receipt.public_assignment.mode == ProseMode::Review,
            _ => false,
        },
        detail: "author request must name the exact base packet and generated templates".to_owned(),
    });

    let review_request = receipt.review_request.as_ref().and_then(|artifact| {
        resolve_output_path(root, &artifact.locator)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ReviewRequest>(&bytes).ok())
    });
    let review_export_root = operational_export_root(
        &receipt.run_id,
        receipt.review_export.as_ref(),
        receipt.review.is_some(),
    );
    if let Some(request) = &review_request {
        checks.push(verify_artifact(
            root,
            &request.submission_template,
            "review submission template",
        ));
        for observation in &request.mechanical_observations {
            checks.push(verify_output(
                root,
                &observation.stdout,
                "review packet stdout",
            ));
            checks.push(verify_output(
                root,
                &observation.stderr,
                "review packet stderr",
            ));
            if let Some(export_root) = &review_export_root {
                checks.push(verify_output(
                    export_root,
                    &observation.stdout,
                    "review export stdout",
                ));
                checks.push(verify_output(
                    export_root,
                    &observation.stderr,
                    "review export stderr",
                ));
            }
        }
    }
    let review_binding = receipt.review_packet.as_ref().and_then(|artifact| {
        resolve_output_path(root, &artifact.locator)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ReviewPacketBinding>(&bytes).ok())
    });
    if let Some(binding) = &review_binding {
        for input in &binding.inputs {
            checks.push(verify_artifact(root, input, "review packet input"));
            if let Some(export_root) = &review_export_root {
                checks.push(verify_artifact(export_root, input, "review export input"));
            }
        }
    }
    checks.push(InspectionCheck {
        name: "review_packet_binding".to_owned(),
        passed: match (&receipt.review_packet, &review_request) {
            (Some(packet), Some(request)) => {
                request.schema == REVIEW_REQUEST_SCHEMA
                    && request.review_packet_sha256 == packet.sha256
                    && request.run_id == receipt.run_id
                    && request.assignment_id == receipt.participant_assignment_id
                    && review_binding.as_ref().is_some_and(|binding| {
                        binding.schema == crate::prose_model::PROSE_PACKET_SCHEMA
                            && authority_assignment.as_ref().is_some_and(|authority| {
                                binding.assignment
                                    == review_assignment_projection(
                                        &receipt.participant_assignment_id,
                                        authority,
                                    )
                            })
                            && binding.inputs.iter().any(|input| {
                                input.locator == request.submission_template.locator
                                    && input.sha256 == request.submission_template.sha256
                            })
                    })
            }
            (None, None) => receipt.status == ProseRunStatus::AwaitingAuthor,
            _ => false,
        },
        detail: "review request must name the exact retained review packet".to_owned(),
    });

    let review_submission = receipt.review.as_ref().and_then(|stage| {
        resolve_output_path(root, &stage.submission.locator)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ReviewSubmission>(&bytes).ok())
    });
    let review_submission_valid = match (
        authority_assignment.as_ref().or(assignment.as_ref()),
        &receipt.review,
        &review_submission,
    ) {
        (Some(assignment), Some(stage), Some(submission)) => {
            let expected_candidate = receipt
                .author
                .as_ref()
                .and_then(|author| author.candidate.as_ref())
                .or_else(|| {
                    receipt
                        .inputs
                        .iter()
                        .find(|artifact| artifact.locator.starts_with("inputs/candidate/"))
                })
                .map(|artifact| artifact.sha256.as_str());
            submission.schema == REVIEW_SUBMISSION_SCHEMA
                && submission.validate(assignment).is_ok()
                && submission.run_id == receipt.run_id
                && submission.assignment_id == receipt.participant_assignment_id
                && submission.candidate_sha256.as_deref() == expected_candidate
                && submission.reviewer.id == stage.reviewer.id
                && submission.disposition == stage.disposition
        }
        (_, None, None) => true,
        _ => false,
    };
    checks.push(InspectionCheck {
        name: "review_submission_binding".to_owned(),
        passed: review_submission_valid,
        detail: "review stage must reproduce the strict submission bound to the candidate"
            .to_owned(),
    });
    let oracle_consistent = match (
        authority_assignment
            .as_ref()
            .and_then(|assignment| assignment.oracle.as_ref()),
        receipt.review.as_ref(),
        review_submission.as_ref(),
    ) {
        (Some(oracle), Some(stage), Some(submission)) => {
            let expected = crate::prose::evaluate_oracle(oracle, submission);
            stage.oracle.as_ref().is_some_and(|observed| {
                observed.passed == expected.passed
                    && observed.missing_required_tags == expected.missing_required_tags
                    && observed.present_forbidden_tags == expected.present_forbidden_tags
                    && observed.failed_axis_expectations == expected.failed_axis_expectations
                    && observed.disposition_allowed == expected.disposition_allowed
            })
        }
        (None, Some(stage), Some(_)) => stage.oracle.is_none(),
        (_, None, None) => true,
        _ => false,
    };
    checks.push(InspectionCheck {
        name: "oracle_evaluation".to_owned(),
        passed: oracle_consistent,
        detail: "held-out oracle result must be recomputable from the strict review submission"
            .to_owned(),
    });

    let independent = receipt.review.as_ref().is_none_or(|review| {
        receipt
            .author
            .as_ref()
            .is_none_or(|author| author.author.id != review.reviewer.id)
    });
    checks.push(InspectionCheck {
        name: "reviewer_independence".to_owned(),
        passed: independent,
        detail: "an authoring run cannot use the recorded author identity as reviewer".to_owned(),
    });
    let expected_status = receipt.review.as_ref().map_or_else(
        || {
            if receipt.author.is_some() || receipt.public_assignment.mode == ProseMode::Review {
                ProseRunStatus::AwaitingReview
            } else {
                ProseRunStatus::AwaitingAuthor
            }
        },
        |review| match review.disposition {
            ReviewDisposition::Accept => ProseRunStatus::ReviewedAccept,
            ReviewDisposition::Revise => ProseRunStatus::ReviewedRevise,
            ReviewDisposition::Block => ProseRunStatus::ReviewedBlock,
        },
    );
    checks.push(InspectionCheck {
        name: "prose_status_consistent".to_owned(),
        passed: receipt.status == expected_status
            && receipt.evaluation_complete == receipt.review.is_some(),
        detail: format!(
            "recorded {:?}, recomputed {:?}, evaluation_complete={}",
            receipt.status, expected_status, receipt.evaluation_complete
        ),
    });
    checks.push(binary_identity_check(
        repository,
        &receipt.driver,
        "driver_identity",
    ));
    checks.push(binary_identity_check(
        repository,
        &receipt.tool,
        "tool_identity",
    ));
    let evidence_replayed = checks.iter().all(|check| check.passed);
    Ok(ReceiptInspection {
        schema: INSPECTION_SCHEMA.to_owned(),
        source_schema: PROSE_RECEIPT_SCHEMA.to_owned(),
        source_status: enum_name(receipt.status),
        evidence_replayed,
        authenticity: ReceiptAuthenticity::Unanchored,
        checks,
    })
}

fn operational_export_root(
    run_id: &str,
    export: Option<&ParticipantExport>,
    submission_retained: bool,
) -> Option<PathBuf> {
    if submission_retained {
        None
    } else {
        export.and_then(|export| operational_participant_export_root(run_id, export).ok())
    }
}

fn inspect_prose_suite(
    path: &Path,
    repository: &Path,
    receipt: ProseSuiteReceipt,
) -> Result<ReceiptInspection> {
    let root = path.parent().context("prose suite receipt has no parent")?;
    let mut checks = Vec::new();
    let suite_path = root.join("inputs/prose-suite.json");
    let suite = fs::read(&suite_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ProseSuite>(&bytes).ok());
    checks.push(match sha256_file(&suite_path) {
        Ok((digest, _)) => InspectionCheck {
            name: "prose_suite_input".to_owned(),
            passed: digest == receipt.suite.sha256
                && suite.as_ref().is_some_and(|suite| {
                    suite.validate().is_ok()
                        && suite.id == receipt.suite.id
                        && suite.title == receipt.suite.title
                        && suite.required_coverage == receipt.required_coverage
                }),
            detail: format!("expected {}, observed {digest}", receipt.suite.sha256),
        },
        Err(error) => InspectionCheck {
            name: "prose_suite_input".to_owned(),
            passed: false,
            detail: format!("{error:#}"),
        },
    });
    checks.push(InspectionCheck {
        name: "prose_suite_catalog_locator".to_owned(),
        passed: suite
            .as_ref()
            .is_some_and(|suite| receipt.suite.locator == prose_suite_catalog_locator(&suite.id)),
        detail: suite.as_ref().map_or_else(
            || "retained prose suite could not re-derive its catalog locator".to_owned(),
            |suite| {
                format!(
                    "expected {}, recorded {}",
                    prose_suite_catalog_locator(&suite.id),
                    receipt.suite.locator
                )
            },
        ),
    });
    let terminal = receipt.status != ProseSuiteStatus::Prepared;
    let mut prepared_coverage = BTreeSet::new();
    let mut demonstrated_coverage = BTreeSet::new();
    let mut live_evaluation_complete = true;
    let mut demonstration_failed = false;
    for run in &receipt.runs {
        let check = verify_artifact(root, &run.preparation_receipt, "prepared prose receipt");
        let parsed = resolve_output_path(root, &run.preparation_receipt.locator)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ProseReceipt>(&bytes).ok());
        let identity_matches = parsed.as_ref().is_some_and(|child| {
            prepared_coverage.extend(child.public_assignment.coverage.iter().cloned());
            child.public_assignment.id == run.assignment_id
        });
        checks.push(InspectionCheck {
            name: format!("prose_suite_child:{}", run.assignment_id),
            passed: check.passed && identity_matches,
            detail: format!("{}; identity_matches={identity_matches}", check.detail),
        });
        let current_check = verify_artifact(root, &run.current_receipt, "current prose receipt");
        let live_receipt_path = resolve_prose_suite_child_receipt(root, run).ok();
        let live = live_receipt_path
            .as_ref()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ProseReceipt>(&bytes).ok());
        let live_inspection = live.as_ref().and_then(|child| {
            inspect_prose(live_receipt_path.as_ref()?, repository, child.clone()).ok()
        });
        let live_replayed = live_inspection
            .as_ref()
            .is_some_and(|inspection| inspection.evidence_replayed);
        let same_prepared_run = parsed
            .as_ref()
            .zip(live.as_ref())
            .is_some_and(|(prepared, child)| same_prose_run_identity(prepared, child));
        let current_identity_advanced = live_receipt_path
            .as_ref()
            .and_then(|path| sha256_file(path).ok())
            .is_some_and(|(digest, bytes)| {
                digest != run.current_receipt.sha256 || bytes != run.current_receipt.bytes
            });
        let (current_identity_accepted, pending_suite_update) = suite_current_identity_state(
            terminal,
            current_check.passed,
            current_identity_advanced,
            same_prepared_run,
            live_replayed,
        );
        checks.push(InspectionCheck {
            name: format!("prose_suite_current_receipt:{}", run.assignment_id),
            passed: current_identity_accepted,
            detail: format!(
                "{}; pending_suite_update={pending_suite_update}; same_prepared_run={same_prepared_run}",
                current_check.detail
            ),
        });
        let live_valid = live.as_ref().is_some_and(|child| {
            current_identity_accepted
                && same_prepared_run
                && child.public_assignment.id == run.assignment_id
                && live_replayed
        });
        let reviewed = live_valid
            && !pending_suite_update
            && live
                .as_ref()
                .is_some_and(|child| child.evaluation_complete && child.review.is_some());
        live_evaluation_complete &= reviewed;
        let (status_matches, coverage_status_matches) = if pending_suite_update {
            (true, true)
        } else if let Some(child) = &live
            && live_valid
        {
            let coverage_status = if reviewed {
                let live_root = live_receipt_path
                    .as_deref()
                    .and_then(Path::parent)
                    .context("live prose receipt has no parent")?;
                let authority = resolve_output_path(live_root, &child.authority.locator)
                    .ok()
                    .and_then(|path| fs::read(path).ok())
                    .and_then(|bytes| serde_json::from_slice::<ProseAssignment>(&bytes).ok());
                authority
                    .as_ref()
                    .and_then(|assignment| prose_coverage_status(child, assignment, live_root).ok())
                    .unwrap_or(ProseCoverageStatus::ReviewIncomplete)
            } else {
                ProseCoverageStatus::AwaitingReview
            };
            if reviewed {
                demonstration_failed |= coverage_status != ProseCoverageStatus::Demonstrated;
                if coverage_status == ProseCoverageStatus::Demonstrated {
                    demonstrated_coverage.extend(child.public_assignment.coverage.iter().cloned());
                }
            }
            let coverage_matches = run.coverage_status == coverage_status;
            (
                child.status == run.status && coverage_matches,
                coverage_matches,
            )
        } else {
            (false, false)
        };
        checks.push(InspectionCheck {
            name: format!("prose_suite_live_run:{}", run.assignment_id),
            passed: live_valid && status_matches,
            detail: format!(
                "live receipt {}, evidence_replayed={live_replayed}, status_matches={status_matches}, coverage_status_matches={coverage_status_matches}, current_check=({}), pending_suite_update={pending_suite_update}",
                live_receipt_path
                    .as_ref()
                    .map_or_else(|| "<invalid locator>".to_owned(), |path| path.display().to_string()),
                current_check.detail,
            ),
        });
    }
    let recorded_prepared = receipt
        .prepared_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let recorded_demonstrated = receipt
        .demonstrated_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let required = receipt
        .required_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    checks.push(InspectionCheck {
        name: "prose_suite_prepared_coverage".to_owned(),
        passed: recorded_prepared == prepared_coverage && required.is_subset(&prepared_coverage),
        detail: format!(
            "required {:?}, recorded {:?}, prepared {:?}",
            required, recorded_prepared, prepared_coverage
        ),
    });
    let expected_status = prose_suite_status_from_live_children(
        live_evaluation_complete,
        demonstration_failed,
        &required,
        &demonstrated_coverage,
    );
    checks.push(InspectionCheck {
        name: "prose_suite_demonstrated_coverage".to_owned(),
        passed: recorded_demonstrated == demonstrated_coverage
            && (receipt.status != ProseSuiteStatus::Passed
                || required.is_subset(&demonstrated_coverage)),
        detail: format!(
            "recorded {:?}, demonstrated {:?}",
            recorded_demonstrated, demonstrated_coverage
        ),
    });
    checks.push(InspectionCheck {
        name: "prose_suite_status".to_owned(),
        passed: receipt.status == expected_status
            && receipt.evaluation_complete == live_evaluation_complete,
        detail: format!(
            "recorded {:?}/complete={}, recomputed {:?}/complete={}",
            receipt.status, receipt.evaluation_complete, expected_status, live_evaluation_complete
        ),
    });
    checks.push(binary_identity_check(
        repository,
        &receipt.driver,
        "driver_identity",
    ));
    let evidence_replayed = checks.iter().all(|check| check.passed);
    Ok(ReceiptInspection {
        schema: INSPECTION_SCHEMA.to_owned(),
        source_schema: PROSE_SUITE_RECEIPT_SCHEMA.to_owned(),
        source_status: enum_name(receipt.status),
        evidence_replayed,
        authenticity: ReceiptAuthenticity::Unanchored,
        checks,
    })
}

fn suite_current_identity_state(
    terminal: bool,
    current_identity_matches: bool,
    current_identity_advanced: bool,
    same_prepared_run: bool,
    live_replayed: bool,
) -> (bool, bool) {
    let pending_suite_update = !terminal
        && !current_identity_matches
        && current_identity_advanced
        && same_prepared_run
        && live_replayed;
    (
        current_identity_matches || pending_suite_update,
        pending_suite_update,
    )
}

fn prose_suite_status_from_live_children(
    evaluation_complete: bool,
    demonstration_failed: bool,
    required: &BTreeSet<String>,
    demonstrated: &BTreeSet<String>,
) -> ProseSuiteStatus {
    if !evaluation_complete {
        ProseSuiteStatus::Prepared
    } else if demonstration_failed || !required.is_subset(demonstrated) {
        ProseSuiteStatus::Failed
    } else {
        ProseSuiteStatus::Passed
    }
}

fn same_prose_run_identity(prepared: &ProseReceipt, live: &ProseReceipt) -> bool {
    prepared.schema == live.schema
        && prepared.run_id == live.run_id
        && prepared.participant_assignment_id == live.participant_assignment_id
        && prepared.driver == live.driver
        && prepared.public_assignment == live.public_assignment
        && prepared.tool == live.tool
        && prepared.created_at_unix_ms == live.created_at_unix_ms
        && prepared.authority == live.authority
        && prepared.packet == live.packet
        && prepared.inputs == live.inputs
}

fn binary_identity_check(
    repository: &Path,
    identity: &crate::model::ToolIdentity,
    name: &str,
) -> InspectionCheck {
    let result = resolve_recorded_identity_path(repository, identity, name).and_then(|path| {
        let (digest, _) = sha256_file(&path)?;
        Ok((path, digest))
    });
    match result {
        Ok((path, digest)) => InspectionCheck {
            name: name.to_owned(),
            passed: digest == identity.sha256,
            detail: format!(
                "expected {} ({}), observed {digest} at {}",
                identity.sha256,
                identity.version,
                path.display()
            ),
        },
        Err(error) => InspectionCheck {
            name: name.to_owned(),
            passed: false,
            detail: format!("{error:#}"),
        },
    }
}

fn enum_name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn verify_artifact(root: &Path, artifact: &ArtifactIdentity, kind: &str) -> InspectionCheck {
    let result = resolve_output_path(root, &artifact.locator).and_then(|path| {
        let (digest, bytes) = sha256_file(&path)?;
        Ok((path, digest, bytes))
    });
    match result {
        Ok((path, digest, bytes)) => InspectionCheck {
            name: format!("{kind}:{}", artifact.locator),
            passed: digest == artifact.sha256 && bytes == artifact.bytes,
            detail: format!(
                "expected {} / {} bytes, observed {digest} / {bytes} bytes at {}",
                artifact.sha256,
                artifact.bytes,
                path.display()
            ),
        },
        Err(error) => InspectionCheck {
            name: format!("{kind}:{}", artifact.locator),
            passed: false,
            detail: format!("{error:#}"),
        },
    }
}

fn verify_output(
    root: &Path,
    output: &crate::model::OutputArtifact,
    kind: &str,
) -> InspectionCheck {
    let result = resolve_output_path(root, &output.locator).and_then(|path| {
        let (digest, bytes) = sha256_file(&path)?;
        Ok((path, digest, bytes))
    });
    match result {
        Ok((path, digest, bytes)) => {
            let shape = output.observed_bytes >= output.stored_bytes
                && output.truncated == (output.observed_bytes > output.stored_bytes);
            let full_digest_bound = output.truncated || output.sha256 == digest;
            InspectionCheck {
                name: format!("{kind}:{}", output.locator),
                passed: digest == output.stored_sha256
                    && bytes == output.stored_bytes
                    && shape
                    && full_digest_bound,
                detail: format!(
                    "stored digest {digest}, bytes {bytes}, observed {}, truncated {}, path {}",
                    output.observed_bytes,
                    output.truncated,
                    path.display()
                ),
            }
        }
        Err(error) => InspectionCheck {
            name: format!("{kind}:{}", output.locator),
            passed: false,
            detail: format!("{error:#}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{DRIVER_BINARY_TOKEN, current_driver_identity};
    use crate::model::{
        CommandExpectation, CoverageBinding, JsonScalarCapture, OutputArtifact,
        ScenarioEnvironment, ScenarioIdentity, ScenarioKind, SuiteIdentity, SuiteRunEntry,
        ToolIdentity,
    };
    use crate::prose_model::ProseAssignmentIdentity;

    fn tool_identity() -> ToolIdentity {
        ToolIdentity {
            locator: "tool".to_owned(),
            sha256: "0".repeat(64),
            version: "test".to_owned(),
        }
    }

    #[test]
    fn binary_identity_check_resolves_the_external_driver_token() {
        let repository = tempfile::tempdir().expect("external repository");
        let identity = current_driver_identity(repository.path()).expect("driver identity");
        assert_eq!(identity.locator, DRIVER_BINARY_TOKEN);

        let check = binary_identity_check(repository.path(), &identity, "driver_identity");
        assert!(check.passed, "{}", check.detail);
    }

    fn command_step(id: &str) -> ScenarioStep {
        ScenarioStep::Command {
            id: id.to_owned(),
            argv: vec!["tool".to_owned()],
            cwd: None,
            timeout_ms: None,
            environment: Default::default(),
            captures: Vec::new(),
            expect: CommandExpectation {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                files: Vec::new(),
            },
        }
    }

    fn scenario(steps: &[&str], coverage: Vec<CoverageBinding>) -> ScenarioManifest {
        ScenarioManifest {
            schema: crate::model::SCENARIO_SCHEMA.to_owned(),
            id: "scenario".to_owned(),
            title: "Scenario".to_owned(),
            description: "Test scenario".to_owned(),
            kind: ScenarioKind::Mechanical,
            environment: ScenarioEnvironment::Isolated,
            timeout_ms: 1_000,
            coverage,
            requirements: Vec::new(),
            mediawiki_fixture: None,
            steps: steps.iter().map(|id| command_step(id)).collect(),
        }
    }

    fn step(id: &str, status: RunStatus) -> StepReceipt {
        StepReceipt {
            id: id.to_owned(),
            action: "command".to_owned(),
            status,
            duration_ms: 1,
            argv: vec!["tool".to_owned()],
            exit_code: Some(0),
            timed_out: Some(false),
            stdout: None,
            stderr: None,
            assertions: Vec::new(),
            captures: Vec::new(),
            copied: None,
            observation: None,
            failure: None,
        }
    }

    fn run_receipt(status: RunStatus, steps: Vec<StepReceipt>) -> RunReceipt {
        RunReceipt {
            schema: RECEIPT_SCHEMA.to_owned(),
            run_id: "run".to_owned(),
            driver: tool_identity(),
            scenario: ScenarioIdentity {
                id: "scenario".to_owned(),
                title: "Scenario".to_owned(),
                kind: ScenarioKind::Mechanical,
                environment: ScenarioEnvironment::Isolated,
                coverage: Vec::new(),
                locator: "inputs/scenario.json".to_owned(),
                sha256: "0".repeat(64),
            },
            tool: tool_identity(),
            status,
            started_at_unix_ms: 0,
            finished_at_unix_ms: 1,
            duration_ms: 1,
            complete: true,
            output_truncated: false,
            inputs: Vec::new(),
            requirements: Vec::new(),
            steps,
            failure: None,
        }
    }

    fn prose_receipt(status: ProseRunStatus, evaluation_complete: bool) -> ProseReceipt {
        let artifact = |locator: &str| ArtifactIdentity {
            locator: locator.to_owned(),
            sha256: "a".repeat(64),
            bytes: 1,
        };
        ProseReceipt {
            schema: PROSE_RECEIPT_SCHEMA.to_owned(),
            run_id: "prose-run-1".to_owned(),
            participant_assignment_id: "case-1".to_owned(),
            driver: tool_identity(),
            public_assignment: ProseAssignmentIdentity {
                id: "case".to_owned(),
                title: "Case".to_owned(),
                mode: ProseMode::Review,
                coverage: vec!["controlled-review".to_owned()],
                locator: "inputs/assignment.json".to_owned(),
                sha256: "b".repeat(64),
            },
            tool: tool_identity(),
            status,
            created_at_unix_ms: 1,
            updated_at_unix_ms: if evaluation_complete { 2 } else { 1 },
            evaluation_complete,
            authority: artifact("holdout/assignment.json"),
            packet: artifact("inputs/packet.json"),
            inputs: vec![artifact("inputs/assignment.json")],
            author_request: None,
            author_export: None,
            author: None,
            review_request: None,
            review_export: None,
            review_packet: None,
            review: None,
        }
    }

    fn write_identity(root: &Path, locator: &str, bytes: &[u8]) -> ArtifactIdentity {
        let path = root.join(locator);
        fs::create_dir_all(path.parent().expect("artifact parent")).expect("artifact directory");
        fs::write(&path, bytes).expect("artifact bytes");
        ArtifactIdentity {
            locator: locator.to_owned(),
            sha256: sha256_bytes(bytes),
            bytes: bytes.len() as u64,
        }
    }

    #[test]
    fn unsupported_receipts_are_rejected() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("receipt.json");
        fs::write(&path, br#"{"schema":"unknown"}"#).expect("write");
        assert!(inspect_receipt(&path, directory.path()).is_err());
    }

    #[test]
    fn retained_submission_removes_external_export_from_replay_dependencies() {
        let export = ParticipantExport {
            root: "wikitest:participant-export:reviewer".to_owned(),
            request: ArtifactIdentity {
                locator: "request.json".to_owned(),
                sha256: "0".repeat(64),
                bytes: 0,
            },
            output_directory: "output".to_owned(),
        };
        assert_eq!(
            operational_export_root("run-1", Some(&export), false),
            Some(
                std::env::temp_dir()
                    .join("wikitest-participant-exports")
                    .join("run-1-reviewer")
            )
        );
        assert_eq!(operational_export_root("run-1", Some(&export), true), None);
    }

    #[test]
    fn passed_receipt_cannot_omit_declared_steps() {
        let manifest = scenario(&["execute"], Vec::new());
        let receipt = run_receipt(RunStatus::Passed, Vec::new());

        assert!(!steps_form_valid_prefix(&manifest, &receipt));
    }

    #[test]
    fn receipt_cannot_omit_a_declared_requirement() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut manifest = scenario(&["execute"], Vec::new());
        manifest.requirements = vec![crate::model::Requirement::PathExists {
            path: "{HOST_ROOT}/.wikitool/data/wikitool.db".to_owned(),
            on_missing: crate::model::MissingDisposition::Skip,
        }];
        let mut receipt = run_receipt(RunStatus::Skipped, Vec::new());
        assert!(!requirements_match(directory.path(), &manifest, &receipt));
        let observation = RequirementObservation {
            schema: REQUIREMENT_OBSERVATION_SCHEMA.to_owned(),
            kind: "path_exists".to_owned(),
            declared_path: "{HOST_ROOT}/.wikitool/data/wikitool.db".to_owned(),
            expanded_path: "host/.wikitool/data/wikitool.db".to_owned(),
            exists: false,
            path_kind: None,
        };
        let bytes = serde_json::to_vec_pretty(&observation).expect("observation JSON");
        let observation_path = directory.path().join("requirements/0000-path-exists.json");
        fs::create_dir_all(observation_path.parent().expect("parent"))
            .expect("requirements directory");
        fs::write(&observation_path, &bytes).expect("observation");
        receipt.requirements.push(crate::model::RequirementReceipt {
            kind: "path_exists".to_owned(),
            passed: false,
            disposition: crate::model::MissingDisposition::Skip,
            detail: "path is missing: host/.wikitool/data/wikitool.db".to_owned(),
            observation: ArtifactIdentity {
                locator: "requirements/0000-path-exists.json".to_owned(),
                sha256: sha256_bytes(&bytes),
                bytes: bytes.len() as u64,
            },
        });
        assert!(requirements_match(directory.path(), &manifest, &receipt));
        receipt.requirements[0].passed = true;
        receipt.requirements[0].detail = "path exists: host/.wikitool/data/wikitool.db".to_owned();
        assert!(!requirements_match(directory.path(), &manifest, &receipt));
    }

    #[test]
    fn flipped_assertion_outcome_is_not_replayable() {
        let expected = AssertionReceipt {
            target: "stdout".to_owned(),
            assertion: "contains".to_owned(),
            passed: false,
            detail: "expected evaluation".to_owned(),
            file_evidence: None,
        };
        let mut forged = expected.clone();
        forged.passed = true;
        forged.detail = "forged".to_owned();

        assert!(!assertion_outcomes_match(&[expected], &[forged]));
    }

    #[test]
    fn captured_value_and_source_hash_are_replayed_from_retained_stdout() {
        let directory = tempfile::tempdir().expect("tempdir");
        let stdout_bytes = br#"{"report":{"plan_id":"plan-123"}}"#;
        let stderr_bytes = b"";
        fs::write(directory.path().join("stdout.json"), stdout_bytes).expect("stdout");
        fs::write(directory.path().join("stderr.txt"), stderr_bytes).expect("stderr");
        let output = |locator: &str, bytes: &[u8]| OutputArtifact {
            locator: locator.to_owned(),
            sha256: sha256_bytes(bytes),
            stored_sha256: sha256_bytes(bytes),
            observed_bytes: bytes.len() as u64,
            stored_bytes: bytes.len() as u64,
            truncated: false,
        };
        let declared = ScenarioStep::Command {
            id: "preview".to_owned(),
            argv: vec!["push".to_owned()],
            cwd: None,
            timeout_ms: None,
            environment: Default::default(),
            captures: vec![JsonScalarCapture {
                name: "PLAN_ID".to_owned(),
                pointer: "/report/plan_id".to_owned(),
            }],
            expect: CommandExpectation {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                files: Vec::new(),
            },
        };
        let mut recorded = StepReceipt {
            id: "preview".to_owned(),
            action: "command".to_owned(),
            status: RunStatus::Passed,
            duration_ms: 1,
            argv: vec!["push".to_owned()],
            exit_code: Some(0),
            timed_out: Some(false),
            stdout: Some(output("stdout.json", stdout_bytes)),
            stderr: Some(output("stderr.txt", stderr_bytes)),
            assertions: Vec::new(),
            captures: Vec::new(),
            copied: None,
            observation: None,
            failure: None,
        };
        let ScenarioStep::Command {
            id,
            captures,
            expect,
            ..
        } = &declared
        else {
            unreachable!();
        };
        let (assertions, replayed_captures) =
            replay_command_assertions(directory.path(), id, captures, expect, &recorded)
                .expect("replay capture");
        recorded.assertions = assertions;
        recorded.captures = replayed_captures;
        assert!(replay_step(directory.path(), &declared, &recorded).expect("valid replay"));

        recorded.captures[0].value = "forged-plan".to_owned();
        assert!(!replay_step(directory.path(), &declared, &recorded).expect("forged replay"));
        recorded.captures[0].value = "plan-123".to_owned();
        recorded.captures[0].source_stdout_sha256 = "0".repeat(64);
        assert!(!replay_step(directory.path(), &declared, &recorded).expect("forged hash"));
    }

    #[test]
    fn mutated_retained_artifact_fails_replay() {
        let directory = tempfile::tempdir().expect("tempdir");
        fs::write(directory.path().join("artifact.bin"), b"mutated").expect("write");
        let identity = ArtifactIdentity {
            locator: "artifact.bin".to_owned(),
            sha256: sha256_bytes(b"original"),
            bytes: 8,
        };

        assert!(!verify_artifact(directory.path(), &identity, "test").passed);
    }

    #[test]
    fn prepared_suite_accepts_pending_child_advancement_until_evaluation_refresh() {
        let original = tempfile::tempdir().expect("original suite");
        let prepared = prose_receipt(ProseRunStatus::AwaitingReview, false);
        let prepared_bytes = serde_json::to_vec_pretty(&prepared).expect("prepared receipt");
        let preparation_identity = write_identity(
            original.path(),
            "prepared/case.receipt.json",
            &prepared_bytes,
        );
        let mut current_identity = write_identity(
            original.path(),
            "runs/prose-run-1/receipt.json",
            &prepared_bytes,
        );
        assert_eq!(
            suite_current_identity_state(true, true, false, true, true),
            (true, false),
            "an exact evaluated child identity is replayable"
        );

        let reviewed = prose_receipt(ProseRunStatus::ReviewedAccept, true);
        assert!(same_prose_run_identity(&prepared, &reviewed));
        let reviewed_bytes = serde_json::to_vec_pretty(&reviewed).expect("reviewed receipt");
        fs::write(
            original.path().join(&current_identity.locator),
            &reviewed_bytes,
        )
        .expect("submit-review advances live child");
        let stale_check = verify_artifact(original.path(), &current_identity, "current receipt");
        assert!(!stale_check.passed);
        assert_eq!(
            suite_current_identity_state(false, false, true, true, true),
            (true, true),
            "a prepared suite must expose a replayed child advance as a pending suite update"
        );
        assert_eq!(
            suite_current_identity_state(true, false, true, true, true),
            (false, false),
            "an evaluated suite cannot retain the stale child identity"
        );

        current_identity = write_identity(
            original.path(),
            "runs/prose-run-1/receipt.json",
            &reviewed_bytes,
        );
        assert!(verify_artifact(original.path(), &current_identity, "current receipt").passed);

        let relocated = tempfile::tempdir().expect("relocated suite");
        write_identity(
            relocated.path(),
            &preparation_identity.locator,
            &prepared_bytes,
        );
        write_identity(relocated.path(), &current_identity.locator, &reviewed_bytes);
        assert!(
            verify_artifact(relocated.path(), &preparation_identity, "prepared receipt").passed
        );
        assert!(verify_artifact(relocated.path(), &current_identity, "current receipt").passed);
        assert_eq!(
            suite_current_identity_state(true, true, false, true, true),
            (true, false),
            "evaluate-suite refresh remains replayable after relocation"
        );

        let required =
            BTreeSet::from(["controlled-review".to_owned(), "source-fidelity".to_owned()]);
        let partial = BTreeSet::from(["controlled-review".to_owned()]);
        assert_eq!(
            prose_suite_status_from_live_children(false, false, &required, &partial),
            ProseSuiteStatus::Prepared,
            "evaluated child coverage remains recorded while another child is incomplete"
        );
        assert_eq!(
            prose_suite_status_from_live_children(true, false, &required, &required),
            ProseSuiteStatus::Passed,
            "all refreshed children may close the suite"
        );
    }

    #[test]
    fn coverage_requires_every_bound_step_to_pass() {
        let binding = CoverageBinding {
            capability: "sync".to_owned(),
            steps: vec!["pull".to_owned(), "push".to_owned()],
        };
        let mut receipt = run_receipt(
            RunStatus::Failed,
            vec![
                step("pull", RunStatus::Passed),
                step("push", RunStatus::Failed),
            ],
        );
        receipt.scenario.coverage = vec![binding];
        assert!(successful_coverage(&receipt).is_empty());

        receipt.steps[1].status = RunStatus::Passed;
        assert_eq!(
            successful_coverage(&receipt),
            BTreeSet::from(["sync".to_owned()])
        );
    }

    #[test]
    fn suite_membership_is_exact_and_ordered() {
        let manifest = SuiteManifest {
            schema: crate::model::SUITE_SCHEMA.to_owned(),
            id: "suite".to_owned(),
            title: "Suite".to_owned(),
            required_coverage: Vec::new(),
            scenarios: vec!["a/scenario.json".to_owned(), "b/scenario.json".to_owned()],
        };
        let mut receipt = SuiteReceipt {
            schema: SUITE_RECEIPT_SCHEMA.to_owned(),
            run_id: "suite-run".to_owned(),
            driver: tool_identity(),
            suite: SuiteIdentity {
                id: "suite".to_owned(),
                title: "Suite".to_owned(),
                locator: "inputs/suite.json".to_owned(),
                sha256: "0".repeat(64),
            },
            status: RunStatus::Passed,
            require_all: true,
            started_at_unix_ms: 0,
            finished_at_unix_ms: 1,
            duration_ms: 1,
            complete: true,
            required_coverage: Vec::new(),
            observed_coverage: Vec::new(),
            runs: vec![
                SuiteRunEntry {
                    scenario: "a/scenario.json".to_owned(),
                    scenario_id: None,
                    status: RunStatus::Passed,
                    receipt_locator: None,
                    receipt_sha256: None,
                    error: None,
                },
                SuiteRunEntry {
                    scenario: "b/scenario.json".to_owned(),
                    scenario_id: None,
                    status: RunStatus::Passed,
                    receipt_locator: None,
                    receipt_sha256: None,
                    error: None,
                },
            ],
        };
        assert!(suite_membership_matches(&manifest, &receipt));

        receipt.runs.pop();
        assert!(!suite_membership_matches(&manifest, &receipt));
    }
}
