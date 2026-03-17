use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use crate::cli_support::{
    copy_file, normalize_path, resolve_git_hooks_dir, resolve_repo_root, set_executable_if_unix,
};

#[derive(Debug, Args)]
pub(crate) struct DevArgs {
    #[command(subcommand)]
    command: DevSubcommand,
}

#[derive(Debug, Subcommand)]
enum DevSubcommand {
    #[command(
        name = "install-git-hooks",
        about = "Install the commit-msg hook into the target Git worktree"
    )]
    InstallGitHooks(InstallGitHooksArgs),
}

#[derive(Debug, Args)]
pub(crate) struct InstallGitHooksArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Repository root containing .git/hooks (default: current directory)"
    )]
    pub(crate) repo_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Hook source file (default: scripts/git-hooks/commit-msg under repo root)"
    )]
    pub(crate) source: Option<PathBuf>,
    #[arg(
        long,
        help = "Do not fail when .git/hooks is missing (useful for zip-distributed binaries)"
    )]
    pub(crate) allow_missing_git: bool,
}

pub(crate) fn run_dev(args: DevArgs) -> Result<()> {
    match args.command {
        DevSubcommand::InstallGitHooks(options) => run_dev_install_git_hooks(options),
    }
}

pub(crate) fn run_dev_install_git_hooks(args: InstallGitHooksArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root)?;
    let Some(hooks_dir) = resolve_git_hooks_dir(&repo_root)? else {
        if args.allow_missing_git {
            println!(
                "No git hooks directory found under {}. Skipping hook install.",
                normalize_path(&repo_root)
            );
            return Ok(());
        }
        bail!(
            "no git hooks directory found under {}",
            normalize_path(&repo_root)
        );
    };

    let source = args
        .source
        .unwrap_or_else(|| repo_root.join("scripts/git-hooks/commit-msg"));
    if !source.is_file() {
        bail!("hook source not found: {}", normalize_path(&source));
    }
    let destination = hooks_dir.join("commit-msg");
    copy_file(&source, &destination)?;
    set_executable_if_unix(&destination)?;

    println!(
        "Installed commit-msg hook: {}",
        normalize_path(&destination)
    );
    Ok(())
}
