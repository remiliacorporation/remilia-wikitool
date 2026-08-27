use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::artifact::{portable, sha256_file};
use crate::model::ToolIdentity;

pub fn current_driver_identity(repository: &Path) -> Result<ToolIdentity> {
    let executable = env::current_exe().context("failed to locate the running wikitest binary")?;
    let executable = fs::canonicalize(&executable).with_context(|| {
        format!(
            "failed to resolve the running wikitest binary {}",
            executable.display()
        )
    })?;
    let (sha256, _) = sha256_file(&executable)?;
    let locator = executable
        .strip_prefix(repository)
        .map(portable)
        .unwrap_or_else(|_| portable(&executable));
    Ok(ToolIdentity {
        locator,
        sha256,
        version: format!("wikitest {}", env!("CARGO_PKG_VERSION")),
    })
}

pub fn verify_recorded_identity(
    repository: &Path,
    identity: &ToolIdentity,
    label: &str,
) -> Result<()> {
    let candidate = PathBuf::from(&identity.locator);
    let path = if candidate.is_absolute() {
        candidate
    } else {
        repository.join(candidate)
    };
    verify_path_identity(&path, identity, label)
}

pub fn verify_path_identity(path: &Path, identity: &ToolIdentity, label: &str) -> Result<()> {
    let path = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve {label} binary {}", path.display()))?;
    let (observed, _) = sha256_file(&path)?;
    if observed != identity.sha256 {
        bail!(
            "{label} binary changed after the receipt: expected {}, observed {} at {}",
            identity.sha256,
            observed,
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn recorded_identity_detects_binary_changes() {
        let root = tempdir().expect("tempdir");
        let binary = root.path().join("driver.bin");
        fs::write(&binary, b"first driver").expect("write driver");
        let (sha256, _) = sha256_file(&binary).expect("hash driver");
        let identity = ToolIdentity {
            locator: portable(&binary),
            sha256,
            version: "wikitest test".to_owned(),
        };

        verify_recorded_identity(root.path(), &identity, "driver").expect("identity matches");
        fs::write(&binary, b"changed driver").expect("change driver");
        let error = verify_recorded_identity(root.path(), &identity, "driver")
            .expect_err("changed identity must fail");
        assert!(error.to_string().contains("changed after the receipt"));
    }

    #[test]
    fn current_driver_identity_is_self_verifiable() {
        let root = tempdir().expect("tempdir");
        let identity = current_driver_identity(root.path()).expect("driver identity");
        assert!(identity.version.starts_with("wikitest "));
        verify_recorded_identity(root.path(), &identity, "driver").expect("identity verifies");
    }
}
