use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use wikitool_core::import_cargo::{
    CargoImportOptions, ImportSourceType, ImportUpdateMode, import_to_cargo,
};

use crate::cli_support::{format_flag, normalize_option, normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct ImportArgs {
    #[command(subcommand)]
    command: ImportSubcommand,
}

#[derive(Debug, Subcommand)]
enum ImportSubcommand {
    Cargo {
        path: String,
        #[arg(long, value_name = "NAME", help = "Cargo table name")]
        table: String,
        #[arg(long, value_name = "TYPE", help = "Input type: csv|json")]
        r#type: Option<String>,
        #[arg(long, value_name = "NAME", help = "Template wrapper name")]
        template: Option<String>,
        #[arg(long, value_name = "FIELD", help = "Field name to use as page title")]
        title_field: Option<String>,
        #[arg(long, value_name = "PREFIX", help = "Prefix for generated page titles")]
        title_prefix: Option<String>,
        #[arg(long, value_name = "NAME", help = "Category to add to generated pages")]
        category: Option<String>,
        #[arg(
            long,
            default_value = "create",
            value_name = "MODE",
            help = "create|update|upsert"
        )]
        mode: String,
        #[arg(long, help = "Write files (default: dry-run)")]
        write: bool,
        #[arg(
            long,
            default_value = "text",
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: String,
        #[arg(
            long,
            help = "Add SHORTDESC + Article quality header in main namespace"
        )]
        article_header: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
    },
}

pub(crate) fn run_import(runtime: &RuntimeOptions, args: ImportArgs) -> Result<()> {
    match args.command {
        ImportSubcommand::Cargo {
            path,
            table,
            r#type,
            template,
            title_field,
            title_prefix,
            category,
            mode,
            write,
            format,
            article_header,
            no_meta: _,
        } => run_import_cargo(
            runtime,
            &path,
            &table,
            r#type.as_deref(),
            template.as_deref(),
            title_field.as_deref(),
            title_prefix.as_deref(),
            category.as_deref(),
            &mode,
            write,
            &format,
            article_header,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_import_cargo(
    runtime: &RuntimeOptions,
    path: &str,
    table: &str,
    source_type: Option<&str>,
    template: Option<&str>,
    title_field: Option<&str>,
    title_prefix: Option<&str>,
    category: Option<&str>,
    mode: &str,
    write: bool,
    format: &str,
    article_header: bool,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let Some(source_type) = ImportSourceType::resolve(path, source_type) else {
        bail!("unable to determine import type (use --type csv|json)");
    };
    let update_mode = parse_import_mode(mode)?;
    let format = format.to_ascii_lowercase();
    if format != "text" && format != "json" {
        bail!("unsupported import format: {format} (expected text|json)");
    }

    let source_path = resolve_import_source_path(path)?;
    let result = import_to_cargo(
        &paths,
        &source_path,
        source_type,
        &CargoImportOptions {
            table_name: table.to_string(),
            template_name: normalize_option(template),
            title_field: normalize_option(title_field),
            title_prefix: normalize_option(title_prefix),
            update_mode,
            category_name: normalize_option(category),
            article_header,
            write,
        },
    )?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("import cargo");
        println!("source_path: {}", normalize_path(&source_path));
        println!("source_type: {}", source_type.as_str());
        println!("table: {table}");
        println!("update_mode: {}", mode.to_ascii_lowercase());
        println!("write: {}", format_flag(write));
        println!("created: {}", result.pages_created.len());
        println!("updated: {}", result.pages_updated.len());
        println!("skipped: {}", result.pages_skipped.len());
        println!("errors: {}", result.errors.len());
        for error in result.errors.iter().take(10) {
            println!(
                "error: row={} message={} title={}",
                error.row,
                error.message,
                error.title.as_deref().unwrap_or("<none>")
            );
        }
        for page in result.pages.iter().take(10) {
            println!(
                "page: action={:?} title={} path={}",
                page.action, page.title, page.relative_path
            );
        }
        if !write {
            println!("warning: dry-run only. Use --write to apply changes.");
        }
    }

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn resolve_import_source_path(path: &str) -> Result<PathBuf> {
    if Path::new(path).is_absolute() {
        return Ok(PathBuf::from(path));
    }

    Ok(env::current_dir()
        .context("failed to resolve current directory")?
        .join(path))
}

fn parse_import_mode(value: &str) -> Result<ImportUpdateMode> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "create" => Ok(ImportUpdateMode::Create),
        "update" => Ok(ImportUpdateMode::Update),
        "upsert" => Ok(ImportUpdateMode::Upsert),
        _ => bail!("unsupported import mode: {value} (expected create|update|upsert)"),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_import_mode;
    use wikitool_core::import_cargo::ImportUpdateMode;

    #[test]
    fn parse_import_mode_accepts_supported_values() {
        assert!(matches!(
            parse_import_mode("create").expect("create"),
            ImportUpdateMode::Create
        ));
        assert!(matches!(
            parse_import_mode("update").expect("update"),
            ImportUpdateMode::Update
        ));
        assert!(matches!(
            parse_import_mode("upsert").expect("upsert"),
            ImportUpdateMode::Upsert
        ));
    }

    #[test]
    fn parse_import_mode_rejects_unknown_values() {
        assert!(parse_import_mode("replace").is_err());
    }
}
