use anyhow::Result;
use wikitool_core::filesystem::{ScanOptions, scan_stats};
use wikitool_core::knowledge::content_index::load_stored_index_stats;
use wikitool_core::knowledge::inspect::{query_empty_categories, query_orphans};

use crate::cli_support::{
    normalize_path, print_scan_stats, print_stored_index_stats, resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};
pub(super) fn run_inspect_orphans(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("knowledge inspect orphans");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("mode: report-only");
    match query_orphans(&paths)? {
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

pub(super) fn run_inspect_empty_categories(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;

    println!("knowledge inspect empty-categories");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("mode: report-only");
    match query_empty_categories(&paths)? {
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

pub(super) fn run_inspect_stats(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let scan = scan_stats(&paths, &ScanOptions::default())?;
    let stored = load_stored_index_stats(&paths)?;

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
