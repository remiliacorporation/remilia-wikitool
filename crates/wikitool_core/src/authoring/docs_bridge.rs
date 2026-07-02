use std::collections::BTreeSet;

use anyhow::Result;

use crate::docs::{DocsContextOptions, build_docs_context};
use crate::knowledge::model::{
    AuthoringDocsContext, ModuleUsageSummary, StubTemplateHint, TemplateReference,
};
use crate::profile::{
    load_latest_wiki_capabilities, normalize_parser_function_name, normalize_parser_tag_name,
};
use crate::runtime::ResolvedPaths;

const AUTHORING_DOCS_QUERY_LIMIT: usize = 4;
const AUTHORING_DOCS_TOKEN_BUDGET: usize = 560;

pub(crate) fn build_authoring_docs_context(
    paths: &ResolvedPaths,
    template_references: &[TemplateReference],
    module_patterns: &[ModuleUsageSummary],
    stub_detected_templates: &[StubTemplateHint],
    stub_content: Option<&str>,
    docs_profile: &str,
) -> Result<Option<AuthoringDocsContext>> {
    let normalized_profile = normalize_spaces(&docs_profile.replace('_', " "));
    if normalized_profile.is_empty() {
        return Ok(None);
    }

    let stub_signals = collect_stub_docs_signals(paths, stub_content)?;
    let queries = build_authoring_docs_queries(
        template_references,
        module_patterns,
        stub_detected_templates,
        &stub_signals,
    );
    if queries.is_empty() {
        return Ok(None);
    }

    let mut pages = Vec::new();
    let mut sections = Vec::new();
    let mut symbols = Vec::new();
    let mut examples = Vec::new();
    let mut seen_pages = BTreeSet::new();
    let mut seen_sections = BTreeSet::new();
    let mut seen_symbols = BTreeSet::new();
    let mut seen_examples = BTreeSet::new();

    for query in &queries {
        let report = build_docs_context(
            paths,
            query,
            &DocsContextOptions {
                profile: Some(normalized_profile.clone()),
                limit: AUTHORING_DOCS_QUERY_LIMIT,
                token_budget: AUTHORING_DOCS_TOKEN_BUDGET,
            },
        )?;

        for page in report.pages {
            let key = format!(
                "{}|{}|{}|{}",
                page.corpus_id,
                page.tier,
                page.page_title,
                page.section_heading.as_deref().unwrap_or("")
            );
            if seen_pages.insert(key) {
                pages.push(page);
            }
        }
        for section in report.sections {
            let key = format!(
                "{}|{}|{}",
                section.corpus_id,
                section.page_title,
                section.section_heading.as_deref().unwrap_or("")
            );
            if seen_sections.insert(key) {
                sections.push(section);
            }
        }
        for symbol in report.symbols {
            let key = format!(
                "{}|{}|{}|{}",
                symbol.corpus_id, symbol.page_title, symbol.symbol_kind, symbol.symbol_name
            );
            if seen_symbols.insert(key) {
                symbols.push(symbol);
            }
        }
        for example in report.examples {
            let key = format!(
                "{}|{}|{}|{}",
                example.corpus_id,
                example.page_title,
                example.example_kind,
                example.section_heading.as_deref().unwrap_or("")
            );
            if seen_examples.insert(key) {
                examples.push(example);
            }
        }
    }

    pages.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.title.cmp(&right.title))
    });
    sections.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.page_title.cmp(&right.page_title))
    });
    symbols.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.symbol_name.cmp(&right.symbol_name))
    });
    examples.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.page_title.cmp(&right.page_title))
    });

    pages.truncate(AUTHORING_DOCS_QUERY_LIMIT * 2);
    sections.truncate(AUTHORING_DOCS_QUERY_LIMIT * 3);
    symbols.truncate(AUTHORING_DOCS_QUERY_LIMIT * 3);
    examples.truncate(AUTHORING_DOCS_QUERY_LIMIT * 2);

    if pages.is_empty() && sections.is_empty() && symbols.is_empty() && examples.is_empty() {
        return Ok(None);
    }

    let token_estimate_total = pages
        .iter()
        .map(|page| estimate_tokens(&page.snippet).max(1))
        .sum::<usize>()
        .saturating_add(
            sections
                .iter()
                .map(|section| section.token_estimate.max(1))
                .sum::<usize>(),
        )
        .saturating_add(
            symbols
                .iter()
                .map(|symbol| {
                    estimate_tokens(&format!("{} {}", symbol.summary_text, symbol.detail_text))
                        .max(1)
                })
                .sum::<usize>(),
        )
        .saturating_add(
            examples
                .iter()
                .map(|example| example.token_estimate.max(1))
                .sum::<usize>(),
        );

    Ok(Some(AuthoringDocsContext {
        profile: normalized_profile,
        queries,
        pages,
        sections,
        symbols,
        examples,
        token_estimate_total,
    }))
}

#[derive(Debug, Default, PartialEq, Eq)]
struct StubDocsSignals {
    /// Bare lowercase parser-function names (no leading `#`), in stub order.
    parser_functions: Vec<String>,
    /// Bare lowercase extension-tag names, in stub order.
    extension_tags: Vec<String>,
}

fn collect_stub_docs_signals(
    paths: &ResolvedPaths,
    stub_content: Option<&str>,
) -> Result<StubDocsSignals> {
    let Some(content) = stub_content.filter(|content| !content.trim().is_empty()) else {
        return Ok(StubDocsSignals::default());
    };
    let manifest = load_latest_wiki_capabilities(paths)?;
    let known_parser_functions = manifest
        .as_ref()
        .map(|manifest| {
            manifest
                .parser_function_hooks
                .iter()
                .map(|hook| normalize_parser_function_name(hook))
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let known_extension_tags = manifest
        .as_ref()
        .map(|manifest| {
            manifest
                .parser_extension_tags
                .iter()
                .map(|tag| normalize_parser_tag_name(tag))
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    Ok(scan_stub_docs_signals(
        content,
        &known_parser_functions,
        &known_extension_tags,
    ))
}

/// Character-scan the stub for `{{#name:` parser-function calls and `<name ...>` extension
/// tags. Parser-function syntax is self-identifying, so detections are kept even without a
/// capability manifest (the known set only filters when present). `<name` is ambiguous with
/// plain HTML, so tag detections require membership in the manifest's extension-tag set.
/// Tags every article uses do not merit one of the four capped docs-query
/// slots: their usage is baseline wiki literacy and their docs would crowd out
/// the tags an agent actually needs help with (cargo, tabber, DPL, math).
const UBIQUITOUS_EXTENSION_TAGS: &[&str] = &[
    "ref",
    "references",
    "nowiki",
    "pre",
    "includeonly",
    "noinclude",
    "onlyinclude",
];

fn scan_stub_docs_signals(
    content: &str,
    known_parser_functions: &BTreeSet<String>,
    known_extension_tags: &BTreeSet<String>,
) -> StubDocsSignals {
    let bytes = content.as_bytes();
    let mut signals = StubDocsSignals::default();
    let mut seen_functions = BTreeSet::new();
    let mut seen_tags = BTreeSet::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"{{#") {
            let start = cursor + 3;
            let mut end = start;
            while end < bytes.len() && is_stub_symbol_name_byte(bytes[end]) {
                end += 1;
            }
            if end > start && end < bytes.len() && bytes[end] == b':' {
                let name = content[start..end].to_ascii_lowercase();
                if (known_parser_functions.is_empty() || known_parser_functions.contains(&name))
                    && seen_functions.insert(name.clone())
                {
                    signals.parser_functions.push(name);
                }
            }
            cursor = end.max(cursor + 1);
            continue;
        }
        if bytes[cursor] == b'<' {
            let start = cursor + 1;
            let mut end = start;
            while end < bytes.len() && is_stub_symbol_name_byte(bytes[end]) {
                end += 1;
            }
            if end > start
                && end < bytes.len()
                && matches!(bytes[end], b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/')
            {
                let name = content[start..end].to_ascii_lowercase();
                if known_extension_tags.contains(&name)
                    && !UBIQUITOUS_EXTENSION_TAGS.contains(&name.as_str())
                    && seen_tags.insert(name.clone())
                {
                    signals.extension_tags.push(name);
                }
            }
            cursor = end.max(cursor + 1);
            continue;
        }
        cursor += 1;
    }
    signals
}

fn is_stub_symbol_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn build_authoring_docs_queries(
    template_references: &[TemplateReference],
    module_patterns: &[ModuleUsageSummary],
    stub_detected_templates: &[StubTemplateHint],
    stub_signals: &StubDocsSignals,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    // Stub-derived signals first: what the draft explicitly invokes is the direct
    // docs need. Pack-derived template references are indirect (suggested by
    // comparables) and must not crowd stub signals out of the query cap.
    // Signal names keep their underscores (`cargo_query`): the docs FTS index treats `_`
    // as a term character, and the docs corpus spells parser functions that way.
    for name in &stub_signals.parser_functions {
        push_authoring_docs_query_raw(&mut out, &mut seen, &format!("{name} parser function"));
    }
    for name in &stub_signals.extension_tags {
        push_authoring_docs_query_raw(&mut out, &mut seen, &format!("{name} extension tag"));
    }
    for template in stub_detected_templates {
        if let Some(tail) = template
            .template_title
            .split_once(':')
            .map(|(_, tail)| tail)
        {
            push_authoring_docs_query(&mut out, &mut seen, tail);
        }
    }
    if !stub_detected_templates.is_empty() {
        push_authoring_docs_query(&mut out, &mut seen, "template parameters");
    }
    for reference in template_references {
        if out.len() >= AUTHORING_DOCS_QUERY_LIMIT {
            break;
        }
        if let Some(tail) = reference
            .template
            .template_title
            .split_once(':')
            .map(|(_, tail)| tail)
        {
            push_authoring_docs_query(&mut out, &mut seen, tail);
        }
    }
    if !module_patterns.is_empty() {
        push_authoring_docs_query(&mut out, &mut seen, "Scribunto #invoke");
    }
    out.truncate(AUTHORING_DOCS_QUERY_LIMIT);
    out
}

fn push_authoring_docs_query(queries: &mut Vec<String>, seen: &mut BTreeSet<String>, value: &str) {
    push_authoring_docs_query_raw(queries, seen, &value.replace('_', " "));
}

fn push_authoring_docs_query_raw(
    queries: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    value: &str,
) {
    let normalized = normalize_spaces(value);
    if normalized.is_empty() {
        return;
    }
    let key = normalized.to_ascii_lowercase();
    if seen.insert(key) {
        queries.push(normalized);
    }
}

fn normalize_spaces(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
                previous_was_space = true;
            }
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }
    output.trim().to_string()
}

fn estimate_tokens(value: &str) -> usize {
    if value.trim().is_empty() {
        return 0;
    }
    value.len().div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_template(title: &str) -> StubTemplateHint {
        StubTemplateHint {
            template_title: title.to_string(),
            parameter_keys: Vec::new(),
        }
    }

    fn module_pattern(title: &str) -> ModuleUsageSummary {
        ModuleUsageSummary {
            module_title: title.to_string(),
            usage_count: 1,
            distinct_page_count: 1,
            function_stats: Vec::new(),
            example_pages: Vec::new(),
            example_invocations: Vec::new(),
        }
    }

    #[test]
    fn scan_detects_parser_functions_and_skips_ubiquitous_tags() {
        let known_functions = BTreeSet::from(["cargo_query".to_string(), "if".to_string()]);
        let known_tags = BTreeSet::from(["ref".to_string(), "tabber".to_string()]);
        let signals = scan_stub_docs_signals(
            "Intro.<ref name=\"a\">source</ref>\n{{#cargo_query:tables=Traits|fields=Name}}\n<div class=\"x\">{{#if:yes|then}}</div>\n<tabber>One=body</tabber>",
            &known_functions,
            &known_tags,
        );
        assert_eq!(signals.parser_functions, vec!["cargo_query", "if"]);
        // `<ref>` is manifest-known but ubiquitous; it must not claim a query slot.
        assert_eq!(signals.extension_tags, vec!["tabber"]);
    }

    #[test]
    fn scan_filters_parser_functions_by_known_set_only_when_present() {
        let known_tags = BTreeSet::new();
        let filtered = scan_stub_docs_signals(
            "{{#cargo_query:tables=Traits}} {{#unknown_fn:x}}",
            &BTreeSet::from(["cargo_query".to_string()]),
            &known_tags,
        );
        assert_eq!(filtered.parser_functions, vec!["cargo_query"]);

        let unfiltered = scan_stub_docs_signals(
            "{{#cargo_query:tables=Traits}} {{#unknown_fn:x}}",
            &BTreeSet::new(),
            &known_tags,
        );
        assert_eq!(
            unfiltered.parser_functions,
            vec!["cargo_query", "unknown_fn"]
        );
    }

    #[test]
    fn scan_ignores_tags_outside_known_set_and_non_invocations() {
        let signals = scan_stub_docs_signals(
            "<div>{{Infobox person|name=A}}</div> {{#cargo_query tables}} <ref>x</ref>",
            &BTreeSet::new(),
            &BTreeSet::from(["ref".to_string()]),
        );
        // `{{#cargo_query tables}}` lacks the trailing colon, so it is not an invocation,
        // and `<ref>` is filtered as ubiquitous even when manifest-known.
        assert!(signals.parser_functions.is_empty());
        assert!(signals.extension_tags.is_empty());
    }

    #[test]
    fn queries_fire_from_stub_signals_without_templates() {
        let signals = StubDocsSignals {
            parser_functions: vec!["cargo_query".to_string()],
            extension_tags: vec!["tabber".to_string()],
        };
        let queries = build_authoring_docs_queries(&[], &[], &[], &signals);
        assert_eq!(
            queries,
            vec!["cargo_query parser function", "tabber extension tag"]
        );
    }

    #[test]
    fn queries_put_stub_signals_before_templates_and_cap_at_limit() {
        let signals = StubDocsSignals {
            parser_functions: vec!["cargo_query".to_string()],
            extension_tags: vec!["tabber".to_string()],
        };
        let queries = build_authoring_docs_queries(
            &[],
            &[module_pattern("Module:Infobox")],
            &[stub_template("Template:Infobox person")],
            &signals,
        );
        // Stub-invoked parser functions and tags are the direct docs need and
        // outrank template-derived queries for the capped slots.
        assert_eq!(
            queries,
            vec![
                "cargo_query parser function",
                "tabber extension tag",
                "Infobox person",
                "template parameters",
            ]
        );
        assert_eq!(queries.len(), AUTHORING_DOCS_QUERY_LIMIT);
    }

    #[test]
    fn queries_stay_empty_without_any_signal() {
        let queries = build_authoring_docs_queries(&[], &[], &[], &StubDocsSignals::default());
        assert!(queries.is_empty());
    }

    #[test]
    fn queries_keep_scribunto_lane_for_module_patterns() {
        let queries = build_authoring_docs_queries(
            &[],
            &[module_pattern("Module:Traits")],
            &[],
            &StubDocsSignals::default(),
        );
        assert_eq!(queries, vec!["Scribunto #invoke"]);
    }
}
