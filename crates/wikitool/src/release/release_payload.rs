use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use wikitool_core::site::{
    parse_template_engineering_contract, render_template_scaffold, site_adapter_resource_paths,
};

use crate::cli_support::{copy_dir_recursive, copy_file, normalize_path};

const REMILIA_TEMPLATE_CONTRACT_RESOURCES: &[&str] = &[
    "template_contracts/README.md",
    "template_contracts/ambox.json",
    "template_contracts/claim-status.json",
    "template_contracts/infobox-subject.json",
    "template_contracts/preservation-audio.json",
    "template_contracts/preservation-image.json",
    "template_contracts/section-notice.json",
    "template_contracts/table-cell-status.json",
    "template_contracts/version-label.json",
    "template_contracts/version-range.json",
];

pub(super) fn stage_release_payload(
    repo_root: &Path,
    output_dir: &Path,
    host_project_root: Option<&Path>,
) -> Result<()> {
    for file in [
        ".env.template",
        "README.md",
        "LICENSE",
        "LICENSE-SSL",
        "LICENSE-VPL",
    ] {
        copy_required_file(
            &repo_root.join(file),
            &output_dir.join(file),
            "release file",
        )?;
    }

    let docs = repo_root.join("docs/wikitool");
    require_directory(&docs, "Wikitool documentation")?;
    copy_dir_recursive(&docs, &output_dir.join("docs/wikitool"))?;

    stage_generic_adapter(repo_root, output_dir)?;
    stage_remilia_adapter(repo_root, output_dir)?;
    if let Some(host_root) = host_project_root {
        stage_host_adapter(host_root, output_dir)?;
    }
    Ok(())
}

fn stage_generic_adapter(repo_root: &Path, output_dir: &Path) -> Result<()> {
    let source = repo_root.join("site_adapters/generic");
    let policy = source.join("site-adapter.toml");
    let resources =
        site_adapter_resource_paths(&policy).context("generic site-adapter template is invalid")?;
    if resources.len() != 1 {
        bail!("generic site-adapter template must be self-contained");
    }
    copy_adapter_resources(
        &source,
        &output_dir.join("site_adapters/generic"),
        &resources,
    )
}

fn stage_remilia_adapter(repo_root: &Path, output_dir: &Path) -> Result<()> {
    let source = repo_root.join("site_adapters/remilia-wiki");
    let policy = source.join("site-adapter.toml");
    let resources = site_adapter_resource_paths(&policy)
        .context("bundled Remilia Wiki site adapter is invalid")?;
    let destination = output_dir.join("site_adapters/remilia-wiki");
    copy_adapter_resources(&source, &destination, &resources)?;
    for relative in REMILIA_TEMPLATE_CONTRACT_RESOURCES {
        let source_file = source.join(relative);
        require_regular_file(&source_file, "bundled Remilia Wiki template contract")?;
        if source_file
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            validate_template_contract(&source_file)?;
        }
        copy_file(&source_file, &destination.join(relative))?;
    }
    Ok(())
}

fn stage_host_adapter(host_root: &Path, output_dir: &Path) -> Result<()> {
    let host_root = fs::canonicalize(host_root)
        .with_context(|| format!("failed to resolve {}", normalize_path(host_root)))?;
    let source = host_root.join("wikitool_adapter");
    require_directory(&source, "host wikitool_adapter")?;
    let policy = source.join("site-adapter.toml");
    let resources = site_adapter_resource_paths(&policy).context("host site adapter is invalid")?;
    copy_adapter_resources(
        &source,
        &output_dir.join("site_adapters/project"),
        &resources,
    )
}

fn copy_adapter_resources(root: &Path, destination: &Path, resources: &[PathBuf]) -> Result<()> {
    for source in resources {
        let relative = source
            .strip_prefix(root)
            .with_context(|| format!("site-adapter resource escaped {}", normalize_path(root)))?;
        copy_file(source, &destination.join(relative))?;
    }
    Ok(())
}

fn validate_template_contract(path: &Path) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {} as UTF-8", normalize_path(path)))?;
    let contract = parse_template_engineering_contract(&content)
        .with_context(|| format!("invalid template contract: {}", normalize_path(path)))?;
    render_template_scaffold(&contract)
        .with_context(|| format!("invalid template contract: {}", normalize_path(path)))?;
    Ok(())
}

fn copy_required_file(source: &Path, destination: &Path, kind: &str) -> Result<()> {
    require_regular_file(source, kind)?;
    copy_file(source, destination)
}

fn require_regular_file(path: &Path, kind: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("missing {kind}: {}", normalize_path(path)))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "{kind} must be a regular non-symlink file: {}",
            normalize_path(path)
        );
    }
    Ok(())
}

fn require_directory(path: &Path, kind: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("missing {kind}: {}", normalize_path(path)))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "{kind} must be a non-symlink directory: {}",
            normalize_path(path)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_payload_keeps_agent_files_in_their_own_stage() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = tempfile::tempdir().expect("output");
        stage_release_payload(&repo_root, output.path(), None).expect("stage payload");
        assert!(output.path().join("README.md").is_file());
        assert!(
            output
                .path()
                .join("site_adapters/generic/site-adapter.toml")
                .is_file()
        );
        assert!(
            output
                .path()
                .join("site_adapters/remilia-wiki/site-adapter.toml")
                .is_file()
        );
        assert!(!output.path().join("agent").exists());
        assert!(!output.path().join("AGENTS.md").exists());
        assert!(!output.path().join("CLAUDE.md").exists());
        assert!(!output.path().join(".claude").exists());
    }

    #[test]
    fn explicit_host_without_an_adapter_fails_packaging() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = tempfile::tempdir().expect("output");
        let host = tempfile::tempdir().expect("host");
        let error = stage_release_payload(&repo_root, output.path(), Some(host.path()))
            .expect_err("missing host adapter must fail");
        assert!(error.to_string().contains("host wikitool_adapter"));
    }
}
