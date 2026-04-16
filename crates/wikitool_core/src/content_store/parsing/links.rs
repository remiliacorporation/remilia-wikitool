use super::*;

pub(crate) fn extract_wikilinks(content: &str) -> Vec<ParsedLink> {
    extract_wikilinks_scoped(content, &[], false)
}

pub(crate) fn extract_wikilinks_for_namespace(content: &str, namespace: &str) -> Vec<ParsedLink> {
    if namespace.eq_ignore_ascii_case(Namespace::Module.as_str()) {
        return Vec::new();
    }

    let ignored_tags = if namespace.eq_ignore_ascii_case(Namespace::Template.as_str()) {
        &[
            "nowiki",
            "pre",
            "syntaxhighlight",
            "source",
            "code",
            "templatedata",
            "noinclude",
        ][..]
    } else {
        &[
            "nowiki",
            "pre",
            "syntaxhighlight",
            "source",
            "code",
            "templatedata",
        ][..]
    };

    extract_wikilinks_scoped(content, ignored_tags, true)
}

fn extract_wikilinks_scoped(
    content: &str,
    ignored_tags: &[&str],
    skip_html_comments: bool,
) -> Vec<ParsedLink> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor + 1 < bytes.len() {
        if let Some(next_cursor) =
            skipped_wikilink_region_end(bytes, cursor, ignored_tags, skip_html_comments)
        {
            cursor = next_cursor;
            continue;
        }
        if bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
            let start = cursor + 2;
            let mut end = start;
            while end + 1 < bytes.len() {
                if bytes[end] == b']' && bytes[end + 1] == b']' {
                    break;
                }
                end += 1;
            }
            if end + 1 >= bytes.len() {
                break;
            }

            let inner = &content[start..end];
            if let Some(link) = parse_wikilink(inner) {
                out.push(link);
            }
            cursor = end + 2;
            continue;
        }
        cursor += 1;
    }

    out
}

fn skipped_wikilink_region_end(
    bytes: &[u8],
    cursor: usize,
    ignored_tags: &[&str],
    skip_html_comments: bool,
) -> Option<usize> {
    if skip_html_comments && starts_with_bytes(bytes, cursor, b"<!--") {
        return Some(
            find_bytes(bytes, cursor + 4, b"-->")
                .map(|end| end + 3)
                .unwrap_or(bytes.len()),
        );
    }

    if bytes.get(cursor).copied() != Some(b'<') {
        return None;
    }

    for tag in ignored_tags {
        if !starts_with_opening_html_tag(bytes, cursor, tag.as_bytes()) {
            continue;
        }
        let close = format!("</{tag}>");
        return Some(
            find_ascii_case_insensitive_bytes(bytes, cursor + 1, close.as_bytes())
                .map(|end| end + close.len())
                .unwrap_or(bytes.len()),
        );
    }

    None
}

fn starts_with_opening_html_tag(bytes: &[u8], cursor: usize, tag: &[u8]) -> bool {
    if bytes.get(cursor).copied() != Some(b'<') {
        return false;
    }
    let name_start = cursor + 1;
    let name_end = name_start + tag.len();
    if name_end > bytes.len() {
        return false;
    }
    if !bytes_ascii_case_insensitive_eq(&bytes[name_start..name_end], tag) {
        return false;
    }
    matches!(
        bytes.get(name_end).copied(),
        Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\n') | Some(b'\r') | Some(b'\t')
    )
}

fn starts_with_bytes(bytes: &[u8], cursor: usize, needle: &[u8]) -> bool {
    cursor
        .checked_add(needle.len())
        .map(|end| end <= bytes.len() && &bytes[cursor..end] == needle)
        .unwrap_or(false)
}

fn find_bytes(bytes: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || start >= bytes.len() {
        return None;
    }
    let last_start = bytes.len().checked_sub(needle.len())?;
    let mut cursor = start;
    while cursor <= last_start {
        if &bytes[cursor..cursor + needle.len()] == needle {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn find_ascii_case_insensitive_bytes(bytes: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || start >= bytes.len() {
        return None;
    }
    let last_start = bytes.len().checked_sub(needle.len())?;
    let mut cursor = start;
    while cursor <= last_start {
        if bytes_ascii_case_insensitive_eq(&bytes[cursor..cursor + needle.len()], needle) {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn bytes_ascii_case_insensitive_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| ascii_lower(*left) == ascii_lower(*right))
}

fn ascii_lower(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + 32
    } else {
        byte
    }
}

pub(crate) fn parse_wikilink(inner: &str) -> Option<ParsedLink> {
    let target_part = inner.split('|').next().unwrap_or("").trim();
    if target_part.is_empty() {
        return None;
    }

    let mut target = target_part;
    let mut leading_colon = false;
    while let Some(stripped) = target.strip_prefix(':') {
        leading_colon = true;
        target = stripped.trim_start();
    }
    if target.is_empty() {
        return None;
    }

    if let Some((without_fragment, _)) = target.split_once('#') {
        target = without_fragment.trim_end();
    }
    if target.is_empty() {
        return None;
    }

    if target.starts_with("http://") || target.starts_with("https://") || target.starts_with("//") {
        return None;
    }

    let target = normalize_spaces(&target.replace('_', " "));
    if target.is_empty() {
        return None;
    }
    if is_parser_placeholder_title(&target) {
        return None;
    }

    let (title, namespace) = normalize_title_and_namespace(&target)?;
    let is_category_membership = namespace == Namespace::Category.as_str() && !leading_colon;

    Some(ParsedLink {
        target_title: title,
        target_namespace: namespace.to_string(),
        is_category_membership,
    })
}

pub(crate) fn is_parser_placeholder_title(value: &str) -> bool {
    let trimmed = value.trim();
    let title = trimmed
        .split_once(':')
        .map(|(_, body)| body.trim())
        .unwrap_or(trimmed);
    if title.len() < 3 || !title.starts_with('%') || !title.ends_with('%') {
        return false;
    }

    let inner = &title[1..title.len() - 1];
    !inner.is_empty()
        && inner
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

pub(crate) fn normalize_title_and_namespace(value: &str) -> Option<(String, &'static str)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((prefix, rest)) = trimmed.split_once(':')
        && let Some(namespace) = canonical_namespace(prefix)
    {
        let body = normalize_spaces(rest);
        if body.is_empty() {
            return None;
        }
        return Some((format!("{namespace}:{body}"), namespace));
    }

    Some((trimmed.to_string(), Namespace::Main.as_str()))
}

pub(crate) fn canonical_namespace(prefix: &str) -> Option<&'static str> {
    let trimmed = prefix.trim();
    if trimmed.eq_ignore_ascii_case("Category") {
        return Some(Namespace::Category.as_str());
    }
    if trimmed.eq_ignore_ascii_case("File") {
        return Some(Namespace::File.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Image") {
        return Some(Namespace::File.as_str());
    }
    if trimmed.eq_ignore_ascii_case("User") {
        return Some(Namespace::User.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Template") {
        return Some(Namespace::Template.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Module") {
        return Some(Namespace::Module.as_str());
    }
    if trimmed.eq_ignore_ascii_case("MediaWiki") {
        return Some(Namespace::MediaWiki.as_str());
    }
    None
}
