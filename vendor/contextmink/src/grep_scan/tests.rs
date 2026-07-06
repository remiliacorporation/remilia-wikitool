use super::*;
use crate::text::TextMatcher;
use std::fs;
use std::path::Path;

fn limits(lines_per_file: usize, context: usize) -> ScanLimits {
    ScanLimits {
        lines_per_file,
        context,
        max_line_chars: 200,
        max_file_bytes: 1_000_000,
    }
}

fn write_fixture(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

fn fixture_dir(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("contextmink-scan-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir); // guardrail: allow-ignore-result cleanup is best-effort for reused test temp dirs
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn counts_matching_lines_not_occurrences() {
    let dir = fixture_dir("count-lines");
    let path = write_fixture(&dir, "a.txt", "alpha alpha alpha\nnone\nalpha\n");
    let matcher = TextMatcher::new("alpha", true, false).unwrap();
    match scan_file(path, &matcher, &limits(3, 0)).unwrap() {
        FileScan::Matched {
            matching_lines,
            samples,
            ..
        } => {
            assert_eq!(matching_lines, 2);
            assert_eq!(samples.len(), 2);
            assert!(samples.iter().all(|sample| sample.is_match));
        }
        other => panic!("expected match, got {other:?}"),
    }
}

#[test]
fn captures_context_lines_without_duplicates() {
    let dir = fixture_dir("context");
    let path = write_fixture(&dir, "a.txt", "one\ntwo\nneedle\nfour\nneedle\nsix\n");
    let matcher = TextMatcher::new("needle", true, false).unwrap();
    match scan_file(path, &matcher, &limits(4, 1)).unwrap() {
        FileScan::Matched { samples, .. } => {
            let rendered: Vec<(usize, bool)> = samples
                .iter()
                .map(|sample| (sample.line, sample.is_match))
                .collect();
            assert_eq!(
                rendered,
                vec![(2, false), (3, true), (4, false), (5, true), (6, false)]
            );
        }
        other => panic!("expected match, got {other:?}"),
    }
}

#[test]
fn utf16_files_are_scanned_not_skipped() {
    let dir = fixture_dir("utf16");
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "needle text\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let path = dir.join("utf16.log");
    fs::write(&path, bytes).unwrap();
    let matcher = TextMatcher::new("needle", true, false).unwrap();
    assert!(matches!(
        scan_file(path, &matcher, &limits(1, 0)).unwrap(),
        FileScan::Matched { .. }
    ));
}

#[test]
fn ordered_scan_matches_sequential_semantics() {
    let dir = fixture_dir("ordered");
    let mut files = Vec::new();
    for index in 0..100 {
        let text = if index % 3 == 0 {
            format!("needle {index}\n")
        } else {
            format!("plain {index}\n")
        };
        files.push(write_fixture(&dir, &format!("file_{index:03}.txt"), &text));
    }
    let matcher = TextMatcher::new("needle", true, false).unwrap();
    let scan_limits = limits(1, 0);
    let mut matched = 0usize;
    let (results, consumed) = scan_files_ordered(files, &matcher, &scan_limits, |scan| {
        if matches!(scan, FileScan::Matched { .. }) {
            matched += 1;
        }
        matched >= 5
    })
    .unwrap();
    assert_eq!(matched, 5);
    // The fifth match is file_012 (0,3,6,9,12), the 13th file in order.
    assert_eq!(consumed, 13);
    assert_eq!(results.len(), 13);
    let matched_paths: Vec<String> = results
        .iter()
        .filter_map(|scan| match scan {
            FileScan::Matched { path, .. } => {
                Some(path.file_name().unwrap().to_string_lossy().into_owned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        matched_paths,
        vec![
            "file_000.txt",
            "file_003.txt",
            "file_006.txt",
            "file_009.txt",
            "file_012.txt"
        ]
    );
}
