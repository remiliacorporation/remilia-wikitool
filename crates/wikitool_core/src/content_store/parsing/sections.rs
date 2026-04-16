use super::*;

pub(crate) fn chunk_article_context(content: &str) -> Vec<ArticleContextChunkRow> {
    let sections = parse_content_sections(content);
    chunk_article_context_from_sections(&sections)
}

pub(crate) fn extract_section_records(content: &str) -> Vec<IndexedSectionRecord> {
    let sections = parse_content_sections(content);
    extract_section_records_from_sections(&sections)
}

pub(crate) fn extract_section_records_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<IndexedSectionRecord> {
    sections
        .iter()
        .map(|section| IndexedSectionRecord {
            section_heading: section.section_heading.clone(),
            section_level: section.section_level,
            summary_text: summarize_words(
                &normalize_multiline_spaces(&section.section_text),
                AUTHORING_PAGE_SUMMARY_WORD_LIMIT,
            ),
            token_estimate: estimate_tokens(&section.section_text),
            section_text: normalize_multiline_spaces(&section.section_text),
        })
        .collect()
}

pub(crate) fn chunk_article_context_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<ArticleContextChunkRow> {
    let mut out = Vec::new();
    for section in sections {
        for chunk_text in chunk_section_text(&section.section_text) {
            out.push(ArticleContextChunkRow {
                section_heading: section.section_heading.clone(),
                token_estimate: estimate_tokens(&chunk_text),
                chunk_text,
            });
        }
    }
    out
}

pub(crate) fn parse_content_sections(content: &str) -> Vec<ParsedContentSection> {
    let mut out = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_level = 1u8;
    let mut current_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((level, heading)) = parse_heading_line(trimmed) {
            flush_content_section(
                &mut out,
                current_heading.take(),
                current_level,
                &current_lines,
            );
            current_lines.clear();
            current_heading = Some(heading);
            current_level = level;
            continue;
        }
        current_lines.push(line);
    }
    flush_content_section(&mut out, current_heading, current_level, &current_lines);
    out
}

pub(crate) fn flush_content_section(
    out: &mut Vec<ParsedContentSection>,
    section_heading: Option<String>,
    section_level: u8,
    lines: &[&str],
) {
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() {
        return;
    }
    out.push(ParsedContentSection {
        section_heading,
        section_level,
        section_text: text,
    });
}

pub(crate) fn chunk_section_text(section_text: &str) -> Vec<String> {
    let paragraphs = section_text
        .split("\n\n")
        .map(normalize_multiline_spaces)
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>();
    if paragraphs.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current_parts = Vec::<String>::new();
    let mut current_words = 0usize;
    for paragraph in paragraphs {
        let paragraph_words = count_words(&paragraph);
        if paragraph_words > INDEX_CHUNK_WORD_TARGET {
            if !current_parts.is_empty() {
                out.push(current_parts.join(" "));
                current_parts.clear();
                current_words = 0;
            }
            out.extend(split_text_by_words(&paragraph, INDEX_CHUNK_WORD_TARGET));
            continue;
        }
        if !current_parts.is_empty()
            && current_words.saturating_add(paragraph_words) > INDEX_CHUNK_WORD_TARGET
        {
            out.push(current_parts.join(" "));
            current_parts.clear();
            current_words = 0;
        }
        current_words = current_words.saturating_add(paragraph_words);
        current_parts.push(paragraph);
    }
    if !current_parts.is_empty() {
        out.push(current_parts.join(" "));
    }
    out
}

pub(crate) fn split_text_by_words(text: &str, word_target: usize) -> Vec<String> {
    let words = text.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < words.len() {
        let end = (cursor + word_target.max(1)).min(words.len());
        let chunk_text = words[cursor..end].join(" ");
        if !chunk_text.is_empty() {
            out.push(chunk_text);
        }
        cursor = end;
    }
    out
}

pub(crate) fn parse_heading_line(value: &str) -> Option<(u8, String)> {
    if value.len() < 4 || !value.starts_with('=') || !value.ends_with('=') {
        return None;
    }
    let leading = value.chars().take_while(|ch| *ch == '=').count();
    let trailing = value.chars().rev().take_while(|ch| *ch == '=').count();
    if leading != trailing || !(2..=6).contains(&leading) {
        return None;
    }
    if leading * 2 >= value.len() {
        return None;
    }
    let heading = value[leading..value.len() - trailing].trim();
    if heading.is_empty() {
        return None;
    }
    Some((u8::try_from(leading).unwrap_or(6), heading.to_string()))
}

pub(crate) fn summarize_words(value: &str, max_words: usize) -> String {
    normalize_spaces(value)
        .split_whitespace()
        .take(max_words.max(1))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn normalize_multiline_spaces(value: &str) -> String {
    value
        .lines()
        .map(normalize_spaces)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn estimate_tokens(value: &str) -> usize {
    value.chars().count().div_ceil(4)
}

pub(crate) fn extract_template_titles(content: &str) -> Vec<String> {
    extract_template_invocations(content)
        .into_iter()
        .map(|invocation| invocation.template_title)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn extract_link_titles(content: &str) -> Vec<String> {
    extract_wikilinks(content)
        .into_iter()
        .map(|link| link.target_title)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn flatten_markup_excerpt(value: &str) -> String {
    let mut output = String::new();
    let bytes = value.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if cursor + 1 < bytes.len() && bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
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
            if let Some(display) = display_text_for_wikilink(&value[start..end])
                && !display.is_empty()
            {
                if !output.ends_with(' ') && !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&display);
                output.push(' ');
            }
            cursor = end + 2;
            continue;
        }

        if bytes[cursor] == b'<' {
            let mut end = cursor + 1;
            while end < bytes.len() && bytes[end] != b'>' {
                end += 1;
            }
            cursor = end.saturating_add(1);
            continue;
        }

        if cursor + 1 < bytes.len() && bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            let mut depth = 1usize;
            let mut end = cursor + 2;
            while end + 1 < bytes.len() && depth > 0 {
                if bytes[end] == b'{' && bytes[end + 1] == b'{' {
                    depth += 1;
                    end += 2;
                    continue;
                }
                if bytes[end] == b'}' && bytes[end + 1] == b'}' {
                    depth = depth.saturating_sub(1);
                    end += 2;
                    continue;
                }
                end += 1;
            }
            cursor = end.min(bytes.len());
            continue;
        }

        if bytes[cursor] == b'[' && (cursor + 1 >= bytes.len() || bytes[cursor + 1] != b'[') {
            let mut end = cursor + 1;
            while end < bytes.len() && bytes[end] != b']' {
                end += 1;
            }
            let inner = if end < bytes.len() {
                &value[cursor + 1..end]
            } else {
                &value[cursor + 1..]
            };
            let label = inner
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            if !label.is_empty() {
                if !output.ends_with(' ') && !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&label);
                output.push(' ');
            }
            cursor = end.saturating_add(1);
            continue;
        }

        output.push(bytes[cursor] as char);
        cursor += 1;
    }

    normalize_spaces(&output)
}

pub(crate) fn display_text_for_wikilink(inner: &str) -> Option<String> {
    let segments = split_template_segments(inner);
    let target = segments.first()?.trim();
    if target.is_empty() {
        return None;
    }
    let display = segments.last().map(String::as_str).unwrap_or(target).trim();
    if let Some((_, tail)) = display.rsplit_once(':') {
        return Some(normalize_spaces(&tail.replace('_', " ")));
    }
    Some(normalize_spaces(&display.replace('_', " ")))
}
