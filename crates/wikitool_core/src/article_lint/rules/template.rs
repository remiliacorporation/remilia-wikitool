use std::collections::BTreeSet;

use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::profile::{
    TemplateCatalogEntry, TemplateCatalogEntryLookup, find_template_catalog_entry,
    unknown_template_parameter_keys,
};

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

pub(super) fn lint_template_availability(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let Some(catalog) = resources.template_catalog.as_ref() else {
        return;
    };
    let mut seen_missing = BTreeSet::new();
    for template in &document.templates {
        if let TemplateCatalogEntryLookup::Found(entry) =
            find_template_catalog_entry(catalog, &template.template_title)
        {
            lint_template_parameters(document, template, &entry, matches);
            continue;
        }
        if !seen_missing.insert(template.template_title.to_ascii_lowercase()) {
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

fn lint_template_parameters(
    document: &ParsedArticleDocument,
    template: &crate::article_lint::document::TemplateOccurrence,
    entry: &TemplateCatalogEntry,
    matches: &mut Vec<IssueMatch>,
) {
    let unknown = unknown_template_parameter_keys(entry, &template.parameter_keys);
    if unknown.is_empty() {
        return;
    }

    matches.push(IssueMatch {
        issue: ArticleLintIssue {
            rule_id: "template.unknown_parameter".to_string(),
            severity: ArticleLintSeverity::Warning,
            message: "Template invocation uses parameters that are not present in the current TemplateData-backed catalog."
                .to_string(),
            span: document.span_for_range(template.start, template.end),
            evidence: Some(format!(
                "{} unknown_parameters={}",
                template.template_title,
                unknown.join(", ")
            )),
            suggested_remediation: Some(
                "Run `wikitool templates show` for the template and either use a documented parameter, update TemplateData/source documentation, or remove the stray parameter."
                    .to_string(),
            ),
            suggested_fixes: Vec::new(),
        },
        safe_fixes: Vec::new(),
    });
}
