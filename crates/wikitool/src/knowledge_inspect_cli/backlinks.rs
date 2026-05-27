use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::knowledge::inspect::query_backlinks;

use crate::cli_support::{normalize_path, normalize_title_query, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::*;
#[derive(Debug, Serialize)]
struct BacklinksJson {
    project_root: String,
    storage_ready: bool,
    title: String,
    count: usize,
    backlinks: Vec<String>,
}

pub(super) fn run_inspect_backlinks(
    runtime: &RuntimeOptions,
    title: &str,
    format: OutputFormat,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let normalized = normalize_title_query(title);
    if normalized.is_empty() {
        bail!("knowledge inspect backlinks requires a non-empty TITLE");
    }

    let backlinks = query_backlinks(&paths, &normalized)?;
    if format.is_json() {
        let storage_ready = backlinks.is_some();
        let backlinks = backlinks.unwrap_or_default();
        println!(
            "{}",
            serde_json::to_string_pretty(&BacklinksJson {
                project_root: normalize_path(&paths.project_root),
                storage_ready,
                title: normalized.clone(),
                count: backlinks.len(),
                backlinks,
            })?
        );
        return Ok(());
    }

    println!("knowledge inspect backlinks");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("title: {normalized}");
    match backlinks {
        Some(backlinks) => {
            println!("backlinks.count: {}", backlinks.len());
            if backlinks.is_empty() {
                println!("backlinks: <none>");
            } else {
                for link in backlinks {
                    println!("backlink: {link}");
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
