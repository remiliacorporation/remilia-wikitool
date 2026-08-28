use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const PROCESS_TERMINATION_GRACE: Duration = Duration::from_secs(5);
const CAPTURE_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

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
    for (key, _) in std::env::vars_os() {
        if is_wikitool_environment_key(&key) {
            command.env_remove(key);
        }
    }
    command
        .args(arguments)
        .current_dir(cwd)
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_tree(&mut command);
    let started = Instant::now();
    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to start {} with arguments {:?}",
            executable.display(),
            arguments
        )
    })?;
    let process_tree = match ProcessTree::attach(&child) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let _ = child.kill();
            let _ = wait_for_child_exit(&mut child, PROCESS_TERMINATION_GRACE);
            return Err(error);
        }
    };
    if let Err(error) = process_tree.resume(&child) {
        let _ = process_tree.terminate();
        let _ = wait_for_child_exit(&mut child, PROCESS_TERMINATION_GRACE);
        return Err(error);
    }
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
            process_tree
                .terminate()
                .context("failed to terminate timed-out process tree")?;
            let status = wait_for_child_exit(&mut child, PROCESS_TERMINATION_GRACE)?
                .context("timed-out process tree did not terminate within the grace period")?;
            break (status, true);
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    };
    process_tree
        .terminate()
        .context("failed to terminate descendant processes after command exit")?;

    let stdout = stdout_capture
        .finish(CAPTURE_DRAIN_TIMEOUT, "stdout")
        .context("stdout capture did not finish after process-tree termination")?;
    let stderr = stderr_capture
        .finish(CAPTURE_DRAIN_TIMEOUT, "stderr")
        .context("stderr capture did not finish after process-tree termination")?;
    Ok(ProcessOutcome {
        status,
        timed_out,
        duration_ms: started.elapsed().as_millis(),
        stdout,
        stderr,
    })
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().context("failed to poll child process")? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

#[cfg(unix)]
fn configure_process_tree(command: &mut Command) {
    // SAFETY: setpgid is async-signal-safe and the closure performs no allocation.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(unix)]
struct ProcessTree {
    process_group: libc::pid_t,
}

#[cfg(unix)]
impl ProcessTree {
    fn attach(child: &Child) -> Result<Self> {
        let process_group = libc::pid_t::try_from(child.id()).context("child PID exceeds pid_t")?;
        Ok(Self { process_group })
    }

    fn terminate(&self) -> Result<()> {
        // SAFETY: the negative PID addresses only the process group created in pre_exec.
        let result = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error).context("failed to kill Unix process group")
        }
    }

    fn resume(&self, _child: &Child) -> Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
fn configure_process_tree(command: &mut Command) {
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
}

#[cfg(windows)]
struct ProcessTree {
    job: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl ProcessTree {
    fn attach(child: &Child) -> Result<Self> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        // SAFETY: null security/name pointers request an unnamed job with default security.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error()).context("failed to create Windows job");
        }
        let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: information points to the declared structure for the selected information class.
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(information).cast(),
                std::mem::size_of_val(&information) as u32,
            )
        };
        if configured == 0 {
            let error = std::io::Error::last_os_error();
            close_job(job);
            return Err(error).context("failed to configure Windows job");
        }
        // SAFETY: Child owns a valid process handle for the newly spawned process.
        let assigned = unsafe {
            AssignProcessToJobObject(
                job,
                child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
            )
        };
        if assigned == 0 {
            let error = std::io::Error::last_os_error();
            close_job(job);
            return Err(error).context("failed to assign child process to Windows job");
        }
        Ok(Self { job })
    }

    fn resume(&self, child: &Child) -> Result<()> {
        resume_suspended_process(child.id())
    }

    fn terminate(&self) -> Result<()> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        // SAFETY: self.job remains valid until Drop closes it.
        if unsafe { TerminateJobObject(self.job, 1) } == 0 {
            Err(std::io::Error::last_os_error()).context("failed to terminate Windows job")
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
fn resume_suspended_process(process_id: u32) -> Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    // The child was created with CREATE_SUSPENDED, so it cannot spawn descendants
    // before its threads are enumerated and the process is attached to the job.
    // SAFETY: the snapshot call has no pointer arguments and returns an owned handle.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error())
            .context("failed to snapshot threads for suspended Windows child");
    }

    let thread_ids = (|| -> Result<Vec<u32>> {
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..THREADENTRY32::default()
        };
        // SAFETY: snapshot is valid and entry advertises the exact structure size.
        if unsafe { Thread32First(snapshot, std::ptr::addr_of_mut!(entry)) } == 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to enumerate threads for suspended Windows child");
        }
        let mut thread_ids = Vec::new();
        loop {
            if entry.th32OwnerProcessID == process_id {
                thread_ids.push(entry.th32ThreadID);
            }
            // SAFETY: snapshot and entry remain valid for the enumeration lifetime.
            if unsafe { Thread32Next(snapshot, std::ptr::addr_of_mut!(entry)) } == 0 {
                break;
            }
        }
        if thread_ids.is_empty() {
            bail!("suspended Windows child {process_id} had no enumerable threads");
        }
        Ok(thread_ids)
    })();
    // SAFETY: snapshot is an owned snapshot handle and is closed exactly once.
    unsafe { CloseHandle(snapshot) };

    for thread_id in thread_ids? {
        // SAFETY: the requested access is limited to resuming the enumerated child thread.
        let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
        if thread.is_null() {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("failed to open suspended Windows thread {thread_id}"));
        }
        // SAFETY: thread is a valid owned thread handle opened with resume access.
        let previous_suspend_count = unsafe { ResumeThread(thread) };
        let resume_error = if previous_suspend_count == u32::MAX {
            Some(std::io::Error::last_os_error())
        } else if previous_suspend_count == 0 {
            Some(std::io::Error::other(format!(
                "Windows thread {thread_id} was not suspended before job attachment"
            )))
        } else {
            None
        };
        // SAFETY: thread is an owned thread handle and is closed exactly once.
        unsafe { CloseHandle(thread) };
        if let Some(error) = resume_error {
            return Err(error).context("failed to resume job-bound Windows child");
        }
    }
    Ok(())
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        close_job(self.job);
    }
}

#[cfg(windows)]
fn close_job(job: windows_sys::Win32::Foundation::HANDLE) {
    // SAFETY: job is an owned job handle created by CreateJobObjectW.
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(job);
    }
}

fn is_wikitool_environment_key(key: &OsStr) -> bool {
    key.to_string_lossy()
        .to_ascii_uppercase()
        .starts_with("WIKITOOL_")
}

fn spawn_capture<R>(mut reader: R, path: std::path::PathBuf, maximum_bytes: usize) -> CaptureTask
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = capture_stream(&mut reader, &path, maximum_bytes);
        let _ = sender.send(result);
    });
    CaptureTask { receiver }
}

struct CaptureTask {
    receiver: Receiver<Result<CapturedStream>>,
}

impl CaptureTask {
    fn finish(self, timeout: Duration, stream: &str) -> Result<CapturedStream> {
        match self.receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                bail!("{stream} capture exceeded its bounded drain timeout")
            }
            Err(RecvTimeoutError::Disconnected) => {
                bail!("{stream} capture thread terminated without a result")
            }
        }
    }
}

fn capture_stream<R>(reader: &mut R, path: &Path, maximum_bytes: usize) -> Result<CapturedStream>
where
    R: Read,
{
    let mut file =
        fs::File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
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
    use std::ffi::OsString;
    use std::sync::Mutex;

    static ENVIRONMENT_TEST_LOCK: Mutex<()> = Mutex::new(());

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

    #[test]
    fn wikitool_environment_keys_are_scrubbed_case_insensitively() {
        assert!(is_wikitool_environment_key(OsStr::new(
            "WIKITOOL_WIKI_API_URL"
        )));
        assert!(is_wikitool_environment_key(OsStr::new("wikitool_data_dir")));
        assert!(!is_wikitool_environment_key(OsStr::new("PATH")));
    }

    #[test]
    fn hostile_wikitool_api_environment_is_not_inherited() {
        if std::env::var_os("WIKITEST_ENVIRONMENT_CHILD").is_some() {
            assert!(std::env::var_os("WIKITOOL_WIKI_API_URL").is_none());
            return;
        }
        let _guard = ENVIRONMENT_TEST_LOCK.lock().expect("environment lock");
        let previous = std::env::var_os("WIKITOOL_WIKI_API_URL");
        // SAFETY: this test serializes its mutation and restores the prior value
        // before observing the child result.
        unsafe {
            std::env::set_var("WIKITOOL_WIKI_API_URL", "https://hostile.invalid/api.php");
        }
        let directory = tempfile::tempdir().expect("tempdir");
        let environment =
            BTreeMap::from([("WIKITEST_ENVIRONMENT_CHILD".to_owned(), "1".to_owned())]);
        let result = run_bounded(
            &std::env::current_exe().expect("test executable"),
            &[
                "--exact".to_owned(),
                "process::tests::hostile_wikitool_api_environment_is_not_inherited".to_owned(),
                "--nocapture".to_owned(),
            ],
            directory.path(),
            &environment,
            Duration::from_secs(10),
            64 * 1024,
            &directory.path().join("stdout"),
            &directory.path().join("stderr"),
        );
        restore_environment("WIKITOOL_WIKI_API_URL", previous);
        let outcome = result.expect("child process");
        assert!(outcome.status.success());
        assert!(!outcome.timed_out);
    }

    #[test]
    fn timeout_terminates_descendant_tree_and_bounded_pipe_capture() {
        match std::env::var("WIKITEST_PROCESS_TREE_ROLE").as_deref() {
            Ok("descendant") => {
                thread::sleep(Duration::from_secs(30));
                return;
            }
            Ok("parent") => {
                let marker = std::env::var("WIKITEST_PROCESS_TREE_MARKER")
                    .expect("process-tree marker path");
                let mut descendant = Command::new(
                    std::env::current_exe().expect("test executable"),
                )
                .args([
                    "--exact",
                    "process::tests::timeout_terminates_descendant_tree_and_bounded_pipe_capture",
                    "--nocapture",
                ])
                .env("WIKITEST_PROCESS_TREE_ROLE", "descendant")
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("spawn descendant holding inherited pipes");
                fs::write(marker, b"spawned\n").expect("write descendant marker");
                thread::sleep(Duration::from_secs(30));
                let _ = descendant.wait();
                return;
            }
            _ => {}
        }

        let directory = tempfile::tempdir().expect("tempdir");
        let marker = directory.path().join("descendant-spawned");
        let environment = BTreeMap::from([
            ("WIKITEST_PROCESS_TREE_ROLE".to_owned(), "parent".to_owned()),
            (
                "WIKITEST_PROCESS_TREE_MARKER".to_owned(),
                marker.to_string_lossy().into_owned(),
            ),
        ]);
        let started = Instant::now();
        let outcome = run_bounded(
            &std::env::current_exe().expect("test executable"),
            &[
                "--exact".to_owned(),
                "process::tests::timeout_terminates_descendant_tree_and_bounded_pipe_capture"
                    .to_owned(),
                "--nocapture".to_owned(),
            ],
            directory.path(),
            &environment,
            Duration::from_secs(2),
            64 * 1024,
            &directory.path().join("stdout"),
            &directory.path().join("stderr"),
        )
        .expect("bounded process tree");
        assert!(
            marker.exists(),
            "child did not spawn the pipe-holding descendant"
        );
        assert!(outcome.timed_out);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "process-tree timeout exceeded the bounded teardown budget"
        );
    }

    fn restore_environment(key: &str, value: Option<OsString>) {
        // SAFETY: caller holds ENVIRONMENT_TEST_LOCK and restores exactly the
        // value that preceded the test mutation.
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
