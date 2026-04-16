use std::collections::BTreeSet;

use anyhow::Result;

use crate::article_lint::document::ParsedArticleDocument;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::content_store::parsing::{extract_wikilinks, load_page_record};
use crate::filesystem::Namespace;
use crate::graph::{GraphFilter, GraphKind, build_graph, compute_scc};
use crate::runtime::ResolvedPaths;

use super::IssueMatch;
use super::common::line_has_short_description;
use crate::article_lint::resources::LoadedResources;

const ALLOWED_SOURCE_HTML_TAGS: &[&str] = &[
    "abbr",
    "b",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "dd",
    "del",
    "div",
    "dl",
    "dt",
    "em",
    "font",
    "gallery",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "includeonly",
    "ins",
    "kbd",
    "li",
    "noinclude",
    "ol",
    "onlyinclude",
    "p",
    "pre",
    "q",
    "rb",
    "rp",
    "rt",
    "rtc",
    "ruby",
    "s",
    "samp",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "th",
    "thead",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "wbr",
];

pub(super) fn lint_red_links_in_see_also(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) -> Result<()> {
    let Some(section) = document.find_section("See also") else {
        return Ok(());
    };
    let Some(connection) = resources.index_connection.as_ref() else {
        return Ok(());
    };

    for link in extract_wikilinks(&section.body_text) {
        if link.is_category_membership || link.target_namespace != Namespace::Main.as_str() {
            continue;
        }
        if load_page_record(connection, &link.target_title)?.is_some() {
            continue;
        }
        let evidence = format!("[[{}]]", link.target_title);
        let start = section.body_start + section.body_text.find(&evidence).unwrap_or(0);
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "integration.red_link_in_see_also".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "See also contains a red link.".to_string(),
                span: document.span_for_range(start, start + evidence.len()),
                evidence: Some(link.target_title.clone()),
                suggested_remediation: Some(
                    "Only keep See also links that resolve to existing local pages.".to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
    Ok(())
}

pub(super) fn lint_capability_rules(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let Some(capabilities) = resources.capabilities.as_ref() else {
        return;
    };

    if !capabilities.has_short_description {
        for line in document.top_nonblank_lines(6) {
            if !line_has_short_description(&line.text) {
                continue;
            }
            matches.push(IssueMatch {
                issue: ArticleLintIssue {
                    rule_id: "capability.short_description_unsupported".to_string(),
                    severity: ArticleLintSeverity::Warning,
                    message: "Draft uses a short-description form that the last synced wiki capabilities do not advertise."
                        .to_string(),
                    span: document.span_for_line(line),
                    evidence: Some(line.text.trim().to_string()),
                    suggested_remediation: Some(
                        "Re-sync wiki capabilities or verify that the target wiki still supports short descriptions."
                            .to_string(),
                    ),
                    suggested_fixes: Vec::new(),
                },
                safe_fixes: Vec::new(),
            });
        }
    }

    let supported_tags = capabilities
        .parser_extension_tags
        .iter()
        .map(|tag| normalize_tag_name(tag))
        .collect::<BTreeSet<_>>();
    for tag in &document.parser_tags {
        if supported_tags.contains(&tag.tag_name)
            || ALLOWED_SOURCE_HTML_TAGS.contains(&tag.tag_name.as_str())
        {
            continue;
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "capability.unsupported_extension_tag".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "Draft uses an extension or HTML tag that is not present in the last synced wiki capability manifest or local source allowlist."
                    .to_string(),
                span: document.span_for_range(tag.start, tag.start + tag.tag_name.len() + 1),
                evidence: Some(format!("<{}>", tag.tag_name)),
                suggested_remediation: Some(
                    "Use only parser tags confirmed by `wikitool wiki capabilities show`, or source HTML tags that are known-safe on the target wiki."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}

fn normalize_tag_name(tag: &str) -> String {
    tag.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim_start_matches('/')
        .trim()
        .to_ascii_lowercase()
}

pub(super) fn lint_graph_rules(
    _paths: &ResolvedPaths,
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) -> Result<()> {
    let Some(connection) = resources.index_connection.as_ref() else {
        return Ok(());
    };
    let Some(record) = load_page_record(connection, &document.title)? else {
        return Ok(());
    };

    if record.is_redirect {
        let graph = build_graph(connection, GraphKind::Redirects, &GraphFilter::default())?;
        let scc = compute_scc(&graph);
        if let Some(component_size) =
            component_size_for_title(&graph, &scc, &record.title, &record.namespace)
            && component_size > 1
        {
            matches.push(IssueMatch {
                issue: ArticleLintIssue {
                    rule_id: "graph.redirect_loop".to_string(),
                    severity: ArticleLintSeverity::Error,
                    message: "Redirect participates in a redirect cycle.".to_string(),
                    span: document
                        .first_nonblank_line()
                        .and_then(|line| document.span_for_line(line)),
                    evidence: document.redirect_target.clone(),
                    suggested_remediation: Some(
                        "Break the redirect loop so the page resolves to a final target."
                            .to_string(),
                    ),
                    suggested_fixes: Vec::new(),
                },
                safe_fixes: Vec::new(),
            });
        }
        return Ok(());
    }

    match record.namespace.as_str() {
        "Category" => {
            let graph = build_graph(connection, GraphKind::Categories, &GraphFilter::default())?;
            let scc = compute_scc(&graph);
            if let Some(component_size) =
                component_size_for_title(&graph, &scc, &record.title, &record.namespace)
                && component_size > 1
            {
                matches.push(IssueMatch {
                    issue: ArticleLintIssue {
                        rule_id: "graph.category_cycle".to_string(),
                        severity: ArticleLintSeverity::Warning,
                        message: "Category participates in a local category cycle.".to_string(),
                        span: document
                            .first_nonblank_line()
                            .and_then(|line| document.span_for_line(line)),
                        evidence: Some(format!("component_size={component_size}")),
                        suggested_remediation: Some(
                            "Verify that the category relationship is intentional rather than an accidental loop."
                                .to_string(),
                        ),
                        suggested_fixes: Vec::new(),
                    },
                    safe_fixes: Vec::new(),
                });
            }
        }
        "Template" | "Module" => {
            let graph = build_graph(connection, GraphKind::Transclusion, &GraphFilter::default())?;
            let scc = compute_scc(&graph);
            if let Some(component_size) =
                component_size_for_title(&graph, &scc, &record.title, &record.namespace)
                && component_size > 1
            {
                matches.push(IssueMatch {
                    issue: ArticleLintIssue {
                        rule_id: "graph.transclusion_cycle".to_string(),
                        severity: ArticleLintSeverity::Warning,
                        message: "Page sits inside a template/module dependency cycle.".to_string(),
                        span: document
                            .first_nonblank_line()
                            .and_then(|line| document.span_for_line(line)),
                        evidence: Some(format!("component_size={component_size}")),
                        suggested_remediation: Some(
                            "Review the transclusion SCC before making structural changes because the blast radius is broad."
                                .to_string(),
                        ),
                        suggested_fixes: Vec::new(),
                    },
                    safe_fixes: Vec::new(),
                });
            }
        }
        _ => {}
    }
    Ok(())
}

fn component_size_for_title(
    graph: &crate::graph::DirectedGraph,
    scc: &crate::graph::SccIndex,
    title: &str,
    namespace: &str,
) -> Option<usize> {
    let node = graph
        .nodes
        .iter()
        .find(|node| node.title == title && node.namespace == namespace)?;
    let component = scc.component_of(node.id)?;
    if !component.is_cyclic {
        return None;
    }
    Some(component.members.len())
}
