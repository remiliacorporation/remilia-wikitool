use anyhow::{Result, bail};

use crate::cli_support::{copy_file, normalize_path, set_executable_if_unix};
use crate::{DevArgs, DevSubcommand, InstallGitHooksArgs, release};

pub(crate) fn run_dev(args: DevArgs) -> Result<()> {
    match args.command {
        DevSubcommand::InstallGitHooks(options) => run_dev_install_git_hooks(options),
    }
}

pub(crate) fn run_dev_install_git_hooks(args: InstallGitHooksArgs) -> Result<()> {
    let repo_root = release::resolve_repo_root(args.repo_root)?;
    let hooks_dir = repo_root.join(".git/hooks");
    if !hooks_dir.is_dir() {
        if args.allow_missing_git {
            println!(
                "No .git/hooks directory found at {}. Skipping hook install.",
                normalize_path(&hooks_dir)
            );
            return Ok(());
        }
        bail!(
            "no .git/hooks directory found at {}",
            normalize_path(&hooks_dir)
        );
    }

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
