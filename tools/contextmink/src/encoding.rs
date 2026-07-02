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
