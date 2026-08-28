pub(super) fn format_reference_entry(marker: &str, ref_text: &str) -> String {
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
pub(super) fn push_blank_separator(output: &mut Vec<String>) {
    if !matches!(output.last(), Some(value) if value.is_empty()) {
        output.push(String::new());
    }
}

pub(super) fn push_fenced_wikitext(output: &mut Vec<String>, raw: &str) {
    output.push("```wikitext".to_string());
    output.extend(raw.lines().map(str::to_string));
    output.push("```".to_string());
    output.push(String::new());
}

pub(super) fn append_agent_sections(
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
