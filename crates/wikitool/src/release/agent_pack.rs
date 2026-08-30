use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::agent_pack::{
    AGENT_PACK_SCHEMA, AgentPackFile, AgentPackManifest, AgentPackSkill, PUBLIC_SKILL_IDS,
    load_agent_pack, sha256_bytes,
};
use crate::cli_support::{
    copy_file, normalize_path, reset_directory, resolve_repo_root, validate_release_output,
};

use super::ReleaseBuildAgentPackArgs;

#[derive(Debug)]
pub(super) struct AgentPackBuildResult {
    pub(super) output_dir: PathBuf,
    pub(super) manifest_sha256: String,
    pub(super) skill_count: usize,
    pub(super) file_count: usize,
}

pub(super) fn run_release_build_agent_pack(args: ReleaseBuildAgentPackArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root)?;
    let output_dir = args
        .output_dir
        .unwrap_or_else(|| repo_root.join("dist/agent"));
    let result = build_agent_pack(&repo_root, &output_dir)?;

    println!("release build-agent-pack");
    println!("repo_root: {}", normalize_path(&repo_root));
    print_agent_pack_build(&result);
    Ok(())
}

pub(super) fn print_agent_pack_build(result: &AgentPackBuildResult) {
    println!("agent_pack_dir: {}", normalize_path(&result.output_dir));
    println!("agent_pack_manifest_sha256: {}", result.manifest_sha256);
    println!("agent_pack_skills: {}", result.skill_count);
    println!("agent_pack_files: {}", result.file_count);
}

pub(super) fn build_agent_pack(
    repo_root: &Path,
    output_dir: &Path,
) -> Result<AgentPackBuildResult> {
    let source_root = repo_root.join("agent-pack");
    require_regular_file(&source_root.join("README.md"), "agent pack README")?;
    require_directory(
        &source_root.join("integration"),
        "agent pack integration directory",
    )?;
    require_directory(&source_root.join("skills"), "agent pack skills directory")?;

    validate_release_output(repo_root, output_dir, "agent pack output")?;
    reset_directory(output_dir)?;
    copy_regular_tree(&source_root, output_dir)?;

    let mut skills = Vec::with_capacity(PUBLIC_SKILL_IDS.len());
    for id in PUBLIC_SKILL_IDS {
        let skill_root = output_dir.join("skills").join(id);
        let entrypoint = skill_root.join("SKILL.md");
        let metadata = skill_root.join("agents/openai.yaml");
        require_regular_file(&entrypoint, "skill entrypoint")?;
        require_regular_file(&metadata, "skill metadata")?;
        skills.push(AgentPackSkill {
            id: id.to_string(),
            description: read_skill_description(&entrypoint)?,
            path: format!("skills/{id}"),
        });
    }

    let mut files = collect_regular_files(output_dir)?
        .into_iter()
        .filter(|path| path != Path::new("manifest.json"))
        .map(|relative| {
            let bytes = fs::read(output_dir.join(&relative)).with_context(|| {
                format!(
                    "failed to read {}",
                    normalize_path(output_dir.join(&relative))
                )
            })?;
            Ok(AgentPackFile {
                path: normalize_path(&relative),
                bytes: bytes.len() as u64,
                sha256: sha256_bytes(&bytes),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    skills.sort_by(|left, right| left.id.cmp(&right.id));

    let manifest = AgentPackManifest {
        schema: AGENT_PACK_SCHEMA.to_string(),
        wikitool_version: env!("CARGO_PKG_VERSION").to_string(),
        skills,
        files,
    };
    let manifest_path = output_dir.join("manifest.json");
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    wikitool_core::support::atomic_write(&manifest_path, &manifest_bytes)
        .with_context(|| format!("failed to write {}", normalize_path(&manifest_path)))?;

    let validated = load_agent_pack(output_dir)?;
    Ok(AgentPackBuildResult {
        output_dir: validated.root,
        manifest_sha256: validated.manifest_sha256,
        skill_count: validated.manifest.skills.len(),
        file_count: validated.manifest.files.len(),
    })
}

fn read_skill_description(path: &Path) -> Result<String> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read {} as UTF-8", normalize_path(path)))?;
    let mut lines = source.lines();
    if lines.next() != Some("---") {
        bail!(
            "skill entrypoint has no YAML frontmatter: {}",
            normalize_path(path)
        );
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(description) = line.strip_prefix("description:") {
            let description = description.trim();
            if !description.is_empty() {
                return Ok(description.to_string());
            }
            break;
        }
    }
    bail!(
        "skill entrypoint has no description: {}",
        normalize_path(path)
    )
}

fn copy_regular_tree(source_root: &Path, destination_root: &Path) -> Result<()> {
    for relative in collect_regular_files(source_root)? {
        if relative == Path::new("manifest.json") {
            bail!("agent pack source must not contain a generated manifest.json");
        }
        copy_file(
            &source_root.join(&relative),
            &destination_root.join(relative),
        )?;
    }
    Ok(())
}

fn collect_regular_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .with_context(|| format!("failed to read {}", normalize_path(&directory)))?
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("failed to inspect {}", normalize_path(&path)))?;
            if metadata.file_type().is_symlink() {
                bail!(
                    "agent pack source contains a symlink: {}",
                    normalize_path(&path)
                );
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                files.push(path.strip_prefix(root).expect("root prefix").to_path_buf());
            } else {
                bail!(
                    "unsupported agent pack source entry: {}",
                    normalize_path(&path)
                );
            }
        }
    }
    files.sort_by_key(|path| normalize_path(path));
    Ok(files)
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
    fn source_tree_build_is_deterministic() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let first = tempfile::tempdir().expect("first output");
        let second = tempfile::tempdir().expect("second output");
        let first_result = build_agent_pack(&repo_root, first.path()).expect("first build");
        let second_result = build_agent_pack(&repo_root, second.path()).expect("second build");
        assert_eq!(first_result.manifest_sha256, second_result.manifest_sha256);
        assert_eq!(first_result.file_count, second_result.file_count);
    }

    #[test]
    fn source_tree_cannot_be_selected_as_agent_pack_output() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let error = build_agent_pack(&repo_root, &repo_root.join("agent-pack"))
            .expect_err("source output must fail");
        assert!(error.to_string().contains("must remain under dist"));
    }
}
