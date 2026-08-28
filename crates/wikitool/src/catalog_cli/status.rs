use anyhow::Result;
use wikitool_core::catalog::status::catalog_status;

use crate::cli_support::{
    normalize_path, print_database_schema_status, resolve_runtime_with_docs_profile,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::shared::print_catalog_status;
use super::*;
pub(super) fn run_catalog_status(runtime: &RuntimeOptions, args: CatalogStatusArgs) -> Result<()> {
    let (paths, _config, docs_profile) =
        resolve_runtime_with_docs_profile(runtime, args.docs_profile.as_deref())?;
    let status = catalog_status(&paths, &docs_profile)?;

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    println!("catalog status");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_catalog_status("catalog", &status);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
