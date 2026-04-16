mod fix;
mod model;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

use crate::content_store::parsing::{
    canonical_template_title, extract_wikilinks, find_closing_html_tag, load_page_record,
    make_content_preview, normalize_spaces, open_indexed_connection, parse_heading_line,
    parse_open_tag, starts_with_html_tag,
};
use crate::filesystem::{
    Namespace, namespace_from_title, relative_path_to_title, validate_scoped_path,
};
use crate::graph::{GraphFilter, GraphKind, build_graph, compute_scc};
use crate::profile::{
    ProfileOverlay, TemplateCatalog, TemplateCatalogEntryLookup, WikiCapabilityManifest,
    build_template_catalog_with_overlay, find_template_catalog_entry,
    load_latest_wiki_capabilities, load_or_build_remilia_profile_overlay,
};
use crate::research::export::{WikitextLintIssue, lint_wikitext};
use crate::runtime::ResolvedPaths;
use crate::support::{normalize_path, parse_redirect};

#[cfg(test)]
use crate::knowledge::status::KNOWLEDGE_GENERATION;
#[cfg(test)]
use crate::schema::open_initialized_database_connection;

pub use model::{
    AppliedFixRecord, ArticleFixApplyMode, ArticleFixResult, ArticleLintIssue, ArticleLintReport,
    ArticleLintResourcesStatus, ArticleLintSeverity, SuggestedFix, SuggestedFixKind, TextSpan,
};

use fix::{TextEdit, apply_text_edits};

const ARTICLE_LINT_SCHEMA_VERSION: &str = "article_lint_v1";
const ARTICLE_FIX_SCHEMA_VERSION: &str = "article_fix_v1";
const REMILIA_PROFILE_ID: &str = "remilia";
const COMMON_SENTENCE_CASE_HEADINGS: &[(&str, &str)] = &[
    ("see also", "See also"),
    ("external links", "External links"),
    ("further reading", "Further reading"),
    ("early life", "Early life"),
    ("early life and education", "Early life and education"),
    ("personal life", "Personal life"),
    ("notable works", "Notable works"),
    ("notable work", "Notable work"),
];
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

#[derive(Debug, Clone)]
struct LineRecord {
    number: usize,
    start: usize,
    end: usize,
    text: String,
}

#[derive(Debug, Clone)]
struct HeadingRecord {
    level: u8,
    text: String,
    line: usize,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
struct ArticleSection {
    heading: Option<HeadingRecord>,
    body_start: usize,
    body_end: usize,
    body_text: String,
}

#[derive(Debug, Clone)]
struct TemplateOccurrence {
    template_title: String,
    parameter_keys: Vec<String>,
    raw_wikitext: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
struct RefOccurrence {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
struct ParserTagOccurrence {
    tag_name: String,
    start: usize,
}

#[derive(Debug, Clone)]
struct ParsedArticleDocument {
    relative_path: String,
    title: String,
    namespace: String,
    content: String,
    is_redirect: bool,
    redirect_target: Option<String>,
    lines: Vec<LineRecord>,
    sections: Vec<ArticleSection>,
    templates: Vec<TemplateOccurrence>,
    references: Vec<RefOccurrence>,
    parser_tags: Vec<ParserTagOccurrence>,
}

#[derive(Debug)]
struct LoadedResources {
    overlay: ProfileOverlay,
    capabilities: Option<WikiCapabilityManifest>,
    template_catalog: Option<TemplateCatalog>,
    index_connection: Option<Connection>,
}

#[derive(Debug, Clone)]
struct SafeFixEdit {
    rule_id: String,
    label: String,
    line: Option<usize>,
    edit: TextEdit,
}

#[derive(Debug, Clone)]
struct IssueMatch {
    issue: ArticleLintIssue,
    safe_fixes: Vec<SafeFixEdit>,
}

impl ParsedArticleDocument {
    fn span_for_range(&self, start: usize, end: usize) -> Option<TextSpan> {
        let line = self.line_for_offset(start)?;
        let end_line = self.line_for_offset(end.min(self.content.len()))?;
        let column = self.column_for_offset(line, start);
        let end_column = self.column_for_offset(end_line, end.min(self.content.len()));
        Some(TextSpan {
            line: line.number,
            column,
            end_line: Some(end_line.number),
            end_column: Some(end_column),
        })
    }

    fn span_for_line(&self, line: &LineRecord) -> Option<TextSpan> {
        Some(TextSpan {
            line: line.number,
            column: 1,
            end_line: Some(line.number),
            end_column: Some(line.text.chars().count().saturating_add(1)),
        })
    }

    fn line_for_offset(&self, offset: usize) -> Option<&LineRecord> {
        self.lines
            .iter()
            .find(|line| offset >= line.start && offset <= line.end)
            .or_else(|| self.lines.last())
    }

    fn column_for_offset(&self, line: &LineRecord, offset: usize) -> usize {
        line.text[..offset.saturating_sub(line.start).min(line.text.len())]
            .chars()
            .count()
            .saturating_add(1)
    }

    fn first_nonblank_line(&self) -> Option<&LineRecord> {
        self.lines.iter().find(|line| !line.text.trim().is_empty())
    }

    fn top_nonblank_lines(&self, limit: usize) -> Vec<&LineRecord> {
        self.lines
            .iter()
            .filter(|line| !line.text.trim().is_empty())
            .take(limit)
            .collect()
    }

    fn find_section(&self, heading: &str) -> Option<&ArticleSection> {
        self.sections.iter().find(|section| {
            section
                .heading
                .as_ref()
                .map(|candidate| candidate.text.eq_ignore_ascii_case(heading))
                .unwrap_or(false)
        })
    }
}

pub fn lint_article(
    paths: &ResolvedPaths,
    article_path: &Path,
    profile_id: Option<&str>,
) -> Result<ArticleLintReport> {
    let profile_id = normalize_profile_id(profile_id)?;
    let document = load_article_document(paths, article_path)?;
    let resources = load_resources(paths, &profile_id)?;
    let matches = collect_issue_matches(paths, &document, &resources)?;
    Ok(build_report(&document, &profile_id, &resources, matches))
}

pub fn fix_article(
    paths: &ResolvedPaths,
    article_path: &Path,
    profile_id: Option<&str>,
    apply_mode: ArticleFixApplyMode,
) -> Result<ArticleFixResult> {
    let profile_id = normalize_profile_id(profile_id)?;
    let document = load_article_document(paths, article_path)?;
    let resources = load_resources(paths, &profile_id)?;
    let matches = collect_issue_matches(paths, &document, &resources)?;
    let safe_fixes = collect_safe_fixes(&matches);
    let changed = apply_mode == ArticleFixApplyMode::Safe && !safe_fixes.is_empty();
    if changed {
        let new_content = apply_text_edits(
            &document.content,
            &safe_fixes
                .iter()
                .map(|fix| fix.edit.clone())
                .collect::<Vec<_>>(),
        )?;
        let absolute_path = paths.project_root.join(&document.relative_path);
        fs::write(&absolute_path, new_content)
            .with_context(|| format!("failed to write {}", absolute_path.display()))?;
    }

    let remaining_report = lint_article(paths, article_path, Some(&profile_id))?;
    Ok(ArticleFixResult {
        schema_version: ARTICLE_FIX_SCHEMA_VERSION.to_string(),
        profile_id,
        relative_path: remaining_report.relative_path.clone(),
        title: remaining_report.title.clone(),
        namespace: remaining_report.namespace.clone(),
        apply_mode: apply_mode.as_str().to_string(),
        changed,
        applied_fix_count: if changed { safe_fixes.len() } else { 0 },
        applied_fixes: if changed {
            safe_fixes
                .into_iter()
                .map(|fix| AppliedFixRecord {
                    rule_id: fix.rule_id,
                    label: fix.label,
                    line: fix.line,
                })
                .collect()
        } else {
            Vec::new()
        },
        remaining_report,
    })
}

fn normalize_profile_id(profile_id: Option<&str>) -> Result<String> {
    let profile_id = profile_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(REMILIA_PROFILE_ID);
    if !profile_id.eq_ignore_ascii_case(REMILIA_PROFILE_ID) {
        bail!("unsupported article lint profile: {profile_id} (expected remilia)");
    }
    Ok(REMILIA_PROFILE_ID.to_string())
}

fn load_resources(paths: &ResolvedPaths, profile_id: &str) -> Result<LoadedResources> {
    let overlay = if profile_id.eq_ignore_ascii_case(REMILIA_PROFILE_ID) {
        load_or_build_remilia_profile_overlay(paths)?
    } else {
        bail!("unsupported article lint profile: {profile_id}");
    };

    let capabilities = if paths.db_path.exists() {
        load_latest_wiki_capabilities(paths)?
    } else {
        None
    };
    let template_catalog = {
        let built = build_template_catalog_with_overlay(paths, &overlay)?;
        if built.entries.is_empty() {
            None
        } else {
            Some(built)
        }
    };
    let index_connection = open_indexed_connection(paths)?;

    Ok(LoadedResources {
        overlay,
        capabilities,
        template_catalog,
        index_connection,
    })
}

fn load_article_document(
    paths: &ResolvedPaths,
    article_path: &Path,
) -> Result<ParsedArticleDocument> {
    let absolute_path = if article_path.is_absolute() {
        article_path.to_path_buf()
    } else {
        paths.project_root.join(article_path)
    };
    validate_scoped_path(paths, &absolute_path)?;
    let relative_path = absolute_path
        .strip_prefix(&paths.project_root)
        .map(normalize_path)
        .unwrap_or_else(|_| normalize_path(&absolute_path));
    let content = fs::read_to_string(&absolute_path)
        .with_context(|| format!("failed to read {}", absolute_path.display()))?;
    let title = relative_path_to_title(paths, &relative_path)?;
    let namespace = namespace_from_title(&title).as_str().to_string();
    let (is_redirect, redirect_target) = parse_redirect(&content);
    let lines = collect_lines(&content);
    let sections = parse_sections(&content, &lines);
    let templates = extract_template_occurrences(&content);
    let references = extract_ref_occurrences(&content);
    let parser_tags = extract_open_tag_occurrences(&content);

    Ok(ParsedArticleDocument {
        relative_path,
        title,
        namespace,
        content,
        is_redirect,
        redirect_target,
        lines,
        sections,
        templates,
        references,
        parser_tags,
    })
}

fn collect_lines(content: &str) -> Vec<LineRecord> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    let mut line_number = 1usize;
    for segment in content.split_inclusive('\n') {
        let line = segment.trim_end_matches('\n').trim_end_matches('\r');
        out.push(LineRecord {
            number: line_number,
            start: offset,
            end: offset + line.len(),
            text: line.to_string(),
        });
        offset += segment.len();
        line_number += 1;
    }
    if content.is_empty() {
        out.push(LineRecord {
            number: 1,
            start: 0,
            end: 0,
            text: String::new(),
        });
    }
    out
}

fn parse_sections(content: &str, lines: &[LineRecord]) -> Vec<ArticleSection> {
    let mut out = Vec::new();
    let mut current_heading: Option<HeadingRecord> = None;
    let mut body_start = 0usize;

    for line in lines {
        let trimmed = line.text.trim();
        if let Some((level, heading)) = parse_heading_line(trimmed) {
            out.push(ArticleSection {
                heading: current_heading.take(),
                body_start,
                body_end: line.start,
                body_text: content[body_start..line.start].to_string(),
            });
            current_heading = Some(HeadingRecord {
                level,
                text: heading,
                line: line.number,
                start: line.start,
                end: line.end,
            });
            body_start = if line.end < content.len() && content.as_bytes()[line.end] == b'\n' {
                line.end + 1
            } else {
                line.end
            };
        }
    }

    out.push(ArticleSection {
        heading: current_heading,
        body_start,
        body_end: content.len(),
        body_text: content[body_start..].to_string(),
    });
    out
}

fn extract_template_occurrences(content: &str) -> Vec<TemplateOccurrence> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut stack = Vec::new();

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            stack.push(cursor);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'}' && bytes[cursor + 1] == b'}' {
            if let Some(start) = stack.pop()
                && cursor >= start + 2
            {
                let inner = &content[start + 2..cursor];
                let segments = split_template_segments(inner);
                let raw_name = segments.first().map(String::as_str).unwrap_or("").trim();
                if let Some(template_title) = canonical_template_title(raw_name) {
                    let parameter_keys = collect_parameter_keys(&segments);
                    out.push(TemplateOccurrence {
                        template_title,
                        parameter_keys,
                        raw_wikitext: content[start..cursor + 2].to_string(),
                        start,
                        end: cursor + 2,
                    });
                }
            }
            cursor += 2;
            continue;
        }
        cursor += 1;
    }

    out
}

fn collect_parameter_keys(segments: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut positional_index = 1usize;
    for segment in segments.iter().skip(1) {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, _)) = split_once_top_level_equals(trimmed) {
            let key = normalize_spaces(&key.replace('_', " ")).to_ascii_lowercase();
            if !key.is_empty() {
                out.push(key);
                continue;
            }
        }
        out.push(format!("${positional_index}"));
        positional_index += 1;
    }
    out.sort();
    out.dedup();
    out
}

fn extract_ref_occurrences(content: &str) -> Vec<RefOccurrence> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if !starts_with_html_tag(bytes, cursor, "ref") {
            cursor += 1;
            continue;
        }
        let Some((tag_end, _, self_closing)) = parse_open_tag(content, cursor, "ref") else {
            cursor += 1;
            continue;
        };
        if self_closing {
            out.push(RefOccurrence {
                start: cursor,
                end: tag_end,
            });
            cursor = tag_end;
            continue;
        }
        if let Some((_, close_end)) = find_closing_html_tag(content, tag_end, "ref") {
            out.push(RefOccurrence {
                start: cursor,
                end: close_end,
            });
            cursor = close_end;
            continue;
        }
        out.push(RefOccurrence {
            start: cursor,
            end: tag_end,
        });
        cursor = tag_end;
    }

    out
}

fn extract_open_tag_occurrences(content: &str) -> Vec<ParserTagOccurrence> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if bytes[cursor] != b'<' {
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 1;
        if cursor >= bytes.len() {
            break;
        }
        if matches!(bytes[cursor], b'/' | b'!' | b'?') || !bytes[cursor].is_ascii_alphabetic() {
            continue;
        }
        let name_start = cursor;
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'-')
        {
            cursor += 1;
        }
        let tag_name = content[name_start..cursor].to_ascii_lowercase();
        out.push(ParserTagOccurrence { tag_name, start });
    }

    out
}

fn collect_issue_matches(
    paths: &ResolvedPaths,
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
) -> Result<Vec<IssueMatch>> {
    let mut matches = Vec::new();
    lint_missing_short_description(document, resources, &mut matches);
    lint_article_quality_banner(document, resources, &mut matches);
    lint_markdown_headings(document, &mut matches);
    lint_raw_wikitext_balance(document, &mut matches);
    lint_malformed_headings(document, &mut matches);
    lint_duplicate_headings(document, &mut matches);
    lint_sentence_case_headings(document, &mut matches);
    lint_missing_references_section(document, resources, &mut matches);
    lint_missing_reflist(document, resources, &mut matches);
    lint_citation_after_punctuation(document, &mut matches);
    lint_curly_quotes(document, &mut matches);
    lint_placeholder_fragments(document, resources, &mut matches);
    lint_citation_needed(document, &mut matches);
    lint_remilia_parent_group(document, resources, &mut matches);
    lint_template_availability(document, resources, &mut matches);
    lint_red_links_in_see_also(document, resources, &mut matches)?;
    lint_capability_rules(document, resources, &mut matches);
    lint_graph_rules(paths, document, resources, &mut matches)?;

    matches.sort_by(compare_issue_matches);
    Ok(matches)
}

fn lint_raw_wikitext_balance(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
    for issue in lint_wikitext(&document.content) {
        matches.push(wikitext_balance_issue(document, &issue));
    }
}

fn wikitext_balance_issue(
    document: &ParsedArticleDocument,
    issue: &WikitextLintIssue,
) -> IssueMatch {
    IssueMatch {
        issue: ArticleLintIssue {
            rule_id: format!("wikitext.{}", issue.rule_id),
            severity: ArticleLintSeverity::Error,
            message: issue.message.clone(),
            span: document.span_for_range(issue.byte_offset, issue.byte_offset.saturating_add(1)),
            evidence: Some(make_content_preview(
                &document.content[issue.byte_offset.min(document.content.len())..],
                96,
            )),
            suggested_remediation: Some(
                "Repair the raw wikitext balance at the reported location before revising article prose."
                    .to_string(),
            ),
            suggested_fixes: Vec::new(),
        },
        safe_fixes: Vec::new(),
    }
}

fn lint_missing_short_description(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if document.namespace != Namespace::Main.as_str() || document.is_redirect {
        return;
    }
    if !resources.overlay.authoring.require_short_description {
        return;
    }
    if document
        .top_nonblank_lines(6)
        .iter()
        .any(|line| line_has_short_description(&line.text))
    {
        return;
    }

    let first_line = document.first_nonblank_line();
    matches.push(IssueMatch {
        issue: ArticleLintIssue {
            rule_id: "structure.require_short_description".to_string(),
            severity: ArticleLintSeverity::Error,
            message: "Article is missing the required short description header.".to_string(),
            span: first_line.and_then(|line| document.span_for_line(line)),
            evidence: first_line.map(|line| make_content_preview(&line.text, 96)),
            suggested_remediation: Some(
                "Insert the required short description at the top of the article before the quality banner.".to_string(),
            ),
            suggested_fixes: vec![SuggestedFix {
                label: "Insert short description header".to_string(),
                kind: SuggestedFixKind::AssistedFix,
                replacement_preview: Some(preferred_short_description_snippet(&resources.overlay)),
                patch: None,
            }],
        },
        safe_fixes: Vec::new(),
    });
}

fn lint_article_quality_banner(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if document.namespace != Namespace::Main.as_str() || document.is_redirect {
        return;
    }
    if !resources.overlay.authoring.require_article_quality_banner {
        return;
    }

    let top_lines = document.top_nonblank_lines(8);
    let banner_line = top_lines
        .iter()
        .find(|line| line.text.trim_start().starts_with("{{Article quality|"));
    if banner_line.is_none() {
        let insertion_offset = top_lines
            .iter()
            .find(|line| line_has_short_description(&line.text))
            .map(|line| line.end)
            .or_else(|| document.first_nonblank_line().map(|line| line.start))
            .unwrap_or(0);
        let replacement = if top_lines
            .iter()
            .any(|line| line_has_short_description(&line.text))
        {
            "\n{{Article quality|unverified}}".to_string()
        } else {
            "{{Article quality|unverified}}\n".to_string()
        };
        let edit = TextEdit {
            start: insertion_offset,
            end: insertion_offset,
            replacement,
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "structure.require_article_quality_banner".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "AI-generated drafts should include the article quality banner near the top.".to_string(),
                span: document.first_nonblank_line().and_then(|line| document.span_for_line(line)),
                evidence: document.first_nonblank_line().map(|line| make_content_preview(&line.text, 96)),
                suggested_remediation: Some(
                    "Insert {{Article quality|unverified}} immediately below the short description.".to_string(),
                ),
                suggested_fixes: vec![safe_fix_for_edit(
                    document,
                    &edit,
                    "Insert article quality banner",
                )],
            },
            safe_fixes: vec![SafeFixEdit {
                rule_id: "structure.require_article_quality_banner".to_string(),
                label: "Insert article quality banner".to_string(),
                line: document.first_nonblank_line().map(|line| line.number),
                edit,
            }],
        });
        return;
    }

    if let Some(expected_state) = resources
        .overlay
        .authoring
        .article_quality_default_state
        .as_deref()
        && let Some(line) = banner_line
        && !line
            .text
            .to_ascii_lowercase()
            .contains(&format!("|{}", expected_state.to_ascii_lowercase()))
    {
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "structure.article_quality_state".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Article quality banner is using a non-default state for an AI-authored draft.".to_string(),
                span: document.span_for_line(line),
                evidence: Some(line.text.trim().to_string()),
                suggested_remediation: Some(
                    "Use {{Article quality|unverified}} until a human editor changes the review state.".to_string(),
                ),
                suggested_fixes: vec![SuggestedFix {
                    label: "Normalize article quality state".to_string(),
                    kind: SuggestedFixKind::AssistedFix,
                    replacement_preview: Some("{{Article quality|unverified}}".to_string()),
                    patch: None,
                }],
            },
            safe_fixes: Vec::new(),
        });
    }
}

fn lint_markdown_headings(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
    for line in &document.lines {
        let Some((level, text)) = parse_markdown_heading(&line.text) else {
            continue;
        };
        let replacement = format!(
            "{} {} {}",
            "=".repeat(level as usize),
            text,
            "=".repeat(level as usize)
        );
        let edit = TextEdit {
            start: line.start,
            end: line.end,
            replacement: replacement.clone(),
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "structure.markdown_heading".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "Markdown heading syntax is not valid article wikitext.".to_string(),
                span: document.span_for_line(line),
                evidence: Some(line.text.trim().to_string()),
                suggested_remediation: Some(
                    "Replace Markdown headings with MediaWiki heading markup.".to_string(),
                ),
                suggested_fixes: vec![safe_fix_for_edit(
                    document,
                    &edit,
                    "Convert Markdown heading to MediaWiki heading",
                )],
            },
            safe_fixes: vec![SafeFixEdit {
                rule_id: "structure.markdown_heading".to_string(),
                label: "Convert Markdown heading to MediaWiki heading".to_string(),
                line: Some(line.number),
                edit,
            }],
        });
    }
}

fn lint_malformed_headings(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
    for line in &document.lines {
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if (trimmed.starts_with('=') || trimmed.ends_with('='))
            && parse_heading_line(trimmed).is_none()
        {
            matches.push(IssueMatch {
                issue: ArticleLintIssue {
                    rule_id: "structure.malformed_heading".to_string(),
                    severity: ArticleLintSeverity::Error,
                    message: "Heading markup is malformed.".to_string(),
                    span: document.span_for_line(line),
                    evidence: Some(trimmed.to_string()),
                    suggested_remediation: Some(
                        "Use balanced MediaWiki heading markers such as == Heading ==.".to_string(),
                    ),
                    suggested_fixes: Vec::new(),
                },
                safe_fixes: Vec::new(),
            });
        }
    }
}

fn lint_duplicate_headings(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
    let mut seen = BTreeSet::new();
    for section in &document.sections {
        let Some(heading) = &section.heading else {
            continue;
        };
        let normalized = normalize_spaces(&heading.text).to_ascii_lowercase();
        if normalized.is_empty() || seen.insert(normalized) {
            continue;
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "structure.duplicate_heading".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Article reuses the same heading more than once.".to_string(),
                span: document.span_for_range(heading.start, heading.end),
                evidence: Some(heading.text.clone()),
                suggested_remediation: Some(
                    "Merge duplicated sections or rename one heading so the structure is unambiguous.".to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}

fn lint_sentence_case_headings(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
    for section in &document.sections {
        let Some(heading) = &section.heading else {
            continue;
        };
        let Some(canonical) = canonical_sentence_case_heading(&heading.text) else {
            continue;
        };
        if canonical == heading.text {
            continue;
        }
        let mut safe_fixes = Vec::new();
        let mut suggested_fixes = Vec::new();
        if safe_heading_rewrite_available(&heading.text, &canonical) {
            let edit = TextEdit {
                start: heading.start,
                end: heading.end,
                replacement: format!(
                    "{} {} {}",
                    "=".repeat(heading.level as usize),
                    canonical,
                    "=".repeat(heading.level as usize)
                ),
            };
            suggested_fixes.push(safe_fix_for_edit(
                document,
                &edit,
                "Normalize heading to sentence case",
            ));
            safe_fixes.push(SafeFixEdit {
                rule_id: "style.sentence_case_heading".to_string(),
                label: "Normalize heading to sentence case".to_string(),
                line: Some(heading.line),
                edit,
            });
        } else {
            suggested_fixes.push(SuggestedFix {
                label: "Rewrite heading in sentence case".to_string(),
                kind: SuggestedFixKind::AssistedFix,
                replacement_preview: Some(canonical),
                patch: None,
            });
        }

        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "style.sentence_case_heading".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Headings should use sentence case.".to_string(),
                span: document.span_for_range(heading.start, heading.end),
                evidence: Some(heading.text.clone()),
                suggested_remediation: Some(
                    "Rewrite the heading with sentence case capitalization.".to_string(),
                ),
                suggested_fixes,
            },
            safe_fixes,
        });
    }
}

fn lint_missing_references_section(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    if document.namespace != Namespace::Main.as_str() || document.is_redirect {
        return;
    }
    if !resources
        .overlay
        .authoring
        .required_appendix_sections
        .iter()
        .any(|section| section.eq_ignore_ascii_case("References"))
    {
        return;
    }
    if document.find_section("References").is_some() {
        return;
    }

    let severity = if document.references.is_empty() {
        ArticleLintSeverity::Warning
    } else {
        ArticleLintSeverity::Error
    };
    matches.push(IssueMatch {
        issue: ArticleLintIssue {
            rule_id: "structure.require_references_section".to_string(),
            severity,
            message: "Article is missing the required References section.".to_string(),
            span: document
                .first_nonblank_line()
                .and_then(|line| document.span_for_line(line)),
            evidence: Some("== References ==".to_string()),
            suggested_remediation: Some(
                "Add a References section near the end of the article and render it with {{Reflist}}.".to_string(),
            ),
            suggested_fixes: vec![SuggestedFix {
                label: "Insert References section".to_string(),
                kind: SuggestedFixKind::AssistedFix,
                replacement_preview: Some("== References ==\n{{Reflist}}".to_string()),
                patch: None,
            }],
        },
        safe_fixes: Vec::new(),
    });
}

fn lint_missing_reflist(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let Some(section) = document.find_section("References") else {
        return;
    };
    let Some(references_template) = resources.overlay.authoring.references_template.as_deref()
    else {
        return;
    };
    if section_body_contains_template(section, &document.templates, references_template) {
        return;
    }

    let edit = TextEdit {
        start: section.body_start,
        end: section.body_start,
        replacement: "{{Reflist}}\n".to_string(),
    };
    let safe_fixes = vec![SafeFixEdit {
        rule_id: "structure.require_reflist".to_string(),
        label: "Insert {{Reflist}} into References section".to_string(),
        line: section
            .heading
            .as_ref()
            .map(|heading| heading.line.saturating_add(1)),
        edit: edit.clone(),
    }];
    let suggested_fixes = vec![safe_fix_for_edit(
        document,
        &edit,
        "Insert {{Reflist}} into References section",
    )];

    matches.push(IssueMatch {
        issue: ArticleLintIssue {
            rule_id: "structure.require_reflist".to_string(),
            severity: ArticleLintSeverity::Error,
            message: "References section does not render citations with {{Reflist}}.".to_string(),
            span: section
                .heading
                .as_ref()
                .and_then(|heading| document.span_for_range(heading.start, heading.end)),
            evidence: Some(make_content_preview(&section.body_text, 96)),
            suggested_remediation: Some(
                "Render inline citations in the References section with {{Reflist}}.".to_string(),
            ),
            suggested_fixes,
        },
        safe_fixes,
    });
}

fn lint_citation_after_punctuation(
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

fn lint_curly_quotes(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
    let mut grouped = BTreeMap::<usize, Vec<(usize, char)>>::new();
    for (offset, ch) in document.content.char_indices() {
        if !matches!(ch, '“' | '”' | '‘' | '’') {
            continue;
        }
        if let Some(line) = document.line_for_offset(offset) {
            grouped.entry(line.number).or_default().push((offset, ch));
        }
    }

    for (line_number, replacements) in grouped {
        let Some(line) = document
            .lines
            .iter()
            .find(|candidate| candidate.number == line_number)
        else {
            continue;
        };
        let mut safe_fixes = Vec::new();
        let mut suggested_fixes = Vec::new();
        for (offset, ch) in replacements {
            let replacement = straight_quote_for(ch);
            let edit = TextEdit {
                start: offset,
                end: offset + ch.len_utf8(),
                replacement: replacement.to_string(),
            };
            safe_fixes.push(SafeFixEdit {
                rule_id: "style.curly_quotes".to_string(),
                label: "Replace curly quotes with straight quotes".to_string(),
                line: Some(line.number),
                edit: edit.clone(),
            });
            suggested_fixes.push(safe_fix_for_edit(
                document,
                &edit,
                "Replace curly quotes with straight quotes",
            ));
        }
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "style.curly_quotes".to_string(),
                severity: ArticleLintSeverity::Warning,
                message: "Article contains curly quotes or apostrophes.".to_string(),
                span: document.span_for_line(line),
                evidence: Some(line.text.clone()),
                suggested_remediation: Some(
                    "Use straight ASCII quotes in article prose and citations.".to_string(),
                ),
                suggested_fixes,
            },
            safe_fixes,
        });
    }
}

fn lint_placeholder_fragments(
    document: &ParsedArticleDocument,
    resources: &LoadedResources,
    matches: &mut Vec<IssueMatch>,
) {
    let lowered = document.content.to_ascii_lowercase();
    for fragment in &resources.overlay.lint.forbid_placeholder_fragments {
        let lowered_fragment = fragment.to_ascii_lowercase();
        let Some(start) = lowered.find(&lowered_fragment) else {
            continue;
        };
        matches.push(IssueMatch {
            issue: ArticleLintIssue {
                rule_id: "style.placeholder_fragment".to_string(),
                severity: ArticleLintSeverity::Error,
                message: "Draft still contains placeholder or system-artifact text.".to_string(),
                span: document.span_for_range(start, start + fragment.len()),
                evidence: Some(fragment.clone()),
                suggested_remediation: Some(
                    "Delete placeholder text and replace it with sourced article content."
                        .to_string(),
                ),
                suggested_fixes: Vec::new(),
            },
            safe_fixes: Vec::new(),
        });
    }
}

fn lint_citation_needed(document: &ParsedArticleDocument, matches: &mut Vec<IssueMatch>) {
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

fn lint_remilia_parent_group(
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

fn lint_template_availability(
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

fn lint_red_links_in_see_also(
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

fn lint_capability_rules(
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

fn lint_graph_rules(
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

fn build_report(
    document: &ParsedArticleDocument,
    profile_id: &str,
    resources: &LoadedResources,
    matches: Vec<IssueMatch>,
) -> ArticleLintReport {
    let issues = matches
        .into_iter()
        .map(|item| item.issue)
        .collect::<Vec<_>>();
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == ArticleLintSeverity::Error)
        .count();
    let warnings = issues
        .iter()
        .filter(|issue| issue.severity == ArticleLintSeverity::Warning)
        .count();
    let suggestions = issues
        .iter()
        .filter(|issue| issue.severity == ArticleLintSeverity::Suggestion)
        .count();

    ArticleLintReport {
        schema_version: ARTICLE_LINT_SCHEMA_VERSION.to_string(),
        profile_id: profile_id.to_string(),
        relative_path: document.relative_path.clone(),
        title: document.title.clone(),
        namespace: document.namespace.clone(),
        issue_count: issues.len(),
        errors,
        warnings,
        suggestions,
        resources: ArticleLintResourcesStatus {
            capabilities_loaded: resources.capabilities.is_some(),
            template_catalog_loaded: resources.template_catalog.is_some(),
            index_ready: resources.index_connection.is_some(),
            graph_ready: resources.index_connection.is_some(),
        },
        issues,
    }
}

fn collect_safe_fixes(matches: &[IssueMatch]) -> Vec<SafeFixEdit> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for issue in matches {
        for fix in &issue.safe_fixes {
            let key = (
                fix.edit.start,
                fix.edit.end,
                fix.edit.replacement.clone(),
                fix.rule_id.clone(),
                fix.label.clone(),
            );
            if seen.insert(key) {
                out.push(fix.clone());
            }
        }
    }
    out.sort_by(|left, right| {
        left.edit
            .start
            .cmp(&right.edit.start)
            .then(left.edit.end.cmp(&right.edit.end))
            .then(left.label.cmp(&right.label))
    });
    out
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

fn parse_markdown_heading(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim();
    let count = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(2..=6).contains(&count) {
        return None;
    }
    let text = trimmed[count..].trim();
    if text.is_empty() {
        return None;
    }
    Some((u8::try_from(count).unwrap_or(6), text.to_string()))
}

fn line_has_short_description(line: &str) -> bool {
    let trimmed = line.trim();
    let lowered = trimmed.to_ascii_lowercase();
    lowered.starts_with("{{shortdesc:")
        || lowered.starts_with("{{short description|")
        || lowered.starts_with("{{short description |")
}

fn preferred_short_description_snippet(overlay: &ProfileOverlay) -> String {
    if overlay
        .authoring
        .short_description_forms
        .iter()
        .any(|form| form.eq_ignore_ascii_case("magic_word:SHORTDESC"))
    {
        return "{{SHORTDESC:Brief one-line description}}".to_string();
    }
    "{{Short description|Brief one-line description}}".to_string()
}

fn section_body_contains_template(
    section: &ArticleSection,
    templates: &[TemplateOccurrence],
    template_title: &str,
) -> bool {
    templates.iter().any(|template| {
        template.start >= section.body_start
            && template.end <= section.body_end
            && template.template_title.eq_ignore_ascii_case(template_title)
    })
}

fn canonical_sentence_case_heading(heading: &str) -> Option<String> {
    let normalized = normalize_spaces(heading);
    if normalized.is_empty() {
        return None;
    }
    for (wrong, right) in COMMON_SENTENCE_CASE_HEADINGS {
        if normalized.eq_ignore_ascii_case(wrong) {
            return Some((*right).to_string());
        }
    }

    let words = normalized
        .split_whitespace()
        .filter(|word| word.chars().any(|ch| ch.is_ascii_alphabetic()))
        .collect::<Vec<_>>();
    if words.len() < 3 {
        return None;
    }
    if words
        .iter()
        .skip(1)
        .any(|word| is_stopword(word) && is_title_case_word(word))
    {
        return Some(lowercase_heading_tail(&normalized));
    }
    if words
        .iter()
        .skip(1)
        .filter(|word| is_title_case_word(word))
        .count()
        >= 2
    {
        return Some(lowercase_heading_tail(&normalized));
    }
    None
}

fn safe_heading_rewrite_available(original: &str, canonical: &str) -> bool {
    COMMON_SENTENCE_CASE_HEADINGS.iter().any(|(wrong, _)| {
        original.eq_ignore_ascii_case(wrong) || canonical.eq_ignore_ascii_case(wrong)
    })
}

fn lowercase_heading_tail(value: &str) -> String {
    let mut out = Vec::new();
    for (index, word) in value.split_whitespace().enumerate() {
        if index == 0
            || word
                .chars()
                .all(|ch| !ch.is_ascii_alphabetic() || ch.is_ascii_uppercase())
        {
            out.push(word.to_string());
        } else {
            out.push(word.to_ascii_lowercase());
        }
    }
    out.join(" ")
}

fn is_title_case_word(word: &str) -> bool {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_uppercase()
        && chars.clone().any(|ch| ch.is_ascii_lowercase())
        && chars.all(|ch| !ch.is_ascii_alphabetic() || ch.is_ascii_lowercase())
}

fn is_stopword(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "and" | "of" | "the" | "for" | "in" | "to" | "on" | "with"
    )
}

fn straight_quote_for(ch: char) -> char {
    match ch {
        '“' | '”' => '"',
        '‘' | '’' => '\'',
        _ => ch,
    }
}

fn safe_fix_for_edit(
    document: &ParsedArticleDocument,
    edit: &TextEdit,
    label: &str,
) -> SuggestedFix {
    let patch = patch_preview(document, edit);
    SuggestedFix {
        label: label.to_string(),
        kind: SuggestedFixKind::SafeAutofix,
        replacement_preview: Some(make_content_preview(&edit.replacement, 96)),
        patch: Some(patch),
    }
}

fn patch_preview(document: &ParsedArticleDocument, edit: &TextEdit) -> String {
    let line = document
        .line_for_offset(edit.start)
        .map(|line| line.number)
        .unwrap_or(1);
    let before = if edit.start == edit.end {
        "<insert>".to_string()
    } else {
        make_content_preview(&document.content[edit.start..edit.end], 96)
    };
    let after = if edit.replacement.is_empty() {
        "<delete>".to_string()
    } else {
        make_content_preview(&edit.replacement, 96)
    };
    format!("@@ line {line} @@\n- {before}\n+ {after}")
}

fn split_template_segments(inner: &str) -> Vec<String> {
    let bytes = inner.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut segment_start = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;

    while cursor < bytes.len() {
        if cursor + 1 < bytes.len() && bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            template_depth += 1;
            cursor += 2;
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b'}' && bytes[cursor + 1] == b'}' {
            template_depth = template_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
            link_depth += 1;
            cursor += 2;
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b']' && bytes[cursor + 1] == b']' {
            link_depth = link_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'|' && template_depth == 0 && link_depth == 0 {
            out.push(inner[segment_start..cursor].to_string());
            cursor += 1;
            segment_start = cursor;
            continue;
        }
        cursor += 1;
    }

    out.push(inner[segment_start..].to_string());
    out
}

fn split_once_top_level_equals(value: &str) -> Option<(String, String)> {
    let bytes = value.as_bytes();
    let mut cursor = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;

    while cursor < bytes.len() {
        if cursor + 1 < bytes.len() && bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            template_depth += 1;
            cursor += 2;
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b'}' && bytes[cursor + 1] == b'}' {
            template_depth = template_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
            link_depth += 1;
            cursor += 2;
            continue;
        }
        if cursor + 1 < bytes.len() && bytes[cursor] == b']' && bytes[cursor + 1] == b']' {
            link_depth = link_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'=' && template_depth == 0 && link_depth == 0 {
            return Some((
                value[..cursor].trim().to_string(),
                value[cursor + 1..].trim().to_string(),
            ));
        }
        cursor += 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use crate::filesystem::ScanOptions;
    use crate::knowledge::content_index::rebuild_index;
    use crate::runtime::{ResolvedPaths, ValueSource};

    use super::*;

    fn paths(project_root: &Path) -> ResolvedPaths {
        let state_dir = project_root.join(".wikitool");
        let data_dir = state_dir.join("data");
        fs::create_dir_all(project_root.join("wiki_content/Main")).expect("wiki content");
        fs::create_dir_all(project_root.join("templates")).expect("templates");
        fs::create_dir_all(&data_dir).expect("data");
        fs::create_dir_all(project_root.join("tools/wikitool/ai-pack/llm_instructions"))
            .expect("instructions");
        ResolvedPaths {
            project_root: project_root.to_path_buf(),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir,
            data_dir: data_dir.clone(),
            db_path: data_dir.join("wikitool.db"),
            config_path: project_root.join(".wikitool/config.toml"),
            parser_config_path: project_root.join(".wikitool/parser-config.json"),
            root_source: ValueSource::Default,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, content).expect("write file");
    }

    fn write_instruction_sources(paths: &ResolvedPaths) {
        write_file(
            &paths
                .project_root
                .join("tools/wikitool/ai-pack/llm_instructions/article_structure.md"),
            "{{SHORTDESC:Example}}\n{{Article quality|unverified}}\n== References ==\n{{Reflist}}\nparent_group = Remilia",
        );
        write_file(
            &paths
                .project_root
                .join("tools/wikitool/ai-pack/llm_instructions/style_rules.md"),
            "**Never use:**\n- \"stands as\"\n### No placeholder content\n- Never output: `INSERT_SOURCE_URL`\n### No system artifacts\n- Never output: `contentReference[oaicite:0]`\nStraight quotes only",
        );
        write_file(
            &paths
                .project_root
                .join("tools/wikitool/ai-pack/llm_instructions/writing_guide.md"),
            "raw MediaWiki wikitext\nNever output Markdown\nUse 2-4 categories per article\n[[Category:Remilia]]\n{{Article quality|unverified}}\nparent_group = Remilia\n### Citation templates\n```wikitext\n{{Cite web|url=}}\n```\n## 6. Infobox selection\n| Subject type | Infobox |\n|---|---|\n| NFT Collection | `{{Infobox NFT collection}}` |\n",
        );
    }

    fn write_common_templates(paths: &ResolvedPaths) {
        write_file(
            &paths
                .templates_dir
                .join("misc")
                .join("Template_Article_quality.wiki"),
            "<includeonly>{{{1|unverified}}}</includeonly>",
        );
        write_file(
            &paths
                .templates_dir
                .join("misc")
                .join("Template_Reflist.wiki"),
            "<references />",
        );
        write_file(
            &paths
                .templates_dir
                .join("infobox")
                .join("Template_Infobox_NFT_collection.wiki"),
            "<includeonly>{{{name|}}} {{{parent_group|}}}</includeonly>",
        );
    }

    fn write_capability_manifest(paths: &ResolvedPaths, manifest: &WikiCapabilityManifest) {
        let connection = open_initialized_database_connection(&paths.db_path).expect("db");
        connection
            .execute(
                "INSERT INTO knowledge_artifacts (
                    artifact_key,
                    artifact_kind,
                    profile,
                    schema_generation,
                    built_at_unix,
                    row_count,
                    metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    "wiki_capabilities:test",
                    "wiki_capabilities",
                    Some("wiki.remilia.org"),
                    KNOWLEDGE_GENERATION,
                    1i64,
                    1i64,
                    serde_json::to_string(manifest).expect("manifest json"),
                ],
            )
            .expect("insert manifest");
    }

    fn has_rule(report: &ArticleLintReport, rule_id: &str) -> bool {
        report.issues.iter().any(|issue| issue.rule_id == rule_id)
    }

    #[test]
    fn detects_markdown_heading_and_applies_safe_fix() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n## History\nText.\n\n== References ==\n{{Reflist}}\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "structure.markdown_heading"));

        let fixed =
            fix_article(&paths, &article_path, None, ArticleFixApplyMode::Safe).expect("safe fix");
        assert!(fixed.changed);
        let content = fs::read_to_string(&article_path).expect("read article");
        assert!(content.contains("== History =="));
    }

    #[test]
    fn detects_sentence_case_heading() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n== Early Life ==\nText.\n\n== References ==\n{{Reflist}}\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "style.sentence_case_heading"));
    }

    #[test]
    fn detects_missing_short_description() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{Article quality|unverified}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "structure.require_short_description"));
    }

    #[test]
    fn inserts_missing_article_quality_banner_with_safe_fix() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
        );

        let fixed =
            fix_article(&paths, &article_path, None, ArticleFixApplyMode::Safe).expect("safe fix");
        assert!(fixed.changed);
        let content = fs::read_to_string(&article_path).expect("read article");
        assert!(content.contains("{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n"));
    }

    #[test]
    fn detects_missing_reflist_and_applies_safe_fix() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page.<ref>{{Cite web|title=Source}}</ref>\n\n== References ==\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "structure.require_reflist"));

        let fixed =
            fix_article(&paths, &article_path, None, ArticleFixApplyMode::Safe).expect("safe fix");
        assert!(fixed.changed);
        let content = fs::read_to_string(&article_path).expect("read article");
        assert!(content.contains("== References ==\n{{Reflist}}\n"));
    }

    #[test]
    fn inserts_reflist_before_reference_section_trailing_categories() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page.<ref>{{Cite web|title=Source}}</ref>\n\n== References ==\n[[Category:Ideas and Concepts]]\n",
        );

        let fixed =
            fix_article(&paths, &article_path, None, ArticleFixApplyMode::Safe).expect("safe fix");
        assert!(fixed.changed);
        let content = fs::read_to_string(&article_path).expect("read article");
        assert!(content.contains("== References ==\n{{Reflist}}\n[[Category:Ideas and Concepts]]"));
    }

    #[test]
    fn detects_citation_after_punctuation_and_applies_safe_fix() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page<ref>{{Cite web|title=Source}}</ref>.\n\n== References ==\n{{Reflist}}\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "citation.after_punctuation"));

        let fixed =
            fix_article(&paths, &article_path, None, ArticleFixApplyMode::Safe).expect("safe fix");
        assert!(fixed.changed);
        let content = fs::read_to_string(&article_path).expect("read article");
        assert!(content.contains("page.<ref>{{Cite web|title=Source}}</ref>"));
    }

    #[test]
    fn clustered_citations_move_punctuation_before_the_whole_cluster() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page<ref name=\"a\">{{Cite web|title=Source A}}</ref><ref name=\"b\">{{Cite web|title=Source B}}</ref>.\n\n== References ==\n{{Reflist}}\n",
        );

        let fixed =
            fix_article(&paths, &article_path, None, ArticleFixApplyMode::Safe).expect("safe fix");
        assert!(fixed.changed);
        let content = fs::read_to_string(&article_path).expect("read article");
        assert!(content.contains("page.<ref name=\"a\">{{Cite web|title=Source A}}</ref><ref name=\"b\">{{Cite web|title=Source B}}</ref>"));
        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(!has_rule(&report, "citation.after_punctuation"));
    }

    #[test]
    fn detects_remilia_parent_group_rule() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths
            .wiki_content_dir
            .join("Main")
            .join("Milady_Maker.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{Infobox NFT collection\n| name = Milady Maker\n| creator = Remilia\n}}\n\n'''Milady Maker''' is a page.\n\n== References ==\n{{Reflist}}\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "profile.remilia_parent_group"));
    }

    #[test]
    fn rejects_citation_needed_templates() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page. {{Citation needed}}\n\n== References ==\n{{Reflist}}\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "profile.no_citation_needed"));
    }

    #[test]
    fn detects_red_links_in_see_also() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        write_file(
            &paths.wiki_content_dir.join("Main").join("Existing.wiki"),
            "{{SHORTDESC:Existing}}\n{{Article quality|unverified}}\n\n'''Existing''' is a page.\n\n== References ==\n{{Reflist}}\n",
        );
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n'''Alpha''' is a page.\n\n== See also ==\n* [[Existing]]\n* [[Missing]]\n\n== References ==\n{{Reflist}}\n",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "integration.red_link_in_see_also"));
    }

    #[test]
    fn detects_unavailable_templates_against_local_catalog() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n{{Mystery box|value=1}}\n\n'''Alpha''' is a page.\n\n== References ==\n{{Reflist}}\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "template.unavailable"));
    }

    #[test]
    fn detects_unsupported_extension_tags_from_capabilities() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        write_capability_manifest(
            &paths,
            &WikiCapabilityManifest {
                schema_version: "wiki_capabilities_v1".to_string(),
                wiki_id: "wiki.remilia.org".to_string(),
                wiki_url: "https://wiki.remilia.org".to_string(),
                api_url: "https://wiki.remilia.org/api.php".to_string(),
                rest_url: None,
                article_path: "/$1".to_string(),
                mediawiki_version: Some("1.44.3".to_string()),
                namespaces: Vec::new(),
                extensions: Vec::new(),
                parser_extension_tags: vec!["math".to_string()],
                parser_function_hooks: Vec::new(),
                special_pages: Vec::new(),
                search_backend_hint: None,
                has_visual_editor: false,
                has_templatedata: false,
                has_citoid: false,
                has_cargo: false,
                has_page_forms: false,
                has_short_description: true,
                has_scribunto: false,
                has_timed_media_handler: false,
                supports_parse_api_html: true,
                supports_rest_html: false,
                rest_html_path_template: None,
                refreshed_at: "1".to_string(),
            },
        );
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n<tabber>\n|-|One=Alpha\n</tabber>\n\n== References ==\n{{Reflist}}\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "capability.unsupported_extension_tag"));
    }

    #[test]
    fn detects_suspicious_html_tags_even_when_they_are_not_known_extensions() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        write_instruction_sources(&paths);
        write_common_templates(&paths);
        write_capability_manifest(
            &paths,
            &WikiCapabilityManifest {
                schema_version: "wiki_capabilities_v1".to_string(),
                wiki_id: "wiki.remilia.org".to_string(),
                wiki_url: "https://wiki.remilia.org".to_string(),
                api_url: "https://wiki.remilia.org/api.php".to_string(),
                rest_url: None,
                article_path: "/$1".to_string(),
                mediawiki_version: Some("1.44.3".to_string()),
                namespaces: Vec::new(),
                extensions: Vec::new(),
                parser_extension_tags: vec!["<ref>".to_string(), "<references>".to_string()],
                parser_function_hooks: Vec::new(),
                special_pages: Vec::new(),
                search_backend_hint: None,
                has_visual_editor: false,
                has_templatedata: false,
                has_citoid: false,
                has_cargo: false,
                has_page_forms: false,
                has_short_description: true,
                has_scribunto: false,
                has_timed_media_handler: false,
                supports_parse_api_html: true,
                supports_rest_html: false,
                rest_html_path_template: None,
                refreshed_at: "1".to_string(),
            },
        );
        let article_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        write_file(
            &article_path,
            "{{SHORTDESC:Alpha}}\n{{Article quality|unverified}}\n\n<blink>Alpha</blink>\n\n== References ==\n{{Reflist}}\n",
        );

        let report = lint_article(&paths, &article_path, None).expect("lint");
        assert!(has_rule(&report, "capability.unsupported_extension_tag"));
    }
}
