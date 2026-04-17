use std::collections::BTreeSet;

use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::content_store::parsing::{
    normalize_template_parameter_key, split_once_top_level_equals, split_template_segments,
};
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
    lint_d3chart_invocation_contracts(document, matches);
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

fn lint_d3chart_invocation_contracts(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    for invocation in &document.module_invocations {
        if normalize_module_title(&invocation.module_title) != "Module:D3Chart" {
            continue;
        }
        let args = parse_module_args(&invocation.raw_wikitext);
        let chart_type = resolve_d3chart_type(&invocation.function_name, &args);
        if !is_known_d3chart_type(&chart_type) {
            matches.push(module_error(
                document,
                "module.d3chart_unknown_type",
                "D3Chart invocation uses an unknown chart type.",
                invocation.start,
                invocation.end.saturating_sub(invocation.start),
                &invocation.raw_wikitext,
                "Use one of `bar`, `hbar`, `line`, `pie`, `donut`, `scatter`, or `area`.",
            ));
            continue;
        }

        let has_manual_data = args
            .value("data")
            .is_some_and(|value| !value.trim().is_empty());
        let has_cargo_source = args
            .value("table")
            .or_else(|| args.value("tables"))
            .is_some_and(|value| !value.trim().is_empty());
        if !has_manual_data && !has_cargo_source {
            matches.push(module_error(
                document,
                "module.d3chart_missing_data_source",
                "D3Chart invocation has no data source.",
                invocation.start,
                invocation.end.saturating_sub(invocation.start),
                &invocation.raw_wikitext,
                "Add `data=` for manual chart data or `table=`/`tables=` for a Cargo query.",
            ));
            continue;
        }

        if let Some(data) = args.value("data")
            && let Some(invalid_pair) = invalid_d3chart_data_pair(data, &chart_type)
        {
            matches.push(module_error(
                document,
                "module.d3chart_invalid_data",
                "D3Chart manual data does not match the chart parser's expected colon-separated shape.",
                invocation.start,
                invocation.end.saturating_sub(invocation.start),
                invalid_pair,
                "Use `label:value` pairs for non-scatter charts, and `x:y` or `label:x:y` pairs for scatter charts.",
            ));
        }
    }
}

#[derive(Debug, Default)]
struct ModuleArgs {
    positional: Vec<String>,
    named: Vec<(String, String)>,
}

impl ModuleArgs {
    fn value(&self, key: &str) -> Option<&str> {
        self.named
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }
}

fn parse_module_args(raw_wikitext: &str) -> ModuleArgs {
    let inner = raw_wikitext
        .trim()
        .strip_prefix("{{")
        .and_then(|value| value.strip_suffix("}}"))
        .unwrap_or(raw_wikitext);
    let segments = split_template_segments(inner);
    let mut args = ModuleArgs::default();
    for segment in segments.into_iter().skip(2) {
        let value = segment.trim();
        if value.is_empty() {
            continue;
        }
        if let Some((key, value)) = split_once_top_level_equals(value) {
            let key = normalize_template_parameter_key(&key);
            if !key.is_empty() {
                args.named.push((key, value.trim().to_string()));
                continue;
            }
        }
        args.positional.push(value.to_string());
    }
    args
}

fn resolve_d3chart_type(function_name: &str, args: &ModuleArgs) -> String {
    let function_name = function_name.trim().to_ascii_lowercase();
    if function_name != "chart" {
        return function_name;
    }
    args.value("type")
        .or_else(|| args.positional.first().map(String::as_str))
        .unwrap_or("bar")
        .trim()
        .to_ascii_lowercase()
}

fn is_known_d3chart_type(value: &str) -> bool {
    matches!(
        value,
        "bar" | "hbar" | "line" | "pie" | "donut" | "scatter" | "area"
    )
}

fn invalid_d3chart_data_pair<'a>(data: &'a str, chart_type: &str) -> Option<&'a str> {
    for pair in data
        .split(',')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
    {
        let colon_count = pair.chars().filter(|value| *value == ':').count();
        if chart_type == "scatter" {
            if colon_count == 1 || colon_count == 2 {
                continue;
            }
        } else if colon_count == 1 {
            continue;
        }
        return Some(pair);
    }
    None
}

fn module_error(
    document: &ParsedArticleDocument,
    rule_id: &str,
    message: &str,
    start: usize,
    len: usize,
    evidence: &str,
    remediation: &str,
) -> IssueMatch {
    IssueMatch {
        issue: ArticleLintIssue {
            rule_id: rule_id.to_string(),
            severity: ArticleLintSeverity::Error,
            message: message.to_string(),
            span: document.span_for_range(start, start.saturating_add(len)),
            evidence: Some(evidence.to_string()),
            suggested_remediation: Some(remediation.to_string()),
            suggested_fixes: Vec::new(),
        },
        safe_fixes: Vec::new(),
    }
}
