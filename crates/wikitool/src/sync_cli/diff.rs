use anyhow::Result;
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{DiffOptions, diff_local_against_sync};

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::DiffArgs;
use super::shared::{format_baseline_status, format_diff_change_type, load_sync_selection};

pub(crate) fn run_diff(runtime: &RuntimeOptions, args: DiffArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;
    let selection = load_sync_selection(&args.titles, &args.paths, args.titles_file.as_ref())?;

    let report = match diff_local_against_sync(
        &paths,
        &DiffOptions {
            include_templates: args.templates,
            categories_only: args.categories,
            include_content: args.content,
            selection: selection.clone(),
        },
    )? {
        Some(report) => report,
        None => {
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::json!({
                        "project_root": normalize_path(&paths.project_root),
                        "sync_ledger_ready": false,
                        "templates": args.templates,
                        "categories": args.categories,
                        "content": args.content,
                        "selection": selection,
                    })
                );
            } else {
                println!("diff");
                println!("project_root: {}", normalize_path(&paths.project_root));
                println!("templates: {}", args.templates);
                println!("categories: {}", args.categories);
                println!("content: {}", args.content);
                println!(
                    "diff.sync_ledger: <not built> (run `wikitool pull --full{}`)",
                    if args.templates { " --templates" } else { "" }
                );
                println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
                if runtime.diagnostics {
                    println!("\n[diagnostics]\n{}", paths.diagnostics());
                }
            }
            return Ok(());
        }
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("diff");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("templates: {}", args.templates);
    println!("categories: {}", args.categories);
    println!("verbose: {}", args.verbose);
    println!("content: {}", args.content);
    if !selection.titles.is_empty() {
        println!("selection.titles: {}", selection.titles.join(" | "));
    }
    if !selection.paths.is_empty() {
        println!("selection.paths: {}", selection.paths.join(" | "));
    }

    println!("diff.new_local: {}", report.new_local);
    println!("diff.modified_local: {}", report.modified_local);
    println!("diff.deleted_local: {}", report.deleted_local);
    println!("diff.conflicts.count: {}", report.conflict_count);
    println!("diff.total: {}", report.changes.len());

    if report.changes.is_empty() {
        println!("diff.changes: <none>");
    } else {
        for change in &report.changes {
            println!(
                "diff.change: type={} title={} path={}",
                format_diff_change_type(&change.change_type),
                change.title,
                change.relative_path
            );
            if args.verbose {
                println!(
                    "diff.change.hashes: local={} synced={}",
                    change.local_hash.as_deref().unwrap_or("<none>"),
                    change.synced_hash.as_deref().unwrap_or("<none>")
                );
                println!(
                    "diff.change.synced_wiki_timestamp: {}",
                    change.synced_wiki_timestamp.as_deref().unwrap_or("<none>")
                );
                if args.content {
                    println!(
                        "diff.change.baseline_status: {}",
                        format_baseline_status(change.baseline_status.as_ref())
                    );
                }
            }
            if args.content
                && let Some(unified_diff) = &change.unified_diff
            {
                println!("diff.change.content:");
                print!("{unified_diff}");
            }
        }
    }

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
