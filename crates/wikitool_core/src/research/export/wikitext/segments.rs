use super::super::super::template_render::TemplateContext;
use super::inline::index_of_ignore_case;
pub(super) enum Segment<'a> {
    Prose(&'a str),
    Template { inner: &'a str },
    ExtensionBlock { raw: &'a str },
}

pub(super) fn split_prose_lines_preserving_opaque_blocks(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut cursor = 0usize;
    let mut line_start = 0usize;

    while cursor < bytes.len() {
        if bytes[cursor] == b'<'
            && let Some(end) = skip_inline_opaque_html_block(text, cursor)
        {
            cursor = end;
            continue;
        }
        if bytes[cursor] == b'\n' {
            lines.push(&text[line_start..cursor]);
            cursor += 1;
            line_start = cursor;
            continue;
        }
        cursor += 1;
    }
    lines.push(&text[line_start..]);
    lines
}

pub(super) fn segment_wikitext(content: &str) -> Vec<Segment<'_>> {
    let bytes = content.as_bytes();
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    let mut prose_start = 0usize;

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'<'
            && let Some(end) = skip_complex_extension_block(content, cursor)
        {
            if cursor > prose_start {
                segments.push(Segment::Prose(&content[prose_start..cursor]));
            }
            segments.push(Segment::ExtensionBlock {
                raw: &content[cursor..end],
            });
            cursor = end;
            prose_start = cursor;
            continue;
        }
        if bytes[cursor] == b'<'
            && let Some(end) = skip_inline_opaque_html_block(content, cursor)
        {
            cursor = end;
            continue;
        }
        if bytes[cursor] == b'['
            && cursor + 1 < bytes.len()
            && bytes[cursor + 1] == b'['
            && let Some(end) = skip_wikilink_span(content, cursor)
        {
            cursor = end;
            continue;
        }
        if bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            if cursor > prose_start {
                segments.push(Segment::Prose(&content[prose_start..cursor]));
            }
            let inner_start = cursor + 2;
            let mut depth = 1usize;
            let mut scan = inner_start;
            while scan + 1 < bytes.len() && depth > 0 {
                if bytes[scan] == b'{' && bytes[scan + 1] == b'{' {
                    depth += 1;
                    scan += 2;
                    continue;
                }
                if bytes[scan] == b'}' && bytes[scan + 1] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    scan += 2;
                    continue;
                }
                scan += 1;
            }
            if depth == 0 && scan + 1 < bytes.len() {
                segments.push(Segment::Template {
                    inner: &content[inner_start..scan],
                });
                cursor = scan + 2;
                prose_start = cursor;
                continue;
            }
            segments.push(Segment::Prose(&content[cursor..]));
            return segments;
        }
        cursor += 1;
    }
    if prose_start < bytes.len() {
        segments.push(Segment::Prose(&content[prose_start..]));
    }
    segments
}

fn skip_wikilink_span(content: &str, cursor: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    if bytes.get(cursor).copied() != Some(b'[') || bytes.get(cursor + 1).copied() != Some(b'[') {
        return None;
    }
    let mut scan = cursor + 2;
    while scan + 1 < bytes.len() {
        if bytes[scan] == b']' && bytes[scan + 1] == b']' {
            return Some(scan + 2);
        }
        scan += 1;
    }
    None
}

fn skip_inline_opaque_html_block(content: &str, cursor: usize) -> Option<usize> {
    const OPAQUE_TAGS: &[&str] = &["ref", "nowiki"];
    for tag in OPAQUE_TAGS {
        if let Some(end) = skip_named_opaque_html_block(content, cursor, tag) {
            return Some(end);
        }
    }
    None
}

fn skip_complex_extension_block(content: &str, cursor: usize) -> Option<usize> {
    const EXTENSION_TAGS: &[&str] = &[
        "gallery",
        "math",
        "chem",
        "syntaxhighlight",
        "source",
        "score",
        "timeline",
        "graph",
        "mapframe",
        "maplink",
    ];
    for tag in EXTENSION_TAGS {
        if let Some(end) = skip_named_opaque_html_block(content, cursor, tag) {
            return Some(end);
        }
    }
    None
}

fn skip_named_opaque_html_block(content: &str, cursor: usize, tag: &str) -> Option<usize> {
    let bytes = content.as_bytes();
    if bytes.get(cursor).copied() != Some(b'<') {
        return None;
    }
    let tag_bytes = tag.as_bytes();
    let name_end = cursor + 1 + tag_bytes.len();
    if name_end > bytes.len() {
        return None;
    }
    if !bytes[cursor + 1..name_end].eq_ignore_ascii_case(tag_bytes) {
        return None;
    }
    let boundary = bytes.get(name_end).copied();
    if !matches!(
        boundary,
        Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
    ) {
        return None;
    }
    let open_close = cursor + content[cursor..].find('>')?;
    let open_tag = &content[cursor..=open_close];
    if open_tag.trim_end_matches('>').trim_end().ends_with('/') {
        return Some(open_close + 1);
    }
    let close_needle = format!("</{tag}");
    let search_from = open_close + 1;
    let close_offset = index_of_ignore_case(content, &close_needle, search_from)?;
    let close_end = content[close_offset..].find('>')?;
    Some(close_offset + close_end + 1)
}

pub(super) fn classify_template_context(segments: &[Segment<'_>], index: usize) -> TemplateContext {
    let prev_ok = if index == 0 {
        true
    } else if let Segment::Prose(text) = segments[index - 1] {
        line_tail_is_blank(text)
    } else {
        true
    };
    let next_ok = if index + 1 >= segments.len() {
        true
    } else if let Segment::Prose(text) = segments[index + 1] {
        line_head_is_blank_or_newline(text)
    } else {
        true
    };
    if prev_ok && next_ok {
        TemplateContext::Block
    } else {
        TemplateContext::Inline
    }
}

fn line_tail_is_blank(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let after_newline = text.rfind('\n').map(|position| position + 1).unwrap_or(0);
    text[after_newline..].chars().all(char::is_whitespace)
}

fn line_head_is_blank_or_newline(text: &str) -> bool {
    let head_end = text.find('\n').unwrap_or(text.len());
    text[..head_end].chars().all(char::is_whitespace)
}
