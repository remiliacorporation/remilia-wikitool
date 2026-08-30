use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use crate::RuntimeOptions;
use crate::agent_pack::{
    AGENT_INSTALL_RECEIPT, AgentInstallAction, apply_agent_install, apply_agent_uninstall,
    load_agent_pack, plan_agent_install, plan_agent_uninstall, resolve_project_root,
    verify_agent_install,
};
use crate::cli_support::{OutputFormat, normalize_path};

#[derive(Debug, Args)]
pub(crate) struct AgentArgs {
    #[command(subcommand)]
    command: AgentSubcommand,
}

#[derive(Debug, Subcommand)]
enum AgentSubcommand {
    #[command(about = "Validate and describe a Wikitool agent pack")]
    Inspect(AgentInspectArgs),
    #[command(
        name = "setup-project",
        about = "Install Wikitool skills into an agent project"
    )]
    SetupProject(AgentSetupArgs),
    #[command(
        name = "uninstall-project",
        about = "Remove an unchanged receipt-owned Wikitool skill installation"
    )]
    UninstallProject(AgentUninstallArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum AgentTarget {
    Auto,
    Agents,
    Claude,
    Both,
}

#[derive(Debug, Args)]
struct AgentInspectArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Agent pack root (default: agent/ beside the executable)"
    )]
    pack_root: Option<PathBuf>,
    #[arg(
        value_name = "PROJECT",
        help = "Also inspect this project's install receipt"
    )]
    project_root: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json, value_name = "FORMAT")]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct AgentSetupArgs {
    #[arg(value_name = "PROJECT")]
    project_root: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Agent pack root (default: agent/ beside the executable)"
    )]
    pack_root: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = AgentTarget::Auto)]
    target: AgentTarget,
    #[arg(
        long,
        help = "Validate and print the exact plan without changing files"
    )]
    dry_run: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, value_name = "FORMAT")]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct AgentUninstallArgs {
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
struct AgentInspectOutput {
    schema: &'static str,
    status: &'static str,
    pack_root: String,
    wikitool_version: String,
    manifest_sha256: String,
    skills: Vec<String>,
    file_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<AgentProjectStatus>,
}

#[derive(Debug, Serialize)]
struct AgentProjectStatus {
    project_root: String,
    receipt_path: String,
    installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    wikitool_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pack_manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pack_aligned: Option<bool>,
    skill_targets: Vec<String>,
    managed_file_count: usize,
}

#[derive(Debug, Serialize)]
struct AgentMutationOutput {
    schema: &'static str,
    operation: &'static str,
    status: &'static str,
    project_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pack_root: Option<String>,
    skill_targets: Vec<String>,
    actions: Vec<AgentInstallAction>,
}

pub(crate) fn run_agent(runtime: &RuntimeOptions, args: AgentArgs) -> Result<()> {
    match args.command {
        AgentSubcommand::Inspect(options) => run_inspect(runtime, options),
        AgentSubcommand::SetupProject(options) => run_setup(runtime, options),
        AgentSubcommand::UninstallProject(options) => run_uninstall(runtime, options),
    }
}

fn run_inspect(runtime: &RuntimeOptions, args: AgentInspectArgs) -> Result<()> {
    let pack = load_agent_pack(&resolve_pack_root(args.pack_root)?)?;
    let project = args
        .project_root
        .or_else(|| runtime.project_root().map(Path::to_path_buf))
        .map(|path| inspect_project(&path, &pack.manifest_sha256))
        .transpose()?;
    let output = AgentInspectOutput {
        schema: "wikitool.agent-inspect.v1",
        status: "valid",
        pack_root: normalize_path(&pack.root),
        wikitool_version: pack.manifest.wikitool_version.clone(),
        manifest_sha256: pack.manifest_sha256,
        skills: pack
            .manifest
            .skills
            .iter()
            .map(|skill| skill.id.clone())
            .collect(),
        file_count: pack.manifest.files.len(),
        project,
    };
    print_inspect(&output, args.format)
}

fn run_setup(runtime: &RuntimeOptions, args: AgentSetupArgs) -> Result<()> {
    let requested = resolve_requested_project(args.project_root, runtime);
    let project_root = resolve_project_root(&requested)?;
    let pack = load_agent_pack(&resolve_pack_root(args.pack_root)?)?;
    let targets = resolve_targets(args.target, &project_root);
    let plan = plan_agent_install(&project_root, &pack, &targets)?;
    let output = AgentMutationOutput {
        schema: "wikitool.agent-mutation.v1",
        operation: "setup-project",
        status: if args.dry_run { "planned" } else { "applied" },
        project_root: normalize_path(&project_root),
        pack_root: Some(normalize_path(&pack.root)),
        skill_targets: targets.iter().map(|target| (*target).to_string()).collect(),
        actions: plan.actions.clone(),
    };
    if !args.dry_run {
        apply_agent_install(&project_root, plan)?;
    }
    print_mutation(&output, args.format)
}

fn run_uninstall(runtime: &RuntimeOptions, args: AgentUninstallArgs) -> Result<()> {
    let requested = resolve_requested_project(args.project_root, runtime);
    let project_root = resolve_project_root(&requested)?;
    let Some((receipt, actions)) = plan_agent_uninstall(&project_root)? else {
        let output = AgentMutationOutput {
            schema: "wikitool.agent-mutation.v1",
            operation: "uninstall-project",
            status: "not_installed",
            project_root: normalize_path(&project_root),
            pack_root: None,
            skill_targets: Vec::new(),
            actions: Vec::new(),
        };
        return print_mutation(&output, args.format);
    };
    let output = AgentMutationOutput {
        schema: "wikitool.agent-mutation.v1",
        operation: "uninstall-project",
        status: if args.dry_run { "planned" } else { "applied" },
        project_root: normalize_path(&project_root),
        pack_root: None,
        skill_targets: receipt.skill_targets.clone(),
        actions,
    };
    if !args.dry_run {
        apply_agent_uninstall(&project_root, &receipt)?;
    }
    print_mutation(&output, args.format)
}

fn resolve_pack_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let executable =
        std::env::current_exe().context("failed to resolve the Wikitool executable path")?;
    let parent = executable
        .parent()
        .context("Wikitool executable path has no parent directory")?;
    Ok(parent.join("agent"))
}

fn resolve_requested_project(explicit: Option<PathBuf>, runtime: &RuntimeOptions) -> PathBuf {
    explicit
        .or_else(|| runtime.project_root().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_targets(target: AgentTarget, project_root: &Path) -> Vec<&'static str> {
    match target {
        AgentTarget::Agents => vec!["agents"],
        AgentTarget::Claude => vec!["claude"],
        AgentTarget::Both => vec!["agents", "claude"],
        AgentTarget::Auto => {
            let agents =
                project_root.join("AGENTS.md").exists() || project_root.join(".agents").exists();
            let claude =
                project_root.join("CLAUDE.md").exists() || project_root.join(".claude").exists();
            match (agents, claude) {
                (true, true) => vec!["agents", "claude"],
                (false, true) => vec!["claude"],
                _ => vec!["agents"],
            }
        }
    }
}

fn inspect_project(path: &Path, current_manifest_sha256: &str) -> Result<AgentProjectStatus> {
    let root = resolve_project_root(path)?;
    let receipt = verify_agent_install(&root)?;
    Ok(AgentProjectStatus {
        project_root: normalize_path(&root),
        receipt_path: normalize_path(root.join(AGENT_INSTALL_RECEIPT)),
        installed: receipt.is_some(),
        wikitool_version: receipt.as_ref().map(|value| value.wikitool_version.clone()),
        pack_manifest_sha256: receipt
            .as_ref()
            .map(|value| value.pack_manifest_sha256.clone()),
        pack_aligned: receipt
            .as_ref()
            .map(|value| value.pack_manifest_sha256 == current_manifest_sha256),
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

fn print_inspect(output: &AgentInspectOutput, format: OutputFormat) -> Result<()> {
    if format.is_json() {
        println!("{}", serde_json::to_string_pretty(output)?);
    } else {
        println!("agent pack: {}", output.status);
        println!("pack_root: {}", output.pack_root);
        println!("wikitool_version: {}", output.wikitool_version);
        println!("skills: {}", output.skills.join(", "));
        println!("files: {}", output.file_count);
        if let Some(project) = &output.project {
            println!("project_installed: {}", project.installed);
            if let Some(aligned) = project.pack_aligned {
                println!("project_pack_aligned: {aligned}");
            }
        }
    }
    Ok(())
}

fn print_mutation(output: &AgentMutationOutput, format: OutputFormat) -> Result<()> {
    if format.is_json() {
        println!("{}", serde_json::to_string_pretty(output)?);
    } else {
        println!("agent {}: {}", output.operation, output.status);
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
            resolve_targets(AgentTarget::Auto, root.path()),
            vec!["agents"]
        );
    }

    #[test]
    fn auto_selects_both_marked_harnesses() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("AGENTS.md"), "# Agents\n").expect("agents marker");
        std::fs::write(root.path().join("CLAUDE.md"), "# Claude\n").expect("claude marker");
        assert_eq!(
            resolve_targets(AgentTarget::Auto, root.path()),
            vec!["agents", "claude"]
        );
    }
}
