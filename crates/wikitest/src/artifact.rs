use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("failed to open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes = 0;
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to hash {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), bytes))
}

pub fn unix_ms() -> Result<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_millis())
}

pub fn portable(path: &Path) -> String {
    let value = path.to_string_lossy();
    let value = value
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| value.strip_prefix(r"\\?\").map(str::to_owned))
        .unwrap_or_else(|| value.into_owned());
    value.replace('\\', "/")
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

    let source = windows_extended_file_path(source)?
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = windows_extended_file_path(destination)?
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

#[cfg(windows)]
fn windows_extended_file_path(path: &Path) -> std::io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path {} has no parent", path.display()),
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path {} has no file name", path.display()),
        )
    })?;
    let mut extended = fs::canonicalize(parent)?;
    extended.push(file_name);
    Ok(extended)
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
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("artifact");
        for (length, digest) in [
            (
                0,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                131089,
                "e3c33f1a7c00a23610a13fa6b862df2882931be5b0262aeada7a47e282f1c679",
            ),
        ] {
            fs::write(&path, vec![b'x'; length]).unwrap();
            assert_eq!(sha256_file(&path).unwrap(), (digest.into(), length as u64));
        }
    }

    #[test]
    fn portable_paths_hide_windows_verbatim_prefixes() {
        assert_eq!(portable(Path::new(r"\\?\F:\AI\wiki")), "F:/AI/wiki");
        assert_eq!(
            portable(Path::new(r"\\?\UNC\server\share\wiki")),
            "//server/share/wiki"
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

    #[cfg(windows)]
    #[test]
    fn atomic_write_replaces_files_beyond_the_legacy_windows_path_limit() {
        use std::os::windows::ffi::OsStrExt;

        let directory = tempfile::tempdir().expect("tempdir");
        let mut path = directory.path().to_path_buf();
        for index in 0..6 {
            path.push(format!("segment-{index}-{}", "x".repeat(40)));
        }
        path.push("receipt.json");
        assert!(path.as_os_str().encode_wide().count() > 260);

        atomic_write(&path, b"prepared\n").expect("initial long-path write");
        atomic_write(&path, b"evaluated\n").expect("replacement long-path write");
        assert_eq!(fs::read(path).expect("long-path receipt"), b"evaluated\n");
    }
}
