use std::env;
use std::path::{Path, PathBuf};

use super::entities::decode_html_entities;
use super::model::{DEFAULT_EXPORTS_DIR, ExportFormat};

pub fn wikitext_to_markdown(content: &str, _code_language: Option<&str>) -> String {
    let mut renderer = WikitextMarkdownRenderer::default();
    renderer.render(content)
}

pub fn source_content_to_markdown(
    content: &str,
    content_format: &str,
    code_language: Option<&str>,
) -> String {
    if content_format.eq_ignore_ascii_case("wikitext") {
        return wikitext_to_markdown(content, code_language);
    }
    if content_format.eq_ignore_ascii_case("html") {
        return html_to_markdown(content);
    }
    if content_format.eq_ignore_ascii_case("markdown") {
        return normalize_markdown(content);
    }
    if content_format.eq_ignore_ascii_case("text") {
        return text_to_markdown(content);
    }
    fenced_source(content, content_format)
}

pub fn generate_frontmatter(
    title: &str,
    source_url: &str,
    source_domain: &str,
    timestamp: &str,
    extra_fields: &[(String, String)],
) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("title: \"{}\"", title.replace('"', "\\\"")),
        format!("source_url: \"{}\"", source_url.replace('"', "\\\"")),
        format!("source_domain: \"{}\"", source_domain.replace('"', "\\\"")),
        format!("fetched_at: \"{}\"", timestamp.replace('"', "\\\"")),
    ];
    for (key, value) in extra_fields {
        lines.push(format!("{key}: \"{}\"", value.replace('"', "\\\"")));
    }
    lines.push("---".to_string());
    lines.join("\n")
}

pub fn sanitize_filename(value: &str) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        let mapped = match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => Some(ch),
            '-' | '_' => Some(ch),
            _ if ch.is_whitespace() => Some('-'),
            _ => None,
        };
        if let Some(ch) = mapped {
            let is_separator = ch == '-' || ch == '_';
            if is_separator && last_was_separator {
                continue;
            }
            output.push(ch);
            last_was_separator = is_separator;
        }
    }
    output.trim_matches(['-', '_']).to_string()
}

pub fn default_export_path(
    project_root: &Path,
    title: &str,
    is_directory: bool,
    format: ExportFormat,
) -> Option<PathBuf> {
    if env::var("WIKITOOL_NO_DEFAULT_EXPORTS").is_ok() {
        return None;
    }
    let filename = sanitize_filename(title);
    let exports_dir = project_root.join(DEFAULT_EXPORTS_DIR);
    if is_directory {
        return Some(exports_dir.join(filename));
    }
    Some(exports_dir.join(format!("{}.{}", filename, format.file_extension())))
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
    metadata_template_depth: Option<isize>,
    in_table: bool,
}

impl WikitextMarkdownRenderer {
    fn render(&mut self, content: &str) -> String {
        let mut lines = Vec::new();
        for line in strip_html_comments(content).lines() {
            let trimmed = line.trim();
            if self.skip_metadata_template_line(trimmed) {
                continue;
            }

            if self.in_table {
                self.table_buffer.push(line.to_string());
                if line.trim_start().starts_with("|}") {
                    lines.push(self.flush_table());
                }
                continue;
            }

            if trimmed.starts_with("{|") {
                self.in_table = true;
                self.table_buffer.clear();
                self.table_buffer.push(line.to_string());
                continue;
            }
            if let Some(category) = extract_category_link(trimmed) {
                self.categories.push(category);
                continue;
            }
            if let Some(media) = extract_media_link(trimmed) {
                self.media.push(media);
                continue;
            }

            let converted = convert_heading(line).unwrap_or_else(|| {
                let line = convert_list_prefix(line);
                let line = self.convert_refs(&line);
                convert_inline_wikitext(&line)
            });
            lines.push(converted);
        }
        if self.in_table {
            lines.push(self.flush_table());
        }
        append_agent_sections(&mut lines, &self.media, &self.categories, &self.references);
        normalize_markdown(&lines.join("\n"))
    }

    fn skip_metadata_template_line(&mut self, trimmed: &str) -> bool {
        if let Some(depth) = self.metadata_template_depth {
            let next_depth = depth + template_brace_pair_balance(trimmed);
            self.metadata_template_depth = (next_depth > 0).then_some(next_depth);
            return true;
        }

        if !is_metadata_template_start(trimmed) {
            return false;
        }

        let depth = template_brace_pair_balance(trimmed);
        if depth > 0 {
            self.metadata_template_depth = Some(depth);
        }
        true
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
                self.references.push(format!("[^{marker}]: {ref_text}"));
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
    let line = convert_external_links(line);
    let line = convert_internal_links(&line);
    let line = strip_simple_html_tags(&line);
    convert_emphasis(&line)
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
    let parts = target.split('|').map(str::trim).collect::<Vec<_>>();
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

fn extract_wrapped_link(trimmed: &str) -> Option<&str> {
    trimmed.strip_prefix("[[")?.strip_suffix("]]")
}

fn is_metadata_template_start(trimmed: &str) -> bool {
    let normalized = trimmed.to_ascii_lowercase();
    normalized.starts_with("{{short description")
        || normalized.starts_with("{{use dmy dates")
        || normalized.starts_with("{{use mdy dates")
        || normalized.starts_with("{{defaultsort:")
        || normalized.starts_with("{{displaytitle:")
        || normalized.starts_with("{{#seo:")
}

fn template_brace_pair_balance(line: &str) -> isize {
    let chars = line.chars().collect::<Vec<_>>();
    let mut balance = 0isize;
    let mut index = 0usize;
    while index + 1 < chars.len() {
        if chars[index] == '{' && chars[index + 1] == '{' {
            balance += 1;
            index += 2;
            continue;
        }
        if chars[index] == '}' && chars[index + 1] == '}' {
            balance -= 1;
            index += 2;
            continue;
        }
        index += 1;
    }
    balance
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

fn html_to_markdown(html: &str) -> String {
    let mut output = String::new();
    let mut index = 0usize;
    while index < html.len() {
        let Some(start) = html[index..].find('<') else {
            output.push_str(&html[index..]);
            break;
        };
        let absolute_start = index + start;
        output.push_str(&html[index..absolute_start]);
        let Some(end) = html[absolute_start..].find('>') else {
            break;
        };
        let tag = html[absolute_start + 1..absolute_start + end]
            .trim()
            .trim_start_matches('/')
            .to_ascii_lowercase();
        if is_html_block_tag(&tag) {
            output.push_str("\n\n");
        }
        index = absolute_start + end + 1;
    }
    normalize_markdown(&decode_basic_entities(&output))
}

fn is_html_block_tag(tag: &str) -> bool {
    let name = tag
        .split(|ch: char| ch.is_ascii_whitespace() || ch == '/')
        .next()
        .unwrap_or("");
    matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "main"
            | "li"
            | "tr"
            | "table"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
    )
}

fn text_to_markdown(content: &str) -> String {
    normalize_markdown(content)
}

fn fenced_source(content: &str, content_format: &str) -> String {
    let language = match content_format {
        "json" => "json",
        "xml" => "xml",
        _ => "text",
    };
    format!("```{language}\n{}\n```", content.trim())
}

fn normalize_markdown(content: &str) -> String {
    let mut lines = Vec::new();
    let mut blank_count = 0usize;
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = merge_isolated_list_markers(&normalized);
    for line in normalized.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.trim().is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                lines.push(String::new());
            }
            continue;
        }
        blank_count = 0;
        lines.push(trimmed_end.to_string());
    }
    while matches!(lines.first(), Some(line) if line.is_empty()) {
        lines.remove(0);
    }
    while matches!(lines.last(), Some(line) if line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

fn merge_isolated_list_markers(content: &str) -> String {
    let source_lines = content.lines().collect::<Vec<_>>();
    let mut lines = Vec::with_capacity(source_lines.len());
    let mut index = 0usize;
    while index < source_lines.len() {
        let line = source_lines[index].trim_end();
        if is_isolated_unordered_list_marker(line)
            && let Some(next) = source_lines.get(index + 1).map(|value| value.trim())
            && !next.is_empty()
        {
            let indentation = &line[..line.len() - line.trim_start().len()];
            lines.push(format!("{indentation}- {next}"));
            index += 2;
            continue;
        }
        lines.push(line.to_string());
        index += 1;
    }
    lines.join("\n")
}

fn is_isolated_unordered_list_marker(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "-" || trimmed == "*"
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
    use super::{source_content_to_markdown, wikitext_to_markdown};

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
    fn source_content_to_markdown_uses_readable_web_text_directly() {
        let markdown = source_content_to_markdown(
            "Title\r\n\r\nReadable paragraph.\r\n\r\n\r\nSecond paragraph.",
            "text",
            None,
        );

        assert_eq!(
            markdown,
            "Title\n\nReadable paragraph.\n\nSecond paragraph."
        );
    }

    #[test]
    fn source_content_to_markdown_merges_extracted_list_markers() {
        let markdown =
            source_content_to_markdown("Status\n-\nNear threatened\n-\nVulnerable", "text", None);

        assert!(markdown.contains("- Near threatened"));
        assert!(markdown.contains("- Vulnerable"));
        assert!(!markdown.contains("\n-\n"));
    }

    #[test]
    fn source_content_to_markdown_strips_basic_html() {
        let markdown = source_content_to_markdown(
            "<article><h1>Title</h1><p>Readable &amp; useful &#x27;text&#x27;.</p></article>",
            "html",
            None,
        );

        assert!(markdown.contains("Title"));
        assert!(markdown.contains("Readable & useful 'text'."));
        assert!(!markdown.contains("<article>"));
    }
}
