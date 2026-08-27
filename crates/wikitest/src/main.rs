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
    resolve_manifest, resolve_wikitool, scan_catalogs,
};
use wikitest::inspection::inspect_receipt;
use wikitest::model::{
    RECEIPT_SCHEMA, RunStatus, SCENARIO_SCHEMA, SUITE_RECEIPT_SCHEMA, SUITE_SCHEMA,
};
use wikitest::runner::{RunOptions, run_scenario, run_suite};

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
    /// Verify a run or suite receipt against its retained artifacts.
    Inspect { receipt: PathBuf },
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
                "manifest_schemas": [SCENARIO_SCHEMA, SUITE_SCHEMA],
                "receipt_schemas": [RECEIPT_SCHEMA, SUITE_RECEIPT_SCHEMA],
                "commands": ["describe", "list", "validate", "run", "suite", "inspect"],
                "scenario_kinds": ["mechanical", "knowledge"],
                "environments": ["isolated", "host_read_only"],
                "authority": {
                    "deterministic": "Wikitest can assert process, structured output, file, hash, and completeness facts.",
                    "editorial": "Prose quality is outside deterministic scenario authority and requires a separate named adjudicator.",
                    "host_safety": "host_read_only manifests are schema-checked against an explicit command allowlist."
                }
            });
            render_value(cli.format, &value, || {
                [
                    "Wikitest — executable Wikitool dogfooding laboratory".to_owned(),
                    format!("scenario schema: {SCENARIO_SCHEMA}"),
                    format!("suite schema: {SUITE_SCHEMA}"),
                    "mechanical and knowledge assertions are deterministic; prose judgment is external".to_owned(),
                ]
                .join("\n")
            })?;
            Ok(0)
        }
        Command::List => {
            let rows = scan_catalogs(&catalogs)?
                .into_iter()
                .map(|entry| {
                    let (scenario_kind, environment) = match &entry.manifest {
                        Manifest::Scenario(value) => (
                            Some(
                                match value.kind {
                                    wikitest::model::ScenarioKind::Mechanical => "mechanical",
                                    wikitest::model::ScenarioKind::Knowledge => "knowledge",
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
                        ),
                        Manifest::Suite(_) => (None, None),
                    };
                    CatalogRow {
                        kind: entry.manifest.kind().to_owned(),
                        id: entry.manifest.id().to_owned(),
                        title: entry.manifest.title().to_owned(),
                        path: display_catalog_path(&repository, &entry.path),
                        scenario_kind,
                        environment,
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
            let options = run_options(&cli, &repository)?;
            let run = run_scenario(&path, &options)?;
            let value = serde_json::to_value(&run.receipt)?;
            render_value(cli.format, &value, || {
                format!(
                    "{}: {:?}\nreceipt: {}",
                    run.receipt.scenario.id,
                    run.receipt.status,
                    portable(&run.receipt_path)
                )
            })?;
            Ok(status_exit(run.receipt.status, require_all))
        }
        Command::Suite { suite, require_all } => {
            validate_output_budget(cli.max_output_bytes)?;
            let path = resolve_manifest(&suite, &catalogs, "suite")?;
            let options = run_options(&cli, &repository)?;
            let run = run_suite(&path, &options, require_all)?;
            let value = serde_json::to_value(&run.receipt)?;
            render_value(cli.format, &value, || {
                format!(
                    "{}: {:?} ({} run(s))\nreceipt: {}",
                    run.receipt.suite.id,
                    run.receipt.status,
                    run.receipt.runs.len(),
                    portable(&run.receipt_path)
                )
            })?;
            Ok(status_exit(run.receipt.status, false))
        }
        Command::Inspect { receipt } => {
            let receipt = canonicalize_existing(&receipt)?;
            let inspection = inspect_receipt(&receipt, &repository)?;
            let value = serde_json::to_value(&inspection)?;
            render_value(cli.format, &value, || {
                let mut lines = vec![format!(
                    "verified: {} ({:?})",
                    inspection.verified, inspection.source_status
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
            Ok(if inspection.verified { 0 } else { 1 })
        }
    }
}

fn run_options(cli: &Cli, repository: &Path) -> Result<RunOptions> {
    let artifacts_root = cli
        .artifacts
        .clone()
        .unwrap_or_else(|| repository.join(".wikitest/runs"));
    let wikitool = resolve_wikitool(cli.wikitool.as_deref(), repository)?;
    let mut options = RunOptions::new(repository.to_path_buf(), artifacts_root, wikitool);
    options.host_root = cli.host_root.clone();
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
