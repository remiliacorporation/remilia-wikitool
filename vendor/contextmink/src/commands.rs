use std::cmp::min;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::files::{CollectOptions, collect_files, display_path};
use crate::grep_scan::{FileScan, SampleLine, ScanLimits, scan_files_ordered};
use crate::merged_paths;
use crate::output::{
    base_receipt, clamp_text, emit_json_checked, no_match_scope, write_receipt_checked,
};
use crate::text::{TextMatcher, collect_single_text_source, parse_line_range};

const SKIPPED_FILES_SAMPLE_LIMIT: usize = 5;
/// Nested-repo disclosure is capped so a workspace with dozens of vendored
/// repos does not flood the transcript; the total count is always exact.
const NESTED_REPOS_RECEIPT_LIMIT: usize = 12;
const NESTED_REPOS_HUMAN_LIMIT: usize = 8;

fn nested_repos_receipt_fields(map: &mut serde_json::Map<String, Value>, nested: &[String]) {
    map.insert(
        "nested_repos_entered_total".to_string(),
        json!(nested.len()),
    );
    map.insert(
        "nested_repos_entered".to_string(),
        json!(
            nested
                .iter()
                .take(NESTED_REPOS_RECEIPT_LIMIT)
                .collect::<Vec<_>>()
        ),
    );
}

#[derive(Debug)]
struct FileMatch {
    path: PathBuf,
    count: usize,
    samples: Vec<SampleLine>,
}

#[derive(Debug)]
struct SkippedFile {
    path: PathBuf,
    reason: &'static str,
    bytes: Option<u64>,
}

pub(crate) struct GrepCaps {
    pub(crate) max_count_files: usize,
    pub(crate) max_files: usize,
    pub(crate) lines_per_file: usize,
    pub(crate) context: usize,
    pub(crate) max_sample_lines: usize,
    pub(crate) max_line_chars: usize,
    pub(crate) max_scan_files: usize,
    pub(crate) max_file_bytes: u64,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_files(
    cli: &Cli,
    config: &ContextConfig,
    paths: &[PathBuf],
    globs: &[String],
    path_terms: &[String],
    extensions: &[String],
    with_excluded: bool,
    with_git_ignored: bool,
    skip_nested_repos: bool,
    quiet: bool,
    max: usize,
    max_line_chars: usize,
    max_scan_files: usize,
) -> Result<()> {
    if max_scan_files == 0 {
        return Err(anyhow!("files --max-scan-files must be greater than zero"));
    }
    let collected = collect_files(
        paths,
        config,
        &CollectOptions {
            globs,
            path_terms,
            extensions,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max_scan_files,
        },
    )?;
    let files = collected.files;
    let shown = min(files.len(), max);
    let truncated = collected.truncated || shown < files.len();
    let cap_reason = if collected.truncated {
        Some("scan")
    } else if shown < files.len() {
        Some("max")
    } else {
        None
    };
    let mut map = base_receipt(
        "files",
        config.profile.as_deref(),
        "files",
        shown,
        collected.total_seen,
        truncated,
        cap_reason,
    );
    map.insert("candidate_files_scanned".to_string(), json!(files.len()));
    // Enumeration always completes; the scan cap bounds the candidate list,
    // not the count, so the total is exact.
    map.insert(
        "candidate_files_total_is_lower_bound".to_string(),
        json!(false),
    );
    nested_repos_receipt_fields(&mut map, &collected.nested_repos_entered);
    // --quiet suppresses only the file list; every receipt field (totals,
    // caps, truncation, nested-repo disclosure) stays intact.
    if quiet {
        map.insert("quiet".to_string(), json!(true));
    }
    if cli.json {
        if !quiet {
            map.insert(
                "files".to_string(),
                json!(
                    files
                        .iter()
                        .take(shown)
                        .map(|path| display_path(path))
                        .collect::<Vec<_>>()
                ),
            );
        }
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        if !quiet {
            for path in files.iter().take(shown) {
                writeln!(
                    stdout,
                    "{}",
                    clamp_text(&display_path(path), max_line_chars)
                )?;
            }
        }
        write_nested_repos_note(&mut stdout, &collected.nested_repos_entered)?;
        if collected.truncated {
            writeln!(
                stdout,
                "[contextmink] capped file scan at {max_scan_files} files; narrow the path or glob before treating this as complete."
            )?;
        }
        write_receipt_checked(cli, map)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_dirs(
    cli: &Cli,
    config: &ContextConfig,
    paths: &[PathBuf],
    depth: usize,
    with_excluded: bool,
    with_git_ignored: bool,
    skip_nested_repos: bool,
    max: usize,
    max_line_chars: usize,
    max_scan_files: usize,
) -> Result<()> {
    if depth == 0 {
        return Err(anyhow!("dirs --depth must be greater than zero"));
    }
    if max_scan_files == 0 {
        return Err(anyhow!("dirs --max-scan-files must be greater than zero"));
    }
    let collected = collect_files(
        paths,
        config,
        &CollectOptions {
            globs: &[],
            path_terms: &[],
            extensions: &[],
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max_scan_files,
        },
    )?;
    let root_prefixes: Vec<String> = paths
        .iter()
        .map(|root| {
            display_path(root)
                .trim_start_matches("./")
                .trim_end_matches('/')
                .to_owned()
        })
        .filter(|root| !root.is_empty() && root != ".")
        .collect();
    // Recursive file counts per directory, keyed by display path, bounded to
    // `depth` levels below the matched root (or below the scan origin when
    // the root is the current directory).
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for file in &collected.files {
        let display = display_path(file);
        let display = display.trim_start_matches("./");
        let root_prefix = root_prefixes
            .iter()
            .filter(|root| display == **root || display.starts_with(&format!("{root}/")))
            .max_by_key(|root| root.len());
        let (base, relative) = match root_prefix {
            Some(root) => (
                root.as_str(),
                display
                    .strip_prefix(root.as_str())
                    .unwrap_or("")
                    .trim_start_matches('/'),
            ),
            None => ("", display),
        };
        let components: Vec<&str> = relative.split('/').collect();
        // The last component is the file name; ancestors are directories.
        let dir_components = components.len().saturating_sub(1);
        for level in 0..=min(dir_components, depth) {
            let mut key = base.to_owned();
            if level > 0 {
                if !key.is_empty() {
                    key.push('/');
                }
                key.push_str(&components[..level].join("/"));
            }
            let key = if key.is_empty() { ".".to_owned() } else { key };
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    let total_dirs = counts.len();
    let shown = min(total_dirs, max);
    let truncated = collected.truncated || shown < total_dirs;
    let cap_reason = if collected.truncated {
        Some("scan")
    } else if shown < total_dirs {
        Some("max")
    } else {
        None
    };
    let mut map = base_receipt(
        "dirs",
        config.profile.as_deref(),
        "dirs",
        shown,
        total_dirs,
        truncated,
        cap_reason,
    );
    map.insert("depth".to_string(), json!(depth));
    map.insert("files_counted".to_string(), json!(collected.files.len()));
    // Enumeration always completes; the scan cap bounds the candidate list,
    // not the count, so the total is exact.
    map.insert(
        "candidate_files_total_is_lower_bound".to_string(),
        json!(false),
    );
    nested_repos_receipt_fields(&mut map, &collected.nested_repos_entered);
    if cli.json {
        map.insert(
            "dirs".to_string(),
            json!(
                counts
                    .iter()
                    .take(shown)
                    .map(|(dir, files)| json!({"path": dir, "files": files}))
                    .collect::<Vec<_>>()
            ),
        );
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        writeln!(stdout, "[contextmink] dirs depth={depth}")?;
        if counts.is_empty() {
            writeln!(stdout, "no_files")?;
        }
        for (dir, files) in counts.iter().take(shown) {
            writeln!(stdout, "{} files={files}", clamp_text(dir, max_line_chars))?;
        }
        write_nested_repos_note(&mut stdout, &collected.nested_repos_entered)?;
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped dirs output; narrow the path or lower --depth before treating this as complete."
            )?;
        }
        write_receipt_checked(cli, map)
    }
}

fn write_nested_repos_note(stdout: &mut impl Write, nested: &[String]) -> Result<()> {
    if nested.is_empty() {
        return Ok(());
    }
    let mut listed = nested
        .iter()
        .take(NESTED_REPOS_HUMAN_LIMIT)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if nested.len() > NESTED_REPOS_HUMAN_LIMIT {
        listed.push_str(&format!(
            ", ... and {} more",
            nested.len() - NESTED_REPOS_HUMAN_LIMIT
        ));
    }
    writeln!(
        stdout,
        "[contextmink] entered {} git-ignored nested repo(s): {listed}",
        nested.len()
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_grep(
    cli: &Cli,
    config: &ContextConfig,
    args: &[String],
    named_paths: &[PathBuf],
    pattern_arg: Option<&str>,
    pattern_file: Option<&Path>,
    literal: bool,
    ignore_case: bool,
    globs: &[String],
    extensions: &[String],
    with_excluded: bool,
    with_git_ignored: bool,
    skip_nested_repos: bool,
    quiet: bool,
    caps: &GrepCaps,
) -> Result<()> {
    let (pattern, effective_paths) = if pattern_arg.is_some() || pattern_file.is_some() {
        (None, merged_paths(&string_args_to_paths(args), named_paths))
    } else {
        let Some((pattern, paths)) = args.split_first() else {
            return Err(anyhow!(
                "grep requires PATTERN, --pattern <pattern>, or --pattern-file <file>"
            ));
        };
        (
            Some(pattern.as_str()),
            merged_paths(&string_args_to_paths(paths), named_paths),
        )
    };
    let pattern =
        collect_single_text_source("grep pattern", pattern.or(pattern_arg), pattern_file, true)?;
    let matcher = TextMatcher::new(&pattern, literal, ignore_case)?;
    command_grep_with_matcher(
        cli,
        config,
        "grep",
        matcher,
        &effective_paths,
        globs,
        extensions,
        with_excluded,
        with_git_ignored,
        skip_nested_repos,
        quiet,
        caps,
    )
}

fn string_args_to_paths(args: &[String]) -> Vec<PathBuf> {
    args.iter().map(PathBuf::from).collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_grep_with_matcher(
    cli: &Cli,
    config: &ContextConfig,
    command_name: &str,
    matcher: TextMatcher,
    paths: &[PathBuf],
    globs: &[String],
    extensions: &[String],
    with_excluded: bool,
    with_git_ignored: bool,
    skip_nested_repos: bool,
    quiet: bool,
    caps: &GrepCaps,
) -> Result<()> {
    if caps.max_count_files == 0 {
        return Err(anyhow!(
            "{command_name} --max-count-files must be greater than zero"
        ));
    }
    if caps.max_scan_files == 0 {
        return Err(anyhow!(
            "{command_name} --max-scan-files must be greater than zero"
        ));
    }
    let collected = collect_files(
        paths,
        config,
        &CollectOptions {
            globs,
            path_terms: &[],
            extensions,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max_scan_files: caps.max_scan_files,
        },
    )?;
    let scan_truncated = collected.truncated;
    let total_candidate_files = collected.total_seen;
    let candidate_files_scanned = collected.files.len();
    let nested_repos_entered = collected.nested_repos_entered;

    let limits = ScanLimits {
        lines_per_file: caps.lines_per_file,
        context: caps.context,
        max_line_chars: caps.max_line_chars,
        max_file_bytes: caps.max_file_bytes,
    };
    let mut matched_so_far = 0usize;
    let (scans, content_files_scanned) =
        scan_files_ordered(collected.files, &matcher, &limits, |scan| {
            if matches!(scan, FileScan::Matched { .. }) {
                matched_so_far += 1;
            }
            matched_so_far >= caps.max_count_files
        })?;
    let mut matches: Vec<FileMatch> = Vec::new();
    let mut total_matches = 0usize;
    let mut skipped: Vec<SkippedFile> = Vec::new();
    let mut skipped_large = 0usize;
    for scan in scans {
        match scan {
            FileScan::Matched {
                path,
                matching_lines,
                samples,
            } => {
                total_matches += matching_lines;
                matches.push(FileMatch {
                    path,
                    count: matching_lines,
                    samples,
                });
            }
            FileScan::SkippedLarge { path, bytes } => {
                skipped_large += 1;
                skipped.push(SkippedFile {
                    path,
                    reason: "large",
                    bytes: Some(bytes),
                });
            }
            FileScan::SkippedBinary { path } => {
                skipped.push(SkippedFile {
                    path,
                    reason: "binary",
                    bytes: None,
                });
            }
            FileScan::NoMatch => {}
        }
    }
    let content_match_capped =
        matches.len() >= caps.max_count_files && content_files_scanned < candidate_files_scanned;
    let matched_files_total_is_lower_bound =
        scan_truncated || content_match_capped || skipped_large > 0;
    let total_matches_is_lower_bound = matched_files_total_is_lower_bound;
    let files_shown = min(matches.len(), caps.max_files);
    // A no-match verdict only covers scanned content: a capped candidate
    // scan or unexamined large files leave text unexamined. Binary skips do
    // not demote scope because binary content has no greppable lines.
    let no_match_scan_incomplete = scan_truncated || skipped_large > 0;

    if cli.json {
        let mut sample_lines_shown = 0usize;
        let mut sample_capped = false;
        let mut files_json = Vec::new();
        for row in matches.iter().take(files_shown) {
            let mut samples = Vec::new();
            for sample in &row.samples {
                if sample_lines_shown >= caps.max_sample_lines {
                    sample_capped = true;
                    break;
                }
                sample_lines_shown += 1;
                samples.push(json!({
                    "line": sample.line,
                    "text": sample.text,
                    "is_match": sample.is_match,
                }));
            }
            files_json.push(json!({
                "path": display_path(&row.path),
                "count": row.count,
                "samples": samples,
            }));
        }
        let cap_reason = grep_cap_reason(
            scan_truncated,
            matched_files_total_is_lower_bound,
            files_shown < matches.len(),
            sample_capped,
        );
        let mut map = base_receipt(
            command_name,
            config.profile.as_deref(),
            "files",
            files_shown,
            matches.len(),
            cap_reason.is_some(),
            cap_reason,
        );
        map.insert("pattern".to_string(), json!(matcher.label()));
        map.insert("matched_files_total".to_string(), json!(matches.len()));
        map.insert("matched_files_shown".to_string(), json!(files_shown));
        map.insert(
            "matched_files_total_is_lower_bound".to_string(),
            json!(matched_files_total_is_lower_bound),
        );
        map.insert("total_matches".to_string(), json!(total_matches));
        map.insert(
            "total_matches_is_lower_bound".to_string(),
            json!(total_matches_is_lower_bound),
        );
        map.insert("sample_lines_shown".to_string(), json!(sample_lines_shown));
        map.insert(
            "candidate_files_total".to_string(),
            json!(total_candidate_files),
        );
        map.insert(
            "candidate_files_scanned".to_string(),
            json!(candidate_files_scanned),
        );
        map.insert(
            "content_files_scanned".to_string(),
            json!(content_files_scanned),
        );
        map.insert(
            "candidate_files_total_is_lower_bound".to_string(),
            json!(false),
        );
        insert_skip_fields(&mut map, &skipped);
        nested_repos_receipt_fields(&mut map, &nested_repos_entered);
        map.insert(
            "no_match_scope".to_string(),
            json!(no_match_scope(matches.is_empty(), no_match_scan_incomplete)),
        );
        // --quiet suppresses only the match content; every receipt field
        // above (totals, lower bounds, caps, scan scope) is still emitted.
        if quiet {
            map.insert("quiet".to_string(), json!(true));
        } else {
            map.insert("files".to_string(), json!(files_json));
        }
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        writeln!(stdout, "[contextmink] grep pattern={}", matcher.label())?;
        writeln!(
            stdout,
            "matched_files_total={} matched_files_total_is_lower_bound={} total_matches={} total_matches_is_lower_bound={}",
            matches.len(),
            matched_files_total_is_lower_bound,
            total_matches,
            total_matches_is_lower_bound
        )?;
        writeln!(
            stdout,
            "candidate_files_total={} candidate_files_scanned={} content_files_scanned={} skipped_large_or_binary={}",
            total_candidate_files,
            candidate_files_scanned,
            content_files_scanned,
            skipped.len()
        )?;
        write_nested_repos_note(&mut stdout, &nested_repos_entered)?;
        write_skipped_note(&mut stdout, &skipped, caps.max_line_chars)?;
        if matches.is_empty() {
            writeln!(stdout, "no_matches")?;
            if no_match_scan_incomplete {
                writeln!(
                    stdout,
                    "[contextmink] scan was capped or skipped large files; no matches were found in the scanned subset only."
                )?;
            }
            let cap_reason = if scan_truncated { Some("scan") } else { None };
            let mut map = grep_receipt_map(
                command_name,
                config,
                0,
                0,
                total_matches,
                0,
                total_candidate_files,
                candidate_files_scanned,
                content_files_scanned,
                matched_files_total_is_lower_bound,
                total_matches_is_lower_bound,
                cap_reason,
                &matcher,
                &skipped,
                &nested_repos_entered,
                no_match_scan_incomplete,
                true,
            );
            if quiet {
                map.insert("quiet".to_string(), json!(true));
            }
            return write_receipt_checked(cli, map);
        }
        // --quiet suppresses only the match content below; the shown/capped
        // accounting still runs so the receipt is identical either way.
        if !quiet {
            writeln!(stdout, "file_counts:")?;
            for row in matches.iter().take(files_shown) {
                writeln!(
                    stdout,
                    "  {}:{}",
                    clamp_text(&display_path(&row.path), caps.max_line_chars),
                    row.count
                )?;
            }
        }
        let mut sample_total = 0usize;
        let mut sample_capped = false;
        if caps.lines_per_file > 0 && files_shown > 0 {
            if !quiet {
                writeln!(stdout, "sample_lines:")?;
            }
            'samples: for row in matches.iter().take(files_shown) {
                for sample in &row.samples {
                    if sample_total >= caps.max_sample_lines {
                        if !quiet {
                            writeln!(
                                stdout,
                                "[contextmink] capped sample lines at {}; narrow the query.",
                                caps.max_sample_lines
                            )?;
                        }
                        sample_capped = true;
                        break 'samples;
                    }
                    if !quiet {
                        let separator = if sample.is_match { ':' } else { '-' };
                        writeln!(
                            stdout,
                            "  {}:{}{}{}",
                            clamp_text(&display_path(&row.path), caps.max_line_chars),
                            sample.line,
                            separator,
                            sample.text
                        )?;
                    }
                    sample_total += 1;
                }
            }
        }
        let cap_reason = grep_cap_reason(
            scan_truncated,
            matched_files_total_is_lower_bound,
            files_shown < matches.len(),
            sample_capped,
        );
        if matches!(
            cap_reason,
            Some("scan") | Some("matched_files") | Some("files")
        ) {
            writeln!(
                stdout,
                "[contextmink] capped grep output or scan; narrow the path or pattern before treating this as complete."
            )?;
        }
        let mut map = grep_receipt_map(
            command_name,
            config,
            files_shown,
            matches.len(),
            total_matches,
            sample_total,
            total_candidate_files,
            candidate_files_scanned,
            content_files_scanned,
            matched_files_total_is_lower_bound,
            total_matches_is_lower_bound,
            cap_reason,
            &matcher,
            &skipped,
            &nested_repos_entered,
            no_match_scan_incomplete,
            false,
        );
        if quiet {
            map.insert("quiet".to_string(), json!(true));
        }
        write_receipt_checked(cli, map)
    }
}

fn grep_cap_reason(
    scan_truncated: bool,
    matched_files_lower_bound: bool,
    files_capped: bool,
    sample_capped: bool,
) -> Option<&'static str> {
    if scan_truncated {
        Some("scan")
    } else if matched_files_lower_bound {
        Some("matched_files")
    } else if files_capped {
        Some("files")
    } else if sample_capped {
        Some("samples")
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn grep_receipt_map(
    command_name: &str,
    config: &ContextConfig,
    files_shown: usize,
    matched_files_total: usize,
    total_matches: usize,
    sample_lines_shown: usize,
    candidate_files_total: usize,
    candidate_files_scanned: usize,
    content_files_scanned: usize,
    matched_files_total_is_lower_bound: bool,
    total_matches_is_lower_bound: bool,
    cap_reason: Option<&str>,
    matcher: &TextMatcher,
    skipped: &[SkippedFile],
    nested_repos_entered: &[String],
    no_match_scan_incomplete: bool,
    no_matches: bool,
) -> serde_json::Map<String, Value> {
    let mut map = base_receipt(
        command_name,
        config.profile.as_deref(),
        "files",
        files_shown,
        matched_files_total,
        cap_reason.is_some(),
        cap_reason,
    );
    map.insert("pattern".to_string(), json!(matcher.label()));
    map.insert("total_matches".to_string(), json!(total_matches));
    map.insert("sample_lines_shown".to_string(), json!(sample_lines_shown));
    map.insert(
        "candidate_files_total".to_string(),
        json!(candidate_files_total),
    );
    map.insert(
        "candidate_files_scanned".to_string(),
        json!(candidate_files_scanned),
    );
    map.insert(
        "content_files_scanned".to_string(),
        json!(content_files_scanned),
    );
    map.insert(
        "candidate_files_total_is_lower_bound".to_string(),
        json!(false),
    );
    map.insert(
        "matched_files_total_is_lower_bound".to_string(),
        json!(matched_files_total_is_lower_bound),
    );
    map.insert(
        "total_matches_is_lower_bound".to_string(),
        json!(total_matches_is_lower_bound),
    );
    insert_skip_fields(&mut map, skipped);
    nested_repos_receipt_fields(&mut map, nested_repos_entered);
    map.insert(
        "no_match_scope".to_string(),
        json!(no_match_scope(no_matches, no_match_scan_incomplete)),
    );
    map
}

fn insert_skip_fields(map: &mut serde_json::Map<String, Value>, skipped: &[SkippedFile]) {
    map.insert("skipped_large_or_binary".to_string(), json!(skipped.len()));
    // Split counts: only skipped *large* files (unexamined text) mark match
    // totals as lower bounds; binary skips are out-of-domain for text search.
    let large = skipped.iter().filter(|skip| skip.reason == "large").count();
    map.insert("skipped_large".to_string(), json!(large));
    map.insert("skipped_binary".to_string(), json!(skipped.len() - large));
    map.insert(
        "skipped_files_sample".to_string(),
        json!(
            skipped
                .iter()
                .take(SKIPPED_FILES_SAMPLE_LIMIT)
                .map(|skip| {
                    json!({
                        "path": display_path(&skip.path),
                        "reason": skip.reason,
                        "bytes": skip.bytes,
                    })
                })
                .collect::<Vec<_>>()
        ),
    );
}

fn write_skipped_note(
    stdout: &mut impl Write,
    skipped: &[SkippedFile],
    max_line_chars: usize,
) -> Result<()> {
    if skipped.is_empty() {
        return Ok(());
    }
    let sample = skipped
        .iter()
        .take(SKIPPED_FILES_SAMPLE_LIMIT)
        .map(|skip| format!("{} ({})", display_path(&skip.path), skip.reason))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        stdout,
        "[contextmink] skipped {} file(s) without scanning: {}",
        skipped.len(),
        clamp_text(&sample, max_line_chars * 2)
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_slice(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    range: Option<&str>,
    start: usize,
    end: Option<usize>,
    tail: Option<usize>,
    lines: usize,
    max_lines: usize,
    max_line_chars: usize,
    char_start: Option<usize>,
    chars: usize,
) -> Result<()> {
    if let Some(char_start) = char_start {
        if range.is_some() || tail.is_some() {
            return Err(anyhow!(
                "slice --char-start cannot be combined with --range or --tail"
            ));
        }
        return command_slice_chars(cli, config, file, char_start, chars, max_line_chars);
    }
    let (text, encoding) = crate::encoding::read_required_text(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let text_lines = text.lines().collect::<Vec<_>>();
    let total_lines = text_lines.len();
    let (start, requested_end) = if let Some(tail) = tail {
        if range.is_some() || start != 1 || end.is_some() {
            return Err(anyhow!(
                "slice --tail cannot be combined with --range, --start, or --end"
            ));
        }
        if tail == 0 {
            return Err(anyhow!("slice --tail must be greater than zero"));
        }
        (total_lines.saturating_sub(tail) + 1, total_lines)
    } else {
        let (start, end) = if let Some(range) = range {
            if start != 1 || end.is_some() {
                return Err(anyhow!(
                    "slice --range cannot be combined with --start or --end"
                ));
            }
            parse_line_range(range)?
        } else {
            (start.max(1), end)
        };
        (
            start,
            end.unwrap_or(start.saturating_add(lines).saturating_sub(1)),
        )
    };
    let capped_end = min(
        requested_end,
        start.saturating_add(max_lines).saturating_sub(1),
    );
    let mut rendered = Vec::new();
    for number in start..=capped_end {
        if let Some(line) = text_lines.get(number - 1) {
            rendered.push((number, clamp_text(line, max_line_chars)));
        }
    }
    let last_available = min(requested_end, total_lines);
    let truncated = start <= total_lines && last_available > capped_end;
    let shown = if start > total_lines {
        0
    } else {
        min(capped_end, total_lines).saturating_sub(start) + 1
    };
    let displayed_end = if shown == 0 {
        start.saturating_sub(1)
    } else {
        min(capped_end, total_lines)
    };
    let cap_reason = if truncated { Some("max_lines") } else { None };
    let mut map = base_receipt(
        "slice",
        config.profile.as_deref(),
        "lines",
        shown,
        total_lines,
        truncated,
        cap_reason,
    );
    map.insert("path".to_string(), json!(display_path(file)));
    map.insert("mode".to_string(), json!("lines"));
    map.insert("encoding".to_string(), json!(encoding));
    map.insert("start".to_string(), json!(start));
    map.insert("end".to_string(), json!(displayed_end));
    map.insert("total_lines".to_string(), json!(total_lines));
    // Whole-file scan (the read already happened); the field only exists
    // when something was found, so clean files cost nothing.
    let suspects = crate::encoding::scan_encoding_suspects(&text, false);
    if !suspects.is_empty() {
        map.insert("encoding_suspects".to_string(), suspects.receipt_value());
    }
    if cli.json {
        map.insert(
            "lines".to_string(),
            json!(
                rendered
                    .iter()
                    .map(|(line, text)| json!({
                        "line": line,
                        "text": text,
                    }))
                    .collect::<Vec<_>>()
            ),
        );
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        for (line, text) in rendered {
            writeln!(stdout, "{line}: {text}")?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped slice at {max_lines} lines; request a narrower range."
            )?;
        }
        if !suspects.is_empty() {
            writeln!(stdout, "{}", suspects.human_note())?;
        }
        write_receipt_checked(cli, map)
    }
}

fn command_slice_chars(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    char_start: usize,
    chars: usize,
    _max_line_chars: usize,
) -> Result<()> {
    let (text, encoding) = crate::encoding::read_required_text(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let total_chars = text.chars().count();
    let shown_text = text
        .chars()
        .skip(char_start)
        .take(chars)
        .collect::<String>();
    let shown = shown_text.chars().count();
    let truncated = char_start + shown < total_chars;
    let cap_reason = if truncated { Some("max_chars") } else { None };
    let mut map = base_receipt(
        "slice",
        config.profile.as_deref(),
        "chars",
        shown,
        total_chars,
        truncated,
        cap_reason,
    );
    map.insert("path".to_string(), json!(display_path(file)));
    map.insert("mode".to_string(), json!("chars"));
    map.insert("encoding".to_string(), json!(encoding));
    map.insert("char_start".to_string(), json!(char_start));
    map.insert("chars_shown".to_string(), json!(shown));
    map.insert("total_chars".to_string(), json!(total_chars));
    if cli.json {
        map.insert("text".to_string(), json!(shown_text));
        return emit_json_checked(cli, Value::Object(map));
    }
    let mut stdout = io::stdout();
    write!(stdout, "{}", shown_text)?;
    if !shown_text.ends_with('\n') {
        writeln!(stdout)?;
    }
    map.remove("text");
    write_receipt_checked(cli, map)
}
