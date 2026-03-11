use super::*;
use super::parse_markup::{find_case_insensitive, find_tag_end};
use super::parse_sections::RawSection;

pub(super) fn extract_examples_for_section(page_title: &str, section: &RawSection) -> Vec<ParsedDocsExample> {
    let mut examples = Vec::new();
    for tag_name in ["syntaxhighlight", "source", "pre", "code"] {
        for (language, body) in extract_tagged_examples(&section.text, tag_name) {
            let example_text = collapse_whitespace(&body);
            if example_text.len() < 4 {
                continue;
            }
            let heading = if section.kind == "lead" {
                None
            } else {
                Some(section.heading.clone())
            };
            let summary_text = if section.kind == "lead" {
                format!("Example from {page_title}")
            } else {
                format!("Example from {} > {}", page_title, section.heading_path)
            };
            let retrieval_text = collapse_whitespace(&format!(
                "{} {} {} {}",
                page_title,
                heading.as_deref().unwrap_or("Lead"),
                language.as_deref().unwrap_or(""),
                example_text
            ));
            examples.push(ParsedDocsExample {
                example_index: 0,
                page_title: page_title.to_string(),
                section_heading: heading,
                example_kind: tag_name.to_string(),
                language_hint: language.clone().unwrap_or_default(),
                language,
                summary_text,
                example_text: body.trim().to_string(),
                retrieval_text,
                token_estimate: estimate_token_count(&body),
            });
        }
    }
    examples
}


fn extract_tagged_examples(content: &str, tag_name: &str) -> Vec<(Option<String>, String)> {
    let mut out = Vec::new();
    let lower = content.to_ascii_lowercase();
    let open_pattern = format!("<{tag_name}");
    let close_pattern = format!("</{tag_name}>");
    let bytes = content.as_bytes();
    let lower_bytes = lower.as_bytes();
    let open_bytes = open_pattern.as_bytes();
    let close_bytes = close_pattern.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if lower_bytes[cursor..].starts_with(open_bytes) {
            let Some(tag_end) = find_tag_end(bytes, cursor) else {
                break;
            };
            let attrs = &content[cursor + 1 + tag_name.len()..tag_end];
            let body_start = tag_end + 1;
            let Some(close_start) = find_case_insensitive(lower_bytes, body_start, close_bytes)
            else {
                break;
            };
            let body = content[body_start..close_start].to_string();
            let language = extract_attribute_value(attrs, "lang")
                .or_else(|| extract_attribute_value(attrs, "language"));
            out.push((language, body));
            cursor = close_start + close_pattern.len();
            continue;
        }
        cursor += 1;
    }

    out
}

fn extract_attribute_value(attrs: &str, key: &str) -> Option<String> {
    for part in attrs.split_whitespace() {
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if name.eq_ignore_ascii_case(key) {
            let normalized = collapse_whitespace(value.trim_matches('"').trim_matches('\''));
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }
    None
}
