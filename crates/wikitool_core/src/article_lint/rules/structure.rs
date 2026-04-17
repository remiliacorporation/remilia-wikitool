use std::collections::BTreeSet;

use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::fix::TextEdit;
use crate::article_lint::model::{
    ArticleLintIssue, ArticleLintSeverity, SuggestedFix, SuggestedFixKind,
};
use crate::content_store::parsing::{make_content_preview, normalize_spaces, parse_heading_line};
use crate::filesystem::Namespace;

use super::common::{
    canonical_sentence_case_heading, line_has_short_description, parse_markdown_heading,
    preferred_short_description_snippet, safe_fix_for_edit, safe_heading_rewrite_available,
    section_body_contains_template,
};
use super::{IssueMatch, SafeFixEdit};
use crate::article_lint::resources::LoadedResources;

pub(super) fn lint_missing_short_description(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if document.namespace != Namespace::Main.as_str() || document.is_redirect {
        return;
    }
    if !resources.overlay.authoring.require_short_description {
        return;
    }
    if document
        .top_nonblank_lines(6)
        .iter()
        .any(|line| line_has_short_description(&line.text))
    {
        return;
    }

    let first_line = document.first_nonblank_line();
    matches.push(IssueMatch {
        issue: ArticleLintIssue {
            rule_id: "structure.require_short_description".to_string(),
            severity: ArticleLintSeverity::Error,
            message: "Article is missing the required short description header.".to_string(),
            span: first_line.and_then(|line| document.span_for_line(line)),
            evidence: first_line.map(|line| make_content_preview(&line.text, 96)),
            suggested_remediation: Some(
                "Insert the required short description at the top of the article before the quality banner.".to_string(),
            ),
            suggested_fixes: vec![SuggestedFix {
                label: "Insert short description header".to_string(),
                kind: SuggestedFixKind::AssistedFix,
                replacement_preview: Some(preferred_short_description_snippet(&resources.overlay)),
                patch: None,
            }],
        },
        safe_fixes: Vec::new(),
    });
}

pub(super) fn lint_article_quality_banner(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if document.namespace != Namespace::Main.as_str() || document.is_redirect {
        return;
    }
    if !resources.overlay.authoring.require_article_quality_banner {
        return;
    }

    let top_lines = document.top_nonblank_lines(8);
    let banner_line = top_lines
        .iter()
        .find(|line| line.text.trim_start().starts_with("{{Article quality|"));
    if banner_line.is_none() {
        let insertion_offset = top_lines
            .iter()
            .find(|line| line_has_short_description(&line.text))
            .map(|line| line.end)
            .or_else(|| document.first_nonblank_line().map(|line| line.start))
            .unwrap_or(0);
        let replacement = if top_lines
            .iter()
            .any(|line| line_has_short_description(&line.text))
        {
            "\n{{Article quality|unverified}}".to_string()
        } else {
            "{{Article quality|unverified}}\n".to_string()
        };
        let edit = TextEdit {
            start: insertion_offset,
            end: insertion_offset,
            replacement,
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "structure.require_article_quality_banner".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "AI-generated drafts should include the article quality banner near the top.".to_string(),
                span: document.first_nonblank_line().and_then(|line| document.span_for_line(line)),
                evidence: document.first_nonblank_line().map(|line| make_content_preview(&line.text, 96)),
                suggested_remediation: Some(
                    "Insert {{Article quality|unverified}} immediately below the short description.".to_string(),
                ),
                suggested_fixes: vec![safe_fix_for_edit(
                    document,
                    &edit,
                    "Insert article quality banner",
                )],
            },
            safe_fixes: vec![SafeFixEdit {
                rule_id: "structure.require_article_quality_banner".to_string(),
                label: "Insert article quality banner".to_string(),
                line: document.first_nonblank_line().map(|line| line.number),
                edit,
            }],
        });
        return;
    }

    if let Some(expected_state) = resources
        .overlay
        .authoring
        .article_quality_default_state
        .as_deref()
        && let Some(line) = banner_line
        && !line
            .text
            .to_ascii_lowercase()
            .contains(&format!("|{}", expected_state.to_ascii_lowercase()))
    {
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "structure.article_quality_state".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Article quality banner is using a non-default state for an AI-authored draft.".to_string(),
                span: document.span_for_line(line),
                evidence: Some(line.text.trim().to_string()),
                suggested_remediation: Some(
                    "Use {{Article quality|unverified}} until a human editor changes the review state.".to_string(),
                ),
                suggested_fixes: vec![SuggestedFix {
                    label: "Normalize article quality state".to_string(),
                    kind: SuggestedFixKind::AssistedFix,
                    replacement_preview: Some("{{Article quality|unverified}}".to_string()),
                    patch: None,
                }],
            },
            safe_fixes: Vec::new(),
        });
    }
}

pub(super) fn lint_markdown_headings(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    for line in &document.lines {
        let Some((level, text)) = parse_markdown_heading(&line.text) else {
            continue;
        };
        let replacement = format!(
            "{} {} {}",
            "=".repeat(level as usize),
            text,
            "=".repeat(level as usize)
        );
        let edit = TextEdit {
            start: line.start,
            end: line.end,
            replacement: replacement.clone(),
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "structure.markdown_heading".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "Markdown heading syntax is not valid article wikitext.".to_string(),
                span: document.span_for_line(line),
                evidence: Some(line.text.trim().to_string()),
                suggested_remediation: Some(
                    "Replace Markdown headings with MediaWiki heading markup.".to_string(),
                ),
                suggested_fixes: vec![safe_fix_for_edit(
                    document,
                    &edit,
                    "Convert Markdown heading to MediaWiki heading",
                )],
            },
            safe_fixes: vec![SafeFixEdit {
                rule_id: "structure.markdown_heading".to_string(),
                label: "Convert Markdown heading to MediaWiki heading".to_string(),
                line: Some(line.number),
                edit,
            }],
        });
    }
}

pub(super) fn lint_malformed_headings(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    for line in &document.lines {
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_tabber_separator_line(trimmed) {
            continue;
        }
        if trimmed.starts_with('|') {
            continue;
        }
        if (trimmed.starts_with('=') || trimmed.ends_with('='))
            && parse_heading_line(trimmed).is_none()
        {
            matches.push(IssueMatch {
                issue: ArticleLintIssue {
                    rule_id: "structure.malformed_heading".to_string(),
                    severity: ArticleLintSeverity::Error,
                    message: "Heading markup is malformed.".to_string(),
                    span: document.span_for_line(line),
                    evidence: Some(trimmed.to_string()),
                    suggested_remediation: Some(
                        "Use balanced MediaWiki heading markers such as == Heading ==.".to_string(),
                    ),
                    suggested_fixes: Vec::new(),
                },
                safe_fixes: Vec::new(),
            });
        }
    }
}

fn is_tabber_separator_line(trimmed: &str) -> bool {
    trimmed.starts_with("|-|") && trimmed.contains('=')
}

pub(super) fn lint_duplicate_headings(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    let mut seen = BTreeSet::new();
    for section in &document.sections {
        let Some(heading) = &section.heading else {
            continue;
        };
        let normalized = normalize_spaces(&heading.text).to_ascii_lowercase();
        if normalized.is_empty() || seen.insert(normalized) {
            continue;
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "structure.duplicate_heading".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Article reuses the same heading more than once.".to_string(),
                span: document.span_for_range(heading.start, heading.end),
                evidence: Some(heading.text.clone()),
                suggested_remediation: Some(
                    "Merge duplicated sections or rename one heading so the structure is unambiguous.".to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}

pub(super) fn lint_sentence_case_headings(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    for section in &document.sections {
        let Some(heading) = &section.heading else {
            continue;
        };
        let Some(canonical) = canonical_sentence_case_heading(&heading.text) else {
            continue;
        };
        if canonical == heading.text {
            continue;
        }
        let mut safe_fixes = Vec::new();
        let mut suggested_fixes = Vec::new();
        if safe_heading_rewrite_available(&heading.text, &canonical) {
            let edit = TextEdit {
                start: heading.start,
                end: heading.end,
                replacement: format!(
                    "{} {} {}",
                    "=".repeat(heading.level as usize),
                    canonical,
                    "=".repeat(heading.level as usize)
                ),
            };
            suggested_fixes.push(safe_fix_for_edit(
                document,
                &edit,
                "Normalize heading to sentence case",
            ));
            safe_fixes.push(SafeFixEdit {
                rule_id: "style.sentence_case_heading".to_string(),
                label: "Normalize heading to sentence case".to_string(),
                line: Some(heading.line),
                edit,
            });
        } else {
            suggested_fixes.push(SuggestedFix {
                label: "Rewrite heading in sentence case".to_string(),
                kind: SuggestedFixKind::AssistedFix,
                replacement_preview: Some(canonical),
                patch: None,
            });
        }

        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "style.sentence_case_heading".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Headings should use sentence case.".to_string(),
                span: document.span_for_range(heading.start, heading.end),
                evidence: Some(heading.text.clone()),
                suggested_remediation: Some(
                    "Rewrite the heading with sentence case capitalization.".to_string(),
                ),
                suggested_fixes,
            },
            safe_fixes,
        });
    }
}

pub(super) fn lint_missing_references_section(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if document.namespace != Namespace::Main.as_str() || document.is_redirect {
        return;
    }
    if !resources
        .overlay
        .authoring
        .required_appendix_sections
        .iter()
        .any(|section| section.eq_ignore_ascii_case("References"))
    {
        return;
    }
    if document.find_section("References").is_some() {
        return;
    }

    let severity = if document.references.is_empty() {
        ArticleLintSeverity::Warning
    } else {
        ArticleLintSeverity::Error
    };
    matches.push(IssueMatch {
        issue: ArticleLintIssue {
            rule_id: "structure.require_references_section".to_string(),
            severity,
            message: "Article is missing the required References section.".to_string(),
            span: document
                .first_nonblank_line()
                .and_then(|line| document.span_for_line(line)),
            evidence: Some("== References ==".to_string()),
            suggested_remediation: Some(
                "Add a References section near the end of the article and render it with {{Reflist}}.".to_string(),
            ),
            suggested_fixes: vec![SuggestedFix {
                label: "Insert References section".to_string(),
                kind: SuggestedFixKind::AssistedFix,
                replacement_preview: Some("== References ==\n{{Reflist}}".to_string()),
                patch: None,
            }],
        },
        safe_fixes: Vec::new(),
    });
}

pub(super) fn lint_missing_reflist(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let Some(section) = document.find_section("References") else {
        return;
    };
    let Some(references_template) = resources.overlay.authoring.references_template.as_deref()
    else {
        return;
    };
    if section_body_contains_template(section, &document.templates, references_template) {
        return;
    }

    let edit = TextEdit {
        start: section.body_start,
        end: section.body_start,
        replacement: "{{Reflist}}\n".to_string(),
    };
    let safe_fixes = vec![SafeFixEdit {
        rule_id: "structure.require_reflist".to_string(),
        label: "Insert {{Reflist}} into References section".to_string(),
        line: section
            .heading
            .as_ref()
            .map(|heading| heading.line.saturating_add(1)),
        edit: edit.clone(),
    }];
    let suggested_fixes = vec![safe_fix_for_edit(
        document,
        &edit,
        "Insert {{Reflist}} into References section",
    )];

    matches.push(IssueMatch {
        issue: ArticleLintIssue {
            rule_id: "structure.require_reflist".to_string(),
            severity: ArticleLintSeverity::Error,
            message: "References section does not render citations with {{Reflist}}.".to_string(),
            span: section
                .heading
                .as_ref()
                .and_then(|heading| document.span_for_range(heading.start, heading.end)),
            evidence: Some(make_content_preview(&section.body_text, 96)),
            suggested_remediation: Some(
                "Render inline citations in the References section with {{Reflist}}.".to_string(),
            ),
            suggested_fixes,
        },
        safe_fixes,
    });
}
