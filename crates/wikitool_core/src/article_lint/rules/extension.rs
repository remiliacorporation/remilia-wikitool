use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::content_store::parsing::{
    find_closing_html_tag, parse_gallery_media_line, parse_open_tag, starts_with_html_tag,
};

use super::IssueMatch;

pub(super) fn lint_extension_contracts(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    lint_tabber_blocks(document, matches);
    lint_gallery_blocks(document, matches);
    lint_nonempty_body_blocks(document, "DPL", "extension.dpl_empty", matches);
    lint_nonempty_body_blocks(
        document,
        "categorytree",
        "extension.categorytree_empty",
        matches,
    );
}

fn lint_tabber_blocks(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
    for block in extension_blocks(&document.content, "tabber") {
        let body = &document.content[block.body_start..block.body_end];
        let mut separator_count = 0usize;
        for (line_offset, line) in body.lines().enumerate() {
            let trimmed = line.trim();
            if !trimmed.starts_with("|-|") {
                continue;
            }
            separator_count += 1;
            let label = trimmed
                .strip_prefix("|-|")
                .and_then(|rest| rest.split_once('='))
                .map(|(label, _)| label.trim())
                .unwrap_or_default();
            if !label.is_empty() {
                continue;
            }
            let line_start = block.body_start + line_start_offset(body, line_offset);
            let trimmed_start = line_start + line.len().saturating_sub(line.trim_start().len());
            matches.push(extension_error(
                document,
                ExtensionIssueSpec {
                    rule_id: "extension.tabber_empty_label",
                    message: "Tabber separator is missing a tab label before `=`.",
                    start: trimmed_start,
                    len: trimmed.len(),
                    evidence: trimmed,
                    remediation:
                        "Use TabberNeue separator syntax like `|-|Label=` with a non-empty label.",
                },
            ));
        }
        if separator_count == 0 {
            matches.push(extension_error(
                document,
                ExtensionIssueSpec {
                    rule_id: "extension.tabber_missing_separator",
                    message: "Tabber block does not contain any `|-|Label=` separator.",
                    start: block.open_start,
                    len: "<tabber>".len(),
                    evidence: "<tabber>",
                    remediation:
                        "Add at least one TabberNeue separator line, or remove the tabber block.",
                },
            ));
        }
    }
}

fn lint_gallery_blocks(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
    for block in extension_blocks(&document.content, "gallery") {
        let body = &document.content[block.body_start..block.body_end];
        if body.lines().any(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && parse_gallery_media_line(None, trimmed, &[]).is_some()
        }) {
            continue;
        }
        matches.push(extension_error(
            document,
            ExtensionIssueSpec {
                rule_id: "extension.gallery_empty",
                message: "Gallery block does not contain any parseable File: entries.",
                start: block.open_start,
                len: "<gallery>".len(),
                evidence: "<gallery>",
                remediation:
                    "Add one file title per line, for example `File:Example.png|Caption`, or remove the gallery block.",
            },
        ));
    }
}

fn lint_nonempty_body_blocks(
    document: &ParsedArticleDocument,
    tag: &str,
    rule_id: &str,
    matches: &mut Vec<IssueMatch>,
) {
    for block in extension_blocks(&document.content, tag) {
        let body = &document.content[block.body_start..block.body_end];
        if body.lines().any(|line| !line.trim().is_empty()) {
            continue;
        }
        let evidence = format!("<{tag}>");
        matches.push(extension_error(
            document,
            ExtensionIssueSpec {
                rule_id,
                message: "Extension block is empty.",
                start: block.open_start,
                len: evidence.len(),
                evidence: &evidence,
                remediation:
                    "Add the extension body required by this tag, or remove the empty block.",
            },
        ));
    }
}

#[derive(Debug, Clone)]
struct ExtensionBlock {
    open_start: usize,
    body_start: usize,
    body_end: usize,
}

fn extension_blocks(content: &str, tag: &str) -> Vec<ExtensionBlock> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if !starts_with_html_tag(bytes, cursor, tag) {
            cursor += 1;
            continue;
        }
        let Some((tag_end, tag_body, self_closing)) = parse_open_tag(content, cursor, tag) else {
            cursor += 1;
            continue;
        };
        if self_closing {
            cursor = tag_end;
            continue;
        }
        let Some((close_start, close_end)) = find_closing_html_tag(content, tag_end, tag) else {
            cursor = tag_end;
            continue;
        };
        if !tag_body.trim_start().starts_with('/') {
            out.push(ExtensionBlock {
                open_start: cursor,
                body_start: tag_end,
                body_end: close_start,
            });
        }
        cursor = close_end;
    }
    out
}

struct ExtensionIssueSpec<'a> {
    rule_id: &'a str,
    message: &'a str,
    start: usize,
    len: usize,
    evidence: &'a str,
    remediation: &'a str,
}

fn extension_error(document: &ParsedArticleDocument, spec: ExtensionIssueSpec<'_>) -> IssueMatch {
    IssueMatch {
        issue: ArticleLintIssue {
            rule_id: spec.rule_id.to_string(),
            severity: ArticleLintSeverity::Error,
            message: spec.message.to_string(),
            span: document.span_for_range(spec.start, spec.start.saturating_add(spec.len)),
            evidence: Some(spec.evidence.to_string()),
            suggested_remediation: Some(spec.remediation.to_string()),
            suggested_fixes: Vec::new(),
        },
        safe_fixes: Vec::new(),
    }
}

fn line_start_offset(body: &str, target_line: usize) -> usize {
    let mut offset = 0usize;
    for (index, segment) in body.split_inclusive('\n').enumerate() {
        if index == target_line {
            return offset;
        }
        offset += segment.len();
    }
    offset
}
