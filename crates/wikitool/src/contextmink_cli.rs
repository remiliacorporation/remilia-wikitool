use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::RuntimeOptions;
use crate::cli_support::{OutputFormat, normalize_path};

/// Configuration written into wikitool projects. Unlike a standalone contextmink
/// install into an arbitrary repository, the wikitool project layout is known, so
/// the excludes are generated instead of left as user homework. Excludes apply to
/// broad scans only; explicit paths (e.g. a draft under .wikitool/drafts/) bypass
/// them.
const WIKITOOL_PROJECT_CONFIG: &str = "profile = \"wikitool-project\"\n\nexclude_globs = [\n  \".wikitool/**\",\n  \"tools/contextmink/**\",\n]\n";

#[derive(Debug, Args)]
pub(crate) struct ContextminkArgs {
    #[command(subcommand)]
    command: ContextminkSubcommand,
}

#[derive(Debug, Subcommand)]
enum ContextminkSubcommand {
    #[command(
        about = "Install the bundled contextmink pack into the current directory or --project-root"
    )]
    Install(ContextminkInstallArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ContextminkInstallArgs {
    #[arg(
        long,
        value_name = "DIR",
        help = "Contextmink pack directory (default: the contextmink/ directory next to the wikitool binary)"
    )]
    from: Option<PathBuf>,
    #[arg(long, help = "Overwrite files that already exist in the project")]
    force: bool,
    #[arg(long, help = "Preview the install without writing files")]
    dry_run: bool,
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
struct ContextminkInstallReport {
    pack_dir: String,
    pack_version: String,
    project_root: String,
    dry_run: bool,
    actions: Vec<InstallAction>,
    /// Version reported by the installed binary; None on dry runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    installed_version: Option<String>,
    verified: bool,
    next_steps: Vec<String>,
}

#[derive(Debug, Serialize)]
struct InstallAction {
    source: String,
    target: String,
    status: String,
}

enum InstallSource {
    PackFile(PathBuf),
    Generated(&'static str),
}

struct PlannedInstall {
    source: InstallSource,
    target_relative: &'static str,
    executable: bool,
}

pub(crate) fn run_contextmink(runtime: &RuntimeOptions, args: ContextminkArgs) -> Result<()> {
    match args.command {
        ContextminkSubcommand::Install(args) => run_contextmink_install(runtime, args),
    }
}

fn run_contextmink_install(runtime: &RuntimeOptions, args: ContextminkInstallArgs) -> Result<()> {
    let project_root = resolve_install_project_root(runtime)?;
    let pack_dir = resolve_pack_dir(args.from.as_deref())?;
    let manifest = read_pack_manifest(&pack_dir)?;

    let binary_target: &'static str = if manifest.binary.ends_with(".exe") {
        "tools/contextmink/bin/contextmink.exe"
    } else {
        "tools/contextmink/bin/contextmink"
    };
    let mut planned = vec![PlannedInstall {
        source: InstallSource::PackFile(pack_dir.join(&manifest.binary)),
        target_relative: binary_target,
        executable: true,
    }];
    if let Some(bridge) = &manifest.bridge_binary {
        planned.push(PlannedInstall {
            source: InstallSource::PackFile(pack_dir.join(bridge)),
            target_relative: "tools/contextmink/bin/contextmink-bridge.exe",
            executable: true,
        });
    }
    planned.push(PlannedInstall {
        source: InstallSource::PackFile(pack_dir.join("templates/scripts/contextmink")),
        target_relative: "scripts/contextmink",
        executable: true,
    });
    planned.push(PlannedInstall {
        source: InstallSource::PackFile(pack_dir.join("templates/CLAUDE.contextmink.md")),
        target_relative: "tools/contextmink/templates/CLAUDE.contextmink.md",
        executable: false,
    });
    planned.push(PlannedInstall {
        source: InstallSource::PackFile(pack_dir.join("templates/AGENTS.contextmink.md")),
        target_relative: "tools/contextmink/templates/AGENTS.contextmink.md",
        executable: false,
    });
    planned.push(PlannedInstall {
        source: InstallSource::Generated(WIKITOOL_PROJECT_CONFIG),
        target_relative: ".contextmink.toml",
        executable: false,
    });

    for item in &planned {
        if let InstallSource::PackFile(source) = &item.source
            && !source.is_file()
        {
            bail!(
                "contextmink pack is missing {}; expected a release pack laid out per contextmink/SETUP.md",
                normalize_path(source)
            );
        }
    }

    let mut actions = Vec::new();
    for item in &planned {
        let target = project_root.join(item.target_relative);
        let status = if args.dry_run {
            if target.exists() && !args.force {
                "would_skip_exists"
            } else {
                "would_install"
            }
        } else if target.exists() && !args.force {
            "skipped_exists"
        } else {
            match &item.source {
                InstallSource::PackFile(source) => install_file(source, &target, item.executable)?,
                InstallSource::Generated(content) => install_generated(content, &target)?,
            }
            "installed"
        };
        actions.push(InstallAction {
            source: match &item.source {
                InstallSource::PackFile(source) => normalize_path(source),
                InstallSource::Generated(_) => "<generated wikitool-project config>".to_string(),
            },
            target: item.target_relative.to_string(),
            status: status.to_string(),
        });
    }

    let (installed_version, verified) = if args.dry_run {
        (None, false)
    } else {
        let version = verify_installed_binary(&project_root.join(binary_target))?;
        let verified = version == manifest.version;
        if !verified {
            bail!(
                "installed contextmink reports version {version} but the pack manifest says {}; the pack is inconsistent",
                manifest.version
            );
        }
        (Some(version), verified)
    };

    let next_steps = vec![
        "merge tools/contextmink/templates/CLAUDE.contextmink.md (Claude) or AGENTS.contextmink.md (Codex) into this project's agent guidance".to_string(),
        "verify from an agent shell: scripts/contextmink files --path . --max 20".to_string(),
    ];

    let report = ContextminkInstallReport {
        pack_dir: normalize_path(&pack_dir),
        pack_version: manifest.version,
        project_root: normalize_path(&project_root),
        dry_run: args.dry_run,
        actions,
        installed_version,
        verified,
        next_steps,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("contextmink install");
    println!("pack_dir: {}", report.pack_dir);
    println!("pack_version: {}", report.pack_version);
    println!("project_root: {}", report.project_root);
    println!("dry_run: {}", report.dry_run);
    for action in &report.actions {
        println!("{}: {}", action.status, action.target);
    }
    if let Some(version) = &report.installed_version {
        println!("installed_version: {version}");
        println!("verified: {}", report.verified);
    }
    for step in &report.next_steps {
        println!("next: {step}");
    }
    if runtime.diagnostics {
        let source = if runtime.project_root.is_some() {
            "flag"
        } else {
            "current-dir"
        };
        println!(
            "\n[diagnostics]\nproject_root={} ({source})",
            normalize_path(&project_root)
        );
    }
    Ok(())
}

struct PackManifest {
    version: String,
    binary: String,
    bridge_binary: Option<String>,
}

fn resolve_install_project_root(runtime: &RuntimeOptions) -> Result<PathBuf> {
    let cwd = env::current_dir().context("failed to resolve current directory")?;
    Ok(resolve_install_project_root_from_cwd(
        &cwd,
        runtime.project_root.as_deref(),
    ))
}

fn resolve_install_project_root_from_cwd(cwd: &Path, project_root: Option<&Path>) -> PathBuf {
    match project_root {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => cwd.join(path),
        None => cwd.to_path_buf(),
    }
}

fn resolve_pack_dir(from: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = from {
        if !dir.is_dir() {
            bail!("--from directory does not exist: {}", normalize_path(dir));
        }
        return Ok(dir.to_path_buf());
    }
    let exe = env::current_exe().context("failed to resolve the running wikitool binary path")?;
    let Some(exe_dir) = exe.parent() else {
        bail!("failed to resolve the directory containing the wikitool binary");
    };
    let sibling = exe_dir.join("contextmink");
    if sibling.is_dir() {
        return Ok(sibling);
    }
    bail!(
        "no contextmink pack found at {}; run this from an unpacked release bundle or pass --from <pack-dir>",
        normalize_path(&sibling)
    );
}

fn read_pack_manifest(pack_dir: &Path) -> Result<PackManifest> {
    let manifest_path = pack_dir.join("manifest.json");
    let text = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "missing contextmink pack manifest: {}",
            normalize_path(&manifest_path)
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("invalid JSON in {}", normalize_path(&manifest_path)))?;
    let name = value.get("name").and_then(serde_json::Value::as_str);
    if name != Some("contextmink") {
        bail!("pack manifest name is {name:?}, expected \"contextmink\"");
    }
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("pack manifest is missing the version field"))?
        .to_string();
    let binary = value
        .get("binary")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("pack manifest is missing the binary field"))?
        .to_string();
    let bridge_binary = value
        .get("bridge_binary")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Ok(PackManifest {
        version,
        binary,
        bridge_binary,
    })
}

fn install_file(source: &Path, target: &Path, executable: bool) -> Result<()> {
    ensure_target_parent(target)?;
    fs::copy(source, target).with_context(|| {
        format!(
            "failed to copy {} to {}",
            normalize_path(source),
            normalize_path(target)
        )
    })?;
    set_executable(target, executable)
}

fn install_generated(content: &str, target: &Path) -> Result<()> {
    ensure_target_parent(target)?;
    fs::write(target, content)
        .with_context(|| format!("failed to write {}", normalize_path(target)))
}

fn ensure_target_parent(target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(target: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if !executable {
        return Ok(());
    }
    let mut permissions = fs::metadata(target)
        .with_context(|| format!("failed to stat {}", normalize_path(target)))?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(target, permissions)
        .with_context(|| format!("failed to chmod {}", normalize_path(target)))
}

#[cfg(not(unix))]
fn set_executable(_target: &Path, _executable: bool) -> Result<()> {
    Ok(())
}

/// Run the installed binary and return the version it reports. The check runs the
/// project-local binary directly (not the bash launcher) so verification works
/// from any shell on any platform.
fn verify_installed_binary(binary: &Path) -> Result<String> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .with_context(|| {
            format!(
                "failed to run installed contextmink at {}",
                normalize_path(binary)
            )
        })?;
    if !output.status.success() {
        bail!(
            "installed contextmink at {} exited with {}",
            normalize_path(binary),
            output.status
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| {
            anyhow::anyhow!("unexpected --version output from installed contextmink: {stdout:?}")
        })?
        .to_string();
    Ok(version)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::resolve_install_project_root_from_cwd;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "wikitool-contextmink-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp test dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn install_root_defaults_to_cwd_even_under_existing_project_marker() {
        let temp = TestDir::new("marker-parent");
        let parent = temp.path.join("wiki");
        let cwd = parent.join("agent-workdir");
        fs::create_dir_all(parent.join(".wikitool")).expect("marker");
        fs::create_dir_all(&cwd).expect("cwd");

        assert_eq!(resolve_install_project_root_from_cwd(&cwd, None), cwd);
    }

    #[test]
    fn install_root_respects_relative_project_root_flag() {
        let temp = TestDir::new("relative-root");
        let cwd = temp.path.join("cwd");
        fs::create_dir_all(&cwd).expect("cwd");

        assert_eq!(
            resolve_install_project_root_from_cwd(&cwd, Some(Path::new("project"))),
            cwd.join("project")
        );
    }
}
