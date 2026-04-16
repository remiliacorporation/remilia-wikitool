use std::collections::BTreeSet;

use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::model::{
    ArticleLintIssue, ArticleLintSeverity, SuggestedFix, SuggestedFixKind,
};
use crate::content_store::parsing::make_content_preview;
use crate::profile::{TemplateCatalogEntryLookup, find_template_catalog_entry};

use super::IssueMatch;
use crate::article_lint::resources::LoadedResources;

pub(super) fn lint_citation_needed(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    for template in &document.templates {
        if !template
            .template_title
            .eq_ignore_ascii_case("Template:Citation needed")
        {
            continue;
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "profile.no_citation_needed".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "AI-generated drafts should not ship with {{Citation needed}} markers."
                    .to_string(),
                span: document.span_for_range(template.start, template.end),
                evidence: Some(template.raw_wikitext.clone()),
                suggested_remediation: Some(
                    "Replace the marker with a real citation or remove the unsupported claim."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}

pub(super) fn lint_remilia_parent_group(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if resources.overlay.remilia.default_parent_group.is_none() {
        return;
    }
    for template in &document.templates {
        if !template
            .template_title
            .eq_ignore_ascii_case("Template:Infobox NFT collection")
        {
            continue;
        }
        let has_parent_group = template
            .parameter_keys
            .iter()
            .any(|key| key == "parent group" || key == "parent_group");
        let has_legacy_group = template
            .parameter_keys
            .iter()
            .any(|key| key == "creator" || key == "artist");
        if has_parent_group || !has_legacy_group {
            continue;
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "profile.remilia_parent_group".to_string(),
                severity: ArticleLintSeverity::Warning,
                message:
                    "Remilia NFT infoboxes should use parent_group instead of creator or artist."
                        .to_string(),
                span: document.span_for_range(template.start, template.end),
                evidence: Some(make_content_preview(&template.raw_wikitext, 120)),
                suggested_remediation: Some(
                    "Replace creator=/artist= with parent_group=Remilia in the infobox."
                        .to_string(),
                ),
                suggested_fixes: vec![SuggestedFix {
                    label: "Rename infobox field to parent_group".to_string(),
                    kind: SuggestedFixKind::AssistedFix,
                    replacement_preview: Some("| parent_group = Remilia".to_string()),
                    patch: None,
                }],
            },
            safe_fixes: Vec::new(),
        });
    }
}

pub(super) fn lint_template_availability(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let Some(catalog) = resources.template_catalog.as_ref() else {
        return;
    };
    let mut seen = BTreeSet::new();
    for template in &document.templates {
        if !seen.insert(template.template_title.to_ascii_lowercase()) {
            continue;
        }
        if matches!(
            find_template_catalog_entry(catalog, &template.template_title),
            TemplateCatalogEntryLookup::Found(_)
        ) {
            continue;
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "template.unavailable".to_string(),
                severity: ArticleLintSeverity::Error,
                message:
                    "Article references a template that is not available on the local wiki surface."
                        .to_string(),
                span: document.span_for_range(template.start, template.end),
                evidence: Some(template.template_title.clone()),
                suggested_remediation: Some(
                    "Use an available template from the local catalog or remove the invocation."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}
