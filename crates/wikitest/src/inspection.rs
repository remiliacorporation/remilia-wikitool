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

pub const INSPECTION_SCHEMA: &str = "wikitest.receipt-inspection.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptInspection {
    pub schema: String,
    pub source_schema: String,
    pub source_status: RunStatus,
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
        source_status: receipt.status,
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
        source_status: receipt.status,
        verified,
        checks,
    })
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
