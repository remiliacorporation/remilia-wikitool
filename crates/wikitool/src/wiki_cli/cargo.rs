use std::collections::BTreeMap;

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use wikitool_core::mw::{
    CargoField, CargoRowsOptions, MediaWikiClient, cargo_count_rows, cargo_list_tables,
    cargo_query_rows, cargo_table_fields,
};

use crate::RuntimeOptions;
use crate::cli_support::{normalize_path, resolve_runtime_with_config};

use super::*;

#[derive(Debug, Serialize)]
struct WikiCargoCountReport {
    table: String,
    rows: u64,
}

#[derive(Debug, Serialize)]
struct WikiCargoTablesReport {
    tables: Vec<String>,
}

#[derive(Debug, Serialize)]
struct WikiCargoFieldsReport {
    table: String,
    fields: Vec<CargoField>,
}

#[derive(Debug, Serialize)]
struct WikiCargoRowsReport {
    table: String,
    fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    where_clause: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order_by: Option<String>,
    limit: usize,
    offset: usize,
    row_count: usize,
    rows: Vec<BTreeMap<String, Value>>,
}

pub(super) fn run_wiki_cargo(runtime: &RuntimeOptions, args: WikiCargoArgs) -> Result<()> {
    match args.command {
        WikiCargoSubcommand::Count(args) => run_wiki_cargo_count(runtime, args),
        WikiCargoSubcommand::Tables(args) => run_wiki_cargo_tables(runtime, args),
        WikiCargoSubcommand::Fields(args) => run_wiki_cargo_fields(runtime, args),
        WikiCargoSubcommand::Rows(args) => run_wiki_cargo_rows(runtime, args),
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

fn run_wiki_cargo_tables(runtime: &RuntimeOptions, args: WikiCargoTablesArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let mut client = MediaWikiClient::from_config(&config)?;
    let report = WikiCargoTablesReport {
        tables: cargo_list_tables(&mut client)?,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("wiki cargo tables");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("tables.count: {}", report.tables.len());
    for table in &report.tables {
        println!("table: {table}");
    }
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_wiki_cargo_fields(runtime: &RuntimeOptions, args: WikiCargoFieldsArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let mut client = MediaWikiClient::from_config(&config)?;
    let report = WikiCargoFieldsReport {
        fields: cargo_table_fields(&mut client, &args.table)?,
        table: args.table,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("wiki cargo fields");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("table: {}", report.table);
    for field in &report.fields {
        let list_suffix = if field.is_list {
            format!(
                " (list, delimiter {})",
                field.delimiter.as_deref().unwrap_or(",")
            )
        } else {
            String::new()
        };
        println!(
            "field: {} = {}{}",
            field.name, field.field_type, list_suffix
        );
    }
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_wiki_cargo_rows(runtime: &RuntimeOptions, args: WikiCargoRowsArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let mut client = MediaWikiClient::from_config(&config)?;

    // Default the selection to the table's full schema so `wiki cargo rows <table>`
    // works without the caller knowing the fields.
    let fields = if args.fields.is_empty() {
        cargo_table_fields(&mut client, &args.table)?
            .into_iter()
            .map(|field| field.name)
            .collect()
    } else {
        args.fields
            .iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };

    let options = CargoRowsOptions {
        table: args.table.clone(),
        fields,
        where_clause: args.where_clause.clone(),
        order_by: args.order_by.clone(),
        limit: args.limit,
        offset: args.offset,
    };
    let rows = cargo_query_rows(&mut client, &options)?;
    let report = WikiCargoRowsReport {
        table: args.table,
        fields: options.fields,
        where_clause: options.where_clause,
        order_by: options.order_by,
        limit: options.limit,
        offset: options.offset,
        row_count: rows.len(),
        rows,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("wiki cargo rows");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("table: {}", report.table);
    println!("row_count: {}", report.row_count);
    for row in &report.rows {
        let rendered = row
            .iter()
            .map(|(key, value)| match value {
                Value::String(text) => format!("{key}={text}"),
                other => format!("{key}={other}"),
            })
            .collect::<Vec<_>>()
            .join(" | ");
        println!("row: {rendered}");
    }
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
