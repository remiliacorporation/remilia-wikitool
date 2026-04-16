use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::fix::TextEdit;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::content_store::parsing::make_content_preview;

use super::common::safe_fix_for_edit;
use super::{IssueMatch, SafeFixEdit};

pub(super) fn lint_citation_after_punctuation(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    let mut index = 0usize;
    while index < document.references.len() {
        let cluster_start = index;
        let mut cluster_end = index;
        while cluster_end + 1 < document.references.len() {
            let current = &document.references[cluster_end];
            let next = &document.references[cluster_end + 1];
            if !document.content[current.end..next.start]
                .chars()
                .all(char::is_whitespace)
            {
                break;
            }
            cluster_end += 1;
        }

        let first_reference = &document.references[cluster_start];
        let last_reference = &document.references[cluster_end];
        let Some(punctuation) = document.content[last_reference.end..]
            .chars()
            .next()
            .filter(|ch| matches!(ch, '.' | ',' | ';' | ':' | '!' | '?'))
        else {
            index = cluster_end + 1;
            continue;
        };
        let punctuation_end = last_reference.end + punctuation.len_utf8();
        let cluster_text = &document.content[first_reference.start..last_reference.end];
        let edit = TextEdit {
            start: first_reference.start,
            end: punctuation_end,
            replacement: format!("{punctuation}{cluster_text}"),
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "citation.after_punctuation".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Inline citations should come after punctuation, not before it."
                    .to_string(),
                span: document.span_for_range(first_reference.start, punctuation_end),
                evidence: Some(make_content_preview(
                    &document.content[first_reference.start..punctuation_end],
                    96,
                )),
                suggested_remediation: Some(
                    "Move the punctuation mark so it appears before the reference tag.".to_string(),
                ),
                suggested_fixes: vec![safe_fix_for_edit(
                    document,
                    &edit,
                    "Move punctuation before reference tag",
                )],
            },
            safe_fixes: vec![SafeFixEdit {
                rule_id: "citation.after_punctuation".to_string(),
                label: "Move punctuation before reference tag".to_string(),
                line: document
                    .line_for_offset(first_reference.start)
                    .map(|line| line.number),
                edit,
            }],
        });
        index = cluster_end + 1;
    }
}
