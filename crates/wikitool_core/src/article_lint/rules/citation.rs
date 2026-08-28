use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::fix::TextEdit;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::content_store::parsing::extract_reference_records;
use crate::content_store::parsing::make_content_preview;
use reqwest::Url;

use super::common::safe_fix_for_edit;
use super::{IssueMatch, SafeFixEdit};
use crate::article_lint::resources::LoadedResources;

pub(super) fn lint_citation_after_punctuation(
    document: &ParsedArticleDocument,
    matches: &mut Vec<IssueMatch>,
) {
    let mut index = 0usize;
    while index < document.references.len() {
        let cluster_start = index;
        let mut cluster_end = index;
        while cluster_end + 1 < document.references.len() {
            let current = &document.references[cluster_end];
            let next = &document.references[cluster_end + 1];
            if !document.content[current.end..next.start]
                .chars()
                .all(char::is_whitespace)
            {
                break;
            }
            cluster_end += 1;
        }

        let first_reference = &document.references[cluster_start];
        let last_reference = &document.references[cluster_end];
        let Some(punctuation) = document.content[last_reference.end..]
            .chars()
            .next()
            .filter(|ch| matches!(ch, '.' | ',' | ';' | ':' | '!' | '?'))
        else {
            index = cluster_end + 1;
            continue;
        };
        let punctuation_end = last_reference.end + punctuation.len_utf8();
        let cluster_text = &document.content[first_reference.start..last_reference.end];
        let edit = TextEdit {
            start: first_reference.start,
            end: punctuation_end,
            replacement: format!("{punctuation}{cluster_text}"),
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "citation.after_punctuation".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Inline citations should come after punctuation, not before it."
                    .to_string(),
                span: document.span_for_range(first_reference.start, punctuation_end),
                evidence: Some(make_content_preview(
                    &document.content[first_reference.start..punctuation_end],
                    96,
                )),
                suggested_remediation: Some(
                    "Move the punctuation mark so it appears before the reference tag.".to_string(),
                ),
                suggested_fixes: vec![safe_fix_for_edit(
                    document,
                    &edit,
                    "Move punctuation before reference tag",
                )],
            },
            safe_fixes: vec![SafeFixEdit {
                rule_id: "citation.after_punctuation".to_string(),
                label: "Move punctuation before reference tag".to_string(),
                line: document
                    .line_for_offset(first_reference.start)
                    .map(|line| line.number),
                edit,
            }],
        });
        index = cluster_end + 1;
    }
}

pub(super) fn lint_source_review_rules(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if resources.adapter.citations.source_review_rules.is_empty() {
        return;
    }
    let references = extract_reference_records(&document.content);
    let lowered_content = document.content.to_ascii_lowercase();
    for rule in &resources.adapter.citations.source_review_rules {
        let review_host = rule.host.trim().to_ascii_lowercase();
        if review_host.is_empty() {
            continue;
        }
        let matched_url = references.iter().find_map(|reference| {
            reference
                .source_urls
                .iter()
                .find(|url| url_matches_review_host(url, &review_host))
                .cloned()
                .or_else(|| {
                    host_matches_review_host(&reference.source_domain, &review_host)
                        .then(|| reference.canonical_url.clone())
                })
        });
        let Some(matched_url) = matched_url else {
            continue;
        };
        let span = lowered_content.find(&review_host).and_then(|start| {
            document.span_for_range(start, start.saturating_add(review_host.len()))
        });
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "citation.source_review".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: format!("Citation matches site-adapter review rule: {}.", rule.label),
                span,
                evidence: Some(matched_url),
                suggested_remediation: Some(rule.reason.clone()),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}

fn url_matches_review_host(raw_url: &str, review_host: &str) -> bool {
    let parsed = if raw_url.starts_with("//") {
        Url::parse(&format!("https:{raw_url}"))
    } else {
        Url::parse(raw_url)
    };
    parsed
        .ok()
        .and_then(|url| url.host_str().map(ToString::to_string))
        .is_some_and(|host| host_matches_review_host(&host, review_host))
}

fn host_matches_review_host(candidate: &str, review_host: &str) -> bool {
    let candidate = candidate.trim_end_matches('.').to_ascii_lowercase();
    candidate == review_host || candidate.ends_with(&format!(".{review_host}"))
}

#[cfg(test)]
mod tests {
    use super::url_matches_review_host;

    #[test]
    fn source_review_host_matches_exact_host_and_subdomains_only() {
        assert!(url_matches_review_host(
            "https://en.wikipedia.org/wiki/Example",
            "wikipedia.org"
        ));
        assert!(url_matches_review_host(
            "//wikipedia.org/wiki/Example",
            "wikipedia.org"
        ));
        assert!(!url_matches_review_host(
            "https://notwikipedia.org/example",
            "wikipedia.org"
        ));
        assert!(!url_matches_review_host(
            "https://example.org/?next=https://wikipedia.org/wiki/Example",
            "wikipedia.org"
        ));
    }
}
