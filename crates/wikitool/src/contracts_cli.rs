use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use wikitool_core::contracts::{command_surface, generate_fixture_snapshot};

#[derive(Debug, Args)]
pub(crate) struct ContractsArgs {
    #[command(subcommand)]
    command: ContractsSubcommand,
}

#[derive(Debug, Subcommand)]
enum ContractsSubcommand {
    #[command(about = "Generate an offline fixture snapshot used by the differential harness")]
    Snapshot(SnapshotArgs),
    #[command(about = "Print frozen command-surface contract as JSON")]
    CommandSurface,
}

#[derive(Debug, Args)]
struct SnapshotArgs {
    #[arg(long, default_value = ".")]
    project_root: PathBuf,
    #[arg(long, default_value = "wiki_content")]
    content_dir: String,
    #[arg(long, default_value = "templates")]
    templates_dir: String,
}

pub(crate) fn run_contracts(args: ContractsArgs) -> Result<()> {
    match args.command {
        ContractsSubcommand::Snapshot(snapshot) => {
            let report = generate_fixture_snapshot(
                &snapshot.project_root,
                &snapshot.content_dir,
                &snapshot.templates_dir,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        ContractsSubcommand::CommandSurface => {
            println!("{}", serde_json::to_string_pretty(&command_surface())?);
        }
    }
    Ok(())
}
