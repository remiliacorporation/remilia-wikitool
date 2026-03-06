use anyhow::Result;
use clap::Args;
use wikitool_core::runtime::{
    embedded_parser_config, lsp_settings_json, materialize_parser_config,
};

use crate::cli_support::{normalize_path, resolve_runtime_paths, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct LspGenerateConfigArgs {
    #[arg(long, help = "Overwrite parser config if it already exists")]
    force: bool,
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

pub(crate) fn run_lsp_status(runtime: &RuntimeOptions) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    println!(
        "parser config: {} ({})",
        normalize_path(&paths.parser_config_path),
        if paths.parser_config_path.exists() {
            "found"
        } else {
            "missing"
        }
    );
    println!(
        "runtime config: {} ({})",
        normalize_path(&paths.config_path),
        if paths.config_path.exists() {
            "found"
        } else {
            "missing"
        }
    );
    println!(
        "embedded parser baseline bytes: {}",
        embedded_parser_config().len()
    );
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

pub(crate) fn run_lsp_info() -> Result<()> {
    println!("wikitext LSP integration");
    println!("  command: wikitool lsp:generate-config");
    println!("  output parser config: <project-root>/.wikitool/parser-config.json");
    println!("  policy: {LOCAL_DB_POLICY_MESSAGE}");
    Ok(())
}
