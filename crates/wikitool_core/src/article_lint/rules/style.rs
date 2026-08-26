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

pub(super) fn lint_curly_quotes(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
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
    for fragment in &resources.overlay.lint.forbid_placeholder_fragments {
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

pub(super) fn lint_synthetic_phrase_prompts(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    for line in &document.lines {
        let lowered_line = line.text.to_ascii_lowercase();
        for phrase in &resources.overlay.lint.synthetic_phrase_prompts {
            let lowered_phrase = phrase.to_ascii_lowercase();
            let Some(relative_start) = lowered_line.find(&lowered_phrase) else {
                continue;
            };
            let start = line.start + relative_start;
            matches.push(IssueMatch {
                issue: ArticleLintIssue {
                    rule_id: "style.synthetic_phrase".to_string(),
                    severity: ArticleLintSeverity::Suggestion,
                    message: "Article prose matches a profile phrase associated with synthetic or inflated writing."
                        .to_string(),
                    span: document.span_for_range(start, start + lowered_phrase.len()),
                    evidence: Some(phrase.clone()),
                    suggested_remediation: Some(
                        "Have a human editor decide whether the sentence states a concrete, sourced fact in natural language; do not replace the phrase mechanically."
                            .to_string(),
                    ),
                    suggested_fixes: Vec::new(),
                },
                safe_fixes: Vec::new(),
            });
        }
    }
}

pub(super) fn lint_discouraged_relationship_headings(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    for section in &document.sections {
        let Some(heading) = &section.heading else {
            continue;
        };
        let Some(configured_heading) = resources
            .overlay
            .lint
            .discouraged_relationship_headings
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(&heading.text))
        else {
            continue;
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "editorial.forced_relationship_frame".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Relationship heading may force an adjacent subject into the wiki owner's frame."
                    .to_string(),
                span: document.span_for_range(heading.start, heading.end),
                evidence: Some(configured_heading.clone()),
                suggested_remediation: Some(
                    "A human editor must confirm that this relationship is subject-defining and proportionate. Otherwise integrate only the relevant sourced fact where it belongs or remove the section."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}

pub(super) fn lint_discouraged_lead_relationship_terms(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let lead_end = document
        .sections
        .iter()
        .filter_map(|section| section.heading.as_ref().map(|heading| heading.start))
        .min()
        .unwrap_or(document.content.len());
    let lead = &document.content[..lead_end];
    let lowered_lead = lead.to_ascii_lowercase();
    let lowered_title = document.title.to_ascii_lowercase();
    for term in &resources.overlay.lint.discouraged_lead_relationship_terms {
        let lowered_term = term.to_ascii_lowercase();
        if lowered_term.is_empty() || lowered_title.contains(&lowered_term) {
            continue;
        }
        let Some(start) = lowered_lead.find(&lowered_term) else {
            continue;
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "editorial.lead_relationship_frame".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "The lead frames this subject through a profile-owner or adjacent-person relationship."
                    .to_string(),
                span: document.span_for_range(start, start + term.len()),
                evidence: Some(term.clone()),
                suggested_remediation: Some(
                    "A human editor must confirm that the relationship is necessary to define the subject and receives proportionate weight. Otherwise define the subject directly and move or remove the relationship claim."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}
