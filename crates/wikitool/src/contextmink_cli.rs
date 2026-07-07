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
        about = "Install bundled or source-built contextmink into the current directory or --project-root"
    )]
    Install(ContextminkInstallArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ContextminkInstallArgs {
    #[arg(
        long,
        value_name = "DIR",
        help = "Contextmink release pack or source checkout (default: sibling contextmink/ pack, then vendored source)"
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
    source_kind: String,
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
    SourceBinary(PathBuf),
    Generated(&'static str),
}

struct PlannedInstall {
    source: InstallSource,
    target_relative: &'static str,
    executable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextminkSourceKind {
    ReleasePack,
    SourceCheckout,
}

impl ContextminkSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReleasePack => "release_pack",
            Self::SourceCheckout => "source_checkout",
        }
    }
}

struct ResolvedContextminkSource {
    dir: PathBuf,
    kind: ContextminkSourceKind,
    manifest: PackManifest,
}

impl ResolvedContextminkSource {
    fn file_source(&self, relative: &str) -> InstallSource {
        let path = match self.kind {
            ContextminkSourceKind::ReleasePack => self.dir.join(relative),
            ContextminkSourceKind::SourceCheckout => self.dir.join(relative),
        };
        InstallSource::PackFile(path)
    }

    fn binary_source(&self, binary: &str) -> InstallSource {
        let path = match self.kind {
            ContextminkSourceKind::ReleasePack => self.dir.join(binary),
            ContextminkSourceKind::SourceCheckout => {
                self.dir.join("target").join("release").join(binary)
            }
        };
        match self.kind {
            ContextminkSourceKind::ReleasePack => InstallSource::PackFile(path),
            ContextminkSourceKind::SourceCheckout => InstallSource::SourceBinary(path),
        }
    }
}

pub(crate) fn run_contextmink(runtime: &RuntimeOptions, args: ContextminkArgs) -> Result<()> {
    match args.command {
        ContextminkSubcommand::Install(args) => run_contextmink_install(runtime, args),
    }
}

fn run_contextmink_install(runtime: &RuntimeOptions, args: ContextminkInstallArgs) -> Result<()> {
    let project_root = resolve_install_project_root(runtime)?;
    let source = resolve_contextmink_source(args.from.as_deref())?;
    let manifest = &source.manifest;

    let binary_target: &'static str = if manifest.binary.ends_with(".exe") {
        "tools/contextmink/bin/contextmink.exe"
    } else {
        "tools/contextmink/bin/contextmink"
    };
    let mut planned = vec![PlannedInstall {
        source: source.binary_source(&manifest.binary),
        target_relative: binary_target,
        executable: true,
    }];
    if let Some(bridge) = &manifest.bridge_binary {
        planned.push(PlannedInstall {
            source: source.binary_source(bridge),
            target_relative: "tools/contextmink/bin/contextmink-bridge.exe",
            executable: true,
        });
    }
    planned.push(PlannedInstall {
        source: source.file_source("templates/scripts/contextmink"),
        target_relative: "scripts/contextmink",
        executable: true,
    });
    planned.push(PlannedInstall {
        source: source.file_source("templates/CLAUDE.contextmink.md"),
        target_relative: "tools/contextmink/templates/CLAUDE.contextmink.md",
        executable: false,
    });
    planned.push(PlannedInstall {
        source: source.file_source("templates/AGENTS.contextmink.md"),
        target_relative: "tools/contextmink/templates/AGENTS.contextmink.md",
        executable: false,
    });
    planned.push(PlannedInstall {
        source: InstallSource::Generated(WIKITOOL_PROJECT_CONFIG),
        target_relative: ".contextmink.toml",
        executable: false,
    });

    if source.kind == ContextminkSourceKind::SourceCheckout && !args.dry_run {
        ensure_source_binaries(&source.dir, manifest)?;
    }

    for item in &planned {
        if let Some(source_path) = required_source_path(&item.source)
            && !source_path.is_file()
            && !(args.dry_run && source.kind == ContextminkSourceKind::SourceCheckout)
        {
            bail!(
                "contextmink install source is missing {}; expected a release pack laid out per contextmink/SETUP.md or a built contextmink source checkout",
                normalize_path(source_path)
            );
        }
    }

    let mut actions = Vec::new();
    for item in &planned {
        let target = project_root.join(item.target_relative);
        let source_missing = required_source_path(&item.source).is_some_and(|path| !path.is_file());
        let status = if args.dry_run {
            if target.exists() && !args.force {
                "would_skip_exists"
            } else if source.kind == ContextminkSourceKind::SourceCheckout && source_missing {
                "would_build_then_install"
            } else {
                "would_install"
            }
        } else if target.exists() && !args.force {
            "skipped_exists"
        } else {
            match &item.source {
                InstallSource::PackFile(source) => install_file(source, &target, item.executable)?,
                InstallSource::SourceBinary(source) => {
                    install_file(source, &target, item.executable)?
                }
                InstallSource::Generated(content) => install_generated(content, &target)?,
            }
            "installed"
        };
        actions.push(InstallAction {
            source: match &item.source {
                InstallSource::PackFile(source) => normalize_path(source),
                InstallSource::SourceBinary(source) => normalize_path(source),
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
        "verify from an agent shell: use `scripts/contextmink files --path . --max 20` from Bash-hosted sessions; from Windows PowerShell use `tools\\contextmink\\bin\\contextmink.exe files --path . --max 20` for the native binary or `tools\\contextmink\\bin\\contextmink-bridge.exe --script scripts/contextmink files --path . --max 20` for the Bash launcher".to_string(),
    ];

    let report = ContextminkInstallReport {
        pack_dir: normalize_path(&source.dir),
        source_kind: source.kind.as_str().to_string(),
        pack_version: manifest.version.clone(),
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
    println!("source_kind: {}", report.source_kind);
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

#[derive(Debug)]
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

fn resolve_contextmink_source(from: Option<&Path>) -> Result<ResolvedContextminkSource> {
    if let Some(dir) = from {
        if !dir.is_dir() {
            bail!("--from directory does not exist: {}", normalize_path(dir));
        }
        return resolve_explicit_contextmink_source(dir);
    }

    let exe = env::current_exe().context("failed to resolve the running wikitool binary path")?;
    let Some(exe_dir) = exe.parent() else {
        bail!("failed to resolve the directory containing the wikitool binary");
    };
    let sibling = exe_dir.join("contextmink");
    if sibling.is_dir() {
        return Ok(ResolvedContextminkSource {
            manifest: read_pack_manifest(&sibling)?,
            dir: sibling,
            kind: ContextminkSourceKind::ReleasePack,
        });
    }
    if let Some(source_dir) = find_vendored_contextmink_source(&exe) {
        return Ok(ResolvedContextminkSource {
            manifest: read_source_manifest(&source_dir)?,
            dir: source_dir,
            kind: ContextminkSourceKind::SourceCheckout,
        });
    }
    bail!(
        "no contextmink pack found at {} and no vendored contextmink source checkout found near {}; run from an unpacked release bundle, pass --from <pack-dir>, or build from a wikitool source checkout",
        normalize_path(&sibling),
        normalize_path(&exe)
    );
}

fn resolve_explicit_contextmink_source(dir: &Path) -> Result<ResolvedContextminkSource> {
    if dir.join("manifest.json").is_file() {
        return Ok(ResolvedContextminkSource {
            manifest: read_pack_manifest(dir)?,
            dir: dir.to_path_buf(),
            kind: ContextminkSourceKind::ReleasePack,
        });
    }
    if is_contextmink_source_checkout(dir) {
        return Ok(ResolvedContextminkSource {
            manifest: read_source_manifest(dir)?,
            dir: dir.to_path_buf(),
            kind: ContextminkSourceKind::SourceCheckout,
        });
    }
    bail!(
        "--from must point at a contextmink release pack with manifest.json or a contextmink source checkout with Cargo.toml: {}",
        normalize_path(dir)
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

fn read_source_manifest(source_dir: &Path) -> Result<PackManifest> {
    let cargo_toml_path = source_dir.join("Cargo.toml");
    let text = fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("failed to read {}", normalize_path(&cargo_toml_path)))?;
    let name = parse_package_field(&text, "name")
        .ok_or_else(|| anyhow::anyhow!("contextmink source Cargo.toml is missing package.name"))?;
    if name != "contextmink" {
        bail!("source Cargo.toml package.name is {name:?}, expected \"contextmink\"");
    }
    let version = parse_package_field(&text, "version").ok_or_else(|| {
        anyhow::anyhow!("contextmink source Cargo.toml is missing package.version")
    })?;
    let suffix = env::consts::EXE_SUFFIX;
    Ok(PackManifest {
        version,
        binary: format!("contextmink{suffix}"),
        bridge_binary: if cfg!(windows) {
            Some(format!("contextmink-bridge{suffix}"))
        } else {
            None
        },
    })
}

fn parse_package_field(text: &str, field: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != field {
            continue;
        }
        let value = value.trim().trim_matches('"');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn find_vendored_contextmink_source(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        let candidate = ancestor.join("vendor").join("contextmink");
        if is_contextmink_source_checkout(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_contextmink_source_checkout(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file()
        && dir
            .join("templates")
            .join("scripts")
            .join("contextmink")
            .is_file()
}

fn ensure_source_binaries(source_dir: &Path, manifest: &PackManifest) -> Result<()> {
    let mut required = vec![manifest.binary.as_str()];
    if let Some(bridge) = manifest.bridge_binary.as_deref() {
        required.push(bridge);
    }
    let missing = required.iter().any(|binary| {
        !source_dir
            .join("target")
            .join("release")
            .join(binary)
            .is_file()
    });
    if !missing {
        return Ok(());
    }

    let cargo = env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"));
    let status = Command::new(&cargo)
        .args(["build", "--release", "--bins", "--manifest-path"])
        .arg(source_dir.join("Cargo.toml"))
        .status()
        .with_context(|| format!("failed to execute {}", normalize_path(&cargo)))?;
    if !status.success() {
        bail!(
            "cargo build failed for contextmink source checkout at {}",
            normalize_path(source_dir)
        );
    }

    for binary in required {
        let path = source_dir.join("target").join("release").join(binary);
        if !path.is_file() {
            bail!(
                "contextmink source build did not produce {}",
                normalize_path(&path)
            );
        }
    }
    Ok(())
}

fn required_source_path(source: &InstallSource) -> Option<&Path> {
    match source {
        InstallSource::PackFile(path) | InstallSource::SourceBinary(path) => Some(path.as_path()),
        InstallSource::Generated(_) => None,
    }
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

    use super::{
        ContextminkSourceKind, parse_package_field, read_source_manifest,
        resolve_explicit_contextmink_source, resolve_install_project_root_from_cwd,
    };

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

    #[test]
    fn package_field_parser_reads_package_section_only() {
        let text = r#"
[workspace.package]
version = "9.9.9"

[package]
name = "contextmink"
version = "0.6.0"
"#;

        assert_eq!(
            parse_package_field(text, "name"),
            Some("contextmink".to_string())
        );
        assert_eq!(
            parse_package_field(text, "version"),
            Some("0.6.0".to_string())
        );
    }

    #[test]
    fn explicit_from_accepts_contextmink_source_checkout() {
        let temp = TestDir::new("contextmink-source");
        fs::write(
            temp.path.join("Cargo.toml"),
            "[package]\nname = \"contextmink\"\nversion = \"0.6.0\"\n",
        )
        .expect("Cargo.toml");
        fs::create_dir_all(temp.path.join("templates/scripts")).expect("templates");
        fs::write(temp.path.join("templates/scripts/contextmink"), "").expect("launcher");

        let source = resolve_explicit_contextmink_source(&temp.path).expect("source checkout");
        assert_eq!(source.kind, ContextminkSourceKind::SourceCheckout);
        assert_eq!(source.manifest.version, "0.6.0");
        assert_eq!(
            source.manifest.binary,
            format!("contextmink{}", std::env::consts::EXE_SUFFIX)
        );
    }

    #[test]
    fn source_manifest_rejects_wrong_crate_name() {
        let temp = TestDir::new("wrong-source");
        fs::write(
            temp.path.join("Cargo.toml"),
            "[package]\nname = \"other\"\nversion = \"0.6.0\"\n",
        )
        .expect("Cargo.toml");

        let error = read_source_manifest(&temp.path).expect_err("wrong crate name");
        assert!(error.to_string().contains("expected \"contextmink\""));
    }
}
