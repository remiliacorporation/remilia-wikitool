use anyhow::Result;

use crate::article_lint::fix::TextEdit;
use crate::article_lint::model::{ArticleLintIssue, ArticleLintSeverity};
use crate::runtime::ResolvedPaths;

use super::document::ParsedArticleDocument;
use super::resources::LoadedResources;

mod asset;
mod citation;
mod common;
mod extension;
mod integration;
mod module;
mod structure;
mod style;
mod template;
mod wikitext;

#[derive(Debug, Clone)]
pub(super) struct SafeFixEdit {
    pub(super) rule_id: String,
    pub(super) label: String,
    pub(super) line: Option<usize>,
    pub(super) edit: TextEdit,
}

#[derive(Debug, Clone)]
pub(super) struct IssueMatch {
    pub(super) issue: ArticleLintIssue,
    pub(super) safe_fixes: Vec<SafeFixEdit>,
}

pub(super) fn collect_issue_matches(
    paths: &ResolvedPaths,
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
) -> Result<Vec<IssueMatch>> {
    let mut matches = Vec::new();
    structure::lint_missing_short_description(document, resources, &mut matches);
    structure::lint_article_quality_banner(document, resources, &mut matches);
    structure::lint_markdown_headings(document, &mut matches);
    wikitext::lint_raw_wikitext_balance(document, &mut matches);
    extension::lint_extension_contracts(document, &mut matches);
    structure::lint_malformed_headings(document, &mut matches);
    structure::lint_duplicate_headings(document, &mut matches);
    structure::lint_sentence_case_headings(document, &mut matches);
    structure::lint_missing_references_section(document, resources, &mut matches);
    structure::lint_missing_reflist(document, resources, &mut matches);
    citation::lint_citation_after_punctuation(document, &mut matches);
    style::lint_curly_quotes(document, &mut matches);
    style::lint_placeholder_fragments(document, resources, &mut matches);
    template::lint_citation_needed(document, &mut matches);
    template::lint_remilia_parent_group(document, resources, &mut matches);
    template::lint_template_availability(document, resources, &mut matches);
    module::lint_module_invocations(document, resources, &mut matches);
    asset::lint_asset_references(document, resources, &mut matches);
    integration::lint_red_links_in_see_also(document, resources, &mut matches)?;
    integration::lint_capability_rules(document, resources, &mut matches);
    integration::lint_graph_rules(paths, document, resources, &mut matches)?;

    matches.sort_by(compare_issue_matches);
    Ok(matches)
}

fn compare_issue_matches(left: &IssueMatch, right: &IssueMatch) -> std::cmp::Ordering {
    severity_rank(left.issue.severity)
        .cmp(&severity_rank(right.issue.severity))
        .then(
            left.issue
                .span
                .as_ref()
                .map(|span| span.line)
                .unwrap_or(usize::MAX)
                .cmp(
                    &right
                        .issue
                        .span
                        .as_ref()
                        .map(|span| span.line)
                        .unwrap_or(usize::MAX),
                ),
        )
        .then(left.issue.rule_id.cmp(&right.issue.rule_id))
}

fn severity_rank(severity: ArticleLintSeverity) -> usize {
    match severity {
        ArticleLintSeverity::Error => 0,
        ArticleLintSeverity::Warning => 1,
        ArticleLintSeverity::Suggestion => 2,
    }
}
