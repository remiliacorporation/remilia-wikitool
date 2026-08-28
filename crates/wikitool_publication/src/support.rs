use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

pub(crate) fn compute_sha256(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn normalize_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

pub(crate) fn unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before the Unix epoch")?
        .as_secs())
}

pub(crate) fn atomic_write(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary file in {}", parent.display()))?;
    temporary
        .write_all(content.as_ref())
        .with_context(|| format!("failed to write temporary file for {}", path.display()))?;
    temporary
        .flush()
        .with_context(|| format!("failed to flush temporary file for {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

pub(crate) fn parse_redirect(content: &str) -> (bool, Option<String>) {
    let trimmed = content.trim_start();
    let upper = trimmed.to_ascii_uppercase();
    if !upper.starts_with("#REDIRECT") {
        return (false, None);
    }
    let target = trimmed
        .find("[[")
        .and_then(|start| {
            trimmed[start + 2..]
                .find("]]")
                .map(|end| &trimmed[start + 2..start + 2 + end])
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    (true, target)
}

pub(crate) fn resolve_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let normalized = normalize_pathbuf(path);
    if normalized.exists() {
        return fs::canonicalize(&normalized)
            .map(|path| normalize_pathbuf(&path))
            .with_context(|| format!("failed to canonicalize {}", normalized.display()));
    }
    let mut cursor = normalized.as_path();
    let mut suffix = Vec::<OsString>::new();
    loop {
        if cursor.exists() {
            let mut resolved = fs::canonicalize(cursor)
                .map(|path| normalize_pathbuf(&path))
                .with_context(|| format!("failed to canonicalize {}", cursor.display()))?;
            for segment in suffix.iter().rev() {
                resolved.push(segment);
            }
            return Ok(normalize_pathbuf(&resolved));
        }
        let Some(name) = cursor.file_name() else {
            return Ok(normalized);
        };
        suffix.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            return Ok(normalized);
        };
        cursor = parent;
    }
}

fn normalize_pathbuf(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
