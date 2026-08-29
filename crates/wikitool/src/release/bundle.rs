use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
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
        host_platform_slug(),
        args.contextmink_dist.as_deref(),
    )?;
    stage_papertiger_pack(
        &repo_root,
        &output_dir,
        host_platform_slug(),
        args.papertiger_dist.as_deref(),
    )?;
    write_release_companion_manifest(&repo_root, &output_dir, host_platform_slug())?;
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
            release_platform_slug(target),
            args.contextmink_dist.as_deref(),
        )?;
        stage_papertiger_pack(
            &repo_root,
            &bundle_dir,
            release_platform_slug(target),
            args.papertiger_dist.as_deref(),
        )?;
        write_release_companion_manifest(&repo_root, &bundle_dir, release_platform_slug(target))?;

        let zip_path = output_dir.join(format!("{bundle_name}.zip"));
        zip_release_bundle(&bundle_dir, &zip_path, &bundle_name)?;

        artifacts.push(ReleaseMatrixArtifact {
            target: target.clone(),
            binary_path,
            bundle_dir,
            zip_path,
        });
    }

    let checksums_path = output_dir.join("SHA256SUMS.txt");
    write_release_checksums(&artifacts, &checksums_path)?;

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
    println!("checksums_path: {}", normalize_path(&checksums_path));
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
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let label = match label.strip_prefix('v') {
        Some(stripped)
            if stripped
                .chars()
                .next()
                .map(|ch| ch.is_ascii_digit())
                .unwrap_or(false) =>
        {
            stripped.to_string()
        }
        _ => label,
    };

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

/// External release packs retain their own version authority. Wikitool reads
/// repository-owned pins solely to validate and compose a release bundle.
fn read_release_pin(repo_root: &Path, product: &str, relative_path: &str) -> Result<String> {
    let path = repo_root.join(relative_path);
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", normalize_path(&path)))?;
    parse_release_semver_pin(&raw).with_context(|| {
        format!(
            "invalid {} version pin in {}",
            product.to_ascii_lowercase(),
            normalize_path(&path)
        )
    })
}

fn parse_release_semver_pin(raw: &str) -> Result<String> {
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

/// Contextmink ships under `contextmink/` and owns its project setup lifecycle.
fn stage_contextmink_pack(
    repo_root: &Path,
    bundle_dir: &Path,
    platform_slug: &str,
    contextmink_dist: Option<&Path>,
) -> Result<()> {
    let dist = contextmink_dist
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repo_root.join("dist/contextmink-dist"));
    if !dist.is_dir() {
        bail!(
            "Contextmink distribution root is missing: {}. Stage the pinned upstream pack with scripts/fetch_contextmink.sh --platform {platform_slug}",
            normalize_path(&dist)
        );
    }
    stage_prebuilt_contextmink_pack(repo_root, bundle_dir, platform_slug, &dist)
}

fn stage_prebuilt_contextmink_pack(
    repo_root: &Path,
    bundle_dir: &Path,
    platform_slug: &str,
    dist: &Path,
) -> Result<()> {
    let pin = read_release_pin(repo_root, "Contextmink", "config/contextmink.version")?;
    let source_commit =
        read_release_source_commit(repo_root, "Contextmink", "config/contextmink.source-commit")?;
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
    validate_contextmink_manifest(&manifest, &pin, &source_commit, platform_slug)?;
    validate_release_archive_receipt(
        repo_root,
        &source,
        &manifest,
        "Contextmink",
        "config/contextmink-sha256s.txt",
        "scripts/fetch_contextmink.sh",
    )?;
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

fn validate_release_archive_receipt(
    repo_root: &Path,
    source: &Path,
    manifest: &serde_json::Value,
    product: &str,
    pins_relative_path: &str,
    fetch_script: &str,
) -> Result<()> {
    let archive = manifest
        .get("archive")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{product} manifest is missing the archive field"))?;
    let pins_path = repo_root.join(pins_relative_path);
    let pins = fs::read_to_string(&pins_path)
        .with_context(|| format!("failed to read {}", normalize_path(&pins_path)))?;
    let expected = release_archive_hash_from_pins(&pins, archive).with_context(|| {
        format!(
            "no repository-pinned {product} archive hash for {archive:?} in {}",
            normalize_path(&pins_path)
        )
    })?;

    let receipt_path = source.join("archive.sha256");
    let receipt = fs::read_to_string(&receipt_path).with_context(|| {
        format!(
            "missing {product} archive verification receipt: {}. Restage with {fetch_script}",
            normalize_path(&receipt_path)
        )
    })?;
    let mut fields = receipt.split_whitespace();
    let actual_hash = fields.next();
    let actual_archive = fields.next();
    if fields.next().is_some()
        || actual_hash != Some(expected.as_str())
        || actual_archive != Some(archive)
    {
        bail!(
            "{product} archive receipt in {} does not match the repository pin for {archive}",
            normalize_path(&receipt_path)
        );
    }
    Ok(())
}

fn release_archive_hash_from_pins(raw: &str, archive: &str) -> Result<String> {
    let mut found = None;
    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let mut fields = line.split_whitespace();
        let hash = fields.next();
        let name = fields.next();
        if fields.next().is_some() || hash.is_none() || name.is_none() {
            bail!("invalid release SHA-256 pin line: {line:?}");
        }
        let hash = hash.expect("checked above");
        if hash.len() != 64 || !hash.chars().all(|ch| ch.is_ascii_hexdigit()) {
            bail!("invalid release SHA-256 value in pin line: {line:?}");
        }
        if name == Some(archive) {
            if found.is_some() {
                bail!("duplicate release SHA-256 pin for {archive:?}");
            }
            found = Some(hash.to_ascii_lowercase());
        }
    }
    found.ok_or_else(|| anyhow::anyhow!("archive is not pinned"))
}

fn validate_contextmink_manifest(
    manifest: &serde_json::Value,
    pin: &str,
    expected_source_commit: &str,
    platform_slug: &str,
) -> Result<()> {
    let schema = manifest.get("schema").and_then(serde_json::Value::as_str);
    if schema != Some("contextmink.release-manifest.v1") {
        bail!(
            "contextmink manifest schema is {schema:?}, expected contextmink.release-manifest.v1"
        );
    }
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
    let source_commit = manifest
        .get("source_commit")
        .and_then(serde_json::Value::as_str);
    if source_commit != Some(expected_source_commit) {
        bail!(
            "contextmink bundle source commit {source_commit:?} does not match the pin {expected_source_commit} in config/contextmink.source-commit"
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

/// Release bundles carry Papertiger as an optional companion under
/// `papertiger/`. Wikitool verifies and transports the upstream release pack,
/// but Papertiger alone owns project setup, task authority, upgrades, Mise,
/// and uninstall behavior.
fn stage_papertiger_pack(
    repo_root: &Path,
    bundle_dir: &Path,
    platform_slug: &str,
    papertiger_dist: Option<&Path>,
) -> Result<()> {
    let dist = papertiger_dist
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repo_root.join("dist/papertiger-dist"));
    if !dist.is_dir() {
        bail!(
            "Papertiger distribution root is missing: {}. Stage the pinned upstream pack with scripts/fetch_papertiger.sh --platform {platform_slug}",
            normalize_path(&dist)
        );
    }
    stage_prebuilt_papertiger_pack(repo_root, bundle_dir, platform_slug, &dist)
}

fn stage_prebuilt_papertiger_pack(
    repo_root: &Path,
    bundle_dir: &Path,
    platform_slug: &str,
    dist: &Path,
) -> Result<()> {
    let pin = read_release_pin(repo_root, "Papertiger", "config/papertiger.version")?;
    let source_commit =
        read_release_source_commit(repo_root, "Papertiger", "config/papertiger.source-commit")?;
    let source = dist.join(platform_slug);
    let manifest_path = source.join("manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "missing prebuilt papertiger bundle for {platform_slug}: {}",
            normalize_path(&manifest_path)
        )
    })?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .with_context(|| format!("invalid JSON in {}", normalize_path(&manifest_path)))?;
    validate_papertiger_manifest(&manifest, &pin, &source_commit, platform_slug)?;
    validate_release_archive_receipt(
        repo_root,
        &source,
        &manifest,
        "Papertiger",
        "config/papertiger-sha256s.txt",
        "scripts/fetch_papertiger.sh",
    )?;

    let binaries = manifest
        .get("binaries")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("papertiger manifest is missing the binaries array"))?;
    for binary in binaries {
        let binary = binary
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("papertiger manifest contains a non-string binary"))?;
        let path = source.join(binary);
        if !path.is_file() {
            bail!(
                "papertiger manifest names binary {binary:?} but it is missing: {}",
                normalize_path(&path)
            );
        }
    }
    for required in [
        "agent_integration.md",
        "README.md",
        "CHANGELOG.md",
        "MISE.md",
        "LICENSE",
        "LICENSE-SSL",
        "LICENSE-VPL",
    ] {
        let path = source.join(required);
        if !path.is_file() {
            bail!(
                "papertiger release pack is missing required file {required:?}: {}",
                normalize_path(&path)
            );
        }
    }

    let pack_dir = bundle_dir.join("papertiger");
    reset_directory(&pack_dir)?;
    copy_dir_contents(&source, &pack_dir)?;
    Ok(())
}

fn validate_papertiger_manifest(
    manifest: &serde_json::Value,
    pin: &str,
    expected_source_commit: &str,
    platform_slug: &str,
) -> Result<()> {
    let schema = manifest.get("schema").and_then(serde_json::Value::as_str);
    if schema != Some("papertiger.release-manifest.v1") {
        bail!(
            "papertiger manifest schema is {schema:?}, expected \"papertiger.release-manifest.v1\""
        );
    }
    let name = manifest.get("name").and_then(serde_json::Value::as_str);
    if name != Some("papertiger") {
        bail!("papertiger manifest name is {name:?}, expected \"papertiger\"");
    }
    let version = manifest.get("version").and_then(serde_json::Value::as_str);
    if version != Some(pin) {
        bail!(
            "papertiger bundle version {version:?} does not match the pin {pin} in config/papertiger.version"
        );
    }
    let source_commit = manifest
        .get("source_commit")
        .and_then(serde_json::Value::as_str);
    if source_commit != Some(expected_source_commit) {
        bail!(
            "papertiger bundle source commit {source_commit:?} does not match the pin {expected_source_commit} in config/papertiger.source-commit"
        );
    }
    let platform = manifest.get("platform").and_then(serde_json::Value::as_str);
    if platform != Some(platform_slug) {
        bail!("papertiger bundle platform {platform:?} does not match requested {platform_slug}");
    }
    let expected_archive = papertiger_archive_name(pin, platform_slug)?;
    let archive = manifest.get("archive").and_then(serde_json::Value::as_str);
    if archive != Some(expected_archive.as_str()) {
        bail!(
            "papertiger manifest archive {archive:?} does not match expected {expected_archive:?}"
        );
    }
    let expected_binaries = expected_papertiger_pack_layout(platform_slug)?;
    let binaries = manifest
        .get("binaries")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("papertiger manifest is missing the binaries array"))?;
    let actual_binaries = binaries
        .iter()
        .map(|value| value.as_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow::anyhow!("papertiger manifest contains a non-string binary"))?;
    if actual_binaries != expected_binaries {
        bail!(
            "papertiger manifest binaries {actual_binaries:?} do not match expected {expected_binaries:?} for {platform_slug}"
        );
    }
    let setup_installs_mise = manifest
        .get("planner_setup_installs_mise")
        .and_then(serde_json::Value::as_bool);
    if setup_installs_mise != Some(false) {
        bail!("papertiger manifest must declare planner_setup_installs_mise=false");
    }
    Ok(())
}

fn read_release_source_commit(
    repo_root: &Path,
    product: &str,
    relative_path: &str,
) -> Result<String> {
    let path = repo_root.join(relative_path);
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", normalize_path(&path)))?;
    let source_commit = raw.trim().to_string();
    if source_commit.len() != 40
        || !source_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("{product} source commit pin must be exactly 40 lowercase hexadecimal characters");
    }
    Ok(source_commit)
}

fn papertiger_archive_name(pin: &str, platform_slug: &str) -> Result<String> {
    expected_papertiger_pack_layout(platform_slug)?;
    let extension = if platform_slug == "windows-x86_64" {
        "zip"
    } else {
        "tar.gz"
    };
    Ok(format!("papertiger-{pin}-{platform_slug}.{extension}"))
}

fn expected_papertiger_pack_layout(platform_slug: &str) -> Result<Vec<&'static str>> {
    match platform_slug {
        "windows-x86_64" => Ok(vec!["papertiger.exe", "papertiger-mise.exe"]),
        "linux-x86_64" | "macos-x86_64" | "macos-arm64" => {
            Ok(vec!["papertiger", "papertiger-mise"])
        }
        other => bail!("unsupported papertiger platform slug {other:?}"),
    }
}

fn write_release_companion_manifest(
    repo_root: &Path,
    bundle_dir: &Path,
    platform_slug: &str,
) -> Result<()> {
    let contextmink_version =
        read_release_pin(repo_root, "Contextmink", "config/contextmink.version")?;
    let contextmink_source_commit =
        read_release_source_commit(repo_root, "Contextmink", "config/contextmink.source-commit")?;
    let papertiger_version =
        read_release_pin(repo_root, "Papertiger", "config/papertiger.version")?;
    let papertiger_source_commit =
        read_release_source_commit(repo_root, "Papertiger", "config/papertiger.source-commit")?;
    let (contextmink_binary, _) = expected_contextmink_pack_layout(platform_slug)?;
    let papertiger_binaries = expected_papertiger_pack_layout(platform_slug)?;
    let manifest = serde_json::json!({
        "schema": "wikitool.release-companions.v1",
        "companions": [
            {
                "id": "contextmink",
                "version": contextmink_version,
                "source_commit": contextmink_source_commit,
                "directory": "contextmink",
                "binary": format!("contextmink/{contextmink_binary}"),
                "manifest": "contextmink/manifest.json",
                "required_for_wikitool": false,
                "project_lifecycle_owner": "contextmink"
            },
            {
                "id": "papertiger",
                "version": papertiger_version,
                "source_commit": papertiger_source_commit,
                "directory": "papertiger",
                "planner_binary": format!("papertiger/{}", papertiger_binaries[0]),
                "mise_binary": format!("papertiger/{}", papertiger_binaries[1]),
                "manifest": "papertiger/manifest.json",
                "agent_contract": "papertiger/agent_integration.md",
                "required_for_wikitool": false,
                "project_lifecycle_owner": "papertiger",
                "setup_initializes_task_authority": false
            }
        ]
    });
    let path = bundle_dir.join("release-companions.json");
    fs::write(&path, serde_json::to_string_pretty(&manifest)? + "\n")
        .with_context(|| format!("failed to write {}", normalize_path(&path)))?;
    Ok(())
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

fn write_release_checksums(artifacts: &[ReleaseMatrixArtifact], output_path: &Path) -> Result<()> {
    let mut lines = Vec::new();
    for artifact in artifacts {
        let file_name = artifact
            .zip_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "release zip path has no UTF-8 file name: {}",
                    normalize_path(&artifact.zip_path)
                )
            })?;
        let digest = sha256_file(&artifact.zip_path)?;
        lines.push(format!("{digest}  {file_name}"));
    }
    lines.sort();
    let mut output = lines.join("\n");
    output.push('\n');
    fs::write(output_path, output)
        .with_context(|| format!("failed to write {}", normalize_path(output_path)))?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open {}", normalize_path(path)))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", normalize_path(path)))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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
        .map(|value| {
            matches!(
                value,
                "wikitool"
                    | "contextmink"
                    | "contextmink-bridge"
                    | "papertiger"
                    | "papertiger-mise"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        host_platform_slug, is_release_binary_entry, papertiger_archive_name,
        parse_release_semver_pin, release_archive_hash_from_pins, release_binary_name_for_target,
        release_bundle_name, release_platform_slug, resolve_release_artifact_version,
        resolve_release_targets, sha256_file, validate_contextmink_manifest,
        validate_papertiger_manifest, write_release_companion_manifest,
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
            Some("1.2.3".to_string())
        );
        assert_eq!(
            resolve_release_artifact_version(None, false).expect("version"),
            Some(env!("CARGO_PKG_VERSION").to_string())
        );
        assert_eq!(
            resolve_release_artifact_version(Some("vendor-smoke"), false).expect("version"),
            Some("vendor-smoke".to_string())
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
        assert_eq!(parse_release_semver_pin(" 0.3.0\n").unwrap(), "0.3.0");
        assert!(parse_release_semver_pin("").is_err());
        assert!(parse_release_semver_pin("v0.3.0").is_err());
        assert!(parse_release_semver_pin("0.3").is_err());
        let archive = "contextmink-0.9.0-windows-x86_64.zip";
        let pins = format!(
            "{}  {}\n{}  other.zip\n",
            "56e1bde757ede27439cdb2971ef60a06298ef1d4d02c986364f4c0e20dc8465c",
            archive,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            release_archive_hash_from_pins(&pins, archive).unwrap(),
            "56e1bde757ede27439cdb2971ef60a06298ef1d4d02c986364f4c0e20dc8465c"
        );
        assert!(release_archive_hash_from_pins(&pins, "missing.zip").is_err());
        assert!(release_archive_hash_from_pins("bad hash line\n", archive).is_err());
        let source_commit = "97c24a117e576c794d48fae7c34b4d334f511c13";
        let manifest: serde_json::Value = serde_json::json!({
            "schema": "contextmink.release-manifest.v1",
            "name": "contextmink",
            "version": "0.3.0",
            "source_commit": source_commit,
            "platform": "windows-x86_64",
            "binary": "contextmink.exe",
            "bridge_binary": "contextmink-bridge.exe",
        });
        assert!(
            validate_contextmink_manifest(&manifest, "0.3.0", source_commit, "windows-x86_64")
                .is_ok()
        );
        assert!(
            validate_contextmink_manifest(&manifest, "0.4.0", source_commit, "windows-x86_64")
                .is_err()
        );
        assert!(
            validate_contextmink_manifest(&manifest, "0.3.0", source_commit, "linux-x86_64")
                .is_err()
        );
        let mut wrong_schema = manifest.clone();
        wrong_schema["schema"] = serde_json::json!("contextmink.release-manifest.v2");
        assert!(
            validate_contextmink_manifest(&wrong_schema, "0.3.0", source_commit, "windows-x86_64")
                .is_err()
        );
        let mut wrong_source = manifest.clone();
        wrong_source["source_commit"] =
            serde_json::json!("0000000000000000000000000000000000000000");
        assert!(
            validate_contextmink_manifest(&wrong_source, "0.3.0", source_commit, "windows-x86_64")
                .is_err()
        );
        let linux_manifest: serde_json::Value = serde_json::json!({
            "schema": "contextmink.release-manifest.v1",
            "name": "contextmink",
            "version": "0.3.0",
            "source_commit": source_commit,
            "platform": "linux-x86_64",
            "binary": "contextmink",
        });
        assert!(
            validate_contextmink_manifest(&linux_manifest, "0.3.0", source_commit, "linux-x86_64")
                .is_ok()
        );
        let linux_with_bridge: serde_json::Value = serde_json::json!({
            "schema": "contextmink.release-manifest.v1",
            "name": "contextmink",
            "version": "0.3.0",
            "source_commit": source_commit,
            "platform": "linux-x86_64",
            "binary": "contextmink",
            "bridge_binary": "contextmink-bridge.exe",
        });
        assert!(
            validate_contextmink_manifest(
                &linux_with_bridge,
                "0.3.0",
                source_commit,
                "linux-x86_64"
            )
            .is_err()
        );
        let windows_without_bridge: serde_json::Value = serde_json::json!({
            "schema": "contextmink.release-manifest.v1",
            "name": "contextmink",
            "version": "0.3.0",
            "source_commit": source_commit,
            "platform": "windows-x86_64",
            "binary": "contextmink.exe",
        });
        assert!(
            validate_contextmink_manifest(
                &windows_without_bridge,
                "0.3.0",
                source_commit,
                "windows-x86_64"
            )
            .is_err()
        );
        let wrong_binary: serde_json::Value = serde_json::json!({
            "schema": "contextmink.release-manifest.v1",
            "name": "contextmink",
            "version": "0.3.0",
            "source_commit": source_commit,
            "platform": "linux-x86_64",
            "binary": "contextmink.exe",
        });
        assert!(
            validate_contextmink_manifest(&wrong_binary, "0.3.0", source_commit, "linux-x86_64")
                .is_err()
        );

        assert!(!host_platform_slug().is_empty());
        for binary in [
            "wikitool",
            "contextmink",
            "contextmink-bridge",
            "papertiger",
            "papertiger-mise",
        ] {
            assert!(is_release_binary_entry(std::path::Path::new(binary)));
        }
        assert!(!is_release_binary_entry(std::path::Path::new(
            "contextmink/README.md"
        )));
    }

    #[test]
    fn papertiger_pin_and_manifest_validation_fail_fast() {
        let manifest: serde_json::Value = serde_json::json!({
            "schema": "papertiger.release-manifest.v1",
            "name": "papertiger",
            "version": "0.9.0",
            "source_commit": "3f2a1ef6f40ad01ca9b07d44b28b10d7a3276af0",
            "target": "x86_64-pc-windows-msvc",
            "platform": "windows-x86_64",
            "archive": "papertiger-0.9.0-windows-x86_64.zip",
            "binaries": ["papertiger.exe", "papertiger-mise.exe"],
            "planner_setup_installs_mise": false,
        });
        let source_commit = "3f2a1ef6f40ad01ca9b07d44b28b10d7a3276af0";
        assert!(
            validate_papertiger_manifest(&manifest, "0.9.0", source_commit, "windows-x86_64")
                .is_ok()
        );
        assert!(
            validate_papertiger_manifest(&manifest, "0.8.0", source_commit, "windows-x86_64")
                .is_err()
        );
        assert!(
            validate_papertiger_manifest(&manifest, "0.9.0", source_commit, "linux-x86_64")
                .is_err()
        );

        let mut wrong_schema = manifest.clone();
        wrong_schema["schema"] = serde_json::json!("papertiger.release-manifest.v2");
        assert!(
            validate_papertiger_manifest(&wrong_schema, "0.9.0", source_commit, "windows-x86_64")
                .is_err()
        );
        let mut wrong_source = manifest.clone();
        wrong_source["source_commit"] =
            serde_json::json!("0000000000000000000000000000000000000000");
        assert!(
            validate_papertiger_manifest(&wrong_source, "0.9.0", source_commit, "windows-x86_64")
                .is_err()
        );
        let mut setup_installs_mise = manifest.clone();
        setup_installs_mise["planner_setup_installs_mise"] = serde_json::json!(true);
        assert!(
            validate_papertiger_manifest(
                &setup_installs_mise,
                "0.9.0",
                source_commit,
                "windows-x86_64"
            )
            .is_err()
        );
        assert_eq!(
            papertiger_archive_name("0.9.0", "macos-arm64").unwrap(),
            "papertiger-0.9.0-macos-arm64.tar.gz"
        );
        assert!(papertiger_archive_name("0.9.0", "freebsd-x86_64").is_err());
    }

    #[test]
    fn release_companion_manifest_exposes_optional_authority_boundaries() {
        let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = tempfile::tempdir().expect("companion manifest tempdir");
        write_release_companion_manifest(&repo_root, output.path(), "windows-x86_64")
            .expect("write companion manifest");
        let body = std::fs::read_to_string(output.path().join("release-companions.json"))
            .expect("read companion manifest");
        let manifest: serde_json::Value =
            serde_json::from_str(&body).expect("parse companion JSON");
        assert_eq!(
            manifest["schema"],
            serde_json::json!("wikitool.release-companions.v1")
        );
        assert_eq!(
            manifest["companions"][0]["required_for_wikitool"],
            serde_json::json!(false)
        );
        assert_eq!(
            manifest["companions"][0]["source_commit"],
            serde_json::json!("97c24a117e576c794d48fae7c34b4d334f511c13")
        );
        assert_eq!(
            manifest["companions"][1]["planner_binary"],
            serde_json::json!("papertiger/papertiger.exe")
        );
        assert_eq!(
            manifest["companions"][1]["source_commit"],
            serde_json::json!("1b7a04fb639bfff7ddfa19d85eea41236c303bb1")
        );
        assert_eq!(
            manifest["companions"][1]["setup_initializes_task_authority"],
            serde_json::json!(false)
        );
    }

    #[test]
    fn release_sha256_file_uses_standard_hex_digest() {
        let path = std::env::temp_dir().join(format!(
            "wikitool-release-sha256-test-{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, b"abc").expect("write temp file");
        let digest = sha256_file(&path).expect("hash temp file");
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
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
