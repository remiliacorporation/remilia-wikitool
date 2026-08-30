use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};

mod agent_pack;
mod bundle;
mod release_payload;

#[derive(Debug, Args)]
pub(crate) struct ReleaseArgs {
    #[command(subcommand)]
    command: ReleaseSubcommand,
}

#[derive(Debug, Subcommand)]
enum ReleaseSubcommand {
    #[command(
        name = "build-agent-pack",
        about = "Build the deterministic Wikitool agent pack"
    )]
    BuildAgentPack(ReleaseBuildAgentPackArgs),
    #[command(about = "Stage one local binary and its release payload")]
    Package(ReleasePackageArgs),
    #[command(name = "build-matrix")]
    #[command(about = "Build and package release bundles for one or more targets")]
    BuildMatrix(ReleaseBuildMatrixArgs),
}

#[derive(Debug, Args)]
struct ReleaseBuildAgentPackArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Wikitool repository root (default: current directory)"
    )]
    repo_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Output directory (default: <repo>/dist/agent)"
    )]
    output_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ReleasePackageArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Wikitool repository root (default: current directory)"
    )]
    repo_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Release binary path (default: <repo>/target/release/wikitool[.exe])"
    )]
    binary_path: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Output directory (default: <repo>/dist/release)"
    )]
    output_dir: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional host project root containing a wikitool_adapter/ supplement"
    )]
    host_project_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Hash-verified Contextmink distribution root (default: <repo>/dist/contextmink-dist)"
    )]
    contextmink_dist: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Hash-verified Papertiger distribution root (default: <repo>/dist/papertiger-dist)"
    )]
    papertiger_dist: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ReleaseBuildMatrixArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Wikitool repository root (default: current directory)"
    )]
    repo_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "TRIPLE",
        value_delimiter = ',',
        help = "Target triples to build (repeat or use comma-separated list). Defaults to windows/linux/macos x86_64 targets."
    )]
    targets: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Output directory for staged folders and zip artifacts (default: <repo>/dist/release-matrix)"
    )]
    output_dir: Option<PathBuf>,
    #[arg(
        long,
        value_name = "LABEL",
        help = "Version label used in bundle names (default: CARGO_PKG_VERSION; a leading v is stripped)"
    )]
    artifact_version: Option<String>,
    #[arg(
        long,
        help = "Use unversioned bundle names (wikitool-<target>) for CI/ephemeral artifacts"
    )]
    unversioned_names: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Cargo executable path used for target builds (default: cargo)"
    )]
    cargo_bin: Option<PathBuf>,
    #[arg(long, help = "Skip cargo build and package existing target binaries")]
    skip_build: bool,
    #[arg(long, help = "Use cargo --locked for target builds (default: true)")]
    locked: bool,
    #[arg(long, help = "Do not pass --locked to cargo builds")]
    no_locked: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional host project root containing a wikitool_adapter/ supplement"
    )]
    host_project_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Hash-verified Contextmink distribution root (default: <repo>/dist/contextmink-dist)"
    )]
    contextmink_dist: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Hash-verified Papertiger distribution root (default: <repo>/dist/papertiger-dist)"
    )]
    papertiger_dist: Option<PathBuf>,
}

pub(crate) fn run_release(args: ReleaseArgs) -> Result<()> {
    match args.command {
        ReleaseSubcommand::BuildAgentPack(options) => {
            agent_pack::run_release_build_agent_pack(options)
        }
        ReleaseSubcommand::Package(options) => bundle::run_release_package(options),
        ReleaseSubcommand::BuildMatrix(options) => bundle::run_release_build_matrix(options),
    }
}
