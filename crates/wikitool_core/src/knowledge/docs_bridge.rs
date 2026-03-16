use std::collections::BTreeSet;

use anyhow::Result;

use crate::docs::{DocsContextOptions, build_docs_context};
use crate::knowledge::model::{AuthoringDocsContext, ModuleUsageSummary, TemplateReference};
use crate::runtime::ResolvedPaths;

const AUTHORING_DOCS_QUERY_LIMIT: usize = 4;
const AUTHORING_DOCS_TOKEN_BUDGET: usize = 560;

pub(crate) fn build_authoring_docs_context(
    paths: &ResolvedPaths,
    topic: &str,
    query_terms: &[String],
    template_references: &[TemplateReference],
    module_patterns: &[ModuleUsageSummary],
    docs_profile: &str,
) -> Result<Option<AuthoringDocsContext>> {
    let normalized_profile = normalize_spaces(&docs_profile.replace('_', " "));
    if normalized_profile.is_empty() {
        return Ok(None);
    }

    let queries =
        build_authoring_docs_queries(topic, query_terms, template_references, module_patterns);
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

fn build_authoring_docs_queries(
    topic: &str,
    query_terms: &[String],
    template_references: &[TemplateReference],
    module_patterns: &[ModuleUsageSummary],
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    push_authoring_docs_query(&mut out, &mut seen, topic);
    for term in query_terms.iter().take(2) {
        push_authoring_docs_query(&mut out, &mut seen, term);
    }
    for template_title in template_references
        .iter()
        .map(|reference| reference.template.template_title.as_str())
    {
        if let Some(tail) = template_title.split_once(':').map(|(_, tail)| tail) {
            push_authoring_docs_query(&mut out, &mut seen, tail);
        }
        if out.len() >= AUTHORING_DOCS_QUERY_LIMIT {
            break;
        }
    }
    if !template_references.is_empty() {
        push_authoring_docs_query(&mut out, &mut seen, "template parameters");
    }
    if !module_patterns.is_empty() {
        push_authoring_docs_query(&mut out, &mut seen, "Scribunto #invoke");
    }
    out.truncate(AUTHORING_DOCS_QUERY_LIMIT);
    out
}

fn push_authoring_docs_query(queries: &mut Vec<String>, seen: &mut BTreeSet<String>, value: &str) {
    let normalized = normalize_spaces(&value.replace('_', " "));
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
