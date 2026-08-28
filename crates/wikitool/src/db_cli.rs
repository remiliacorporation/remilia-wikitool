use std::fs;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::catalog::content_index::load_stored_index_stats;
use wikitool_core::catalog::status::catalog_status;
use wikitool_core::runtime::inspect_runtime;
use wikitool_core::schema::{
    DatabaseSchemaState, reset_catalog_preserving_durable_state, schema_state,
};
use wikitool_core::sync::SyncStoreMigrationStatus;

use crate::cli_support::{
    OutputFormat, format_flag, normalize_path, print_database_schema_status,
    print_stored_index_stats, prompt_yes_no, resolve_runtime_paths,
    resolve_runtime_with_docs_profile,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct DbArgs {
    #[command(subcommand)]
    command: DbSubcommand,
}

#[derive(Debug, Subcommand)]
enum DbSubcommand {
    #[command(about = "Show local database state and catalog readiness")]
    Stats(DbStatsArgs),
    #[command(about = "Delete the disposable local catalog database")]
    Reset {
        #[arg(
            long,
            help = "Assume yes and delete the catalog database without prompting"
        )]
        yes: bool,
    },
}

#[derive(Debug, Args)]
struct DbStatsArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct DbStatsJson {
    db_path: String,
    data_dir: String,
    db_exists: bool,
    db_size_bytes: Option<u64>,
    sync_store_path: String,
    sync_store_exists: bool,
    sync_store_size_bytes: Option<u64>,
    acceptance_store_path: String,
    acceptance_store_exists: bool,
    acceptance_store_size_bytes: Option<u64>,
    content_index: Option<wikitool_core::catalog::content_index::StoredIndexStats>,
    docs_profile_requested: String,
    readiness: wikitool_core::catalog::status::CatalogReadiness,
    degradations: Vec<String>,
    catalog_generation: String,
    database_schema: DbSchemaJson,
}

#[derive(Debug, Serialize)]
struct DbSchemaJson {
    status: String,
    reason: Option<String>,
}

pub(crate) fn run_db(runtime: &RuntimeOptions, args: DbArgs) -> Result<()> {
    match args.command {
        DbSubcommand::Stats(args) => run_db_stats(runtime, args),
        DbSubcommand::Reset { yes } => run_db_reset(runtime, yes),
    }
}

fn run_db_stats(runtime: &RuntimeOptions, args: DbStatsArgs) -> Result<()> {
    let (paths, _config, docs_profile) = resolve_runtime_with_docs_profile(runtime, None)?;
    let status = inspect_runtime(&paths)?;
    let stored = load_stored_index_stats(&paths)?;
    let catalog = catalog_status(&paths, &docs_profile)?;
    let sync_store_path = paths.sync_store_path();
    let sync_store_exists = sync_store_path.exists();
    let sync_store_size_bytes = if sync_store_exists {
        Some(
            fs::metadata(&sync_store_path)
                .with_context(|| format!("failed to inspect {}", sync_store_path.display()))?
                .len(),
        )
    } else {
        None
    };
    let acceptance_store_path = paths.acceptance_store_path();
    let acceptance_store_exists = acceptance_store_path.exists();
    let acceptance_store_size_bytes = if acceptance_store_exists {
        Some(
            fs::metadata(&acceptance_store_path)
                .with_context(|| format!("failed to inspect {}", acceptance_store_path.display()))?
                .len(),
        )
    } else {
        None
    };
    if args.format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&DbStatsJson {
                db_path: normalize_path(&paths.db_path),
                data_dir: normalize_path(&paths.data_dir),
                db_exists: status.db_exists,
                db_size_bytes: status.db_size_bytes,
                sync_store_path: normalize_path(&sync_store_path),
                sync_store_exists,
                sync_store_size_bytes,
                acceptance_store_path: normalize_path(&acceptance_store_path),
                acceptance_store_exists,
                acceptance_store_size_bytes,
                content_index: stored,
                docs_profile_requested: catalog.docs_profile_requested,
                readiness: catalog.readiness,
                degradations: catalog.degradations,
                catalog_generation: catalog.catalog_generation,
                database_schema: db_schema_json(&paths)?,
            })?
        );
        return Ok(());
    }

    println!("db stats");
    println!("db_path: {}", normalize_path(&paths.db_path));
    println!("data_dir: {}", normalize_path(&paths.data_dir));
    println!("db_exists: {}", format_flag(status.db_exists));
    println!(
        "db_size_bytes: {}",
        status
            .db_size_bytes
            .map(|size| size.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    );
    println!("sync_store_path: {}", normalize_path(&sync_store_path));
    println!("sync_store_exists: {}", format_flag(sync_store_exists));
    println!(
        "sync_store_size_bytes: {}",
        sync_store_size_bytes
            .map(|size| size.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    );
    println!(
        "acceptance_store_path: {}",
        normalize_path(&acceptance_store_path)
    );
    println!(
        "acceptance_store_exists: {}",
        format_flag(acceptance_store_exists)
    );
    println!(
        "acceptance_store_size_bytes: {}",
        acceptance_store_size_bytes
            .map(|size| size.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    );
    match stored {
        Some(stored) => print_stored_index_stats("content_index", &stored),
        None => println!("content_index.storage: <not built> (run `wikitool catalog build`)"),
    }
    println!("docs_profile_requested: {}", catalog.docs_profile_requested);
    println!(
        "readiness: {}",
        match catalog.readiness {
            wikitool_core::catalog::status::CatalogReadiness::NotReady => "not_ready",
            wikitool_core::catalog::status::CatalogReadiness::ContentReady => {
                "content_ready"
            }
            wikitool_core::catalog::status::CatalogReadiness::RetrievalReady => {
                "retrieval_ready"
            }
        }
    );
    println!(
        "degradations: {}",
        if catalog.degradations.is_empty() {
            "<none>".to_string()
        } else {
            catalog.degradations.join(", ")
        }
    );
    println!("catalog_generation: {}", catalog.catalog_generation);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn db_schema_json(paths: &wikitool_core::runtime::ResolvedPaths) -> Result<DbSchemaJson> {
    Ok(match schema_state(paths)? {
        DatabaseSchemaState::Missing => DbSchemaJson {
            status: "absent".to_string(),
            reason: None,
        },
        DatabaseSchemaState::Ready => DbSchemaJson {
            status: "ready".to_string(),
            reason: None,
        },
        DatabaseSchemaState::Incompatible { reason } => DbSchemaJson {
            status: "incompatible".to_string(),
            reason: Some(reason),
        },
    })
}

fn run_db_reset(runtime: &RuntimeOptions, yes: bool) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let normalized_path = normalize_path(&paths.db_path);
    if paths.db_path.exists()
        && !yes
        && !prompt_yes_no(&format!(
            "Delete disposable catalog database {normalized_path}? Durable sync identity will be preserved. (y/N) "
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let reset = reset_catalog_preserving_durable_state(&paths)?;

    println!("db reset");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("db_path: {normalized_path}");
    println!("deleted: {}", format_flag(reset.catalog_deleted));
    for path in &reset.catalog_sidecars_deleted {
        println!("deleted_sidecar: {}", normalize_path(path));
    }
    let sync_state = reset.sync_state;
    println!(
        "sync_store_path: {}",
        normalize_path(&sync_state.sync_store_path)
    );
    println!(
        "sync_store_status: {}",
        match sync_state.status {
            SyncStoreMigrationStatus::AlreadyCurrent => "preserved",
            SyncStoreMigrationStatus::MigratedLegacy => "migrated_legacy",
            SyncStoreMigrationStatus::NoLegacyState => "absent",
        }
    );
    println!(
        "sync_store_established: {}",
        format_flag(sync_state.established)
    );
    println!(
        "acceptance_store_path: {}",
        normalize_path(&reset.acceptance_store_path)
    );
    println!(
        "acceptance_store_preserved: {}",
        format_flag(reset.acceptance_store_preserved)
    );
    println!("next_step: run `wikitool catalog build` or `wikitool catalog warm`");
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}
