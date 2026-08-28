use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::{Deserialize, Serialize};

use crate::cli_support::{OutputFormat, normalize_path};

const COMPANION_MANIFEST_SCHEMA: &str = "wikitool.release-companions.v1";

#[derive(Debug, Args)]
pub(crate) struct CompanionsArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Inspect this release-companions.json instead of the file beside the executable"
    )]
    manifest: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Deserialize)]
struct ReleaseCompanionManifest {
    schema: String,
    companions: Vec<CompanionDeclaration>,
}

#[derive(Debug, Deserialize)]
struct CompanionDeclaration {
    id: String,
    version: String,
    #[serde(default)]
    source_commit: Option<String>,
    required_for_wikitool: bool,
    project_lifecycle_owner: String,
    #[serde(default)]
    binary: Option<String>,
    #[serde(default)]
    planner_binary: Option<String>,
    #[serde(default)]
    mise_binary: Option<String>,
    manifest: String,
    #[serde(default)]
    agent_contract: Option<String>,
}

#[derive(Debug, Serialize)]
struct CompanionDiagnostics {
    schema: &'static str,
    status: &'static str,
    manifest_path: String,
    wikitool_available: bool,
    companions: Vec<CompanionStatus>,
}

#[derive(Debug, Serialize)]
struct CompanionStatus {
    id: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_commit: Option<String>,
    required_for_wikitool: bool,
    project_lifecycle_owner: String,
    status: &'static str,
    declared_paths: Vec<String>,
    missing_paths: Vec<String>,
}

pub(crate) fn run_companions(args: CompanionsArgs) -> Result<()> {
    let manifest_path = match args.manifest {
        Some(path) => path,
        None => std::env::current_exe()
            .context("failed to resolve the Wikitool executable path")?
            .parent()
            .context("Wikitool executable path has no parent directory")?
            .join("release-companions.json"),
    };
    let diagnostics = inspect_companions(&manifest_path)?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&diagnostics)?);
    } else {
        println!("companion diagnostics: {}", diagnostics.status);
        println!("manifest: {}", diagnostics.manifest_path);
        for companion in &diagnostics.companions {
            println!(
                "{} {}: {} (lifecycle owner: {})",
                companion.id,
                companion.version,
                companion.status,
                companion.project_lifecycle_owner
            );
            for missing in &companion.missing_paths {
                println!("  missing: {missing}");
            }
        }
    }
    Ok(())
}

fn inspect_companions(manifest_path: &Path) -> Result<CompanionDiagnostics> {
    let manifest_metadata = match fs::symlink_metadata(manifest_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(CompanionDiagnostics {
                schema: "wikitool.companion-diagnostics.v1",
                status: "optional_manifest_absent",
                manifest_path: normalize_path(manifest_path),
                wikitool_available: true,
                companions: Vec::new(),
            });
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect {}", normalize_path(manifest_path)));
        }
    };
    if manifest_metadata.file_type().is_symlink() || !manifest_metadata.is_file() {
        bail!(
            "companion manifest must be a regular non-symlink file: {}",
            normalize_path(manifest_path)
        );
    }
    let text = fs::read_to_string(manifest_path)
        .with_context(|| format!("failed to read {}", normalize_path(manifest_path)))?;
    let manifest: ReleaseCompanionManifest = serde_json::from_str(&text)
        .with_context(|| format!("invalid JSON in {}", normalize_path(manifest_path)))?;
    if manifest.schema != COMPANION_MANIFEST_SCHEMA {
        bail!(
            "companion manifest schema is {:?}, expected {:?}",
            manifest.schema,
            COMPANION_MANIFEST_SCHEMA
        );
    }

    let root = manifest_path
        .parent()
        .context("companion manifest path has no parent directory")?;
    let mut ids = BTreeSet::new();
    let mut companions = Vec::with_capacity(manifest.companions.len());
    for declaration in manifest.companions {
        validate_declaration(&declaration, &mut ids)?;
        let declared_paths = declaration_paths(&declaration);
        let mut missing_paths = Vec::new();
        for relative in &declared_paths {
            validate_relative_path(relative)?;
            let candidate = root.join(relative);
            match fs::symlink_metadata(&candidate) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
                Ok(_) => missing_paths.push(relative.clone()),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    missing_paths.push(relative.clone());
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to inspect companion path {}",
                            normalize_path(&candidate)
                        )
                    });
                }
            }
        }
        let status = if missing_paths.is_empty() {
            "available"
        } else {
            "files_missing"
        };
        companions.push(CompanionStatus {
            id: declaration.id,
            version: declaration.version,
            source_commit: declaration.source_commit,
            required_for_wikitool: declaration.required_for_wikitool,
            project_lifecycle_owner: declaration.project_lifecycle_owner,
            status,
            declared_paths,
            missing_paths,
        });
    }
    let status = if companions.iter().all(|item| item.status == "available") {
        "ok"
    } else {
        "optional_files_missing"
    };
    Ok(CompanionDiagnostics {
        schema: "wikitool.companion-diagnostics.v1",
        status,
        manifest_path: normalize_path(manifest_path),
        wikitool_available: true,
        companions,
    })
}

fn validate_declaration(
    declaration: &CompanionDeclaration,
    ids: &mut BTreeSet<String>,
) -> Result<()> {
    if declaration.id.trim().is_empty() || declaration.version.trim().is_empty() {
        bail!("companion id and version must be nonempty");
    }
    if !ids.insert(declaration.id.clone()) {
        bail!("duplicate companion id {:?}", declaration.id);
    }
    if declaration.required_for_wikitool {
        bail!(
            "companion {:?} declares required_for_wikitool=true; release companions must remain optional",
            declaration.id
        );
    }
    if declaration.project_lifecycle_owner != declaration.id {
        bail!(
            "companion {:?} lifecycle owner is {:?}; the companion must retain its own lifecycle authority",
            declaration.id,
            declaration.project_lifecycle_owner
        );
    }
    Ok(())
}

fn declaration_paths(declaration: &CompanionDeclaration) -> Vec<String> {
    let mut paths = Vec::new();
    for path in [
        declaration.binary.as_ref(),
        declaration.planner_binary.as_ref(),
        declaration.mise_binary.as_ref(),
        Some(&declaration.manifest),
        declaration.agent_contract.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if !paths.contains(path) {
            paths.push(path.clone());
        }
    }
    paths
}

fn validate_relative_path(path: &str) -> Result<()> {
    let candidate = Path::new(path);
    if path.is_empty()
        || candidate.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        bail!("companion path must remain relative to the release bundle: {path:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_manifest_is_an_optional_state() {
        let root = tempfile::tempdir().expect("tempdir");
        let diagnostics = inspect_companions(&root.path().join("missing.json")).expect("inspect");
        assert_eq!(diagnostics.status, "optional_manifest_absent");
        assert!(diagnostics.wikitool_available);
        assert!(diagnostics.companions.is_empty());
    }

    #[test]
    fn companion_paths_cannot_escape_the_release_bundle() {
        assert!(validate_relative_path("papertiger/papertiger").is_ok());
        assert!(validate_relative_path("../papertiger").is_err());
        assert!(validate_relative_path("/tmp/papertiger").is_err());
    }

    #[test]
    fn companion_cannot_transfer_lifecycle_authority_to_wikitool() {
        let declaration = CompanionDeclaration {
            id: "papertiger".to_string(),
            version: "0.9.0".to_string(),
            source_commit: None,
            required_for_wikitool: false,
            project_lifecycle_owner: "wikitool".to_string(),
            binary: None,
            planner_binary: Some("papertiger/papertiger".to_string()),
            mise_binary: None,
            manifest: "papertiger/manifest.json".to_string(),
            agent_contract: None,
        };
        assert!(validate_declaration(&declaration, &mut BTreeSet::new()).is_err());
    }
}
