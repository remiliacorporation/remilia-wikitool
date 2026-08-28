use anyhow::Result;
use wikitool_core::catalog::status::catalog_status;

use crate::cli_support::{
    normalize_path, print_database_schema_status, print_scan_stats,
    resolve_runtime_with_docs_profile,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::shared::{print_catalog_status, rebuild_catalog};
use super::*;
pub(super) fn run_catalog_build(runtime: &RuntimeOptions, args: CatalogBuildArgs) -> Result<()> {
    let (paths, _config, docs_profile) = resolve_runtime_with_docs_profile(runtime, None)?;
    let rebuild = rebuild_catalog(&paths)?;
    let status = catalog_status(&paths, &docs_profile)?;
    let report = CatalogBuildReport { rebuild, status };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("catalog build");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "docs_profile_requested: {}",
        report.status.docs_profile_requested
    );
    println!("catalog_generation: {}", report.status.catalog_generation);
    println!("rebuild.unchanged: {}", report.rebuild.unchanged);
    println!("rebuild.inserted_rows: {}", report.rebuild.inserted_rows);
    println!("rebuild.inserted_links: {}", report.rebuild.inserted_links);
    print_scan_stats("scan", &report.rebuild.scan);
    print_catalog_status("catalog", &report.status);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
