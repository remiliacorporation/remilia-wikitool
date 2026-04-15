use anyhow::{Result, bail};
use clap::Args;
use serde::Serialize;
use wikitool_core::knowledge::inspect::{ValidationReport, run_validation_checks};
use wikitool_core::lint::{LuaLintReport, LuaLintResult, lint_modules};

use crate::cli_support::{OutputFormat, normalize_path, print_string_list, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct ValidateArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
    #[arg(long, help = "Omit detailed issue lists and print category counts")]
    summary: bool,
}

impl Default for ValidateArgs {
    fn default() -> Self {
        Self {
            format: OutputFormat::Text,
            summary: false,
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct LintArgs {
    title: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
    #[arg(long, help = "Treat warnings as errors")]
    strict: bool,
    #[arg(long, help = "Omit metadata from JSON output")]
    no_meta: bool,
}

#[derive(Debug, Serialize)]
struct ValidateJson<'a> {
    project_root: String,
    index_ready: bool,
    status: &'static str,
    issue_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<ValidateSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<&'a ValidationReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ValidateSummary {
    broken_links: usize,
    double_redirects: usize,
    uncategorized_pages: usize,
    orphan_pages: usize,
}

#[derive(Debug, Serialize)]
struct LintJson<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    selene_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selene_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_path: Option<&'a str>,
    inspected_modules: usize,
    total_errors: usize,
    total_warnings: usize,
    results: &'a [LuaLintResult],
}

pub(crate) fn run_validate(runtime: &RuntimeOptions, args: ValidateArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    let report = match run_validation_checks(&paths)? {
        Some(report) => report,
        None => {
            let message = "content_index.storage: <not built> (run `wikitool knowledge build`)";
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ValidateJson {
                        project_root: normalize_path(&paths.project_root),
                        index_ready: false,
                        status: "not_ready",
                        issue_count: 0,
                        summary: None,
                        report: None,
                        message: Some(message),
                    })?
                );
                bail!("validate requires a built local index");
            }
            println!("validate");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("content_index.storage: <not built> (run `wikitool knowledge build`)");
            println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
            if runtime.diagnostics {
                println!("\n[diagnostics]\n{}", paths.diagnostics());
            }
            bail!("validate requires a built local index");
        }
    };

    let issue_count = validation_issue_count(&report);
    let status = if issue_count == 0 { "clean" } else { "failed" };
    if args.format.is_json() {
        let summary = args.summary.then(|| validation_summary(&report));
        println!(
            "{}",
            serde_json::to_string_pretty(&ValidateJson {
                project_root: normalize_path(&paths.project_root),
                index_ready: true,
                status,
                issue_count,
                summary,
                report: if args.summary { None } else { Some(&report) },
                message: None,
            })?
        );
        if issue_count == 0 {
            return Ok(());
        }
        bail!("validation detected {issue_count} issue(s)");
    }

    println!("validate");
    println!("project_root: {}", normalize_path(&paths.project_root));
    if args.summary {
        print_validation_summary(&report);
    } else {
        print_validation_issues(&report);
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if issue_count == 0 {
        println!("validate.status: clean");
        Ok(())
    } else {
        println!("validate.status: failed");
        bail!("validation detected {issue_count} issue(s)")
    }
}

fn validation_issue_count(report: &ValidationReport) -> usize {
    report.broken_links.len()
        + report.double_redirects.len()
        + report.uncategorized_pages.len()
        + report.orphan_pages.len()
}

fn validation_summary(report: &ValidationReport) -> ValidateSummary {
    ValidateSummary {
        broken_links: report.broken_links.len(),
        double_redirects: report.double_redirects.len(),
        uncategorized_pages: report.uncategorized_pages.len(),
        orphan_pages: report.orphan_pages.len(),
    }
}

pub(crate) fn run_lint(runtime: &RuntimeOptions, args: LintArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let report = lint_modules(&paths, args.title.as_deref())?;

    if args.format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&lint_json_output(&report, args.no_meta))?
        );
    } else {
        println!("module lint");
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

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.total_errors > 0 || (args.strict && report.total_warnings > 0) {
        bail!(
            "lint found {} error(s) and {} warning(s)",
            report.total_errors,
            report.total_warnings
        );
    }
    Ok(())
}

fn lint_json_output<'a>(report: &'a LuaLintReport, no_meta: bool) -> LintJson<'a> {
    LintJson {
        selene_available: if no_meta {
            None
        } else {
            Some(report.selene_available)
        },
        selene_path: if no_meta {
            None
        } else {
            report.selene_path.as_deref()
        },
        config_path: if no_meta {
            None
        } else {
            report.config_path.as_deref()
        },
        inspected_modules: report.inspected_modules,
        total_errors: report.total_errors,
        total_warnings: report.total_warnings,
        results: &report.results,
    }
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

fn print_validation_summary(report: &ValidationReport) {
    let summary = validation_summary(report);
    println!("validate.issue_count: {}", validation_issue_count(report));
    println!("validate.broken_links.count: {}", summary.broken_links);
    println!(
        "validate.double_redirects.count: {}",
        summary.double_redirects
    );
    println!(
        "validate.uncategorized_pages.count: {}",
        summary.uncategorized_pages
    );
    println!("validate.orphan_pages.count: {}", summary.orphan_pages);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use wikitool_core::knowledge::inspect::{BrokenLinkIssue, DoubleRedirectIssue};
    use wikitool_core::lint::{LuaLintIssue, LuaLintSeverity};

    #[test]
    fn validate_json_reports_status_and_issue_count() {
        let report = ValidationReport {
            broken_links: vec![BrokenLinkIssue {
                source_title: "Alpha".to_string(),
                target_title: "Missing".to_string(),
            }],
            double_redirects: vec![DoubleRedirectIssue {
                title: "Redirect A".to_string(),
                first_target: "Redirect B".to_string(),
                final_target: "Target".to_string(),
            }],
            uncategorized_pages: vec!["Uncategorized".to_string()],
            orphan_pages: Vec::new(),
        };

        let value = serde_json::to_value(ValidateJson {
            project_root: "/repo".to_string(),
            index_ready: true,
            status: "failed",
            issue_count: validation_issue_count(&report),
            summary: None,
            report: Some(&report),
            message: None,
        })
        .expect("serialize validate json");

        assert_eq!(value["status"], json!("failed"));
        assert_eq!(value["issue_count"], json!(3));
        assert_eq!(
            value["report"]["broken_links"][0]["target_title"],
            json!("Missing")
        );
        assert!(value.get("message").is_none());
    }

    #[test]
    fn validate_json_not_ready_omits_report() {
        let value = serde_json::to_value(ValidateJson {
            project_root: "/repo".to_string(),
            index_ready: false,
            status: "not_ready",
            issue_count: 0,
            summary: None,
            report: None,
            message: Some("build the index"),
        })
        .expect("serialize validate not-ready json");

        assert_eq!(value["index_ready"], json!(false));
        assert_eq!(value["status"], json!("not_ready"));
        assert_eq!(value["message"], json!("build the index"));
        assert!(value.get("report").is_none());
    }

    #[test]
    fn validate_summary_json_omits_detailed_report() {
        let report = ValidationReport {
            broken_links: vec![BrokenLinkIssue {
                source_title: "Alpha".to_string(),
                target_title: "Missing".to_string(),
            }],
            double_redirects: Vec::new(),
            uncategorized_pages: vec!["Uncategorized".to_string()],
            orphan_pages: vec!["Orphan".to_string()],
        };

        let value = serde_json::to_value(ValidateJson {
            project_root: "/repo".to_string(),
            index_ready: true,
            status: "failed",
            issue_count: validation_issue_count(&report),
            summary: Some(validation_summary(&report)),
            report: None,
            message: None,
        })
        .expect("serialize validate summary json");

        assert_eq!(value["issue_count"], json!(3));
        assert_eq!(value["summary"]["broken_links"], json!(1));
        assert_eq!(value["summary"]["uncategorized_pages"], json!(1));
        assert_eq!(value["summary"]["orphan_pages"], json!(1));
        assert!(value.get("report").is_none());
    }

    #[test]
    fn lint_no_meta_json_omits_runtime_fields() {
        let report = LuaLintReport {
            selene_available: true,
            selene_path: Some("embedded:selene-lib".to_string()),
            config_path: Some("config/selene.toml".to_string()),
            inspected_modules: 1,
            total_errors: 1,
            total_warnings: 0,
            results: vec![LuaLintResult {
                title: "Module:Alpha".to_string(),
                errors: vec![LuaLintIssue {
                    line: 1,
                    column: 2,
                    end_line: None,
                    end_column: None,
                    code: "parse_error".to_string(),
                    message: "bad".to_string(),
                    severity: LuaLintSeverity::Error,
                }],
                warnings: vec![],
            }],
        };

        let value = serde_json::to_value(lint_json_output(&report, true)).expect("serialize lint");

        assert_eq!(value["inspected_modules"], json!(1));
        assert_eq!(value["total_errors"], json!(1));
        assert!(value.get("selene_available").is_none());
        assert!(value.get("selene_path").is_none());
        assert!(value.get("config_path").is_none());
    }
}
