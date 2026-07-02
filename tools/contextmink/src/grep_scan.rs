//! Single-pass per-file content scanning with bounded sample/context capture,
//! plus a deterministic chunk-parallel executor.
//!
//! Files are scanned by worker threads in walk-order chunks, but results are
//! consumed strictly in walk order, so output and receipts are identical to a
//! sequential run; parallelism only changes wall-clock time and how much
//! wasted work happens past an early-stop boundary.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use anyhow::Result;

use crate::encoding::{FileText, read_file_text};
use crate::output::clamp_text;
use crate::text::TextMatcher;

const CHUNK_SIZE: usize = 32;
const MAX_WORKERS: usize = 8;

#[derive(Debug)]
pub(crate) struct SampleLine {
    pub(crate) line: usize,
    pub(crate) text: String,
    pub(crate) is_match: bool,
}

#[derive(Debug)]
pub(crate) enum FileScan {
    Matched {
        path: PathBuf,
        /// Count of matching lines in the file.
        matching_lines: usize,
        /// Sample matching lines with optional surrounding context lines,
        /// in file order.
        samples: Vec<SampleLine>,
    },
    NoMatch,
    SkippedLarge {
        path: PathBuf,
        bytes: u64,
    },
    SkippedBinary {
        path: PathBuf,
    },
}

pub(crate) struct ScanLimits {
    pub(crate) lines_per_file: usize,
    pub(crate) context: usize,
    pub(crate) max_line_chars: usize,
    pub(crate) max_file_bytes: u64,
}

pub(crate) fn scan_file(
    path: PathBuf,
    matcher: &TextMatcher,
    limits: &ScanLimits,
) -> Result<FileScan> {
    let text = match read_file_text(&path, limits.max_file_bytes)? {
        FileText::Text { text, .. } => text,
        FileText::SkippedLarge { bytes } => return Ok(FileScan::SkippedLarge { path, bytes }),
        FileText::SkippedBinary => return Ok(FileScan::SkippedBinary { path }),
    };
    let mut matching_lines = 0usize;
    let mut samples: Vec<SampleLine> = Vec::new();
    let mut sampled_matches = 0usize;
    // One-based line number of the last line already present in `samples`,
    // used to avoid duplicating overlapping context windows.
    let mut last_sampled_line = 0usize;
    let mut after_context_remaining = 0usize;
    let mut recent: Vec<(usize, &str)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let matched = matcher.is_match(line);
        if matched {
            matching_lines += 1;
        }
        if matched && sampled_matches < limits.lines_per_file {
            sampled_matches += 1;
            for (context_number, context_line) in &recent {
                if *context_number > last_sampled_line {
                    samples.push(SampleLine {
                        line: *context_number,
                        text: clamp_text(context_line.trim_end(), limits.max_line_chars),
                        is_match: false,
                    });
                    last_sampled_line = *context_number;
                }
            }
            samples.push(SampleLine {
                line: number,
                text: clamp_text(line.trim(), limits.max_line_chars),
                is_match: true,
            });
            last_sampled_line = number;
            after_context_remaining = limits.context;
        } else if matched && after_context_remaining > 0 {
            // A match inside another match's after-context window still
            // counts and renders as a match line.
            samples.push(SampleLine {
                line: number,
                text: clamp_text(line.trim(), limits.max_line_chars),
                is_match: true,
            });
            last_sampled_line = number;
            after_context_remaining = limits.context;
        } else if after_context_remaining > 0 {
            samples.push(SampleLine {
                line: number,
                text: clamp_text(line.trim_end(), limits.max_line_chars),
                is_match: false,
            });
            last_sampled_line = number;
            after_context_remaining -= 1;
        }
        if limits.context > 0 {
            recent.push((number, line));
            if recent.len() > limits.context {
                recent.remove(0);
            }
        }
    }
    if matching_lines == 0 {
        return Ok(FileScan::NoMatch);
    }
    Ok(FileScan::Matched {
        path,
        matching_lines,
        samples,
    })
}

/// Scan candidate files in parallel while preserving sequential semantics.
///
/// `should_stop(scan)` is invoked on results in walk order; once it returns
/// true, later files are neither consumed nor counted. Returns the consumed
/// prefix of results plus how many files were consumed.
pub(crate) fn scan_files_ordered(
    files: Vec<PathBuf>,
    matcher: &TextMatcher,
    limits: &ScanLimits,
    mut should_stop: impl FnMut(&FileScan) -> bool,
) -> Result<(Vec<FileScan>, usize)> {
    let total = files.len();
    if total == 0 {
        return Ok((Vec::new(), 0));
    }
    let workers = thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1)
        .min(MAX_WORKERS)
        .min(total.div_ceil(CHUNK_SIZE));
    if workers <= 1 {
        let mut consumed = Vec::new();
        for path in files {
            let scan = scan_file(path, matcher, limits)?;
            let stop = should_stop(&scan);
            consumed.push(scan);
            if stop {
                break;
            }
        }
        let count = consumed.len();
        return Ok((consumed, count));
    }

    let chunk_count = total.div_ceil(CHUNK_SIZE);
    let next_chunk = AtomicUsize::new(0);
    let stop_flag = AtomicBool::new(false);
    let files_ref = &files;
    let next_ref = &next_chunk;
    let stop_ref = &stop_flag;

    let mut consumed: Vec<FileScan> = Vec::new();
    let mut consumed_count = 0usize;
    let mut error: Option<anyhow::Error> = None;

    thread::scope(|scope| {
        let (sender, receiver) = std::sync::mpsc::channel::<(usize, Result<Vec<FileScan>>)>();
        for _ in 0..workers {
            let sender = sender.clone();
            scope.spawn(move || {
                loop {
                    if stop_ref.load(Ordering::Relaxed) {
                        return;
                    }
                    let chunk = next_ref.fetch_add(1, Ordering::Relaxed);
                    if chunk >= chunk_count {
                        return;
                    }
                    let start = chunk * CHUNK_SIZE;
                    let end = (start + CHUNK_SIZE).min(files_ref.len());
                    let mut results = Vec::with_capacity(end - start);
                    let mut failure: Option<anyhow::Error> = None;
                    for path in &files_ref[start..end] {
                        match scan_file(path.clone(), matcher, limits) {
                            Ok(scan) => results.push(scan),
                            Err(scan_error) => {
                                failure = Some(scan_error);
                                break;
                            }
                        }
                    }
                    let payload = match failure {
                        Some(scan_error) => Err(scan_error),
                        None => Ok(results),
                    };
                    if sender.send((chunk, payload)).is_err() {
                        // Receiver already stopped consuming.
                        return;
                    }
                }
            });
        }
        drop(sender);

        // Consume chunk results strictly in walk order on this thread;
        // out-of-order arrivals wait in `pending`.
        let mut pending: std::collections::BTreeMap<usize, Result<Vec<FileScan>>> =
            std::collections::BTreeMap::new();
        let mut next_expected = 0usize;
        'consume: while next_expected < chunk_count {
            let results = match pending.remove(&next_expected) {
                Some(result) => result,
                None => match receiver.recv() {
                    Ok((chunk, result)) if chunk == next_expected => result,
                    Ok((chunk, result)) => {
                        pending.insert(chunk, result);
                        continue;
                    }
                    Err(_) => break, // All workers exited (stop flag or done).
                },
            };
            next_expected += 1;
            match results {
                Ok(results) => {
                    for scan in results {
                        let stop = should_stop(&scan);
                        consumed.push(scan);
                        consumed_count += 1;
                        if stop {
                            stop_ref.store(true, Ordering::Relaxed);
                            break 'consume;
                        }
                    }
                }
                Err(scan_error) => {
                    error = Some(scan_error);
                    stop_ref.store(true, Ordering::Relaxed);
                    break 'consume;
                }
            }
        }
        stop_ref.store(true, Ordering::Relaxed);
        // Drain remaining sends so workers blocked on an unbounded channel
        // never exist (channel is unbounded, sends never block), then fall
        // out of scope; scoped threads join here.
        drop(receiver);
    });

    match error {
        Some(error) => Err(error),
        None => Ok((consumed, consumed_count)),
    }
}

#[cfg(test)]
mod tests;
