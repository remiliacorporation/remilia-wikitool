use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::mw::{RenderCheckOptions, RenderCheckReport, render_check_wikitext};
use wikitool_core::runtime::ResolvedPaths;
use wikitool_core::site::{
    TemplateContractAssessment, assess_template_engineering_contract,
    capture_template_engineering_contract, parse_template_engineering_contract,
    render_template_scaffold,
};

use super::load_or_sync_catalog;
use crate::RuntimeOptions;
use crate::cli_support::{
    OutputFormat, normalize_path, resolve_runtime_paths, resolve_runtime_with_config,
};

const MAX_TEMPLATE_CONTRACT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Args)]
pub(super) struct TemplatesContractArgs {
    #[command(subcommand)]
    command: TemplatesContractSubcommand,
}

#[derive(Debug, Subcommand)]
enum TemplatesContractSubcommand {
    #[command(about = "Capture an observed local template as an unapproved contract starter")]
    Capture(TemplatesContractCaptureArgs),
    #[command(about = "Validate a template contract and assess compatibility")]
    Check(TemplatesContractCheckArgs),
    #[command(about = "Execute contract render fixtures through the configured MediaWiki parser")]
    RenderCheck(TemplatesContractRenderCheckArgs),
}

#[derive(Debug, Args)]
struct TemplatesContractCaptureArgs {
    #[arg(value_name = "TEMPLATE")]
    template: String,
    #[arg(
        long,
        value_name = "PATH",
        help = "Write a new project-scoped contract starter; existing files are refused"
    )]
    output: PathBuf,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct TemplatesContractCheckArgs {
    #[arg(value_name = "CONTRACT")]
    contract: PathBuf,
    #[arg(
        long,
        value_name = "TEMPLATE",
        help = "Compare with a specific catalog template instead of the contract title"
    )]
    against: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Write the full assessment atomically inside the project root"
    )]
    output: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct TemplatesContractRenderCheckArgs {
    #[arg(value_name = "CONTRACT")]
    contract: PathBuf,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
pub(super) struct TemplatesScaffoldArgs {
    #[arg(value_name = "CONTRACT")]
    contract: PathBuf,
    #[arg(
        long,
        value_name = "PATH",
        help = "Exact project-scoped template output path"
    )]
    output: PathBuf,
    #[arg(
        long,
        value_name = "PLAN_ID",
        help = "Apply the exact content/path/current-state-bound scaffold plan"
    )]
    apply: Option<String>,
    #[arg(
        long,
        help = "Authorize replacing an existing different file during apply"
    )]
    overwrite: bool,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct TemplateContractWriteReceipt<'a> {
    schema: &'static str,
    output_path: &'a str,
    content_sha256: &'a str,
    status: &'a str,
    compatibility: &'a str,
    finding_count: usize,
    parameter_change_count: usize,
    render_fixture_count: usize,
}

#[derive(Debug, Serialize)]
struct TemplateContractCaptureReceipt<'a> {
    schema: &'static str,
    output_path: &'a str,
    content_sha256: &'a str,
    template_title: &'a str,
    status: &'a str,
    compatibility: &'a str,
    parameter_count: usize,
    finding_count: usize,
    parameter_change_count: usize,
    render_fixture_count: usize,
    authority: &'static str,
}

#[derive(Debug, Serialize)]
struct TemplateContractRenderReceipt {
    schema: &'static str,
    status: &'static str,
    project_root: String,
    target_api_url: String,
    render_authority: &'static str,
    contract_path: String,
    contract_sha256: String,
    template_title: String,
    assessment_status: String,
    compatibility: String,
    fixture_count: usize,
    passed_count: usize,
    failed_count: usize,
    request_count: usize,
    fixtures: Vec<TemplateContractFixtureRenderReceipt>,
}

#[derive(Debug, Serialize)]
struct TemplateContractFixtureRenderReceipt {
    fixture_id: String,
    invocation_sha256: String,
    #[serde(flatten)]
    report: RenderCheckReport,
}

#[derive(Debug, Serialize)]
struct TemplateScaffoldPlan<'a> {
    schema: &'static str,
    mode: &'a str,
    contract_path: &'a str,
    contract_sha256: &'a str,
    output_path: &'a str,
    template_title: &'a str,
    proposed_sha256: &'a str,
    observed_sha256: Option<&'a str>,
    output_exists: bool,
    output_unchanged: bool,
    overwrite_required: bool,
    assessment_status: &'a str,
    compatibility: &'a str,
    render_fixture_count: usize,
    plan_id: &'a str,
}

pub(super) fn run_templates_contract(
    runtime: &RuntimeOptions,
    args: TemplatesContractArgs,
) -> Result<()> {
    match args.command {
        TemplatesContractSubcommand::Capture(args) => run_contract_capture(runtime, args),
        TemplatesContractSubcommand::Check(args) => run_contract_check(runtime, args),
        TemplatesContractSubcommand::RenderCheck(args) => run_contract_render_check(runtime, args),
    }
}

fn run_contract_render_check(
    runtime: &RuntimeOptions,
    args: TemplatesContractRenderCheckArgs,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let (contract_path, contract_content) = read_project_text(&paths, &args.contract, "contract")?;
    let contract = parse_template_engineering_contract(&contract_content)?;
    let catalog = load_or_sync_catalog(&paths)?;
    let assessment = assess_template_engineering_contract(&contract, &catalog, None)?;
    if assessment.status == "blocked" {
        bail!(
            "template contract is blocked; run templates contract check before executing fixtures"
        );
    }
    if contract.render_fixtures.is_empty() {
        bail!("template contract has no render fixtures");
    }

    let mut client = wikitool_core::mw::client_from_wikitool_config(&config)?;
    let target_api_url = client.api_url().to_string();
    let mut fixtures = Vec::with_capacity(contract.render_fixtures.len());
    for fixture in &contract.render_fixtures {
        let report = render_check_wikitext(
            &mut client,
            &fixture.invocation,
            &RenderCheckOptions {
                title: "Wikitool template fixture".to_string(),
                scope_class: fixture.scope_class.clone(),
                expected_scope_count: fixture.expected_scope_count,
                require_interactive_link: fixture.require_interactive_link,
                required_href_substrings: fixture.required_href_substrings.clone(),
                required_link_classes: fixture.required_link_classes.clone(),
                required_page_image: None,
                forbid_literal_wikilinks: fixture.forbid_literal_wikilinks,
            },
        )?;
        fixtures.push(TemplateContractFixtureRenderReceipt {
            fixture_id: fixture.id.clone(),
            invocation_sha256: wikitool_core::support::compute_sha256(&fixture.invocation),
            report,
        });
    }

    let failed_count = fixtures
        .iter()
        .filter(|fixture| fixture.report.status != "clean")
        .count();
    let receipt = TemplateContractRenderReceipt {
        schema: "template_contract_render_report_v1",
        status: if failed_count == 0 { "clean" } else { "failed" },
        project_root: normalize_path(&paths.project_root),
        target_api_url,
        render_authority: "mediawiki_action_parse_text_read_only",
        contract_path: normalize_path(&contract_path),
        contract_sha256: wikitool_core::support::compute_sha256(&contract_content),
        template_title: contract.template_title,
        assessment_status: assessment.status,
        compatibility: assessment.compatibility,
        fixture_count: fixtures.len(),
        passed_count: fixtures.len() - failed_count,
        failed_count,
        request_count: fixtures
            .iter()
            .map(|fixture| fixture.report.request_count)
            .sum(),
        fixtures,
    };
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&receipt)?);
    } else {
        println!("templates contract render-check");
        println!("project_root: {}", receipt.project_root);
        println!("target_api_url: {}", receipt.target_api_url);
        println!("render_authority: {}", receipt.render_authority);
        println!("contract_path: {}", receipt.contract_path);
        println!("contract_sha256: {}", receipt.contract_sha256);
        println!("template_title: {}", receipt.template_title);
        println!("status: {}", receipt.status);
        println!("assessment_status: {}", receipt.assessment_status);
        println!("compatibility: {}", receipt.compatibility);
        println!("fixtures.count: {}", receipt.fixture_count);
        println!("fixtures.passed: {}", receipt.passed_count);
        println!("fixtures.failed: {}", receipt.failed_count);
        println!("requests.count: {}", receipt.request_count);
        for fixture in &receipt.fixtures {
            println!(
                "fixture: id={} status={} scopes={} issues={}",
                fixture.fixture_id,
                fixture.report.status,
                fixture.report.scope_count,
                fixture.report.issue_count
            );
        }
    }
    if failed_count > 0 {
        bail!("template contract render-check failed for {failed_count} fixture(s)");
    }
    Ok(())
}

pub(super) fn run_templates_scaffold(
    runtime: &RuntimeOptions,
    args: TemplatesScaffoldArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let (contract_path, contract_content) = read_project_text(&paths, &args.contract, "contract")?;
    let contract = parse_template_engineering_contract(&contract_content)?;
    let catalog = load_or_sync_catalog(&paths)?;
    let assessment = assess_template_engineering_contract(&contract, &catalog, None)?;
    if assessment.status == "blocked" {
        bail!(
            "template contract is blocked; run templates contract check for {} finding(s) and {} parameter change(s)",
            assessment.findings.len(),
            assessment.parameter_changes.len()
        );
    }
    let scaffold = render_template_scaffold(&contract)?;
    let output_path = project_path(&paths, &args.output)?;
    wikitool_core::filesystem::validate_scoped_path(&paths, &output_path)?;
    let observed_content = if output_path.exists() {
        Some(fs::read_to_string(&output_path).with_context(|| {
            format!(
                "failed to read existing scaffold target {} as UTF-8",
                normalize_path(&output_path)
            )
        })?)
    } else {
        None
    };
    let contract_sha256 = wikitool_core::support::compute_sha256(&contract_content);
    let proposed_sha256 = wikitool_core::support::compute_sha256(&scaffold);
    let observed_sha256 = observed_content
        .as_deref()
        .map(wikitool_core::support::compute_sha256);
    let output_unchanged = observed_sha256.as_deref() == Some(proposed_sha256.as_str());
    let overwrite_required = observed_content.is_some() && !output_unchanged;
    let contract_display = normalize_path(&contract_path);
    let output_display = normalize_path(&output_path);
    let plan_id = scaffold_plan_id(
        &contract_sha256,
        &output_display,
        observed_sha256.as_deref(),
        &proposed_sha256,
    );

    let mode = if args.apply.is_some() {
        if args.apply.as_deref() != Some(plan_id.as_str()) {
            bail!(
                "scaffold plan_id mismatch; preview again against current contract and output state"
            );
        }
        if overwrite_required && !args.overwrite {
            bail!("scaffold target differs; repeat the exact apply with --overwrite after review");
        }
        if !output_unchanged {
            wikitool_core::support::atomic_write(&output_path, scaffold)?;
        }
        if output_unchanged {
            "unchanged"
        } else {
            "applied"
        }
    } else {
        "plan"
    };

    let plan = TemplateScaffoldPlan {
        schema: "template_scaffold_plan_v1",
        mode,
        contract_path: &contract_display,
        contract_sha256: &contract_sha256,
        output_path: &output_display,
        template_title: &assessment.template_title,
        proposed_sha256: &proposed_sha256,
        observed_sha256: observed_sha256.as_deref(),
        output_exists: observed_content.is_some(),
        output_unchanged,
        overwrite_required,
        assessment_status: &assessment.status,
        compatibility: &assessment.compatibility,
        render_fixture_count: assessment.render_fixture_bundle.fixtures.len(),
        plan_id: &plan_id,
    };
    print_scaffold_plan(&paths, &plan, args.format, runtime.diagnostics)?;
    Ok(())
}

fn run_contract_check(runtime: &RuntimeOptions, args: TemplatesContractCheckArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let (_, contract_content) = read_project_text(&paths, &args.contract, "contract")?;
    let contract = parse_template_engineering_contract(&contract_content)?;
    let catalog = load_or_sync_catalog(&paths)?;
    let assessment =
        assess_template_engineering_contract(&contract, &catalog, args.against.as_deref())?;
    let blocked = assessment.status == "blocked";

    if let Some(output) = args.output {
        let output = project_path(&paths, &output)?;
        wikitool_core::filesystem::validate_scoped_path(&paths, &output)?;
        let mut encoded = serde_json::to_string_pretty(&assessment)?;
        encoded.push('\n');
        let content_sha256 = wikitool_core::support::compute_sha256(&encoded);
        wikitool_core::support::atomic_write(&output, encoded)?;
        let output_display = normalize_path(output.canonicalize().unwrap_or(output));
        let receipt = TemplateContractWriteReceipt {
            schema: "template_contract_assessment_write_v1",
            output_path: &output_display,
            content_sha256: &content_sha256,
            status: &assessment.status,
            compatibility: &assessment.compatibility,
            finding_count: assessment.findings.len(),
            parameter_change_count: assessment.parameter_changes.len(),
            render_fixture_count: assessment.render_fixture_bundle.fixtures.len(),
        };
        if args.format.is_json() {
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        } else {
            println!("templates contract check");
            println!("output_path: {}", receipt.output_path);
            println!("content_sha256: {}", receipt.content_sha256);
            print_assessment_summary(&assessment);
        }
    } else if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&assessment)?);
    } else {
        println!("templates contract check");
        println!("project_root: {}", normalize_path(&paths.project_root));
        print_assessment_summary(&assessment);
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }

    if blocked {
        bail!(
            "template contract check blocked with {} finding(s) and {} compatibility change(s)",
            assessment.findings.len(),
            assessment.parameter_changes.len()
        );
    }
    Ok(())
}

fn run_contract_capture(
    runtime: &RuntimeOptions,
    args: TemplatesContractCaptureArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let catalog = load_or_sync_catalog(&paths)?;
    let contract = capture_template_engineering_contract(&paths, &catalog, &args.template)?;
    let assessment = assess_template_engineering_contract(&contract, &catalog, None)?;
    let output = project_path(&paths, &args.output)?;
    wikitool_core::filesystem::validate_project_path(&paths, &output)?;
    if output.exists() {
        bail!(
            "contract capture refuses existing output {}; choose a new path and merge deliberately",
            normalize_path(&output)
        );
    }
    let mut encoded = serde_json::to_string_pretty(&contract)?;
    encoded.push('\n');
    let content_sha256 = wikitool_core::support::compute_sha256(&encoded);
    wikitool_core::support::atomic_write(&output, encoded)?;
    let output_display = normalize_path(output.canonicalize().unwrap_or(output));
    let receipt = TemplateContractCaptureReceipt {
        schema: "template_contract_capture_v1",
        output_path: &output_display,
        content_sha256: &content_sha256,
        template_title: &assessment.template_title,
        status: &assessment.status,
        compatibility: &assessment.compatibility,
        parameter_count: contract.parameters.len(),
        finding_count: assessment.findings.len(),
        parameter_change_count: assessment.parameter_changes.len(),
        render_fixture_count: assessment.render_fixture_bundle.fixtures.len(),
        authority: "observed_starter_not_target_design",
    };
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&receipt)?);
    } else {
        println!("templates contract capture");
        println!("output_path: {}", receipt.output_path);
        println!("content_sha256: {}", receipt.content_sha256);
        println!("template_title: {}", receipt.template_title);
        println!("status: {}", receipt.status);
        println!("compatibility: {}", receipt.compatibility);
        println!("parameters.count: {}", receipt.parameter_count);
        println!("findings.count: {}", receipt.finding_count);
        println!("render_fixtures.count: {}", receipt.render_fixture_count);
        println!("authority: {}", receipt.authority);
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }
    Ok(())
}

fn print_assessment_summary(assessment: &TemplateContractAssessment) {
    println!("template_title: {}", assessment.template_title);
    println!("status: {}", assessment.status);
    println!("compatibility: {}", assessment.compatibility);
    println!("scaffold_sha256: {}", assessment.scaffold_sha256);
    println!("scaffold_bytes: {}", assessment.scaffold_bytes);
    println!(
        "parameter_changes.count: {}",
        assessment.parameter_changes.len()
    );
    println!("findings.count: {}", assessment.findings.len());
    println!(
        "render_fixtures.count: {}",
        assessment.render_fixture_bundle.fixtures.len()
    );
    for finding in &assessment.findings {
        println!(
            "finding: severity={} code={} message={}",
            finding.severity, finding.code, finding.message
        );
    }
    for change in &assessment.parameter_changes {
        println!(
            "parameter_change: parameter={} change={} compatibility={} detail={}",
            change.parameter, change.change, change.compatibility, change.detail
        );
    }
}

fn print_scaffold_plan(
    paths: &ResolvedPaths,
    plan: &TemplateScaffoldPlan<'_>,
    format: OutputFormat,
    diagnostics: bool,
) -> Result<()> {
    if format.is_json() {
        println!("{}", serde_json::to_string_pretty(plan)?);
        return Ok(());
    }
    println!("templates scaffold");
    println!("mode: {}", plan.mode);
    println!("contract_path: {}", plan.contract_path);
    println!("output_path: {}", plan.output_path);
    println!("template_title: {}", plan.template_title);
    println!("proposed_sha256: {}", plan.proposed_sha256);
    println!(
        "observed_sha256: {}",
        plan.observed_sha256.unwrap_or("<missing>")
    );
    println!("overwrite_required: {}", plan.overwrite_required);
    println!("assessment_status: {}", plan.assessment_status);
    println!("compatibility: {}", plan.compatibility);
    println!("render_fixtures.count: {}", plan.render_fixture_count);
    println!("plan_id: {}", plan.plan_id);
    if diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn read_project_text(paths: &ResolvedPaths, input: &Path, kind: &str) -> Result<(PathBuf, String)> {
    let path = project_path(paths, input)?;
    wikitool_core::filesystem::validate_project_path(paths, &path)?;
    let size = fs::metadata(&path)
        .with_context(|| format!("failed to inspect {kind} {}", normalize_path(&path)))?
        .len();
    if size > MAX_TEMPLATE_CONTRACT_BYTES {
        bail!(
            "{kind} exceeds the {MAX_TEMPLATE_CONTRACT_BYTES}-byte input limit: {}",
            normalize_path(&path)
        );
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {kind} {} as UTF-8", normalize_path(&path)))?;
    Ok((path, content))
}

fn project_path(paths: &ResolvedPaths, path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    if path.as_os_str().is_empty() {
        bail!("path must be non-empty");
    }
    Ok(path)
}

fn scaffold_plan_id(
    contract_sha256: &str,
    output_path: &str,
    observed_sha256: Option<&str>,
    proposed_sha256: &str,
) -> String {
    wikitool_core::support::compute_sha256(&format!(
        "template_scaffold_plan_v1\ncontract={contract_sha256}\noutput={output_path}\nobserved={}\nproposed={proposed_sha256}\n",
        observed_sha256.unwrap_or("<missing>")
    ))
}
