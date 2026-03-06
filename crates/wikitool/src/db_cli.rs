use std::fs;

use anyhow::Result;
use clap::{Args, Subcommand};
use wikitool_core::filesystem::ScanOptions;
use wikitool_core::index::{load_stored_index_stats, rebuild_index};
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};

use crate::cli_support::{
    format_flag, normalize_path, print_database_schema_status, print_scan_stats,
    print_stored_index_stats, prompt_yes_no, resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct DbArgs {
    #[command(subcommand)]
    command: DbSubcommand,
}

#[derive(Debug, Subcommand)]
enum DbSubcommand {
    Stats,
    Sync,
    Reset {
        #[arg(
            long,
            help = "Assume yes and delete the local database without prompting"
        )]
        yes: bool,
    },
}

pub(crate) fn run_db(runtime: &RuntimeOptions, args: DbArgs) -> Result<()> {
    match args.command {
        DbSubcommand::Stats => run_db_stats(runtime),
        DbSubcommand::Sync => run_db_sync(runtime),
        DbSubcommand::Reset { yes } => run_db_reset(runtime, yes),
    }
}

fn run_db_stats(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    let stored = load_stored_index_stats(&paths)?;

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
    match stored {
        Some(stored) => print_stored_index_stats("index", &stored),
        None => println!("index.storage: <not built> (run `wikitool index rebuild`)"),
    }
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn run_db_sync(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;

    let report = rebuild_index(&paths, &ScanOptions::default())?;

    println!("db sync");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("db_path: {}", normalize_path(&paths.db_path));
    println!("synced_rows: {}", report.inserted_rows);
    println!("synced_links: {}", report.inserted_links);
    print_scan_stats("scan", &report.scan);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn run_db_reset(runtime: &RuntimeOptions, yes: bool) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let normalized_path = normalize_path(&paths.db_path);
    if paths.db_path.exists()
        && !yes
        && !prompt_yes_no(&format!("Delete local database {normalized_path}? (y/N) "))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let deleted = if paths.db_path.exists() {
        fs::remove_file(&paths.db_path)?;
        true
    } else {
        false
    };

    println!("db reset");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("db_path: {normalized_path}");
    println!("deleted: {}", format_flag(deleted));
    println!("next_step: run `wikitool db sync` or `wikitool pull --full --all`");
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}
