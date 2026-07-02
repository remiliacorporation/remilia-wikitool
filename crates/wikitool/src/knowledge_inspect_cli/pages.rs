use anyhow::Result;
use serde::Serialize;
use wikitool_core::filesystem::{ScanOptions, ScanStats, scan_stats};
use wikitool_core::knowledge::content_index::{StoredIndexStats, load_stored_index_stats};
use wikitool_core::knowledge::inspect::{query_empty_categories, query_orphans};

use crate::cli_support::{
    OutputFormat, normalize_path, print_scan_stats, print_stored_index_stats, resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Serialize)]
struct InspectTitleListReport {
    project_root: String,
    index_ready: bool,
    count: usize,
    titles: Vec<String>,
}

#[derive(Debug, Serialize)]
struct InspectStatsReport {
    project_root: String,
    wiki_content_dir: String,
    templates_dir: String,
    scan: ScanStats,
    index_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_index: Option<StoredIndexStats>,
}

pub(super) fn run_inspect_orphans(runtime: &RuntimeOptions, format: OutputFormat) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let orphans = query_orphans(&paths)?;

    if format.is_json() {
        let report = InspectTitleListReport {
            project_root: normalize_path(&paths.project_root),
            index_ready: orphans.is_some(),
            count: orphans.as_ref().map_or(0, Vec::len),
            titles: orphans.unwrap_or_default(),
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("knowledge inspect orphans");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("mode: report-only");
    match orphans {
        Some(orphans) => {
            println!("orphans.count: {}", orphans.len());
            if orphans.is_empty() {
                println!("orphans: <none>");
            } else {
                for title in orphans {
                    println!("orphan.title: {title}");
                }
            }
        }
        None => {
            println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

pub(super) fn run_inspect_empty_categories(
    runtime: &RuntimeOptions,
    format: OutputFormat,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let categories = query_empty_categories(&paths)?;

    if format.is_json() {
        let report = InspectTitleListReport {
            project_root: normalize_path(&paths.project_root),
            index_ready: categories.is_some(),
            count: categories.as_ref().map_or(0, Vec::len),
            titles: categories.unwrap_or_default(),
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("knowledge inspect empty-categories");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("mode: report-only");
    match categories {
        Some(categories) => {
            println!("empty_categories.count: {}", categories.len());
            if categories.is_empty() {
                println!("empty_categories: <none>");
            } else {
                for title in categories {
                    println!("empty_categories.title: {title}");
                }
            }
        }
        None => {
            println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

pub(super) fn run_inspect_stats(runtime: &RuntimeOptions, format: OutputFormat) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let scan = scan_stats(&paths, &ScanOptions::default())?;
    let stored = load_stored_index_stats(&paths)?;

    if format.is_json() {
        let report = InspectStatsReport {
            project_root: normalize_path(&paths.project_root),
            wiki_content_dir: normalize_path(&paths.wiki_content_dir),
            templates_dir: normalize_path(&paths.templates_dir),
            scan,
            index_ready: stored.is_some(),
            content_index: stored,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("knowledge inspect stats");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "wiki_content_dir: {}",
        normalize_path(&paths.wiki_content_dir)
    );
    println!("templates_dir: {}", normalize_path(&paths.templates_dir));
    print_scan_stats("scan", &scan);
    match stored {
        Some(stored) => print_stored_index_stats("content_index", &stored),
        None => println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)"),
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}
