use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use crate::RuntimeOptions;
use crate::cli_support::{OutputFormat, normalize_path};
use crate::skills::{
    SKILLS_INSTALL_RECEIPT, SkillInstallAction, apply_skills_install, apply_skills_uninstall,
    load_skills, plan_skills_install, plan_skills_uninstall, resolve_project_root,
    verify_skills_install,
};

#[derive(Debug, Args)]
pub(crate) struct SkillsArgs {
    #[command(subcommand)]
    command: SkillsSubcommand,
}

#[derive(Debug, Subcommand)]
enum SkillsSubcommand {
    #[command(about = "Validate and describe a Wikitool skills distribution")]
    Inspect(SkillsInspectArgs),
    #[command(
        name = "setup-project",
        about = "Install Wikitool skills into a project"
    )]
    SetupProject(SkillsSetupArgs),
    #[command(
        name = "uninstall-project",
        about = "Remove an unchanged receipt-owned Wikitool skill installation"
    )]
    UninstallProject(SkillsUninstallArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum SkillTarget {
    Auto,
    Agents,
    Claude,
    Both,
}

#[derive(Debug, Args)]
struct SkillsInspectArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Skills root (default: skills/ beside the executable)"
    )]
    skills_root: Option<PathBuf>,
    #[arg(
        value_name = "PROJECT",
        help = "Also inspect this project's install receipt"
    )]
    project_root: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json, value_name = "FORMAT")]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct SkillsSetupArgs {
    #[arg(value_name = "PROJECT")]
    project_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Skills root (default: skills/ beside the executable)"
    )]
    skills_root: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = SkillTarget::Auto)]
    skill_target: SkillTarget,
    #[arg(
        long,
        help = "Validate and print the exact plan without changing files"
    )]
    dry_run: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, value_name = "FORMAT")]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct SkillsUninstallArgs {
    #[arg(value_name = "PROJECT")]
    project_root: Option<PathBuf>,
    #[arg(
        long,
        help = "Validate and print the exact plan without changing files"
    )]
    dry_run: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, value_name = "FORMAT")]
    format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct SkillsInspectOutput {
    schema: &'static str,
    status: &'static str,
    skills_root: String,
    wikitool_version: String,
    manifest_sha256: String,
    skills: Vec<String>,
    file_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<SkillsProjectStatus>,
}

#[derive(Debug, Serialize)]
struct SkillsProjectStatus {
    project_root: String,
    receipt_path: String,
    installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    wikitool_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skills_manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skills_aligned: Option<bool>,
    skill_targets: Vec<String>,
    managed_file_count: usize,
}

#[derive(Debug, Serialize)]
struct SkillsMutationOutput {
    schema: &'static str,
    operation: &'static str,
    status: &'static str,
    project_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    skills_root: Option<String>,
    skill_targets: Vec<String>,
    actions: Vec<SkillInstallAction>,
}

pub(crate) fn run_skills(runtime: &RuntimeOptions, args: SkillsArgs) -> Result<()> {
    match args.command {
        SkillsSubcommand::Inspect(options) => run_inspect(runtime, options),
        SkillsSubcommand::SetupProject(options) => run_setup(runtime, options),
        SkillsSubcommand::UninstallProject(options) => run_uninstall(runtime, options),
    }
}

fn run_inspect(runtime: &RuntimeOptions, args: SkillsInspectArgs) -> Result<()> {
    let skills = load_skills(&resolve_skills_root(args.skills_root)?)?;
    let project = args
        .project_root
        .or_else(|| runtime.project_root().map(Path::to_path_buf))
        .map(|path| inspect_project(&path, &skills.manifest_sha256))
        .transpose()?;
    let output = SkillsInspectOutput {
        schema: "wikitool.skills-inspect.v1",
        status: "valid",
        skills_root: normalize_path(&skills.root),
        wikitool_version: skills.manifest.wikitool_version.clone(),
        manifest_sha256: skills.manifest_sha256,
        skills: skills
            .manifest
            .skills
            .iter()
            .map(|skill| skill.id.clone())
            .collect(),
        file_count: skills.manifest.files.len(),
        project,
    };
    print_inspect(&output, args.format)
}

fn run_setup(runtime: &RuntimeOptions, args: SkillsSetupArgs) -> Result<()> {
    let requested = resolve_requested_project(args.project_root, runtime);
    let project_root = resolve_project_root(&requested)?;
    let skills = load_skills(&resolve_skills_root(args.skills_root)?)?;
    let targets = resolve_targets(args.skill_target, &project_root);
    let plan = plan_skills_install(&project_root, &skills, &targets)?;
    let output = SkillsMutationOutput {
        schema: "wikitool.skills-mutation.v1",
        operation: "setup-project",
        status: if args.dry_run { "planned" } else { "applied" },
        project_root: normalize_path(&project_root),
        skills_root: Some(normalize_path(&skills.root)),
        skill_targets: targets.iter().map(|target| (*target).to_string()).collect(),
        actions: plan.actions.clone(),
    };
    if !args.dry_run {
        apply_skills_install(&project_root, plan)?;
    }
    print_mutation(&output, args.format)
}

fn run_uninstall(runtime: &RuntimeOptions, args: SkillsUninstallArgs) -> Result<()> {
    let requested = resolve_requested_project(args.project_root, runtime);
    let project_root = resolve_project_root(&requested)?;
    let Some((receipt, actions)) = plan_skills_uninstall(&project_root)? else {
        let output = SkillsMutationOutput {
            schema: "wikitool.skills-mutation.v1",
            operation: "uninstall-project",
            status: "not_installed",
            project_root: normalize_path(&project_root),
            skills_root: None,
            skill_targets: Vec::new(),
            actions: Vec::new(),
        };
        return print_mutation(&output, args.format);
    };
    let output = SkillsMutationOutput {
        schema: "wikitool.skills-mutation.v1",
        operation: "uninstall-project",
        status: if args.dry_run { "planned" } else { "applied" },
        project_root: normalize_path(&project_root),
        skills_root: None,
        skill_targets: receipt.skill_targets.clone(),
        actions,
    };
    if !args.dry_run {
        apply_skills_uninstall(&project_root, &receipt)?;
    }
    print_mutation(&output, args.format)
}

fn resolve_skills_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let executable =
        std::env::current_exe().context("failed to resolve the Wikitool executable path")?;
    let parent = executable
        .parent()
        .context("Wikitool executable path has no parent directory")?;
    Ok(parent.join("skills"))
}

fn resolve_requested_project(explicit: Option<PathBuf>, runtime: &RuntimeOptions) -> PathBuf {
    explicit
        .or_else(|| runtime.project_root().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_targets(target: SkillTarget, project_root: &Path) -> Vec<&'static str> {
    match target {
        SkillTarget::Agents => vec!["agents"],
        SkillTarget::Claude => vec!["claude"],
        SkillTarget::Both => vec!["agents", "claude"],
        SkillTarget::Auto => {
            let agents = [".agents", ".codex", ".pi", ".omp", ".opencode"]
                .iter()
                .any(|marker| is_real_directory(&project_root.join(marker)))
                || ["AGENTS.md", "opencode.json", "opencode.jsonc"]
                    .iter()
                    .any(|marker| is_regular_file(&project_root.join(marker)));
            let claude = is_real_directory(&project_root.join(".claude"))
                || is_regular_file(&project_root.join("CLAUDE.md"));
            match (agents, claude) {
                (true, true) => vec!["agents", "claude"],
                (false, true) => vec!["claude"],
                _ => vec!["agents"],
            }
        }
    }
}

fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn is_real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn inspect_project(path: &Path, current_manifest_sha256: &str) -> Result<SkillsProjectStatus> {
    let root = resolve_project_root(path)?;
    let receipt = verify_skills_install(&root)?;
    Ok(SkillsProjectStatus {
        project_root: normalize_path(&root),
        receipt_path: normalize_path(root.join(SKILLS_INSTALL_RECEIPT)),
        installed: receipt.is_some(),
        wikitool_version: receipt.as_ref().map(|value| value.wikitool_version.clone()),
        skills_manifest_sha256: receipt
            .as_ref()
            .map(|value| value.skills_manifest_sha256.clone()),
        skills_aligned: receipt
            .as_ref()
            .map(|value| value.skills_manifest_sha256 == current_manifest_sha256),
        skill_targets: receipt
            .as_ref()
            .map(|value| value.skill_targets.clone())
            .unwrap_or_default(),
        managed_file_count: receipt
            .as_ref()
            .map(|value| value.managed_files.len())
            .unwrap_or_default(),
    })
}

fn print_inspect(output: &SkillsInspectOutput, format: OutputFormat) -> Result<()> {
    if format.is_json() {
        println!("{}", serde_json::to_string_pretty(output)?);
    } else {
        println!("skills: {}", output.status);
        println!("skills_root: {}", output.skills_root);
        println!("wikitool_version: {}", output.wikitool_version);
        println!("skills: {}", output.skills.join(", "));
        println!("files: {}", output.file_count);
        if let Some(project) = &output.project {
            println!("project_installed: {}", project.installed);
            if let Some(aligned) = project.skills_aligned {
                println!("project_skills_aligned: {aligned}");
            }
        }
    }
    Ok(())
}

fn print_mutation(output: &SkillsMutationOutput, format: OutputFormat) -> Result<()> {
    if format.is_json() {
        println!("{}", serde_json::to_string_pretty(output)?);
    } else {
        println!("skills {}: {}", output.operation, output.status);
        println!("project_root: {}", output.project_root);
        if !output.skill_targets.is_empty() {
            println!("skill_targets: {}", output.skill_targets.join(", "));
        }
        for action in &output.actions {
            println!("{}: {}", action.action, action.path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_defaults_to_agents_for_an_unmarked_project() {
        let root = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            resolve_targets(SkillTarget::Auto, root.path()),
            vec!["agents"]
        );
    }

    #[test]
    fn auto_selects_both_marked_harnesses() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("AGENTS.md"), "# Agents\n").expect("agents marker");
        std::fs::write(root.path().join("CLAUDE.md"), "# Claude\n").expect("claude marker");
        assert_eq!(
            resolve_targets(SkillTarget::Auto, root.path()),
            vec!["agents", "claude"]
        );
    }

    #[test]
    fn auto_recognizes_shared_harness_markers_beyond_agents() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(root.path().join(".codex")).expect("Codex marker");
        std::fs::write(root.path().join("CLAUDE.md"), "# Claude\n").expect("Claude marker");
        assert_eq!(
            resolve_targets(SkillTarget::Auto, root.path()),
            vec!["agents", "claude"]
        );
    }

    #[test]
    fn auto_ignores_markers_with_the_wrong_file_type() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join(".agents"), "not a directory\n")
            .expect("false directory marker");
        std::fs::create_dir(root.path().join("CLAUDE.md")).expect("false file marker");
        assert_eq!(
            resolve_targets(SkillTarget::Auto, root.path()),
            vec!["agents"]
        );
    }
}
