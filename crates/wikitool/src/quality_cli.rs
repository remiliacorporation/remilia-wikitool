use anyhow::{Result, bail};
use clap::Args;
use wikitool_core::index::{ValidationReport, run_validation_checks};
use wikitool_core::lint::lint_modules;

use crate::cli_support::{normalize_path, print_string_list, resolve_runtime_paths};
use crate::{MIGRATIONS_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct LintArgs {
    title: Option<String>,
    #[arg(
        long,
        default_value = "text",
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: String,
    #[arg(long, help = "Treat warnings as errors")]
    strict: bool,
    #[arg(long, help = "Omit metadata from JSON output")]
    no_meta: bool,
}

pub(crate) fn run_validate(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("validate");
    println!("project_root: {}", normalize_path(&paths.project_root));
    let report = match run_validation_checks(&paths)? {
        Some(report) => report,
        None => {
            println!("index.storage: <not built> (run `wikitool index rebuild`)");
            println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
            if runtime.diagnostics {
                println!("\n[diagnostics]\n{}", paths.diagnostics());
            }
            bail!("validate requires a built local index");
        }
    };

    print_validation_issues(&report);
    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    let issue_count = report.broken_links.len()
        + report.double_redirects.len()
        + report.uncategorized_pages.len()
        + report.orphan_pages.len();
    if issue_count == 0 {
        println!("validate.status: clean");
        Ok(())
    } else {
        println!("validate.status: failed");
        bail!("validation detected {issue_count} issue(s)")
    }
}

pub(crate) fn run_lint(runtime: &RuntimeOptions, args: LintArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let report = lint_modules(&paths, args.title.as_deref())?;
    let format = args.format.to_ascii_lowercase();
    if format != "text" && format != "json" {
        bail!(
            "unsupported lint format: {} (expected text|json)",
            args.format
        );
    }

    let _ = args.no_meta;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if !report.selene_available {
        println!("lint");
        println!("selene: missing");
        println!("warning: Selene not found");
        println!("hint: install Selene manually or set SELENE_PATH");
    } else {
        println!("lint");
        println!(
            "selene_path: {}",
            report.selene_path.as_deref().unwrap_or("<none>")
        );
        println!(
            "selene_config: {}",
            report.config_path.as_deref().unwrap_or("<none>")
        );
        println!("inspected_modules: {}", report.inspected_modules);
        println!("errors: {}", report.total_errors);
        println!("warnings: {}", report.total_warnings);
        if report.results.is_empty() {
            println!("issues: <none>");
        } else {
            for result in &report.results {
                println!("module: {}", result.title);
                for issue in &result.errors {
                    println!(
                        "  error: {}:{} {} {}",
                        issue.line, issue.column, issue.code, issue.message
                    );
                }
                for issue in &result.warnings {
                    println!(
                        "  warning: {}:{} {} {}",
                        issue.line, issue.column, issue.code, issue.message
                    );
                }
            }
        }
    }

    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.selene_available
        && (report.total_errors > 0 || (args.strict && report.total_warnings > 0))
    {
        bail!(
            "lint found {} error(s) and {} warning(s)",
            report.total_errors,
            report.total_warnings
        );
    }
    Ok(())
}

fn print_validation_issues(report: &ValidationReport) {
    println!("validate.broken_links.count: {}", report.broken_links.len());
    if report.broken_links.is_empty() {
        println!("validate.broken_links: <none>");
    } else {
        for issue in &report.broken_links {
            println!(
                "validate.broken_links.issue: source={} target={}",
                issue.source_title, issue.target_title
            );
        }
    }

    println!(
        "validate.double_redirects.count: {}",
        report.double_redirects.len()
    );
    if report.double_redirects.is_empty() {
        println!("validate.double_redirects: <none>");
    } else {
        for issue in &report.double_redirects {
            println!(
                "validate.double_redirects.issue: title={} first_target={} final_target={}",
                issue.title, issue.first_target, issue.final_target
            );
        }
    }

    print_string_list("validate.uncategorized_pages", &report.uncategorized_pages);
    print_string_list("validate.orphan_pages", &report.orphan_pages);
}
