use std::env;
use std::path::{Path, PathBuf};

use super::model::{DEFAULT_EXPORTS_DIR, ExportFormat};

pub fn wikitext_to_markdown(content: &str, _code_language: Option<&str>) -> String {
    content
        .lines()
        .map(|line| {
            convert_heading(line)
                .unwrap_or_else(|| convert_internal_links(line).replace("'''", "**"))
        })
        .collect::<Vec<_>>()
        .join("\n")
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
