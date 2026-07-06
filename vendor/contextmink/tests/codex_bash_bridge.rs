//! Integration tests for the PowerShell -> Git Bash bridge template
//! (`templates/scripts/codex-bash.sh`). They exercise the bash side of the
//! bridge: argv fidelity, argfile parsing, guard exits, and the content-dump
//! trip-wire. The PowerShell -> bash boundary itself cannot be tested from
//! here; `--print-argv` exists so agents can probe it live.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bash_executable() -> PathBuf {
    if cfg!(windows) {
        let mut candidates = vec![
            PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
            PathBuf::from(r"C:\Program Files\Git\usr\bin\bash.exe"),
        ];
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.insert(0, PathBuf::from(program_files).join(r"Git\bin\bash.exe"));
        }
        candidates
            .into_iter()
            .find(|candidate| candidate.is_file())
            .expect("codex-bash bridge tests require Git Bash (bin\\bash.exe under Program Files)")
    } else {
        PathBuf::from("bash")
    }
}

fn launcher_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("templates/scripts/codex-bash.sh")
        .to_string_lossy()
        .replace('\\', "/")
}

fn run_bridge(args: &[&str]) -> Output {
    Command::new(bash_executable())
        .arg(launcher_path())
        .args(args)
        .env_remove("CODEX_BASH_SUPPRESS_DUMP_WARNING")
        .output()
        .expect("failed to spawn bash for the bridge")
}

fn temp_root(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("codex-bash-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root); // guardrail: allow-ignore-result cleanup is best-effort for reused test temp dirs
    fs::create_dir_all(&root).unwrap();
    root
}

fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn print_argv_reports_exact_arguments() {
    let output = run_bridge(&[
        "--print-argv",
        "--",
        "prog",
        "with space",
        "dollar$sign",
        "^//x",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout,
        "argv[0]=prog\nargv[1]=with space\nargv[2]=dollar$sign\nargv[3]=^//x\n"
    );
}

#[test]
fn argfile_preserves_hostile_arguments_and_strips_bom_and_cr() {
    let root = temp_root("argfile");
    let argfile = root.join("args.txt");
    fs::write(
        &argfile,
        "\u{feff}printf\r\n%s\n embed\"quote and space\ntrail\\backslash\n",
    )
    .unwrap();
    let output = run_bridge(&["--print-argv", "--argfile", &forward_slashes(&argfile)]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout,
        "argv[0]=printf\nargv[1]=%s\nargv[2]= embed\"quote and space\nargv[3]=trail\\backslash\n"
    );

    let empty = root.join("empty.txt");
    fs::write(&empty, "").unwrap();
    let output = run_bridge(&["--argfile", &forward_slashes(&empty)]);
    assert_eq!(output.status.code(), Some(64));
}

#[test]
fn guards_exit_64_instead_of_hanging_or_falling_through() {
    let unknown = run_bridge(&["stray-arg"]);
    assert_eq!(unknown.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown argument"));

    let flags_only = run_bridge(&["--login"]);
    assert_eq!(flags_only.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&flags_only.stderr).contains("require a command form"));

    // No command form and no terminal: must refuse the interactive shell.
    let headless = run_bridge(&[]);
    assert_eq!(headless.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&headless.stderr).contains("not a terminal"));
}

#[test]
fn child_exit_code_propagates() {
    let output = run_bridge(&["--", "false"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn cwd_flag_selects_working_directory() {
    let root = temp_root("cwd");
    let output = run_bridge(&["--cwd", &forward_slashes(&root), "--", "pwd"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let marker = root.file_name().unwrap().to_string_lossy().to_string();
    assert!(stdout.trim_end().ends_with(&marker), "pwd was: {stdout}");
}

#[test]
fn script_mode_runs_bash_scripts_with_arguments() {
    let root = temp_root("script");
    let script = root.join("echo_args");
    fs::write(&script, "printf '%s\\n' \"$@\"\n").unwrap();
    let output = run_bridge(&["--script", &forward_slashes(&script), "one", "two words"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "one\ntwo words\n"
    );
}

#[test]
fn content_dump_trip_wire_warns_on_wide_sed_windows() {
    let root = temp_root("tripwire");
    let file = root.join("lines.txt");
    fs::write(&file, "line\n".repeat(10)).unwrap();

    let wide = run_bridge(&["--", "sed", "-n", "1,500p", &forward_slashes(&file)]);
    assert!(wide.status.success());
    assert!(String::from_utf8_lossy(&wide.stderr).contains("transcript dump"));

    let narrow = run_bridge(&["--", "sed", "-n", "1,5p", &forward_slashes(&file)]);
    assert!(narrow.status.success());
    assert!(!String::from_utf8_lossy(&narrow.stderr).contains("transcript dump"));
}
