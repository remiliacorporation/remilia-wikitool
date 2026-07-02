use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn fixture_root(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("contextmink-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root); // guardrail: allow-ignore-result cleanup is best-effort for reused test temp dirs
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\n",
    )
    .unwrap();
    fs::write(root.join("sample.txt"), "alpha beta\nalpha\nbeta\n").unwrap();
    fs::write(
        root.join("sidecar.json"),
        r#"{"mode":"demo","nested":{"mode":"inner"},"textures":[{"index":0,"texture_type":"diffuse","flags":1,"path":"World|A.blp"},{"index":1,"texture_type":"normal","flags":0,"path":"World|B.blp"}]}"#,
    )
    .unwrap();
    root
}

fn run_contextmink(root: &PathBuf, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_contextmink"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "contextmink failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run_contextmink_raw(root: &PathBuf, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_contextmink"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn parse_json_output(root: &PathBuf, args: &[&str]) -> Value {
    serde_json::from_str(&run_contextmink(root, args)).unwrap()
}

fn assert_envelope(value: &Value, command: &str, unit: &str) {
    assert_eq!(value["tool"], "contextmink");
    assert_eq!(value["command"], command);
    assert_eq!(value["profile"], "test-profile");
    assert_eq!(value["unit"], unit);
    assert!(value["shown"].is_number());
    assert!(value["total"].is_number());
    assert!(value["truncated"].is_boolean());
    assert!(value["complete"].is_boolean());
    assert!(value.get("cap_reason").is_some());
}

#[test]
fn json_commands_share_receipt_envelope() {
    let root = fixture_root("json-envelope");

    let files = parse_json_output(&root, &["--json", "files", ".", "--max", "1"]);
    assert_envelope(&files, "files", "files");
    assert_eq!(files["truncated"], true);
    assert_eq!(files["complete"], false);
    assert_eq!(files["cap_reason"], "max");

    let slice = parse_json_output(&root, &["--json", "slice", "sample.txt", "--range", "1:2"]);
    assert_envelope(&slice, "slice", "lines");
    assert_eq!(slice["complete"], true);
    assert!(slice["cap_reason"].is_null());

    let json_find = parse_json_output(
        &root,
        &[
            "--json",
            "json-find",
            "sidecar.json",
            "--key-contains",
            "mode",
        ],
    );
    assert_envelope(&json_find, "json-find", "matches");
    assert_eq!(json_find["total"], 2);
}

#[test]
fn slice_accepts_named_path_alias() {
    let root = fixture_root("slice-path-alias");

    let json = parse_json_output(
        &root,
        &["--json", "slice", "--path", "sample.txt", "--range", "2:2"],
    );
    assert_envelope(&json, "slice", "lines");
    assert_eq!(json["path"], "sample.txt");
    assert_eq!(json["lines"][0]["line"], 2);
    assert_eq!(json["lines"][0]["text"], "alpha");
}

#[test]
fn outline_maps_declarations_with_receipt_envelope() {
    let root = fixture_root("outline-envelope");
    fs::write(
        root.join("sample.rs"),
        concat!(
            "use std::io;\n",
            "\n",
            "pub struct Frame {\n",
            "    depth: usize,\n",
            "}\n",
            "\n",
            "impl Frame {\n",
            "    pub fn render(&self) {\n",
            "        let local = 1;\n",
            "    }\n",
            "\n",
            "    fn cull_hidden(&mut self) {}\n",
            "}\n",
            "\n",
            "mod tests;\n",
        ),
    )
    .unwrap();

    let json = parse_json_output(&root, &["--json", "outline", "sample.rs"]);
    assert_envelope(&json, "outline", "items");
    assert_eq!(json["language"], "rust");
    assert_eq!(json["path"], "sample.rs");
    assert_eq!(json["total_lines"], 15);
    assert_eq!(json["total"], 5);
    assert_eq!(json["declaration_lines_total"], 5);
    assert_eq!(json["complete"], true);
    assert_eq!(json["items"][0]["line"], 3);
    assert_eq!(json["items"][0]["text"], "pub struct Frame {");
    assert_eq!(json["items"][2]["text"], "    pub fn render(&self) {");

    let filtered = parse_json_output(
        &root,
        &[
            "--json",
            "outline",
            "--path",
            "sample.rs",
            "--contains",
            "CULL",
            "--ignore-case",
        ],
    );
    assert_eq!(filtered["total"], 1);
    assert_eq!(filtered["declaration_lines_total"], 5);
    assert_eq!(
        filtered["items"][0]["text"],
        "    fn cull_hidden(&mut self) {}"
    );

    let capped = parse_json_output(
        &root,
        &["--json", "outline", "sample.rs", "--max-items", "2"],
    );
    assert_eq!(capped["truncated"], true);
    assert_eq!(capped["cap_reason"], "max_items");
    assert_eq!(capped["shown"], 2);
    assert_eq!(capped["total"], 5);

    let human = run_contextmink(&root, &["outline", "sample.rs", "--limit", "2"]);
    assert!(human.contains("[contextmink] outline path=sample.rs language=rust total_lines=15"));
    assert!(human.contains("3: pub struct Frame {"));
    assert!(human.contains("capped outline at 2 items"));
    assert!(human.contains("CONTEXTMINK_RECEIPT "));
}

#[test]
fn outline_fails_fast_without_language_heuristic() {
    let root = fixture_root("outline-unknown");

    let output = run_contextmink_raw(&root, &["outline", "sample.txt"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--lang"), "stderr: {stderr}");

    let custom = parse_json_output(
        &root,
        &["--json", "outline", "sample.txt", "--pattern", "^alpha"],
    );
    assert_eq!(custom["language"], "pattern");
    assert_eq!(custom["total"], 2);

    let prefixed = parse_json_output(
        &root,
        &["--json", "outline", "sample.txt", "--prefix", "alpha"],
    );
    assert_eq!(prefixed["language"], "prefix");
    assert_eq!(prefixed["total"], 2);
    assert_eq!(prefixed["items"][0]["line"], 1);
}

#[test]
fn json_commands_accept_named_path_alias() {
    let root = fixture_root("json-path-alias");

    let find = parse_json_output(
        &root,
        &[
            "--json",
            "json-find",
            "--path",
            "sidecar.json",
            "--key-contains",
            "mode",
        ],
    );
    assert_envelope(&find, "json-find", "matches");
    assert_eq!(find["total"], 2);

    let select = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "--path",
            "sidecar.json",
            "--field",
            "/mode",
        ],
    );
    assert_envelope(&select, "json-select", "rows");
    assert_eq!(select["rows"][0]["fields"]["/mode"], "\"demo\"");
}

#[test]
fn capture_caps_child_stdout_and_reports_exit_status() {
    let root = fixture_root("capture-stdout");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--max-lines",
            "1",
            "--",
            bin,
            "--no-config",
            "slice",
            "sample.txt",
            "--range",
            "1:3",
        ],
    );
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["success"], true);
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["stdout"]["shown_lines"], 1);
    assert!(json["stdout"]["total_lines"].as_u64().unwrap() > 1);
    assert!(json["stdout"]["omitted_lines"].as_u64().unwrap() > 0);
    assert_eq!(json["truncated"], true);
    assert_eq!(json["cap_reason"], "lines");
    // With a one-line budget the tail (verdict end of the output) wins.
    assert!(
        json["stdout_text"]
            .as_str()
            .unwrap()
            .contains("CONTEXTMINK_RECEIPT")
    );
}

#[test]
fn capture_keeps_head_and_tail_when_line_capped() {
    let root = fixture_root("capture-head-tail");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    // slice 1:3 of sample.txt emits 3 content lines plus a receipt line.
    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--max-lines",
            "2",
            "--",
            bin,
            "--no-config",
            "slice",
            "sample.txt",
            "--range",
            "1:3",
        ],
    );
    assert_envelope(&json, "capture", "lines");
    let text = json["stdout_text"].as_str().unwrap();
    assert!(text.contains("alpha beta"), "head kept: {text}");
    assert!(text.contains("CONTEXTMINK_RECEIPT"), "tail kept: {text}");
    assert!(text.contains("omitted"), "omission marker shown: {text}");
    assert_eq!(json["stdout"]["head_lines"], 1);
    assert_eq!(json["stdout"]["tail_lines"], 1);
    assert_eq!(json["stdout"]["omitted_lines"], 2);
}

#[test]
fn run_alias_uses_capture_receipt_shape() {
    let root = fixture_root("run-alias");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "run",
            "--max-lines",
            "1",
            "--",
            bin,
            "--no-config",
            "slice",
            "sample.txt",
            "--range",
            "1:1",
        ],
    );
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["success"], true);
    assert_eq!(json["exit_code"], 0);
}

#[test]
fn fail_if_truncated_exits_nonzero_after_receipt() {
    let root = fixture_root("fail-if-truncated");

    let output = run_contextmink_raw(
        &root,
        &[
            "--fail-if-truncated",
            "files",
            ".",
            "--max",
            "1",
            "--max-scan-files",
            "20",
        ],
    );
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("CONTEXTMINK_RECEIPT "));
    assert!(stdout.contains("\"truncated\":true"));
    assert!(stderr.contains("strict completion requested"));
}

#[test]
fn strict_alias_and_scan_guard_fail_after_receipt() {
    let root = fixture_root("strict-aliases");
    fs::write(root.join("extra_a.txt"), "a\n").unwrap();
    fs::write(root.join("extra_b.txt"), "b\n").unwrap();

    let strict = run_contextmink_raw(&root, &["--strict-complete", "files", ".", "--max", "1"]);
    assert!(!strict.status.success());
    let strict_stdout = String::from_utf8(strict.stdout).unwrap();
    assert!(strict_stdout.contains("CONTEXTMINK_RECEIPT "));

    let display_capped = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scan",
            "files",
            ".",
            "--max",
            "1",
            "--max-scan-files",
            "20",
        ],
    );
    assert!(display_capped.status.success());
    let display_stdout = String::from_utf8(display_capped.stdout).unwrap();
    assert!(display_stdout.contains("\"cap_reason\":\"max\""));

    let scan_capped = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scan",
            "files",
            ".",
            "--max",
            "10",
            "--max-scan-files",
            "1",
        ],
    );
    assert!(!scan_capped.status.success());
    let scan_stdout = String::from_utf8(scan_capped.stdout).unwrap();
    let scan_stderr = String::from_utf8(scan_capped.stderr).unwrap();
    assert!(scan_stdout.contains("\"cap_reason\":\"scan\""));
    assert!(scan_stderr.contains("--require-complete-scan"));
}

#[test]
fn capture_keeps_nonzero_child_status_in_receipt() {
    let root = fixture_root("capture-nonzero");
    let bin = env!("CARGO_BIN_EXE_contextmink");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "capture",
            "--",
            bin,
            "--no-config",
            "slice",
            "missing.txt",
            "--range",
            "1:1",
        ],
    );
    assert_envelope(&json, "capture", "lines");
    assert_eq!(json["success"], false);
    assert_ne!(json["exit_code"], 0);
    assert!(json["stderr"]["total_bytes"].as_u64().unwrap() > 0);
    assert!(
        json["stderr_text"]
            .as_str()
            .unwrap()
            .contains("missing.txt")
    );
}

#[test]
fn files_scan_cap_sets_complete_false() {
    let root = fixture_root("files-scan-cap");
    fs::write(root.join("extra_a.txt"), "a\n").unwrap();
    fs::write(root.join("extra_b.txt"), "b\n").unwrap();

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--max",
            "10",
            "--max-scan-files",
            "2",
        ],
    );
    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 2);
    assert_eq!(files["truncated"], true);
    assert_eq!(files["complete"], false);
    assert_eq!(files["cap_reason"], "scan");
    assert_eq!(files["candidate_files_scanned"], 2);
    assert_eq!(files["candidate_files_total_is_lower_bound"], true);
    assert_eq!(files["total"], 3);
}

#[test]
fn files_glob_matches_basename_inside_explicit_roots() {
    let root = fixture_root("files-basename-glob");
    fs::create_dir_all(root.join("queue")).unwrap();
    fs::write(root.join("queue").join("work.jsonl"), "{}\n").unwrap();
    fs::write(root.join("queue").join("notes.txt"), "skip\n").unwrap();

    let files = parse_json_output(
        &root,
        &[
            "--json", "files", "queue", "--glob", "*.jsonl", "--limit", "5",
        ],
    );

    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 1);
    assert_eq!(files["total"], 1);
    assert_eq!(files["files"][0], "queue/work.jsonl");
}

#[test]
fn files_ext_filters_without_shell_glob_patterns() {
    let root = fixture_root("files-ext-filter");
    fs::create_dir_all(root.join("queue")).unwrap();
    fs::write(root.join("queue").join("work.JSON"), "{}\n").unwrap();
    fs::write(root.join("queue").join("work.jsonl"), "{}\n").unwrap();
    fs::write(root.join("queue").join("notes.txt"), "skip\n").unwrap();

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            "queue",
            "--ext",
            ".json",
            "--extension",
            "jsonl",
            "--limit",
            "5",
        ],
    );

    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 2);
    assert_eq!(files["total"], 2);
    assert_eq!(files["files"][0], "queue/work.JSON");
    assert_eq!(files["files"][1], "queue/work.jsonl");
}

#[test]
fn files_accepts_named_path_without_default_root() {
    let root = fixture_root("files-path-alias");
    fs::create_dir_all(root.join("queue")).unwrap();
    fs::write(root.join("queue").join("work.jsonl"), "{}\n").unwrap();

    let files = parse_json_output(&root, &["--json", "files", "--path", "queue"]);

    assert_envelope(&files, "files", "files");
    assert_eq!(files["total"], 1);
    assert_eq!(files["files"][0], "queue/work.jsonl");
}

#[test]
fn help_names_excluded_file_bypass_positively() {
    let root = fixture_root("help-exclude-globs");

    let help = run_contextmink(&root, &["files", "--help"]);
    assert!(help.contains("--with-excluded"));
    assert!(!help.contains("--ignore-exclude-globs"));
    assert!(!help.contains("--include-noisy"));
}

#[test]
fn explicit_roots_inside_configured_excludes_are_honored() {
    let root = fixture_root("explicit-excluded-root");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\nexclude_globs = [\"artifacts/**\"]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("artifacts").join("notes")).unwrap();
    fs::write(
        root.join("artifacts").join("notes").join("finding.md"),
        "needle\n",
    )
    .unwrap();

    let broad = parse_json_output(&root, &["--json", "files", ".", "--max", "20"]);
    assert_envelope(&broad, "files", "files");
    let broad_files = broad["files"].as_array().unwrap();
    assert!(
        broad_files
            .iter()
            .all(|path| !path.as_str().unwrap().starts_with("artifacts/"))
    );

    let bypass = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--with-excluded",
            "--max",
            "20",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&bypass, "files", "files");
    assert!(
        bypass["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path.as_str().unwrap() == "./artifacts/notes/finding.md")
    );

    let files = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            "artifacts/notes",
            "--max",
            "20",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 1);
    assert_eq!(files["total"], 1);
    assert_eq!(files["files"][0], "artifacts/notes/finding.md");

    let grep = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "needle",
            "artifacts/notes",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&grep, "grep", "files");
    assert_eq!(grep["shown"], 1);
    assert_eq!(grep["total"], 1);
    assert_eq!(grep["total_matches"], 1);
}

#[test]
fn with_git_ignored_includes_gitignored_directories_without_disabling_exclude_globs() {
    let root = fixture_root("with-git-ignored");
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "vendor/*\n").unwrap();
    fs::create_dir_all(root.join("vendor").join("sqlite-tool").join(".git")).unwrap();
    fs::write(
        root.join("vendor").join("sqlite-tool").join("README.md"),
        "sqlite helper\n",
    )
    .unwrap();
    fs::write(
        root.join("vendor")
            .join("sqlite-tool")
            .join(".git")
            .join("config"),
        "ignored metadata\n",
    )
    .unwrap();

    // vendor/sqlite-tool is git-ignored but is itself a repo root: the
    // nested-repo supplement enters it and discloses the entry.
    let without_flag = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--max",
            "20",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&without_flag, "files", "files");
    let files_without_flag = without_flag["files"].as_array().unwrap();
    assert!(
        files_without_flag
            .iter()
            .any(|path| path.as_str().unwrap().trim_start_matches("./")
                == "vendor/sqlite-tool/README.md")
    );
    let nested = without_flag["nested_repos_entered"].as_array().unwrap();
    assert_eq!(nested.len(), 1);
    assert_eq!(
        nested[0].as_str().unwrap().trim_start_matches("./"),
        "vendor/sqlite-tool"
    );

    // --skip-nested-repos restores strict Git-scope behavior.
    let skipped = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--skip-nested-repos",
            "--max",
            "20",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&skipped, "files", "files");
    assert!(
        skipped["files"].as_array().unwrap().iter().all(|path| path
            .as_str()
            .unwrap()
            .trim_start_matches("./")
            != "vendor/sqlite-tool/README.md")
    );
    assert_eq!(skipped["nested_repos_entered"].as_array().unwrap().len(), 0);

    let with_flag = parse_json_output(
        &root,
        &[
            "--json",
            "files",
            ".",
            "--with-git-ignored",
            "--max",
            "20",
            "--max-scan-files",
            "20",
        ],
    );
    assert_envelope(&with_flag, "files", "files");
    let files = with_flag["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .any(|path| path.as_str().unwrap().trim_start_matches("./")
                == "vendor/sqlite-tool/README.md")
    );
    assert!(
        files
            .iter()
            .all(|path| !path.as_str().unwrap().contains("/.git/"))
    );
}

#[test]
fn grep_scan_cap_marks_no_match_as_scanned_subset_only() {
    let root = fixture_root("grep-scan-cap");
    fs::write(root.join("extra_a.txt"), "alpha\n").unwrap();
    fs::write(root.join("extra_b.txt"), "alpha\n").unwrap();

    let grep = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "not-present",
            ".",
            "--max-scan-files",
            "1",
        ],
    );
    assert_envelope(&grep, "grep", "files");
    assert_eq!(grep["shown"], 0);
    assert_eq!(grep["truncated"], true);
    assert_eq!(grep["complete"], false);
    assert_eq!(grep["cap_reason"], "scan");
    assert_eq!(grep["candidate_files_scanned"], 1);
    assert_eq!(grep["candidate_files_total_is_lower_bound"], true);
    assert!(grep["candidate_files_total"].as_u64().unwrap() >= 2);
    assert_eq!(grep["no_match_scope"], "scanned_subset");
}

#[test]
fn grep_terms_reports_public_command_name() {
    let root = fixture_root("grep-terms-command");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
        ],
    );
    assert_envelope(&json, "grep-terms", "files");
    assert_eq!(json["total_matches"], 1);

    let human = run_contextmink(
        &root,
        &["grep-terms", "--term", "alpha", "--term", "beta", "."],
    );
    let receipt = human
        .lines()
        .last()
        .unwrap()
        .strip_prefix("CONTEXTMINK_RECEIPT ")
        .unwrap();
    let receipt: Value = serde_json::from_str(receipt).unwrap();
    assert_envelope(&receipt, "grep-terms", "files");
}

#[test]
fn grep_terms_supports_any_mode_and_term_files() {
    let root = fixture_root("grep-terms-any");
    fs::write(root.join("phrases.txt"), "alpha beta\nmissing phrase\n").unwrap();

    let default_all = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
        ],
    );
    assert_envelope(&default_all, "grep-terms", "files");
    assert_eq!(default_all["pattern"], "all_terms(alpha,beta)");
    assert_eq!(default_all["total_matches"], 1);

    let any = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--mode",
            "any",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
        ],
    );
    assert_envelope(&any, "grep-terms", "files");
    assert_eq!(any["pattern"], "any_terms(alpha,beta)");
    assert_eq!(any["total_matches"], 3);

    let or_alias = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--or",
            "--term",
            "alpha",
            "--term",
            "beta",
            "sample.txt",
        ],
    );
    assert_envelope(&or_alias, "grep-terms", "files");
    assert_eq!(or_alias["pattern"], "any_terms(alpha,beta)");
    assert_eq!(or_alias["total_matches"], 3);

    let term_file = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--mode",
            "any",
            "--term-file",
            "phrases.txt",
            "sample.txt",
        ],
    );
    assert_envelope(&term_file, "grep-terms", "files");
    assert_eq!(term_file["pattern"], "any_terms(alpha beta,missing phrase)");
    assert_eq!(term_file["total_matches"], 1);
}

#[test]
fn grep_terms_accepts_agent_friendly_limit_aliases() {
    let root = fixture_root("grep-terms-aliases");
    fs::write(root.join("flags.txt"), "--flag-like value\n").unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "alpha",
            "--limit",
            "1",
            "--max-matches",
            "1",
            "sample.txt",
        ],
    );
    assert_envelope(&json, "grep-terms", "files");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["sample_lines_shown"], 1);
    assert_eq!(json["cap_reason"], "samples");
    assert_eq!(json["files"][0]["samples"].as_array().unwrap().len(), 1);

    let flag_like = parse_json_output(
        &root,
        &["--json", "grep-terms", "--term", "--flag-like", "flags.txt"],
    );
    assert_envelope(&flag_like, "grep-terms", "files");
    assert_eq!(flag_like["total_matches"], 1);

    let help = run_contextmink(&root, &["grep-terms", "--help"]);
    assert!(help.contains("--max-matched-files"));
    assert!(help.contains("--limit"));
    assert!(help.contains("--max-matches"));
    assert!(help.contains("--max-lines"));
}

#[test]
fn grep_stops_content_scan_at_matching_file_cap() {
    let root = fixture_root("grep-count-cap");
    let matches = root.join("matches");
    fs::create_dir_all(&matches).unwrap();
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
        fs::write(matches.join(name), "needle\n").unwrap();
    }

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "--term",
            "needle",
            "--max-count-files",
            "2",
            "--max-files",
            "2",
            "matches",
        ],
    );
    assert_envelope(&json, "grep-terms", "files");
    assert_eq!(json["shown"], 2);
    assert_eq!(json["matched_files_total"], 2);
    assert_eq!(json["matched_files_total_is_lower_bound"], true);
    assert_eq!(json["total_matches"], 2);
    assert_eq!(json["total_matches_is_lower_bound"], true);
    assert_eq!(json["candidate_files_scanned"], 5);
    assert_eq!(json["content_files_scanned"], 2);
    assert_eq!(json["cap_reason"], "matched_files");
    assert_eq!(json["truncated"], true);

    let guarded = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scan",
            "grep-terms",
            "--term",
            "needle",
            "--max-count-files",
            "2",
            "matches",
        ],
    );
    assert!(!guarded.status.success());
    let guarded_stdout = String::from_utf8(guarded.stdout).unwrap();
    let guarded_stderr = String::from_utf8(guarded.stderr).unwrap();
    assert!(guarded_stdout.contains("\"cap_reason\":\"matched_files\""));
    assert!(guarded_stderr.contains("--require-complete-scan"));
}

#[test]
fn grep_json_honors_global_sample_cap() {
    let root = fixture_root("grep-json-sample-cap");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "alpha",
            "sample.txt",
            "--lines-per-file",
            "3",
            "--max-sample-lines",
            "1",
        ],
    );
    assert_envelope(&json, "grep", "files");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["files"].as_array().unwrap().len(), 1);
    assert_eq!(json["files"][0]["samples"].as_array().unwrap().len(), 1);
    assert_eq!(json["sample_lines_shown"], 1);
    assert_eq!(json["cap_reason"], "samples");
    assert_eq!(json["truncated"], true);
}

#[test]
fn grep_supports_pattern_files_for_shell_fragile_regex() {
    let root = fixture_root("grep-pattern-file");
    fs::write(root.join("pattern.txt"), "\u{feff}alpha|beta\n").unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "--pattern-file",
            "pattern.txt",
            "sample.txt",
        ],
    );
    assert_envelope(&json, "grep", "files");
    assert_eq!(json["pattern"], "\"alpha|beta\"");
    // total_matches counts matching lines: "alpha beta", "alpha", "beta".
    assert_eq!(json["total_matches"], 3);
}

#[test]
fn grep_accepts_named_search_paths() {
    let root = fixture_root("grep-path-alias");

    let json = parse_json_output(&root, &["--json", "grep", "alpha", "--path", "sample.txt"]);
    assert_envelope(&json, "grep", "files");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["total_matches"], 2);
    assert_eq!(json["files"][0]["path"], "sample.txt");
}

#[test]
fn json_select_projects_array_fields_without_jq_filters() {
    let root = fixture_root("json-select");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "sidecar.json",
            "--array",
            "/textures",
            "--field",
            "index",
            "--field",
            "path",
        ],
    );
    assert_envelope(&json, "json-select", "rows");
    assert_eq!(json["total"], 2);
    assert_eq!(json["rows"][0]["fields"]["index"], "0");
    assert_eq!(json["rows"][0]["fields"]["path"], "\"World|A.blp\"");
}

#[test]
fn json_select_tolerates_msys_converted_json_pointers() {
    let root = fixture_root("json-select-msys-pointers");

    let output = Command::new(env!("CARGO_BIN_EXE_contextmink"))
        .current_dir(&root)
        .env("MSYSTEM", "MINGW64")
        .env("EXEPATH", r"C:\Program Files\Git\bin")
        .args([
            "--json",
            "json-select",
            "sidecar.json",
            "--array",
            "C:/Program Files/Git/textures",
            "--field",
            "C:/Program Files/Git/path",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "contextmink failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_envelope(&json, "json-select", "rows");
    assert_eq!(json["array"], "/textures");
    assert_eq!(json["fields"][0], "/path");
    assert_eq!(json["rows"][0]["fields"]["/path"], "\"World|A.blp\"");
}

#[test]
fn json_select_projects_jsonl_rows_and_limit_alias() {
    let root = fixture_root("json-select-jsonl");
    fs::write(
        root.join("queue.jsonl"),
        "{\"addr\":\"0x408690\",\"flags\":[\"custom_register_args\"]}\n{\"addr\":\"0x409880\",\"flags\":[\"fpu_or_reg_dropped\"]}\n",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "queue.jsonl",
            "--field",
            "addr",
            "--field",
            "flags",
            "--limit",
            "1",
        ],
    );

    assert_envelope(&json, "json-select", "rows");
    assert_eq!(json["input_format"], "jsonl");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["total"], 2);
    assert_eq!(json["truncated"], true);
    assert_eq!(json["rows"][0]["fields"]["addr"], "\"0x408690\"");
    assert_eq!(
        json["rows"][0]["fields"]["flags"],
        "[\"custom_register_args\"]"
    );
}

#[test]
fn limit_aliases_match_canonical_caps() {
    let root = fixture_root("limit-aliases");
    fs::write(root.join("extra.txt"), "alpha\n").unwrap();

    let files = parse_json_output(&root, &["--json", "files", ".", "--limit", "1"]);
    assert_envelope(&files, "files", "files");
    assert_eq!(files["shown"], 1);
    assert_eq!(files["truncated"], true);

    let json_find = parse_json_output(
        &root,
        &[
            "--json",
            "json-find",
            "sidecar.json",
            "--key-contains",
            "mode",
            "--limit",
            "1",
        ],
    );
    assert_envelope(&json_find, "json-find", "matches");
    assert_eq!(json_find["shown"], 1);
    assert_eq!(json_find["total"], 2);
    assert_eq!(json_find["truncated"], true);

    let db_path = root.join("limit.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE rows(id INTEGER PRIMARY KEY, label TEXT);
         INSERT INTO rows(label) VALUES ('a'), ('b');",
    )
    .unwrap();
    drop(conn);
    let sqlite = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "limit.sqlite",
            "--sql",
            "SELECT * FROM rows ORDER BY id",
            "--limit",
            "1",
        ],
    );
    assert_envelope(&sqlite, "sqlite", "rows");
    assert_eq!(sqlite["shown"], 1);
    assert_eq!(sqlite["total"], 2);
    assert_eq!(sqlite["truncated"], true);
    assert_eq!(sqlite["rows_total_is_lower_bound"], false);

    let sqlite_scan = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "limit.sqlite",
            "--sql",
            "SELECT * FROM rows ORDER BY id",
            "--max-rows",
            "1",
            "--max-scan-rows",
            "1",
        ],
    );
    assert_envelope(&sqlite_scan, "sqlite", "rows");
    assert_eq!(sqlite_scan["rows_total_is_lower_bound"], true);
    assert_eq!(sqlite_scan["cap_reason"], "scan");

    let guarded_scan = run_contextmink_raw(
        &root,
        &[
            "--require-complete-scan",
            "sqlite",
            "limit.sqlite",
            "--sql",
            "SELECT * FROM rows ORDER BY id",
            "--max-rows",
            "1",
            "--max-scan-rows",
            "1",
        ],
    );
    assert!(!guarded_scan.status.success());
    assert!(
        String::from_utf8(guarded_scan.stderr)
            .unwrap()
            .contains("--require-complete-scan")
    );
}

#[test]
fn sqlite_reads_query_from_file_and_caps_rows() {
    let root = fixture_root("sqlite-query-file");
    let db_path = root.join("sample.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE pairs(id INTEGER PRIMARY KEY, left_value TEXT, right_value TEXT);
         INSERT INTO pairs(left_value, right_value) VALUES ('alpha', 'beta'), ('gamma', 'delta');",
    )
    .unwrap();
    drop(conn);
    fs::write(
        root.join("query.sql"),
        "\u{feff}SELECT id, left_value || ':' || right_value AS joined FROM pairs ORDER BY id\n",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "--path",
            "sample.sqlite",
            "--sql-file",
            "query.sql",
            "--max-rows",
            "1",
        ],
    );
    assert_envelope(&json, "sqlite", "rows");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["total"], 2);
    assert_eq!(json["cap_reason"], "rows");
    assert_eq!(json["rows"][0]["fields"]["joined"], "\"alpha:beta\"");

    let duplicate_db = run_contextmink_raw(
        &root,
        &[
            "--json",
            "sqlite",
            "sample.sqlite",
            "--db",
            "sample.sqlite",
            "--sql",
            "SELECT 1",
        ],
    );
    assert!(!duplicate_db.status.success());
    assert!(
        String::from_utf8(duplicate_db.stderr)
            .unwrap()
            .contains("either positional <DB> or --db/--path <DB>, not both")
    );
}

#[test]
fn sqlite_schema_reports_tables_columns_foreign_keys_and_indexes() {
    let root = fixture_root("sqlite-schema");
    let db_path = root.join("schema.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE parent(rowid INTEGER PRIMARY KEY, label TEXT NOT NULL UNIQUE) STRICT;
         CREATE TABLE child(rowid INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES parent(rowid), note TEXT) STRICT;
         CREATE INDEX child_parent_id_idx ON child(parent_id);
         CREATE INDEX child_note_expr_idx ON child(coalesce(note, ''));",
    )
    .unwrap();
    drop(conn);

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite-schema",
            "--path",
            "schema.sqlite",
            "--table",
            "child",
        ],
    );
    assert_envelope(&json, "sqlite-schema", "tables");
    assert_eq!(json["shown"], 1);
    assert_eq!(json["tables"][0]["name"], "child");
    assert_eq!(json["tables"][0]["strict"], true);
    assert_eq!(json["tables"][0]["columns_total"], 3);
    assert_eq!(json["tables"][0]["columns"][1]["name"], "parent_id");
    assert_eq!(
        json["tables"][0]["columns"][1]["foreign_key"]["table"],
        "parent"
    );
    let indexes = json["tables"][0]["indexes"].as_array().unwrap();
    let parent_index = indexes
        .iter()
        .find(|index| index["name"] == "child_parent_id_idx")
        .unwrap();
    assert_eq!(parent_index["columns"][0], "parent_id");
    let expr_index = indexes
        .iter()
        .find(|index| index["name"] == "child_note_expr_idx")
        .unwrap();
    assert_eq!(expr_index["columns"][0], "<expr>");

    let capped = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite-schema",
            "schema.sqlite",
            "--max-tables",
            "1",
            "--max-columns",
            "1",
        ],
    );
    assert_eq!(capped["truncated"], true);
    assert!(matches!(
        capped["cap_reason"].as_str(),
        Some("tables") | Some("columns")
    ));
}

#[test]
fn slice_past_eof_is_complete_when_every_available_line_is_shown() {
    let root = fixture_root("slice-past-eof");

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "slice",
            "sample.txt",
            "--start",
            "1",
            "--end",
            "260",
        ],
    );
    assert_envelope(&json, "slice", "lines");
    assert_eq!(json["shown"], 3);
    assert_eq!(json["total"], 3);
    assert_eq!(json["end"], 3);
    assert_eq!(json["truncated"], false);
    assert_eq!(json["complete"], true);
    assert!(json["cap_reason"].is_null());
}

#[test]
fn grep_filters_by_extension_and_glob() {
    let root = fixture_root("grep-ext-glob");
    fs::write(root.join("code.rs"), "needle in rust\n").unwrap();
    fs::write(root.join("notes.md"), "needle in markdown\n").unwrap();

    let by_ext = parse_json_output(&root, &["--json", "grep", "needle", ".", "--ext", "rs"]);
    assert_envelope(&by_ext, "grep", "files");
    assert_eq!(by_ext["total"], 1);
    assert_eq!(by_ext["files"][0]["path"], "./code.rs");

    let by_glob = parse_json_output(&root, &["--json", "grep", "needle", ".", "--glob", "*.md"]);
    assert_envelope(&by_glob, "grep", "files");
    assert_eq!(by_glob["total"], 1);
    assert_eq!(by_glob["files"][0]["path"], "./notes.md");
}

#[test]
fn grep_ignore_case_matches_and_labels() {
    let root = fixture_root("grep-ignore-case");
    fs::write(root.join("mixed.txt"), "NeEdLe here\n").unwrap();

    let sensitive = parse_json_output(&root, &["--json", "grep", "needle", "mixed.txt"]);
    assert_eq!(sensitive["total_matches"], 0);

    let insensitive = parse_json_output(
        &root,
        &["--json", "grep", "-i", "--literal", "needle", "mixed.txt"],
    );
    assert_eq!(insensitive["total_matches"], 1);
    assert!(
        insensitive["pattern"]
            .as_str()
            .unwrap()
            .contains("ignore_case")
    );

    let terms = parse_json_output(
        &root,
        &[
            "--json",
            "grep-terms",
            "-i",
            "--term",
            "NEEDLE",
            "mixed.txt",
        ],
    );
    assert_eq!(terms["total_matches"], 1);
}

#[test]
fn grep_context_lines_render_with_dash_separator() {
    let root = fixture_root("grep-context");
    fs::write(root.join("ctx.txt"), "before\nneedle\nafter\n").unwrap();

    let json = parse_json_output(
        &root,
        &["--json", "grep", "needle", "ctx.txt", "--context", "1"],
    );
    let samples = json["files"][0]["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 3);
    assert_eq!(samples[0]["is_match"], false);
    assert_eq!(samples[1]["is_match"], true);
    assert_eq!(samples[2]["is_match"], false);

    let human = run_contextmink(&root, &["grep", "needle", "ctx.txt", "--context", "1"]);
    assert!(human.contains("ctx.txt:1-before"));
    assert!(human.contains("ctx.txt:2:needle"));
    assert!(human.contains("ctx.txt:3-after"));
}

#[test]
fn grep_scans_utf16_files_and_lists_skipped_files() {
    let root = fixture_root("grep-utf16-skips");
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "needle utf16\n".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(root.join("powershell.log"), utf16).unwrap();
    fs::write(root.join("binary.bin"), b"MZ\x00\x00needle").unwrap();

    let json = parse_json_output(&root, &["--json", "grep", "needle", "."]);
    assert_eq!(json["total_matches"], 1);
    assert_eq!(json["files"][0]["path"], "./powershell.log");
    assert_eq!(json["skipped_large_or_binary"], 1);
    let skipped = json["skipped_files_sample"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["path"], "./binary.bin");
    assert_eq!(skipped[0]["reason"], "binary");
}

#[test]
fn grep_no_match_scope_demotes_when_large_files_skipped() {
    let root = fixture_root("grep-large-skip-scope");
    fs::write(root.join("big.txt"), "x".repeat(64)).unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "grep",
            "not-present",
            "big.txt",
            "--max-file-bytes",
            "8",
        ],
    );
    assert_eq!(json["total_matches"], 0);
    assert_eq!(json["no_match_scope"], "scanned_subset");
    assert_eq!(json["skipped_files_sample"][0]["reason"], "large");
}

#[test]
fn slice_tail_returns_last_lines() {
    let root = fixture_root("slice-tail");

    let json = parse_json_output(&root, &["--json", "slice", "sample.txt", "--tail", "2"]);
    assert_envelope(&json, "slice", "lines");
    assert_eq!(json["shown"], 2);
    assert_eq!(json["lines"][0]["line"], 2);
    assert_eq!(json["lines"][0]["text"], "alpha");
    assert_eq!(json["lines"][1]["line"], 3);
    assert_eq!(json["lines"][1]["text"], "beta");
    assert_eq!(json["encoding"], "utf8");
}

#[test]
fn json_select_where_filters_rows() {
    let root = fixture_root("json-select-where");
    fs::write(
        root.join("queue.jsonl"),
        "{\"addr\":\"0x1\",\"state\":\"open\"}\n{\"addr\":\"0x2\",\"state\":\"closed\"}\n{\"addr\":\"0x3\",\"state\":\"open\"}\n",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "queue.jsonl",
            "--field",
            "addr",
            "--where",
            "state=open",
        ],
    );
    assert_envelope(&json, "json-select", "rows");
    assert_eq!(json["total"], 2);
    assert_eq!(json["rows_scanned"], 3);
    assert_eq!(json["rows"][0]["fields"]["addr"], "\"0x1\"");
    assert_eq!(json["rows"][1]["fields"]["addr"], "\"0x3\"");

    let contains = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "queue.jsonl",
            "--field",
            "addr",
            "--where-contains",
            "state=clo",
        ],
    );
    assert_eq!(contains["total"], 1);
    assert_eq!(contains["rows"][0]["fields"]["addr"], "\"0x2\"");
}

#[test]
fn json_select_reports_all_null_fields() {
    let root = fixture_root("json-select-all-null");
    fs::write(
        root.join("rows.jsonl"),
        "{\"addr\":\"0x1\"}\n{\"addr\":\"0x2\"}\n",
    )
    .unwrap();

    let json = parse_json_output(
        &root,
        &[
            "--json",
            "json-select",
            "rows.jsonl",
            "--field",
            "addr",
            "--field",
            "typo_field",
        ],
    );
    let all_null = json["all_null_fields"].as_array().unwrap();
    assert_eq!(all_null.len(), 1);
    assert_eq!(all_null[0], "typo_field");

    let human = run_contextmink(
        &root,
        &["json-select", "rows.jsonl", "--field", "typo_field"],
    );
    assert!(human.contains("warning: field(s) typo_field"));
}

#[test]
fn json_commands_tolerate_utf8_bom_documents() {
    let root = fixture_root("json-bom");
    fs::write(root.join("bom.json"), b"\xEF\xBB\xBF{\"mode\":\"demo\"}").unwrap();

    let json = parse_json_output(
        &root,
        &["--json", "json-find", "bom.json", "--key-contains", "mode"],
    );
    assert_eq!(json["total"], 1);

    let select = parse_json_output(
        &root,
        &["--json", "json-select", "bom.json", "--field", "mode"],
    );
    assert_eq!(select["rows"][0]["fields"]["mode"], "\"demo\"");
}

#[test]
fn dirs_reports_bounded_recursive_file_counts() {
    let root = fixture_root("dirs-overview");
    fs::create_dir_all(root.join("crates").join("alpha").join("src")).unwrap();
    fs::create_dir_all(root.join("crates").join("beta")).unwrap();
    fs::write(
        root.join("crates").join("alpha").join("src").join("lib.rs"),
        "x\n",
    )
    .unwrap();
    fs::write(root.join("crates").join("alpha").join("Cargo.toml"), "x\n").unwrap();
    fs::write(root.join("crates").join("beta").join("Cargo.toml"), "x\n").unwrap();

    let json = parse_json_output(&root, &["--json", "dirs", "crates", "--depth", "1"]);
    assert_envelope(&json, "dirs", "dirs");
    let dirs = json["dirs"].as_array().unwrap();
    let find = |name: &str| {
        dirs.iter()
            .find(|dir| dir["path"] == name)
            .unwrap_or_else(|| panic!("missing dir {name} in {dirs:?}"))
    };
    assert_eq!(find("crates")["files"], 3);
    assert_eq!(find("crates/alpha")["files"], 2);
    assert_eq!(find("crates/beta")["files"], 1);

    let deeper = parse_json_output(&root, &["--json", "dirs", "crates", "--depth", "2"]);
    let dirs = deeper["dirs"].as_array().unwrap();
    assert!(dirs.iter().any(|dir| dir["path"] == "crates/alpha/src"));
}

#[test]
fn config_typos_fail_fast() {
    let root = fixture_root("config-typo");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"x\"\nexclude_glob = [\"typo/**\"]\n",
    )
    .unwrap();

    let output = run_contextmink_raw(&root, &["files", ".", "--max", "1"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unknown key `exclude_glob`")
    );
}

#[test]
fn receipts_carry_duration_ms() {
    let root = fixture_root("duration-ms");

    let json = parse_json_output(&root, &["--json", "files", ".", "--max", "1"]);
    assert!(json["duration_ms"].is_number());
}

#[test]
fn excludes_hold_for_absolute_scan_roots() {
    let root = fixture_root("absolute-root-policy");
    fs::write(
        root.join(".contextmink.toml"),
        "profile = \"test-profile\"\nexclude_globs = [\"artifacts/**\"]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("artifacts")).unwrap();
    fs::write(root.join("artifacts").join("big.log"), "noise\n").unwrap();

    // Anchored excludes must hold even when the scan root is an absolute
    // path (or the command runs from a subdirectory), not only for
    // config-root-relative spellings.
    let absolute_root = root.to_string_lossy().replace('\\', "/");
    let files = parse_json_output(&root, &["--json", "files", &absolute_root, "--max", "50"]);
    assert_envelope(&files, "files", "files");
    let listed = files["files"].as_array().unwrap();
    assert!(
        listed
            .iter()
            .all(|path| !path.as_str().unwrap().contains("artifacts/")),
        "absolute-root scan must honor anchored excludes: {listed:?}"
    );
    assert!(
        listed
            .iter()
            .any(|path| path.as_str().unwrap().ends_with("sample.txt"))
    );

    // An explicit absolute path INTO the excluded tree is still the target.
    let absolute_excluded = format!("{absolute_root}/artifacts");
    let explicit = parse_json_output(
        &root,
        &["--json", "files", &absolute_excluded, "--max", "10"],
    );
    assert_eq!(explicit["total"], 1);
    assert!(
        explicit["files"][0]
            .as_str()
            .unwrap()
            .ends_with("artifacts/big.log")
    );
}

#[test]
fn sqlite_timeout_interrupts_runaway_queries() {
    let root = fixture_root("sqlite-timeout");
    let db_path = root.join("tiny.sqlite");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY);")
        .unwrap();
    drop(conn);

    // A nonterminating recursive CTE must be interrupted, not hang.
    let output = run_contextmink_raw(
        &root,
        &[
            "sqlite",
            "--path",
            "tiny.sqlite",
            "--timeout-secs",
            "1",
            "--sql",
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c) SELECT count(*) FROM c",
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("interrupted after --timeout-secs 1"),
        "stderr: {stderr}"
    );

    // A normal query under the same budget still succeeds.
    let ok = parse_json_output(
        &root,
        &[
            "--json",
            "sqlite",
            "--path",
            "tiny.sqlite",
            "--timeout-secs",
            "1",
            "--sql",
            "SELECT 1 AS one",
        ],
    );
    assert_eq!(ok["rows"][0]["fields"]["one"], "1");
}
