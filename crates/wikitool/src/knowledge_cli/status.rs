use anyhow::Result;
use wikitool_core::knowledge::status::knowledge_status;

use crate::cli_support::{normalize_path, print_database_schema_status, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::shared::print_knowledge_status;
use super::*;
pub(super) fn run_knowledge_status(
    runtime: &RuntimeOptions,
    args: KnowledgeStatusArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = knowledge_status(&paths, &args.docs_profile)?;

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    println!("knowledge status");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_knowledge_status("knowledge", &status);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
