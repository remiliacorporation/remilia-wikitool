use anyhow::Result;
use serde::Serialize;
use wikitool_core::mw::{MediaWikiClient, cargo_count_rows};

use crate::RuntimeOptions;
use crate::cli_support::{normalize_path, resolve_runtime_with_config};

use super::*;

#[derive(Debug, Serialize)]
struct WikiCargoCountReport {
    table: String,
    rows: u64,
}

pub(super) fn run_wiki_cargo(runtime: &RuntimeOptions, args: WikiCargoArgs) -> Result<()> {
    match args.command {
        WikiCargoSubcommand::Count(args) => run_wiki_cargo_count(runtime, args),
    }
}

fn run_wiki_cargo_count(runtime: &RuntimeOptions, args: WikiCargoCountArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let mut client = MediaWikiClient::from_config(&config)?;
    let rows = cargo_count_rows(&mut client, &args.table)?;
    let report = WikiCargoCountReport {
        table: args.table,
        rows,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("wiki cargo count");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("table: {}", report.table);
    println!("rows: {}", report.rows);
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
