use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::json;

use wikitest::artifact::portable;
use wikitest::catalog::{
    Manifest, default_catalog, discover_repository, display_catalog_path, load_manifest,
    resolve_manifest, resolve_wikitool, scan_catalogs, validate_scenario_inputs,
};
use wikitest::inspection::inspect_receipt;
use wikitest::model::{
    RECEIPT_SCHEMA, RunStatus, SCENARIO_SCHEMA, SUITE_RECEIPT_SCHEMA, SUITE_SCHEMA,
};
use wikitest::runner::{RunOptions, run_scenario, run_suite};
use wikitest::{
    prose::{
        ProseOptions, evaluate_suite as evaluate_prose_suite, operational_participant_export_root,
        prepare_assignment, prepare_suite as prepare_prose_suite, submit_author, submit_review,
    },
    prose_model::{
        AUTHOR_REQUEST_SCHEMA, AUTHOR_SUBMISSION_SCHEMA, CLAIM_MAP_SCHEMA, PROSE_ASSIGNMENT_SCHEMA,
        PROSE_RECEIPT_SCHEMA, PROSE_SUITE_RECEIPT_SCHEMA, PROSE_SUITE_SCHEMA,
        REVIEW_REQUEST_SCHEMA, REVIEW_SUBMISSION_SCHEMA,
    },
};

#[derive(Debug, Parser)]
#[command(
    name = "wikitest",
    version,
    about = "Executable Wikitool dogfooding and editorial evaluation laboratory"
)]
struct Cli {
    /// Wikitool source repository containing crates/wikitest.
    #[arg(long, global = true)]
    repo_root: Option<PathBuf>,

    /// Scenario catalog root. Repeat to combine generic and host-owned packs.
    #[arg(long, global = true)]
    catalog: Vec<PathBuf>,

    /// Run artifact directory. Defaults to <repo>/.wikitest/runs.
    #[arg(long, global = true)]
    artifacts: Option<PathBuf>,

    /// Wikitool executable under evaluation.
    #[arg(long, global = true)]
    wikitool: Option<PathBuf>,

    /// Host wiki project root for host_read_only scenarios.
    #[arg(long, global = true)]
    host_root: Option<PathBuf>,

    /// Maximum retained bytes for each stdout and stderr stream.
    #[arg(long, global = true, default_value_t = 2 * 1024 * 1024)]
    max_output_bytes: usize,

    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Describe Wikitest's public schemas and authority boundaries.
    Describe,
    /// List strict scenario and suite manifests.
    List,
    /// Validate one manifest by path/id, or every configured catalog entry.
    Validate { target: Option<String> },
    /// Run one scenario through the public Wikitool executable.
    Run {
        scenario: String,
        /// Treat a declared environmental skip as a failed invocation.
        #[arg(long)]
        require_all: bool,
    },
    /// Run every scenario in a suite.
    Suite {
        suite: String,
        /// Treat any declared environmental skip as a failed suite.
        #[arg(long)]
        require_all: bool,
    },
    /// Replay a run or suite receipt against its retained evidence.
    Inspect { receipt: PathBuf },
    /// Prepare and record externally authored or reviewed prose evaluations.
    Prose {
        #[command(subcommand)]
        command: ProseCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum ProseCommand {
    /// Freeze one assignment and emit a harness-neutral author or review request.
    Prepare { assignment: String },
    /// Prepare every assignment in a prose suite.
    PrepareSuite { suite: String },
    /// Recompute demonstrated coverage from every live reviewed suite run.
    EvaluateSuite { run: PathBuf },
    /// Bind an external author's candidate and claim map to a prepared run.
    SubmitAuthor {
        run: PathBuf,
        #[arg(long)]
        submission: PathBuf,
    },
    /// Bind an independent review to the exact frozen candidate and packet.
    SubmitReview {
        run: PathBuf,
        #[arg(long)]
        submission: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Serialize)]
struct CatalogRow {
    kind: String,
    id: String,
    title: String,
    path: String,
    scenario_kind: Option<String>,
    environment: Option<String>,
    prose_mode: Option<String>,
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("wikitest: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn execute(cli: Cli) -> Result<u8> {
    let repository = discover_repository(cli.repo_root.as_deref())?;
    let catalogs = resolve_catalogs(&repository, &cli.catalog)?;
    match cli.command.clone() {
        Command::Describe => {
            let value = json!({
                "schema": "wikitest.driver.v1",
                "manifest_schemas": [SCENARIO_SCHEMA, SUITE_SCHEMA, PROSE_ASSIGNMENT_SCHEMA, PROSE_SUITE_SCHEMA],
                "receipt_schemas": [RECEIPT_SCHEMA, SUITE_RECEIPT_SCHEMA, PROSE_RECEIPT_SCHEMA, PROSE_SUITE_RECEIPT_SCHEMA],
                "submission_schemas": [AUTHOR_SUBMISSION_SCHEMA, CLAIM_MAP_SCHEMA, REVIEW_SUBMISSION_SCHEMA],
                "request_schemas": [AUTHOR_REQUEST_SCHEMA, REVIEW_REQUEST_SCHEMA],
                "commands": ["describe", "list", "validate", "run", "suite", "prose", "inspect"],
                "scenario_kinds": ["mechanical", "catalog"],
                "environments": ["isolated", "host_read_only"],
                "authority": {
                    "deterministic": "Wikitest can assert process, structured output, file, hash, and completeness facts.",
                    "editorial": "Wikitest binds prose evidence, instructions, candidates, identities, and review records; quality comes only from a distinct named reviewer, never lint or author self-attestation.",
                    "host_safety": "host_read_only manifests are schema-checked against an explicit command allowlist."
                }
            });
            render_value(cli.format, &value, || {
                [
                    "Wikitest — executable Wikitool dogfooding laboratory".to_owned(),
                    format!("scenario schema: {SCENARIO_SCHEMA}"),
                    format!("suite schema: {SUITE_SCHEMA}"),
                    "mechanical and catalog assertions are deterministic; prose judgment is external".to_owned(),
                ]
                .join("\n")
            })?;
            Ok(0)
        }
        Command::List => {
            let rows = scan_catalogs(&catalogs)?
                .into_iter()
                .map(|entry| {
                    let (scenario_kind, environment, prose_mode) = match &entry.manifest {
                        Manifest::Scenario(value) => (
                            Some(
                                match value.kind {
                                    wikitest::model::ScenarioKind::Mechanical => "mechanical",
                                    wikitest::model::ScenarioKind::Catalog => "catalog",
                                }
                                .to_owned(),
                            ),
                            Some(
                                match value.environment {
                                    wikitest::model::ScenarioEnvironment::Isolated => "isolated",
                                    wikitest::model::ScenarioEnvironment::HostReadOnly => {
                                        "host_read_only"
                                    }
                                }
                                .to_owned(),
                            ),
                            None,
                        ),
                        Manifest::ProseAssignment(value) => (
                            None,
                            None,
                            Some(
                                match value.mode {
                                    wikitest::prose_model::ProseMode::Authoring => "authoring",
                                    wikitest::prose_model::ProseMode::Review => "review",
                                }
                                .to_owned(),
                            ),
                        ),
                        Manifest::Suite(_) | Manifest::ProseSuite(_) => (None, None, None),
                    };
                    CatalogRow {
                        kind: entry.manifest.kind().to_owned(),
                        id: entry.manifest.id().to_owned(),
                        title: entry.manifest.title().to_owned(),
                        path: display_catalog_path(&repository, &entry.path),
                        scenario_kind,
                        environment,
                        prose_mode,
                    }
                })
                .collect::<Vec<_>>();
            let value = json!({"schema": "wikitest.catalog.v1", "entries": rows});
            render_value(cli.format, &value, || {
                rows.iter()
                    .map(|row| format!("{} {} — {}", row.kind, row.id, row.title))
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
            Ok(0)
        }
        Command::Validate { target } => {
            let rows = if let Some(target) = target {
                vec![resolve_any_manifest(&target, &catalogs)?]
            } else {
                scan_catalogs(&catalogs)?
            };
            for entry in &rows {
                validate_manifest_closure(&entry.path, &entry.manifest, &catalogs)?;
            }
            let validated = rows
                .iter()
                .map(|entry| {
                    json!({
                        "kind": entry.manifest.kind(),
                        "id": entry.manifest.id(),
                        "path": display_catalog_path(&repository, &entry.path),
                        "valid": true
                    })
                })
                .collect::<Vec<_>>();
            let value = json!({
                "schema": "wikitest.validation.v1",
                "valid": true,
                "count": validated.len(),
                "entries": validated
            });
            render_value(cli.format, &value, || {
                format!("valid: {} manifest(s)", validated.len())
            })?;
            Ok(0)
        }
        Command::Run {
            scenario,
            require_all,
        } => {
            validate_output_budget(cli.max_output_bytes)?;
            let path = resolve_manifest(&scenario, &catalogs, "scenario")?;
            let options = run_options(&cli, &repository, &catalogs)?;
            let run = run_scenario(&path, &options)?;
            require_replayable_receipt(&run.receipt_path, &repository)?;
            let value = serde_json::to_value(&run.receipt)?;
            render_value(cli.format, &value, || {
                let mut lines = vec![format!(
                    "{}: {:?}\nreceipt: {}",
                    run.receipt.scenario.id,
                    run.receipt.status,
                    portable(&run.receipt_path)
                )];
                if let Some(failure) = &run.receipt.failure {
                    lines.push(failure.clone());
                }
                for step in &run.receipt.steps {
                    for assertion in step.assertions.iter().filter(|assertion| !assertion.passed) {
                        lines.push(format!(
                            "[failed] {} / {} / {}: {}",
                            step.id, assertion.target, assertion.assertion, assertion.detail
                        ));
                    }
                }
                lines.join("\n")
            })?;
            Ok(status_exit(run.receipt.status, require_all))
        }
        Command::Suite { suite, require_all } => {
            validate_output_budget(cli.max_output_bytes)?;
            let path = resolve_manifest(&suite, &catalogs, "suite")?;
            let options = run_options(&cli, &repository, &catalogs)?;
            let run = run_suite(&path, &options, require_all)?;
            require_replayable_receipt(&run.receipt_path, &repository)?;
            let value = serde_json::to_value(&run.receipt)?;
            render_value(cli.format, &value, || {
                let mut lines = vec![format!(
                    "{}: {:?} ({} run(s))\nreceipt: {}",
                    run.receipt.suite.id,
                    run.receipt.status,
                    run.receipt.runs.len(),
                    portable(&run.receipt_path)
                )];
                for child in run
                    .receipt
                    .runs
                    .iter()
                    .filter(|child| child.status != RunStatus::Passed)
                {
                    lines.push(format!(
                        "[{:?}] {}: {}",
                        child.status,
                        child.scenario_id.as_deref().unwrap_or(&child.scenario),
                        child
                            .error
                            .as_deref()
                            .or(child.receipt_locator.as_deref())
                            .unwrap_or("no receipt")
                    ));
                }
                lines.join("\n")
            })?;
            Ok(status_exit(run.receipt.status, false))
        }
        Command::Inspect { receipt } => {
            let receipt = canonicalize_existing(&receipt)?;
            let inspection = inspect_receipt(&receipt, &repository)?;
            let value = serde_json::to_value(&inspection)?;
            render_value(cli.format, &value, || {
                let mut lines = vec![format!(
                    "evidence replayed: {} ({}, authenticity: {:?})",
                    inspection.evidence_replayed, inspection.source_status, inspection.authenticity
                )];
                lines.extend(inspection.checks.iter().map(|check| {
                    format!(
                        "[{}] {}: {}",
                        if check.passed { "ok" } else { "error" },
                        check.name,
                        check.detail
                    )
                }));
                lines.join("\n")
            })?;
            Ok(if inspection.evidence_replayed { 0 } else { 1 })
        }
        Command::Prose { command } => {
            validate_output_budget(cli.max_output_bytes)?;
            let options = prose_options(&cli, &repository, &catalogs)?;
            match command {
                ProseCommand::Prepare { assignment } => {
                    let path = resolve_manifest(&assignment, &catalogs, "prose_assignment")?;
                    let prepared = prepare_assignment(&path, &options)?;
                    require_replayable_receipt(&prepared.receipt_path, &repository)?;
                    let value = serde_json::to_value(&prepared.receipt)?;
                    render_value(cli.format, &value, || {
                        format!(
                            "{}: {:?}\nreceipt: {}\nrequest: {}\noutput: {}",
                            prepared.receipt.public_assignment.id,
                            prepared.receipt.status,
                            portable(&prepared.receipt_path),
                            prose_request_path(&prepared.receipt_path, &prepared.receipt)
                                .unwrap_or_else(|| "<none>".to_owned()),
                            prose_output_path(&prepared.receipt)
                                .unwrap_or_else(|| "<none>".to_owned())
                        )
                    })?;
                    Ok(0)
                }
                ProseCommand::PrepareSuite { suite } => {
                    let path = resolve_manifest(&suite, &catalogs, "prose_suite")?;
                    let prepared = prepare_prose_suite(&path, &options)?;
                    require_replayable_receipt(&prepared.receipt_path, &repository)?;
                    let value = serde_json::to_value(&prepared.receipt)?;
                    let suite_root = prepared
                        .receipt_path
                        .parent()
                        .context("prose suite receipt has no parent")?
                        .to_path_buf();
                    render_value(cli.format, &value, || {
                        let mut lines = vec![format!(
                            "{}: prepared {} assignment(s)\nreceipt: {}",
                            prepared.receipt.suite.id,
                            prepared.receipt.runs.len(),
                            portable(&prepared.receipt_path)
                        )];
                        lines.extend(prepared.receipt.runs.iter().map(|run| {
                            format!(
                                "{}: {}",
                                run.assignment_id,
                                portable(&suite_root.join(&run.run_locator))
                            )
                        }));
                        lines.join("\n")
                    })?;
                    Ok(0)
                }
                ProseCommand::EvaluateSuite { run } => {
                    require_replayable_receipt(&prose_receipt_candidate(&run), &repository)?;
                    let evaluated = evaluate_prose_suite(&run, &options)?;
                    require_replayable_receipt(&evaluated.receipt_path, &repository)?;
                    let value = serde_json::to_value(&evaluated.receipt)?;
                    render_value(cli.format, &value, || {
                        format!(
                            "{}: {:?} (demonstrated {}/{})\nreceipt: {}",
                            evaluated.receipt.suite.id,
                            evaluated.receipt.status,
                            evaluated.receipt.demonstrated_coverage.len(),
                            evaluated.receipt.required_coverage.len(),
                            portable(&evaluated.receipt_path)
                        )
                    })?;
                    Ok(match evaluated.receipt.status {
                        wikitest::prose_model::ProseSuiteStatus::Failed => 1,
                        wikitest::prose_model::ProseSuiteStatus::Prepared
                        | wikitest::prose_model::ProseSuiteStatus::Passed => 0,
                    })
                }
                ProseCommand::SubmitAuthor { run, submission } => {
                    require_replayable_receipt(&prose_receipt_candidate(&run), &repository)?;
                    let prepared = submit_author(&run, &submission, &options)?;
                    require_replayable_receipt(&prepared.receipt_path, &repository)?;
                    let value = serde_json::to_value(&prepared.receipt)?;
                    render_value(cli.format, &value, || {
                        format!(
                            "{}: {:?}\nreceipt: {}\nreview request: {}\noutput: {}",
                            prepared.receipt.public_assignment.id,
                            prepared.receipt.status,
                            portable(&prepared.receipt_path),
                            prose_request_path(&prepared.receipt_path, &prepared.receipt)
                                .unwrap_or_else(|| "<none>".to_owned()),
                            prose_output_path(&prepared.receipt)
                                .unwrap_or_else(|| "<none>".to_owned())
                        )
                    })?;
                    Ok(0)
                }
                ProseCommand::SubmitReview { run, submission } => {
                    require_replayable_receipt(&prose_receipt_candidate(&run), &repository)?;
                    let prepared = submit_review(&run, &submission, &options)?;
                    require_replayable_receipt(&prepared.receipt_path, &repository)?;
                    let oracle = prepared
                        .receipt
                        .review
                        .as_ref()
                        .and_then(|review| review.oracle.as_ref());
                    let oracle_passed = oracle.is_some_and(|oracle| oracle.passed);
                    let value = serde_json::to_value(&prepared.receipt)?;
                    render_value(cli.format, &value, || {
                        format!(
                            "{}: {:?}\nreceipt: {}\noracle: {}",
                            prepared.receipt.public_assignment.id,
                            prepared.receipt.status,
                            portable(&prepared.receipt_path),
                            match oracle {
                                None => "not configured",
                                Some(oracle) if oracle.passed => "satisfied",
                                Some(_) => "not satisfied",
                            }
                        )
                    })?;
                    Ok(if oracle_passed { 0 } else { 1 })
                }
            }
        }
    }
}

fn prose_receipt_candidate(run: &Path) -> PathBuf {
    if run.is_dir() {
        run.join("receipt.json")
    } else {
        run.to_path_buf()
    }
}

fn require_replayable_receipt(receipt_path: &Path, repository: &Path) -> Result<()> {
    let inspection = inspect_receipt(receipt_path, repository)?;
    if inspection.evidence_replayed {
        return Ok(());
    }

    let failed = inspection
        .checks
        .iter()
        .filter(|check| !check.passed)
        .map(|check| format!("{}: {}", check.name, check.detail))
        .collect::<Vec<_>>()
        .join("; ");
    bail!(
        "Wikitest wrote an unverifiable receipt at {}: {}",
        portable(receipt_path),
        failed
    );
}

fn prose_options(cli: &Cli, repository: &Path, catalogs: &[PathBuf]) -> Result<ProseOptions> {
    Ok(ProseOptions {
        repository: repository.to_path_buf(),
        artifacts_root: cli
            .artifacts
            .clone()
            .unwrap_or_else(|| repository.join(".wikitest/runs")),
        wikitool: resolve_wikitool(cli.wikitool.as_deref(), repository)?,
        catalogs: catalogs.to_vec(),
        maximum_output_bytes: cli.max_output_bytes,
    })
}

fn prose_request_path(
    receipt_path: &Path,
    receipt: &wikitest::prose_model::ProseReceipt,
) -> Option<String> {
    if let Some(export) = receipt
        .review_export
        .as_ref()
        .or(receipt.author_export.as_ref())
    {
        let root = operational_participant_export_root(&receipt.run_id, export).ok()?;
        return Some(portable(&root.join(&export.request.locator)));
    }
    let root = receipt_path.parent()?;
    let artifact = receipt
        .review_request
        .as_ref()
        .or(receipt.author_request.as_ref())?;
    Some(portable(&root.join(&artifact.locator)))
}

fn prose_output_path(receipt: &wikitest::prose_model::ProseReceipt) -> Option<String> {
    let export = receipt
        .review_export
        .as_ref()
        .or(receipt.author_export.as_ref())?;
    let root = operational_participant_export_root(&receipt.run_id, export).ok()?;
    Some(portable(&root.join(&export.output_directory)))
}

fn run_options(cli: &Cli, repository: &Path, catalogs: &[PathBuf]) -> Result<RunOptions> {
    let artifacts_root = cli
        .artifacts
        .clone()
        .unwrap_or_else(|| repository.join(".wikitest/runs"));
    let wikitool = resolve_wikitool(cli.wikitool.as_deref(), repository)?;
    let mut options = RunOptions::new(repository.to_path_buf(), artifacts_root, wikitool);
    options.host_root = cli.host_root.clone();
    options.catalogs = catalogs.to_vec();
    options.maximum_output_bytes = cli.max_output_bytes;
    Ok(options)
}

fn resolve_catalogs(repository: &Path, configured: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let candidates = if configured.is_empty() {
        vec![default_catalog(repository)]
    } else {
        configured.to_vec()
    };
    candidates
        .into_iter()
        .map(|path| {
            let absolute = if path.is_absolute() {
                path
            } else {
                env::current_dir()?.join(path)
            };
            std::fs::canonicalize(&absolute)
                .with_context(|| format!("failed to resolve catalog {}", absolute.display()))
        })
        .collect()
}

fn resolve_any_manifest(
    selector: &str,
    catalogs: &[PathBuf],
) -> Result<wikitest::catalog::CatalogEntry> {
    let explicit = PathBuf::from(selector);
    if explicit.exists() {
        let path = canonicalize_existing(&explicit)?;
        let (manifest, _) = load_manifest(&path)?;
        return Ok(wikitest::catalog::CatalogEntry { path, manifest });
    }
    let matches = scan_catalogs(catalogs)?
        .into_iter()
        .filter(|entry| entry.manifest.id() == selector)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [entry] => Ok(entry.clone()),
        [] => bail!("no manifest named '{selector}' in configured catalogs"),
        _ => bail!("manifest id '{selector}' is ambiguous across kinds"),
    }
}

fn validate_manifest_closure(path: &Path, manifest: &Manifest, catalogs: &[PathBuf]) -> Result<()> {
    match manifest {
        Manifest::Suite(suite) => {
            let mut observed = std::collections::BTreeSet::new();
            for member in &suite.scenarios {
                let path = resolve_manifest(member, catalogs, "scenario")?;
                let (resolved, _) = load_manifest(&path)?;
                let Manifest::Scenario(scenario) = resolved else {
                    bail!("suite member '{member}' resolved to the wrong manifest kind");
                };
                scenario.validate()?;
                validate_scenario_inputs(&path, &scenario)?;
                observed.extend(
                    scenario
                        .coverage
                        .into_iter()
                        .map(|binding| binding.capability),
                );
            }
            require_declared_coverage(manifest.id(), &suite.required_coverage, &observed)
        }
        Manifest::ProseSuite(suite) => {
            let mut observed = std::collections::BTreeSet::new();
            for member in &suite.assignments {
                let path = resolve_manifest(member, catalogs, "prose_assignment")?;
                let (resolved, _) = load_manifest(&path)?;
                let Manifest::ProseAssignment(assignment) = resolved else {
                    bail!("suite member '{member}' resolved to the wrong manifest kind");
                };
                observed.extend(assignment.coverage);
            }
            require_declared_coverage(manifest.id(), &suite.required_coverage, &observed)
        }
        Manifest::Scenario(scenario) => validate_scenario_inputs(path, scenario),
        Manifest::ProseAssignment(_) => Ok(()),
    }
}

fn require_declared_coverage(
    suite: &str,
    required: &[String],
    observed: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let missing = required
        .iter()
        .filter(|capability| !observed.contains(capability.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!("suite '{suite}' is missing required coverage {missing:?}");
    }
    Ok(())
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).with_context(|| format!("failed to resolve {}", path.display()))
}

fn validate_output_budget(value: usize) -> Result<()> {
    if !(1024..=64 * 1024 * 1024).contains(&value) {
        bail!("--max-output-bytes must be in [1024, 67108864]");
    }
    Ok(())
}

fn status_exit(status: RunStatus, require_all: bool) -> u8 {
    match status {
        RunStatus::Passed => 0,
        RunStatus::Skipped if !require_all => 0,
        RunStatus::Skipped | RunStatus::Failed | RunStatus::Error => 1,
    }
}

fn render_value<T>(format: OutputFormat, value: &T, text: impl FnOnce() -> String) -> Result<()>
where
    T: Serialize,
{
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value)?),
        OutputFormat::Text => println!("{}", text()),
    }
    Ok(())
}
