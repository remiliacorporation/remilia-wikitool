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

const DEFAULT_RELEASE_MATRIX_TARGETS: [&str; 3] = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
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

fn release_bundle_name(target: &str, artifact_version: Option<&str>) -> String {
    match artifact_version {
        Some(version) => format!("wikitool-{version}-{target}"),
        None => format!("wikitool-{target}"),
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
        .arg("--no-default-features")
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
        .map(|value| value == "wikitool")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        release_binary_name_for_target, release_bundle_name, resolve_release_artifact_version,
        resolve_release_targets,
    };

    #[test]
    fn release_targets_default_and_deduped() {
        assert_eq!(
            resolve_release_targets(&[]),
            vec![
                "x86_64-pc-windows-msvc".to_string(),
                "x86_64-unknown-linux-gnu".to_string(),
                "x86_64-apple-darwin".to_string()
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
            release_bundle_name("x86_64-unknown-linux-gnu", Some("v0.1.0")),
            "wikitool-v0.1.0-x86_64-unknown-linux-gnu"
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
}
