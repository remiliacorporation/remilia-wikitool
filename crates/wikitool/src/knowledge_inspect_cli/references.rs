use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::knowledge::references::{
    ReferenceAuditFilters, ReferenceAuditSummaryReport, ReferenceDuplicateGroup,
    ReferenceDuplicatesReport, ReferenceListItem, ReferenceListReport,
    inspect_reference_duplicates, inspect_reference_list, inspect_reference_summary,
};

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::*;
#[derive(Debug, Args)]
pub(crate) struct ReferenceInspectArgs {
    #[command(subcommand)]
    command: ReferenceInspectSubcommand,
}

#[derive(Debug, Subcommand)]
enum ReferenceInspectSubcommand {
    /// Show aggregate reference audit counts
    Summary(ReferenceInspectSummaryArgs),
    /// List individual indexed references
    List(ReferenceInspectListArgs),
    /// Show strong duplicate reference groups
    Duplicates(ReferenceInspectListArgs),
}

#[derive(Debug, Args)]
struct ReferenceInspectSummaryArgs {
    #[arg(long = "title", value_name = "TITLE")]
    titles: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    titles_file: Option<PathBuf>,
    #[arg(long, help = "Inspect all indexed pages")]
    all: bool,
    #[arg(long)]
    domain: Option<String>,
    #[arg(long)]
    template: Option<String>,
    #[arg(long)]
    authority: Option<String>,
    #[arg(long = "identifier-key")]
    identifier_key: Option<String>,
    #[arg(long = "identifier")]
    identifier: Option<String>,
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
struct ReferenceInspectListArgs {
    #[arg(long = "title", value_name = "TITLE")]
    titles: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    titles_file: Option<PathBuf>,
    #[arg(long, help = "Inspect all indexed pages")]
    all: bool,
    #[arg(long)]
    domain: Option<String>,
    #[arg(long)]
    template: Option<String>,
    #[arg(long)]
    authority: Option<String>,
    #[arg(long = "identifier-key")]
    identifier_key: Option<String>,
    #[arg(long = "identifier")]
    identifier: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Clone, Serialize)]
struct ReferenceInspectSelection {
    all: bool,
    titles: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ReferenceInspectEnvelope<T> {
    project_root: String,
    storage_ready: bool,
    selection: ReferenceInspectSelection,
    filters: ReferenceAuditFilters,
    report: Option<T>,
}

pub(super) fn run_inspect_references(
    runtime: &RuntimeOptions,
    args: ReferenceInspectArgs,
) -> Result<()> {
    match args.command {
        ReferenceInspectSubcommand::Summary(args) => run_inspect_reference_summary(runtime, args),
        ReferenceInspectSubcommand::List(args) => run_inspect_reference_list(runtime, args),
        ReferenceInspectSubcommand::Duplicates(args) => {
            run_inspect_reference_duplicates(runtime, args)
        }
    }
}

fn run_inspect_reference_summary(
    runtime: &RuntimeOptions,
    args: ReferenceInspectSummaryArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let selection = load_reference_selection(&args.titles, args.titles_file.as_ref(), args.all)?;
    let filters = reference_audit_filters_from_args(
        args.domain,
        args.template,
        args.authority,
        args.identifier_key,
        args.identifier,
    );
    let selected_titles = if selection.all {
        Vec::new()
    } else {
        selection.titles.clone()
    };
    let report = inspect_reference_summary(&paths, &selected_titles, &filters)?;

    if args.format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&ReferenceInspectEnvelope {
                project_root: normalize_path(&paths.project_root),
                storage_ready: report.is_some(),
                selection,
                filters,
                report,
            })?
        );
        return Ok(());
    }

    println!("knowledge inspect references summary");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_reference_selection(&selection);
    print_reference_filters(&filters);
    match report {
        Some(report) => print_reference_summary_report(&report),
        None => println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)"),
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_inspect_reference_list(
    runtime: &RuntimeOptions,
    args: ReferenceInspectListArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let selection = load_reference_selection(&args.titles, args.titles_file.as_ref(), args.all)?;
    if !selection.all && selection.titles.is_empty() {
        bail!("knowledge inspect references list requires --all or at least one --title");
    }
    let filters = reference_audit_filters_from_args(
        args.domain,
        args.template,
        args.authority,
        args.identifier_key,
        args.identifier,
    );
    let selected_titles = if selection.all {
        Vec::new()
    } else {
        selection.titles.clone()
    };
    let report = inspect_reference_list(&paths, &selected_titles, &filters)?;

    if args.format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&ReferenceInspectEnvelope {
                project_root: normalize_path(&paths.project_root),
                storage_ready: report.is_some(),
                selection,
                filters,
                report,
            })?
        );
        return Ok(());
    }

    println!("knowledge inspect references list");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_reference_selection(&selection);
    print_reference_filters(&filters);
    match report {
        Some(report) => print_reference_list_report(&report),
        None => println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)"),
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_inspect_reference_duplicates(
    runtime: &RuntimeOptions,
    args: ReferenceInspectListArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let selection = load_reference_selection(&args.titles, args.titles_file.as_ref(), args.all)?;
    if !selection.all && selection.titles.is_empty() {
        bail!("knowledge inspect references duplicates requires --all or at least one --title");
    }
    let filters = reference_audit_filters_from_args(
        args.domain,
        args.template,
        args.authority,
        args.identifier_key,
        args.identifier,
    );
    let selected_titles = if selection.all {
        Vec::new()
    } else {
        selection.titles.clone()
    };
    let report = inspect_reference_duplicates(&paths, &selected_titles, &filters)?;

    if args.format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&ReferenceInspectEnvelope {
                project_root: normalize_path(&paths.project_root),
                storage_ready: report.is_some(),
                selection,
                filters,
                report,
            })?
        );
        return Ok(());
    }

    println!("knowledge inspect references duplicates");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_reference_selection(&selection);
    print_reference_filters(&filters);
    match report {
        Some(report) => print_reference_duplicates_report(&report),
        None => println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)"),
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn load_reference_selection(
    titles: &[String],
    titles_file: Option<&PathBuf>,
    all: bool,
) -> Result<ReferenceInspectSelection> {
    let mut loaded_titles = titles.to_vec();
    if let Some(titles_file) = titles_file {
        let content = fs::read_to_string(titles_file)
            .map_err(anyhow::Error::from)
            .map(|content| {
                content
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })?;
        loaded_titles.extend(content);
    }
    if all && !loaded_titles.is_empty() {
        bail!("cannot combine --all with --title/--titles-file");
    }
    Ok(ReferenceInspectSelection {
        all,
        titles: loaded_titles,
    })
}

fn reference_audit_filters_from_args(
    domain: Option<String>,
    template: Option<String>,
    authority: Option<String>,
    identifier_key: Option<String>,
    identifier: Option<String>,
) -> ReferenceAuditFilters {
    ReferenceAuditFilters {
        domain: normalize_optional_string(domain),
        template: normalize_optional_string(template),
        authority: normalize_optional_string(authority),
        identifier_key: normalize_optional_string(identifier_key),
        identifier: normalize_optional_string(identifier),
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn print_reference_selection(selection: &ReferenceInspectSelection) {
    println!(
        "selection.all: {}",
        if selection.all { "yes" } else { "no" }
    );
    if selection.titles.is_empty() {
        println!("selection.titles: <none>");
    } else {
        println!("selection.titles: {}", selection.titles.join(" | "));
    }
}

fn print_reference_filters(filters: &ReferenceAuditFilters) {
    println!(
        "filters.domain: {}",
        filters.domain.as_deref().unwrap_or("<none>")
    );
    println!(
        "filters.template: {}",
        filters.template.as_deref().unwrap_or("<none>")
    );
    println!(
        "filters.authority: {}",
        filters.authority.as_deref().unwrap_or("<none>")
    );
    println!(
        "filters.identifier_key: {}",
        filters.identifier_key.as_deref().unwrap_or("<none>")
    );
    println!(
        "filters.identifier: {}",
        filters.identifier.as_deref().unwrap_or("<none>")
    );
}

fn print_reference_summary_report(report: &ReferenceAuditSummaryReport) {
    println!("references.count: {}", report.reference_count);
    println!("references.distinct_pages: {}", report.distinct_page_count);
    println!(
        "references.distinct_domains: {}",
        report.distinct_domain_count
    );
    println!(
        "references.distinct_templates: {}",
        report.distinct_template_count
    );
    println!(
        "references.distinct_authorities: {}",
        report.distinct_authority_count
    );
    println!(
        "references.distinct_identifier_keys: {}",
        report.distinct_identifier_key_count
    );
    println!(
        "references.distinct_identifier_entries: {}",
        report.distinct_identifier_entry_count
    );
    println!(
        "references.top_domains: {}",
        join_or_none(&report.top_domains)
    );
    println!(
        "references.top_templates: {}",
        join_or_none(&report.top_templates)
    );
    println!(
        "references.top_authorities: {}",
        join_or_none(&report.top_authorities)
    );
    println!(
        "references.top_identifier_keys: {}",
        join_or_none(&report.top_identifier_keys)
    );
    println!(
        "references.top_identifier_entries: {}",
        join_or_none(&report.top_identifier_entries)
    );
}

fn print_reference_list_report(report: &ReferenceListReport) {
    println!("references.count: {}", report.reference_count);
    if report.items.is_empty() {
        println!("references: <none>");
        return;
    }
    for item in &report.items {
        print_reference_item(item);
    }
}

fn print_reference_duplicates_report(report: &ReferenceDuplicatesReport) {
    println!("duplicate_groups.count: {}", report.duplicate_group_count);
    println!(
        "duplicate_groups.duplicated_reference_count: {}",
        report.duplicated_reference_count
    );
    if report.groups.is_empty() {
        println!("duplicate_groups: <none>");
        return;
    }
    for group in &report.groups {
        print_reference_duplicate_group(group);
    }
}

fn print_reference_item(item: &ReferenceListItem) {
    println!(
        "reference: page={} path={} index={} section={} name={} group={} template={} authority={} domain={} canonical_url={} identifiers={} title={} summary={}",
        item.source_title,
        item.source_relative_path,
        item.reference_index,
        item.section_heading.as_deref().unwrap_or("<lead>"),
        item.reference_name.as_deref().unwrap_or("<none>"),
        item.reference_group.as_deref().unwrap_or("<none>"),
        item.primary_template_title.as_deref().unwrap_or("<none>"),
        if item.source_authority.is_empty() {
            "<none>"
        } else {
            &item.source_authority
        },
        if item.source_domain.is_empty() {
            "<none>"
        } else {
            &item.source_domain
        },
        if item.canonical_url.is_empty() {
            "<none>"
        } else {
            &item.canonical_url
        },
        join_or_none(&item.identifier_entries),
        if item.reference_title.is_empty() {
            "<none>"
        } else {
            &item.reference_title
        },
        if item.summary_text.is_empty() {
            "<none>"
        } else {
            &item.summary_text
        }
    );
}

fn print_reference_duplicate_group(group: &ReferenceDuplicateGroup) {
    println!(
        "duplicate_group: kind={} key={} references={} pages={} titles={}",
        match group.kind {
            wikitool_core::knowledge::references::ReferenceDuplicateKind::CanonicalUrl => {
                "canonical_url"
            }
            wikitool_core::knowledge::references::ReferenceDuplicateKind::NormalizedIdentifier => {
                "normalized_identifier"
            }
            wikitool_core::knowledge::references::ReferenceDuplicateKind::ExactReferenceWikitext => {
                "exact_reference_wikitext"
            }
        },
        group.match_key,
        group.reference_count,
        group.distinct_page_count,
        join_or_none(&group.source_titles)
    );
    for item in &group.items {
        print_reference_item(item);
    }
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "<none>".to_string()
    } else {
        values.join(" | ")
    }
}
