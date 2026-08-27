use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::artifact::{
    atomic_write, atomic_write_json, portable, relative_locator, resolve_existing_plain_file,
    resolve_output_path, sha256_bytes, sha256_file, unix_ms,
};
use crate::catalog::{Manifest, load_manifest, resolve_manifest};
use crate::identity::current_driver_identity;
use crate::model::{
    ArtifactIdentity, AssertionReceipt, FileAssertion, MissingDisposition, OutputArtifact,
    OutputAssertion, RECEIPT_SCHEMA, Requirement, RequirementReceipt, RunReceipt, RunStatus,
    SUITE_RECEIPT_SCHEMA, ScenarioEnvironment, ScenarioIdentity, ScenarioManifest, ScenarioStep,
    StepReceipt, SuiteIdentity, SuiteReceipt, SuiteRunEntry, ToolIdentity,
};
use crate::process::{CapturedStream, probe_version, run_bounded};

const DEFAULT_OUTPUT_BUDGET: usize = 2 * 1024 * 1024;
const FILE_ASSERTION_BUDGET: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub repository: PathBuf,
    pub artifacts_root: PathBuf,
    pub host_root: Option<PathBuf>,
    pub wikitool: PathBuf,
    pub catalogs: Vec<PathBuf>,
    pub maximum_output_bytes: usize,
}

impl RunOptions {
    pub fn new(repository: PathBuf, artifacts_root: PathBuf, wikitool: PathBuf) -> Self {
        let default_catalog = repository.join("wikitest");
        Self {
            repository,
            artifacts_root,
            host_root: None,
            wikitool,
            catalogs: vec![default_catalog],
            maximum_output_bytes: DEFAULT_OUTPUT_BUDGET,
        }
    }
}

#[derive(Debug)]
pub struct CompletedRun {
    pub receipt: RunReceipt,
    pub receipt_path: PathBuf,
    pub run_directory: PathBuf,
}

#[derive(Debug)]
pub struct CompletedSuite {
    pub receipt: SuiteReceipt,
    pub receipt_path: PathBuf,
    pub run_directory: PathBuf,
}

pub fn run_scenario(path: &Path, options: &RunOptions) -> Result<CompletedRun> {
    let (manifest, scenario_bytes) = load_manifest(path)?;
    let Manifest::Scenario(scenario) = manifest else {
        bail!("{} is not a scenario", path.display());
    };
    run_loaded_scenario(path, &scenario, &scenario_bytes, options)
}

fn run_loaded_scenario(
    scenario_path: &Path,
    scenario: &ScenarioManifest,
    scenario_bytes: &[u8],
    options: &RunOptions,
) -> Result<CompletedRun> {
    scenario.validate()?;
    if options.maximum_output_bytes == 0 {
        bail!("maximum output bytes must be nonzero");
    }
    let started_at = unix_ms()?;
    let started = Instant::now();
    let (run_id, run_directory) = create_run_directory(
        &options.artifacts_root,
        &scenario.id,
        started_at,
        std::process::id(),
    )?;
    let receipt_path = run_directory.join("receipt.json");
    let inputs_directory = run_directory.join("inputs");
    let steps_directory = run_directory.join("steps");
    fs::create_dir_all(&inputs_directory)?;
    fs::create_dir_all(&steps_directory)?;

    let scenario_input = inputs_directory.join("scenario.json");
    atomic_write(&scenario_input, scenario_bytes)?;
    let scenario_sha256 = sha256_bytes(scenario_bytes);
    let scenario_locator = scenario_path
        .strip_prefix(&options.repository)
        .map(portable)
        .unwrap_or_else(|_| portable(scenario_path));
    let mut inputs = vec![ArtifactIdentity {
        locator: relative_locator(&run_directory, &scenario_input)?,
        sha256: scenario_sha256.clone(),
        bytes: scenario_bytes.len() as u64,
    }];

    let scenario_directory = scenario_path
        .parent()
        .context("scenario path has no parent directory")?;
    let fixture_snapshots = snapshot_copy_inputs(
        scenario,
        scenario_directory,
        &inputs_directory,
        &run_directory,
        &mut inputs,
    )?;

    let workspace = match scenario.environment {
        ScenarioEnvironment::Isolated => {
            let path = run_directory.join("workspace");
            fs::create_dir_all(&path)?;
            path
        }
        ScenarioEnvironment::HostReadOnly => options
            .host_root
            .as_ref()
            .context("host_read_only scenario requires --host-root")
            .and_then(|path| {
                fs::canonicalize(path)
                    .with_context(|| format!("failed to resolve host root {}", path.display()))
            })?,
    };

    let executable = fs::canonicalize(&options.wikitool).with_context(|| {
        format!(
            "failed to resolve wikitool executable {}",
            options.wikitool.display()
        )
    })?;
    let (tool_sha256, _) = sha256_file(&executable)?;
    let version = probe_version(
        &executable,
        &options.repository,
        &run_directory.join("tool-version.stdout.txt"),
        &run_directory.join("tool-version.stderr.txt"),
    )?;
    let tool_locator = executable
        .strip_prefix(&options.repository)
        .map(portable)
        .unwrap_or_else(|_| portable(&executable));

    let mut receipt = RunReceipt {
        schema: RECEIPT_SCHEMA.to_owned(),
        run_id,
        driver: current_driver_identity(&options.repository)?,
        scenario: ScenarioIdentity {
            id: scenario.id.clone(),
            title: scenario.title.clone(),
            kind: scenario.kind,
            environment: scenario.environment,
            coverage: scenario.coverage.clone(),
            locator: scenario_locator,
            sha256: scenario_sha256,
        },
        tool: ToolIdentity {
            locator: tool_locator,
            sha256: tool_sha256,
            version,
        },
        status: RunStatus::Error,
        started_at_unix_ms: started_at,
        finished_at_unix_ms: started_at,
        duration_ms: 0,
        complete: false,
        output_truncated: false,
        inputs,
        requirements: Vec::new(),
        steps: Vec::new(),
        failure: Some("run has not completed".to_owned()),
    };
    atomic_write_json(&receipt_path, &receipt)?;

    let variables = Variables {
        repository: &options.repository,
        scenario_directory,
        run_directory: &run_directory,
        workspace: &workspace,
        host_root: options.host_root.as_deref(),
    };
    if let Some(status) = evaluate_requirements(scenario, &variables, &mut receipt)? {
        receipt.status = status;
        receipt.complete = true;
        receipt.failure = match status {
            RunStatus::Skipped => Some("one or more declared requirements requested skip".into()),
            RunStatus::Failed => Some("one or more declared requirements failed".into()),
            RunStatus::Passed | RunStatus::Error => None,
        };
        finish_receipt(&mut receipt, started)?;
        atomic_write_json(&receipt_path, &receipt)?;
        return Ok(CompletedRun {
            receipt,
            receipt_path,
            run_directory,
        });
    }
    atomic_write_json(&receipt_path, &receipt)?;

    let scenario_timeout = Duration::from_millis(scenario.timeout_ms);
    let mut failed = false;
    let mut execution_error = false;
    for (index, step) in scenario.steps.iter().enumerate() {
        if started.elapsed() >= scenario_timeout {
            receipt.failure = Some(format!(
                "scenario exceeded its {} ms execution budget before step '{}'",
                scenario.timeout_ms,
                step.id()
            ));
            receipt.status = RunStatus::Error;
            failed = true;
            execution_error = true;
            break;
        }
        let step_receipt = match step {
            ScenarioStep::Copy { id, target, .. } => run_copy_step(
                index,
                id,
                target,
                fixture_snapshots
                    .get(id)
                    .with_context(|| format!("missing snapshotted fixture for step '{id}'"))?,
                &workspace,
                &steps_directory,
                &run_directory,
            ),
            ScenarioStep::Command {
                id,
                argv,
                cwd,
                timeout_ms,
                environment,
                expect,
            } => {
                let remaining = scenario_timeout.saturating_sub(started.elapsed());
                let declared = timeout_ms.map(Duration::from_millis).unwrap_or(remaining);
                let timeout = declared.min(remaining);
                run_command_step(
                    index,
                    id,
                    argv,
                    cwd.as_deref(),
                    environment,
                    expect,
                    &variables,
                    &executable,
                    timeout,
                    options.maximum_output_bytes,
                    &steps_directory,
                    &run_directory,
                    &workspace,
                )
            }
        };

        match step_receipt {
            Ok(step_receipt) => {
                receipt.output_truncated |= step_receipt
                    .stdout
                    .as_ref()
                    .is_some_and(|output| output.truncated)
                    || step_receipt
                        .stderr
                        .as_ref()
                        .is_some_and(|output| output.truncated);
                let passed = step_receipt.status == RunStatus::Passed;
                if !passed {
                    receipt.failure = Some(format!("step '{}' did not pass", step.id()));
                }
                receipt.steps.push(step_receipt);
                atomic_write_json(&receipt_path, &receipt)?;
                if !passed {
                    failed = true;
                    break;
                }
            }
            Err(error) => {
                receipt.steps.push(StepReceipt {
                    id: step.id().to_owned(),
                    action: step_action(step).to_owned(),
                    status: RunStatus::Error,
                    duration_ms: 0,
                    argv: Vec::new(),
                    exit_code: None,
                    timed_out: None,
                    stdout: None,
                    stderr: None,
                    assertions: Vec::new(),
                    copied: None,
                    failure: Some(format!("{error:#}")),
                });
                receipt.failure =
                    Some(format!("step '{}' failed to execute: {error:#}", step.id()));
                receipt.status = RunStatus::Error;
                failed = true;
                execution_error = true;
                break;
            }
        }
    }

    receipt.status = if execution_error {
        RunStatus::Error
    } else if failed {
        RunStatus::Failed
    } else {
        RunStatus::Passed
    };
    receipt.complete = !receipt.output_truncated;
    if !failed {
        receipt.failure = None;
    } else if receipt.output_truncated {
        receipt.failure = Some("run output was truncated; assertions are incomplete".to_owned());
    }
    finish_receipt(&mut receipt, started)?;
    atomic_write_json(&receipt_path, &receipt)?;
    Ok(CompletedRun {
        receipt,
        receipt_path,
        run_directory,
    })
}

pub fn run_suite(path: &Path, options: &RunOptions, require_all: bool) -> Result<CompletedSuite> {
    let (manifest, suite_bytes) = load_manifest(path)?;
    let Manifest::Suite(suite) = manifest else {
        bail!("{} is not a suite", path.display());
    };
    suite.validate()?;
    let started_at = unix_ms()?;
    let started = Instant::now();
    let (run_id, run_directory) = create_run_directory(
        &options.artifacts_root,
        &format!("suite-{}", suite.id),
        started_at,
        std::process::id(),
    )?;
    let receipt_path = run_directory.join("receipt.json");
    let suite_input = run_directory.join("inputs/suite.json");
    atomic_write(&suite_input, &suite_bytes)?;
    let suite_sha256 = sha256_bytes(&suite_bytes);
    let suite_locator = path
        .strip_prefix(&options.repository)
        .map(portable)
        .unwrap_or_else(|_| portable(path));
    let mut observed_coverage = BTreeSet::new();
    for scenario_id in &suite.scenarios {
        let scenario_path = resolve_manifest(scenario_id, &options.catalogs, "scenario")?;
        let (manifest, _) = load_manifest(&scenario_path)?;
        let Manifest::Scenario(scenario) = manifest else {
            bail!("suite entry '{scenario_id}' does not identify a scenario");
        };
        observed_coverage.extend(scenario.coverage);
    }
    let missing_coverage = suite
        .required_coverage
        .iter()
        .filter(|coverage| !observed_coverage.contains(coverage.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_coverage.is_empty() {
        bail!(
            "suite '{}' is missing required coverage {:?}",
            suite.id,
            missing_coverage
        );
    }
    let mut receipt = SuiteReceipt {
        schema: SUITE_RECEIPT_SCHEMA.to_owned(),
        run_id,
        driver: current_driver_identity(&options.repository)?,
        suite: SuiteIdentity {
            id: suite.id,
            title: suite.title,
            locator: suite_locator,
            sha256: suite_sha256,
        },
        status: RunStatus::Error,
        require_all,
        started_at_unix_ms: started_at,
        finished_at_unix_ms: started_at,
        duration_ms: 0,
        complete: false,
        required_coverage: suite.required_coverage,
        observed_coverage: observed_coverage.into_iter().collect(),
        runs: Vec::new(),
    };
    atomic_write_json(&receipt_path, &receipt)?;
    let mut all_child_complete = true;

    for scenario in suite.scenarios {
        let scenario_path = match resolve_manifest(&scenario, &options.catalogs, "scenario") {
            Ok(path) => path,
            Err(error) => {
                all_child_complete = false;
                receipt.runs.push(SuiteRunEntry {
                    scenario,
                    scenario_id: None,
                    status: RunStatus::Error,
                    receipt_locator: None,
                    receipt_sha256: None,
                    error: Some(format!("{error:#}")),
                });
                atomic_write_json(&receipt_path, &receipt)?;
                continue;
            }
        };
        match run_scenario(&scenario_path, options) {
            Ok(run) => {
                all_child_complete &= run.receipt.complete;
                let (digest, _) = sha256_file(&run.receipt_path)?;
                receipt.runs.push(SuiteRunEntry {
                    scenario,
                    scenario_id: Some(run.receipt.scenario.id),
                    status: run.receipt.status,
                    receipt_locator: Some(relative_locator(
                        &options.artifacts_root,
                        &run.receipt_path,
                    )?),
                    receipt_sha256: Some(digest),
                    error: None,
                });
            }
            Err(error) => {
                all_child_complete = false;
                receipt.runs.push(SuiteRunEntry {
                    scenario,
                    scenario_id: None,
                    status: RunStatus::Error,
                    receipt_locator: None,
                    receipt_sha256: None,
                    error: Some(format!("{error:#}")),
                });
            }
        }
        atomic_write_json(&receipt_path, &receipt)?;
    }

    let has_error = receipt
        .runs
        .iter()
        .any(|run| run.status == RunStatus::Error);
    let has_failure = receipt
        .runs
        .iter()
        .any(|run| run.status == RunStatus::Failed);
    let has_skip = receipt
        .runs
        .iter()
        .any(|run| run.status == RunStatus::Skipped);
    receipt.status = if has_error {
        RunStatus::Error
    } else if has_failure || (require_all && has_skip) {
        RunStatus::Failed
    } else {
        RunStatus::Passed
    };
    receipt.complete = all_child_complete
        && receipt.runs.iter().all(|run| {
            run.error.is_none()
                && run
                    .receipt_locator
                    .as_ref()
                    .is_some_and(|locator| options.artifacts_root.join(locator).is_file())
        });
    receipt.finished_at_unix_ms = unix_ms()?;
    receipt.duration_ms = started.elapsed().as_millis();
    atomic_write_json(&receipt_path, &receipt)?;
    Ok(CompletedSuite {
        receipt,
        receipt_path,
        run_directory,
    })
}

fn snapshot_copy_inputs(
    scenario: &ScenarioManifest,
    scenario_directory: &Path,
    inputs_directory: &Path,
    run_directory: &Path,
    inputs: &mut Vec<ArtifactIdentity>,
) -> Result<HashMap<String, PathBuf>> {
    let mut snapshots = HashMap::new();
    for step in &scenario.steps {
        let ScenarioStep::Copy {
            id, source, sha256, ..
        } = step
        else {
            continue;
        };
        let source_path = resolve_existing_plain_file(scenario_directory, source)?;
        let bytes = fs::read(&source_path)?;
        let digest = sha256_bytes(&bytes);
        if *sha256 != digest {
            bail!("fixture digest mismatch for step '{id}': got {digest}, expected {sha256}");
        }
        let file_name = source_path
            .file_name()
            .and_then(|value| value.to_str())
            .context("fixture file name is not UTF-8")?;
        let snapshot = inputs_directory.join(id).join(file_name);
        atomic_write(&snapshot, &bytes)?;
        inputs.push(ArtifactIdentity {
            locator: relative_locator(run_directory, &snapshot)?,
            sha256: digest,
            bytes: bytes.len() as u64,
        });
        snapshots.insert(id.clone(), snapshot);
    }
    Ok(snapshots)
}

fn run_copy_step(
    index: usize,
    id: &str,
    target: &str,
    snapshot: &Path,
    workspace: &Path,
    steps_directory: &Path,
    run_directory: &Path,
) -> Result<StepReceipt> {
    let started = Instant::now();
    let destination = resolve_output_path(workspace, target)?;
    let bytes = fs::read(snapshot)?;
    atomic_write(&destination, &bytes)?;
    let retained_output = steps_directory.join(format!("{index:03}-{id}.copied"));
    atomic_write(&retained_output, &bytes)?;
    let digest = sha256_bytes(&bytes);
    Ok(StepReceipt {
        id: id.to_owned(),
        action: "copy".to_owned(),
        status: RunStatus::Passed,
        duration_ms: started.elapsed().as_millis(),
        argv: Vec::new(),
        exit_code: None,
        timed_out: None,
        stdout: None,
        stderr: None,
        assertions: vec![AssertionReceipt {
            target: target.to_owned(),
            assertion: "copied_sha256".to_owned(),
            passed: true,
            detail: digest.clone(),
        }],
        copied: Some(ArtifactIdentity {
            locator: relative_locator(run_directory, &retained_output)?,
            sha256: digest,
            bytes: bytes.len() as u64,
        }),
        failure: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_command_step(
    index: usize,
    id: &str,
    declared_argv: &[String],
    declared_cwd: Option<&str>,
    declared_environment: &BTreeMap<String, String>,
    expect: &crate::model::CommandExpectation,
    variables: &Variables<'_>,
    executable: &Path,
    timeout: Duration,
    maximum_output_bytes: usize,
    steps_directory: &Path,
    run_directory: &Path,
    workspace: &Path,
) -> Result<StepReceipt> {
    let expanded_argv = declared_argv
        .iter()
        .map(|value| variables.expand(value))
        .collect::<Result<Vec<_>>>()?;
    let mut process_argv = vec![
        "--project-root".to_owned(),
        workspace.to_string_lossy().into_owned(),
    ];
    process_argv.extend(expanded_argv);
    let cwd = match declared_cwd {
        Some(relative) => resolve_output_path(workspace, relative)?,
        None => workspace.to_path_buf(),
    };
    if !cwd.is_dir() {
        bail!("step '{id}' cwd does not exist: {}", cwd.display());
    }
    let environment = declared_environment
        .iter()
        .map(|(key, value)| Ok((key.clone(), variables.expand(value)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let stdout_path = steps_directory.join(format!("{index:03}-{id}.stdout.txt"));
    let stderr_path = steps_directory.join(format!("{index:03}-{id}.stderr.txt"));
    let outcome = run_bounded(
        executable,
        &process_argv,
        &cwd,
        &environment,
        timeout,
        maximum_output_bytes,
        &stdout_path,
        &stderr_path,
    )?;
    let stdout_artifact = output_artifact(run_directory, &stdout_path, &outcome.stdout)?;
    let stderr_artifact = output_artifact(run_directory, &stderr_path, &outcome.stderr)?;
    let mut assertions = vec![AssertionReceipt {
        target: "process".to_owned(),
        assertion: "exit_code".to_owned(),
        passed: outcome.status.code() == Some(expect.exit_code),
        detail: format!(
            "expected {}, observed {}",
            expect.exit_code,
            outcome.status.code().map_or_else(
                || "terminated without an exit code".to_owned(),
                |code| code.to_string()
            )
        ),
    }];
    assertions.push(AssertionReceipt {
        target: "process".to_owned(),
        assertion: "within_timeout".to_owned(),
        passed: !outcome.timed_out,
        detail: if outcome.timed_out {
            format!("process exceeded {} ms", timeout.as_millis())
        } else {
            format!("process completed in {} ms", outcome.duration_ms)
        },
    });
    assertions.extend(evaluate_output_assertions(
        "stdout",
        &outcome.stdout,
        &expect.stdout,
    ));
    assertions.extend(evaluate_output_assertions(
        "stderr",
        &outcome.stderr,
        &expect.stderr,
    ));
    assertions.extend(evaluate_file_assertions(workspace, &expect.files));
    if outcome.stdout.truncated || outcome.stderr.truncated {
        assertions.push(AssertionReceipt {
            target: "process".to_owned(),
            assertion: "output_complete".to_owned(),
            passed: false,
            detail: "captured output exceeded the configured byte budget".to_owned(),
        });
    }
    let passed = assertions.iter().all(|assertion| assertion.passed);
    Ok(StepReceipt {
        id: id.to_owned(),
        action: "command".to_owned(),
        status: if passed {
            RunStatus::Passed
        } else {
            RunStatus::Failed
        },
        duration_ms: outcome.duration_ms,
        argv: declared_argv.to_vec(),
        exit_code: outcome.status.code(),
        timed_out: Some(outcome.timed_out),
        stdout: Some(stdout_artifact),
        stderr: Some(stderr_artifact),
        assertions,
        copied: None,
        failure: (!passed).then(|| "one or more command expectations failed".to_owned()),
    })
}

fn evaluate_output_assertions(
    target: &str,
    output: &CapturedStream,
    assertions: &[OutputAssertion],
) -> Vec<AssertionReceipt> {
    let text = String::from_utf8_lossy(&output.bytes);
    let parsed = if output.truncated {
        None
    } else {
        serde_json::from_slice::<Value>(&output.bytes).ok()
    };
    assertions
        .iter()
        .map(|assertion| match assertion {
            OutputAssertion::Contains { value } => {
                let found = text.contains(value);
                AssertionReceipt {
                    target: target.to_owned(),
                    assertion: "contains".to_owned(),
                    passed: found,
                    detail: if found {
                        format!("found {value:?}")
                    } else if output.truncated {
                        format!("did not find {value:?} in truncated output")
                    } else {
                        format!("did not find {value:?}")
                    },
                }
            }
            OutputAssertion::NotContains { value } => {
                let found = text.contains(value);
                let passed = !found && !output.truncated;
                AssertionReceipt {
                    target: target.to_owned(),
                    assertion: "not_contains".to_owned(),
                    passed,
                    detail: if found {
                        format!("found forbidden text {value:?}")
                    } else if output.truncated {
                        format!("absence of {value:?} cannot be proved from truncated output")
                    } else {
                        format!("did not find {value:?}")
                    },
                }
            }
            OutputAssertion::JsonPointerExists { pointer } => {
                let value = parsed
                    .as_ref()
                    .and_then(|document| document.pointer(pointer));
                AssertionReceipt {
                    target: target.to_owned(),
                    assertion: "json_pointer_exists".to_owned(),
                    passed: value.is_some(),
                    detail: json_detail(output, parsed.as_ref(), pointer, value),
                }
            }
            OutputAssertion::JsonPointerEquals { pointer, value } => {
                let observed = parsed
                    .as_ref()
                    .and_then(|document| document.pointer(pointer));
                AssertionReceipt {
                    target: target.to_owned(),
                    assertion: "json_pointer_equals".to_owned(),
                    passed: observed == Some(value),
                    detail: format!(
                        "pointer {pointer:?}: expected {value}, observed {}",
                        observed.map_or_else(|| "<missing>".to_owned(), Value::to_string)
                    ),
                }
            }
            OutputAssertion::JsonArrayContains { pointer, value } => {
                let observed = parsed
                    .as_ref()
                    .and_then(|document| document.pointer(pointer));
                let passed = observed
                    .and_then(Value::as_array)
                    .is_some_and(|items| items.contains(value));
                AssertionReceipt {
                    target: target.to_owned(),
                    assertion: "json_array_contains".to_owned(),
                    passed,
                    detail: format!(
                        "pointer {pointer:?}: expected array member {value}, observed {}",
                        observed.map_or_else(|| "<missing>".to_owned(), Value::to_string)
                    ),
                }
            }
            OutputAssertion::JsonArrayItemPointerEquals {
                pointer,
                item_pointer,
                value,
            } => {
                let observed = parsed.as_ref().and_then(|document| document.pointer(pointer));
                let matched = observed
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items
                            .iter()
                            .any(|item| item.pointer(item_pointer) == Some(value))
                    });
                AssertionReceipt {
                    target: target.to_owned(),
                    assertion: "json_array_item_pointer_equals".to_owned(),
                    passed: matched,
                    detail: format!(
                        "pointer {pointer:?}: expected an item whose {item_pointer:?} equals {value}, observed {} item(s)",
                        observed.and_then(Value::as_array).map_or(0, Vec::len)
                    ),
                }
            }
            OutputAssertion::JsonPointerU64AtLeast { pointer, value } => {
                let observed = parsed
                    .as_ref()
                    .and_then(|document| document.pointer(pointer))
                    .and_then(Value::as_u64);
                AssertionReceipt {
                    target: target.to_owned(),
                    assertion: "json_pointer_u64_at_least".to_owned(),
                    passed: observed.is_some_and(|observed| observed >= *value),
                    detail: format!(
                        "pointer {pointer:?}: expected at least {value}, observed {}",
                        observed.map_or_else(|| "<missing or non-u64>".to_owned(), |value| value.to_string())
                    ),
                }
            }
            OutputAssertion::JsonPointerNonBlank { pointer } => {
                let observed = parsed
                    .as_ref()
                    .and_then(|document| document.pointer(pointer))
                    .and_then(Value::as_str);
                AssertionReceipt {
                    target: target.to_owned(),
                    assertion: "json_pointer_non_blank".to_owned(),
                    passed: observed.is_some_and(|value| !value.trim().is_empty()),
                    detail: format!(
                        "pointer {pointer:?}: observed {}",
                        observed.map_or("<missing or non-string>", |value| value)
                    ),
                }
            }
        })
        .collect()
}

fn json_detail(
    output: &CapturedStream,
    parsed: Option<&Value>,
    pointer: &str,
    observed: Option<&Value>,
) -> String {
    if output.truncated {
        return format!("pointer {pointer:?} cannot be checked in truncated JSON");
    }
    if parsed.is_none() {
        return "output is not a JSON document".to_owned();
    }
    observed.map_or_else(
        || format!("pointer {pointer:?} is missing"),
        |value| format!("pointer {pointer:?} resolves to {value}"),
    )
}

fn evaluate_file_assertions(root: &Path, assertions: &[FileAssertion]) -> Vec<AssertionReceipt> {
    assertions
        .iter()
        .map(|assertion| match evaluate_file_assertion(root, assertion) {
            Ok(receipt) => receipt,
            Err(error) => AssertionReceipt {
                target: assertion.path().to_owned(),
                assertion: file_assertion_name(assertion).to_owned(),
                passed: false,
                detail: format!("{error:#}"),
            },
        })
        .collect()
}

fn evaluate_file_assertion(root: &Path, assertion: &FileAssertion) -> Result<AssertionReceipt> {
    let path = resolve_output_path(root, assertion.path())?;
    let (passed, detail) = match assertion {
        FileAssertion::Exists { .. } => (
            path.is_file(),
            if path.is_file() {
                "plain file exists".to_owned()
            } else {
                "plain file does not exist".to_owned()
            },
        ),
        FileAssertion::Missing { .. } => (
            !path.exists(),
            if path.exists() {
                "path exists".to_owned()
            } else {
                "path is absent".to_owned()
            },
        ),
        FileAssertion::Contains { value, .. } | FileAssertion::NotContains { value, .. } => {
            let metadata = fs::metadata(&path)
                .with_context(|| format!("failed to inspect {}", path.display()))?;
            if !metadata.is_file() {
                bail!("{} is not a plain file", path.display());
            }
            if metadata.len() > FILE_ASSERTION_BUDGET {
                bail!(
                    "{} exceeds the {} byte file assertion budget",
                    path.display(),
                    FILE_ASSERTION_BUDGET
                );
            }
            let bytes = fs::read(&path)?;
            let text = String::from_utf8(bytes).context("asserted file is not UTF-8")?;
            let found = text.contains(value);
            let expected_found = matches!(assertion, FileAssertion::Contains { .. });
            (
                found == expected_found,
                format!(
                    "text {value:?} was {}",
                    if found { "found" } else { "not found" }
                ),
            )
        }
        FileAssertion::Sha256 { value, .. } => {
            let (observed, _) = sha256_file(&path)?;
            (
                observed == *value,
                format!("expected {value}, observed {observed}"),
            )
        }
    };
    Ok(AssertionReceipt {
        target: assertion.path().to_owned(),
        assertion: file_assertion_name(assertion).to_owned(),
        passed,
        detail,
    })
}

fn file_assertion_name(assertion: &FileAssertion) -> &'static str {
    match assertion {
        FileAssertion::Exists { .. } => "exists",
        FileAssertion::Missing { .. } => "missing",
        FileAssertion::Contains { .. } => "contains",
        FileAssertion::NotContains { .. } => "not_contains",
        FileAssertion::Sha256 { .. } => "sha256",
    }
}

fn output_artifact(
    run_directory: &Path,
    path: &Path,
    stream: &CapturedStream,
) -> Result<OutputArtifact> {
    Ok(OutputArtifact {
        locator: relative_locator(run_directory, path)?,
        sha256: stream.sha256.clone(),
        stored_sha256: stream.stored_sha256.clone(),
        observed_bytes: stream.observed_bytes,
        stored_bytes: stream.stored_bytes,
        truncated: stream.truncated,
    })
}

fn evaluate_requirements(
    scenario: &ScenarioManifest,
    variables: &Variables<'_>,
    receipt: &mut RunReceipt,
) -> Result<Option<RunStatus>> {
    let mut status = None;
    for requirement in &scenario.requirements {
        match requirement {
            Requirement::PathExists { path, on_missing } => {
                let expanded = variables.expand(path)?;
                let exists = Path::new(&expanded).exists();
                receipt.requirements.push(RequirementReceipt {
                    kind: "path_exists".to_owned(),
                    passed: exists,
                    disposition: *on_missing,
                    detail: if exists {
                        format!("path exists: {}", portable(Path::new(&expanded)))
                    } else {
                        format!("path is missing: {}", portable(Path::new(&expanded)))
                    },
                });
                if !exists {
                    let observed = match on_missing {
                        MissingDisposition::Fail => RunStatus::Failed,
                        MissingDisposition::Skip => RunStatus::Skipped,
                    };
                    if status != Some(RunStatus::Failed) {
                        status = Some(observed);
                    }
                }
            }
        }
    }
    Ok(status)
}

fn finish_receipt(receipt: &mut RunReceipt, started: Instant) -> Result<()> {
    receipt.finished_at_unix_ms = unix_ms()?;
    receipt.duration_ms = started.elapsed().as_millis();
    Ok(())
}

fn create_run_directory(
    artifacts_root: &Path,
    scenario_id: &str,
    started_at: u128,
    process_id: u32,
) -> Result<(String, PathBuf)> {
    fs::create_dir_all(artifacts_root).with_context(|| {
        format!(
            "failed to create artifact root {}",
            artifacts_root.display()
        )
    })?;
    for sequence in 0_u16..=u16::MAX {
        let run_id = if sequence == 0 {
            format!("{scenario_id}-{started_at}-{process_id}")
        } else {
            format!("{scenario_id}-{started_at}-{process_id}-{sequence}")
        };
        let path = artifacts_root.join(&run_id);
        match fs::create_dir(&path) {
            Ok(()) => return Ok((run_id, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to create run directory {}", path.display()));
            }
        }
    }
    bail!("could not allocate a unique run directory")
}

fn step_action(step: &ScenarioStep) -> &'static str {
    match step {
        ScenarioStep::Copy { .. } => "copy",
        ScenarioStep::Command { .. } => "command",
    }
}

struct Variables<'a> {
    repository: &'a Path,
    scenario_directory: &'a Path,
    run_directory: &'a Path,
    workspace: &'a Path,
    host_root: Option<&'a Path>,
}

impl Variables<'_> {
    fn expand(&self, value: &str) -> Result<String> {
        let mut output = value.to_owned();
        for (token, replacement) in [
            ("{REPO_ROOT}", Some(self.repository)),
            ("{SCENARIO_DIR}", Some(self.scenario_directory)),
            ("{RUN_DIR}", Some(self.run_directory)),
            ("{WORKSPACE}", Some(self.workspace)),
            ("{HOST_ROOT}", self.host_root),
        ] {
            if output.contains(token) {
                let replacement =
                    replacement.with_context(|| format!("{token} is unavailable in this run"))?;
                output = output.replace(token, &replacement.to_string_lossy());
            }
        }
        if output.contains('{') || output.contains('}') {
            bail!("unknown interpolation token in {value:?}");
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::CapturedStream;

    fn output(bytes: &[u8], truncated: bool) -> CapturedStream {
        CapturedStream {
            sha256: sha256_bytes(bytes),
            stored_sha256: sha256_bytes(bytes),
            observed_bytes: bytes.len() as u64 + u64::from(truncated),
            stored_bytes: bytes.len() as u64,
            truncated,
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn negative_assertions_refuse_truncated_output() {
        let receipts = evaluate_output_assertions(
            "stdout",
            &output(b"partial", true),
            &[OutputAssertion::NotContains {
                value: "failure".into(),
            }],
        );
        assert!(!receipts[0].passed);
        assert!(receipts[0].detail.contains("cannot be proved"));
    }

    #[test]
    fn json_pointer_assertions_use_structured_output() {
        let receipts = evaluate_output_assertions(
            "stdout",
            &output(br#"{"readiness":"content_ready"}"#, false),
            &[OutputAssertion::JsonPointerEquals {
                pointer: "/readiness".into(),
                value: serde_json::json!("content_ready"),
            }],
        );
        assert!(receipts[0].passed);
    }

    #[test]
    fn interpolation_refuses_unknown_tokens() {
        let root = Path::new("root");
        let variables = Variables {
            repository: root,
            scenario_directory: root,
            run_directory: root,
            workspace: root,
            host_root: None,
        };
        assert!(variables.expand("{UNKNOWN}").is_err());
    }
}
