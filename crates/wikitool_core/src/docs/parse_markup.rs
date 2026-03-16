use super::*;
use std::collections::BTreeSet;

pub(super) fn extract_link_titles(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    let mut titles = Vec::new();
    while cursor + 1 < bytes.len() {
        if bytes[cursor..].starts_with(b"[[")
            && let Some(end) = find_delimited(bytes, cursor + 2, b"]]")
        {
            let body = &content[cursor + 2..end];
            let title = body
                .split('|')
                .next()
                .unwrap_or(body)
                .split('#')
                .next()
                .unwrap_or(body)
                .trim()
                .trim_start_matches(':');
            let normalized = normalize_title(title);
            if !normalized.is_empty()
                && !normalized.starts_with("http://")
                && !normalized.starts_with("https://")
            {
                titles.push(normalized);
            }
            cursor = end + 2;
            continue;
        }
        cursor += 1;
    }
    dedupe_strings(&mut titles);
    titles
}

pub(super) fn extract_template_titles(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    let mut titles = Vec::new();
    while cursor + 1 < bytes.len() {
        if bytes[cursor..].starts_with(b"{{") {
            let start = cursor + 2;
            let mut end = start;
            while end < bytes.len() {
                let ch = bytes[end] as char;
                if matches!(ch, '|' | '}' | '\n' | '\r') {
                    break;
                }
                end += 1;
            }
            if end > start {
                let name = normalize_title(content[start..end].trim());
                if !name.is_empty() && !name.starts_with('#') {
                    titles.push(name);
                }
            }
            cursor = end;
            continue;
        }
        cursor += 1;
    }
    dedupe_strings(&mut titles);
    titles
}

pub(super) fn decamelize(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 8);
    let mut previous_lower_or_digit = false;
    for ch in value.chars() {
        if ch.is_ascii_uppercase() && previous_lower_or_digit {
            output.push(' ');
        } else if (ch == '_' || ch == '-' || ch == '/') && !output.ends_with(' ') {
            output.push(' ');
            previous_lower_or_digit = false;
            continue;
        }
        output.push(ch);
        previous_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
    }
    collapse_whitespace(&output)
}

pub(super) fn strip_tagged_block(value: &str, tag_name: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let bytes = value.as_bytes();
    let lower_bytes = lower.as_bytes();
    let open_pattern = format!("<{tag_name}").into_bytes();
    let close_pattern = format!("</{tag_name}>").into_bytes();
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if lower_bytes[cursor..].starts_with(&open_pattern) {
            let Some(tag_end) = find_tag_end(bytes, cursor) else {
                break;
            };
            if let Some(close_start) =
                find_case_insensitive(lower_bytes, tag_end + 1, &close_pattern)
            {
                cursor = close_start + close_pattern.len();
                output.push(' ');
                continue;
            }
        }
        output.push(bytes[cursor] as char);
        cursor += 1;
    }

    collapse_whitespace(&output)
}

pub(super) fn find_balanced_braces(bytes: &[u8], start: usize) -> Option<usize> {
    if !bytes.get(start..)?.starts_with(b"{{") {
        return None;
    }
    let mut depth = 0usize;
    let mut cursor = start;
    while cursor + 1 < bytes.len() {
        if bytes[cursor..].starts_with(b"{{") {
            depth += 1;
            cursor += 2;
            continue;
        }
        if bytes[cursor..].starts_with(b"}}") {
            depth = depth.saturating_sub(1);
            cursor += 2;
            if depth == 0 {
                return Some(cursor);
            }
            continue;
        }
        cursor += 1;
    }
    None
}

pub(super) fn find_delimited(bytes: &[u8], start: usize, pattern: &[u8]) -> Option<usize> {
    let mut cursor = start;
    while cursor + pattern.len() <= bytes.len() {
        if bytes[cursor..].starts_with(pattern) {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

pub(super) fn find_tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut quote: Option<u8> = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(quote_char) = quote {
            if byte == quote_char {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

pub(super) fn find_case_insensitive(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    let mut cursor = start;
    while cursor + needle.len() <= haystack.len() {
        if haystack[cursor..].starts_with(needle) {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

pub(super) fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| {
        let key = value.to_ascii_lowercase();
        if seen.contains(&key) {
            return false;
        }
        seen.insert(key);
        true
    });
}
