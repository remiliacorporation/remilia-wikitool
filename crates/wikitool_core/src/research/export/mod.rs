use std::env;
use std::path::{Path, PathBuf};

use super::model::{DEFAULT_EXPORTS_DIR, ExportFormat};

mod html_text;
mod wikitext;

pub use html_text::source_content_to_markdown;
pub(crate) use wikitext::lint::{WikitextLintIssue, lint_wikitext};
pub use wikitext::wikitext_to_markdown;

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

pub(crate) fn normalize_markdown(content: &str) -> String {
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
