use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct CapturedStream {
    pub sha256: String,
    pub stored_sha256: String,
    pub observed_bytes: u64,
    pub stored_bytes: u64,
    pub truncated: bool,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct ProcessOutcome {
    pub status: ExitStatus,
    pub timed_out: bool,
    pub duration_ms: u128,
    pub stdout: CapturedStream,
    pub stderr: CapturedStream,
}

#[allow(clippy::too_many_arguments)]
pub fn run_bounded(
    executable: &Path,
    arguments: &[String],
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    timeout: Duration,
    maximum_output_bytes: usize,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<ProcessOutcome> {
    if timeout.is_zero() {
        bail!("process timeout must be nonzero");
    }
    if maximum_output_bytes == 0 {
        bail!("process output budget must be nonzero");
    }
    if let Some(parent) = stdout_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = stderr_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(cwd)
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to start {} with arguments {:?}",
            executable.display(),
            arguments
        )
    })?;
    let stdout = child.stdout.take().context("child stdout was not piped")?;
    let stderr = child.stderr.take().context("child stderr was not piped")?;
    let stdout_capture = spawn_capture(stdout, stdout_path.to_path_buf(), maximum_output_bytes);
    let stderr_capture = spawn_capture(stderr, stderr_path.to_path_buf(), maximum_output_bytes);

    let deadline = started + timeout;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().context("failed to poll child process")? {
            break (status, false);
        }
        if Instant::now() >= deadline {
            child
                .kill()
                .context("failed to terminate timed-out child")?;
            break (
                child
                    .wait()
                    .context("failed to reap timed-out child process")?,
                true,
            );
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdout = stdout_capture
        .join()
        .map_err(|_| anyhow::anyhow!("stdout capture thread panicked"))??;
    let stderr = stderr_capture
        .join()
        .map_err(|_| anyhow::anyhow!("stderr capture thread panicked"))??;
    Ok(ProcessOutcome {
        status,
        timed_out,
        duration_ms: started.elapsed().as_millis(),
        stdout,
        stderr,
    })
}

fn spawn_capture<R>(
    mut reader: R,
    path: std::path::PathBuf,
    maximum_bytes: usize,
) -> thread::JoinHandle<Result<CapturedStream>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut file = fs::File::create(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        let mut hasher = Sha256::new();
        let mut observed_bytes = 0_u64;
        let mut stored = Vec::with_capacity(maximum_bytes.min(64 * 1024));
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let count = reader
                .read(&mut buffer)
                .with_context(|| format!("failed to capture stream into {}", path.display()))?;
            if count == 0 {
                break;
            }
            observed_bytes = observed_bytes.saturating_add(count as u64);
            hasher.update(&buffer[..count]);
            let remaining = maximum_bytes.saturating_sub(stored.len());
            let retained = count.min(remaining);
            if retained > 0 {
                file.write_all(&buffer[..retained])?;
                stored.extend_from_slice(&buffer[..retained]);
            }
        }
        file.flush()?;
        file.sync_all()?;
        Ok(CapturedStream {
            sha256: format!("{:x}", hasher.finalize()),
            stored_sha256: format!("{:x}", Sha256::digest(&stored)),
            observed_bytes,
            stored_bytes: stored.len() as u64,
            truncated: observed_bytes > stored.len() as u64,
            bytes: stored,
        })
    })
}

pub fn probe_version(
    executable: &Path,
    cwd: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<String> {
    let outcome = run_bounded(
        executable,
        &["--version".to_owned()],
        cwd,
        &BTreeMap::new(),
        Duration::from_secs(5),
        64 * 1024,
        stdout_path,
        stderr_path,
    )?;
    if outcome.timed_out || !outcome.status.success() || outcome.stdout.truncated {
        bail!("wikitool --version did not produce a complete successful result");
    }
    let version = String::from_utf8(outcome.stdout.bytes)
        .context("wikitool --version output was not UTF-8")?
        .trim()
        .to_owned();
    if version.is_empty() {
        bail!("wikitool --version returned an empty version");
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_output_budget_is_rejected_before_spawn() {
        let directory = tempfile::tempdir().expect("tempdir");
        let error = run_bounded(
            Path::new("not-used"),
            &[],
            directory.path(),
            &BTreeMap::new(),
            Duration::from_secs(1),
            0,
            &directory.path().join("stdout"),
            &directory.path().join("stderr"),
        )
        .expect_err("zero budget");
        assert!(error.to_string().contains("budget"));
    }
}
