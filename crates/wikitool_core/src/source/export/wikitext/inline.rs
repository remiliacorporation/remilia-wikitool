use super::super::super::entities::decode_html_entities;
pub(super) fn convert_heading(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('=') || !trimmed.ends_with('=') || trimmed.len() < 4 {
        return None;
    }
    let start_equals = trimmed.chars().take_while(|ch| *ch == '=').count();
    let end_equals = trimmed.chars().rev().take_while(|ch| *ch == '=').count();
    if start_equals < 2 || start_equals != end_equals {
        return None;
    }
    let level = start_equals.min(6);
    let content = trimmed[start_equals..trimmed.len() - end_equals].trim();
    if content.is_empty() {
        return None;
    }
    Some(format!("{} {}", "#".repeat(level), content))
}

pub(super) fn convert_inline_wikitext(line: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0usize;
    while cursor < line.len() {
        let Some(nowiki_start) = index_of_ignore_case(line, "<nowiki", cursor) else {
            output.push_str(&convert_inline_wikitext_segment(&line[cursor..]));
            break;
        };
        output.push_str(&convert_inline_wikitext_segment(
            &line[cursor..nowiki_start],
        ));
        let Some(open_end_offset) = line[nowiki_start..].find('>') else {
            output.push_str(&convert_inline_wikitext_segment(&line[nowiki_start..]));
            break;
        };
        let open_end = nowiki_start + open_end_offset;
        let open_tag = &line[nowiki_start..=open_end];
        if open_tag.trim_end_matches('>').trim_end().ends_with('/') {
            cursor = open_end + 1;
            continue;
        }
        let Some(close_start) = index_of_ignore_case(line, "</nowiki", open_end + 1) else {
            output.push_str(&decode_basic_entities(&line[open_end + 1..]));
            break;
        };
        output.push_str(&decode_basic_entities(&line[open_end + 1..close_start]));
        let close_end = line[close_start..]
            .find('>')
            .map(|offset| close_start + offset + 1)
            .unwrap_or(line.len());
        cursor = close_end;
    }
    output
}

fn convert_inline_wikitext_segment(line: &str) -> String {
    let line = convert_external_links(line);
    let line = convert_internal_links(&line);
    let line = strip_simple_html_tags(&line);
    convert_emphasis(&line)
}

pub(super) fn convert_redirect_line(line: &str) -> Option<String> {
    let rest = strip_prefix_ignore_case(line.trim_start(), "#redirect")?;
    let rest = rest
        .trim_start()
        .strip_prefix(':')
        .unwrap_or(rest)
        .trim_start();
    let inner = extract_first_wrapped_link(rest)?;
    let mut parts = inner.splitn(2, '|');
    let target = parts.next().unwrap_or("").trim();
    let label = parts.next().map(str::trim).unwrap_or(target);
    if target.is_empty() || label.is_empty() {
        return None;
    }
    Some(format!("Redirect to [{label}](wiki://{target})"))
}

pub(super) fn line_starts_redirect(line: &str) -> bool {
    strip_prefix_ignore_case(line.trim_start(), "#redirect").is_some()
}

fn strip_prefix_ignore_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let bytes = value.as_bytes();
    let prefix_bytes = prefix.as_bytes();
    if bytes.len() < prefix_bytes.len() {
        return None;
    }
    for (actual, expected) in bytes.iter().zip(prefix_bytes.iter()) {
        if !actual.eq_ignore_ascii_case(expected) {
            return None;
        }
    }
    Some(&value[prefix.len()..])
}

pub(super) fn convert_list_prefix(line: &str) -> String {
    let trimmed = line.trim_start();
    let indentation = &line[..line.len() - trimmed.len()];
    if trimmed.starts_with('*') {
        let depth = trimmed.chars().take_while(|ch| *ch == '*').count();
        let content = trimmed[depth..].trim_start();
        return format!("{}- {}", "  ".repeat(depth.saturating_sub(1)), content);
    }
    if trimmed.starts_with('#') {
        let depth = trimmed.chars().take_while(|ch| *ch == '#').count();
        let content = trimmed[depth..].trim_start();
        return format!("{}1. {}", "  ".repeat(depth.saturating_sub(1)), content);
    }
    if let Some(stripped) = trimmed.strip_prefix(';') {
        let content = stripped.trim_start();
        if content.is_empty() {
            return String::new();
        }
        if let Some((term, definition)) = split_definition_list_pair(content) {
            return format!("{indentation}- **{}:** {}", term.trim(), definition.trim());
        }
        return format!("{indentation}- **{content}**");
    }
    if trimmed.starts_with(':') {
        let depth = trimmed.chars().take_while(|ch| *ch == ':').count();
        let content = trimmed[depth..].trim_start();
        return format!("{}{}", "  ".repeat(depth), content);
    }
    format!("{indentation}{trimmed}")
}

fn split_definition_list_pair(content: &str) -> Option<(&str, &str)> {
    let chars = content.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    let mut wiki_link_depth = 0usize;
    let mut template_depth = 0usize;
    let mut external_link_depth = 0usize;

    while index < chars.len() {
        if index + 1 < chars.len() && chars[index] == '[' && chars[index + 1] == '[' {
            wiki_link_depth += 1;
            index += 2;
            continue;
        }
        if index + 1 < chars.len()
            && chars[index] == ']'
            && chars[index + 1] == ']'
            && wiki_link_depth > 0
        {
            wiki_link_depth -= 1;
            index += 2;
            continue;
        }
        if index + 1 < chars.len() && chars[index] == '{' && chars[index + 1] == '{' {
            template_depth += 1;
            index += 2;
            continue;
        }
        if index + 1 < chars.len()
            && chars[index] == '}'
            && chars[index + 1] == '}'
            && template_depth > 0
        {
            template_depth -= 1;
            index += 2;
            continue;
        }
        if chars[index] == '[' && wiki_link_depth == 0 {
            external_link_depth += 1;
            index += 1;
            continue;
        }
        if chars[index] == ']' && external_link_depth > 0 {
            external_link_depth -= 1;
            index += 1;
            continue;
        }
        if chars[index] == ':'
            && wiki_link_depth == 0
            && template_depth == 0
            && external_link_depth == 0
            && matches!(index.checked_sub(1).and_then(|prev| chars.get(prev)), Some(ch) if ch.is_whitespace())
            && matches!(chars.get(index + 1), Some(ch) if ch.is_whitespace())
        {
            let split_index = content
                .char_indices()
                .nth(index)
                .map(|(byte_index, _)| byte_index)
                .unwrap_or(content.len());
            return Some((&content[..split_index], &content[split_index + 1..]));
        }
        index += 1;
    }
    None
}

fn convert_emphasis(line: &str) -> String {
    line.replace("'''''", "***")
        .replace("'''", "**")
        .replace("''", "*")
}

fn convert_internal_links(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if index + 1 < chars.len() && chars[index] == '[' && chars[index + 1] == '[' {
            let mut cursor = index + 2;
            let mut found = None::<usize>;
            while cursor + 1 < chars.len() {
                if chars[cursor] == ']' && chars[cursor + 1] == ']' {
                    found = Some(cursor);
                    break;
                }
                cursor += 1;
            }
            if let Some(end) = found {
                let inner = chars[index + 2..end].iter().collect::<String>();
                let mut parts = inner.splitn(2, '|');
                let target = parts.next().unwrap_or("").trim();
                let label = parts.next().map(str::trim).unwrap_or(target);
                if !target.is_empty() && !label.is_empty() {
                    output.push_str(&format!("[{label}](wiki://{target})"));
                    index = end + 2;
                    continue;
                }
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn convert_external_links(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '[' && !matches!(chars.get(index + 1), Some('[')) {
            let mut cursor = index + 1;
            let mut found = None::<usize>;
            while cursor < chars.len() {
                if chars[cursor] == ']' {
                    found = Some(cursor);
                    break;
                }
                cursor += 1;
            }
            if let Some(end) = found {
                let inner = chars[index + 1..end].iter().collect::<String>();
                if let Some((url, label)) = split_external_link(&inner) {
                    output.push_str(&format!("[{label}]({url})"));
                    index = end + 1;
                    continue;
                }
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn split_external_link(value: &str) -> Option<(&str, &str)> {
    let trimmed = value.trim();
    let url_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let url = &trimmed[..url_end];
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    let label = trimmed[url_end..].trim();
    if label.is_empty() {
        return Some((url, url));
    }
    Some((url, label))
}

pub(super) fn extract_category_link(trimmed: &str) -> Option<String> {
    let inner = extract_wrapped_link(trimmed)?;
    let title = inner
        .strip_prefix("Category:")
        .or_else(|| inner.strip_prefix("category:"))?;
    Some(title.split('|').next().unwrap_or(title).trim().to_string())
}

pub(super) fn extract_media_link(trimmed: &str) -> Option<String> {
    let inner = extract_wrapped_link(trimmed)?;
    let target = inner
        .strip_prefix("File:")
        .or_else(|| inner.strip_prefix("Image:"))
        .or_else(|| inner.strip_prefix("file:"))
        .or_else(|| inner.strip_prefix("image:"))?;
    let parts = split_wikitext_pipe_parts(target)
        .into_iter()
        .map(str::trim)
        .collect::<Vec<_>>();
    let filename = parts.first().copied().unwrap_or("").trim();
    if filename.is_empty() {
        return None;
    }
    let caption = parts
        .iter()
        .rev()
        .find(|part| {
            !part.is_empty()
                && !matches!(
                    part.to_ascii_lowercase().as_str(),
                    "thumb"
                        | "thumbnail"
                        | "frame"
                        | "frameless"
                        | "right"
                        | "left"
                        | "center"
                        | "none"
                )
                && !part.ends_with("px")
                && !part.contains('=')
        })
        .copied()
        .unwrap_or(filename);
    Some(format!("{filename} - {}", convert_inline_wikitext(caption)))
}

fn split_wikitext_pipe_parts(value: &str) -> Vec<&str> {
    let chars = value.char_indices().collect::<Vec<_>>();
    let mut parts = Vec::new();
    let mut part_start = 0usize;
    let mut index = 0usize;
    let mut wiki_link_depth = 0usize;
    let mut template_depth = 0usize;

    while index < chars.len() {
        let (byte_index, ch) = chars[index];
        if index + 1 < chars.len() && ch == '[' && chars[index + 1].1 == '[' {
            wiki_link_depth += 1;
            index += 2;
            continue;
        }
        if index + 1 < chars.len() && ch == ']' && chars[index + 1].1 == ']' && wiki_link_depth > 0
        {
            wiki_link_depth -= 1;
            index += 2;
            continue;
        }
        if index + 1 < chars.len() && ch == '{' && chars[index + 1].1 == '{' {
            template_depth += 1;
            index += 2;
            continue;
        }
        if index + 1 < chars.len() && ch == '}' && chars[index + 1].1 == '}' && template_depth > 0 {
            template_depth -= 1;
            index += 2;
            continue;
        }
        if ch == '|' && wiki_link_depth == 0 && template_depth == 0 {
            parts.push(&value[part_start..byte_index]);
            part_start = byte_index + 1;
        }
        index += 1;
    }

    parts.push(&value[part_start..]);
    parts
}

fn extract_wrapped_link(trimmed: &str) -> Option<&str> {
    trimmed.strip_prefix("[[")?.strip_suffix("]]")
}

fn extract_first_wrapped_link(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("[[")?;
    let end = rest.find("]]")?;
    Some(&rest[..end])
}

pub(super) fn parse_ref_name(open_tag: &str) -> Option<String> {
    let mut index = 0usize;
    while let Some(name_offset) = index_of_ignore_case(open_tag, "name", index) {
        let mut cursor = name_offset + "name".len();
        while cursor < open_tag.len() && open_tag.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if open_tag.as_bytes().get(cursor).copied() != Some(b'=') {
            index = cursor;
            continue;
        }
        cursor += 1;
        while cursor < open_tag.len() && open_tag.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let bytes = open_tag.as_bytes();
        if let Some(quote) = bytes
            .get(cursor)
            .copied()
            .filter(|byte| *byte == b'"' || *byte == b'\'')
        {
            cursor += 1;
            let start = cursor;
            while cursor < open_tag.len() && open_tag.as_bytes()[cursor] != quote {
                cursor += 1;
            }
            return sanitize_anchor(&open_tag[start..cursor]);
        }
        let start = cursor;
        while cursor < open_tag.len()
            && !open_tag.as_bytes()[cursor].is_ascii_whitespace()
            && open_tag.as_bytes()[cursor] != b'/'
            && open_tag.as_bytes()[cursor] != b'>'
        {
            cursor += 1;
        }
        return sanitize_anchor(&open_tag[start..cursor]);
    }
    None
}

fn sanitize_anchor(value: &str) -> Option<String> {
    let mut output = String::new();
    let mut last_dash = false;
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            output.push('-');
            last_dash = true;
        }
    }
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}

pub(super) fn strip_html_comments(content: &str) -> String {
    let mut output = String::new();
    let mut index = 0usize;
    while index < content.len() {
        let Some(start) = content[index..].find("<!--") else {
            output.push_str(&content[index..]);
            break;
        };
        let absolute_start = index + start;
        output.push_str(&content[index..absolute_start]);
        let Some(end) = content[absolute_start + 4..].find("-->") else {
            break;
        };
        index = absolute_start + 4 + end + 3;
    }
    output
}

pub(super) fn strip_simple_html_tags(line: &str) -> String {
    let mut output = String::new();
    let mut index = 0usize;
    while index < line.len() {
        let Some(start) = line[index..].find('<') else {
            output.push_str(&line[index..]);
            break;
        };
        let absolute_start = index + start;
        output.push_str(&line[index..absolute_start]);
        let Some(end) = line[absolute_start..].find('>') else {
            output.push_str(&line[absolute_start..]);
            break;
        };
        let tag = &line[absolute_start + 1..absolute_start + end];
        let normalized = tag.trim().trim_start_matches('/').to_ascii_lowercase();
        if matches!(normalized.as_str(), "br" | "br/" | "br /") {
            output.push('\n');
        }
        index = absolute_start + end + 1;
    }
    decode_basic_entities(&output)
}

fn decode_basic_entities(text: &str) -> String {
    decode_html_entities(text)
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
