use super::super::entities::decode_html_entities;
use super::{normalize_markdown, wikitext_to_markdown};

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

fn html_to_markdown(html: &str) -> String {
    let mut output = String::new();
    let mut index = 0usize;
    let mut skip_depth = 0usize;

    while index < html.len() {
        let Some(start) = html[index..].find('<') else {
            if skip_depth == 0 {
                append_text_segment(&mut output, &html[index..]);
            }
            break;
        };
        let tag_start = index + start;
        if skip_depth == 0 {
            append_text_segment(&mut output, &html[index..tag_start]);
        }
        if starts_with_at(html, tag_start, "<!--") {
            let Some(end) = index_of_ignore_case(html, "-->", tag_start + 4) else {
                break;
            };
            index = end + 3;
            continue;
        }
        let Some(tag_end) = find_tag_end(html, tag_start) else {
            break;
        };
        let raw_tag = &html[tag_start..=tag_end];
        let Some((tag_name, is_closing, is_self_closing)) = parse_tag_descriptor(raw_tag) else {
            index = tag_end + 1;
            continue;
        };

        if is_closing {
            if is_skip_tag(tag_name) && skip_depth > 0 {
                skip_depth -= 1;
            }
            if skip_depth == 0 {
                if is_paragraph_block_tag(tag_name) {
                    append_separator(&mut output, "\n\n");
                } else if is_block_tag(tag_name) {
                    append_separator(&mut output, "\n");
                }
            }
        } else if is_skip_tag(tag_name) && !is_self_closing {
            skip_depth += 1;
        } else if skip_depth == 0 {
            if tag_name.eq_ignore_ascii_case("br") {
                append_separator(&mut output, "\n");
            } else if tag_name.eq_ignore_ascii_case("li") {
                append_separator(&mut output, "\n- ");
            } else if is_paragraph_block_tag(tag_name) {
                append_separator(&mut output, "\n\n");
            } else if is_block_tag(tag_name) {
                append_separator(&mut output, "\n");
            }
        }

        index = tag_end + 1;
    }

    normalize_markdown(&output)
}

fn append_text_segment(output: &mut String, text: &str) {
    let decoded = decode_html_entities(text);
    if !decoded.is_empty() {
        output.push_str(&decoded);
    }
}

fn append_separator(output: &mut String, separator: &str) {
    if output.is_empty() {
        return;
    }
    match separator {
        "\n\n" => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if output.ends_with("\n\n") {
                return;
            }
            if output.ends_with('\n') {
                output.push('\n');
            } else {
                output.push_str("\n\n");
            }
        }
        "\n- " => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if output.ends_with('\n') {
                output.push_str("- ");
            } else {
                output.push_str("\n- ");
            }
        }
        "\n" => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }
        other => output.push_str(other),
    }
}

fn is_paragraph_block_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "p" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    )
}

fn is_block_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "p" | "div"
            | "section"
            | "article"
            | "main"
            | "li"
            | "ul"
            | "ol"
            | "table"
            | "tr"
            | "blockquote"
            | "pre"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

fn is_skip_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "script" | "style" | "noscript" | "template" | "svg" | "canvas"
    )
}

fn is_self_closing_tag(tag_raw: &str, tag_name: &str) -> bool {
    let normalized = tag_name.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "br" | "hr" | "img" | "meta" | "link" | "input" | "source"
    ) {
        return true;
    }
    tag_raw.trim_end().ends_with("/>")
}

fn parse_tag_descriptor(tag_raw: &str) -> Option<(&str, bool, bool)> {
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

fn find_tag_end(html: &str, start: usize) -> Option<usize> {
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

fn starts_with_at(text: &str, index: usize, sequence: &str) -> bool {
    if index + sequence.len() > text.len() {
        return false;
    }
    &text[index..index + sequence.len()] == sequence
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

#[cfg(test)]
mod tests {
    use super::source_content_to_markdown;

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

    #[test]
    fn source_content_to_markdown_separates_html_paragraph_blocks() {
        let markdown = source_content_to_markdown(
            "<article><h1>Title</h1><p>First paragraph.</p><p>Second paragraph.</p><blockquote>Quoted text.</blockquote></article>",
            "html",
            None,
        );

        assert!(markdown.contains("Title\n\nFirst paragraph."));
        assert!(markdown.contains("First paragraph.\n\nSecond paragraph."));
        assert!(markdown.contains("Second paragraph.\n\nQuoted text."));
    }

    #[test]
    fn source_content_to_markdown_keeps_html_list_items_compact() {
        let markdown = source_content_to_markdown(
            "<article><p>Status</p><ul><li>Near threatened</li><li>Vulnerable</li></ul></article>",
            "html",
            None,
        );

        assert!(markdown.contains("Status\n\n- Near threatened\n- Vulnerable"));
        assert!(!markdown.contains("- Near threatened\n\n- Vulnerable"));
    }
}
