use std::io::Read;

use anyhow::{Context, Result};

use super::tags::{
    decode_html, extract_tag_contents, find_tag_end, index_of_ignore_case, is_block_tag,
    is_paragraph_block_tag, is_skip_tag, parse_tag_descriptor, starts_with_at,
};
use crate::research::model::ExtractionQuality;
pub(in crate::research::web_fetch) fn extract_readable_text(
    html: &str,
    max_bytes: usize,
) -> String {
    let candidate = extract_tag_contents(html, "article")
        .or_else(|| extract_tag_contents(html, "main"))
        .or_else(|| extract_tag_contents(html, "body"))
        .unwrap_or(html);
    let mut output = String::new();
    let mut index = 0usize;
    let mut skip_depth = 0usize;

    while index < candidate.len() {
        let Some(next_lt) = candidate[index..].find('<') else {
            if skip_depth == 0 {
                append_text_segment(&mut output, &candidate[index..]);
            }
            break;
        };

        let tag_start = index + next_lt;
        if skip_depth == 0 {
            append_text_segment(&mut output, &candidate[index..tag_start]);
        }

        if starts_with_at(candidate, tag_start, "<!--") {
            if let Some(end) = index_of_ignore_case(candidate, "-->", tag_start + 4) {
                index = end + 3;
            } else {
                break;
            }
            continue;
        }

        let Some(tag_end) = find_tag_end(candidate, tag_start) else {
            break;
        };
        let raw_tag = &candidate[tag_start..=tag_end];
        let Some((tag_name, is_closing, is_self_closing)) = parse_tag_descriptor(raw_tag) else {
            index = tag_end + 1;
            continue;
        };

        if is_closing {
            if is_skip_tag(tag_name) && skip_depth > 0 {
                skip_depth -= 1;
            }
            if skip_depth == 0 {
                if is_paragraph_block_tag(tag_name) {
                    append_separator(&mut output, "\n\n");
                } else if is_block_tag(tag_name) {
                    append_separator(&mut output, "\n");
                }
            }
        } else if is_skip_tag(tag_name) && !is_self_closing {
            skip_depth += 1;
        } else if skip_depth == 0 {
            if tag_name == "br" {
                append_separator(&mut output, "\n");
            } else if tag_name == "li" {
                append_separator(&mut output, "\n- ");
            } else if is_paragraph_block_tag(tag_name) {
                append_separator(&mut output, "\n\n");
            } else if is_block_tag(tag_name) {
                append_separator(&mut output, "\n");
            }
        }

        index = tag_end + 1;
    }

    normalize_extracted_text(&output, max_bytes)
}

fn append_text_segment(output: &mut String, text: &str) {
    let decoded = decode_html(text);
    if !decoded.is_empty() {
        output.push_str(&decoded);
    }
}

fn append_separator(output: &mut String, separator: &str) {
    if output.is_empty() {
        return;
    }
    match separator {
        "\n\n" => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if output.ends_with("\n\n") {
                return;
            }
            if output.ends_with('\n') {
                output.push('\n');
            } else {
                output.push_str("\n\n");
            }
        }
        "\n- " => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if output.ends_with('\n') {
                output.push_str("- ");
            } else {
                output.push_str("\n- ");
            }
        }
        "\n" => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }
        other => output.push_str(other),
    }
}

pub(in crate::research::web_fetch) fn normalize_extracted_text(
    value: &str,
    max_bytes: usize,
) -> String {
    let mut lines = Vec::new();
    let mut blank_count = 0usize;
    for line in value.lines() {
        let collapsed = collapse_inline_whitespace(line);
        if collapsed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                lines.push(String::new());
            }
            continue;
        }
        blank_count = 0;
        lines.push(collapsed);
    }
    let mut merged = merge_isolated_bullet_markers(&lines);
    compact_adjacent_list_item_spacing(&mut merged);
    while matches!(merged.first(), Some(line) if line.is_empty()) {
        merged.remove(0);
    }
    while matches!(merged.last(), Some(line) if line.is_empty()) {
        merged.pop();
    }
    truncate_to_byte_limit(&merged.join("\n"), max_bytes)
}

/// Drop isolated single-character list markers (`-`, `*`, `•`) emitted as their own
/// line by HTML-to-text extractors, joining them with the following text line. Also
/// strips leading image/credit attribution lines (`© …`) that sit above their related
/// prose but carry no reader-facing content on their own.
fn merge_isolated_bullet_markers(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    while index < lines.len() {
        let line = &lines[index];
        let trimmed = line.trim();
        if matches!(trimmed, "-" | "*" | "\u{2022}")
            && let Some((next_index, next)) = next_nonblank_line(lines, index + 1)
            && !next.trim().is_empty()
        {
            out.push(format!("- {}", next.trim()));
            index = next_index + 1;
            continue;
        }
        index += 1;
        out.push(line.clone());
    }
    out
}

fn next_nonblank_line(lines: &[String], start: usize) -> Option<(usize, &str)> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| (index, line.as_str()))
}

fn compact_adjacent_list_item_spacing(lines: &mut Vec<String>) {
    let mut index = 1usize;
    while index + 1 < lines.len() {
        if lines[index].is_empty()
            && is_markdown_unordered_list_item(&lines[index - 1])
            && is_markdown_unordered_list_item(&lines[index + 1])
        {
            lines.remove(index);
            continue;
        }
        index += 1;
    }
}

fn is_markdown_unordered_list_item(line: &str) -> bool {
    line.trim_start().starts_with("- ")
}

pub(in crate::research::web_fetch) fn collapse_inline_whitespace(value: &str) -> String {
    let mut output = String::new();
    let mut pending_space = false;

    for ch in value.chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !output.is_empty() {
            output.push(' ');
        }
        output.push(ch);
        pending_space = false;
    }

    output.trim().to_string()
}

pub(in crate::research::web_fetch) fn summarize_text(
    value: &str,
    max_chars: usize,
) -> Option<String> {
    let text = collapse_inline_whitespace(value);
    if text.is_empty() {
        return None;
    }
    let mut output = String::new();
    for ch in text.chars().take(max_chars) {
        output.push(ch);
    }
    Some(output)
}

pub(in crate::research::web_fetch) fn score_extraction_quality(
    text: &str,
    extract: Option<&str>,
) -> ExtractionQuality {
    let word_count = text.split_whitespace().count();
    if word_count >= 350 {
        return ExtractionQuality::High;
    }
    if word_count >= 40 || extract.is_some_and(|value| value.len() >= 40) {
        return ExtractionQuality::Medium;
    }
    ExtractionQuality::Low
}

pub(in crate::research::web_fetch) fn detect_app_shell_html(html: &str) -> bool {
    let lowered = html.to_ascii_lowercase();
    let signals = [
        "__next_f.push(",
        "_next/static/",
        "__next_data__",
        "data-reactroot",
        "ng-version",
        "window.__nuxt__",
        "id=\"app\"",
        "id='app'",
    ];
    signals
        .iter()
        .filter(|signal| lowered.contains(**signal))
        .count()
        >= 2
}

pub(in crate::research::web_fetch) fn detect_access_challenge(html: &str) -> bool {
    if detect_access_challenge_vendor(html).is_some() {
        return true;
    }
    detect_generic_access_challenge(html)
}

pub(in crate::research::web_fetch) fn detect_access_challenge_vendor(
    html: &str,
) -> Option<&'static str> {
    let lowered = html.to_ascii_lowercase();
    let vendor_signals = [
        ("cloudflare", "__cf_chl_"),
        ("cloudflare", "cf-browser-verification"),
        ("cloudflare", "just a moment..."),
        ("aws_waf", "awswafintegration"),
        ("aws_waf", "awswafcookiedomainlist"),
        ("anubis", "anubis-auth"),
        ("anubis", "anubis-cookie-verification"),
        ("anubis", "/.within.website/x/cmd/anubis"),
        ("anubis", "making sure you're not a bot"),
        ("anubis", "proof-of-work challenge"),
        ("datadome", "captcha-delivery"),
        ("datadome", "datadome"),
        ("perimeterx", "perimeterx"),
        ("perimeterx", "px-captcha"),
    ];
    for (vendor, signal) in vendor_signals {
        if lowered.contains(signal) {
            return Some(vendor);
        }
    }
    None
}

// Generic markers are individually weak: "challenge-container", "captcha", and
// "access denied" all occur on legitimate pages, so any single one is not enough
// to classify a fetch as an anti-bot wall. Require at least two co-occurring signals.
// Unambiguous vendor fingerprints are handled in detect_access_challenge_vendor.
fn detect_generic_access_challenge(html: &str) -> bool {
    let lowered = html.to_ascii_lowercase();
    let generic_signals = [
        "verify that you're not a robot",
        "checking your browser",
        "challenge-container",
        "enable javascript and then reload the page",
        "javascript is disabled",
        "access denied",
        "captcha",
        "challenge.js",
    ];
    generic_signals
        .iter()
        .filter(|signal| lowered.contains(**signal))
        .count()
        >= 2
}

pub(in crate::research::web_fetch) fn read_text_body_limited<R: Read>(
    reader: R,
    max_bytes: usize,
) -> Result<String> {
    if max_bytes == 0 {
        return Ok(String::new());
    }

    let mut body = Vec::with_capacity(max_bytes.min(8192));
    let mut limited = reader.take(max_bytes as u64);
    limited
        .read_to_end(&mut body)
        .context("failed to read response body")?;

    let body = strip_utf8_bom(&body);
    let text = String::from_utf8_lossy(body);
    Ok(truncate_to_byte_limit(&text, max_bytes))
}

fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    const UTF8_BOM: &[u8; 3] = b"\xEF\xBB\xBF";

    bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes)
}

pub(crate) fn truncate_to_byte_limit(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
