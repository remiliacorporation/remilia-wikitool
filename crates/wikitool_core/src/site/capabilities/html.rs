use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TableCell {
    pub(super) text: String,
    pub(super) href: Option<String>,
}

pub(super) fn extract_table_rows_by_id(html: &str, table_id: &str) -> Vec<Vec<TableCell>> {
    let Some(table_html) = extract_element_inner_html_by_id(html, "table", table_id) else {
        return Vec::new();
    };

    extract_tag_blocks(table_html, "tr")
        .into_iter()
        .filter_map(|row| {
            let cells = extract_table_cells(row);
            if cells.is_empty() { None } else { Some(cells) }
        })
        .collect()
}

pub(super) fn extract_table_cells(row_html: &str) -> Vec<TableCell> {
    extract_tag_blocks(row_html, "td")
        .into_iter()
        .filter_map(|cell| {
            let content = inner_html_from_block(cell, "td")?;
            let text = html_text(content);
            if text.is_empty() {
                return None;
            }
            Some(TableCell {
                text,
                href: extract_first_href(content),
            })
        })
        .collect()
}

pub(super) fn extract_table_blocks_with_class<'a>(html: &'a str, class_name: &str) -> Vec<&'a str> {
    extract_tag_blocks(html, "table")
        .into_iter()
        .filter(|table| tag_block_has_class(table, "table", class_name))
        .collect()
}

pub(super) fn extract_caption_text(table_html: &str) -> Option<String> {
    extract_tag_blocks(table_html, "caption")
        .into_iter()
        .next()
        .and_then(|caption| inner_html_from_block(caption, "caption"))
        .map(html_text)
        .and_then(|value| clean_label(&value))
}

pub(super) fn extract_code_values(html: &str) -> Vec<String> {
    extract_tag_blocks(html, "code")
        .into_iter()
        .filter_map(|code| inner_html_from_block(code, "code"))
        .map(html_text)
        .filter(|value| !value.is_empty())
        .collect()
}

pub(super) fn extract_first_href(html: &str) -> Option<String> {
    for tag in scan_tags(html, "a") {
        if let Some(href) = tag.attrs.get("href") {
            let decoded = decode_html(href).trim().to_string();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    None
}

pub(super) fn extract_first_tag_text_with_class(html: &str, class_name: &str) -> Option<String> {
    let mut index = 0usize;
    while index < html.len() {
        let Some(lt) = html[index..].find('<') else {
            break;
        };
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", at + 4) {
                index = end + 3;
            } else {
                break;
            }
            continue;
        }
        let Some(open_end) = find_tag_end(html, at) else {
            break;
        };
        let raw = &html[at..=open_end];
        let Some((tag_name, is_closing, is_self_closing)) = parse_tag_descriptor(raw) else {
            index = open_end + 1;
            continue;
        };
        if is_closing {
            index = open_end + 1;
            continue;
        }
        let attrs = parse_attributes(raw, tag_name);
        if class_contains(attrs.get("class"), class_name) {
            if is_self_closing {
                return None;
            }
            let close = find_matching_close_tag(html, tag_name, open_end + 1)?;
            return clean_label(&html_text(&html[open_end + 1..close]));
        }
        index = open_end + 1;
    }
    None
}

pub(super) fn extract_element_inner_html_by_id<'a>(
    html: &'a str,
    tag_name: &str,
    element_id: &str,
) -> Option<&'a str> {
    let (start, open_end) = find_tag_by_id(html, tag_name, element_id, 0)?;
    let close = find_matching_close_tag(html, tag_name, open_end + 1)?;
    let _ = start;
    Some(&html[open_end + 1..close])
}

pub(super) fn extract_section_between_ids<'a>(
    html: &'a str,
    start_id: &str,
    end_id: &str,
) -> Option<&'a str> {
    let (_, open_end) = find_tag_by_id(html, "h2", start_id, 0)?;
    let end = find_tag_by_id(html, "h2", end_id, open_end + 1)
        .map(|(start, _)| start)
        .unwrap_or(html.len());
    Some(&html[open_end + 1..end])
}

pub(super) fn find_tag_by_id(
    html: &str,
    tag_name: &str,
    element_id: &str,
    start: usize,
) -> Option<(usize, usize)> {
    let mut index = start;
    while let Some(at) = find_tag_start(html, tag_name, index) {
        let open_end = find_tag_end(html, at)?;
        let attrs = parse_attributes(&html[at..=open_end], tag_name);
        if attrs
            .get("id")
            .is_some_and(|value| value.eq_ignore_ascii_case(element_id))
        {
            return Some((at, open_end));
        }
        index = open_end + 1;
    }
    None
}

pub(super) fn extract_tag_blocks<'a>(html: &'a str, tag_name: &str) -> Vec<&'a str> {
    let mut output = Vec::new();
    let mut index = 0usize;

    while let Some(at) = find_tag_start(html, tag_name, index) {
        let Some(open_end) = find_tag_end(html, at) else {
            break;
        };
        let Some(close_start) = find_matching_close_tag(html, tag_name, open_end + 1) else {
            index = open_end + 1;
            continue;
        };
        let Some(close_end) = find_tag_end(html, close_start) else {
            break;
        };
        output.push(&html[at..=close_end]);
        index = close_end + 1;
    }

    output
}

pub(super) fn inner_html_from_block<'a>(html: &'a str, tag_name: &str) -> Option<&'a str> {
    let open_end = find_tag_end(html, 0)?;
    let close_start = find_matching_close_tag(html, tag_name, open_end + 1)?;
    Some(&html[open_end + 1..close_start])
}

pub(super) fn tag_block_has_class(html: &str, tag_name: &str, class_name: &str) -> bool {
    let Some(open_end) = find_tag_end(html, 0) else {
        return false;
    };
    let attrs = parse_attributes(&html[..=open_end], tag_name);
    class_contains(attrs.get("class"), class_name)
}

pub(super) fn class_contains(value: Option<&String>, needle: &str) -> bool {
    value.is_some_and(|value| value.split_whitespace().any(|part| part == needle))
}

pub(super) fn resolve_href(wiki_url: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(href) {
        return Some(url.to_string());
    }
    let base = Url::parse(wiki_url).ok()?;
    base.join(href.trim_start_matches('/'))
        .ok()
        .map(|url| url.to_string())
}

pub(super) fn clean_version_label(value: &str) -> Option<String> {
    let value = clean_label(value)?;
    if matches!(value.as_str(), "-" | "–" | "—") {
        None
    } else {
        Some(value)
    }
}

pub(super) fn parse_mediawiki_version(generator: &str) -> Option<String> {
    let trimmed = generator.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(version) = trimmed.strip_prefix("MediaWiki ") {
        return clean_label(version);
    }
    clean_label(trimmed)
}

pub(super) fn normalize_extension_name(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("Extension:")
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for value in values {
        let Some(normalized) = clean_label(&value) else {
            continue;
        };
        if seen.insert(normalized.to_ascii_lowercase()) {
            out.push(normalized);
        }
    }
    out.sort_unstable();
    out
}

pub(super) fn normalize_preserved_string_list(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for value in values {
        let normalized = collapse_whitespace(&value);
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.to_ascii_lowercase()) {
            out.push(normalized);
        }
    }
    out.sort_unstable();
    out
}

pub(super) fn fallback_article_path(value: &str) -> String {
    clean_label(value).unwrap_or_else(|| DEFAULT_ARTICLE_PATH.to_string())
}

pub(super) fn clean_label(value: &str) -> Option<String> {
    let normalized = value
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[derive(Debug, Clone)]
pub(super) struct TagMatch {
    pub(super) attrs: BTreeMap<String, String>,
}

pub(super) fn extract_head(html: &str) -> String {
    let Some(head_start) = find_tag_start(html, "head", 0) else {
        return html.to_string();
    };
    let Some(open_end) = find_tag_end(html, head_start) else {
        return html.to_string();
    };
    let Some(close_index) = index_of_ignore_case(html, "</head>", open_end + 1) else {
        return html[open_end + 1..].to_string();
    };
    html[open_end + 1..close_index].to_string()
}

pub(super) fn scan_tags(html: &str, tag_name: &str) -> Vec<TagMatch> {
    let mut output = Vec::new();
    let mut index = 0usize;

    while index < html.len() {
        let Some(lt) = html[index..].find('<') else {
            break;
        };
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", at + 4) {
                index = end + 3;
            } else {
                index = html.len();
            }
            continue;
        }
        if is_tag_at(html, at, tag_name) {
            let Some(end) = find_tag_end(html, at) else {
                break;
            };
            output.push(TagMatch {
                attrs: parse_attributes(&html[at..=end], tag_name),
            });
            index = end + 1;
            continue;
        }
        index = at + 1;
    }

    output
}

pub(super) fn html_text(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut index = 0usize;
    while index < html.len() {
        if starts_with_at(html, index, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", index + 4) {
                index = end + 3;
            } else {
                break;
            }
            continue;
        }
        let Some(ch) = html[index..].chars().next() else {
            break;
        };
        if ch == '<' {
            let Some(end) = find_tag_end(html, index) else {
                break;
            };
            if let Some((tag_name, _, _)) = parse_tag_descriptor(&html[index..=end])
                && is_block_like_tag(tag_name)
                && !output.ends_with(' ')
            {
                output.push(' ');
            }
            index = end + 1;
            continue;
        }
        output.push(ch);
        index += ch.len_utf8();
    }
    collapse_whitespace(&decode_html(&output))
}

pub(super) fn is_block_like_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "div"
            | "p"
            | "li"
            | "tr"
            | "td"
            | "th"
            | "table"
            | "caption"
            | "code"
            | "br"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

pub(super) fn find_matching_close_tag(html: &str, tag_name: &str, start: usize) -> Option<usize> {
    let mut index = start;
    let mut depth = 1usize;

    while index < html.len() {
        let lt = html[index..].find('<')?;
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            {
                let end = index_of_ignore_case(html, "-->", at + 4)?;
                index = end + 3;
            }
            continue;
        }
        if is_close_tag_at(html, at, tag_name) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(at);
            }
            index = find_tag_end(html, at)? + 1;
            continue;
        }
        if is_tag_at(html, at, tag_name) {
            let end = find_tag_end(html, at)?;
            if !is_self_closing_tag(&html[at..=end], tag_name) {
                depth += 1;
            }
            index = end + 1;
            continue;
        }
        index = at + 1;
    }

    None
}

pub(super) fn parse_tag_descriptor(tag_raw: &str) -> Option<(&str, bool, bool)> {
    let bytes = tag_raw.as_bytes();
    if bytes.first().copied() != Some(b'<') {
        return None;
    }

    let mut index = 1usize;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let is_closing = bytes.get(index).copied() == Some(b'/');
    if is_closing {
        index += 1;
    }
    let name_start = index;
    while index < bytes.len() {
        let ch = bytes[index];
        if ch.is_ascii_whitespace() || ch == b'>' || ch == b'/' {
            break;
        }
        index += 1;
    }
    if name_start == index {
        return None;
    }
    let tag_name = &tag_raw[name_start..index];
    Some((tag_name, is_closing, is_self_closing_tag(tag_raw, tag_name)))
}

pub(super) fn find_tag_start(html: &str, tag_name: &str, start: usize) -> Option<usize> {
    let mut index = start;
    while index < html.len() {
        let lt = html[index..].find('<')?;
        let at = index + lt;
        if is_tag_at(html, at, tag_name) {
            return Some(at);
        }
        index = at + 1;
    }
    None
}

pub(super) fn is_tag_at(html: &str, at: usize, tag_name: &str) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(at).copied() != Some(b'<') {
        return false;
    }
    let mut index = at + 1;
    if index >= bytes.len() || bytes[index] == b'/' {
        return false;
    }
    for expected in tag_name.as_bytes() {
        let Some(actual) = bytes.get(index) else {
            return false;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return false;
        }
        index += 1;
    }
    matches!(
        bytes.get(index).copied(),
        Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/')
    )
}

pub(super) fn is_close_tag_at(html: &str, at: usize, tag_name: &str) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(at).copied() != Some(b'<') || bytes.get(at + 1).copied() != Some(b'/') {
        return false;
    }
    let mut index = at + 2;
    for expected in tag_name.as_bytes() {
        let Some(actual) = bytes.get(index) else {
            return false;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return false;
        }
        index += 1;
    }
    matches!(
        bytes.get(index).copied(),
        Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>')
    )
}

pub(super) fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut index = start;
    let mut quote = None::<u8>;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if byte == b'>' {
            return Some(index);
        }
        index += 1;
    }
    None
}

pub(super) fn parse_attributes(tag_raw: &str, tag_name: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = tag_raw.as_bytes();
    let mut index = tag_name.len() + 1;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'>' {
            break;
        }
        if byte == b'/' || byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        let name_start = index;
        while index < bytes.len() {
            let ch = bytes[index];
            if ch.is_ascii_whitespace() || ch == b'=' || ch == b'>' || ch == b'/' {
                break;
            }
            index += 1;
        }
        if name_start == index {
            index += 1;
            continue;
        }
        let name = tag_raw[name_start..index].trim().to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let mut value = String::new();
        if bytes.get(index).copied() == Some(b'=') {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if let Some(quote) = bytes
                .get(index)
                .copied()
                .filter(|byte| *byte == b'"' || *byte == b'\'')
            {
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                value = tag_raw[value_start..index].to_string();
                if bytes.get(index).copied() == Some(quote) {
                    index += 1;
                }
            } else {
                let value_start = index;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                {
                    index += 1;
                }
                value = tag_raw[value_start..index].to_string();
            }
        }

        if !value.is_empty() {
            attrs.insert(name, value);
        } else {
            attrs.entry(name).or_default();
        }
    }

    attrs
}

pub(super) fn index_of_ignore_case(text: &str, search: &str, start: usize) -> Option<usize> {
    if search.is_empty() {
        return Some(start);
    }
    let text_bytes = text.as_bytes();
    let search_bytes = search.as_bytes();
    if search_bytes.len() > text_bytes.len() || start >= text_bytes.len() {
        return None;
    }

    let last_start = text_bytes.len().saturating_sub(search_bytes.len());
    for index in start..=last_start {
        let mut matched = true;
        for offset in 0..search_bytes.len() {
            if !text_bytes[index + offset].eq_ignore_ascii_case(&search_bytes[offset]) {
                matched = false;
                break;
            }
        }
        if matched {
            return Some(index);
        }
    }
    None
}

pub(super) fn starts_with_at(text: &str, index: usize, sequence: &str) -> bool {
    let bytes = text.as_bytes();
    let sequence = sequence.as_bytes();
    bytes
        .get(index..index.saturating_add(sequence.len()))
        .is_some_and(|window| window == sequence)
}

pub(super) fn decode_html(text: &str) -> String {
    let mut value = text.to_string();
    value = value.replace("&amp;", "&");
    value = value.replace("&quot;", "\"");
    value = value.replace("&#39;", "'");
    value = value.replace("&lt;", "<");
    value = value.replace("&gt;", ">");
    value = value.replace("&nbsp;", " ");
    value = value.replace("&ndash;", "–");
    value = value.replace("&mdash;", "—");
    value
}

pub(super) fn collapse_whitespace(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
                previous_was_space = true;
            }
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }
    output.trim().to_string()
}

pub(super) fn is_self_closing_tag(tag_raw: &str, tag_name: &str) -> bool {
    let normalized = tag_name.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "br" | "hr" | "img" | "meta" | "link" | "input" | "source"
    ) {
        return true;
    }
    tag_raw.trim_end().ends_with("/>")
}
