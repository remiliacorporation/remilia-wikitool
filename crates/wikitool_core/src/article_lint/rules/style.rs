use std::collections::BTreeMap;

use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::fix::TextEdit;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};

use super::common::{safe_fix_for_edit, straight_quote_for};
use super::{IssueMatch, SafeFixEdit};
use crate::article_lint::resources::LoadedResources;

const MOJIBAKE_REPLACEMENTS: &[(&str, &str)] = &[
    ("\u{00e2}\u{20ac}\u{201d}", "—"),
    ("\u{00e2}\u{20ac}\u{201c}", "–"),
    ("\u{00e2}\u{20ac}\u{0153}", "“"),
    ("\u{00e2}\u{20ac}\u{009d}", "”"),
    ("\u{00e2}\u{20ac}\u{02dc}", "‘"),
    ("\u{00e2}\u{20ac}\u{2122}", "’"),
    ("\u{00c2}\u{00a0}", " "),
];

pub(super) fn lint_mojibake(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
    let mut grouped = BTreeMap::<usize, Vec<TextEdit>>::new();
    for (encoded, decoded) in MOJIBAKE_REPLACEMENTS {
        for (start, _) in document.content.match_indices(encoded) {
            let Some(line) = document.line_for_offset(start) else {
                continue;
            };
            grouped.entry(line.number).or_default().push(TextEdit {
                start,
                end: start + encoded.len(),
                replacement: (*decoded).to_string(),
            });
        }
    }

    for (start, _) in document.content.match_indices('\u{fffd}') {
        let Some(line) = document.line_for_offset(start) else {
            continue;
        };
        grouped.entry(line.number).or_default();
    }

    for (line_number, mut edits) in grouped {
        let Some(line) = document
            .lines
            .iter()
            .find(|candidate| candidate.number == line_number)
        else {
            continue;
        };
        edits.sort_by_key(|edit| edit.start);
        edits.dedup_by(|left, right| left.start == right.start && left.end == right.end);

        let mut safe_fixes = Vec::new();
        let mut suggested_fixes = Vec::new();
        for edit in edits {
            safe_fixes.push(SafeFixEdit {
                rule_id: "style.mojibake".to_string(),
                label: "Repair misdecoded UTF-8 text".to_string(),
                line: Some(line.number),
                edit: edit.clone(),
            });
            suggested_fixes.push(safe_fix_for_edit(
                document,
                &edit,
                "Repair misdecoded UTF-8 text",
            ));
        }

        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "style.mojibake".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "Article contains text decoded with the wrong character encoding."
                    .to_string(),
                span: document.span_for_line(line),
                evidence: Some(line.text.clone()),
                suggested_remediation: Some(
                    "Restore the intended Unicode text before publishing; do not copy the corrupted characters forward."
                        .to_string(),
                ),
                suggested_fixes,
            },
            safe_fixes,
        });
    }
}

pub(super) fn lint_curly_quotes(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if !resources.profile.lint.forbid_curly_quotes {
        return;
    }
    let mut grouped = BTreeMap::<usize, Vec<(usize, char)>>::new();
    for (offset, ch) in document.content.char_indices() {
        if !matches!(ch, '“' | '”' | '‘' | '’') {
            continue;
        }
        if is_inside_known_mojibake(&document.content, offset) {
            continue;
        }
        if let Some(line) = document.line_for_offset(offset) {
            grouped.entry(line.number).or_default().push((offset, ch));
        }
    }

    for (line_number, replacements) in grouped {
        let Some(line) = document
            .lines
            .iter()
            .find(|candidate| candidate.number == line_number)
        else {
            continue;
        };
        let mut safe_fixes = Vec::new();
        let mut suggested_fixes = Vec::new();
        for (offset, ch) in replacements {
            let replacement = straight_quote_for(ch);
            let edit = TextEdit {
                start: offset,
                end: offset + ch.len_utf8(),
                replacement: replacement.to_string(),
            };
            safe_fixes.push(SafeFixEdit {
                rule_id: "style.curly_quotes".to_string(),
                label: "Replace curly quotes with straight quotes".to_string(),
                line: Some(line.number),
                edit: edit.clone(),
            });
            suggested_fixes.push(safe_fix_for_edit(
                document,
                &edit,
                "Replace curly quotes with straight quotes",
            ));
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "style.curly_quotes".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Article contains curly quotes or apostrophes.".to_string(),
                span: document.span_for_line(line),
                evidence: Some(line.text.clone()),
                suggested_remediation: Some(
                    "Use straight ASCII quotes in article prose and citations.".to_string(),
                ),
                suggested_fixes,
            },
            safe_fixes,
        });
    }
}

fn is_inside_known_mojibake(content: &str, offset: usize) -> bool {
    MOJIBAKE_REPLACEMENTS.iter().any(|(encoded, _)| {
        content
            .match_indices(encoded)
            .any(|(start, _)| offset >= start && offset < start + encoded.len())
    })
}

pub(super) fn lint_placeholder_fragments(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let lowered = document.content.to_ascii_lowercase();
    for fragment in &resources.profile.lint.forbid_placeholder_fragments {
        let lowered_fragment = fragment.to_ascii_lowercase();
        let Some(start) = lowered.find(&lowered_fragment) else {
            continue;
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "style.placeholder_fragment".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "Draft still contains placeholder or system-artifact text.".to_string(),
                span: document.span_for_range(start, start + fragment.len()),
                evidence: Some(fragment.clone()),
                suggested_remediation: Some(
                    "Delete placeholder text and replace it with sourced article content."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}
