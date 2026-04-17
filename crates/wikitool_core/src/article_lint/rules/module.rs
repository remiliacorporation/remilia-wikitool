use std::collections::BTreeSet;

use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::profile::{normalize_module_title, supports_invoke_function};

use super::IssueMatch;
use crate::article_lint::resources::LoadedResources;

pub(super) fn lint_module_invocations(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if document.module_invocations.is_empty() {
        return;
    }
    lint_invoke_capability(document, resources, matches);
    lint_module_availability(document, resources, matches);
    lint_module_function_availability(document, resources, matches);
}

fn lint_invoke_capability(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let Some(capabilities) = resources.capabilities.as_ref() else {
        return;
    };
    if supports_invoke_function(capabilities) {
        return;
    }

    let Some(first) = document.module_invocations.first() else {
        return;
    };
    matches.push(IssueMatch {
        issue: ArticleLintIssue {
            rule_id: "capability.scribunto_unsupported".to_string(),
            severity: ArticleLintSeverity::Error,
            message:
                "Draft uses Scribunto #invoke, but the last synced wiki capability manifest does not show Scribunto or the invoke parser function."
                    .to_string(),
            span: document.span_for_range(first.start, first.end),
            evidence: Some(first.raw_wikitext.clone()),
            suggested_remediation: Some(
                "Run `wikitool wiki capabilities sync` if the live wiki changed; otherwise remove the #invoke usage or enable Scribunto on the target wiki."
                    .to_string(),
            ),
            suggested_fixes: Vec::new(),
        },
        safe_fixes: Vec::new(),
    });
}

fn lint_module_availability(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let known_modules = known_module_titles(resources);
    let mut seen_missing = BTreeSet::new();
    for invocation in &document.module_invocations {
        let normalized = normalize_module_title(&invocation.module_title);
        if normalized.is_empty() || known_modules.contains(&normalized) {
            continue;
        }
        if !seen_missing.insert(normalized.clone()) {
            continue;
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "module.unavailable".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "Draft invokes a module that is not available on the local wiki surface."
                    .to_string(),
                span: document.span_for_range(invocation.start, invocation.end),
                evidence: Some(format!(
                    "{} function={} parameters={}",
                    invocation.module_title,
                    invocation.function_name,
                    if invocation.parameter_keys.is_empty() {
                        "<none>".to_string()
                    } else {
                        invocation.parameter_keys.join(", ")
                    }
                )),
                suggested_remediation: Some(
                    "Use a local Module: page from `wikitool wiki surface show`, add/sync the module source, or replace the direct #invoke with an available template."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}

fn lint_module_function_availability(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let mut seen_missing = BTreeSet::new();
    for invocation in &document.module_invocations {
        let normalized = normalize_module_title(&invocation.module_title);
        if normalized.is_empty() {
            continue;
        }
        let Some(functions) = resources.local_module_functions.get(&normalized) else {
            continue;
        };
        if functions.contains(&invocation.function_name) {
            continue;
        }
        let key = format!("{normalized}\0{}", invocation.function_name);
        if !seen_missing.insert(key) {
            continue;
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "module.unavailable_function".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "Draft invokes a module function that is not exported by the local module source."
                    .to_string(),
                span: document.span_for_range(invocation.start, invocation.end),
                evidence: Some(format!(
                    "{} function={} available_functions={}",
                    invocation.module_title,
                    invocation.function_name,
                    if functions.is_empty() {
                        "<unknown>".to_string()
                    } else {
                        functions.iter().cloned().collect::<Vec<_>>().join(", ")
                    }
                )),
                suggested_remediation: Some(
                    "Use a function exported by the local Module: source, or sync/update the module before invoking it."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}

fn known_module_titles(resources: &LoadedResources) -> BTreeSet<String> {
    let mut titles = resources
        .local_module_titles
        .iter()
        .map(|title| normalize_module_title(title))
        .filter(|title| !title.is_empty())
        .collect::<BTreeSet<_>>();
    if let Some(catalog) = resources.template_catalog.as_ref() {
        for entry in &catalog.entries {
            for module_title in &entry.module_titles {
                let normalized = normalize_module_title(module_title);
                if !normalized.is_empty() {
                    titles.insert(normalized);
                }
            }
        }
    }
    titles
}
