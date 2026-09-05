use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use walkdir::WalkDir;

use crate::artifact::{portable, resolve_existing_plain_file, sha256_file};
use crate::mediawiki::MediaWikiFixture;
use crate::model::{SCENARIO_SCHEMA, SUITE_SCHEMA, ScenarioManifest, ScenarioStep, SuiteManifest};
use crate::prose_model::{
    PROSE_ASSIGNMENT_SCHEMA, PROSE_SUITE_SCHEMA, ProseAssignment, ProseSuite,
};

#[derive(Debug, Clone)]
pub enum Manifest {
    Scenario(ScenarioManifest),
    Suite(SuiteManifest),
    ProseAssignment(Box<ProseAssignment>),
    ProseSuite(ProseSuite),
}

impl Manifest {
    pub fn id(&self) -> &str {
        match self {
            Self::Scenario(value) => &value.id,
            Self::Suite(value) => &value.id,
            Self::ProseAssignment(value) => &value.id,
            Self::ProseSuite(value) => &value.id,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Scenario(_) => "scenario",
            Self::Suite(_) => "suite",
            Self::ProseAssignment(_) => "prose_assignment",
            Self::ProseSuite(_) => "prose_suite",
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Scenario(value) => &value.title,
            Self::Suite(value) => &value.title,
            Self::ProseAssignment(value) => &value.title,
            Self::ProseSuite(value) => &value.title,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Scenario(value) => value.validate(),
            Self::Suite(value) => value.validate(),
            Self::ProseAssignment(value) => value.validate(),
            Self::ProseSuite(value) => value.validate(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub path: PathBuf,
    pub manifest: Manifest,
}

#[derive(Debug, Deserialize)]
struct SchemaProbe {
    schema: String,
    #[serde(flatten)]
    rest: BTreeMap<String, serde_json::Value>,
}

pub fn load_manifest(path: &Path) -> Result<(Manifest, Vec<u8>)> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect manifest {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "manifest must be a non-symlink plain file: {}",
            path.display()
        );
    }
    let bytes =
        fs::read(path).with_context(|| format!("failed to read manifest {}", path.display()))?;
    let probe: SchemaProbe = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid manifest JSON {}", path.display()))?;
    let _ = probe.rest;
    let manifest = match probe.schema.as_str() {
        SCENARIO_SCHEMA => Manifest::Scenario(
            serde_json::from_slice(&bytes)
                .with_context(|| format!("invalid scenario {}", path.display()))?,
        ),
        SUITE_SCHEMA => Manifest::Suite(
            serde_json::from_slice(&bytes)
                .with_context(|| format!("invalid suite {}", path.display()))?,
        ),
        PROSE_ASSIGNMENT_SCHEMA => Manifest::ProseAssignment(Box::new(
            serde_json::from_slice(&bytes)
                .with_context(|| format!("invalid prose assignment {}", path.display()))?,
        )),
        PROSE_SUITE_SCHEMA => Manifest::ProseSuite(
            serde_json::from_slice(&bytes)
                .with_context(|| format!("invalid prose suite {}", path.display()))?,
        ),
        schema => bail!(
            "unsupported manifest schema '{schema}' in {}",
            path.display()
        ),
    };
    manifest.validate()?;
    Ok((manifest, bytes))
}

/// Check immutable scenario inputs before any command executes. Execution still checks
/// each copied input again so edits made after preflight cannot silently enter a run.
pub fn validate_scenario_inputs(path: &Path, scenario: &ScenarioManifest) -> Result<()> {
    let root = path.parent().context("scenario has no parent directory")?;
    let verify = |source: &str, expected: &str| -> Result<PathBuf> {
        let input = resolve_existing_plain_file(root, source)?;
        let (actual, _) = sha256_file(&input)?;
        if actual != expected {
            bail!(
                "scenario '{}' input '{source}' SHA-256 mismatch: expected {expected}, observed {actual}",
                scenario.id
            );
        }
        Ok(input)
    };
    if let Some(fixture) = &scenario.mediawiki_fixture {
        MediaWikiFixture::from_path(&verify(&fixture.source, &fixture.sha256)?)?;
    }
    for step in &scenario.steps {
        if let ScenarioStep::Copy { source, sha256, .. } = step {
            verify(source, sha256)?;
        }
    }
    Ok(())
}

pub fn scan_catalogs(roots: &[PathBuf]) -> Result<Vec<CatalogEntry>> {
    let mut entries = Vec::new();
    let mut identities = BTreeMap::<(String, String), PathBuf>::new();
    for root in roots {
        if !root.is_dir() {
            bail!("catalog root does not exist: {}", root.display());
        }
        let walker = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0 || entry.file_name() != OsStr::new("evidence")
            });
        for entry in walker {
            let entry = entry.with_context(|| format!("failed to scan {}", root.display()))?;
            if !entry.file_type().is_file() {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy();
            if file_name != "scenario.json"
                && file_name != "suite.json"
                && file_name != "assignment.json"
                && file_name != "prose-suite.json"
            {
                continue;
            }
            let (manifest, _) = load_manifest(entry.path())?;
            let key = (manifest.kind().to_owned(), manifest.id().to_owned());
            if let Some(first) = identities.insert(key.clone(), entry.path().to_path_buf()) {
                bail!(
                    "duplicate {} id '{}' in {} and {}",
                    key.0,
                    key.1,
                    first.display(),
                    entry.path().display()
                );
            }
            entries.push(CatalogEntry {
                path: entry.path().to_path_buf(),
                manifest,
            });
        }
    }
    entries.sort_by(|left, right| {
        left.manifest
            .kind()
            .cmp(right.manifest.kind())
            .then_with(|| left.manifest.id().cmp(right.manifest.id()))
    });
    Ok(entries)
}

pub fn resolve_manifest(selector: &str, roots: &[PathBuf], expected_kind: &str) -> Result<PathBuf> {
    let explicit = PathBuf::from(selector);
    if explicit.exists() {
        let canonical = fs::canonicalize(&explicit)
            .with_context(|| format!("failed to resolve {}", explicit.display()))?;
        let (manifest, _) = load_manifest(&canonical)?;
        if manifest.kind() != expected_kind {
            bail!(
                "{} is a {}, expected {expected_kind}",
                canonical.display(),
                manifest.kind()
            );
        }
        return Ok(canonical);
    }
    let matches = scan_catalogs(roots)?
        .into_iter()
        .filter(|entry| entry.manifest.kind() == expected_kind && entry.manifest.id() == selector)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [entry] => fs::canonicalize(&entry.path)
            .with_context(|| format!("failed to resolve catalog entry {}", entry.path.display())),
        [] => bail!("no {expected_kind} named '{selector}' in configured catalogs"),
        _ => bail!("ambiguous {expected_kind} named '{selector}'"),
    }
}

pub fn discover_repository(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        let resolved = fs::canonicalize(path)
            .with_context(|| format!("failed to resolve repository root {}", path.display()))?;
        require_repository_shape(&resolved)?;
        return Ok(resolved);
    }
    let mut current = env::current_dir().context("failed to read current directory")?;
    loop {
        if require_repository_shape(&current).is_ok() {
            return fs::canonicalize(&current).context("failed to canonicalize repository root");
        }
        if !current.pop() {
            break;
        }
    }
    bail!("could not discover the wikitool source root; pass --repo-root")
}

fn require_repository_shape(path: &Path) -> Result<()> {
    if !path.join("Cargo.toml").is_file() || !path.join("crates/wikitest/Cargo.toml").is_file() {
        bail!("{} is not a wikitool source root", path.display());
    }
    Ok(())
}

pub fn resolve_wikitool(explicit: Option<&Path>, repository: &Path) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return require_executable_file(path);
    }
    if let Some(value) = env::var_os("WIKITOOL_BIN") {
        return require_executable_file(Path::new(&value));
    }

    let executable_name = if cfg!(windows) {
        "wikitool.exe"
    } else {
        "wikitool"
    };
    if let Ok(current) = env::current_exe()
        && let Some(parent) = current.parent()
    {
        let sibling = parent.join(executable_name);
        if sibling.is_file() {
            return require_executable_file(&sibling);
        }
    }
    for profile in ["debug", "release"] {
        let candidate = repository
            .join("target")
            .join(profile)
            .join(executable_name);
        if candidate.is_file() {
            return require_executable_file(&candidate);
        }
    }
    if let Some(path) = executable_on_path(executable_name) {
        return require_executable_file(&path);
    }
    bail!("could not locate wikitool; build it or pass --wikitool")
}

fn require_executable_file(path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve executable {}", path.display()))?;
    let metadata = fs::metadata(&canonical)?;
    if !metadata.is_file() {
        bail!("wikitool executable is not a file: {}", canonical.display());
    }
    Ok(canonical)
}

fn executable_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

pub fn default_catalog(repository: &Path) -> PathBuf {
    repository.join("wikitest")
}

pub fn display_catalog_path(repository: &Path, path: &Path) -> String {
    path.strip_prefix(repository)
        .map(portable)
        .unwrap_or_else(|_| portable(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn input_preflight_refuses_missing_changed_and_invalid_fixtures() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("scenario.json");
        let input = root.path().join("input.wiki");
        fs::write(&input, "retained bytes").unwrap();
        let (digest, _) = sha256_file(&input).unwrap();
        let mut scenario: ScenarioManifest = serde_json::from_value(serde_json::json!({
            "schema": SCENARIO_SCHEMA, "id":"preflight", "title":"Preflight",
            "description":"Refuse invalid inputs before execution.", "kind":"mechanical",
            "environment":"isolated", "timeout_ms":1000, "coverage":[],
            "steps":[{"action":"copy", "id":"input", "source":"input.wiki", "target":"copy.wiki", "sha256":digest}]
        })).unwrap();
        validate_scenario_inputs(&path, &scenario).unwrap();
        fs::write(&input, "changed bytes").unwrap();
        assert!(
            validate_scenario_inputs(&path, &scenario)
                .unwrap_err()
                .to_string()
                .contains("SHA-256 mismatch")
        );
        fs::remove_file(&input).unwrap();
        assert!(validate_scenario_inputs(&path, &scenario).is_err());

        fs::write(&input, "{}").unwrap();
        scenario.steps.clear();
        scenario.mediawiki_fixture = Some(crate::model::MediaWikiFixtureRef {
            source: "input.wiki".into(),
            sha256: sha256_file(&input).unwrap().0,
        });
        assert!(
            validate_scenario_inputs(&path, &scenario)
                .unwrap_err()
                .to_string()
                .contains("invalid MediaWiki fixture")
        );
    }

    #[test]
    fn catalog_refuses_duplicate_ids() {
        let directory = tempfile::tempdir().expect("tempdir");
        for name in ["one", "two"] {
            let path = directory.path().join(name);
            fs::create_dir_all(&path).expect("directory");
            let mut file = fs::File::create(path.join("scenario.json")).expect("manifest");
            write!(
                file,
                "{{\"schema\":\"{SCENARIO_SCHEMA}\",\"id\":\"same\",\"title\":\"Same\",\"description\":\"Same scenario.\",\"kind\":\"mechanical\",\"environment\":\"isolated\",\"timeout_ms\":1000,\"coverage\":[{{\"capability\":\"public-cli\",\"steps\":[\"status\"]}}],\"steps\":[{{\"action\":\"command\",\"id\":\"status\",\"argv\":[\"status\"],\"expect\":{{\"exit_code\":0}}}}]}}"
            )
            .expect("write manifest");
        }
        let error = scan_catalogs(&[directory.path().to_path_buf()]).expect_err("duplicates");
        assert!(
            error.to_string().contains("duplicate scenario id"),
            "{error:#}"
        );
    }

    #[test]
    fn catalog_excludes_retained_evidence_trees() {
        let directory = tempfile::tempdir().expect("tempdir");
        for parent in ["live", "evidence/archived/inputs"] {
            let path = directory.path().join(parent);
            fs::create_dir_all(&path).expect("directory");
            let mut file = fs::File::create(path.join("scenario.json")).expect("manifest");
            write!(
                file,
                "{{\"schema\":\"{SCENARIO_SCHEMA}\",\"id\":\"same\",\"title\":\"Same\",\"description\":\"Same scenario.\",\"kind\":\"mechanical\",\"environment\":\"isolated\",\"timeout_ms\":1000,\"coverage\":[{{\"capability\":\"public-cli\",\"steps\":[\"status\"]}}],\"steps\":[{{\"action\":\"command\",\"id\":\"status\",\"argv\":[\"status\"],\"expect\":{{\"exit_code\":0}}}}]}}"
            )
            .expect("write manifest");
        }

        let entries = scan_catalogs(&[directory.path().to_path_buf()]).expect("scan catalog");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].manifest.id(), "same");
        assert!(entries[0].path.ends_with("live/scenario.json"));
    }
}
