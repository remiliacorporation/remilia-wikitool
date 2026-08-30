use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cli_support::normalize_path;

pub(crate) const AGENT_PACK_SCHEMA: &str = "wikitool.agent-pack.v1";
pub(crate) const AGENT_INSTALL_SCHEMA: &str = "wikitool.agent-install.v1";
pub(crate) const AGENT_INSTALL_RECEIPT: &str = ".wikitool-agent/project-install.json";
pub(crate) const PUBLIC_SKILL_IDS: [&str; 4] = [
    "prose-review",
    "wiki-interview",
    "wiki-writing",
    "wikitool-operator",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct AgentPackManifest {
    pub(crate) schema: String,
    pub(crate) wikitool_version: String,
    pub(crate) skills: Vec<AgentPackSkill>,
    pub(crate) files: Vec<AgentPackFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct AgentPackSkill {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct AgentPackFile {
    pub(crate) path: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Debug)]
pub(crate) struct ValidatedAgentPack {
    pub(crate) root: PathBuf,
    pub(crate) manifest: AgentPackManifest,
    pub(crate) manifest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct AgentInstallReceipt {
    pub(crate) schema: String,
    pub(crate) wikitool_version: String,
    pub(crate) pack_manifest_sha256: String,
    pub(crate) skill_targets: Vec<String>,
    pub(crate) managed_files: Vec<ManagedAgentFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ManagedAgentFile {
    pub(crate) path: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct AgentInstallAction {
    pub(crate) action: &'static str,
    pub(crate) path: String,
}

#[derive(Debug)]
pub(crate) struct DesiredAgentInstall {
    pub(crate) receipt: AgentInstallReceipt,
    pub(crate) writes: Vec<(PathBuf, Vec<u8>)>,
    pub(crate) removals: Vec<PathBuf>,
    pub(crate) actions: Vec<AgentInstallAction>,
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn load_agent_pack(root: &Path) -> Result<ValidatedAgentPack> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("failed to resolve agent pack {}", normalize_path(root)))?;
    if !root.is_dir() {
        bail!(
            "agent pack root is not a directory: {}",
            normalize_path(&root)
        );
    }
    let manifest_path = root.join("manifest.json");
    require_regular_file(&manifest_path, "agent pack manifest")?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", normalize_path(&manifest_path)))?;
    let manifest: AgentPackManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("invalid JSON in {}", normalize_path(&manifest_path)))?;
    validate_agent_pack_manifest(&manifest)?;

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
        bail!("agent pack file inventory mismatch; missing={missing:?}; extra={extra:?}");
    }

    for file in &manifest.files {
        let path = root.join(&file.path);
        require_regular_file(&path, "agent pack file")?;
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", normalize_path(&path)))?;
        if bytes.len() as u64 != file.bytes || sha256_bytes(&bytes) != file.sha256 {
            bail!("agent pack file identity mismatch: {}", file.path);
        }
    }
    for skill in &manifest.skills {
        let entrypoint = root.join(&skill.path).join("SKILL.md");
        let source = fs::read_to_string(&entrypoint)
            .with_context(|| format!("failed to read {} as UTF-8", normalize_path(&entrypoint)))?;
        let (name, description) = parse_skill_identity(&source, &entrypoint)?;
        if name != skill.id || description != skill.description {
            bail!("agent pack skill metadata does not match {}", skill.path);
        }
    }

    Ok(ValidatedAgentPack {
        root,
        manifest,
        manifest_sha256: sha256_bytes(&manifest_bytes),
    })
}

pub(crate) fn validate_agent_pack_manifest(manifest: &AgentPackManifest) -> Result<()> {
    if manifest.schema != AGENT_PACK_SCHEMA {
        bail!(
            "agent pack schema is {:?}, expected {:?}",
            manifest.schema,
            AGENT_PACK_SCHEMA
        );
    }
    Version::parse(&manifest.wikitool_version)
        .context("agent pack wikitool_version is not valid semver")?;

    let mut skill_ids = BTreeSet::new();
    for skill in &manifest.skills {
        if !skill_ids.insert(skill.id.clone()) {
            bail!("duplicate agent pack skill id {:?}", skill.id);
        }
        if !PUBLIC_SKILL_IDS.contains(&skill.id.as_str()) {
            bail!("unsupported agent pack skill id {:?}", skill.id);
        }
        if skill.description.trim().is_empty() {
            bail!("agent pack skill {:?} has an empty description", skill.id);
        }
        let expected_path = format!("skills/{}", skill.id);
        if skill.path != expected_path {
            bail!(
                "agent pack skill {:?} path is {:?}, expected {:?}",
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
        bail!("agent pack must contain exactly the four public Wikitool skills");
    }

    let mut paths = BTreeSet::new();
    for file in &manifest.files {
        validate_relative_path(Path::new(&file.path), "agent pack file")?;
        if !paths.insert(file.path.clone()) {
            bail!("duplicate agent pack file path {:?}", file.path);
        }
        validate_sha256(&file.sha256, "agent pack file")?;
        let in_public_skill = PUBLIC_SKILL_IDS
            .iter()
            .any(|skill| file.path.starts_with(&format!("skills/{skill}/")));
        let in_integration = file.path.starts_with("integration/") && file.path.ends_with(".md");
        if file.path != "README.md" && !in_public_skill && !in_integration {
            bail!(
                "agent pack file is outside the public pack boundary: {:?}",
                file.path
            );
        }
    }
    for required in [
        "README.md",
        "integration/agent_integration.md",
        "integration/site_adapters.md",
    ] {
        if !paths.contains(required) {
            bail!("agent pack is missing required file {required:?}");
        }
    }
    for skill in &manifest.skills {
        let entrypoint = format!("{}/SKILL.md", skill.path);
        let metadata = format!("{}/agents/openai.yaml", skill.path);
        if !paths.contains(&entrypoint) || !paths.contains(&metadata) {
            bail!("agent pack skill {:?} is incomplete", skill.id);
        }
    }
    Ok(())
}

pub(crate) fn load_install_receipt(project_root: &Path) -> Result<Option<AgentInstallReceipt>> {
    let path = project_root.join(AGENT_INSTALL_RECEIPT);
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
            "agent install receipt must be a regular file: {}",
            normalize_path(&path)
        );
    }
    let bytes =
        fs::read(&path).with_context(|| format!("failed to read {}", normalize_path(&path)))?;
    let receipt: AgentInstallReceipt = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid JSON in {}", normalize_path(&path)))?;
    validate_install_receipt(&receipt)?;
    Ok(Some(receipt))
}

pub(crate) fn verify_agent_install(project_root: &Path) -> Result<Option<AgentInstallReceipt>> {
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

pub(crate) fn plan_agent_install(
    project_root: &Path,
    pack: &ValidatedAgentPack,
    targets: &[&str],
) -> Result<DesiredAgentInstall> {
    if targets.is_empty() {
        bail!("agent setup requires at least one resolved skill target");
    }
    let previous = load_install_receipt(project_root)?;
    if let Some(receipt) = &previous {
        let installed = Version::parse(&receipt.wikitool_version)
            .context("installed agent receipt has invalid wikitool_version")?;
        let incoming = Version::parse(&pack.manifest.wikitool_version)
            .context("agent pack has invalid wikitool_version")?;
        if incoming < installed {
            bail!(
                "agent setup refuses downgrade from {} to {}",
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

    let mut desired_files = BTreeMap::<String, (ManagedAgentFile, Vec<u8>)>::new();
    for target in targets {
        if !matches!(*target, "agents" | "claude") {
            bail!("unsupported resolved skill target {target:?}");
        }
        let harness_root = if *target == "agents" {
            ".agents/skills"
        } else {
            ".claude/skills"
        };
        for skill in &pack.manifest.skills {
            let source_prefix = format!("{}/", skill.path);
            for file in pack
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
                let bytes = fs::read(pack.root.join(&file.path))
                    .with_context(|| format!("failed to read agent pack file {}", file.path))?;
                let managed = ManagedAgentFile {
                    path: destination.clone(),
                    bytes: file.bytes,
                    sha256: file.sha256.clone(),
                };
                if desired_files
                    .insert(destination.clone(), (managed, bytes))
                    .is_some()
                {
                    bail!("duplicate desired agent install path {destination:?}");
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
                actions.push(AgentInstallAction {
                    action: "unchanged",
                    path: path.clone(),
                });
            }
            Some(_) => {
                actions.push(AgentInstallAction {
                    action: "replace",
                    path: path.clone(),
                });
                writes.push((destination, bytes.clone()));
            }
            None => {
                match fs::symlink_metadata(&destination) {
                    Ok(_) => bail!("agent setup refuses unowned existing path {path:?}"),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("failed to inspect {}", normalize_path(&destination))
                        });
                    }
                }
                actions.push(AgentInstallAction {
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
            actions.push(AgentInstallAction {
                action: "remove",
                path: path.clone(),
            });
            removals.push(project_root.join(path));
        }
    }
    actions.sort_by(|left, right| left.path.cmp(&right.path));
    writes.sort_by(|left, right| left.0.cmp(&right.0));
    removals.sort();

    let receipt = AgentInstallReceipt {
        schema: AGENT_INSTALL_SCHEMA.to_string(),
        wikitool_version: pack.manifest.wikitool_version.clone(),
        pack_manifest_sha256: pack.manifest_sha256.clone(),
        skill_targets: targets.iter().map(|target| (*target).to_string()).collect(),
        managed_files: desired_files
            .into_values()
            .map(|(managed, _)| managed)
            .collect(),
    };
    validate_install_receipt(&receipt)?;
    Ok(DesiredAgentInstall {
        receipt,
        writes,
        removals,
        actions,
    })
}

pub(crate) fn apply_agent_install(project_root: &Path, plan: DesiredAgentInstall) -> Result<()> {
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
    let receipt_path = project_root.join(AGENT_INSTALL_RECEIPT);
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

pub(crate) fn plan_agent_uninstall(
    project_root: &Path,
) -> Result<Option<(AgentInstallReceipt, Vec<AgentInstallAction>)>> {
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
        .map(|file| AgentInstallAction {
            action: "remove",
            path: file.path.clone(),
        })
        .collect::<Vec<_>>();
    actions.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Some((receipt, actions)))
}

pub(crate) fn apply_agent_uninstall(
    project_root: &Path,
    receipt: &AgentInstallReceipt,
) -> Result<()> {
    for file in &receipt.managed_files {
        let path = project_root.join(&file.path);
        fs::remove_file(&path)
            .with_context(|| format!("failed to remove {}", normalize_path(&path)))?;
    }
    let receipt_path = project_root.join(AGENT_INSTALL_RECEIPT);
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

fn validate_install_receipt(receipt: &AgentInstallReceipt) -> Result<()> {
    if receipt.schema != AGENT_INSTALL_SCHEMA {
        bail!(
            "agent install receipt schema is {:?}, expected {:?}",
            receipt.schema,
            AGENT_INSTALL_SCHEMA
        );
    }
    Version::parse(&receipt.wikitool_version)
        .context("agent install receipt wikitool_version is not valid semver")?;
    validate_sha256(&receipt.pack_manifest_sha256, "agent install receipt")?;
    let mut targets = BTreeSet::<String>::new();
    for target in &receipt.skill_targets {
        if !matches!(target.as_str(), "agents" | "claude") || !targets.insert(target.clone()) {
            bail!("invalid agent install receipt skill target {target:?}");
        }
    }
    if targets.is_empty() {
        bail!("agent install receipt must contain at least one skill target");
    }
    let mut paths = BTreeSet::new();
    for file in &receipt.managed_files {
        validate_managed_skill_path(Path::new(&file.path))?;
        validate_sha256(&file.sha256, "managed agent file")?;
        if !paths.insert(&file.path) {
            bail!("duplicate managed agent path {:?}", file.path);
        }
    }
    for target in &targets {
        for skill in PUBLIC_SKILL_IDS {
            for required in ["SKILL.md", "agents/openai.yaml"] {
                let path = format!(".{target}/skills/{skill}/{required}");
                if !paths.contains(&path) {
                    bail!("agent install receipt is missing required managed file {path:?}");
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
            bail!("managed agent path has no declared skill target: {path:?}");
        }
    }
    Ok(())
}

fn verify_previous_files(
    project_root: &Path,
    files: &BTreeMap<String, ManagedAgentFile>,
) -> Result<()> {
    for (relative, expected) in files {
        validate_destination(project_root, Path::new(relative))?;
        let path = project_root.join(relative);
        require_regular_file(&path, "receipt-owned agent file")?;
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", normalize_path(&path)))?;
        if bytes.len() as u64 != expected.bytes || sha256_bytes(&bytes) != expected.sha256 {
            bail!("receipt-owned agent file was modified: {relative}");
        }
    }
    Ok(())
}

fn verify_owned_skill_directories(
    project_root: &Path,
    files: &BTreeMap<String, ManagedAgentFile>,
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
                    bail!("agent setup refuses unowned file inside managed skill: {normalized}");
                }
            }
        }
    }
    Ok(())
}

fn validate_managed_skill_path(path: &Path) -> Result<()> {
    validate_relative_path(path, "managed agent file")?;
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
            "managed agent path is outside the Wikitool skill boundary: {:?}",
            path
        );
    }
    Ok(())
}

fn validate_destination(root: &Path, relative: &Path) -> Result<()> {
    validate_relative_path(relative, "agent install destination")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "agent install destination crosses a symlink: {}",
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
                    "agent pack or managed skill contains a symlink: {}",
                    normalize_path(&path)
                );
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                files.push(path.strip_prefix(root).expect("root prefix").to_path_buf());
            } else {
                bail!("unsupported agent pack entry: {}", normalize_path(&path));
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

    fn write_test_pack(root: &Path, version: &str) {
        let mut files = Vec::new();
        for (relative, bytes) in [
            ("README.md", b"# Test agent pack\n".as_slice()),
            (
                "integration/agent_integration.md",
                b"# Test agent integration\n".as_slice(),
            ),
            (
                "integration/site_adapters.md",
                b"# Test site adapters\n".as_slice(),
            ),
        ] {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("file parent")).expect("pack directories");
            fs::write(&path, bytes).expect("pack file");
            files.push(AgentPackFile {
                path: relative.to_string(),
                bytes: bytes.len() as u64,
                sha256: sha256_bytes(bytes),
            });
        }
        for skill in PUBLIC_SKILL_IDS {
            let skill_root = root.join("skills").join(skill);
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
                files.push(AgentPackFile {
                    path: format!("skills/{skill}/{relative}"),
                    bytes: bytes.len() as u64,
                    sha256: sha256_bytes(&bytes),
                });
            }
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let mut skills = PUBLIC_SKILL_IDS
            .iter()
            .map(|skill| AgentPackSkill {
                id: (*skill).to_string(),
                description: format!("Test {skill}."),
                path: format!("skills/{skill}"),
            })
            .collect::<Vec<_>>();
        skills.sort_by(|left, right| left.id.cmp(&right.id));
        let manifest = AgentPackManifest {
            schema: AGENT_PACK_SCHEMA.to_string(),
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
        let manifest = AgentPackManifest {
            schema: AGENT_PACK_SCHEMA.to_string(),
            wikitool_version: "0.8.0".to_string(),
            skills: Vec::new(),
            files: Vec::new(),
        };
        assert!(validate_agent_pack_manifest(&manifest).is_err());
    }

    #[test]
    fn install_is_idempotent_and_uninstall_removes_only_owned_trees() {
        let pack_root = tempfile::tempdir().expect("pack");
        let project = tempfile::tempdir().expect("project");
        write_test_pack(pack_root.path(), "0.8.0");
        let pack = load_agent_pack(pack_root.path()).expect("valid pack");

        let plan =
            plan_agent_install(project.path(), &pack, &["agents", "claude"]).expect("install plan");
        assert!(plan.actions.iter().all(|action| action.action == "create"));
        apply_agent_install(project.path(), plan).expect("install");

        let receipt = verify_agent_install(project.path())
            .expect("verify install")
            .expect("receipt");
        assert_eq!(receipt.skill_targets, ["agents", "claude"]);
        assert_eq!(receipt.managed_files.len(), PUBLIC_SKILL_IDS.len() * 4);

        let second = plan_agent_install(project.path(), &pack, &["agents", "claude"])
            .expect("idempotent plan");
        assert!(second.writes.is_empty());
        assert!(second.removals.is_empty());
        assert!(
            second
                .actions
                .iter()
                .all(|action| action.action == "unchanged")
        );

        let (receipt, _) = plan_agent_uninstall(project.path())
            .expect("uninstall plan")
            .expect("installed");
        apply_agent_uninstall(project.path(), &receipt).expect("uninstall");
        assert!(!project.path().join(".agents").exists());
        assert!(!project.path().join(".claude").exists());
        assert!(!project.path().join(".wikitool-agent").exists());
    }

    #[test]
    fn install_and_uninstall_refuse_modified_or_foreign_skill_files() {
        let pack_root = tempfile::tempdir().expect("pack");
        let project = tempfile::tempdir().expect("project");
        write_test_pack(pack_root.path(), "0.8.0");
        let pack = load_agent_pack(pack_root.path()).expect("valid pack");
        let plan = plan_agent_install(project.path(), &pack, &["agents"]).expect("install plan");
        apply_agent_install(project.path(), plan).expect("install");

        let managed = project.path().join(".agents/skills/wiki-writing/SKILL.md");
        fs::write(&managed, "modified\n").expect("modify managed file");
        assert!(
            plan_agent_install(project.path(), &pack, &["agents"])
                .expect_err("modified setup must fail")
                .to_string()
                .contains("was modified")
        );
        assert!(
            plan_agent_uninstall(project.path())
                .expect_err("modified uninstall must fail")
                .to_string()
                .contains("was modified")
        );

        let source = pack.root.join("skills/wiki-writing/SKILL.md");
        fs::copy(source, &managed).expect("restore managed file");
        let foreign = project
            .path()
            .join(".agents/skills/wiki-writing/foreign.md");
        fs::write(&foreign, "foreign\n").expect("foreign file");
        assert!(
            plan_agent_install(project.path(), &pack, &["agents"])
                .expect_err("foreign setup must fail")
                .to_string()
                .contains("unowned file")
        );
    }

    #[test]
    fn pack_tampering_and_downgrades_fail_closed() {
        let first_root = tempfile::tempdir().expect("first pack");
        let older_root = tempfile::tempdir().expect("older pack");
        let project = tempfile::tempdir().expect("project");
        write_test_pack(first_root.path(), "0.8.0");
        write_test_pack(older_root.path(), "0.7.0");
        let first = load_agent_pack(first_root.path()).expect("valid first pack");
        let plan = plan_agent_install(project.path(), &first, &["agents"]).expect("install plan");
        apply_agent_install(project.path(), plan).expect("install");
        let older = load_agent_pack(older_root.path()).expect("valid older pack");
        assert!(
            plan_agent_install(project.path(), &older, &["agents"])
                .expect_err("downgrade must fail")
                .to_string()
                .contains("refuses downgrade")
        );

        fs::write(
            first_root.path().join("skills/wiki-writing/SKILL.md"),
            "tampered\n",
        )
        .expect("tamper pack");
        assert!(
            load_agent_pack(first_root.path())
                .expect_err("tampered pack must fail")
                .to_string()
                .contains("identity mismatch")
        );
    }
}
