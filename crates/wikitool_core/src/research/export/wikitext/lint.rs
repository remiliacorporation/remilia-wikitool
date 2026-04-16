#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WikitextLintIssue {
    pub rule_id: &'static str,
    pub message: String,
    pub byte_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Construct {
    Template,
    Link,
}

#[derive(Debug, Clone, Copy)]
struct OpenConstruct {
    kind: Construct,
    offset: usize,
}

const BALANCED_EXTENSION_TAGS: &[&str] = &[
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

pub(crate) fn lint_wikitext(content: &str) -> Vec<WikitextLintIssue> {
    let bytes = content.as_bytes();
    let mut issues = Vec::new();
    let mut stack = Vec::<OpenConstruct>::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if starts_with_at(content, cursor, "<!--") {
            if let Some(end) = index_of_ignore_case(content, "-->", cursor + 4) {
                cursor = end + 3;
                continue;
            }
            issues.push(issue(
                "unclosed_html_comment",
                "HTML comment is missing a closing --> marker.",
                cursor,
            ));
            break;
        }

        if starts_with_tag(content, cursor, "nowiki") {
            match skip_named_html_block(content, cursor, "nowiki") {
                Some(end) => {
                    cursor = end;
                    continue;
                }
                None => {
                    issues.push(issue(
                        "unclosed_nowiki",
                        "Nowiki block is missing a closing </nowiki> tag.",
                        cursor,
                    ));
                    break;
                }
            }
        }

        if starts_with_tag(content, cursor, "ref") {
            let Some(tag_end) = find_tag_end(content, cursor) else {
                issues.push(issue(
                    "malformed_ref_tag",
                    "Reference tag is missing a closing > marker.",
                    cursor,
                ));
                break;
            };
            if content[cursor..=tag_end].trim_end().ends_with("/>") {
                cursor = tag_end + 1;
                continue;
            }
            let Some(end) = index_of_ignore_case(content, "</ref>", tag_end + 1) else {
                issues.push(issue(
                    "unclosed_ref",
                    "Reference tag is missing a closing </ref> tag.",
                    cursor,
                ));
                cursor = tag_end + 1;
                continue;
            };
            cursor = end + "</ref>".len();
            continue;
        }

        if bytes[cursor] == b'<' {
            let mut skipped = false;
            for tag in BALANCED_EXTENSION_TAGS {
                if starts_with_tag(content, cursor, tag) {
                    match skip_named_html_block(content, cursor, tag) {
                        Some(end) => {
                            cursor = end;
                        }
                        None => {
                            issues.push(issue(
                                "unclosed_extension_block",
                                format!(
                                    "Extension block <{tag}> is missing a closing </{tag}> tag."
                                ),
                                cursor,
                            ));
                            cursor += 1;
                        }
                    }
                    skipped = true;
                    break;
                }
            }
            if skipped {
                continue;
            }
        }

        if cursor + 1 < bytes.len() {
            match (bytes[cursor], bytes[cursor + 1]) {
                (b'{', b'{') => {
                    stack.push(OpenConstruct {
                        kind: Construct::Template,
                        offset: cursor,
                    });
                    cursor += 2;
                    continue;
                }
                (b'}', b'}') => {
                    close_construct(&mut stack, &mut issues, Construct::Template, cursor);
                    cursor += 2;
                    continue;
                }
                (b'[', b'[') => {
                    stack.push(OpenConstruct {
                        kind: Construct::Link,
                        offset: cursor,
                    });
                    cursor += 2;
                    continue;
                }
                (b']', b']') => {
                    close_construct(&mut stack, &mut issues, Construct::Link, cursor);
                    cursor += 2;
                    continue;
                }
                _ => {}
            }
        }

        cursor += 1;
    }

    for open in stack.into_iter().rev() {
        let (rule_id, message) = match open.kind {
            Construct::Template => (
                "unclosed_template",
                "Template invocation is missing a closing }} marker.",
            ),
            Construct::Link => (
                "unclosed_wikilink",
                "Wikilink is missing a closing ]] marker.",
            ),
        };
        issues.push(issue(rule_id, message, open.offset));
    }

    issues
}

fn close_construct(
    stack: &mut Vec<OpenConstruct>,
    issues: &mut Vec<WikitextLintIssue>,
    expected: Construct,
    offset: usize,
) {
    let Some(open) = stack.pop() else {
        let (rule_id, message) = match expected {
            Construct::Template => (
                "unexpected_template_close",
                "Template close marker }} has no matching {{ opener.",
            ),
            Construct::Link => (
                "unexpected_wikilink_close",
                "Wikilink close marker ]] has no matching [[ opener.",
            ),
        };
        issues.push(issue(rule_id, message, offset));
        return;
    };

    if open.kind == expected {
        return;
    }

    let (rule_id, message) = match open.kind {
        Construct::Template => (
            "unclosed_template",
            "Template invocation is missing a closing }} marker before another construct closes.",
        ),
        Construct::Link => (
            "unclosed_wikilink",
            "Wikilink is missing a closing ]] marker before another construct closes.",
        ),
    };
    issues.push(issue(rule_id, message, open.offset));
}

fn issue(
    rule_id: &'static str,
    message: impl Into<String>,
    byte_offset: usize,
) -> WikitextLintIssue {
    WikitextLintIssue {
        rule_id,
        message: message.into(),
        byte_offset,
    }
}

fn skip_named_html_block(content: &str, cursor: usize, tag: &str) -> Option<usize> {
    let open_end = find_tag_end(content, cursor)?;
    if content[cursor..=open_end].trim_end().ends_with("/>") {
        return Some(open_end + 1);
    }
    let close = format!("</{tag}>");
    index_of_ignore_case(content, &close, open_end + 1).map(|end| end + close.len())
}

fn starts_with_tag(content: &str, cursor: usize, tag: &str) -> bool {
    let bytes = content.as_bytes();
    if bytes.get(cursor).copied() != Some(b'<') {
        return false;
    }
    let mut index = cursor + 1;
    if bytes.get(index).copied() == Some(b'/') {
        return false;
    }
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let tag_bytes = tag.as_bytes();
    if index + tag_bytes.len() > bytes.len() {
        return false;
    }
    if !bytes_ascii_case_insensitive_eq(&bytes[index..index + tag_bytes.len()], tag_bytes) {
        return false;
    }
    let next = bytes.get(index + tag_bytes.len()).copied();
    matches!(
        next,
        Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
    )
}

fn find_tag_end(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
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
    index + sequence.len() <= text.len() && &text[index..index + sequence.len()] == sequence
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
    (start..=last_start).find(|&index| {
        bytes_ascii_case_insensitive_eq(
            &text_bytes[index..index + search_bytes.len()],
            search_bytes,
        )
    })
}

fn bytes_ascii_case_insensitive_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

#[cfg(test)]
mod tests {
    use super::lint_wikitext;

    #[test]
    fn lint_wikitext_accepts_balanced_article_markup() {
        let issues = lint_wikitext(
            "Lead with [[Target|label]] and {{cvt|1|m}}.<ref>{{cite web|title=A}}</ref>",
        );

        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn lint_wikitext_reports_unclosed_raw_constructs() {
        let issues = lint_wikitext("Lead {{Infobox\n|name = A\n[[Target");
        let rule_ids = issues.iter().map(|issue| issue.rule_id).collect::<Vec<_>>();

        assert!(rule_ids.contains(&"unclosed_template"));
        assert!(rule_ids.contains(&"unclosed_wikilink"));
    }

    #[test]
    fn lint_wikitext_ignores_nowiki_constructs() {
        let issues = lint_wikitext("<nowiki>{{not a template}} [[not a link]]</nowiki>");

        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn lint_wikitext_reports_unclosed_ref_and_extension_blocks() {
        let issues = lint_wikitext("<ref>{{cite web|title=A}</ref>\n<gallery>\nFile:A.jpg");
        let rule_ids = issues.iter().map(|issue| issue.rule_id).collect::<Vec<_>>();

        assert!(rule_ids.contains(&"unclosed_extension_block"));
        assert!(!rule_ids.contains(&"unclosed_ref"));
    }
}
