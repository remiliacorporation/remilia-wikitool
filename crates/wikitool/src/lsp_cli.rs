use anyhow::Result;
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::runtime::{
    embedded_parser_config, lsp_settings_json, materialize_parser_config,
};

use crate::cli_support::{
    OutputFormat, normalize_path, resolve_runtime_paths, resolve_runtime_with_config,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct LspArgs {
    #[command(subcommand)]
    command: LspSubcommand,
}

#[derive(Debug, Subcommand)]
enum LspSubcommand {
    #[command(about = "Write parser config and print editor settings JSON")]
    GenerateConfig(LspGenerateConfigArgs),
    #[command(about = "Show parser config and runtime config status")]
    Status(LspStatusArgs),
    #[command(about = "Show the preferred LSP integration entry point")]
    Info,
}

#[derive(Debug, Args)]
pub(crate) struct LspGenerateConfigArgs {
    #[arg(long, help = "Overwrite parser config if it already exists")]
    force: bool,
}

#[derive(Debug, Args)]
pub(crate) struct LspStatusArgs {
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
struct LspStatusJson {
    parser_config_path: String,
    parser_config_exists: bool,
    runtime_config_path: String,
    runtime_config_exists: bool,
    embedded_parser_baseline_bytes: usize,
}

pub(crate) fn run_lsp(runtime: &RuntimeOptions, args: LspArgs) -> Result<()> {
    match args.command {
        LspSubcommand::GenerateConfig(args) => run_lsp_generate_config(runtime, args),
        LspSubcommand::Status(args) => run_lsp_status(runtime, args),
        LspSubcommand::Info => run_lsp_info(),
    }
}

pub(crate) fn run_lsp_generate_config(
    runtime: &RuntimeOptions,
    args: LspGenerateConfigArgs,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let wrote = materialize_parser_config(&paths, args.force)?;
    if wrote {
        println!(
            "Wrote parser config: {}",
            normalize_path(&paths.parser_config_path)
        );
    } else {
        println!(
            "Parser config already exists: {} (use --force to overwrite)",
            normalize_path(&paths.parser_config_path)
        );
    }
    println!();
    println!("{}", lsp_settings_json(&paths, &config));
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

pub(crate) fn run_lsp_status(runtime: &RuntimeOptions, args: LspStatusArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let parser_config_exists = paths.parser_config_path.exists();
    let runtime_config_exists = paths.config_path.exists();
    let embedded_parser_baseline_bytes = embedded_parser_config().len();
    if args.format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&LspStatusJson {
                parser_config_path: normalize_path(&paths.parser_config_path),
                parser_config_exists,
                runtime_config_path: normalize_path(&paths.config_path),
                runtime_config_exists,
                embedded_parser_baseline_bytes,
            })?
        );
        return Ok(());
    }
    println!(
        "parser config: {} ({})",
        normalize_path(&paths.parser_config_path),
        if parser_config_exists {
            "found"
        } else {
            "missing"
        }
    );
    println!(
        "runtime config: {} ({})",
        normalize_path(&paths.config_path),
        if runtime_config_exists {
            "found"
        } else {
            "missing"
        }
    );
    println!(
        "embedded parser baseline bytes: {}",
        embedded_parser_baseline_bytes
    );
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

pub(crate) fn run_lsp_info() -> Result<()> {
    println!("wikitext LSP integration");
    println!("  command: wikitool lsp generate-config");
    println!("  output parser config: <project-root>/.wikitool/parser-config.json");
    println!("  policy: {LOCAL_DB_POLICY_MESSAGE}");
    Ok(())
}
