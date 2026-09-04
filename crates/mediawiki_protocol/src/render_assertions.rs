//! Deterministic assertions about server HTML. CSS layout and the browser accessibility tree
//! are deliberately outside this evidence surface.
use std::collections::BTreeMap;

use anyhow::{Result, bail};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};

const MAX_ASSERTIONS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RenderDomAssertion {
    pub selector: String,
    #[serde(default = "one")]
    pub min_count: usize,
    #[serde(default)]
    pub max_count: Option<usize>,
    /// Every match must contain this text, with whitespace normalized on both sides.
    #[serde(default)]
    pub text_contains: Option<String>,
    /// null requires presence; a string requires the exact decoded attribute value.
    #[serde(default)]
    pub attributes: BTreeMap<String, Option<String>>,
}

const fn one() -> usize {
    1
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderDomAssertionResult {
    pub assertion_index: usize,
    pub scope_index: Option<usize>,
    pub selector: String,
    pub matched_count: usize,
    pub mismatched_count: usize,
    pub passed: bool,
    pub evaluated: bool,
    pub failure_reason: Option<&'static str>,
}

pub fn validate_dom_assertions(assertions: &[RenderDomAssertion]) -> Result<()> {
    if assertions.len() > MAX_ASSERTIONS {
        bail!("render checks support at most {MAX_ASSERTIONS} DOM assertions");
    }
    for assertion in assertions {
        if assertion.selector.is_empty() || assertion.selector.len() > 256 {
            bail!("DOM assertion selector must contain 1..=256 bytes");
        }
        Selector::parse(&assertion.selector)
            .map_err(|error| anyhow::anyhow!("invalid DOM assertion selector: {error:?}"))?;
        if assertion
            .max_count
            .is_some_and(|max| max < assertion.min_count)
        {
            bail!("DOM assertion max_count is below min_count");
        }
        if assertion
            .text_contains
            .as_ref()
            .is_some_and(|text| text.len() > 4096 || normalize_text(text).is_empty())
        {
            bail!("DOM assertion text_contains must be nonblank and at most 4096 bytes");
        }
        if assertion.attributes.len() > 32
            || assertion.attributes.iter().any(|(name, value)| {
                name.is_empty()
                    || name.len() > 128
                    || !name.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_:".contains(&byte)
                    })
                    || value.as_ref().is_some_and(|value| value.len() > 4096)
            })
        {
            bail!("DOM assertion attributes exceed bounds or use invalid lowercase HTML names");
        }
    }
    Ok(())
}

pub(super) fn analyze_dom_assertions(
    html: &str,
    scope_class: Option<&str>,
    assertions: &[RenderDomAssertion],
) -> Vec<RenderDomAssertionResult> {
    if assertions.is_empty() {
        return Vec::new();
    }
    let document = Html::parse_fragment(html);
    let all = Selector::parse("*").expect("constant selector");
    if document.select(&all).take(100_001).count() > 100_000 {
        return failed_assertions(assertions, "dom_node_limit_exceeded");
    }
    let scopes: Vec<_> = match scope_class {
        Some(class) => document
            .select(&all)
            .filter(|element| element.value().classes().any(|value| value == class))
            .collect(),
        None => vec![document.root_element()],
    };
    // A missing scope must fail even when every assertion allows zero matches. Otherwise
    // removal of the component could turn its entire contract into a vacuous success.
    if scopes.len().saturating_mul(assertions.len()) > 4096 {
        return failed_assertions(assertions, "scope_assertion_limit_exceeded");
    }
    if scopes.is_empty() {
        return failed_assertions(assertions, "scope_missing");
    }
    let mut results = Vec::new();
    for (assertion_index, assertion) in assertions.iter().enumerate() {
        let selector = Selector::parse(&assertion.selector).expect("validated DOM selector");
        for (scope_index, scope) in scopes.iter().enumerate() {
            let mut matched_count = 0;
            let mut mismatched_count = 0;
            for element in std::iter::once(*scope).chain(scope.select(&selector)) {
                if !selector.matches(&element) {
                    continue;
                }
                matched_count += 1;
                if !matches_requirements(element, assertion) {
                    mismatched_count += 1;
                }
            }
            let passed = matched_count >= assertion.min_count
                && assertion.max_count.is_none_or(|max| matched_count <= max)
                && mismatched_count == 0;
            results.push(RenderDomAssertionResult {
                assertion_index,
                scope_index: scope_class.map(|_| scope_index),
                selector: assertion.selector.clone(),
                matched_count,
                mismatched_count,
                passed,
                evaluated: true,
                failure_reason: (!passed).then_some("requirements_mismatch"),
            });
        }
    }
    results
}

fn failed_assertions(
    assertions: &[RenderDomAssertion],
    reason: &'static str,
) -> Vec<RenderDomAssertionResult> {
    assertions
        .iter()
        .enumerate()
        .map(|(index, assertion)| RenderDomAssertionResult {
            assertion_index: index,
            scope_index: None,
            selector: assertion.selector.clone(),
            matched_count: 0,
            mismatched_count: 0,
            passed: false,
            evaluated: false,
            failure_reason: Some(reason),
        })
        .collect()
}

fn matches_requirements(element: ElementRef<'_>, assertion: &RenderDomAssertion) -> bool {
    assertion.attributes.iter().all(|(name, expected)| {
        element
            .value()
            .attr(name)
            .is_some_and(|actual| expected.as_ref().is_none_or(|expected| actual == expected))
    }) && assertion.text_contains.as_ref().is_none_or(|expected| {
        // Concatenate text nodes as HTML does: adding separators would manufacture text.
        let text: String = element.text().collect();
        normalize_text(&text).contains(&normalize_text(expected))
    })
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests;
