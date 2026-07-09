use anyhow::{Result, bail};
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use wikitool_core::mw::{
    MediaWikiClient, RenderCheckOptions, RenderCheckReport, render_check_page,
};

use crate::RuntimeOptions;
use crate::briefs::{BriefCommand, brief_command_owned};
use crate::cli_support::{normalize_path, resolve_runtime_with_config};

use super::WikiRenderCheckArgs;

#[derive(Debug, Serialize)]
struct WikiRenderCheckOutput {
    project_root: String,
    #[serde(flatten)]
    report: RenderCheckReport,
}

#[derive(Debug, Serialize)]
struct WikiRenderCheckBrief<'a> {
    schema_version: &'static str,
    command: &'static str,
    view: &'static str,
    project_root: String,
    status: &'static str,
    title: &'a str,
    revision_id: Option<i64>,
    scope_class: Option<&'a str>,
    expected_scope_count: Option<usize>,
    scope_count: usize,
    literal_wikilink_count: usize,
    parser_error_count: usize,
    issue_count: usize,
    issue_codes: BTreeMap<&'a str, usize>,
    failed_scope_indices: Vec<usize>,
    sample_issues: Vec<&'a wikitool_core::mw::RenderCheckIssue>,
    request_count: usize,
    full_view_command: BriefCommand,
}

pub(super) fn run_wiki_render_check(
    runtime: &RuntimeOptions,
    args: WikiRenderCheckArgs,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let mut client = MediaWikiClient::from_config(&config)?;
    let report = render_check_page(
        &mut client,
        &RenderCheckOptions {
            title: args.title,
            scope_class: args.scope_class,
            expected_scope_count: args.expected_scope_count,
            require_interactive_link: args.require_interactive_link,
            required_href_substrings: args.required_href_substrings,
            required_link_classes: args.required_link_classes,
            forbid_literal_wikilinks: !args.allow_literal_wikilinks,
        },
    )?;
    let clean = report.status == "clean";
    let issue_count = report.issue_count;

    if args.format.is_json() {
        if args.view.is_full() {
            println!(
                "{}",
                serde_json::to_string_pretty(&WikiRenderCheckOutput {
                    project_root: normalize_path(&paths.project_root),
                    report,
                })?
            );
        } else {
            let mut issue_codes = BTreeMap::new();
            let mut failed_scope_indices = BTreeSet::new();
            for issue in &report.issues {
                *issue_codes.entry(issue.code.as_str()).or_insert(0) += 1;
                if let Some(index) = issue.scope_index {
                    failed_scope_indices.insert(index);
                }
            }
            let mut full_args = vec![
                "wikitool".to_string(),
                "wiki".to_string(),
                "render-check".to_string(),
                report.title.clone(),
            ];
            if let Some(scope_class) = &report.scope_class {
                full_args.extend(["--scope-class".to_string(), scope_class.clone()]);
            }
            if let Some(expected_scope_count) = report.expected_scope_count {
                full_args.extend([
                    "--expect-scopes".to_string(),
                    expected_scope_count.to_string(),
                ]);
            }
            if report.require_interactive_link {
                full_args.push("--require-interactive-link".to_string());
            }
            for required_href in &report.required_href_substrings {
                full_args.extend(["--require-href-contains".to_string(), required_href.clone()]);
            }
            for required_class in &report.required_link_classes {
                full_args.extend(["--require-link-class".to_string(), required_class.clone()]);
            }
            if !report.forbid_literal_wikilinks {
                full_args.push("--allow-literal-wikilinks".to_string());
            }
            full_args.extend([
                "--format".to_string(),
                "json".to_string(),
                "--view".to_string(),
                "full".to_string(),
            ]);
            println!(
                "{}",
                serde_json::to_string_pretty(&WikiRenderCheckBrief {
                    schema_version: "wikitool_brief_v1",
                    command: "wiki render-check",
                    view: args.view.as_str(),
                    project_root: normalize_path(&paths.project_root),
                    status: report.status,
                    title: &report.title,
                    revision_id: report.revision_id,
                    scope_class: report.scope_class.as_deref(),
                    expected_scope_count: report.expected_scope_count,
                    scope_count: report.scope_count,
                    literal_wikilink_count: report.literal_wikilink_count,
                    parser_error_count: report.parser_error_count,
                    issue_count: report.issue_count,
                    issue_codes,
                    failed_scope_indices: failed_scope_indices.into_iter().collect(),
                    sample_issues: report.issues.iter().take(8).collect(),
                    request_count: report.request_count,
                    full_view_command: brief_command_owned(full_args),
                })?
            );
        }
    } else {
        println!("wiki render-check");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("title: {}", report.title);
        println!("status: {}", report.status);
        println!(
            "scope_class: {}",
            report.scope_class.as_deref().unwrap_or("<none>")
        );
        println!("scope_count: {}", report.scope_count);
        println!("literal_wikilink_count: {}", report.literal_wikilink_count);
        println!("parser_error_count: {}", report.parser_error_count);
        println!("issue_count: {}", report.issue_count);
        for issue in &report.issues {
            println!(
                "issue: code={} scope={} message={}",
                issue.code,
                issue
                    .scope_index
                    .map(|index| index.to_string())
                    .unwrap_or_else(|| "<page>".to_string()),
                issue.message
            );
        }
        for scope in &report.scopes {
            println!(
                "scope: index={} tag={} interactive_links={} hrefs={} link_classes={}",
                scope.index,
                scope.tag,
                scope.interactive_link_count,
                if scope.interactive_hrefs.is_empty() {
                    "<none>".to_string()
                } else {
                    scope.interactive_hrefs.join(", ")
                },
                if scope.interactive_link_classes.is_empty() {
                    "<none>".to_string()
                } else {
                    scope.interactive_link_classes.join(", ")
                }
            );
        }
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }

    if clean {
        Ok(())
    } else {
        bail!("render-check detected {issue_count} issue(s)")
    }
}
