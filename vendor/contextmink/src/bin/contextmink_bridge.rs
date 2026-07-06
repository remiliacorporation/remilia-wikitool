//! Native PowerShell -> Git Bash bridge.
//!
//! A PowerShell-hosted agent on Windows cannot reach Git Bash natively and
//! PowerShell 5.1 marshals argv to native processes lossily (embedded quotes
//! vanish and arguments merge). No receiver can reconstruct that loss, so the
//! bridge offers channels that avoid it instead:
//!
//! - `--argv-b64 <token>`: the whole argv as one base64 token (UTF-8 args,
//!   NUL-separated). A single token without spaces or quotes survives every
//!   PowerShell version verbatim. This is the preferred agent channel.
//! - `--argfile <file>`: one argument per line, no quoting.
//! - `-- <program> [args...]`: plain argv for arguments known to be tame.
//!
//! Direct argv modes spawn the child natively — no MSYS layer exists, so
//! slash-bearing arguments (`^// PART`) are never rewritten. A program
//! spelled as a path (`./gradlew`) resolves against `--cwd` like a POSIX
//! exec; bare names use PATH. `--script` and `--login` locate Git Bash
//! themselves (no hardcoded path needed on the agent side), and
//! extensionless Bash scripts passed as the program are retried through Git
//! Bash automatically.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

#[path = "../config.rs"]
mod config;
#[path = "../destructive_guard.rs"]
mod destructive_guard;

const EXIT_USAGE: i32 = 64;
const EXIT_MISSING_PATH: i32 = 66;
const EXIT_SPAWN_FAILED: i32 = 126;
const EXIT_NO_BASH: i32 = 127;

/// Threshold tracks the contextmink slice window guidance in repository
/// bounded-output instructions (same knob as the codex-bash.sh template).
const DUMP_WARN_LINES: usize = 150;
const SUPPRESS_ENV: &str = "CODEX_BASH_SUPPRESS_DUMP_WARNING";

fn usage() -> String {
    "Usage:\n\
     \x20 contextmink-bridge [flags] -- <program> [args...]\n\
     \x20 contextmink-bridge [flags] --script <script> [args...]\n\
     \x20 contextmink-bridge [flags] --argfile <file>\n\
     \x20 contextmink-bridge [flags] --argv-b64 <token>\n\
     \n\
     Flags (must precede the command form):\n\
     \x20 --cwd <dir>     Working directory; relative paths resolve from the\n\
     \x20                 bridge root (CONTEXTMINK_BRIDGE_ROOT; else the\n\
     \x20                 nearest ancestor of the bridge binary with\n\
     \x20                 .contextmink.toml, so a vendored checkout anchors to\n\
     \x20                 the workspace it serves; else the nearest ancestor\n\
     \x20                 with .git; else the current directory).\n\
     \x20 --login         Run the command through a Git Bash login shell\n\
     \x20                 (argv-safe; no command text is shell-reparsed).\n\
     \x20 --print-argv    Print the assembled argv one entry per line and exit.\n\
     \x20 --print-root    Print the resolved bridge root (CONTEXTMINK_BRIDGE_ROOT,\n\
     \x20                 else the policy/.git anchor described under --cwd) and\n\
     \x20                 exit.\n\
     \n\
     Channels for PowerShell-fragile arguments:\n\
     \x20 --argv-b64: base64 of the UTF-8 argv joined with NUL. One token has\n\
     \x20 no spaces or quotes, so PowerShell 5.1 cannot mangle it:\n\
     \x20   $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($argv -join [char]0)))\n\
     \x20 --argfile: one argument per line, no quoting; UTF-8 BOM and trailing\n\
     \x20 CRs are stripped.\n\
     \n\
     Direct argv modes spawn natively (no MSYS argument rewriting). A program\n\
     spelled as a path (./gradlew, sub/tool) resolves against --cwd; bare\n\
     names use PATH; extensionless Bash scripts retry through Git Bash.\n\
     \n\
     Destructive-command deny-list: argv matching `git clean` is refused\n\
     before spawn (exit 64; nested bash/powershell/cmd payloads are scanned\n\
     too). Repositories can add protected deletion fragments in\n\
     .contextmink.toml. Break-glass:\n\
     CONTEXTMINK_BRIDGE_ALLOW_DESTRUCTIVE=1 runs the command anyway with a\n\
     loud stderr warning — for human operators only; agents must never set\n\
     it.\n\
     \n\
     Exit codes:\n\
     64 usage, 66 missing path, 126 spawn failure, 127 no bash; otherwise the\n\
     child's exit code.\n\
     \n\
     Purpose-built for Windows hosts (PowerShell argv mangling, MSYS\n\
     rewriting, Git Bash discovery); on POSIX hosts run commands directly.\n"
        .to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(args) {
        Ok(code) => exit(code),
        Err(BridgeError { message, code }) => {
            eprintln!("contextmink-bridge: {message}");
            exit(code);
        }
    }
}

#[derive(Debug)]
struct BridgeError {
    message: String,
    code: i32,
}

fn fail(code: i32, message: impl Into<String>) -> BridgeError {
    BridgeError {
        message: message.into(),
        code,
    }
}

fn run(args: Vec<String>) -> Result<i32, BridgeError> {
    run_with_root(args, bridge_root())
}

fn run_with_root(args: Vec<String>, root: PathBuf) -> Result<i32, BridgeError> {
    let mut cwd: Option<String> = None;
    let mut login = false;
    let mut print_argv = false;
    let mut iter = args.into_iter().peekable();
    let mut command_form: Option<(String, Vec<String>)> = None;

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--cwd" => {
                cwd = Some(
                    iter.next()
                        .ok_or_else(|| fail(EXIT_USAGE, "--cwd requires a directory"))?,
                );
            }
            "--login" => login = true,
            "--print-argv" => print_argv = true,
            "--print-root" => {
                // Disclose the resolved root before any command form is
                // required: a silently wrong root is the failure mode of the
                // anchoring chain.
                println!("{}", bridge_root().display());
                return Ok(0);
            }
            "--help" | "-h" => {
                print!("{}", usage());
                return Ok(0);
            }
            "--version" | "-V" => {
                println!("contextmink-bridge {}", env!("CARGO_PKG_VERSION"));
                return Ok(0);
            }
            "--" | "--script" | "--argfile" | "--argv-b64" => {
                command_form = Some((arg, iter.collect()));
                break;
            }
            other => {
                return Err(fail(
                    EXIT_USAGE,
                    format!(
                        "unknown argument: {other} (use -- to separate the command, or --help)"
                    ),
                ));
            }
        }
    }

    let target_cwd = match &cwd {
        Some(dir) => resolve_from_root(&root, dir),
        None => root.clone(),
    };
    if !target_cwd.is_dir() {
        return Err(fail(
            EXIT_MISSING_PATH,
            format!("working directory not found: {}", target_cwd.display()),
        ));
    }

    let Some((form, rest)) = command_form else {
        return Err(fail(
            EXIT_USAGE,
            format!(
                "a command form (--, --script, --argfile, or --argv-b64) is required\n{}",
                usage()
            ),
        ));
    };

    let (script_mode, argv) = assemble_argv(&form, rest, &root)?;
    if argv.is_empty() {
        return Err(fail(EXIT_USAGE, format!("{form} requires a command")));
    }

    if print_argv {
        let mut stdout = std::io::stdout();
        for (index, arg) in argv.iter().enumerate() {
            writeln!(stdout, "argv[{index}]={arg}").map_err(|error| {
                fail(EXIT_SPAWN_FAILED, format!("stdout write failed: {error}"))
            })?;
        }
        return Ok(0);
    }

    // Blocking deny-list for destructive argv. Runs for every command form,
    // including script mode (unlike the warn-only dump trip-wire below).
    let guard_config = load_destructive_guard_config(&root)?;
    match destructive_guard::evaluate_argv(
        &argv,
        &guard_config,
        destructive_guard::destructive_override_active(),
    ) {
        destructive_guard::DenyDecision::Allow => {}
        destructive_guard::DenyDecision::AllowWithOverride { message } => {
            eprintln!(
                "contextmink-bridge: WARNING: {}=1 break-glass override active (human operators \
                 only); running a command the destructive deny-list would block: {message}",
                destructive_guard::ALLOW_DESTRUCTIVE_ENV
            );
        }
        destructive_guard::DenyDecision::Deny { message } => {
            return Err(fail(
                EXIT_USAGE,
                format!("destructive command blocked: {message}"),
            ));
        }
    }

    if !script_mode {
        warn_content_dump(&argv);
    }

    let status = if script_mode || login {
        let bash = locate_bash().ok_or_else(|| {
            fail(
                EXIT_NO_BASH,
                "unable to locate a Git Bash executable (set CONTEXTMINK_BASH to override)",
            )
        })?;
        let mut command = Command::new(&bash);
        if login {
            command.arg("--login");
        }
        if script_mode {
            command.args(&argv);
        } else {
            // Constant -c text; the user command rides in as positional
            // parameters, so no command text is shell-reparsed.
            command.arg("-c").arg("exec \"$@\"").arg("bash").args(&argv);
        }
        command.current_dir(&target_cwd);
        command.status().map_err(|error| {
            fail(
                EXIT_SPAWN_FAILED,
                format!("failed to spawn {bash:?}: {error}"),
            )
        })?
    } else {
        spawn_direct(&argv, &target_cwd)?
    };

    Ok(exit_code(status))
}

fn load_destructive_guard_config(
    root: &Path,
) -> Result<config::DestructiveGuardConfig, BridgeError> {
    let path = root.join(".contextmink.toml");
    if !path.is_file() {
        return Ok(config::DestructiveGuardConfig::default());
    }
    config::load_context_config(Some(&path), false)
        .map(|config| config.destructive_guard)
        .map_err(|error| {
            fail(
                EXIT_USAGE,
                format!(
                    "failed to load bridge destructive guard config {}: {error:#}",
                    path.display()
                ),
            )
        })
}

fn assemble_argv(
    form: &str,
    rest: Vec<String>,
    root: &Path,
) -> Result<(bool, Vec<String>), BridgeError> {
    match form {
        "--" => Ok((false, rest)),
        "--script" => {
            let Some((script, args)) = rest.split_first() else {
                return Err(fail(EXIT_USAGE, "--script requires a script path"));
            };
            let script = resolve_from_root(root, script);
            if !script.is_file() {
                return Err(fail(
                    EXIT_MISSING_PATH,
                    format!("script not found: {}", script.display()),
                ));
            }
            let mut argv = vec![script.to_string_lossy().replace('\\', "/")];
            argv.extend(args.iter().cloned());
            Ok((true, argv))
        }
        "--argfile" => {
            let [file] = rest.as_slice() else {
                return Err(fail(
                    EXIT_USAGE,
                    "--argfile takes exactly one file and no further arguments",
                ));
            };
            let file = resolve_from_root(root, file);
            let text = std::fs::read_to_string(&file).map_err(|error| {
                fail(
                    EXIT_MISSING_PATH,
                    format!("failed to read argfile {}: {error}", file.display()),
                )
            })?;
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            let argv: Vec<String> = text
                .lines()
                .map(|line| line.trim_end_matches('\r').to_owned())
                .collect();
            if argv.is_empty() {
                return Err(fail(
                    EXIT_USAGE,
                    format!("argfile is empty: {}", file.display()),
                ));
            }
            Ok((false, argv))
        }
        "--argv-b64" => {
            let [token] = rest.as_slice() else {
                return Err(fail(
                    EXIT_USAGE,
                    "--argv-b64 takes exactly one token and no further arguments",
                ));
            };
            let bytes = decode_base64(token)
                .map_err(|error| fail(EXIT_USAGE, format!("invalid --argv-b64 token: {error}")))?;
            let joined = String::from_utf8(bytes)
                .map_err(|_| fail(EXIT_USAGE, "--argv-b64 payload is not valid UTF-8"))?;
            // The documented encoder (`$argv -join [char]0`) never emits a
            // trailing NUL, so every split entry — including a trailing empty
            // string — is a genuine argument. Only a payload that is empty or
            // a single NUL carries no arguments at all.
            let argv: Vec<String> = if joined.is_empty() || joined == "\0" {
                Vec::new()
            } else {
                joined.split('\0').map(str::to_owned).collect()
            };
            if argv.is_empty() {
                return Err(fail(
                    EXIT_USAGE,
                    "--argv-b64 payload decodes to no arguments",
                ));
            }
            Ok((false, argv))
        }
        _ => unreachable!("command forms are matched before assemble_argv"),
    }
}

fn spawn_direct(
    argv: &[String],
    target_cwd: &Path,
) -> Result<std::process::ExitStatus, BridgeError> {
    let (given, args) = argv.split_first().expect("argv checked non-empty");
    let program = resolve_program(given, target_cwd);
    let mut command = Command::new(&program);
    command.args(args).current_dir(target_cwd);
    match command.status() {
        Ok(status) => Ok(status),
        // ERROR_BAD_EXE_FORMAT: an extensionless Bash script was given as the
        // program; retry through Git Bash as argv, not as a shell string.
        // The retry is the designed path for repo scripts, so it stays silent
        // by default: a stderr line on a successful run reads as a warning
        // and PowerShell 5.1 wraps native stderr in NativeCommandError
        // records, which can mark the whole pipeline failed despite exit 0.
        // CONTEXTMINK_BRIDGE_DEBUG=1 discloses the interpreter choice.
        Err(error) if cfg!(windows) && error.raw_os_error() == Some(193) => {
            let bash = locate_bash().ok_or_else(|| {
                fail(
                    EXIT_NO_BASH,
                    format!(
                        "{program} is not a Win32 executable and no Git Bash was found for script fallback"
                    ),
                )
            })?;
            if std::env::var_os("CONTEXTMINK_BRIDGE_DEBUG").is_some_and(|value| value == "1") {
                eprintln!(
                    "contextmink-bridge: retrying {program} through {}",
                    bash.display()
                );
            }
            Command::new(&bash)
                .arg(&program)
                .args(args)
                .current_dir(target_cwd)
                .status()
                .map_err(|error| {
                    fail(
                        EXIT_SPAWN_FAILED,
                        format!("failed to spawn {bash:?}: {error}"),
                    )
                })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(fail(EXIT_NO_BASH, not_found_message(given, &program)))
        }
        Err(error) => Err(fail(
            EXIT_SPAWN_FAILED,
            format!("failed to spawn {program}: {error}"),
        )),
    }
}

/// A program spelled as a path (containing a separator) resolves against the
/// child's working directory, matching POSIX exec semantics; bare names keep
/// PATH lookup. Rust's `Command` resolves relative programs against the
/// parent's cwd instead, which would break `--cwd <dir> -- ./script` and
/// starve the ERROR_BAD_EXE_FORMAT script fallback of a resolvable path.
fn resolve_program(program: &str, target_cwd: &Path) -> String {
    let path = Path::new(program);
    let is_pathlike = program.chars().any(std::path::is_separator);
    if !is_pathlike || path.is_absolute() || path.has_root() {
        return program.to_owned();
    }
    let mut resolved = target_cwd.to_path_buf();
    for component in path.components() {
        if component != std::path::Component::CurDir {
            resolved.push(component.as_os_str());
        }
    }
    resolved.to_string_lossy().replace('\\', "/")
}

/// Teach the fix at the point of failure: a path-like program that does not
/// resolve is usually a repo script the caller meant to address from the
/// bridge root, which is what `--script` does.
fn not_found_message(given: &str, resolved: &str) -> String {
    let mut message = format!("command not found: {resolved}");
    if given != resolved {
        message.push_str(&format!(" (from {given}, resolved against --cwd)"));
    }
    if given.chars().any(std::path::is_separator) {
        message.push_str("; repo scripts resolve from the bridge root via --script <path>");
    }
    message
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(EXIT_SPAWN_FAILED)
}

/// Root for resolving relative `--cwd`, `--script`, and `--argfile` paths:
/// explicit env override, else the workspace root derived from the bridge
/// binary's location, else the current directory.
fn bridge_root() -> PathBuf {
    if let Some(root) = std::env::var_os("CONTEXTMINK_BRIDGE_ROOT") {
        return PathBuf::from(root);
    }
    std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::parent)
        .and_then(resolve_root_from_exe_dir)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// The nearest ancestor carrying `.contextmink.toml` (the contextmink policy
/// root) wins over the nearest `.git`: a vendored contextmink checkout is its
/// own git repository nested inside the workspace it serves, so `.git` alone
/// would anchor relative paths to the vendored tree instead of the workspace.
fn resolve_root_from_exe_dir(exe_dir: &Path) -> Option<PathBuf> {
    let mut cursor = Some(exe_dir);
    while let Some(dir) = cursor {
        if dir.join(".contextmink.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    let mut cursor = Some(exe_dir);
    while let Some(dir) = cursor {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

fn resolve_from_root(root: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    root.join(path)
}

fn locate_bash() -> Option<PathBuf> {
    if let Some(bash) = std::env::var_os("CONTEXTMINK_BASH") {
        let bash = PathBuf::from(bash);
        if bash.is_file() {
            return Some(bash);
        }
    }
    if cfg!(windows) {
        windows_bash_candidates()
            .into_iter()
            .find(|candidate| candidate.is_file())
    } else {
        // PATH resolution is safe off Windows; there is no WSL shadow.
        Some(PathBuf::from("bash"))
    }
}

/// Git-for-Windows installs only. Cygwin and MSYS2 bash have different path
/// and file-locking semantics and must not silently substitute for Git Bash;
/// exotic hosts point CONTEXTMINK_BASH at their shell explicitly.
fn windows_bash_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(program_files).join(r"Git\bin\bash.exe"));
    }
    candidates.push(PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"));
    candidates.push(PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"));
    candidates
}

/// Warn (never block) when argv is a raw content dump a bounded read would
/// serve better. Mirrors the codex-bash.sh template trip-wire.
fn warn_content_dump(argv: &[String]) {
    if std::env::var_os(SUPPRESS_ENV).is_some_and(|value| value == "1") {
        return;
    }
    let Some((program, args)) = argv.split_first() else {
        return;
    };
    let program = Path::new(program)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(program.as_str())
        .to_ascii_lowercase();
    match program.as_str() {
        "sed" => {
            for arg in args {
                if let Some(span) = sed_window_span(arg)
                    && span > DUMP_WARN_LINES
                {
                    eprintln!(
                        "contextmink-bridge: sed window of {span} lines is a transcript dump; prefer contextmink outline <file> then slice --range START:END ({SUPPRESS_ENV}=1 silences)"
                    );
                }
            }
        }
        "cat" | "nl" => {
            for arg in args {
                if arg.starts_with('-') {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(arg) else {
                    continue;
                };
                let lines = text.lines().count();
                if lines > DUMP_WARN_LINES {
                    eprintln!(
                        "contextmink-bridge: {program} on {arg} ({lines} lines) is a transcript dump; prefer contextmink outline/slice ({SUPPRESS_ENV}=1 silences)"
                    );
                }
            }
        }
        "head" | "tail" => {
            let mut expect_count = false;
            for arg in args {
                let count = if expect_count {
                    expect_count = false;
                    arg.parse::<usize>().ok()
                } else if arg == "-n" || arg == "--lines" {
                    expect_count = true;
                    continue;
                } else {
                    arg.strip_prefix("--lines=")
                        .or_else(|| arg.strip_prefix("-n"))
                        .or_else(|| arg.strip_prefix('-'))
                        .and_then(|digits| digits.parse::<usize>().ok())
                };
                if let Some(count) = count
                    && count > DUMP_WARN_LINES
                {
                    eprintln!(
                        "contextmink-bridge: {program} -n {count} is a transcript dump; prefer contextmink outline/slice ({SUPPRESS_ENV}=1 silences)"
                    );
                }
            }
        }
        _ => {}
    }
}

/// `N,Mp` or `-nN,Mp` sed print windows.
fn sed_window_span(arg: &str) -> Option<usize> {
    let arg = arg.strip_prefix("-n").unwrap_or(arg);
    let body = arg.strip_suffix('p')?;
    let (start, end) = body.split_once(',')?;
    let start: usize = start.parse().ok()?;
    let end: usize = end.parse().ok()?;
    Some(end.saturating_sub(start) + 1)
}

/// Standard base64 (also accepting URL-safe `-`/`_`), whitespace-tolerant,
/// optional padding. Hand-rolled to keep the dependency surface at zero.
fn decode_base64(token: &str) -> Result<Vec<u8>, String> {
    fn value_of(ch: u8) -> Result<u8, String> {
        match ch {
            b'A'..=b'Z' => Ok(ch - b'A'),
            b'a'..=b'z' => Ok(ch - b'a' + 26),
            b'0'..=b'9' => Ok(ch - b'0' + 52),
            b'+' | b'-' => Ok(62),
            b'/' | b'_' => Ok(63),
            other => Err(format!("invalid base64 byte 0x{other:02x}")),
        }
    }
    let mut output = Vec::with_capacity(token.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in token.bytes() {
        if byte.is_ascii_whitespace() || byte == b'=' {
            continue;
        }
        buffer = (buffer << 6) | u32::from(value_of(byte)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
        }
    }
    Ok(output)
}

#[cfg(test)]
#[path = "contextmink_bridge/tests.rs"]
mod tests;
