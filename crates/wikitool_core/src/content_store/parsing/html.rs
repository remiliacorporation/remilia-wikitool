use super::*;

pub(crate) fn starts_with_html_tag(bytes: &[u8], cursor: usize, tag_name: &str) -> bool {
    let tag_bytes = tag_name.as_bytes();
    if cursor + tag_bytes.len() + 1 >= bytes.len() || bytes[cursor] != b'<' {
        return false;
    }
    let start = cursor + 1;
    let end = start + tag_bytes.len();
    if end > bytes.len() || !bytes[start..end].eq_ignore_ascii_case(tag_bytes) {
        return false;
    }
    matches!(
        bytes.get(end).copied(),
        Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
    )
}

pub(crate) fn parse_open_tag(
    content: &str,
    start: usize,
    tag_name: &str,
) -> Option<(usize, String, bool)> {
    let bytes = content.as_bytes();
    if !starts_with_html_tag(bytes, start, tag_name) {
        return None;
    }

    let mut cursor = start + tag_name.len() + 1;
    let mut quote = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            cursor += 1;
            continue;
        }
        if byte == b'>' {
            let raw_body = &content[start + tag_name.len() + 1..cursor];
            let trimmed = raw_body.trim();
            let self_closing = trimmed.ends_with('/');
            let body = if self_closing {
                trimmed.trim_end_matches('/').trim_end().to_string()
            } else {
                trimmed.to_string()
            };
            return Some((cursor + 1, body, self_closing));
        }
        cursor += 1;
    }
    None
}

pub(crate) fn find_closing_html_tag(
    content: &str,
    start: usize,
    tag_name: &str,
) -> Option<(usize, usize)> {
    let bytes = content.as_bytes();
    let needle = format!("</{tag_name}");
    let needle_bytes = needle.as_bytes();
    let mut cursor = start;

    while cursor + needle_bytes.len() < bytes.len() {
        if bytes[cursor] == b'<'
            && bytes[cursor..cursor + needle_bytes.len()].eq_ignore_ascii_case(needle_bytes)
        {
            let boundary = bytes.get(cursor + needle_bytes.len()).copied();
            if !matches!(
                boundary,
                Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
            ) {
                cursor += 1;
                continue;
            }
            let mut end = cursor + needle_bytes.len();
            while end < bytes.len() && bytes[end] != b'>' {
                end += 1;
            }
            if end < bytes.len() {
                return Some((cursor, end + 1));
            }
        }
        cursor += 1;
    }
    None
}

pub(crate) fn parse_html_attributes(value: &str) -> BTreeMap<String, String> {
    let chars = value.chars().collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut out = BTreeMap::new();

    while cursor < chars.len() {
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor >= chars.len() {
            break;
        }

        let key_start = cursor;
        while cursor < chars.len()
            && !chars[cursor].is_whitespace()
            && chars[cursor] != '='
            && chars[cursor] != '/'
        {
            cursor += 1;
        }
        let key = chars[key_start..cursor]
            .iter()
            .collect::<String>()
            .trim()
            .to_ascii_lowercase();
        if key.is_empty() {
            cursor += 1;
            continue;
        }

        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        let mut value_out = String::new();
        if cursor < chars.len() && chars[cursor] == '=' {
            cursor += 1;
            while cursor < chars.len() && chars[cursor].is_whitespace() {
                cursor += 1;
            }
            if cursor < chars.len() && (chars[cursor] == '"' || chars[cursor] == '\'') {
                let quote = chars[cursor];
                cursor += 1;
                let start = cursor;
                while cursor < chars.len() && chars[cursor] != quote {
                    cursor += 1;
                }
                value_out = chars[start..cursor].iter().collect::<String>();
                if cursor < chars.len() {
                    cursor += 1;
                }
            } else {
                let start = cursor;
                while cursor < chars.len() && !chars[cursor].is_whitespace() && chars[cursor] != '/'
                {
                    cursor += 1;
                }
                value_out = chars[start..cursor].iter().collect::<String>();
            }
        }

        out.insert(key, normalize_spaces(&value_out));
    }

    out
}
