use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok((sha256_bytes(&bytes), bytes.len() as u64))
}

pub fn unix_ms() -> Result<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_millis())
}

pub fn portable(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn relative_locator(root: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(root).with_context(|| {
        format!(
            "path {} is outside locator root {}",
            path.display(),
            root.display()
        )
    })?;
    Ok(portable(relative))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("path {} has no parent", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("wikitest"),
        std::process::id()
    ));
    if temporary.exists() {
        fs::remove_file(&temporary)
            .with_context(|| format!("failed to replace stale {}", temporary.display()))?;
    }
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .with_context(|| format!("failed to create {}", temporary.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", temporary.display()))?;
        drop(file);
        replace_file(&temporary, path).with_context(|| {
            format!(
                "failed to promote {} to {}",
                temporary.display(),
                path.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both buffers are NUL-terminated UTF-16 paths and live for the call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).context("failed to serialize JSON")?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

pub fn resolve_existing_plain_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let candidate = join_relative(root, relative)?;
    reject_symlink_components(root, &candidate)?;
    let metadata = fs::metadata(&candidate)
        .with_context(|| format!("missing input file {}", candidate.display()))?;
    if !metadata.is_file() {
        bail!("input is not a plain file: {}", candidate.display());
    }
    Ok(candidate)
}

pub fn resolve_output_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let candidate = join_relative(root, relative)?;
    if let Some(parent) = candidate.parent() {
        reject_symlink_components(root, parent)?;
    }
    if candidate.exists() && fs::symlink_metadata(&candidate)?.file_type().is_symlink() {
        bail!("output path is a symlink: {}", candidate.display());
    }
    Ok(candidate)
}

pub fn join_relative(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute() {
        bail!("expected relative path, got {}", path.display());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir if relative == "." => {}
            _ => bail!("unsafe relative path {relative}"),
        }
    }
    Ok(if relative == "." {
        root.to_path_buf()
    } else {
        root.join(path)
    })
}

fn reject_symlink_components(root: &Path, candidate: &Path) -> Result<()> {
    let relative = candidate.strip_prefix(root).with_context(|| {
        format!(
            "path {} escapes root {}",
            candidate.display(),
            root.display()
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        if current.exists() && fs::symlink_metadata(&current)?.file_type().is_symlink() {
            bail!(
                "symlinked scenario path is not allowed: {}",
                current.display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_lowercase_and_stable() {
        assert_eq!(
            sha256_bytes(b"wikitest"),
            "ccd9e4b198436f47e42cada7755bbb2a150cc75ab2d6757dfc8f1f511c2edab0"
        );
    }

    #[test]
    fn output_paths_cannot_escape() {
        let directory = tempfile::tempdir().expect("tempdir");
        assert!(resolve_output_path(directory.path(), "../escape").is_err());
    }

    #[test]
    fn atomic_json_replaces_complete_document() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("receipt.json");
        atomic_write_json(&path, &serde_json::json!({"complete": true})).expect("write");
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(path).expect("read")).expect("json");
        assert_eq!(value["complete"], true);
    }
}
