use super::parse_markup::{
    decamelize, dedupe_strings, find_balanced_braces, find_delimited, find_tag_end,
    strip_tagged_block,
};
use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub(super) struct RawSection {
    pub(super) heading: String,
    pub(super) heading_path: String,
    pub(super) level: u8,
    pub(super) kind: String,
    pub(super) text: String,
}

pub(super) fn split_into_sections(content: &str) -> Vec<RawSection> {
    let mut sections = Vec::new();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut current_heading = "Lead".to_string();
    let mut current_level = 1u8;
    let mut current_kind = "lead".to_string();
    let mut current_lines = Vec::new();

    let flush_section = |sections: &mut Vec<RawSection>,
                         heading: &str,
                         level: u8,
                         kind: &str,
                         lines: &mut Vec<String>,
                         heading_stack: &[(u8, String)]| {
        let text = lines.join("\n").trim().to_string();
        if text.is_empty() && kind != "lead" {
            lines.clear();
            return;
        }
        let heading_path = if kind == "lead" {
            "Lead".to_string()
        } else {
            heading_stack
                .iter()
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>()
                .join(" > ")
        };
        sections.push(RawSection {
            heading: heading.to_string(),
            heading_path,
            level,
            kind: kind.to_string(),
            text,
        });
        lines.clear();
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((level, heading)) = parse_heading_line(trimmed) {
            flush_section(
                &mut sections,
                &current_heading,
                current_level,
                &current_kind,
                &mut current_lines,
                &heading_stack,
            );
            while heading_stack
                .last()
                .is_some_and(|(existing_level, _)| *existing_level >= level)
            {
                heading_stack.pop();
            }
            heading_stack.push((level, heading.clone()));
            current_heading = heading;
            current_level = level;
            current_kind = "section".to_string();
        } else {
            current_lines.push(line.to_string());
        }
    }

    flush_section(
        &mut sections,
        &current_heading,
        current_level,
        &current_kind,
        &mut current_lines,
        &heading_stack,
    );

    if sections.is_empty() {
        sections.push(RawSection {
            heading: "Lead".to_string(),
            heading_path: "Lead".to_string(),
            level: 1,
            kind: "lead".to_string(),
            text: content.trim().to_string(),
        });
    }
    sections
}

fn parse_heading_line(value: &str) -> Option<(u8, String)> {
    if value.len() < 4 || !value.starts_with('=') || !value.ends_with('=') {
        return None;
    }

    let leading = value.chars().take_while(|ch| *ch == '=').count();
    let trailing = value.chars().rev().take_while(|ch| *ch == '=').count();
    if leading != trailing || !(2..=6).contains(&leading) {
        return None;
    }

    let inner = value[leading..value.len() - trailing].trim();
    if inner.is_empty() || inner.contains('=') {
        return None;
    }

    Some((leading as u8, normalize_title(inner)))
}

pub(super) fn strip_summary_noise(value: &str) -> String {
    let without_blocks = strip_tagged_block(value, "syntaxhighlight");
    let without_blocks = strip_tagged_block(&without_blocks, "source");
    let without_blocks = strip_tagged_block(&without_blocks, "pre");
    let without_blocks = strip_tagged_block(&without_blocks, "code");
    let mut output = String::with_capacity(without_blocks.len());
    let bytes = without_blocks.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"[[")
            && let Some(end) = find_delimited(bytes, cursor + 2, b"]]")
        {
            let body = &without_blocks[cursor + 2..end];
            let display = body
                .split('|')
                .next_back()
                .unwrap_or(body)
                .split('#')
                .next()
                .unwrap_or(body);
            output.push_str(display.trim_start_matches(':'));
            cursor = end + 2;
            continue;
        }
        if bytes[cursor..].starts_with(b"{{")
            && let Some(end) = find_balanced_braces(bytes, cursor)
        {
            cursor = end;
            continue;
        }
        if bytes[cursor] == b'<'
            && let Some(end) = find_tag_end(bytes, cursor)
        {
            cursor = end + 1;
            continue;
        }
        output.push(bytes[cursor] as char);
        cursor += 1;
    }

    collapse_whitespace(&output)
}

pub(super) fn build_page_aliases(page_title: &str) -> Vec<String> {
    let mut aliases = vec![page_title.to_string()];
    if let Some((_, tail)) = page_title.split_once(':') {
        aliases.push(normalize_title(tail));
        let decamelized = decamelize(tail);
        if !decamelized.is_empty() {
            aliases.push(decamelized);
        }
    }
    dedupe_strings(&mut aliases);
    aliases
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_semantic_text(
    page_title: &str,
    page_kind: &str,
    summary_text: &str,
    headings_text: &str,
    alias_titles: &[String],
    symbol_names: &[String],
    link_titles: &[String],
    sections: &[ParsedDocsSection],
    examples: &[ParsedDocsExample],
) -> String {
    let mut terms = vec![
        page_title.to_string(),
        page_kind.to_string(),
        summary_text.to_string(),
        headings_text.to_string(),
    ];
    terms.extend(alias_titles.iter().cloned());
    terms.extend(symbol_names.iter().cloned());
    terms.extend(link_titles.iter().cloned());
    terms.extend(sections.iter().map(|section| section.summary_text.clone()));
    terms.extend(examples.iter().map(|example| example.summary_text.clone()));
    collapse_whitespace(&terms.join(" | "))
}

pub(super) fn build_section_semantic_text(
    page_title: &str,
    section: &RawSection,
    summary_text: &str,
    symbol_names: &[String],
    link_titles: &[String],
) -> String {
    let mut terms = vec![
        page_title.to_string(),
        section.kind.clone(),
        section.heading.clone(),
        section.heading_path.clone(),
        summary_text.to_string(),
    ];
    terms.extend(symbol_names.iter().cloned());
    terms.extend(link_titles.iter().cloned());
    collapse_whitespace(&terms.join(" | "))
}

pub(super) fn build_page_links(
    link_titles: &[String],
    template_titles: &[String],
) -> Vec<ParsedDocsLink> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for title in link_titles {
        let key = format!("wikilink|{}", title.to_ascii_lowercase());
        if seen.insert(key) {
            out.push(ParsedDocsLink {
                target_title: title.clone(),
                relation_kind: "wikilink".to_string(),
                display_text: title.clone(),
            });
        }
    }
    for title in template_titles {
        let key = format!("template|{}", title.to_ascii_lowercase());
        if seen.insert(key) {
            out.push(ParsedDocsLink {
                target_title: title.clone(),
                relation_kind: "template".to_string(),
                display_text: title.clone(),
            });
        }
    }
    out
}
