use std::io::{self, Read, Write};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::output::{base_receipt, clamp_text, emit_json, write_receipt_checked};

struct RawCapturedStream {
    /// First `max_bytes` of the stream.
    head: Vec<u8>,
    /// Last `max_bytes` of the stream (empty when the head holds everything).
    tail: Vec<u8>,
    /// Absolute byte offset where `tail` begins.
    tail_start: usize,
    total_bytes: usize,
    total_lines: usize,
}

struct CapturedStream {
    text: String,
    total_bytes: usize,
    captured_bytes: usize,
    total_lines: usize,
    shown_lines: usize,
    head_lines: usize,
    tail_lines: usize,
    omitted_lines: usize,
    byte_truncated: bool,
    line_truncated: bool,
    char_truncated: bool,
}

pub(crate) fn command_capture(
    cli: &Cli,
    config: &ContextConfig,
    max_lines: usize,
    max_bytes: usize,
    max_line_chars: usize,
    argv: &[String],
) -> Result<()> {
    if max_lines == 0 {
        return Err(anyhow!("capture --max-lines must be greater than zero"));
    }
    if max_bytes == 0 {
        return Err(anyhow!("capture --max-bytes must be greater than zero"));
    }
    if max_line_chars == 0 {
        return Err(anyhow!(
            "capture --max-line-chars must be greater than zero"
        ));
    }
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| anyhow!("capture requires a command after --"))?;

    let started = Instant::now();
    let (mut child, effective_argv) = spawn_captured_child(program, args)?;

    let stdout_pipe = child
        .stdout
        .take()
        .context("failed to capture child stdout")?;
    let stderr_pipe = child
        .stderr
        .take()
        .context("failed to capture child stderr")?;
    let stdout_handle = thread::spawn(move || read_captured_stream(stdout_pipe, max_bytes));
    let stderr_handle = thread::spawn(move || read_captured_stream(stderr_pipe, max_bytes));
    let status = child
        .wait()
        .context("failed to wait for captured command")?;
    let stdout_raw = stdout_handle
        .join()
        .map_err(|_| anyhow!("stdout capture thread panicked"))?
        .context("failed to read captured stdout")?;
    let stderr_raw = stderr_handle
        .join()
        .map_err(|_| anyhow!("stderr capture thread panicked"))?
        .context("failed to read captured stderr")?;
    let stdout = render_captured_stream(stdout_raw, max_lines, max_line_chars);
    let stderr = render_captured_stream(stderr_raw, max_lines, max_line_chars);
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let shown = stdout.shown_lines + stderr.shown_lines;
    let total = stdout.total_lines + stderr.total_lines;
    let truncated = captured_stream_truncated(&stdout) || captured_stream_truncated(&stderr);
    let cap_reason = capture_cap_reason(&stdout, &stderr);

    let mut map = base_receipt(
        "capture",
        config.profile.as_deref(),
        "lines",
        shown,
        total,
        truncated,
        cap_reason,
    );
    map.insert("argv".to_string(), json!(argv));
    map.insert("effective_argv".to_string(), json!(effective_argv));
    map.insert(
        "spawn_fallback".to_string(),
        json!(effective_argv.as_ref().map(|_| "bash")),
    );
    map.insert("exit_code".to_string(), json!(status.code()));
    map.insert("success".to_string(), json!(status.success()));
    map.insert("duration_ms".to_string(), json!(duration_ms));
    map.insert("stdout".to_string(), captured_stream_json(&stdout));
    map.insert("stderr".to_string(), captured_stream_json(&stderr));

    if cli.json {
        let mut object = map;
        object.insert("stdout_text".to_string(), json!(stdout.text));
        object.insert("stderr_text".to_string(), json!(stderr.text));
        return emit_json(Value::Object(object));
    }

    let mut out = io::stdout();
    writeln!(
        out,
        "[contextmink] capture command={} exit_code={} success={} duration_ms={}",
        clamp_text(&format!("{argv:?}"), 500),
        status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "null".to_string()),
        status.success(),
        duration_ms
    )?;
    if let Some(effective_argv) = &effective_argv {
        writeln!(
            out,
            "spawn_fallback=bash effective_command={}",
            clamp_text(&format!("{effective_argv:?}"), 500)
        )?;
    }
    writeln!(
        out,
        "stdout: shown_lines={} total_lines={} captured_bytes={} total_bytes={}",
        stdout.shown_lines, stdout.total_lines, stdout.captured_bytes, stdout.total_bytes
    )?;
    if !stdout.text.is_empty() {
        writeln!(out, "{}", stdout.text)?;
    }
    writeln!(
        out,
        "stderr: shown_lines={} total_lines={} captured_bytes={} total_bytes={}",
        stderr.shown_lines, stderr.total_lines, stderr.captured_bytes, stderr.total_bytes
    )?;
    if !stderr.text.is_empty() {
        writeln!(out, "{}", stderr.text)?;
    }
    if truncated {
        writeln!(
            out,
            "[contextmink] capped captured output; rerun the underlying command with native filters or raise caps only after confirming command scope."
        )?;
    }
    write_receipt_checked(cli, map)
}

fn spawn_captured_child(
    program: &str,
    args: &[String],
) -> Result<(std::process::Child, Option<Vec<String>>)> {
    let mut command = ProcessCommand::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match command.spawn() {
        Ok(child) => Ok((child, None)),
        Err(error) if cfg!(windows) && error.raw_os_error() == Some(193) => {
            let Some(bash) = std::env::var_os("CONTEXTMINK_BASH") else {
                return Err(error)
                    .with_context(|| format!("failed to spawn captured command {program:?}"));
            };
            let mut effective_argv = Vec::with_capacity(args.len() + 2);
            effective_argv.push(bash.to_string_lossy().into_owned());
            effective_argv.push(program.to_owned());
            effective_argv.extend(args.iter().cloned());

            let mut fallback = ProcessCommand::new(&bash);
            fallback
                .arg(program)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let child = fallback.spawn().with_context(|| {
                format!(
                    "failed to spawn captured command {program:?} through CONTEXTMINK_BASH={}",
                    bash.to_string_lossy()
                )
            })?;
            Ok((child, Some(effective_argv)))
        }
        Err(error) => {
            Err(error).with_context(|| format!("failed to spawn captured command {program:?}"))
        }
    }
}

/// Retain the first and last `max_bytes` of the stream. Tool output puts its
/// verdict at the end (test summaries, compiler error totals), so keeping
/// only the head would drop exactly the part an agent needs most.
fn read_captured_stream<R: Read>(mut reader: R, max_bytes: usize) -> io::Result<RawCapturedStream> {
    let mut head = Vec::with_capacity(max_bytes.min(8192));
    let mut tail: Vec<u8> = Vec::new();
    let mut tail_start = 0usize;
    let mut total_bytes = 0usize;
    let mut newline_count = 0usize;
    let mut saw_any = false;
    let mut last_was_newline = false;
    let mut buffer = [0u8; 8192];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        saw_any = true;
        for byte in &buffer[..read] {
            if *byte == b'\n' {
                newline_count += 1;
                last_was_newline = true;
            } else {
                last_was_newline = false;
            }
        }
        let head_remaining = max_bytes.saturating_sub(head.len());
        if head_remaining > 0 {
            head.extend_from_slice(&buffer[..read.min(head_remaining)]);
        }
        if read > head_remaining {
            let overflow = &buffer[head_remaining..read];
            let overflow_start = total_bytes + head_remaining;
            if tail.is_empty() {
                tail_start = overflow_start;
            }
            tail.extend_from_slice(overflow);
            if tail.len() > max_bytes {
                let drop = tail.len() - max_bytes;
                tail.drain(..drop);
                tail_start += drop;
            }
        }
        total_bytes += read;
    }

    let total_lines = newline_count + usize::from(saw_any && !last_was_newline);
    Ok(RawCapturedStream {
        head,
        tail,
        tail_start,
        total_bytes,
        total_lines,
    })
}

fn render_captured_stream(
    raw: RawCapturedStream,
    max_lines: usize,
    max_line_chars: usize,
) -> CapturedStream {
    let captured_bytes = raw.head.len() + raw.tail.len();
    let byte_truncated = raw.total_bytes > captured_bytes;
    // Bytes between the head and the retained tail were dropped whenever the
    // tail does not start exactly where the head ended.
    let tail_contiguous = raw.tail.is_empty() || raw.tail_start == raw.head.len();

    let mut clamp_state = ClampState::default();
    let (head_lines, head_partial_last) = decode_lines(&raw.head);
    let mut head_lines = head_lines;
    if !raw.tail.is_empty() && head_partial_last && !tail_contiguous {
        // The head ends mid-line and the rest of that line was dropped.
        head_lines.pop();
    }
    let mut tail_lines = Vec::new();
    if !raw.tail.is_empty() {
        let (mut lines, _) = decode_lines(&raw.tail);
        if !tail_contiguous && !lines.is_empty() {
            // The first retained tail line is almost certainly partial.
            lines.remove(0);
        }
        tail_lines = lines;
    }

    let (text, head_shown, tail_shown, omitted_lines) = if tail_lines.is_empty() {
        if head_lines.len() <= max_lines {
            let shown = head_lines.len();
            let rendered = head_lines
                .iter()
                .map(|line| clamp_state.clamp(line, max_line_chars))
                .collect::<Vec<_>>()
                .join("\n");
            (rendered, shown, 0usize, 0usize)
        } else {
            // Everything fits in the head buffer but exceeds the line budget:
            // split the budget so the end of the output (summaries, error
            // totals) stays visible.
            let head_budget = max_lines / 2;
            let tail_shown = max_lines - head_budget;
            let omitted = head_lines.len() - max_lines;
            let mut parts = Vec::new();
            parts.extend(
                head_lines
                    .iter()
                    .take(head_budget)
                    .map(|line| clamp_state.clamp(line, max_line_chars)),
            );
            if omitted > 0 {
                parts.push(format!("[contextmink] ... omitted {omitted} line(s) ..."));
            }
            parts.extend(
                head_lines
                    .iter()
                    .skip(head_lines.len() - tail_shown)
                    .map(|line| clamp_state.clamp(line, max_line_chars)),
            );
            (parts.join("\n"), head_budget, tail_shown, omitted)
        }
    } else {
        let head_budget = max_lines / 2;
        let head_shown = head_lines.len().min(head_budget);
        let tail_budget = max_lines.saturating_sub(head_shown).max(1);
        let tail_shown = tail_lines.len().min(tail_budget);
        let omitted = raw
            .total_lines
            .saturating_sub(head_shown)
            .saturating_sub(tail_shown);
        let mut parts = Vec::new();
        parts.extend(
            head_lines
                .iter()
                .take(head_shown)
                .map(|line| clamp_state.clamp(line, max_line_chars)),
        );
        if omitted > 0 {
            parts.push(format!("[contextmink] ... omitted {omitted} line(s) ..."));
        }
        parts.extend(
            tail_lines
                .iter()
                .skip(tail_lines.len() - tail_shown)
                .map(|line| clamp_state.clamp(line, max_line_chars)),
        );
        (parts.join("\n"), head_shown, tail_shown, omitted)
    };

    let shown_lines = head_shown + tail_shown;
    CapturedStream {
        text,
        total_bytes: raw.total_bytes,
        captured_bytes,
        total_lines: raw.total_lines,
        shown_lines,
        head_lines: head_shown,
        tail_lines: tail_shown,
        omitted_lines,
        byte_truncated,
        line_truncated: omitted_lines > 0,
        char_truncated: clamp_state.truncated,
    }
}

#[derive(Default)]
struct ClampState {
    truncated: bool,
}

impl ClampState {
    fn clamp(&mut self, line: &str, max_line_chars: usize) -> String {
        if line.chars().count() > max_line_chars {
            self.truncated = true;
        }
        clamp_text(line, max_line_chars)
    }
}

/// Decode captured bytes into trimmed lines; the boolean reports whether the
/// final line lacked a terminating newline (possibly partial content).
fn decode_lines(bytes: &[u8]) -> (Vec<String>, bool) {
    let decoded = String::from_utf8_lossy(bytes);
    let partial_last = !decoded.is_empty() && !decoded.ends_with('\n');
    let lines = decoded
        .lines()
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect();
    (lines, partial_last)
}

fn captured_stream_truncated(stream: &CapturedStream) -> bool {
    stream.byte_truncated || stream.line_truncated || stream.char_truncated
}

fn capture_cap_reason(stdout: &CapturedStream, stderr: &CapturedStream) -> Option<&'static str> {
    if stdout.byte_truncated || stderr.byte_truncated {
        Some("bytes")
    } else if stdout.line_truncated || stderr.line_truncated {
        Some("lines")
    } else if stdout.char_truncated || stderr.char_truncated {
        Some("chars")
    } else {
        None
    }
}

fn captured_stream_json(stream: &CapturedStream) -> Value {
    json!({
        "shown_lines": stream.shown_lines,
        "head_lines": stream.head_lines,
        "tail_lines": stream.tail_lines,
        "omitted_lines": stream.omitted_lines,
        "total_lines": stream.total_lines,
        "captured_bytes": stream.captured_bytes,
        "total_bytes": stream.total_bytes,
        "truncated": captured_stream_truncated(stream),
        "byte_truncated": stream.byte_truncated,
        "line_truncated": stream.line_truncated,
        "char_truncated": stream.char_truncated,
    })
}
