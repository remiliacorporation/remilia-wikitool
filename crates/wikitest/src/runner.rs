use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use crate::artifact::{
    atomic_write, atomic_write_json, portable, relative_locator, resolve_existing_plain_file,
    resolve_output_path, sha256_bytes, sha256_file, unix_ms,
};
use crate::canonical::canonicalize_exact_paths;
use crate::catalog::{Manifest, load_manifest, resolve_manifest};
use crate::identity::{current_driver_identity, repository_binary_locator};
use crate::mediawiki::{MediaWikiFixture, MediaWikiService, evaluate_expectation};
use crate::model::{
    ArtifactIdentity, AssertionReceipt, FileAssertion, FileObservation, FileObservationState,
    JsonScalarCapture, JsonScalarCaptureReceipt, MissingDisposition, OutputArtifact,
    OutputAssertion, RECEIPT_SCHEMA, REQUIREMENT_OBSERVATION_SCHEMA, Requirement,
    RequirementObservation, RequirementReceipt, RunReceipt, RunStatus, SUITE_RECEIPT_SCHEMA,
    ScenarioEnvironment, ScenarioIdentity, ScenarioManifest, ScenarioStep, StepReceipt,
    SuiteIdentity, SuiteReceipt, SuiteRunEntry, ToolIdentity,
};
use crate::process::{CapturedStream, probe_version, run_bounded};

const DEFAULT_OUTPUT_BUDGET: usize = 2 * 1024 * 1024;
const FILE_ASSERTION_BUDGET: u64 = 16 * 1024 * 1024;
const WORKSPACE_ROOT_TOKEN: &str = "<WIKITEST_WORKSPACE_ROOT>";
const WORKSPACE_DATA_TOKEN: &str = "<WIKITEST_WORKSPACE_DATA>";
const WORKSPACE_CONFIG_TOKEN: &str = "<WIKITEST_WORKSPACE_CONFIG>";
const EXECUTION_WORKSPACES_TOKEN: &str = "<WIKITEST_EXECUTION_WORKSPACES>";
const TOOL_BINARY_TOKEN: &str = "<WIKITEST_TOOL_BINARY>";
const HOST_SNAPSHOT_PATHS: &[&str] =
    &[".wikitool", "wiki_content", "templates", "wikitool_adapter"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostSnapshotEntry {
    locator: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug)]
struct HostSnapshot {
    root: PathBuf,
    entries: Vec<HostSnapshotEntry>,
}

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
    let mediawiki_snapshot = snapshot_mediawiki_fixture(
        scenario,
        scenario_directory,
        &inputs_directory,
        &run_directory,
        &mut inputs,
    )?;

    let workspace = create_execution_workspace(&run_id)?;
    let host_root = match scenario.environment {
        ScenarioEnvironment::Isolated => None,
        ScenarioEnvironment::HostReadOnly => Some(
            options
                .host_root
                .as_ref()
                .context("host_read_only scenario requires --host-root")
                .and_then(|path| {
                    fs::canonicalize(path)
                        .with_context(|| format!("failed to resolve host root {}", path.display()))
                })?,
        ),
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
    let tool_locator =
        repository_binary_locator(&executable, &options.repository, "configured Wikitool")?;

    let mediawiki = mediawiki_snapshot
        .as_deref()
        .map(MediaWikiFixture::from_path)
        .transpose()?
        .map(MediaWikiService::start)
        .transpose()?;

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

    let mut variables = Variables {
        repository: &options.repository,
        scenario_directory,
        run_directory: &run_directory,
        workspace: &workspace,
        host_root: host_root.as_deref(),
        mediawiki_api_url: mediawiki.as_ref().map(MediaWikiService::endpoint),
        captures: BTreeMap::new(),
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
    let host_snapshot = host_root
        .as_deref()
        .map(|root| {
            snapshot_host_root(
                root,
                &workspace,
                &inputs_directory,
                &run_directory,
                &mut receipt.inputs,
            )
        })
        .transpose()?;
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
                captures,
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
                    captures,
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
            ScenarioStep::MediaWikiUpdate { id, page } => run_mediawiki_update_step(
                index,
                id,
                page,
                mediawiki
                    .as_ref()
                    .context("mediawiki update step has no fixture service")?,
                &steps_directory,
                &run_directory,
            ),
            ScenarioStep::MediaWikiAssert { id, expect } => run_mediawiki_assert_step(
                index,
                id,
                expect,
                mediawiki
                    .as_ref()
                    .context("mediawiki assertion step has no fixture service")?,
                &steps_directory,
                &run_directory,
            ),
        };

        match step_receipt {
            Ok(mut step_receipt) => {
                receipt.output_truncated |= step_receipt
                    .stdout
                    .as_ref()
                    .is_some_and(|output| output.truncated)
                    || step_receipt
                        .stderr
                        .as_ref()
                        .is_some_and(|output| output.truncated);
                let mut passed = step_receipt.status == RunStatus::Passed;
                if passed && let Err(error) = variables.bind_captures(&step_receipt.captures) {
                    step_receipt.status = RunStatus::Error;
                    step_receipt.failure = Some(format!("capture binding failed: {error:#}"));
                    receipt.failure = Some(format!(
                        "step '{}' could not bind captured values: {error:#}",
                        step.id()
                    ));
                    execution_error = true;
                    passed = false;
                } else if !passed {
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
                    captures: Vec::new(),
                    copied: None,
                    observation: None,
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

    if let Some(snapshot) = &host_snapshot
        && let Err(error) = verify_host_unchanged(snapshot)
    {
        receipt.failure = Some(format!("{error:#}"));
        failed = true;
        execution_error = true;
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
    let mut declared_coverage = BTreeSet::new();
    for scenario_id in &suite.scenarios {
        let scenario_path = resolve_manifest(scenario_id, &options.catalogs, "scenario")?;
        let (manifest, _) = load_manifest(&scenario_path)?;
        let Manifest::Scenario(scenario) = manifest else {
            bail!("suite entry '{scenario_id}' does not identify a scenario");
        };
        scenario.validate()?;
        declared_coverage.extend(
            scenario
                .coverage
                .iter()
                .map(|binding| binding.capability.clone()),
        );
    }
    let missing_coverage = suite
        .required_coverage
        .iter()
        .filter(|coverage| !declared_coverage.contains(coverage.as_str()))
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
        observed_coverage: Vec::new(),
        runs: Vec::new(),
    };
    atomic_write_json(&receipt_path, &receipt)?;
    let mut all_child_complete = true;
    let mut observed_coverage = BTreeSet::new();

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
                observed_coverage.extend(successful_coverage(&run.receipt));
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
    receipt.observed_coverage = observed_coverage.into_iter().collect();
    let runtime_missing_coverage = receipt
        .required_coverage
        .iter()
        .any(|required| !receipt.observed_coverage.contains(required));
    receipt.status = if has_error {
        RunStatus::Error
    } else if has_failure || runtime_missing_coverage || (require_all && has_skip) {
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

pub(crate) fn successful_coverage(receipt: &RunReceipt) -> BTreeSet<String> {
    let passed_steps = receipt
        .steps
        .iter()
        .filter(|step| step.status == RunStatus::Passed)
        .map(|step| step.id.as_str())
        .collect::<BTreeSet<_>>();
    receipt
        .scenario
        .coverage
        .iter()
        .filter(|binding| {
            !binding.steps.is_empty()
                && binding
                    .steps
                    .iter()
                    .all(|step| passed_steps.contains(step.as_str()))
        })
        .map(|binding| binding.capability.clone())
        .collect()
}

pub(crate) fn create_execution_workspace(run_id: &str) -> Result<PathBuf> {
    let base = std::env::temp_dir().join("wikitest-execution-workspaces");
    fs::create_dir_all(&base)?;
    let workspace = base.join(run_id);
    if workspace.exists() {
        bail!(
            "execution workspace already exists and will not be reused: {}",
            workspace.display()
        );
    }
    fs::create_dir(&workspace)?;
    atomic_write(&workspace.join(".env"), b"")?;
    fs::canonicalize(&workspace).with_context(|| {
        format!(
            "failed to resolve execution workspace {}",
            workspace.display()
        )
    })
}

fn snapshot_host_root(
    host_root: &Path,
    workspace: &Path,
    inputs_directory: &Path,
    run_directory: &Path,
    inputs: &mut Vec<ArtifactIdentity>,
) -> Result<HostSnapshot> {
    let entries = capture_host_entries(host_root)?;
    if entries.is_empty() {
        bail!("host_read_only root has no recognized Wikitool runtime surfaces");
    }
    for entry in &entries {
        let source = resolve_existing_plain_file(host_root, &entry.locator)?;
        let bytes = fs::read(&source)?;
        let digest = sha256_bytes(&bytes);
        if digest != entry.sha256 || bytes.len() as u64 != entry.bytes {
            bail!(
                "host source changed while it was being snapshotted: {}",
                entry.locator
            );
        }
        let destination = resolve_output_path(workspace, &entry.locator)?;
        atomic_write(&destination, &bytes)?;
    }
    if capture_host_entries(host_root)? != entries {
        bail!("host source changed while the isolated snapshot was being created");
    }
    verify_snapshot_adapter_is_contained(workspace, &entries)?;
    let manifest_path = inputs_directory.join("host-snapshot.json");
    atomic_write_json(&manifest_path, &entries)?;
    let (sha256, bytes) = sha256_file(&manifest_path)?;
    inputs.push(ArtifactIdentity {
        locator: relative_locator(run_directory, &manifest_path)?,
        sha256,
        bytes,
    });
    Ok(HostSnapshot {
        root: host_root.to_path_buf(),
        entries,
    })
}

fn verify_snapshot_adapter_is_contained(
    workspace: &Path,
    entries: &[HostSnapshotEntry],
) -> Result<()> {
    let config_path = workspace.join(".wikitool/config.toml");
    let config_bytes = match fs::read(&config_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("failed to read snapshotted Wikitool config"),
    };
    let config = std::str::from_utf8(&config_bytes)
        .context("snapshotted Wikitool config is not UTF-8")?
        .parse::<toml::Value>()
        .context("snapshotted Wikitool config is invalid TOML")?;
    let Some(adapter_path) = config
        .get("adapter")
        .and_then(toml::Value::as_table)
        .and_then(|adapter| adapter.get("path"))
    else {
        return Ok(());
    };
    let adapter_path = adapter_path
        .as_str()
        .context("snapshotted [adapter].path must be a string")?;
    let resolved = resolve_output_path(workspace, adapter_path).with_context(|| {
        format!("snapshotted adapter.path must remain inside the isolated snapshot: {adapter_path}")
    })?;
    let metadata = fs::symlink_metadata(&resolved).with_context(|| {
        format!(
            "snapshotted adapter.path is not present in the isolated snapshot: {}",
            resolved.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "snapshotted adapter.path is not a regular retained file: {}",
            resolved.display()
        );
    }
    let locator = relative_locator(workspace, &resolved)?;
    if !entries.iter().any(|entry| entry.locator == locator) {
        bail!("snapshotted adapter.path is not bound by the host snapshot: {locator}");
    }
    Ok(())
}

fn capture_host_entries(host_root: &Path) -> Result<Vec<HostSnapshotEntry>> {
    let mut entries = Vec::new();
    for relative in HOST_SNAPSHOT_PATHS {
        let path = host_root.join(relative);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            bail!("host snapshot surface is a symlink: {}", path.display());
        }
        if metadata.is_file() {
            entries.push(host_snapshot_entry(host_root, &path)?);
            continue;
        }
        if !metadata.is_dir() {
            bail!(
                "host snapshot surface is not a file or directory: {}",
                path.display()
            );
        }
        for entry in WalkDir::new(&path).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_symlink() {
                bail!(
                    "host snapshot contains a symlink: {}",
                    entry.path().display()
                );
            }
            if entry.file_type().is_file() {
                entries.push(host_snapshot_entry(host_root, entry.path())?);
            }
        }
    }
    entries.sort_by(|left, right| left.locator.cmp(&right.locator));
    Ok(entries)
}

fn host_snapshot_entry(host_root: &Path, path: &Path) -> Result<HostSnapshotEntry> {
    let bytes = fs::read(path)?;
    Ok(HostSnapshotEntry {
        locator: relative_locator(host_root, path)?,
        sha256: sha256_bytes(&bytes),
        bytes: bytes.len() as u64,
    })
}

fn verify_host_unchanged(snapshot: &HostSnapshot) -> Result<()> {
    let observed = capture_host_entries(&snapshot.root)?;
    if observed != snapshot.entries {
        bail!("host_read_only source changed during isolated scenario execution");
    }
    Ok(())
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

fn snapshot_mediawiki_fixture(
    scenario: &ScenarioManifest,
    scenario_directory: &Path,
    inputs_directory: &Path,
    run_directory: &Path,
    inputs: &mut Vec<ArtifactIdentity>,
) -> Result<Option<PathBuf>> {
    let Some(fixture) = &scenario.mediawiki_fixture else {
        return Ok(None);
    };
    let source = resolve_existing_plain_file(scenario_directory, &fixture.source)?;
    let bytes = fs::read(&source)?;
    let digest = sha256_bytes(&bytes);
    if fixture.sha256 != digest {
        bail!(
            "MediaWiki fixture digest mismatch: got {digest}, expected {}",
            fixture.sha256
        );
    }
    let snapshot = inputs_directory.join("mediawiki-fixture.json");
    atomic_write(&snapshot, &bytes)?;
    inputs.push(ArtifactIdentity {
        locator: relative_locator(run_directory, &snapshot)?,
        sha256: digest,
        bytes: bytes.len() as u64,
    });
    Ok(Some(snapshot))
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
            file_evidence: None,
        }],
        captures: Vec::new(),
        copied: Some(ArtifactIdentity {
            locator: relative_locator(run_directory, &retained_output)?,
            sha256: digest,
            bytes: bytes.len() as u64,
        }),
        observation: None,
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
    declared_captures: &[JsonScalarCapture],
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
        "--data-dir".to_owned(),
        workspace
            .join(".wikitool/data")
            .to_string_lossy()
            .into_owned(),
        "--config".to_owned(),
        workspace
            .join(".wikitool/config.toml")
            .to_string_lossy()
            .into_owned(),
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
    let raw_directory = workspace.join(".wikitest-private/process-output");
    let raw_stdout_path = raw_directory.join(format!("{index:03}-{id}.stdout.txt"));
    let raw_stderr_path = raw_directory.join(format!("{index:03}-{id}.stderr.txt"));
    let mut outcome = run_bounded(
        executable,
        &process_argv,
        &cwd,
        &environment,
        timeout,
        maximum_output_bytes,
        &raw_stdout_path,
        &raw_stderr_path,
    )?;
    outcome.stdout =
        canonicalized_process_stream(&outcome.stdout, &stdout_path, workspace, executable)?;
    outcome.stderr =
        canonicalized_process_stream(&outcome.stderr, &stderr_path, workspace, executable)?;
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
        file_evidence: None,
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
        file_evidence: None,
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
    let (captures, capture_assertions) =
        evaluate_json_captures(id, declared_captures, &outcome.stdout);
    assertions.extend(capture_assertions);
    let file_evidence_directory = steps_directory.join(format!("{index:03}-{id}.files"));
    assertions.extend(evaluate_file_assertions(
        workspace,
        &expect.files,
        &file_evidence_directory,
        run_directory,
    ));
    if outcome.stdout.truncated || outcome.stderr.truncated {
        assertions.push(AssertionReceipt {
            target: "process".to_owned(),
            assertion: "output_complete".to_owned(),
            passed: false,
            detail: "captured output exceeded the configured byte budget".to_owned(),
            file_evidence: None,
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
        captures,
        copied: None,
        observation: None,
        failure: (!passed).then(|| "one or more command expectations failed".to_owned()),
    })
}

pub(crate) fn evaluate_json_captures(
    source_step: &str,
    declared: &[JsonScalarCapture],
    stdout: &CapturedStream,
) -> (Vec<JsonScalarCaptureReceipt>, Vec<AssertionReceipt>) {
    if declared.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let document = if stdout.truncated {
        Err("stdout was truncated".to_owned())
    } else {
        serde_json::from_slice::<Value>(&stdout.bytes)
            .map_err(|error| format!("stdout is not valid JSON: {error}"))
    };
    let mut captured = Vec::new();
    let mut assertions = Vec::with_capacity(declared.len());
    for capture in declared {
        let result = match &document {
            Ok(document) => document
                .pointer(&capture.pointer)
                .ok_or_else(|| "JSON pointer is missing".to_owned())
                .and_then(captured_scalar_value),
            Err(error) => Err(error.clone()),
        };
        match result {
            Ok(value) => {
                captured.push(JsonScalarCaptureReceipt {
                    name: capture.name.clone(),
                    source_step: source_step.to_owned(),
                    pointer: capture.pointer.clone(),
                    value: value.clone(),
                    source_stdout_sha256: stdout.stored_sha256.clone(),
                });
                assertions.push(AssertionReceipt {
                    target: format!("capture:{}", capture.name),
                    assertion: "json_scalar_capture".to_owned(),
                    passed: true,
                    detail: format!(
                        "captured {} from step '{}' pointer '{}' stdout {}",
                        value, source_step, capture.pointer, stdout.stored_sha256
                    ),
                    file_evidence: None,
                });
            }
            Err(error) => assertions.push(AssertionReceipt {
                target: format!("capture:{}", capture.name),
                assertion: "json_scalar_capture".to_owned(),
                passed: false,
                detail: format!(
                    "could not capture step '{}' pointer '{}': {error}",
                    source_step, capture.pointer
                ),
                file_evidence: None,
            }),
        }
    }
    (captured, assertions)
}

fn captured_scalar_value(value: &Value) -> std::result::Result<String, String> {
    match value {
        Value::String(value) if !value.trim().is_empty() && !value.contains('\0') => {
            Ok(value.clone())
        }
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::String(value) if value.contains('\0') => {
            Err("captured string contains NUL".to_owned())
        }
        Value::String(_) => Err("captured string is blank".to_owned()),
        Value::Null | Value::Array(_) | Value::Object(_) => {
            Err("captured value is not a string, number, or boolean".to_owned())
        }
    }
}

fn run_mediawiki_update_step(
    index: usize,
    id: &str,
    page: &crate::model::MediaWikiPage,
    service: &MediaWikiService,
    steps_directory: &Path,
    run_directory: &Path,
) -> Result<StepReceipt> {
    let started = Instant::now();
    service.update_page(page.clone())?;
    let observation = service.observation()?;
    let observation_path = steps_directory.join(format!("{index:03}-{id}.mediawiki.json"));
    atomic_write_json(&observation_path, &observation)?;
    let (sha256, bytes) = sha256_file(&observation_path)?;
    Ok(StepReceipt {
        id: id.to_owned(),
        action: "mediawiki_update".to_owned(),
        status: RunStatus::Passed,
        duration_ms: started.elapsed().as_millis(),
        argv: Vec::new(),
        exit_code: None,
        timed_out: None,
        stdout: None,
        stderr: None,
        assertions: vec![AssertionReceipt {
            target: format!("mediawiki.page:{}", page.title),
            assertion: "fixture_page_updated".to_owned(),
            passed: true,
            detail: format!("revision_id={}", page.revision_id),
            file_evidence: None,
        }],
        captures: Vec::new(),
        copied: None,
        observation: Some(ArtifactIdentity {
            locator: relative_locator(run_directory, &observation_path)?,
            sha256,
            bytes,
        }),
        failure: None,
    })
}

fn run_mediawiki_assert_step(
    index: usize,
    id: &str,
    expect: &crate::model::MediaWikiExpectation,
    service: &MediaWikiService,
    steps_directory: &Path,
    run_directory: &Path,
) -> Result<StepReceipt> {
    let started = Instant::now();
    let observation = service.observation()?;
    let assertions = evaluate_expectation(&observation, expect);
    let passed = assertions.iter().all(|assertion| assertion.passed);
    let observation_path = steps_directory.join(format!("{index:03}-{id}.mediawiki.json"));
    atomic_write_json(&observation_path, &observation)?;
    let (sha256, bytes) = sha256_file(&observation_path)?;
    Ok(StepReceipt {
        id: id.to_owned(),
        action: "mediawiki_assert".to_owned(),
        status: if passed {
            RunStatus::Passed
        } else {
            RunStatus::Failed
        },
        duration_ms: started.elapsed().as_millis(),
        argv: Vec::new(),
        exit_code: None,
        timed_out: None,
        stdout: None,
        stderr: None,
        assertions,
        captures: Vec::new(),
        copied: None,
        observation: Some(ArtifactIdentity {
            locator: relative_locator(run_directory, &observation_path)?,
            sha256,
            bytes,
        }),
        failure: (!passed).then(|| "one or more MediaWiki expectations failed".to_owned()),
    })
}

pub(crate) fn evaluate_output_assertions(
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
                    file_evidence: None,
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
                    file_evidence: None,
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
                    file_evidence: None,
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
                    file_evidence: None,
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
                    file_evidence: None,
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
                    file_evidence: None,
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
                    file_evidence: None,
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
                    file_evidence: None,
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

fn evaluate_file_assertions(
    root: &Path,
    assertions: &[FileAssertion],
    evidence_directory: &Path,
    run_directory: &Path,
) -> Vec<AssertionReceipt> {
    assertions
        .iter()
        .enumerate()
        .map(|(index, assertion)| {
            capture_file_observation(
                root,
                assertion.path(),
                evidence_directory,
                run_directory,
                index,
            )
            .and_then(|(evidence, bytes)| {
                evaluate_file_assertion(assertion, evidence, bytes.as_deref())
            })
            .unwrap_or_else(|error| AssertionReceipt {
                target: assertion.path().to_owned(),
                assertion: file_assertion_name(assertion).to_owned(),
                passed: false,
                detail: format!("{error:#}"),
                file_evidence: None,
            })
        })
        .collect()
}

fn capture_file_observation(
    root: &Path,
    relative: &str,
    evidence_directory: &Path,
    run_directory: &Path,
    index: usize,
) -> Result<(FileObservation, Option<Vec<u8>>)> {
    let path = resolve_output_path(root, relative)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((
                FileObservation {
                    state: FileObservationState::Missing,
                    artifact: None,
                },
                None,
            ));
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok((
            FileObservation {
                state: FileObservationState::Other,
                artifact: None,
            },
            None,
        ));
    }
    if metadata.len() > FILE_ASSERTION_BUDGET {
        bail!(
            "{} exceeds the {} byte file assertion budget",
            path.display(),
            FILE_ASSERTION_BUDGET
        );
    }
    let bytes = fs::read(&path)?;
    let snapshot = evidence_directory.join(format!("{index:03}.bin"));
    atomic_write(&snapshot, &bytes)?;
    let evidence = FileObservation {
        state: FileObservationState::PlainFile,
        artifact: Some(ArtifactIdentity {
            locator: relative_locator(run_directory, &snapshot)?,
            sha256: sha256_bytes(&bytes),
            bytes: bytes.len() as u64,
        }),
    };
    Ok((evidence, Some(bytes)))
}

pub(crate) fn evaluate_file_assertion(
    assertion: &FileAssertion,
    evidence: FileObservation,
    bytes: Option<&[u8]>,
) -> Result<AssertionReceipt> {
    let (passed, detail) = match assertion {
        FileAssertion::Exists { .. } => (
            evidence.state == FileObservationState::PlainFile,
            if evidence.state == FileObservationState::PlainFile {
                "plain file exists".to_owned()
            } else {
                "plain file does not exist".to_owned()
            },
        ),
        FileAssertion::Missing { .. } => (
            evidence.state == FileObservationState::Missing,
            if evidence.state == FileObservationState::Missing {
                "path is absent".to_owned()
            } else {
                "path exists".to_owned()
            },
        ),
        FileAssertion::Contains { value, .. } | FileAssertion::NotContains { value, .. } => {
            let text =
                String::from_utf8(bytes.context("asserted file bytes are missing")?.to_vec())
                    .context("asserted file is not UTF-8")?;
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
            let observed = sha256_bytes(bytes.context("asserted file bytes are missing")?);
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
        file_evidence: Some(evidence),
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

fn canonicalized_process_stream(
    stream: &CapturedStream,
    retained_path: &Path,
    workspace: &Path,
    executable: &Path,
) -> Result<CapturedStream> {
    let bytes = if stream.truncated {
        Vec::new()
    } else {
        canonicalize_scenario_output(&stream.bytes, workspace, executable)?
    };
    atomic_write(retained_path, &bytes)?;
    let stored_sha256 = sha256_bytes(&bytes);
    if stream.truncated {
        return Ok(CapturedStream {
            sha256: stream.sha256.clone(),
            stored_sha256,
            observed_bytes: stream.observed_bytes,
            stored_bytes: 0,
            truncated: true,
            bytes,
        });
    }
    let bytes_len = bytes.len() as u64;
    Ok(CapturedStream {
        sha256: stored_sha256.clone(),
        stored_sha256,
        observed_bytes: bytes_len,
        stored_bytes: bytes_len,
        truncated: false,
        bytes,
    })
}

fn canonicalize_scenario_output(
    bytes: &[u8],
    workspace: &Path,
    executable: &Path,
) -> Result<Vec<u8>> {
    let execution_workspaces = workspace
        .parent()
        .context("execution workspace has no runner-owned parent")?;
    canonicalize_exact_paths(
        bytes,
        &[
            (
                workspace.join(".wikitool/config.toml"),
                WORKSPACE_CONFIG_TOKEN,
            ),
            (workspace.join(".wikitool/data"), WORKSPACE_DATA_TOKEN),
            (workspace.to_path_buf(), WORKSPACE_ROOT_TOKEN),
            (
                execution_workspaces.to_path_buf(),
                EXECUTION_WORKSPACES_TOKEN,
            ),
            (executable.to_path_buf(), TOOL_BINARY_TOKEN),
        ],
    )
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
    for (index, requirement) in scenario.requirements.iter().enumerate() {
        match requirement {
            Requirement::PathExists { path, on_missing } => {
                let expanded = variables.expand(path)?;
                let metadata = match fs::metadata(&expanded) {
                    Ok(metadata) => Some(metadata),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("failed to inspect required path {expanded}")
                        });
                    }
                };
                let exists = metadata.is_some();
                let expanded_path = portable(Path::new(&expanded));
                let observation = RequirementObservation {
                    schema: REQUIREMENT_OBSERVATION_SCHEMA.to_owned(),
                    kind: "path_exists".to_owned(),
                    declared_path: path.clone(),
                    expanded_path: expanded_path.clone(),
                    exists,
                    path_kind: metadata.as_ref().map(|metadata| {
                        if metadata.is_file() {
                            "file"
                        } else if metadata.is_dir() {
                            "directory"
                        } else {
                            "other"
                        }
                        .to_owned()
                    }),
                };
                let observation_path = variables
                    .run_directory
                    .join(format!("requirements/{index:04}-path-exists.json"));
                atomic_write_json(&observation_path, &observation)?;
                let (sha256, bytes) = sha256_file(&observation_path)?;
                receipt.requirements.push(RequirementReceipt {
                    kind: "path_exists".to_owned(),
                    passed: exists,
                    disposition: *on_missing,
                    detail: if exists {
                        format!("path exists: {expanded_path}")
                    } else {
                        format!("path is missing: {expanded_path}")
                    },
                    observation: ArtifactIdentity {
                        locator: relative_locator(variables.run_directory, &observation_path)?,
                        sha256,
                        bytes,
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
        ScenarioStep::MediaWikiUpdate { .. } => "mediawiki_update",
        ScenarioStep::MediaWikiAssert { .. } => "mediawiki_assert",
    }
}

struct Variables<'a> {
    repository: &'a Path,
    scenario_directory: &'a Path,
    run_directory: &'a Path,
    workspace: &'a Path,
    host_root: Option<&'a Path>,
    mediawiki_api_url: Option<&'a str>,
    captures: BTreeMap<String, String>,
}

impl Variables<'_> {
    fn expand(&self, value: &str) -> Result<String> {
        let mut output = String::with_capacity(value.len());
        let mut remaining = value;
        while !remaining.is_empty() {
            if let Some(after_start) = remaining.strip_prefix("${") {
                let end = after_start
                    .find('}')
                    .with_context(|| format!("unterminated capture interpolation in {value:?}"))?;
                let name = &after_start[..end];
                let replacement = self
                    .captures
                    .get(name)
                    .with_context(|| format!("capture '{name}' is unavailable in this run"))?;
                output.push_str(replacement);
                remaining = &after_start[end + 1..];
            } else if let Some(after_start) = remaining.strip_prefix('{') {
                let end = after_start
                    .find('}')
                    .with_context(|| format!("unterminated interpolation token in {value:?}"))?;
                let name = &after_start[..end];
                match name {
                    "REPO_ROOT" => output.push_str(&portable(self.repository)),
                    "SCENARIO_DIR" => output.push_str(&portable(self.scenario_directory)),
                    "RUN_DIR" => output.push_str(&portable(self.run_directory)),
                    "WORKSPACE" => output.push_str(&portable(self.workspace)),
                    "HOST_ROOT" => output.push_str(&portable(
                        self.host_root
                            .context("{HOST_ROOT} is unavailable in this run")?,
                    )),
                    "MEDIAWIKI_API_URL" => output.push_str(
                        self.mediawiki_api_url
                            .context("{MEDIAWIKI_API_URL} is unavailable in this run")?,
                    ),
                    _ => bail!("unknown interpolation token '{{{name}}}' in {value:?}"),
                }
                remaining = &after_start[end + 1..];
            } else if remaining.starts_with('}') {
                bail!("unmatched interpolation terminator in {value:?}");
            } else {
                let ch = remaining
                    .chars()
                    .next()
                    .context("empty interpolation input")?;
                output.push(ch);
                remaining = &remaining[ch.len_utf8()..];
            }
        }
        Ok(output)
    }

    fn bind_captures(&mut self, captures: &[JsonScalarCaptureReceipt]) -> Result<()> {
        for capture in captures {
            if self.captures.contains_key(&capture.name) {
                bail!("capture '{}' would be redefined", capture.name);
            }
        }
        for capture in captures {
            self.captures
                .insert(capture.name.clone(), capture.value.clone());
        }
        Ok(())
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
            mediawiki_api_url: None,
            captures: BTreeMap::new(),
        };
        assert!(variables.expand("{UNKNOWN}").is_err());
        assert!(variables.expand("${UNKNOWN}").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn path_interpolation_normalizes_verbatim_windows_roots_before_suffixes() {
        let root = Path::new(r"\\?\F:\AI\wiki");
        let variables = Variables {
            repository: root,
            scenario_directory: root,
            run_directory: root,
            workspace: root,
            host_root: Some(root),
            mediawiki_api_url: None,
            captures: BTreeMap::new(),
        };

        assert_eq!(
            variables
                .expand("{HOST_ROOT}/.wikitool/data/wikitool.db")
                .expect("expand host path"),
            "F:/AI/wiki/.wikitool/data/wikitool.db"
        );
    }

    #[test]
    fn retained_scenario_output_hides_windows_runner_paths() {
        let workspace = PathBuf::from(
            r"\\?\C:\Users\Onno\AppData\Local\Temp\wikitest-execution-workspaces\run-1",
        );
        let executable = PathBuf::from(r"\\?\F:\AI\wiki\dist\wikitool.exe");
        let input = br#"root=C:/Users/Onno/AppData/Local/Temp/wikitest-execution-workspaces/run-1 config=\\\\?\\C:\\Users\\Onno\\AppData\\Local\\Temp\\wikitest-execution-workspaces\\run-1\\.wikitool\\config.toml backup=//?/C:/Users/Onno/AppData/Local/Temp/wikitest-execution-workspaces/run-1/.wikitool/sync/backups tool=F:/AI/wiki/dist/wikitool.exe"#;
        let canonical =
            canonicalize_scenario_output(input, &workspace, &executable).expect("canonical output");
        let text = String::from_utf8(canonical).expect("utf8 output");
        assert!(text.contains(WORKSPACE_ROOT_TOKEN));
        assert!(text.contains(WORKSPACE_CONFIG_TOKEN));
        assert!(text.contains(TOOL_BINARY_TOKEN));
        assert!(!text.contains("Onno"));
        assert!(!text.contains("AppData"));
        assert!(!text.contains("//?/C:"));
        assert!(!text.contains(r"\\?\C:"));
    }

    #[test]
    fn json_captures_bind_hash_bound_scalars_for_later_argv() {
        let stdout = output(
            br#"{"report":{"plan_id":"plan-123","count":2,"ready":true}}"#,
            false,
        );
        let declared = vec![
            JsonScalarCapture {
                name: "PLAN_ID".to_owned(),
                pointer: "/report/plan_id".to_owned(),
            },
            JsonScalarCapture {
                name: "COUNT".to_owned(),
                pointer: "/report/count".to_owned(),
            },
            JsonScalarCapture {
                name: "READY".to_owned(),
                pointer: "/report/ready".to_owned(),
            },
        ];
        let (captures, assertions) = evaluate_json_captures("preview", &declared, &stdout);
        assert!(assertions.iter().all(|assertion| assertion.passed));
        assert_eq!(
            captures
                .iter()
                .map(|capture| capture.value.as_str())
                .collect::<Vec<_>>(),
            ["plan-123", "2", "true"]
        );
        assert!(
            captures
                .iter()
                .all(|capture| capture.source_stdout_sha256 == stdout.stored_sha256)
        );

        let root = Path::new("root");
        let mut variables = Variables {
            repository: root,
            scenario_directory: root,
            run_directory: root,
            workspace: root,
            host_root: None,
            mediawiki_api_url: None,
            captures: BTreeMap::new(),
        };
        variables.bind_captures(&captures).expect("bind captures");
        assert_eq!(
            variables
                .expand("push --apply ${PLAN_ID}")
                .expect("expand capture"),
            "push --apply plan-123"
        );
        assert!(variables.bind_captures(&captures).is_err());
    }

    #[test]
    fn json_captures_fail_on_missing_blank_nonscalar_or_truncated_values() {
        let declared = [
            JsonScalarCapture {
                name: "MISSING".to_owned(),
                pointer: "/missing".to_owned(),
            },
            JsonScalarCapture {
                name: "OBJECT".to_owned(),
                pointer: "/object".to_owned(),
            },
            JsonScalarCapture {
                name: "BLANK".to_owned(),
                pointer: "/blank".to_owned(),
            },
        ];
        let (captures, assertions) = evaluate_json_captures(
            "preview",
            &declared,
            &output(br#"{"object":{},"blank":"  "}"#, false),
        );
        assert!(captures.is_empty());
        assert!(assertions.iter().all(|assertion| !assertion.passed));

        let (captures, assertions) =
            evaluate_json_captures("preview", &declared[..1], &output(b"{}", true));
        assert!(captures.is_empty());
        assert!(!assertions[0].passed);
        assert!(assertions[0].detail.contains("truncated"));
    }

    #[test]
    fn host_snapshot_is_mutable_without_mutating_source_and_detects_source_drift() {
        let directory = tempfile::tempdir().expect("tempdir");
        let host = directory.path().join("host");
        let workspace = directory.path().join("workspace");
        let run = directory.path().join("run");
        let inputs_directory = run.join("inputs");
        fs::create_dir_all(host.join(".wikitool/data")).expect("host data");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::create_dir_all(&inputs_directory).expect("inputs");
        fs::write(host.join(".wikitool/data/wikitool.db"), b"host-db").expect("host db");
        let mut inputs = Vec::new();
        let snapshot = snapshot_host_root(&host, &workspace, &inputs_directory, &run, &mut inputs)
            .expect("snapshot");

        fs::write(
            workspace.join(".wikitool/data/wikitool.db"),
            b"snapshot mutation",
        )
        .expect("mutate snapshot");
        verify_host_unchanged(&snapshot).expect("host remains unchanged");
        assert_eq!(
            fs::read(host.join(".wikitool/data/wikitool.db")).expect("host bytes"),
            b"host-db"
        );

        fs::write(host.join(".wikitool/data/wikitool.db"), b"host mutation").expect("mutate host");
        assert!(verify_host_unchanged(&snapshot).is_err());
    }

    #[test]
    fn host_snapshot_refuses_an_adapter_outside_the_retained_tree() {
        let directory = tempfile::tempdir().expect("tempdir");
        let host = directory.path().join("host");
        let workspace = directory.path().join("workspace");
        let run = directory.path().join("run");
        let outside = directory.path().join("outside/site-adapter.toml");
        fs::create_dir_all(host.join(".wikitool")).expect("host config directory");
        fs::create_dir_all(outside.parent().expect("outside parent")).expect("outside directory");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::create_dir_all(run.join("inputs")).expect("inputs");
        fs::write(&outside, b"schema = \"wikitool.site-adapter.v1\"\n").expect("external adapter");
        let escaped = outside.to_string_lossy().replace('\\', "\\\\");
        fs::write(
            host.join(".wikitool/config.toml"),
            format!("[adapter]\npath = \"{escaped}\"\n"),
        )
        .expect("host config");

        let error = snapshot_host_root(
            &host,
            &workspace,
            &run.join("inputs"),
            &run,
            &mut Vec::new(),
        )
        .expect_err("external adapter must be refused");
        assert!(error.to_string().contains("adapter.path"));
    }

    #[test]
    fn execution_workspace_has_a_local_empty_dotenv() {
        let run_id = format!(
            "workspace-test-{}-{}",
            std::process::id(),
            unix_ms().unwrap()
        );
        let workspace = create_execution_workspace(&run_id).expect("workspace");
        assert_eq!(fs::read(workspace.join(".env")).expect("dotenv"), b"");
        assert!(!workspace.starts_with(env!("CARGO_MANIFEST_DIR")));
    }
}
