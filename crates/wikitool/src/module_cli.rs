use anyhow::Result;
use clap::{Args, Subcommand};

use crate::RuntimeOptions;
use crate::quality_cli;

#[derive(Debug, Args)]
pub(crate) struct ModuleArgs {
    #[command(subcommand)]
    command: ModuleSubcommand,
}

#[derive(Debug, Subcommand)]
enum ModuleSubcommand {
    #[command(about = "Lint Lua modules")]
    Lint(quality_cli::LintArgs),
}

pub(crate) fn run_module(runtime: &RuntimeOptions, args: ModuleArgs) -> Result<()> {
    match args.command {
        ModuleSubcommand::Lint(args) => quality_cli::run_lint(runtime, args),
    }
}
