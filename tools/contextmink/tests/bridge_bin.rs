//! Integration tests for the native `contextmink-bridge` binary. Direct argv
//! modes spawn without any shell, so these run identically on Windows and
//! Unix; Git-Bash-dependent modes (`--script`, `--login`) are covered by the
//! bash-template tests in `codex_bash_bridge.rs` and exercised here only
//! through the shared discovery logic.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bridge_exe() -> &'static str {
    env!("CARGO_BIN_EXE_contextmink-bridge")
}

fn contextmink_exe() -> &'static str {
    env!("CARGO_BIN_EXE_contextmink")
}

fn run_bridge(args: &[&str]) -> Output {
    Command::new(bridge_exe())
        .args(args)
        .env_remove("CODEX_BASH_SUPPRESS_DUMP_WARNING")
        .env_remove("CONTEXTMINK_BRIDGE_ROOT")
        .output()
        .expect("failed to spawn contextmink-bridge")
}

fn temp_root(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("bridge-bin-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root); // guardrail: allow-ignore-result cleanup is best-effort for reused test temp dirs
    fs::create_dir_all(&root).unwrap();
    root
}

fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn encode_argv_b64(argv: &[&str]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let joined = argv.join("\0");
    let mut token = String::new();
    for chunk in joined.as_bytes().chunks(3) {
        let mut buffer = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            buffer |= u32::from(*byte) << (16 - 8 * index);
        }
        for position in 0..=chunk.len() {
            let shift = 18 - 6 * position;
            token.push(ALPHABET[((buffer >> shift) & 0x3f) as usize] as char);
        }
    }
    token
}

#[test]
fn print_argv_reports_exact_arguments_for_every_channel() {
    let hostile = &[
        "prog",
        "with space",
        "embed\"quote",
        "dollar$sign",
        "^// PART",
    ];

    let mut plain = vec!["--print-argv", "--"];
    plain.extend_from_slice(hostile);
    let output = run_bridge(&plain);
    assert!(output.status.success());
    let expected = "argv[0]=prog\nargv[1]=with space\nargv[2]=embed\"quote\nargv[3]=dollar$sign\nargv[4]=^// PART\n";
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);

    let token = encode_argv_b64(hostile);
    let output = run_bridge(&["--print-argv", "--argv-b64", &token]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);

    let root = temp_root("argfile");
    let argfile = root.join("args.txt");
    fs::write(&argfile, format!("\u{feff}{}\r\n", hostile.join("\r\n"))).unwrap();
    let output = run_bridge(&["--print-argv", "--argfile", &forward_slashes(&argfile)]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn direct_spawn_runs_child_and_propagates_exit_code() {
    let output = run_bridge(&["--", contextmink_exe(), "--help"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("transcript guard"));

    // A missing input makes contextmink exit nonzero; the bridge must forward it.
    let output = run_bridge(&["--", contextmink_exe(), "slice", "no-such-file.txt"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn slash_bearing_arguments_reach_native_children_verbatim() {
    let root = temp_root("msys-free");
    let notes = root.join("notes.h");
    fs::write(&notes, "// PART 1: strides\nint x;\n// PART 2: fixups\n").unwrap();
    let output = run_bridge(&[
        "--",
        contextmink_exe(),
        "outline",
        &forward_slashes(&notes),
        "--prefix",
        "// PART",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1: // PART 1: strides"), "stdout: {stdout}");
    assert!(stdout.contains("3: // PART 2: fixups"), "stdout: {stdout}");
}

#[test]
fn version_and_help_identify_the_bridge() {
    let version = run_bridge(&["--version"]);
    assert!(version.status.success());
    assert!(
        String::from_utf8_lossy(&version.stdout).starts_with("contextmink-bridge 0."),
        "stdout: {}",
        String::from_utf8_lossy(&version.stdout)
    );

    let help = run_bridge(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("argv-b64"));
}

#[test]
fn guards_fail_fast_with_usage_exit_codes() {
    let unknown = run_bridge(&["stray-arg"]);
    assert_eq!(unknown.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown argument"));

    let no_form = run_bridge(&["--login"]);
    assert_eq!(no_form.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&no_form.stderr).contains("command form"));

    let empty = run_bridge(&["--"]);
    assert_eq!(empty.status.code(), Some(64));

    let bad_token = run_bridge(&["--argv-b64", "not!base64"]);
    assert_eq!(bad_token.status.code(), Some(64));

    let missing_cwd = run_bridge(&["--cwd", "no-such-dir-anywhere", "--", "prog"]);
    assert_eq!(missing_cwd.status.code(), Some(66));

    let not_found = run_bridge(&["--", "no-such-program-anywhere"]);
    assert_eq!(not_found.status.code(), Some(127));
}

#[test]
fn cwd_flag_selects_working_directory_for_the_child() {
    let root = temp_root("cwd");
    fs::write(root.join("only-here.txt"), "alpha\n").unwrap();
    let output = run_bridge(&[
        "--cwd",
        &forward_slashes(&root),
        "--",
        contextmink_exe(),
        "slice",
        "only-here.txt",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1: alpha"));
}

#[test]
fn content_dump_trip_wire_warns_before_spawning() {
    // The warning fires on argv shape; sed itself may not exist on PATH here,
    // so only the stderr warning is asserted.
    let output = run_bridge(&["--", "sed", "-n", "1,500p", "whatever.txt"]);
    assert!(String::from_utf8_lossy(&output.stderr).contains("transcript dump"));

    let narrow = run_bridge(&["--", "sed", "-n", "1,5p", "whatever.txt"]);
    assert!(!String::from_utf8_lossy(&narrow.stderr).contains("transcript dump"));
}
