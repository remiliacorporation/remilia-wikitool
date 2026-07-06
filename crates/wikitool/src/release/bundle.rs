use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result, bail};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::cli_support::{
    copy_dir_contents, copy_file, normalize_path, reset_directory, resolve_default_true_flag,
    resolve_repo_root,
};

use super::ai_pack::{build_ai_pack, print_ai_pack_build_flags};
use super::{ReleaseBuildMatrixArgs, ReleasePackageArgs};

pub(super) fn run_release_package(args: ReleasePackageArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root)?;
    let output_dir = args
        .output_dir
        .unwrap_or_else(|| repo_root.join("dist/release"));
    let binary_path = args.binary_path.unwrap_or_else(|| {
        repo_root
            .join("target/release")
            .join(default_release_binary_name())
    });
    if !binary_path.is_file() {
        bail!("missing release binary: {}", normalize_path(&binary_path));
    }

    let staging_dir = repo_root.join("dist/release-ai-pack-staging");
    let ai_pack_result =
        build_ai_pack(&repo_root, &staging_dir, args.host_project_root.as_deref())?;

    stage_release_bundle(
        &output_dir,
        &binary_path,
        default_release_binary_name(),
        &staging_dir,
    )?;
    stage_contextmink_pack(
        &repo_root,
        &output_dir,
        None,
        host_platform_slug(),
        args.contextmink_dist.as_deref(),
        &PathBuf::from("cargo"),
        true,
    )?;
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir)
            .with_context(|| format!("failed to remove {}", normalize_path(&staging_dir)))?;
    }

    println!("release package");
    println!("repo_root: {}", normalize_path(&repo_root));
    println!("binary_path: {}", normalize_path(&binary_path));
    println!("output_dir: {}", normalize_path(&output_dir));
    print_ai_pack_build_flags(&ai_pack_result);
    Ok(())
}

#[derive(Debug)]
struct ReleaseMatrixArtifact {
    target: String,
    binary_path: PathBuf,
    bundle_dir: PathBuf,
    zip_path: PathBuf,
}

pub(super) fn run_release_build_matrix(args: ReleaseBuildMatrixArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root)?;
    let output_dir = args
        .output_dir
        .unwrap_or_else(|| repo_root.join("dist/release-matrix"));
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", normalize_path(&output_dir)))?;

    let cargo_bin = args.cargo_bin.unwrap_or_else(|| PathBuf::from("cargo"));
    let use_locked = resolve_default_true_flag(
        args.locked,
        args.no_locked,
        "release build-matrix lockfile flag",
    )?;
    let targets = resolve_release_targets(&args.targets);
    let artifact_version =
        resolve_release_artifact_version(args.artifact_version.as_deref(), args.unversioned_names)?;

    let ai_pack_dir = output_dir.join("_ai-pack-staging");
    let ai_pack_result =
        build_ai_pack(&repo_root, &ai_pack_dir, args.host_project_root.as_deref())?;

    let mut artifacts = Vec::new();
    for target in &targets {
        if !args.skip_build {
            run_cargo_release_build_for_target(&repo_root, &cargo_bin, target, use_locked)?;
        }

        let binary_path = release_binary_path_for_target(&repo_root, target);
        if !binary_path.is_file() {
            bail!(
                "missing built binary for target {target}: {}",
                normalize_path(&binary_path)
            );
        }

        let bundle_name = release_bundle_name(target, artifact_version.as_deref());
        let bundle_dir = output_dir.join(&bundle_name);
        stage_release_bundle(
            &bundle_dir,
            &binary_path,
            release_binary_name_for_target(target),
            &ai_pack_dir,
        )?;
        stage_contextmink_pack(
            &repo_root,
            &bundle_dir,
            Some(target),
            release_platform_slug(target),
            args.contextmink_dist.as_deref(),
            &cargo_bin,
            use_locked,
        )?;

        let zip_path = output_dir.join(format!("{bundle_name}.zip"));
        zip_release_bundle(&bundle_dir, &zip_path, &bundle_name)?;

        artifacts.push(ReleaseMatrixArtifact {
            target: target.clone(),
            binary_path,
            bundle_dir,
            zip_path,
        });
    }

    if ai_pack_dir.exists() {
        fs::remove_dir_all(&ai_pack_dir)
            .with_context(|| format!("failed to remove {}", normalize_path(&ai_pack_dir)))?;
    }

    println!("release build-matrix");
    println!("repo_root: {}", normalize_path(&repo_root));
    println!("output_dir: {}", normalize_path(&output_dir));
    println!(
        "artifact_version: {}",
        artifact_version.as_deref().unwrap_or("<none>")
    );
    println!("target_count: {}", artifacts.len());
    print_ai_pack_build_flags(&ai_pack_result);
    for artifact in &artifacts {
        println!("artifact.target: {}", artifact.target);
        println!(
            "artifact.binary_path: {}",
            normalize_path(&artifact.binary_path)
        );
        println!(
            "artifact.bundle_dir: {}",
            normalize_path(&artifact.bundle_dir)
        );
        println!("artifact.zip_path: {}", normalize_path(&artifact.zip_path));
    }
    Ok(())
}

fn default_release_binary_name() -> &'static str {
    if cfg!(windows) {
        "wikitool.exe"
    } else {
        "wikitool"
    }
}

fn stage_release_bundle(
    output_dir: &Path,
    binary_path: &Path,
    bundle_binary_name: &str,
    ai_pack_dir: &Path,
) -> Result<()> {
    reset_directory(output_dir)?;
    copy_file(binary_path, &output_dir.join(bundle_binary_name))?;
    copy_dir_contents(ai_pack_dir, output_dir)?;
    Ok(())
}

const DEFAULT_RELEASE_MATRIX_TARGETS: [&str; 4] = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

fn resolve_release_targets(raw_targets: &[String]) -> Vec<String> {
    let mut targets = Vec::new();
    for raw in raw_targets {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !targets.iter().any(|existing| existing == trimmed) {
            targets.push(trimmed.to_string());
        }
    }
    if targets.is_empty() {
        return DEFAULT_RELEASE_MATRIX_TARGETS
            .iter()
            .map(|target| (*target).to_string())
            .collect();
    }
    targets
}

fn resolve_release_artifact_version(
    raw_label: Option<&str>,
    unversioned_names: bool,
) -> Result<Option<String>> {
    if unversioned_names {
        if raw_label.is_some() {
            bail!("cannot combine --artifact-version with --unversioned-names");
        }
        return Ok(None);
    }

    let label = raw_label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));

    if !label
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        bail!("invalid artifact version label `{label}`: allowed characters are [A-Za-z0-9._-]");
    }
    Ok(Some(label))
}

/// Friendly os-arch slug for release artifact names, e.g. `macos-arm64` instead of
/// the raw `aarch64-apple-darwin` target triple. Unknown triples fall back to the
/// triple so an unmapped target still produces a usable name.
fn release_platform_slug(target: &str) -> &str {
    match target {
        "x86_64-pc-windows-msvc" => "windows-x86_64",
        "x86_64-unknown-linux-gnu" => "linux-x86_64",
        "x86_64-apple-darwin" => "macos-x86_64",
        "aarch64-apple-darwin" => "macos-arm64",
        other => other,
    }
}

fn release_bundle_name(target: &str, artifact_version: Option<&str>) -> String {
    let platform = release_platform_slug(target);
    match artifact_version {
        Some(version) => format!("wikitool-{version}-{platform}"),
        None => format!("wikitool-{platform}"),
    }
}

fn run_cargo_release_build_for_target(
    repo_root: &Path,
    cargo_bin: &Path,
    target: &str,
    use_locked: bool,
) -> Result<()> {
    let mut command = ProcessCommand::new(cargo_bin);
    command
        .current_dir(repo_root)
        .arg("build")
        .arg("--package")
        .arg("wikitool")
        .arg("--release")
        .arg("--target")
        .arg(target);
    if use_locked {
        command.arg("--locked");
    }
    let status = command.status().with_context(|| {
        format!(
            "failed to execute {} for target {target}",
            normalize_path(cargo_bin)
        )
    })?;
    if !status.success() {
        bail!("cargo build failed for target {target}");
    }
    Ok(())
}

fn release_binary_name_for_target(target: &str) -> &'static str {
    if target.to_ascii_lowercase().contains("windows") {
        "wikitool.exe"
    } else {
        "wikitool"
    }
}

fn release_binary_path_for_target(repo_root: &Path, target: &str) -> PathBuf {
    repo_root
        .join("target")
        .join(target)
        .join("release")
        .join(release_binary_name_for_target(target))
}

/// Release bundles ship contextmink in a `contextmink/` subdirectory so
/// wikitool users get bounded-read tooling without a separate install. It
/// stays a separate binary: contextmink is project-generic, and agents must
/// not have to route bounded reads through wikitool. The default release path
/// builds the vendored `vendor/contextmink` source at the version pinned in
/// `config/contextmink.version`; `--contextmink-dist` is only an explicit
/// prebuilt-pack override.
fn read_contextmink_pin(repo_root: &Path) -> Result<String> {
    let path = repo_root.join("config/contextmink.version");
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", normalize_path(&path)))?;
    parse_contextmink_pin(&raw).with_context(|| {
        format!(
            "invalid contextmink version pin in {}",
            normalize_path(&path)
        )
    })
}

fn parse_contextmink_pin(raw: &str) -> Result<String> {
    let version = raw.trim();
    let is_semver = !version.is_empty()
        && version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()));
    if !is_semver {
        bail!("expected bare semver (x.y.z), got {raw:?}");
    }
    Ok(version.to_string())
}

fn host_platform_slug() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-arm64"
    } else if cfg!(target_os = "macos") {
        "macos-x86_64"
    } else {
        "linux-x86_64"
    }
}

fn stage_contextmink_pack(
    repo_root: &Path,
    bundle_dir: &Path,
    target: Option<&str>,
    platform_slug: &str,
    contextmink_dist: Option<&Path>,
    cargo_bin: &Path,
    use_locked: bool,
) -> Result<()> {
    if let Some(dist) = contextmink_dist {
        return stage_prebuilt_contextmink_pack(repo_root, bundle_dir, platform_slug, dist);
    }
    stage_vendored_contextmink_pack(
        repo_root,
        bundle_dir,
        target,
        platform_slug,
        cargo_bin,
        use_locked,
    )
}

fn stage_prebuilt_contextmink_pack(
    repo_root: &Path,
    bundle_dir: &Path,
    platform_slug: &str,
    dist: &Path,
) -> Result<()> {
    let pin = read_contextmink_pin(repo_root)?;
    let source = dist.join(platform_slug);
    let manifest_path = source.join("manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "missing prebuilt contextmink bundle for {platform_slug}: {}",
            normalize_path(&manifest_path)
        )
    })?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .with_context(|| format!("invalid JSON in {}", normalize_path(&manifest_path)))?;
    validate_contextmink_manifest(&manifest, &pin, platform_slug)?;
    for key in ["binary", "bridge_binary"] {
        if let Some(binary) = manifest.get(key).and_then(serde_json::Value::as_str) {
            let path = source.join(binary);
            if !path.is_file() {
                bail!(
                    "contextmink manifest names {key} {binary:?} but it is missing: {}",
                    normalize_path(&path)
                );
            }
        }
    }
    let pack_dir = bundle_dir.join("contextmink");
    reset_directory(&pack_dir)?;
    copy_dir_contents(&source, &pack_dir)?;
    Ok(())
}

fn stage_vendored_contextmink_pack(
    repo_root: &Path,
    bundle_dir: &Path,
    target: Option<&str>,
    platform_slug: &str,
    cargo_bin: &Path,
    use_locked: bool,
) -> Result<()> {
    let pin = read_contextmink_pin(repo_root)?;
    let source_root = repo_root.join("vendor/contextmink");
    let manifest_path = source_root.join("Cargo.toml");
    if !manifest_path.is_file() {
        bail!(
            "vendored contextmink source is missing: {}",
            normalize_path(&manifest_path)
        );
    }
    let source_version = read_contextmink_source_version(cargo_bin, &manifest_path)?;
    if source_version != pin {
        bail!(
            "vendored contextmink version {source_version:?} does not match the pin {pin} in config/contextmink.version"
        );
    }

    run_cargo_contextmink_build_for_target(&source_root, cargo_bin, target, use_locked)?;

    let (binary_name, bridge_name) = expected_contextmink_pack_layout(platform_slug)?;
    let binary_path = contextmink_build_binary_path(&source_root, target, binary_name);
    if !binary_path.is_file() {
        bail!(
            "missing built contextmink binary for {platform_slug}: {}",
            normalize_path(&binary_path)
        );
    }

    let pack_dir = bundle_dir.join("contextmink");
    reset_directory(&pack_dir)?;
    copy_file(&binary_path, &pack_dir.join(binary_name))?;
    if let Some(bridge) = bridge_name {
        let bridge_path = contextmink_build_binary_path(&source_root, target, bridge);
        if !bridge_path.is_file() {
            bail!(
                "missing built contextmink bridge for {platform_slug}: {}",
                normalize_path(&bridge_path)
            );
        }
        copy_file(&bridge_path, &pack_dir.join(bridge))?;
    }

    for file in [
        "README.md",
        "SETUP.md",
        "LICENSE",
        "LICENSE-SSL",
        "LICENSE-VPL",
    ] {
        copy_file(&source_root.join(file), &pack_dir.join(file))?;
    }
    copy_dir_contents(&source_root.join("docs"), &pack_dir.join("docs"))?;
    copy_dir_contents(&source_root.join("templates"), &pack_dir.join("templates"))?;

    let mut manifest = serde_json::json!({
        "name": "contextmink",
        "version": source_version,
        "target": target.unwrap_or("host"),
        "platform": platform_slug,
        "binary": binary_name,
        "archive": "vendored-source"
    });
    if let Some(bridge) = bridge_name {
        manifest["bridge_binary"] = serde_json::Value::String(bridge.to_string());
    }
    validate_contextmink_manifest(&manifest, &pin, platform_slug)?;
    fs::write(
        pack_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )
    .with_context(|| {
        format!(
            "failed to write {}",
            normalize_path(pack_dir.join("manifest.json"))
        )
    })?;
    Ok(())
}

fn read_contextmink_source_version(cargo_bin: &Path, manifest_path: &Path) -> Result<String> {
    let output = ProcessCommand::new(cargo_bin)
        .arg("pkgid")
        .arg("--manifest-path")
        .arg(manifest_path)
        .output()
        .with_context(|| {
            format!(
                "failed to execute {} pkgid for vendored contextmink",
                normalize_path(cargo_bin)
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo pkgid failed for vendored contextmink: {stderr}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    contextmink_version_from_pkgid(&stdout)
}

fn contextmink_version_from_pkgid(raw: &str) -> Result<String> {
    let pkgid = raw.trim();
    let Some(index) = pkgid.rfind('@').or_else(|| pkgid.rfind('#')) else {
        bail!("unexpected cargo pkgid for vendored contextmink: {pkgid:?}");
    };
    let version = &pkgid[index + 1..];
    parse_contextmink_pin(version)
}

fn run_cargo_contextmink_build_for_target(
    source_root: &Path,
    cargo_bin: &Path,
    target: Option<&str>,
    use_locked: bool,
) -> Result<()> {
    let manifest_path = source_root.join("Cargo.toml");
    let mut command = ProcessCommand::new(cargo_bin);
    command
        .current_dir(source_root)
        .arg("build")
        .arg("--release")
        .arg("--bins")
        .arg("--manifest-path")
        .arg(&manifest_path);
    if let Some(target) = target {
        command.arg("--target").arg(target);
    }
    if use_locked {
        command.arg("--locked");
    }
    let status = command.status().with_context(|| {
        format!(
            "failed to execute {} for vendored contextmink",
            normalize_path(cargo_bin)
        )
    })?;
    if !status.success() {
        match target {
            Some(target) => bail!("cargo build failed for vendored contextmink target {target}"),
            None => bail!("cargo build failed for vendored contextmink"),
        }
    }
    Ok(())
}

fn contextmink_build_binary_path(
    source_root: &Path,
    target: Option<&str>,
    binary_name: &str,
) -> PathBuf {
    let mut path = source_root.join("target");
    if let Some(target) = target {
        path = path.join(target);
    }
    path.join("release").join(binary_name)
}

fn validate_contextmink_manifest(
    manifest: &serde_json::Value,
    pin: &str,
    platform_slug: &str,
) -> Result<()> {
    let name = manifest.get("name").and_then(serde_json::Value::as_str);
    if name != Some("contextmink") {
        bail!("contextmink manifest name is {name:?}, expected \"contextmink\"");
    }
    let version = manifest.get("version").and_then(serde_json::Value::as_str);
    if version != Some(pin) {
        bail!(
            "contextmink bundle version {version:?} does not match the pin {pin} in config/contextmink.version"
        );
    }
    let platform = manifest.get("platform").and_then(serde_json::Value::as_str);
    if platform != Some(platform_slug) {
        bail!("contextmink bundle platform {platform:?} does not match requested {platform_slug}");
    }
    let binary = manifest
        .get("binary")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("contextmink manifest is missing the binary field"))?;
    let (expected_binary, expected_bridge) = expected_contextmink_pack_layout(platform_slug)?;
    if binary != expected_binary {
        bail!(
            "contextmink manifest binary {binary:?} does not match expected {expected_binary:?} for {platform_slug}"
        );
    }
    let bridge = manifest
        .get("bridge_binary")
        .and_then(serde_json::Value::as_str);
    match expected_bridge {
        Some(expected) if bridge != Some(expected) => {
            bail!(
                "contextmink manifest bridge_binary {bridge:?} does not match expected {expected:?} for {platform_slug}"
            );
        }
        None if bridge.is_some() => {
            bail!("contextmink manifest unexpectedly includes bridge_binary for {platform_slug}");
        }
        _ => {}
    }
    Ok(())
}

fn expected_contextmink_pack_layout(
    platform_slug: &str,
) -> Result<(&'static str, Option<&'static str>)> {
    match platform_slug {
        "windows-x86_64" => Ok(("contextmink.exe", Some("contextmink-bridge.exe"))),
        "linux-x86_64" | "macos-x86_64" | "macos-arm64" => Ok(("contextmink", None)),
        other => bail!("unsupported contextmink platform slug {other:?}"),
    }
}

fn zip_release_bundle(source_dir: &Path, zip_path: &Path, bundle_name: &str) -> Result<()> {
    if !source_dir.is_dir() {
        bail!("directory not found: {}", normalize_path(source_dir));
    }
    if let Some(parent) = zip_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }

    let zip_file = fs::File::create(zip_path)
        .with_context(|| format!("failed to create {}", normalize_path(zip_path)))?;
    let mut zip_writer = ZipWriter::new(zip_file);
    let dir_options = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);
    zip_writer
        .add_directory(format!("{bundle_name}/"), dir_options)
        .with_context(|| format!("failed to create zip root in {}", normalize_path(zip_path)))?;

    for relative_path in collect_relative_file_paths(source_dir)? {
        let source_path = source_dir.join(&relative_path);
        let normalized_relative = normalize_path(&relative_path);
        let entry_name = format!("{bundle_name}/{normalized_relative}");
        let mode = if is_release_binary_entry(&relative_path) {
            0o755
        } else {
            0o644
        };
        let file_options = FileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(mode);
        zip_writer
            .start_file(&entry_name, file_options)
            .with_context(|| {
                format!(
                    "failed to create zip entry {} in {}",
                    entry_name,
                    normalize_path(zip_path)
                )
            })?;
        let mut input = fs::File::open(&source_path)
            .with_context(|| format!("failed to open {}", normalize_path(&source_path)))?;
        io::copy(&mut input, &mut zip_writer).with_context(|| {
            format!(
                "failed to write zip entry {} in {}",
                entry_name,
                normalize_path(zip_path)
            )
        })?;
    }

    zip_writer
        .finish()
        .with_context(|| format!("failed to finalize {}", normalize_path(zip_path)))?;
    Ok(())
}

fn collect_relative_file_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_relative_file_paths_recursive(root, root, &mut files)?;
    files.sort_by_key(|path| normalize_path(path));
    Ok(files)
}

fn collect_relative_file_paths_recursive(
    root: &Path,
    current: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(current)
        .with_context(|| format!("failed to read {}", normalize_path(current)))?
    {
        entries.push(entry?);
    }
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to read metadata {}", normalize_path(&path)))?;
        if metadata.is_dir() {
            collect_relative_file_paths_recursive(root, &path, output)?;
        } else if metadata.is_file() {
            let relative = path.strip_prefix(root).with_context(|| {
                format!(
                    "failed to derive relative path from {} using root {}",
                    normalize_path(&path),
                    normalize_path(root)
                )
            })?;
            output.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn is_release_binary_entry(relative_path: &Path) -> bool {
    relative_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| matches!(value, "wikitool" | "contextmink" | "contextmink-bridge"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        contextmink_version_from_pkgid, host_platform_slug, is_release_binary_entry,
        parse_contextmink_pin, release_binary_name_for_target, release_bundle_name,
        release_platform_slug, resolve_release_artifact_version, resolve_release_targets,
        validate_contextmink_manifest,
    };

    #[test]
    fn release_targets_default_and_deduped() {
        assert_eq!(
            resolve_release_targets(&[]),
            vec![
                "x86_64-pc-windows-msvc".to_string(),
                "x86_64-unknown-linux-gnu".to_string(),
                "x86_64-apple-darwin".to_string(),
                "aarch64-apple-darwin".to_string()
            ]
        );
        assert_eq!(
            resolve_release_targets(&[
                "x86_64-unknown-linux-gnu".to_string(),
                " x86_64-unknown-linux-gnu ".to_string(),
                "aarch64-apple-darwin".to_string(),
            ]),
            vec![
                "x86_64-unknown-linux-gnu".to_string(),
                "aarch64-apple-darwin".to_string()
            ]
        );
    }

    #[test]
    fn release_artifact_version_validates_flags_and_characters() {
        assert_eq!(
            resolve_release_artifact_version(Some("v1.2.3"), false).expect("version"),
            Some("v1.2.3".to_string())
        );
        assert_eq!(
            resolve_release_artifact_version(None, true).expect("unversioned"),
            None
        );
        assert!(resolve_release_artifact_version(Some("bad label!"), false).is_err());
        assert!(resolve_release_artifact_version(Some("v1"), true).is_err());
    }

    #[test]
    fn release_bundle_and_binary_names_are_platform_aware() {
        assert_eq!(
            release_bundle_name("x86_64-unknown-linux-gnu", Some("0.1.0")),
            "wikitool-0.1.0-linux-x86_64"
        );
        assert_eq!(
            release_bundle_name("aarch64-apple-darwin", Some("0.3.1")),
            "wikitool-0.3.1-macos-arm64"
        );
        assert_eq!(
            release_binary_name_for_target("x86_64-pc-windows-msvc"),
            "wikitool.exe"
        );
        assert_eq!(
            release_binary_name_for_target("x86_64-unknown-linux-gnu"),
            "wikitool"
        );
    }

    #[test]
    fn contextmink_pin_and_manifest_validation_fail_fast() {
        assert_eq!(parse_contextmink_pin(" 0.3.0\n").unwrap(), "0.3.0");
        assert!(parse_contextmink_pin("").is_err());
        assert!(parse_contextmink_pin("v0.3.0").is_err());
        assert!(parse_contextmink_pin("0.3").is_err());
        assert_eq!(
            contextmink_version_from_pkgid("path+file:///repo/vendor/contextmink#0.6.0\n").unwrap(),
            "0.6.0"
        );

        let manifest: serde_json::Value = serde_json::json!({
            "name": "contextmink",
            "version": "0.3.0",
            "platform": "windows-x86_64",
            "binary": "contextmink.exe",
            "bridge_binary": "contextmink-bridge.exe",
        });
        assert!(validate_contextmink_manifest(&manifest, "0.3.0", "windows-x86_64").is_ok());
        assert!(validate_contextmink_manifest(&manifest, "0.4.0", "windows-x86_64").is_err());
        assert!(validate_contextmink_manifest(&manifest, "0.3.0", "linux-x86_64").is_err());
        let linux_manifest: serde_json::Value = serde_json::json!({
            "name": "contextmink",
            "version": "0.3.0",
            "platform": "linux-x86_64",
            "binary": "contextmink",
        });
        assert!(validate_contextmink_manifest(&linux_manifest, "0.3.0", "linux-x86_64").is_ok());
        let linux_with_bridge: serde_json::Value = serde_json::json!({
            "name": "contextmink",
            "version": "0.3.0",
            "platform": "linux-x86_64",
            "binary": "contextmink",
            "bridge_binary": "contextmink-bridge.exe",
        });
        assert!(
            validate_contextmink_manifest(&linux_with_bridge, "0.3.0", "linux-x86_64").is_err()
        );
        let windows_without_bridge: serde_json::Value = serde_json::json!({
            "name": "contextmink",
            "version": "0.3.0",
            "platform": "windows-x86_64",
            "binary": "contextmink.exe",
        });
        assert!(
            validate_contextmink_manifest(&windows_without_bridge, "0.3.0", "windows-x86_64")
                .is_err()
        );
        let wrong_binary: serde_json::Value = serde_json::json!({
            "name": "contextmink",
            "version": "0.3.0",
            "platform": "linux-x86_64",
            "binary": "contextmink.exe",
        });
        assert!(validate_contextmink_manifest(&wrong_binary, "0.3.0", "linux-x86_64").is_err());

        assert!(!host_platform_slug().is_empty());
        for binary in ["wikitool", "contextmink", "contextmink-bridge"] {
            assert!(is_release_binary_entry(std::path::Path::new(binary)));
        }
        assert!(!is_release_binary_entry(std::path::Path::new(
            "contextmink/README.md"
        )));
    }

    #[test]
    fn release_platform_slug_maps_known_triples_and_falls_back() {
        assert_eq!(
            release_platform_slug("x86_64-pc-windows-msvc"),
            "windows-x86_64"
        );
        assert_eq!(release_platform_slug("x86_64-apple-darwin"), "macos-x86_64");
        assert_eq!(release_platform_slug("aarch64-apple-darwin"), "macos-arm64");
        assert_eq!(
            release_platform_slug("riscv64gc-unknown-linux-gnu"),
            "riscv64gc-unknown-linux-gnu"
        );
    }
}
