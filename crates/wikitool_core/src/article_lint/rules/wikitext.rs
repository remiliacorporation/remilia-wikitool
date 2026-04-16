use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::content_store::parsing::make_content_preview;
use crate::wikitext::lint::{WikitextLintIssue, lint_wikitext};

use super::IssueMatch;

pub(super) fn lint_raw_wikitext_balance(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    for issue in lint_wikitext(&document.content) {
        matches.push(wikitext_balance_issue(document, &issue));
    }
}

fn wikitext_balance_issue(
    document: &ParsedArticleDocument,
    issue: &WikitextLintIssue,
) -> IssueMatch {
    IssueMatch {
        issue: ArticleLintIssue {
            rule_id: format!("wikitext.{}", issue.rule_id),
            severity: ArticleLintSeverity::Error,
            message: issue.message.clone(),
            span: document.span_for_range(issue.byte_offset, issue.byte_offset.saturating_add(1)),
            evidence: Some(make_content_preview(
                &document.content[issue.byte_offset.min(document.content.len())..],
                96,
            )),
            suggested_remediation: Some(
                "Repair the raw wikitext balance at the reported location before revising article prose."
                    .to_string(),
            ),
            suggested_fixes: Vec::new(),
        },
        safe_fixes: Vec::new(),
    }
}
