use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::article_lint::model::TextSpan;
use crate::content_store::parsing::{
    canonical_template_title, find_closing_html_tag, normalize_spaces, parse_heading_line,
    parse_open_tag, starts_with_html_tag,
};
use crate::filesystem::{namespace_from_title, relative_path_to_title, validate_scoped_path};
use crate::runtime::ResolvedPaths;
use crate::support::{normalize_path, parse_redirect};

#[derive(Debug, Clone)]
pub(super) struct LineRecord {
    pub(super) number: usize,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) text: String,
}

#[derive(Debug, Clone)]
pub(super) struct HeadingRecord {
    pub(super) level: u8,
    pub(super) text: String,
    pub(super) line: usize,
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ArticleSection {
    pub(super) heading: Option<HeadingRecord>,
    pub(super) body_start: usize,
    pub(super) body_end: usize,
    pub(super) body_text: String,
}

#[derive(Debug, Clone)]
pub(super) struct TemplateOccurrence {
    pub(super) template_title: String,
    pub(super) parameter_keys: Vec<String>,
    pub(super) raw_wikitext: String,
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Debug, Clone)]
pub(super) struct RefOccurrence {
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ParserTagOccurrence {
    pub(super) tag_name: String,
    pub(super) start: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedArticleDocument {
    pub(super) relative_path: String,
    pub(super) title: String,
    pub(super) namespace: String,
    pub(super) content: String,
    pub(super) is_redirect: bool,
    pub(super) redirect_target: Option<String>,
    pub(super) lines: Vec<LineRecord>,
    pub(super) sections: Vec<ArticleSection>,
    pub(super) templates: Vec<TemplateOccurrence>,
    pub(super) references: Vec<RefOccurrence>,
    pub(super) parser_tags: Vec<ParserTagOccurrence>,
}

impl ParsedArticleDocument {
    pub(super) fn span_for_range(&self, start: usize, end: usize) -> Option<TextSpan> {
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

    pub(super) fn span_for_line(&self, line: &LineRecord) -> Option<TextSpan> {
        Some(TextSpan {
            line: line.number,
            column: 1,
            end_line: Some(line.number),
            end_column: Some(line.text.chars().count().saturating_add(1)),
        })
    }

    pub(super) fn line_for_offset(&self, offset: usize) -> Option<&LineRecord> {
        self.lines
            .iter()
            .find(|line| offset >= line.start && offset <= line.end)
            .or_else(|| self.lines.last())
    }

    pub(super) fn column_for_offset(&self, line: &LineRecord, offset: usize) -> usize {
        line.text[..offset.saturating_sub(line.start).min(line.text.len())]
            .chars()
            .count()
            .saturating_add(1)
    }

    pub(super) fn first_nonblank_line(&self) -> Option<&LineRecord> {
        self.lines.iter().find(|line| !line.text.trim().is_empty())
    }

    pub(super) fn top_nonblank_lines(&self, limit: usize) -> Vec<&LineRecord> {
        self.lines
            .iter()
            .filter(|line| !line.text.trim().is_empty())
            .take(limit)
            .collect()
    }

    pub(super) fn find_section(&self, heading: &str) -> Option<&ArticleSection> {
        self.sections.iter().find(|section| {
            section
                .heading
                .as_ref()
                .map(|candidate| candidate.text.eq_ignore_ascii_case(heading))
                .unwrap_or(false)
        })
    }
}

pub(super) fn load_article_document(
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
