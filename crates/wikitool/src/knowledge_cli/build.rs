use anyhow::Result;
use wikitool_core::knowledge::status::{DEFAULT_DOCS_PROFILE, knowledge_status};

use crate::cli_support::{
    normalize_path, print_database_schema_status, print_scan_stats, resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::shared::{print_knowledge_status, rebuild_knowledge_index};
use super::*;
pub(super) fn run_knowledge_build(
    runtime: &RuntimeOptions,
    args: KnowledgeBuildArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let rebuild = rebuild_knowledge_index(&paths)?;
    let status = knowledge_status(&paths, DEFAULT_DOCS_PROFILE)?;
    let report = KnowledgeBuildReport { rebuild, status };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("knowledge build");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "docs_profile_requested: {}",
        report.status.docs_profile_requested
    );
    println!(
        "knowledge_generation: {}",
        report.status.knowledge_generation
    );
    println!("rebuild.unchanged: {}", report.rebuild.unchanged);
    println!("rebuild.inserted_rows: {}", report.rebuild.inserted_rows);
    println!("rebuild.inserted_links: {}", report.rebuild.inserted_links);
    print_scan_stats("scan", &report.rebuild.scan);
    print_knowledge_status("knowledge", &report.status);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
