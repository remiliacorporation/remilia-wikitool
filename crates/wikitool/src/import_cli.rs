use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use wikitool_core::import_cargo::{
    CargoImportOptions, ImportError, ImportPageResult, ImportSourceType, ImportUpdateMode,
    import_to_cargo,
};

use crate::cli_support::{
    OutputFormat, format_flag, normalize_option, normalize_path, resolve_runtime_paths,
};
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
        #[arg(long, value_enum, value_name = "TYPE", help = "Input type: csv|json")]
        r#type: Option<ImportSourceTypeArg>,
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
            value_enum,
            default_value_t = ImportUpdateModeArg::Create,
            value_name = "MODE",
            help = "create|update|upsert"
        )]
        mode: ImportUpdateModeArg,
        #[arg(long, help = "Write files (default: dry-run)")]
        write: bool,
        #[arg(
            long,
            value_enum,
            default_value_t = OutputFormat::Text,
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: OutputFormat,
        #[arg(
            long,
            help = "Add SHORTDESC + Article quality header in main namespace"
        )]
        article_header: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
    },
}

#[derive(Debug, Serialize)]
struct ImportJson<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pages_created: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pages_updated: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pages_skipped: Option<&'a [String]>,
    errors: &'a [ImportError],
    pages: &'a [ImportPageResult],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ImportSourceTypeArg {
    Csv,
    Json,
}

impl From<ImportSourceTypeArg> for ImportSourceType {
    fn from(value: ImportSourceTypeArg) -> Self {
        match value {
            ImportSourceTypeArg::Csv => Self::Csv,
            ImportSourceTypeArg::Json => Self::Json,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ImportUpdateModeArg {
    Create,
    Update,
    Upsert,
}

impl ImportUpdateModeArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Upsert => "upsert",
        }
    }
}

impl From<ImportUpdateModeArg> for ImportUpdateMode {
    fn from(value: ImportUpdateModeArg) -> Self {
        match value {
            ImportUpdateModeArg::Create => Self::Create,
            ImportUpdateModeArg::Update => Self::Update,
            ImportUpdateModeArg::Upsert => Self::Upsert,
        }
    }
}

impl std::fmt::Display for ImportUpdateModeArg {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
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
            no_meta,
        } => run_import_cargo(
            runtime,
            &path,
            &table,
            r#type.map(ImportSourceType::from),
            template.as_deref(),
            title_field.as_deref(),
            title_prefix.as_deref(),
            category.as_deref(),
            ImportUpdateMode::from(mode),
            write,
            format,
            article_header,
            no_meta,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_import_cargo(
    runtime: &RuntimeOptions,
    path: &str,
    table: &str,
    source_type: Option<ImportSourceType>,
    template: Option<&str>,
    title_field: Option<&str>,
    title_prefix: Option<&str>,
    category: Option<&str>,
    update_mode: ImportUpdateMode,
    write: bool,
    format: OutputFormat,
    article_header: bool,
    no_meta: bool,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let Some(source_type) = source_type.or_else(|| ImportSourceType::resolve(path, None)) else {
        bail!("unable to determine import type (use --type csv|json)");
    };

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

    if format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&import_json_output(&result, no_meta))?
        );
    } else {
        println!("import cargo");
        println!("source_path: {}", normalize_path(&source_path));
        println!("source_type: {}", source_type.as_str());
        println!("table: {table}");
        println!("update_mode: {}", update_mode.as_str());
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

fn import_json_output<'a>(
    result: &'a wikitool_core::import_cargo::ImportResult,
    no_meta: bool,
) -> ImportJson<'a> {
    ImportJson {
        pages_created: if no_meta {
            None
        } else {
            Some(&result.pages_created)
        },
        pages_updated: if no_meta {
            None
        } else {
            Some(&result.pages_updated)
        },
        pages_skipped: if no_meta {
            None
        } else {
            Some(&result.pages_skipped)
        },
        errors: &result.errors,
        pages: &result.pages,
    }
}

fn resolve_import_source_path(path: &str) -> Result<PathBuf> {
    if Path::new(path).is_absolute() {
        return Ok(PathBuf::from(path));
    }

    Ok(env::current_dir()
        .context("failed to resolve current directory")?
        .join(path))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use wikitool_core::import_cargo::{ImportPageAction, ImportResult};

    #[test]
    fn import_no_meta_json_omits_summary_indexes() {
        let result = ImportResult {
            pages_created: vec!["Alpha".to_string()],
            pages_updated: vec!["Beta".to_string()],
            pages_skipped: vec!["Gamma".to_string()],
            errors: vec![ImportError {
                row: 3,
                message: "Missing title".to_string(),
                title: None,
            }],
            pages: vec![ImportPageResult {
                title: "Alpha".to_string(),
                relative_path: "wiki_content/Main/Alpha.wiki".to_string(),
                action: ImportPageAction::Create,
                content: Some("Alpha content".to_string()),
            }],
        };

        let value =
            serde_json::to_value(import_json_output(&result, true)).expect("serialize import");

        assert!(value.get("pages_created").is_none());
        assert!(value.get("pages_updated").is_none());
        assert!(value.get("pages_skipped").is_none());
        assert_eq!(value["errors"][0]["row"], json!(3));
        assert_eq!(value["pages"][0]["title"], json!("Alpha"));
    }
}
