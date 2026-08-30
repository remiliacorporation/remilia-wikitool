use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cli_support::normalize_path;

pub(crate) const SKILLS_MANIFEST_SCHEMA: &str = "wikitool.skills-manifest.v1";
pub(crate) const SKILLS_INSTALL_SCHEMA: &str = "wikitool.skills-install.v1";
pub(crate) const SKILLS_INSTALL_RECEIPT: &str = ".wikitool-skills/project-install.json";
pub(crate) const PUBLIC_SKILL_IDS: [&str; 4] =
    ["prose-review", "wiki-interview", "wiki-writing", "wikitool"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SkillsManifest {
    pub(crate) schema: String,
    pub(crate) wikitool_version: String,
    pub(crate) skills: Vec<SkillManifestEntry>,
    pub(crate) files: Vec<SkillManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SkillManifestEntry {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SkillManifestFile {
    pub(crate) path: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Debug)]
pub(crate) struct ValidatedSkills {
    pub(crate) root: PathBuf,
    pub(crate) manifest: SkillsManifest,
    pub(crate) manifest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SkillsInstallReceipt {
    pub(crate) schema: String,
    pub(crate) wikitool_version: String,
    pub(crate) skills_manifest_sha256: String,
    pub(crate) skill_targets: Vec<String>,
    pub(crate) managed_files: Vec<ManagedSkillFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ManagedSkillFile {
    pub(crate) path: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct SkillInstallAction {
    pub(crate) action: &'static str,
    pub(crate) path: String,
}

#[derive(Debug)]
pub(crate) struct DesiredSkillsInstall {
    pub(crate) receipt: SkillsInstallReceipt,
    pub(crate) writes: Vec<(PathBuf, Vec<u8>)>,
    pub(crate) removals: Vec<PathBuf>,
    pub(crate) actions: Vec<SkillInstallAction>,
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn load_skills(root: &Path) -> Result<ValidatedSkills> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("failed to resolve skills root {}", normalize_path(root)))?;
    if !root.is_dir() {
        bail!("skills root is not a directory: {}", normalize_path(&root));
    }
    let manifest_path = root.join("manifest.json");
    require_regular_file(&manifest_path, "skills manifest")?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", normalize_path(&manifest_path)))?;
    let manifest: SkillsManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("invalid JSON in {}", normalize_path(&manifest_path)))?;
    validate_skills_manifest(&manifest)?;

    let declared = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let actual = collect_regular_files(&root)?
        .into_iter()
        .filter(|path| path != Path::new("manifest.json"))
        .map(|path| normalize_path(&path))
        .collect::<BTreeSet<_>>();
    if actual != declared {
        let missing = declared.difference(&actual).cloned().collect::<Vec<_>>();
        let extra = actual.difference(&declared).cloned().collect::<Vec<_>>();
        bail!("skills file inventory mismatch; missing={missing:?}; extra={extra:?}");
    }

    for file in &manifest.files {
        let path = root.join(&file.path);
        require_regular_file(&path, "skill file")?;
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", normalize_path(&path)))?;
        if bytes.len() as u64 != file.bytes || sha256_bytes(&bytes) != file.sha256 {
            bail!("skill file identity mismatch: {}", file.path);
        }
    }
    for skill in &manifest.skills {
        let entrypoint = root.join(&skill.path).join("SKILL.md");
        let source = fs::read_to_string(&entrypoint)
            .with_context(|| format!("failed to read {} as UTF-8", normalize_path(&entrypoint)))?;
        let (name, description) = parse_skill_identity(&source, &entrypoint)?;
        if name != skill.id || description != skill.description {
            bail!("skill metadata does not match {}", skill.path);
        }
    }

    Ok(ValidatedSkills {
        root,
        manifest,
        manifest_sha256: sha256_bytes(&manifest_bytes),
    })
}

pub(crate) fn validate_skills_manifest(manifest: &SkillsManifest) -> Result<()> {
    if manifest.schema != SKILLS_MANIFEST_SCHEMA {
        bail!(
            "skills manifest schema is {:?}, expected {:?}",
            manifest.schema,
            SKILLS_MANIFEST_SCHEMA
        );
    }
    Version::parse(&manifest.wikitool_version)
        .context("skills manifest wikitool_version is not valid semver")?;

    let mut skill_ids = BTreeSet::new();
    for skill in &manifest.skills {
        if !skill_ids.insert(skill.id.clone()) {
            bail!("duplicate skill id {:?}", skill.id);
        }
        if !PUBLIC_SKILL_IDS.contains(&skill.id.as_str()) {
            bail!("unsupported public skill id {:?}", skill.id);
        }
        if skill.description.trim().is_empty() {
            bail!("skill {:?} has an empty description", skill.id);
        }
        let expected_path = skill.id.clone();
        if skill.path != expected_path {
            bail!(
                "skill {:?} path is {:?}, expected {:?}",
                skill.id,
                skill.path,
                expected_path
            );
        }
    }
    let expected = PUBLIC_SKILL_IDS
        .iter()
        .map(|value| (*value).to_string())
        .collect::<BTreeSet<_>>();
    if skill_ids != expected {
        bail!("skills manifest must contain exactly the four public Wikitool skills");
    }

    let mut paths = BTreeSet::new();
    for file in &manifest.files {
        validate_relative_path(Path::new(&file.path), "skill file")?;
        if !paths.insert(file.path.clone()) {
            bail!("duplicate skill file path {:?}", file.path);
        }
        validate_sha256(&file.sha256, "skill file")?;
        let in_public_skill = PUBLIC_SKILL_IDS
            .iter()
            .any(|skill| file.path.starts_with(&format!("{skill}/")));
        if !in_public_skill {
            bail!(
                "file is outside the public skills boundary: {:?}",
                file.path
            );
        }
    }
    for skill in &manifest.skills {
        let entrypoint = format!("{}/SKILL.md", skill.path);
        let metadata = format!("{}/agents/openai.yaml", skill.path);
        if !paths.contains(&entrypoint) || !paths.contains(&metadata) {
            bail!("skill {:?} is incomplete", skill.id);
        }
    }
    Ok(())
}

pub(crate) fn load_install_receipt(project_root: &Path) -> Result<Option<SkillsInstallReceipt>> {
    let path = project_root.join(SKILLS_INSTALL_RECEIPT);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect {}", normalize_path(&path)));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "skills install receipt must be a regular file: {}",
            normalize_path(&path)
        );
    }
    let bytes =
        fs::read(&path).with_context(|| format!("failed to read {}", normalize_path(&path)))?;
    let receipt: SkillsInstallReceipt = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid JSON in {}", normalize_path(&path)))?;
    validate_install_receipt(&receipt)?;
    Ok(Some(receipt))
}

pub(crate) fn verify_skills_install(project_root: &Path) -> Result<Option<SkillsInstallReceipt>> {
    let Some(receipt) = load_install_receipt(project_root)? else {
        return Ok(None);
    };
    let files = receipt
        .managed_files
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect::<BTreeMap<_, _>>();
    verify_previous_files(project_root, &files)?;
    verify_owned_skill_directories(project_root, &files)?;
    Ok(Some(receipt))
}

pub(crate) fn plan_skills_install(
    project_root: &Path,
    skills: &ValidatedSkills,
    targets: &[&str],
) -> Result<DesiredSkillsInstall> {
    if targets.is_empty() {
        bail!("skills setup requires at least one resolved skill target");
    }
    let previous = load_install_receipt(project_root)?;
    if let Some(receipt) = &previous {
        let installed = Version::parse(&receipt.wikitool_version)
            .context("installed skills receipt has invalid wikitool_version")?;
        let incoming = Version::parse(&skills.manifest.wikitool_version)
            .context("skills manifest has invalid wikitool_version")?;
        if incoming < installed {
            bail!(
                "skills setup refuses downgrade from {} to {}",
                installed,
                incoming
            );
        }
    }

    let previous_files = previous
        .as_ref()
        .map(|receipt| {
            receipt
                .managed_files
                .iter()
                .map(|file| (file.path.clone(), file.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    verify_previous_files(project_root, &previous_files)?;
    verify_owned_skill_directories(project_root, &previous_files)?;

    let mut desired_files = BTreeMap::<String, (ManagedSkillFile, Vec<u8>)>::new();
    for target in targets {
        if !matches!(*target, "agents" | "claude") {
            bail!("unsupported resolved skill target {target:?}");
        }
        let harness_root = if *target == "agents" {
            ".agents/skills"
        } else {
            ".claude/skills"
        };
        for skill in &skills.manifest.skills {
            let source_prefix = format!("{}/", skill.path);
            for file in skills
                .manifest
                .files
                .iter()
                .filter(|file| file.path.starts_with(&source_prefix))
            {
                let suffix = file
                    .path
                    .strip_prefix(&source_prefix)
                    .expect("checked prefix");
                let destination = format!("{harness_root}/{}/{suffix}", skill.id);
                validate_managed_skill_path(Path::new(&destination))?;
                let bytes = fs::read(skills.root.join(&file.path))
                    .with_context(|| format!("failed to read skill file {}", file.path))?;
                let managed = ManagedSkillFile {
                    path: destination.clone(),
                    bytes: file.bytes,
                    sha256: file.sha256.clone(),
                };
                if desired_files
                    .insert(destination.clone(), (managed, bytes))
                    .is_some()
                {
                    bail!("duplicate desired skills install path {destination:?}");
                }
            }
        }
    }

    let mut actions = Vec::new();
    let mut writes = Vec::new();
    for (path, (managed, bytes)) in &desired_files {
        validate_destination(project_root, Path::new(path))?;
        let destination = project_root.join(path);
        match previous_files.get(path) {
            Some(old) if old.sha256 == managed.sha256 && old.bytes == managed.bytes => {
                actions.push(SkillInstallAction {
                    action: "unchanged",
                    path: path.clone(),
                });
            }
            Some(_) => {
                actions.push(SkillInstallAction {
                    action: "replace",
                    path: path.clone(),
                });
                writes.push((destination, bytes.clone()));
            }
            None => {
                match fs::symlink_metadata(&destination) {
                    Ok(_) => bail!("skills setup refuses unowned existing path {path:?}"),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("failed to inspect {}", normalize_path(&destination))
                        });
                    }
                }
                actions.push(SkillInstallAction {
                    action: "create",
                    path: path.clone(),
                });
                writes.push((destination, bytes.clone()));
            }
        }
    }

    let mut removals = Vec::new();
    for path in previous_files.keys() {
        if !desired_files.contains_key(path) {
            actions.push(SkillInstallAction {
                action: "remove",
                path: path.clone(),
            });
            removals.push(project_root.join(path));
        }
    }
    actions.sort_by(|left, right| left.path.cmp(&right.path));
    writes.sort_by(|left, right| left.0.cmp(&right.0));
    removals.sort();

    let receipt = SkillsInstallReceipt {
        schema: SKILLS_INSTALL_SCHEMA.to_string(),
        wikitool_version: skills.manifest.wikitool_version.clone(),
        skills_manifest_sha256: skills.manifest_sha256.clone(),
        skill_targets: targets.iter().map(|target| (*target).to_string()).collect(),
        managed_files: desired_files
            .into_values()
            .map(|(managed, _)| managed)
            .collect(),
    };
    validate_install_receipt(&receipt)?;
    Ok(DesiredSkillsInstall {
        receipt,
        writes,
        removals,
        actions,
    })
}

pub(crate) fn apply_skills_install(project_root: &Path, plan: DesiredSkillsInstall) -> Result<()> {
    for (path, bytes) in &plan.writes {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
        }
        wikitool_core::support::atomic_write(path, bytes)
            .with_context(|| format!("failed to install {}", normalize_path(path)))?;
    }
    for path in &plan.removals {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove {}", normalize_path(path)))?;
    }
    let receipt_path = project_root.join(SKILLS_INSTALL_RECEIPT);
    let receipt_bytes = serde_json::to_vec_pretty(&plan.receipt)?;
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }
    wikitool_core::support::atomic_write(&receipt_path, receipt_bytes)
        .with_context(|| format!("failed to write {}", normalize_path(&receipt_path)))?;
    remove_empty_skill_directories(project_root)?;
    Ok(())
}

pub(crate) fn plan_skills_uninstall(
    project_root: &Path,
) -> Result<Option<(SkillsInstallReceipt, Vec<SkillInstallAction>)>> {
    let Some(receipt) = load_install_receipt(project_root)? else {
        return Ok(None);
    };
    let files = receipt
        .managed_files
        .iter()
        .map(|file| (file.path.clone(), file.clone()))
        .collect::<BTreeMap<_, _>>();
    verify_previous_files(project_root, &files)?;
    verify_owned_skill_directories(project_root, &files)?;
    let mut actions = receipt
        .managed_files
        .iter()
        .map(|file| SkillInstallAction {
            action: "remove",
            path: file.path.clone(),
        })
        .collect::<Vec<_>>();
    actions.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Some((receipt, actions)))
}

pub(crate) fn apply_skills_uninstall(
    project_root: &Path,
    receipt: &SkillsInstallReceipt,
) -> Result<()> {
    for file in &receipt.managed_files {
        let path = project_root.join(&file.path);
        fs::remove_file(&path)
            .with_context(|| format!("failed to remove {}", normalize_path(&path)))?;
    }
    let receipt_path = project_root.join(SKILLS_INSTALL_RECEIPT);
    fs::remove_file(&receipt_path)
        .with_context(|| format!("failed to remove {}", normalize_path(&receipt_path)))?;
    remove_empty_skill_directories(project_root)?;
    if let Some(parent) = receipt_path.parent() {
        remove_dir_if_empty(parent)?;
    }
    Ok(())
}

pub(crate) fn resolve_project_root(path: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve project root {}", normalize_path(path)))?;
    if !root.is_dir() {
        bail!("project root is not a directory: {}", normalize_path(&root));
    }
    Ok(root)
}

fn validate_install_receipt(receipt: &SkillsInstallReceipt) -> Result<()> {
    if receipt.schema != SKILLS_INSTALL_SCHEMA {
        bail!(
            "skills install receipt schema is {:?}, expected {:?}",
            receipt.schema,
            SKILLS_INSTALL_SCHEMA
        );
    }
    Version::parse(&receipt.wikitool_version)
        .context("skills install receipt wikitool_version is not valid semver")?;
    validate_sha256(&receipt.skills_manifest_sha256, "skills install receipt")?;
    let mut targets = BTreeSet::<String>::new();
    for target in &receipt.skill_targets {
        if !matches!(target.as_str(), "agents" | "claude") || !targets.insert(target.clone()) {
            bail!("invalid skills install receipt target {target:?}");
        }
    }
    if targets.is_empty() {
        bail!("skills install receipt must contain at least one skill target");
    }
    let mut paths = BTreeSet::new();
    for file in &receipt.managed_files {
        validate_managed_skill_path(Path::new(&file.path))?;
        validate_sha256(&file.sha256, "managed skill file")?;
        if !paths.insert(&file.path) {
            bail!("duplicate managed skill path {:?}", file.path);
        }
    }
    for target in &targets {
        for skill in PUBLIC_SKILL_IDS {
            for required in ["SKILL.md", "agents/openai.yaml"] {
                let path = format!(".{target}/skills/{skill}/{required}");
                if !paths.contains(&path) {
                    bail!("skills install receipt is missing required managed file {path:?}");
                }
            }
        }
    }
    for path in &paths {
        let declared_target = if path.starts_with(".agents/") {
            "agents"
        } else {
            "claude"
        };
        if !targets.contains(declared_target) {
            bail!("managed skill path has no declared target: {path:?}");
        }
    }
    Ok(())
}

fn verify_previous_files(
    project_root: &Path,
    files: &BTreeMap<String, ManagedSkillFile>,
) -> Result<()> {
    for (relative, expected) in files {
        validate_destination(project_root, Path::new(relative))?;
        let path = project_root.join(relative);
        require_regular_file(&path, "receipt-owned skill file")?;
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", normalize_path(&path)))?;
        if bytes.len() as u64 != expected.bytes || sha256_bytes(&bytes) != expected.sha256 {
            bail!("receipt-owned skill file was modified: {relative}");
        }
    }
    Ok(())
}

fn verify_owned_skill_directories(
    project_root: &Path,
    files: &BTreeMap<String, ManagedSkillFile>,
) -> Result<()> {
    let owned = files.keys().cloned().collect::<BTreeSet<_>>();
    for target_root in [".agents/skills", ".claude/skills"] {
        for skill in PUBLIC_SKILL_IDS {
            let root = project_root.join(target_root).join(skill);
            if !root.exists() {
                continue;
            }
            validate_destination(project_root, &Path::new(target_root).join(skill))?;
            for relative in collect_regular_files(&root)? {
                let project_relative = Path::new(target_root).join(skill).join(relative);
                let normalized = normalize_path(&project_relative);
                if !owned.contains(&normalized) {
                    bail!("skills setup refuses unowned file inside managed skill: {normalized}");
                }
            }
        }
    }
    Ok(())
}

fn validate_managed_skill_path(path: &Path) -> Result<()> {
    validate_relative_path(path, "managed skill file")?;
    let components = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if components.len() < 4
        || !matches!(components[0].as_str(), ".agents" | ".claude")
        || components[1] != "skills"
        || !PUBLIC_SKILL_IDS.contains(&components[2].as_str())
    {
        bail!(
            "managed skill path is outside the Wikitool skill boundary: {:?}",
            path
        );
    }
    Ok(())
}

fn validate_destination(root: &Path, relative: &Path) -> Result<()> {
    validate_relative_path(relative, "skills install destination")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "skills install destination crosses a symlink: {}",
                    normalize_path(&current)
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect {}", normalize_path(&current)));
            }
        }
    }
    Ok(())
}

fn validate_relative_path(path: &Path, kind: &str) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_)
                    | Component::RootDir
                    | Component::ParentDir
                    | Component::CurDir
            )
        })
    {
        bail!("{kind} path must remain relative: {:?}", path);
    }
    Ok(())
}

fn validate_sha256(value: &str, kind: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("{kind} has an invalid SHA-256 value");
    }
    Ok(())
}

fn parse_skill_identity(source: &str, path: &Path) -> Result<(String, String)> {
    let mut lines = source.lines();
    if lines.next() != Some("---") {
        bail!(
            "skill entrypoint has no YAML frontmatter: {}",
            normalize_path(path)
        );
    }
    let mut name = None;
    let mut description = None;
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        if let Some(value) = line.strip_prefix("name:")
            && name.replace(value.trim().to_string()).is_some()
        {
            bail!("skill entrypoint repeats name: {}", normalize_path(path));
        }
        if let Some(value) = line.strip_prefix("description:")
            && description.replace(value.trim().to_string()).is_some()
        {
            bail!(
                "skill entrypoint repeats description: {}",
                normalize_path(path)
            );
        }
    }
    if !closed {
        bail!(
            "skill entrypoint frontmatter is not closed: {}",
            normalize_path(path)
        );
    }
    let name = name
        .filter(|value| !value.is_empty())
        .with_context(|| format!("skill entrypoint has no name: {}", normalize_path(path)))?;
    let description = description
        .filter(|value| !value.is_empty())
        .with_context(|| {
            format!(
                "skill entrypoint has no description: {}",
                normalize_path(path)
            )
        })?;
    Ok((name, description))
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
                    "skills distribution or managed skill contains a symlink: {}",
                    normalize_path(&path)
                );
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                files.push(path.strip_prefix(root).expect("root prefix").to_path_buf());
            } else {
                bail!(
                    "unsupported skills distribution entry: {}",
                    normalize_path(&path)
                );
            }
        }
    }
    files.sort_by_key(|path| normalize_path(path));
    Ok(files)
}

fn remove_empty_skill_directories(project_root: &Path) -> Result<()> {
    for target_root in [".agents/skills", ".claude/skills"] {
        for skill in PUBLIC_SKILL_IDS {
            let skill_root = project_root.join(target_root).join(skill);
            prune_empty_directories(&skill_root, &skill_root)?;
        }
        remove_dir_if_empty(&project_root.join(target_root))?;
        if let Some(harness_root) = Path::new(target_root).parent() {
            remove_dir_if_empty(&project_root.join(harness_root))?;
        }
    }
    Ok(())
}

fn prune_empty_directories(root: &Path, directory: &Path) -> Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    let children = fs::read_dir(directory)
        .with_context(|| format!("failed to inspect {}", normalize_path(directory)))?
        .collect::<std::io::Result<Vec<_>>>()?;
    for child in children {
        let path = child.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to inspect {}", normalize_path(&path)))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            prune_empty_directories(root, &path)?;
        }
    }
    if directory.starts_with(root) {
        remove_dir_if_empty(directory)?;
    }
    Ok(())
}

fn remove_dir_if_empty(path: &Path) -> Result<()> {
    if !path.is_dir() {
        return Ok(());
    }
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("failed to inspect {}", normalize_path(path)))?;
    if entries.next().is_none() {
        fs::remove_dir(path)
            .with_context(|| format!("failed to remove empty {}", normalize_path(path)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_test_skills(root: &Path, version: &str) {
        let mut files = Vec::new();
        for skill in PUBLIC_SKILL_IDS {
            let skill_root = root.join(skill);
            fs::create_dir_all(skill_root.join("agents")).expect("skill directories");
            for (relative, bytes) in [
                (
                    "SKILL.md",
                    format!("---\nname: {skill}\ndescription: Test {skill}.\n---\n\n# Test\n")
                        .into_bytes(),
                ),
                (
                    "agents/openai.yaml",
                    format!("interface:\n  display_name: Test {skill}\n").into_bytes(),
                ),
            ] {
                fs::write(skill_root.join(relative), &bytes).expect("skill file");
                files.push(SkillManifestFile {
                    path: format!("{skill}/{relative}"),
                    bytes: bytes.len() as u64,
                    sha256: sha256_bytes(&bytes),
                });
            }
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let mut skills = PUBLIC_SKILL_IDS
            .iter()
            .map(|skill| SkillManifestEntry {
                id: (*skill).to_string(),
                description: format!("Test {skill}."),
                path: (*skill).to_string(),
            })
            .collect::<Vec<_>>();
        skills.sort_by(|left, right| left.id.cmp(&right.id));
        let manifest = SkillsManifest {
            schema: SKILLS_MANIFEST_SCHEMA.to_string(),
            wikitool_version: version.to_string(),
            skills,
            files,
        };
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest JSON"),
        )
        .expect("manifest");
    }

    #[test]
    fn managed_skill_paths_are_narrow() {
        assert!(
            validate_managed_skill_path(Path::new(".agents/skills/wiki-writing/SKILL.md")).is_ok()
        );
        assert!(validate_managed_skill_path(Path::new("../wiki-writing/SKILL.md")).is_err());
        assert!(
            validate_managed_skill_path(Path::new(".agents/skills/contextmink/SKILL.md")).is_err()
        );
    }

    #[test]
    fn manifest_requires_the_complete_public_skill_set() {
        let manifest = SkillsManifest {
            schema: SKILLS_MANIFEST_SCHEMA.to_string(),
            wikitool_version: "0.8.0".to_string(),
            skills: Vec::new(),
            files: Vec::new(),
        };
        assert!(validate_skills_manifest(&manifest).is_err());
    }

    #[test]
    fn install_is_idempotent_and_uninstall_removes_only_owned_trees() {
        let skills_root = tempfile::tempdir().expect("skills root");
        let project = tempfile::tempdir().expect("project");
        write_test_skills(skills_root.path(), "0.8.0");
        let skills = load_skills(skills_root.path()).expect("valid skills");

        let plan = plan_skills_install(project.path(), &skills, &["agents", "claude"])
            .expect("install plan");
        assert!(plan.actions.iter().all(|action| action.action == "create"));
        apply_skills_install(project.path(), plan).expect("install");

        let receipt = verify_skills_install(project.path())
            .expect("verify install")
            .expect("receipt");
        assert_eq!(receipt.skill_targets, ["agents", "claude"]);
        assert_eq!(receipt.managed_files.len(), PUBLIC_SKILL_IDS.len() * 4);

        let second = plan_skills_install(project.path(), &skills, &["agents", "claude"])
            .expect("idempotent plan");
        assert!(second.writes.is_empty());
        assert!(second.removals.is_empty());
        assert!(
            second
                .actions
                .iter()
                .all(|action| action.action == "unchanged")
        );

        let (receipt, _) = plan_skills_uninstall(project.path())
            .expect("uninstall plan")
            .expect("installed");
        apply_skills_uninstall(project.path(), &receipt).expect("uninstall");
        assert!(!project.path().join(".agents").exists());
        assert!(!project.path().join(".claude").exists());
        assert!(!project.path().join(".wikitool-skills").exists());
    }

    #[test]
    fn install_and_uninstall_refuse_modified_or_foreign_skill_files() {
        let skills_root = tempfile::tempdir().expect("skills root");
        let project = tempfile::tempdir().expect("project");
        write_test_skills(skills_root.path(), "0.8.0");
        let skills = load_skills(skills_root.path()).expect("valid skills");
        let plan = plan_skills_install(project.path(), &skills, &["agents"]).expect("install plan");
        apply_skills_install(project.path(), plan).expect("install");

        let managed = project.path().join(".agents/skills/wiki-writing/SKILL.md");
        fs::write(&managed, "modified\n").expect("modify managed file");
        assert!(
            plan_skills_install(project.path(), &skills, &["agents"])
                .expect_err("modified setup must fail")
                .to_string()
                .contains("was modified")
        );
        assert!(
            plan_skills_uninstall(project.path())
                .expect_err("modified uninstall must fail")
                .to_string()
                .contains("was modified")
        );

        let source = skills.root.join("wiki-writing/SKILL.md");
        fs::copy(source, &managed).expect("restore managed file");
        let foreign = project
            .path()
            .join(".agents/skills/wiki-writing/foreign.md");
        fs::write(&foreign, "foreign\n").expect("foreign file");
        assert!(
            plan_skills_install(project.path(), &skills, &["agents"])
                .expect_err("foreign setup must fail")
                .to_string()
                .contains("unowned file")
        );
    }

    #[test]
    fn skill_tampering_and_downgrades_fail_closed() {
        let first_root = tempfile::tempdir().expect("first skills root");
        let older_root = tempfile::tempdir().expect("older skills root");
        let project = tempfile::tempdir().expect("project");
        write_test_skills(first_root.path(), "0.8.0");
        write_test_skills(older_root.path(), "0.7.0");
        let first = load_skills(first_root.path()).expect("valid first skills");
        let plan = plan_skills_install(project.path(), &first, &["agents"]).expect("install plan");
        apply_skills_install(project.path(), plan).expect("install");
        let older = load_skills(older_root.path()).expect("valid older skills");
        assert!(
            plan_skills_install(project.path(), &older, &["agents"])
                .expect_err("downgrade must fail")
                .to_string()
                .contains("refuses downgrade")
        );

        fs::write(
            first_root.path().join("wiki-writing/SKILL.md"),
            "tampered\n",
        )
        .expect("tamper skill");
        assert!(
            load_skills(first_root.path())
                .expect_err("tampered skill must fail")
                .to_string()
                .contains("identity mismatch")
        );
    }
}
