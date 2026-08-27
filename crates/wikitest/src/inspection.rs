use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::artifact::{resolve_output_path, sha256_file};
use crate::model::{
    ArtifactIdentity, RECEIPT_SCHEMA, RunReceipt, RunStatus, SUITE_RECEIPT_SCHEMA,
    ScenarioManifest, SuiteManifest, SuiteReceipt,
};
use crate::prose_model::{
    AUTHOR_REQUEST_SCHEMA, AuthorRequest, PROSE_RECEIPT_SCHEMA, PROSE_SUITE_RECEIPT_SCHEMA,
    ProseAssignment, ProseMode, ProseReceipt, ProseRunStatus, ProseSuite, ProseSuiteReceipt,
    REVIEW_REQUEST_SCHEMA, REVIEW_SUBMISSION_SCHEMA, ReviewDisposition, ReviewPacketBinding,
    ReviewRequest, ReviewSubmission,
};

pub const INSPECTION_SCHEMA: &str = "wikitest.receipt-inspection.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptInspection {
    pub schema: String,
    pub source_schema: String,
    pub source_status: String,
    pub verified: bool,
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
    }
    let tool_path = {
        let candidate = PathBuf::from(&receipt.tool.locator);
        if candidate.is_absolute() {
            candidate
        } else {
            repository.join(candidate)
        }
    };
    checks.push(match sha256_file(&tool_path) {
        Ok((digest, _)) => InspectionCheck {
            name: "tool_identity".to_owned(),
            passed: digest == receipt.tool.sha256,
            detail: format!(
                "expected {}, observed {} at {}",
                receipt.tool.sha256,
                digest,
                tool_path.display()
            ),
        },
        Err(error) => InspectionCheck {
            name: "tool_identity".to_owned(),
            passed: false,
            detail: format!("{error:#}"),
        },
    });
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
    let status_consistent = match receipt.status {
        RunStatus::Passed => {
            receipt
                .steps
                .iter()
                .all(|step| step.status == RunStatus::Passed)
                && receipt.failure.is_none()
        }
        RunStatus::Failed => {
            receipt
                .steps
                .iter()
                .any(|step| step.status == RunStatus::Failed)
                || receipt.requirements.iter().any(|requirement| {
                    !requirement.passed
                        && requirement.disposition == crate::model::MissingDisposition::Fail
                })
        }
        RunStatus::Skipped => receipt.requirements.iter().any(|requirement| {
            !requirement.passed && requirement.disposition == crate::model::MissingDisposition::Skip
        }),
        RunStatus::Error => receipt.failure.is_some(),
    };
    checks.push(InspectionCheck {
        name: "status_consistent".to_owned(),
        passed: status_consistent,
        detail: format!("source status is {:?}", receipt.status),
    });
    let verified = checks.iter().all(|check| check.passed);
    Ok(ReceiptInspection {
        schema: INSPECTION_SCHEMA.to_owned(),
        source_schema: RECEIPT_SCHEMA.to_owned(),
        source_status: enum_name(receipt.status),
        verified,
        checks,
    })
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
        name: "suite_identity".to_owned(),
        passed: suite_manifest.as_ref().is_some_and(|manifest| {
            manifest.id == receipt.suite.id
                && manifest.title == receipt.suite.title
                && manifest.required_coverage == receipt.required_coverage
        }),
        detail: "receipt identity must equal the retained strict suite manifest".to_owned(),
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
                if let Some(child) = &parsed {
                    observed_coverage.extend(child.scenario.coverage.iter().cloned());
                }
                child_complete &= parsed.as_ref().is_some_and(|child| child.complete);
                InspectionCheck {
                    name: format!("child_receipt:{}", run.scenario),
                    passed: Some(&digest) == run.receipt_sha256.as_ref()
                        && identity_matches
                        && child_inspection
                            .as_ref()
                            .is_some_and(|inspection| inspection.verified),
                    detail: format!(
                        "expected digest {}, observed {digest}, identity_matches={identity_matches}, artifacts_verified={}",
                        run.receipt_sha256.as_deref().unwrap_or("<missing>"),
                        child_inspection
                            .as_ref()
                            .is_some_and(|inspection| inspection.verified)
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
    let verified = checks.iter().all(|check| check.passed);
    Ok(ReceiptInspection {
        schema: INSPECTION_SCHEMA.to_owned(),
        source_schema: SUITE_RECEIPT_SCHEMA.to_owned(),
        source_status: enum_name(receipt.status),
        verified,
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
    checks.push(InspectionCheck {
        name: "prose_assignment_identity".to_owned(),
        passed: assignment.as_ref().is_some_and(|assignment| {
            assignment.validate().is_ok()
                && assignment.id == receipt.assignment.id
                && assignment.title == receipt.assignment.title
                && assignment.mode == receipt.assignment.mode
                && assignment.coverage == receipt.assignment.coverage
                && assignment.oracle.is_none()
        }),
        detail: "reviewer-visible assignment must match the receipt with holdout oracle removed"
            .to_owned(),
    });
    let authority_path = resolve_output_path(root, &receipt.authority.locator)?;
    let authority_assignment = fs::read(&authority_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ProseAssignment>(&bytes).ok());
    let authority_check = verify_artifact(root, &receipt.authority, "prose assignment authority");
    checks.push(InspectionCheck {
        name: "prose_assignment_authority".to_owned(),
        passed: authority_check.passed
            && receipt.authority.sha256 == receipt.assignment.sha256
            && authority_assignment.as_ref().is_some_and(|assignment| {
                assignment.validate().is_ok()
                    && assignment.id == receipt.assignment.id
                    && assignment.mode == receipt.assignment.mode
            }),
        detail: authority_check.detail,
    });
    let packet_valid = resolve_output_path(root, &receipt.packet.locator)
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<crate::prose_model::PacketBinding>(&bytes).ok())
        .is_some_and(|packet| {
            packet.schema == crate::prose_model::PROSE_PACKET_SCHEMA
                && packet.assignment.id == receipt.assignment.id
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
                    && request.assignment_id == receipt.assignment.id
            }
            (None, None) => receipt.assignment.mode == ProseMode::Review,
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
    let review_export_root = receipt.review_request.as_ref().and_then(|artifact| {
        resolve_output_path(root, &artifact.locator)
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
    });
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
                    && request.assignment_id == receipt.assignment.id
                    && review_binding.as_ref().is_some_and(|binding| {
                        binding.schema == crate::prose_model::PROSE_PACKET_SCHEMA
                            && binding.assignment.id == receipt.assignment.id
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
                && submission.assignment_id == receipt.assignment.id
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
            if receipt.author.is_some() || receipt.assignment.mode == ProseMode::Review {
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
    checks.push(tool_identity_check(repository, &receipt.tool));
    let verified = checks.iter().all(|check| check.passed);
    Ok(ReceiptInspection {
        schema: INSPECTION_SCHEMA.to_owned(),
        source_schema: PROSE_RECEIPT_SCHEMA.to_owned(),
        source_status: enum_name(receipt.status),
        verified,
        checks,
    })
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
    let mut observed_coverage = BTreeSet::new();
    for run in &receipt.runs {
        let check = verify_artifact(root, &run.preparation_receipt, "prepared prose receipt");
        let parsed = resolve_output_path(root, &run.preparation_receipt.locator)
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ProseReceipt>(&bytes).ok());
        let identity_matches = parsed.as_ref().is_some_and(|child| {
            observed_coverage.extend(child.assignment.coverage.iter().cloned());
            child.assignment.id == run.assignment_id && child.status == run.status
        });
        checks.push(InspectionCheck {
            name: format!("prose_suite_child:{}", run.assignment_id),
            passed: check.passed && identity_matches,
            detail: format!("{}; identity_matches={identity_matches}", check.detail),
        });
        let live_receipt_path = PathBuf::from(&run.run_locator).join("receipt.json");
        let live = fs::read(&live_receipt_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ProseReceipt>(&bytes).ok());
        let live_inspection = live
            .as_ref()
            .and_then(|child| inspect_prose(&live_receipt_path, repository, child.clone()).ok());
        checks.push(InspectionCheck {
            name: format!("prose_suite_live_run:{}", run.assignment_id),
            passed: live.as_ref().is_some_and(|child| {
                child.assignment.id == run.assignment_id
                    && live_inspection
                        .as_ref()
                        .is_some_and(|inspection| inspection.verified)
            }),
            detail: format!(
                "live receipt {}, artifacts_verified={}",
                live_receipt_path.display(),
                live_inspection
                    .as_ref()
                    .is_some_and(|inspection| inspection.verified)
            ),
        });
    }
    let recorded = receipt
        .observed_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let required = receipt
        .required_coverage
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    checks.push(InspectionCheck {
        name: "prose_suite_coverage".to_owned(),
        passed: recorded == observed_coverage && required.is_subset(&observed_coverage),
        detail: format!(
            "required {:?}, recorded {:?}, observed {:?}",
            required, recorded, observed_coverage
        ),
    });
    let verified = checks.iter().all(|check| check.passed);
    Ok(ReceiptInspection {
        schema: INSPECTION_SCHEMA.to_owned(),
        source_schema: PROSE_SUITE_RECEIPT_SCHEMA.to_owned(),
        source_status: "prepared".to_owned(),
        verified,
        checks,
    })
}

fn tool_identity_check(repository: &Path, tool: &crate::model::ToolIdentity) -> InspectionCheck {
    let candidate = PathBuf::from(&tool.locator);
    let path = if candidate.is_absolute() {
        candidate
    } else {
        repository.join(candidate)
    };
    match sha256_file(&path) {
        Ok((digest, _)) => InspectionCheck {
            name: "tool_identity".to_owned(),
            passed: digest == tool.sha256,
            detail: format!(
                "expected {}, observed {digest} at {}",
                tool.sha256,
                path.display()
            ),
        },
        Err(error) => InspectionCheck {
            name: "tool_identity".to_owned(),
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
            InspectionCheck {
                name: format!("{kind}:{}", output.locator),
                passed: digest == output.stored_sha256 && bytes == output.stored_bytes && shape,
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

    #[test]
    fn unsupported_receipts_are_rejected() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("receipt.json");
        fs::write(&path, br#"{"schema":"unknown"}"#).expect("write");
        assert!(inspect_receipt(&path, directory.path()).is_err());
    }
}
