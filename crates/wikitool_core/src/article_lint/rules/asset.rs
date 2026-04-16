use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::profile::normalize_asset_title;

use super::IssueMatch;
use crate::article_lint::resources::LoadedResources;

pub(super) fn lint_asset_references(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    lint_templatestyles_sources(document, resources, matches);
}

fn lint_templatestyles_sources(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    for occurrence in &document.template_styles {
        let Some(source_title) = occurrence.source_title.as_deref() else {
            matches.push(IssueMatch {
                issue: ArticleLintIssue {
                    rule_id: "asset.templatestyles_missing_src".to_string(),
                    severity: ArticleLintSeverity::Error,
                    message: "TemplateStyles tag is missing a src attribute.".to_string(),
                    span: document.span_for_range(occurrence.start, occurrence.end),
                    evidence: Some(occurrence.raw_tag.clone()),
                    suggested_remediation: Some(
                        "Provide a Template:, Module:, or bare template stylesheet title in the src attribute."
                            .to_string(),
                    ),
                    suggested_fixes: Vec::new(),
                },
                safe_fixes: Vec::new(),
            });
            continue;
        };
        let normalized = normalize_asset_title(source_title);
        if normalized.is_empty()
            || resources
                .local_asset_titles
                .contains(&normalized.to_ascii_lowercase())
        {
            continue;
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "asset.templatestyles_unavailable_source".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "TemplateStyles tag references a stylesheet that is not available on the local wiki surface."
                    .to_string(),
                span: document.span_for_range(occurrence.start, occurrence.end),
                evidence: Some(format!("{source_title} normalized={normalized}")),
                suggested_remediation: Some(
                    "Use a stylesheet listed by `wikitool wiki surface show`, add/sync the stylesheet page, or remove the TemplateStyles tag."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}
