//! Bounded file-to-text decoding shared by the text and JSON commands.
//!
//! Windows hosts routinely produce UTF-16 and BOM-prefixed UTF-8 files
//! (PowerShell `Out-File`, MSVC tool logs). Treating those as binary makes a
//! no-match report dishonest, so decoding is BOM-driven here instead of being
//! left to callers.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

#[derive(Debug)]
pub(crate) enum FileText {
    Text {
        text: String,
        encoding: &'static str,
    },
    SkippedLarge {
        bytes: u64,
    },
    SkippedBinary,
}

/// Read a file as text, honoring a byte cap and decoding by BOM.
///
/// Returns `SkippedLarge` when the file exceeds `max_file_bytes`,
/// `SkippedBinary` when a non-UTF-16 file contains NUL bytes, and decoded
/// text otherwise (UTF-8 decoding is lossy so mixed-encoding files still
/// surface their ASCII content).
pub(crate) fn read_file_text(path: &Path, max_file_bytes: u64) -> Result<FileText> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.len() > max_file_bytes {
        return Ok(FileText::SkippedLarge {
            bytes: metadata.len(),
        });
    }
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(decode_bytes(&bytes))
}

pub(crate) fn decode_bytes(bytes: &[u8]) -> FileText {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return FileText::Text {
            text: decode_utf16(&bytes[2..], u16::from_le_bytes),
            encoding: "utf16le",
        };
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return FileText::Text {
            text: decode_utf16(&bytes[2..], u16::from_be_bytes),
            encoding: "utf16be",
        };
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if bytes.contains(&0) {
        return FileText::SkippedBinary;
    }
    FileText::Text {
        text: String::from_utf8_lossy(bytes).into_owned(),
        encoding: "utf8",
    }
}

fn decode_utf16(bytes: &[u8], combine: fn([u8; 2]) -> u16) -> String {
    let units = bytes
        .chunks(2)
        .map(|pair| match pair {
            [a, b] => combine([*a, *b]),
            // A trailing odd byte decodes as a replacement character rather
            // than silently vanishing.
            _ => 0xFFFD,
        })
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

/// Suspicious encoding artifacts found in decoded text. Detection is
/// proof-based, not pattern-based: `double_encoded` counts only character
/// runs whose CP1252 byte round-trip decodes as valid multi-byte UTF-8 (the
/// three-character sequence an em-dash becomes when UTF-8 is re-read as
/// CP1252, and the like); `replacement_chars` counts U+FFFD, which means a
/// lossy decode already happened — possibly this read, since non-UTF-8 high
/// bytes decode lossily above; `c1_controls` counts raw C1-range characters
/// (CP1252 bytes read as Latin-1). Empty means no evidence found, not proof
/// of health.
#[derive(Debug, Default)]
pub(crate) struct EncodingSuspects {
    pub(crate) double_encoded: usize,
    pub(crate) replacement_chars: usize,
    pub(crate) c1_controls: usize,
    /// First double-encoded finding with its decoded repair.
    pub(crate) sample: Option<String>,
}

impl EncodingSuspects {
    pub(crate) fn is_empty(&self) -> bool {
        self.double_encoded == 0 && self.replacement_chars == 0 && self.c1_controls == 0
    }

    pub(crate) fn receipt_value(&self) -> serde_json::Value {
        serde_json::json!({
            "double_encoded": self.double_encoded,
            "replacement_chars": self.replacement_chars,
            "c1_controls": self.c1_controls,
            "sample": self.sample,
        })
    }

    pub(crate) fn human_note(&self) -> String {
        let mut parts = Vec::new();
        if self.double_encoded > 0 {
            parts.push(format!(
                "{} double-encoded UTF-8 run(s)",
                self.double_encoded
            ));
        }
        if self.replacement_chars > 0 {
            parts.push(format!(
                "{} replacement char(s) (lossy decode)",
                self.replacement_chars
            ));
        }
        if self.c1_controls > 0 {
            parts.push(format!("{} raw C1 control(s)", self.c1_controls));
        }
        let mut note = format!("[contextmink] encoding suspects: {}", parts.join(", "));
        if let Some(sample) = &self.sample {
            note.push_str("; ");
            note.push_str(sample);
        }
        note
    }
}

/// Scan decoded text for encoding suspects. `double_encode_only` skips the
/// replacement-char and C1 counters — captured child output may legitimately
/// carry lossy or control bytes, but a CP1252 round-trip that decodes as
/// valid multi-byte UTF-8 is proof of a double encode in any stream.
///
/// A CP1252 round trip is proof about bytes, not intent, so a lone 2-byte
/// run (an accented capital before punctuation, `CAFÉ»` -> ɻ) is trusted
/// only when its lead is Latin-1 or it clusters with a neighbor (see the
/// per-run comment below). Even so the result is "suspects", reported in
/// receipts only — never a failure.
pub(crate) fn scan_encoding_suspects(text: &str, double_encode_only: bool) -> EncodingSuspects {
    let mut suspects = EncodingSuspects::default();
    let mut runs: Vec<DoubleEncodeRun> = Vec::new();
    let mut line = 1usize;
    let mut char_pos = 0usize;
    let mut iter = text.char_indices().peekable();
    while let Some((index, ch)) = iter.next() {
        let this_char = char_pos;
        char_pos += 1;
        if ch == '\n' {
            line += 1;
            continue;
        }
        if ch == '\u{FFFD}' {
            if !double_encode_only {
                suspects.replacement_chars += 1;
            }
            continue;
        }
        if ('\u{80}'..='\u{9F}').contains(&ch) {
            if !double_encode_only {
                suspects.c1_controls += 1;
            }
            continue;
        }
        let Some(lead) = cp1252_byte(ch) else {
            continue;
        };
        let continuations = match lead {
            0xC2..=0xDF => 1usize,
            0xE0..=0xEF => 2,
            0xF0..=0xF4 => 3,
            _ => continue,
        };
        let mut bytes = vec![lead];
        let mut lookahead = iter.clone();
        for _ in 0..continuations {
            let Some((_, next)) = lookahead.next() else {
                break;
            };
            let Some(byte) = cp1252_byte(next) else {
                break;
            };
            if !(0x80..=0xBF).contains(&byte) {
                break;
            }
            bytes.push(byte);
        }
        if bytes.len() != continuations + 1 {
            continue;
        }
        // from_utf8 rejects overlong, surrogate, and out-of-range sequences,
        // so this is a proof the run re-decodes, not a resemblance check.
        let Ok(decoded) = std::str::from_utf8(&bytes) else {
            continue;
        };
        // A 3+ byte round trip is strong evidence on its own — three or four
        // adjacent CP1252-high characters that form valid UTF-8 do not occur
        // in legitimate text. A 2-byte run is weaker: an accented capital
        // next to punctuation (`CAFÉ»` -> ɻ) is a valid round trip but plain
        // typography. Trust a 2-byte run only when its lead is the Latin-1
        // supplement range (0xC2/0xC3 — é ï ñ « » © nbsp, the dominant real
        // hazard) or it clusters with a neighbor (dense non-Latin mojibake
        // like double-encoded Cyrillic, where every character corrupts).
        let strong = continuations >= 2 || matches!(lead, 0xC2 | 0xC3);
        let source: String = text[index..].chars().take(continuations + 1).collect();
        runs.push(DoubleEncodeRun {
            char_start: this_char,
            char_len: continuations + 1,
            line,
            strong,
            source,
            decoded: decoded.to_owned(),
        });
        char_pos += continuations;
        for _ in 0..continuations {
            iter.next();
        }
    }

    for i in 0..runs.len() {
        let keep = runs[i].strong || run_has_contiguous_neighbor(&runs, i);
        if !keep {
            continue;
        }
        suspects.double_encoded += 1;
        if suspects.sample.is_none() {
            let run = &runs[i];
            suspects.sample = Some(format!(
                "line {}: {:?} is double-encoded UTF-8 for {:?}",
                run.line, run.source, run.decoded
            ));
        }
    }
    suspects
}

#[derive(Debug)]
struct DoubleEncodeRun {
    char_start: usize,
    char_len: usize,
    line: usize,
    strong: bool,
    source: String,
    decoded: String,
}

/// True when an adjacent run abuts this one with no gap — the signature of
/// dense mojibake, where consecutive source characters all corrupt. Runs are
/// collected in ascending, non-overlapping `char_start` order, so a
/// contiguous neighbor can only be the immediately previous or next run;
/// checking just those two keeps the pass O(n) rather than O(n²).
fn run_has_contiguous_neighbor(runs: &[DoubleEncodeRun], i: usize) -> bool {
    let run = &runs[i];
    let end = run.char_start + run.char_len;
    let prev_abuts = i > 0 && {
        let prev = &runs[i - 1];
        prev.char_start + prev.char_len == run.char_start
    };
    let next_abuts = runs.get(i + 1).is_some_and(|next| next.char_start == end);
    prev_abuts || next_abuts
}

/// CP1252 byte for a character, when one exists. U+00A0..U+00FF map to
/// themselves; the 0x80..0x9F range holds the CP1252 specials.
fn cp1252_byte(ch: char) -> Option<u8> {
    match ch {
        '\u{A0}'..='\u{FF}' => Some(ch as u8),
        '\u{20AC}' => Some(0x80),
        '\u{201A}' => Some(0x82),
        '\u{0192}' => Some(0x83),
        '\u{201E}' => Some(0x84),
        '\u{2026}' => Some(0x85),
        '\u{2020}' => Some(0x86),
        '\u{2021}' => Some(0x87),
        '\u{02C6}' => Some(0x88),
        '\u{2030}' => Some(0x89),
        '\u{0160}' => Some(0x8A),
        '\u{2039}' => Some(0x8B),
        '\u{0152}' => Some(0x8C),
        '\u{017D}' => Some(0x8E),
        '\u{2018}' => Some(0x91),
        '\u{2019}' => Some(0x92),
        '\u{201C}' => Some(0x93),
        '\u{201D}' => Some(0x94),
        '\u{2022}' => Some(0x95),
        '\u{2013}' => Some(0x96),
        '\u{2014}' => Some(0x97),
        '\u{02DC}' => Some(0x98),
        '\u{2122}' => Some(0x99),
        '\u{0161}' => Some(0x9A),
        '\u{203A}' => Some(0x9B),
        '\u{0153}' => Some(0x9C),
        '\u{017E}' => Some(0x9E),
        '\u{0178}' => Some(0x9F),
        _ => None,
    }
}

/// Read a file that the caller requires to be text (JSON inputs, slices).
/// Size is uncapped; binary content is an error rather than a skip.
pub(crate) fn read_required_text(path: &Path) -> Result<(String, &'static str)> {
    match read_file_text(path, u64::MAX)? {
        FileText::Text { text, encoding } => Ok((text, encoding)),
        FileText::SkippedBinary => Err(anyhow::anyhow!(
            "{} contains NUL bytes and does not decode as text",
            path.display()
        )),
        FileText::SkippedLarge { .. } => unreachable!("size cap is u64::MAX"),
    }
}

#[cfg(test)]
mod tests;
