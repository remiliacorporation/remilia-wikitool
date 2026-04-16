use super::super::entities::decode_html_entities;
use super::super::template_render::{
    ParsedTemplate, TemplateContext, TemplateRendering, render_template,
};
use super::normalize_markdown;

pub fn wikitext_to_markdown(content: &str, _code_language: Option<&str>) -> String {
    let mut renderer = WikitextMarkdownRenderer::default();
    renderer.render(content)
}

fn convert_heading(line: &str) -> Option<String> {
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

#[derive(Default)]
struct WikitextMarkdownRenderer {
    references: Vec<String>,
    categories: Vec<String>,
    media: Vec<String>,
    table_buffer: Vec<String>,
    in_table: bool,
}

enum Segment<'a> {
    Prose(&'a str),
    Template { inner: &'a str },
    ExtensionBlock { raw: &'a str },
}

impl WikitextMarkdownRenderer {
    fn render(&mut self, content: &str) -> String {
        let cleaned = strip_html_comments(content);
        let body = self.render_fragment(&cleaned);
        let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
        append_agent_sections(&mut lines, &self.media, &self.categories, &self.references);
        normalize_markdown(&lines.join("\n"))
    }

    /// Segment-aware render of an arbitrary wikitext fragment. Used both for the top
    /// level and recursively for template parameter values. State (references,
    /// categories, media, table buffer) is shared across recursive calls so that a
    /// `<ref>` nested inside an infobox parameter is still lifted into the document's
    /// reference section.
    ///
    /// Logical wikitext lines can span multiple segments (an inline template inside a
    /// paragraph is one example). The renderer accumulates the current in-progress
    /// line across segments and only finalizes it when a newline is encountered in a
    /// prose segment or when a block-level template/extension forces a flush.
    fn render_fragment(&mut self, content: &str) -> String {
        let segments = segment_wikitext(content);
        let mut output_lines: Vec<String> = Vec::new();
        let mut current_line = String::new();

        for index in 0..segments.len() {
            match segments[index] {
                Segment::Prose(text) => {
                    let parts = split_prose_lines_preserving_opaque_blocks(text);
                    let mut parts = parts.into_iter();
                    if let Some(first) = parts.next() {
                        current_line.push_str(first);
                    }
                    for part in parts {
                        let completed = std::mem::take(&mut current_line);
                        self.finalize_prose_line(&completed, &mut output_lines);
                        current_line.push_str(part);
                    }
                }
                Segment::Template { inner } => {
                    let context = classify_template_context(&segments, index);
                    let rendering = self.render_template_invocation(inner, context);
                    match rendering {
                        TemplateRendering::Drop => {}
                        TemplateRendering::Inline(text) => current_line.push_str(&text),
                        TemplateRendering::Block(body) => {
                            self.flush_pending_line(&mut current_line, &mut output_lines);
                            push_blank_separator(&mut output_lines);
                            for line in body.lines() {
                                output_lines.push(line.to_string());
                            }
                            output_lines.push(String::new());
                        }
                        TemplateRendering::Fenced => {
                            if line_starts_redirect(&current_line) {
                                continue;
                            }
                            self.flush_pending_line(&mut current_line, &mut output_lines);
                            push_blank_separator(&mut output_lines);
                            push_fenced_wikitext(&mut output_lines, &format!("{{{{{inner}}}}}"));
                        }
                    }
                }
                Segment::ExtensionBlock { raw } => {
                    self.flush_pending_line(&mut current_line, &mut output_lines);
                    push_blank_separator(&mut output_lines);
                    push_fenced_wikitext(&mut output_lines, raw);
                }
            }
        }
        self.flush_pending_line(&mut current_line, &mut output_lines);
        output_lines.join("\n")
    }

    fn flush_pending_line(&mut self, current_line: &mut String, output: &mut Vec<String>) {
        if current_line.is_empty() {
            return;
        }
        let completed = std::mem::take(current_line);
        self.finalize_prose_line(&completed, output);
    }

    fn finalize_prose_line(&mut self, line: &str, output: &mut Vec<String>) {
        if self.in_table {
            self.table_buffer.push(line.to_string());
            if line.trim_start().starts_with("|}") {
                let table = self.flush_table();
                output.push(table);
            }
            return;
        }
        if line.trim_start().starts_with("{|") {
            self.in_table = true;
            self.table_buffer.clear();
            self.table_buffer.push(line.to_string());
            return;
        }
        let trimmed = line.trim();
        if let Some(redirect) = convert_redirect_line(trimmed) {
            output.push(redirect);
            return;
        }
        if let Some(category) = extract_category_link(trimmed) {
            self.categories.push(category);
            return;
        }
        if let Some(media) = extract_media_link(trimmed) {
            self.media.push(media);
            return;
        }
        let converted = convert_heading(line).unwrap_or_else(|| {
            let line_with_list = convert_list_prefix(line);
            let line_with_refs = self.convert_refs(&line_with_list);
            convert_inline_wikitext(&line_with_refs)
        });
        output.push(converted);
    }

    fn render_template_invocation(
        &mut self,
        inner: &str,
        context: TemplateContext,
    ) -> TemplateRendering {
        let Some(template) = ParsedTemplate::parse(inner) else {
            return TemplateRendering::Fenced;
        };
        let mut recurse = |fragment: &str| self.render_fragment(fragment);
        render_template(&template, context, &mut recurse)
    }

    fn convert_refs(&mut self, line: &str) -> String {
        let mut output = String::new();
        let mut index = 0usize;
        while index < line.len() {
            let Some(start_offset) = index_of_ignore_case(line, "<ref", index) else {
                output.push_str(&line[index..]);
                break;
            };
            output.push_str(&line[index..start_offset]);
            let Some(open_end_offset) = line[start_offset..].find('>') else {
                output.push_str(&line[start_offset..]);
                break;
            };
            let open_end = start_offset + open_end_offset;
            let open_tag = &line[start_offset..=open_end];
            let self_closing = open_tag.trim_end().ends_with("/>");
            let name = parse_ref_name(open_tag);
            if self_closing {
                let marker = name.unwrap_or_else(|| format!("ref-{}", self.references.len() + 1));
                output.push_str(&format!("[^{marker}]"));
                index = open_end + 1;
                continue;
            }
            let Some(close_start) = index_of_ignore_case(line, "</ref>", open_end + 1) else {
                output.push_str(&line[start_offset..]);
                break;
            };
            let raw_ref = line[open_end + 1..close_start].trim();
            let marker = name.unwrap_or_else(|| format!("ref-{}", self.references.len() + 1));
            if !raw_ref.is_empty()
                && !self
                    .references
                    .iter()
                    .any(|entry| entry.starts_with(&format!("[^{marker}]:")))
            {
                let ref_text = convert_inline_wikitext(raw_ref);
                self.references
                    .push(format_reference_entry(&marker, &ref_text));
            }
            output.push_str(&format!("[^{marker}]"));
            index = close_start + "</ref>".len();
        }
        output
    }

    fn flush_table(&mut self) -> String {
        self.in_table = false;
        let table = self.table_buffer.join("\n");
        self.table_buffer.clear();
        format!("```wikitext\n{table}\n```")
    }
}

fn split_prose_lines_preserving_opaque_blocks(text: &str) -> Vec<&str> {
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

fn format_reference_entry(marker: &str, ref_text: &str) -> String {
    let mut lines = ref_text.lines();
    let first = lines.next().unwrap_or("").trim_end();
    let mut output = format!("[^{marker}]: {first}");
    for line in lines {
        output.push('\n');
        output.push_str("    ");
        output.push_str(line.trim_end());
    }
    output
}

/// Walk `content` char-by-char and split it into prose runs, top-level template
/// invocations, and complex extension blocks. The inline opaque tags (`ref`,
/// `nowiki`) stay inside prose because their bodies should not affect template
/// segmentation. Complex extension blocks are fenced verbatim rather than flattened.
fn segment_wikitext(content: &str) -> Vec<Segment<'_>> {
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

fn classify_template_context(segments: &[Segment<'_>], index: usize) -> TemplateContext {
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

fn push_blank_separator(output: &mut Vec<String>) {
    if !matches!(output.last(), Some(value) if value.is_empty()) {
        output.push(String::new());
    }
}

fn push_fenced_wikitext(output: &mut Vec<String>, raw: &str) {
    output.push("```wikitext".to_string());
    output.extend(raw.lines().map(str::to_string));
    output.push("```".to_string());
    output.push(String::new());
}

fn append_agent_sections(
    lines: &mut Vec<String>,
    media: &[String],
    categories: &[String],
    references: &[String],
) {
    if !media.is_empty() {
        lines.push(String::new());
        lines.push("## Media".to_string());
        lines.push(String::new());
        for item in media {
            lines.push(format!("- {item}"));
        }
    }
    if !categories.is_empty() {
        lines.push(String::new());
        lines.push("## Categories".to_string());
        lines.push(String::new());
        for category in categories {
            lines.push(format!("- {category}"));
        }
    }
    if !references.is_empty() {
        lines.push(String::new());
        lines.push("## References".to_string());
        lines.push(String::new());
        lines.extend(references.iter().cloned());
    }
}

fn convert_inline_wikitext(line: &str) -> String {
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

fn convert_redirect_line(line: &str) -> Option<String> {
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

fn line_starts_redirect(line: &str) -> bool {
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

fn convert_list_prefix(line: &str) -> String {
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

fn extract_category_link(trimmed: &str) -> Option<String> {
    let inner = extract_wrapped_link(trimmed)?;
    let title = inner
        .strip_prefix("Category:")
        .or_else(|| inner.strip_prefix("category:"))?;
    Some(title.split('|').next().unwrap_or(title).trim().to_string())
}

fn extract_media_link(trimmed: &str) -> Option<String> {
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

fn parse_ref_name(open_tag: &str) -> Option<String> {
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

fn strip_html_comments(content: &str) -> String {
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

fn strip_simple_html_tags(line: &str) -> String {
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

fn index_of_ignore_case(text: &str, search: &str, start: usize) -> Option<usize> {
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

#[cfg(test)]
mod tests {
    use super::wikitext_to_markdown;

    #[test]
    fn wikitext_to_markdown_extracts_agent_sections() {
        let markdown = wikitext_to_markdown(
            r#"
{{Short description|Example}}
'''Milady''' is linked to [[Remilia Corporation|Remilia]].<ref name="site">{{cite web|url=https://example.com|title=Example}}</ref>

== Gallery ==
[[File:Milady.png|thumb|Milady portrait]]

{| class="wikitable"
|-
! A !! B
|-
| 1 || 2
|}

[[Category:Remilia]]
"#,
            None,
        );

        assert!(
            markdown
                .contains("**Milady** is linked to [Remilia](wiki://Remilia Corporation).[^site]")
        );
        assert!(markdown.contains("```wikitext"));
        assert!(markdown.contains("## Media"));
        assert!(markdown.contains("- Milady.png - Milady portrait"));
        assert!(markdown.contains("## Categories"));
        assert!(markdown.contains("- Remilia"));
        assert!(markdown.contains("## References"));
        assert!(markdown.contains("[^site]: {{cite web|url=https://example.com|title=Example}}"));
        assert!(!markdown.contains("Short description"));
    }

    #[test]
    fn wikitext_to_markdown_converts_lists_and_external_links() {
        let markdown = wikitext_to_markdown(
            "* [https://example.com Example]\n** [[Target|Label]]\n# Step",
            None,
        );

        assert!(markdown.contains("- [Example](https://example.com)"));
        assert!(markdown.contains("  - [Label](wiki://Target)"));
        assert!(markdown.contains("1. Step"));
    }

    #[test]
    fn wikitext_to_markdown_skips_metadata_blocks_and_converts_definition_lists() {
        let markdown = wikitext_to_markdown(
            r#"{{#seo:
|title=Hidden metadata
}}
; [[:Category:Things|Things]]
: Useful description with [[Target]].
; Term : Inline definition
"#,
            None,
        );

        assert!(!markdown.contains("#seo"));
        assert!(!markdown.contains("Hidden metadata"));
        assert!(markdown.contains("- **[Things](wiki://:Category:Things)**"));
        assert!(markdown.contains("  Useful description with [Target](wiki://Target)."));
        assert!(markdown.contains("- **Term:** Inline definition"));
    }

    #[test]
    fn wikitext_to_markdown_flattens_infobox_and_inline_templates() {
        let markdown = wikitext_to_markdown(
            r#"{{Short description|Fastest land mammal}}
{{Use British English|date=May 2020}}
{{Good article}}
{{Speciesbox
| name = Cheetah
| status = VU
| authority = ([[Johann Christian Daniel von Schreber|Schreber]], 1775)
}}

The '''cheetah''' reaches {{cvt|93|km/h|mph}} and is native to {{lang|en|Africa}} and central [[Iran]]. In {{small|(older texts)}} the species was called a "hunting leopard".
"#,
            None,
        );

        assert!(!markdown.contains("Short description"));
        assert!(!markdown.contains("Use British English"));
        assert!(!markdown.contains("Good article"));
        assert!(markdown.contains("**Speciesbox**"));
        assert!(markdown.contains("- **name:** Cheetah"));
        assert!(markdown.contains("- **status:** VU"));
        assert!(markdown.contains(
            "- **authority:** ([Schreber](wiki://Johann Christian Daniel von Schreber), 1775)"
        ));
        assert!(markdown.contains("reaches 93 km/h"));
        assert!(markdown.contains("native to Africa"));
        assert!(markdown.contains("In (older texts) the species"));
        assert!(!markdown.contains("{{cvt"));
        assert!(!markdown.contains("{{lang"));
        assert!(!markdown.contains("{{small"));
        assert!(markdown.contains("[Iran](wiki://Iran)"));
    }

    #[test]
    fn wikitext_to_markdown_preserves_unknown_templates_as_fenced_wikitext() {
        let markdown = wikitext_to_markdown(
            "Head.\n\n{{UnknownTemplate\n|kind = test\n|value = 42\n}}\n\nTail.\n",
            None,
        );
        assert!(markdown.contains("Head."));
        assert!(markdown.contains("```wikitext"));
        assert!(markdown.contains("{{UnknownTemplate"));
        assert!(markdown.contains("Tail."));
    }

    #[test]
    fn wikitext_to_markdown_preserves_parser_functions_as_fenced_wikitext() {
        let markdown = wikitext_to_markdown(
            "Lead.\n\n{{#if: condition | visible | hidden}}\n\nTail.",
            None,
        );

        assert!(markdown.contains("Lead."));
        assert!(markdown.contains("```wikitext"));
        assert!(markdown.contains("{{#if: condition | visible | hidden}}"));
        assert!(markdown.contains("Tail."));
    }

    #[test]
    fn wikitext_to_markdown_keeps_prose_after_inline_template_on_same_logical_line() {
        let markdown = wikitext_to_markdown(
            "The cheetah runs at {{cvt|93|km/h|mph|}}; it has powerful hindlimbs.",
            None,
        );
        assert!(
            markdown.contains("runs at 93 km/h; it has powerful hindlimbs."),
            "unexpected render: {markdown}"
        );
        assert!(!markdown.contains("- **it has"));
    }

    #[test]
    fn wikitext_to_markdown_rejects_cite_through_refs_verbatim() {
        let markdown = wikitext_to_markdown(
            "Claim A.<ref name=a>{{cite web|url=https://example.com/a|title=A}}</ref> Claim B.<ref name=b>{{cite news|title=B|url=https://example.com/b}}</ref>\n",
            None,
        );
        assert!(markdown.contains("Claim A.[^a]"));
        assert!(markdown.contains("Claim B.[^b]"));
        assert!(markdown.contains("[^a]: {{cite web|url=https://example.com/a|title=A}}"));
        assert!(markdown.contains("[^b]: {{cite news|title=B|url=https://example.com/b}}"));
    }

    #[test]
    fn wikitext_to_markdown_extracts_multiline_refs_as_single_footnotes() {
        let markdown = wikitext_to_markdown(
            "The cheetah evolved.<ref>{{cite web\n|url=https://example.com\n|title=Example\n}}</ref> It runs fast.",
            None,
        );

        assert!(markdown.contains("The cheetah evolved.[^ref-1] It runs fast."));
        assert!(markdown.contains("[^ref-1]: {{cite web"));
        assert!(markdown.contains("    |url=https://example.com"));
        assert!(markdown.contains("    |title=Example"));
        assert!(!markdown.contains("<ref>"));
    }

    #[test]
    fn wikitext_to_markdown_preserves_nowiki_literal_markup() {
        let markdown = wikitext_to_markdown(
            "Literal <nowiki>[[Not a link]] and {{not a template}}</nowiki> text.",
            None,
        );

        assert!(markdown.contains("Literal [[Not a link]] and {{not a template}} text."));
        assert!(!markdown.contains("wiki://Not a link"));
        assert!(!markdown.contains("```wikitext"));
    }

    #[test]
    fn wikitext_to_markdown_renders_redirects_explicitly() {
        let markdown = wikitext_to_markdown("#REDIRECT [[Target Page]] {{R from move}}", None);

        assert_eq!(markdown, "Redirect to [Target Page](wiki://Target Page)");
        assert!(!markdown.contains("1. REDIRECT"));
    }

    #[test]
    fn wikitext_to_markdown_fences_complex_extension_blocks() {
        let markdown = wikitext_to_markdown(
            "Lead.\n\n<gallery>\nFile:Example.jpg|Caption\n</gallery>\n\nTail.",
            None,
        );

        assert!(markdown.contains("Lead."));
        assert!(
            markdown.contains("```wikitext\n<gallery>\nFile:Example.jpg|Caption\n</gallery>\n```")
        );
        assert!(markdown.contains("Tail."));
    }

    #[test]
    fn wikitext_to_markdown_fences_syntax_and_math_blocks() {
        let markdown = wikitext_to_markdown(
            "<syntaxhighlight lang=\"rust\">\nfn main() {}\n</syntaxhighlight>\n\n<math>E=mc^2</math>",
            None,
        );

        assert!(markdown.contains(
            "```wikitext\n<syntaxhighlight lang=\"rust\">\nfn main() {}\n</syntaxhighlight>\n```"
        ));
        assert!(markdown.contains("```wikitext\n<math>E=mc^2</math>\n```"));
    }

    #[test]
    fn wikitext_to_markdown_does_not_split_templates_inside_wikilinks() {
        let markdown = wikitext_to_markdown(
            "[[File:Icon.svg|alt=Icon|{{dir|en|left|right}}|125x125px]]",
            None,
        );

        assert!(markdown.contains("## Media"));
        assert!(markdown.contains("- Icon.svg - {{dir|en|left|right}}"));
        assert!(!markdown.contains("```wikitext"));
    }
}
