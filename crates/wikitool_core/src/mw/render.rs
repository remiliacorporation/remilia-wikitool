#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::MediaWikiClient;

#[derive(Debug, Clone)]
pub struct RenderedPageHtml {
    pub title: String,
    pub display_title: Option<String>,
    pub revision_id: Option<i64>,
    pub html: String,
}

#[derive(Debug, Clone)]
pub struct RenderCheckOptions {
    pub title: String,
    pub scope_class: Option<String>,
    pub expected_scope_count: Option<usize>,
    pub require_interactive_link: bool,
    pub required_href_substrings: Vec<String>,
    pub required_link_classes: Vec<String>,
    pub forbid_literal_wikilinks: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderCheckReport {
    pub schema_version: &'static str,
    pub status: &'static str,
    pub title: String,
    pub display_title: Option<String>,
    pub revision_id: Option<i64>,
    pub scope_class: Option<String>,
    pub expected_scope_count: Option<usize>,
    pub scope_count: usize,
    pub require_interactive_link: bool,
    pub required_href_substrings: Vec<String>,
    pub required_link_classes: Vec<String>,
    pub forbid_literal_wikilinks: bool,
    pub literal_wikilink_count: usize,
    pub parser_error_count: usize,
    pub issue_count: usize,
    pub issues: Vec<RenderCheckIssue>,
    pub scopes: Vec<RenderedScopeReport>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderCheckIssue {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderedScopeReport {
    pub index: usize,
    pub tag: String,
    pub interactive_link_count: usize,
    pub interactive_hrefs: Vec<String>,
    pub interactive_link_classes: Vec<String>,
    pub literal_wikilinks: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ParseResponse {
    #[serde(default)]
    parse: Option<ParsePayload>,
}

#[derive(Debug, Deserialize, Default)]
struct ParsePayload {
    title: Option<String>,
    displaytitle: Option<String>,
    revid: Option<i64>,
    text: Option<ParseText>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ParseText {
    Html(String),
    StarKey {
        #[serde(default, rename = "*")]
        html: String,
    },
}

impl ParseText {
    fn into_html(self) -> String {
        match self {
            Self::Html(html) => html,
            Self::StarKey { html } => html,
        }
    }
}

pub(crate) fn render_page_html(
    client: &mut MediaWikiClient,
    title: &str,
) -> Result<Option<RenderedPageHtml>> {
    let response = client.request_json_get(&[
        ("action", "parse".to_string()),
        ("page", title.to_string()),
        ("prop", "text|displaytitle|revid".to_string()),
    ])?;
    decode_rendered_page_payload(response, title)
}

pub fn render_check_page(
    client: &mut MediaWikiClient,
    options: &RenderCheckOptions,
) -> Result<RenderCheckReport> {
    validate_render_check_options(options)?;
    let request_count_before = client.request_count;
    let rendered = render_page_html(client, &options.title)?
        .with_context(|| format!("page did not return rendered HTML: {}", options.title))?;
    let mut report = analyze_rendered_page(&rendered, options);
    report.request_count = client.request_count.saturating_sub(request_count_before);
    Ok(report)
}

fn validate_render_check_options(options: &RenderCheckOptions) -> Result<()> {
    if options.title.trim().is_empty() {
        anyhow::bail!("render-check requires a non-empty title");
    }
    if options
        .scope_class
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        anyhow::bail!("--scope-class requires a non-empty CSS class");
    }
    if options.scope_class.is_none()
        && (options.expected_scope_count.is_some()
            || options.require_interactive_link
            || !options.required_href_substrings.is_empty()
            || !options.required_link_classes.is_empty())
    {
        anyhow::bail!("--expect-scopes and scoped link requirements require --scope-class");
    }
    if options
        .required_href_substrings
        .iter()
        .any(|value| value.is_empty())
    {
        anyhow::bail!("--require-href-contains values must be non-empty");
    }
    if options
        .required_link_classes
        .iter()
        .any(|value| value.trim().is_empty() || value.split_whitespace().count() != 1)
    {
        anyhow::bail!("--require-link-class values must be one non-empty CSS class");
    }
    Ok(())
}

fn analyze_rendered_page(
    rendered: &RenderedPageHtml,
    options: &RenderCheckOptions,
) -> RenderCheckReport {
    let analysis = scan_rendered_html(&rendered.html, options.scope_class.as_deref());
    let mut issues = Vec::new();

    if let Some(expected) = options.expected_scope_count
        && analysis.scopes.len() != expected
    {
        issues.push(RenderCheckIssue {
            code: "scope_count_mismatch".to_string(),
            message: format!(
                "expected {expected} elements with class `{}`, found {}",
                options.scope_class.as_deref().unwrap_or_default(),
                analysis.scopes.len()
            ),
            scope_index: None,
        });
    }

    if options.forbid_literal_wikilinks {
        for literal in &analysis.literal_wikilinks {
            issues.push(RenderCheckIssue {
                code: "literal_wikilink".to_string(),
                message: format!("rendered output contains literal wikitext: {literal}"),
                scope_index: literal.scope_index,
            });
        }
    }

    for error_class in &analysis.parser_error_classes {
        issues.push(RenderCheckIssue {
            code: "parser_error_markup".to_string(),
            message: format!("rendered output contains parser error class `{error_class}`"),
            scope_index: None,
        });
    }

    for scope in &analysis.scopes {
        if options.require_interactive_link && scope.interactive_hrefs.is_empty() {
            issues.push(RenderCheckIssue {
                code: "scope_missing_interactive_link".to_string(),
                message: "scope has no interactive link (crawler-only source links are excluded)"
                    .to_string(),
                scope_index: Some(scope.index),
            });
        }
        for required in &options.required_href_substrings {
            if !scope
                .interactive_hrefs
                .iter()
                .any(|href| href.contains(required))
            {
                issues.push(RenderCheckIssue {
                    code: "scope_missing_required_href".to_string(),
                    message: format!("scope has no interactive href containing `{required}`"),
                    scope_index: Some(scope.index),
                });
            }
        }
        for required in &options.required_link_classes {
            if !scope
                .interactive_link_classes
                .iter()
                .any(|class| class == required)
            {
                issues.push(RenderCheckIssue {
                    code: "scope_missing_required_link_class".to_string(),
                    message: format!("scope has no interactive link with class `{required}`"),
                    scope_index: Some(scope.index),
                });
            }
        }
    }

    let status = if issues.is_empty() { "clean" } else { "failed" };
    RenderCheckReport {
        schema_version: "render_check_v1",
        status,
        title: rendered.title.clone(),
        display_title: rendered.display_title.clone(),
        revision_id: rendered.revision_id,
        scope_class: options.scope_class.clone(),
        expected_scope_count: options.expected_scope_count,
        scope_count: analysis.scopes.len(),
        require_interactive_link: options.require_interactive_link,
        required_href_substrings: options.required_href_substrings.clone(),
        required_link_classes: options.required_link_classes.clone(),
        forbid_literal_wikilinks: options.forbid_literal_wikilinks,
        literal_wikilink_count: analysis.literal_wikilinks.len(),
        parser_error_count: analysis.parser_error_classes.len(),
        issue_count: issues.len(),
        issues,
        scopes: analysis.scopes,
        request_count: 0,
    }
}

#[derive(Debug)]
struct HtmlAnalysis {
    scopes: Vec<RenderedScopeReport>,
    literal_wikilinks: Vec<LiteralWikilink>,
    parser_error_classes: Vec<String>,
}

#[derive(Debug)]
struct LiteralWikilink {
    text: String,
    scope_index: Option<usize>,
}

impl std::fmt::Display for LiteralWikilink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.text)
    }
}

#[derive(Debug)]
struct OpenElement {
    name: String,
    scope_indices: Vec<usize>,
}

#[derive(Debug)]
struct ParsedTag {
    name: String,
    closing: bool,
    self_closing: bool,
    attributes: Vec<(String, String)>,
}

fn scan_rendered_html(html: &str, scope_class: Option<&str>) -> HtmlAnalysis {
    let mut analysis = HtmlAnalysis {
        scopes: Vec::new(),
        literal_wikilinks: Vec::new(),
        parser_error_classes: Vec::new(),
    };
    let mut stack: Vec<OpenElement> = Vec::new();
    let bytes = html.as_bytes();
    let mut cursor = 0;
    let mut text_start = 0;

    while cursor < bytes.len() {
        if bytes[cursor] != b'<' {
            cursor += 1;
            continue;
        }

        process_html_text(&html[text_start..cursor], &stack, &mut analysis);
        if html[cursor..].starts_with("<!--") {
            if let Some(relative_end) = html[cursor + 4..].find("-->") {
                cursor += 4 + relative_end + 3;
                text_start = cursor;
                continue;
            }
            break;
        }

        let Some(tag_end) = find_tag_end(html, cursor + 1) else {
            break;
        };
        let raw_tag = &html[cursor + 1..tag_end];
        if let Some(tag) = parse_html_tag(raw_tag) {
            if tag.closing {
                if let Some(position) = stack.iter().rposition(|open| open.name == tag.name) {
                    stack.truncate(position);
                }
            } else {
                let classes = attribute_value(&tag.attributes, "class")
                    .map(|value| value.split_whitespace().collect::<Vec<_>>())
                    .unwrap_or_default();
                for class in &classes {
                    if is_parser_error_class(class)
                        && !analysis
                            .parser_error_classes
                            .iter()
                            .any(|existing| existing == class)
                    {
                        analysis.parser_error_classes.push((*class).to_string());
                    }
                }

                let mut active_scopes = stack
                    .last()
                    .map(|open| open.scope_indices.clone())
                    .unwrap_or_default();
                if scope_class.is_some_and(|scope| classes.contains(&scope)) {
                    let index = analysis.scopes.len();
                    analysis.scopes.push(RenderedScopeReport {
                        index,
                        tag: tag.name.clone(),
                        interactive_link_count: 0,
                        interactive_hrefs: Vec::new(),
                        interactive_link_classes: Vec::new(),
                        literal_wikilinks: Vec::new(),
                    });
                    active_scopes.push(index);
                }

                if tag.name == "a" && !classes.contains(&"mw-file-source") {
                    let href = attribute_value(&tag.attributes, "href").unwrap_or_default();
                    if !href.is_empty() {
                        for index in &active_scopes {
                            let scope = &mut analysis.scopes[*index];
                            scope.interactive_link_count += 1;
                            if !scope.interactive_hrefs.iter().any(|value| value == href) {
                                scope.interactive_hrefs.push(href.to_string());
                            }
                            for class in &classes {
                                if !scope
                                    .interactive_link_classes
                                    .iter()
                                    .any(|value| value == class)
                                {
                                    scope.interactive_link_classes.push((*class).to_string());
                                }
                            }
                        }
                    }
                }

                if !tag.self_closing && !is_void_html_element(&tag.name) {
                    stack.push(OpenElement {
                        name: tag.name,
                        scope_indices: active_scopes,
                    });
                }
            }
        }
        cursor = tag_end + 1;
        text_start = cursor;
    }

    if text_start < html.len() {
        process_html_text(&html[text_start..], &stack, &mut analysis);
    }
    analysis
}

fn process_html_text(text: &str, stack: &[OpenElement], analysis: &mut HtmlAnalysis) {
    if text.is_empty()
        || stack
            .last()
            .is_some_and(|open| matches!(open.name.as_str(), "style" | "script"))
    {
        return;
    }
    let decoded = decode_html_text(text);
    let literals = literal_wikilink_snippets(&decoded);
    if literals.is_empty() {
        return;
    }
    let active_scopes = stack
        .last()
        .map(|open| open.scope_indices.as_slice())
        .unwrap_or_default();
    for literal in literals {
        let primary_scope = active_scopes.last().copied();
        analysis.literal_wikilinks.push(LiteralWikilink {
            text: literal.clone(),
            scope_index: primary_scope,
        });
        for index in active_scopes {
            analysis.scopes[*index]
                .literal_wikilinks
                .push(literal.clone());
        }
    }
}

fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut quote = None;
    let mut cursor = start;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' if quote.is_none() => quote = Some(bytes[cursor]),
            value if quote == Some(value) => quote = None,
            b'>' if quote.is_none() => return Some(cursor),
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn parse_html_tag(raw: &str) -> Option<ParsedTag> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('!') || raw.starts_with('?') {
        return None;
    }
    let closing = raw.starts_with('/');
    let body = if closing { raw[1..].trim_start() } else { raw };
    let bytes = body.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() && is_html_name_byte(bytes[cursor]) {
        cursor += 1;
    }
    if cursor == 0 {
        return None;
    }
    let name = body[..cursor].to_ascii_lowercase();
    let self_closing = body.trim_end().ends_with('/');
    let attributes = if closing {
        Vec::new()
    } else {
        parse_html_attributes(&body[cursor..])
    };
    Some(ParsedTag {
        name,
        closing,
        self_closing,
        attributes,
    })
}

fn parse_html_attributes(raw: &str) -> Vec<(String, String)> {
    let bytes = raw.as_bytes();
    let mut attributes = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        while cursor < bytes.len() && (bytes[cursor].is_ascii_whitespace() || bytes[cursor] == b'/')
        {
            cursor += 1;
        }
        let name_start = cursor;
        while cursor < bytes.len()
            && !bytes[cursor].is_ascii_whitespace()
            && !matches!(bytes[cursor], b'=' | b'/')
        {
            cursor += 1;
        }
        if cursor == name_start {
            break;
        }
        let name = raw[name_start..cursor].to_ascii_lowercase();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let mut value = String::new();
        if cursor < bytes.len() && bytes[cursor] == b'=' {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && matches!(bytes[cursor], b'\'' | b'"') {
                let quote = bytes[cursor];
                cursor += 1;
                let value_start = cursor;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                value = raw[value_start..cursor].to_string();
                if cursor < bytes.len() {
                    cursor += 1;
                }
            } else {
                let value_start = cursor;
                while cursor < bytes.len()
                    && !bytes[cursor].is_ascii_whitespace()
                    && bytes[cursor] != b'/'
                {
                    cursor += 1;
                }
                value = raw[value_start..cursor].to_string();
            }
        }
        attributes.push((name, value));
    }
    attributes
}

fn attribute_value<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn is_html_name_byte(value: u8) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, b':' | b'-')
}

fn is_void_html_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_parser_error_class(class: &str) -> bool {
    matches!(class, "error" | "errorbox" | "mw-error") || class.ends_with("-error")
}

fn decode_html_text(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'&'
            && let Some(relative_end) = text[cursor + 1..].find(';')
            && relative_end <= 12
        {
            let end = cursor + 1 + relative_end;
            let entity = &text[cursor + 1..end];
            if let Some(value) = decode_html_entity(entity) {
                decoded.push(value);
                cursor = end + 1;
                continue;
            }
        }
        let value = text[cursor..].chars().next().unwrap_or_default();
        decoded.push(value);
        cursor += value.len_utf8();
    }
    decoded
}

fn decode_html_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some(' '),
        value if value.starts_with("#x") || value.starts_with("#X") => {
            u32::from_str_radix(&value[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        value if value.starts_with('#') => value[1..].parse().ok().and_then(char::from_u32),
        _ => None,
    }
}

fn literal_wikilink_snippets(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut snippets = Vec::new();
    let mut cursor = 0;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] != b'[' || bytes[cursor + 1] != b'[' {
            cursor += 1;
            continue;
        }
        let start = cursor;
        cursor += 2;
        while cursor + 1 < bytes.len() && !(bytes[cursor] == b']' && bytes[cursor + 1] == b']') {
            cursor += 1;
        }
        let end = if cursor + 1 < bytes.len() {
            cursor + 2
        } else {
            text.len()
        };
        let normalized = text[start..end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        snippets.push(normalized.chars().take(160).collect());
        cursor = end;
    }
    snippets
}

pub(crate) fn decode_rendered_page_payload(
    response: Value,
    requested_title: &str,
) -> Result<Option<RenderedPageHtml>> {
    let parsed: ParseResponse =
        serde_json::from_value(response).context("failed to decode parse API response")?;
    let payload = match parsed.parse {
        Some(payload) => payload,
        None => return Ok(None),
    };
    let html = payload
        .text
        .map(ParseText::into_html)
        .unwrap_or_default()
        .trim()
        .to_string();
    if html.is_empty() {
        return Ok(None);
    }

    Ok(Some(RenderedPageHtml {
        title: payload.title.unwrap_or_else(|| requested_title.to_string()),
        display_title: normalize_optional_string(payload.displaytitle),
        revision_id: payload.revid,
        html,
    }))
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        RenderCheckOptions, RenderedPageHtml, analyze_rendered_page, decode_rendered_page_payload,
    };

    #[test]
    fn decodes_rendered_page_metadata() {
        let rendered = decode_rendered_page_payload(
            json!({
                "parse": {
                    "title": "Main Page",
                    "displaytitle": "<i>Main Page</i>",
                    "revid": 42,
                    "text": {
                        "*": "<p>Hello</p>"
                    }
                }
            }),
            "Main Page",
        )
        .expect("parse response should decode")
        .expect("rendered page should be present");

        assert_eq!(rendered.title, "Main Page");
        assert_eq!(rendered.display_title.as_deref(), Some("<i>Main Page</i>"));
        assert_eq!(rendered.revision_id, Some(42));
        assert_eq!(rendered.html, "<p>Hello</p>");
    }

    #[test]
    fn decodes_star_key_rendered_html() {
        let rendered = decode_rendered_page_payload(
            json!({
                "parse": {
                    "title": "Main Page",
                    "displaytitle": "Main Page",
                    "revid": 43,
                    "text": "<p>Hello v2</p>"
                }
            }),
            "Main Page",
        )
        .expect("parse response should decode")
        .expect("rendered page should be present");

        assert_eq!(rendered.revision_id, Some(43));
        assert_eq!(rendered.html, "<p>Hello v2</p>");
    }

    fn options(scope_class: &str) -> RenderCheckOptions {
        RenderCheckOptions {
            title: "Example".to_string(),
            scope_class: Some(scope_class.to_string()),
            expected_scope_count: Some(1),
            require_interactive_link: true,
            required_href_substrings: Vec::new(),
            required_link_classes: Vec::new(),
            forbid_literal_wikilinks: true,
        }
    }

    fn rendered(html: &str) -> RenderedPageHtml {
        RenderedPageHtml {
            title: "Example".to_string(),
            display_title: None,
            revision_id: Some(7),
            html: html.to_string(),
        }
    }

    #[test]
    fn render_check_rejects_literal_wikilinks_inside_scope() {
        let report = analyze_rendered_page(
            &rendered(
                r#"<div class="trait-item"><a href="/Trait">image</a><div>[[Trait|]]</div></div>"#,
            ),
            &options("trait-item"),
        );

        assert_eq!(report.status, "failed");
        assert_eq!(report.literal_wikilink_count, 1);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "literal_wikilink" && issue.scope_index == Some(0))
        );
    }

    #[test]
    fn render_check_excludes_crawler_source_links() {
        let report = analyze_rendered_page(
            &rendered(
                r#"<span class="trait-infobox"><a href="/images/trait.png" class="mw-file-source">source</a></span>"#,
            ),
            &options("trait-infobox"),
        );

        assert_eq!(report.status, "failed");
        assert_eq!(report.scopes[0].interactive_link_count, 0);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "scope_missing_interactive_link")
        );
    }

    #[test]
    fn render_check_accepts_native_file_link_and_required_href() {
        let mut options = options("trait-infobox");
        options.required_href_substrings = vec!["/File:Remilio_Mouth_".to_string()];
        options.required_link_classes = vec!["mw-file-description".to_string()];
        let report = analyze_rendered_page(
            &rendered(
                r#"<span class="trait-infobox"><a class="mw-file-description" href="/File:Remilio_Mouth_Binky.png"><img alt="Binky"></a><a href="/images/Binky.png" class="mw-file-source">source</a></span>"#,
            ),
            &options,
        );

        assert_eq!(report.status, "clean");
        assert_eq!(report.issue_count, 0);
        assert_eq!(
            report.scopes[0].interactive_hrefs,
            vec!["/File:Remilio_Mouth_Binky.png"]
        );
        assert_eq!(
            report.scopes[0].interactive_link_classes,
            vec!["mw-file-description"]
        );
    }

    #[test]
    fn render_check_rejects_custom_file_link_when_native_class_is_required() {
        let mut options = options("trait-infobox");
        options.required_link_classes = vec!["mw-file-description".to_string()];
        let report = analyze_rendered_page(
            &rendered(
                r#"<span class="trait-infobox"><a href="/File:Remilio_Mouth_Binky.png"><img alt="Binky"></a></span>"#,
            ),
            &options,
        );

        assert_eq!(report.status, "failed");
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "scope_missing_required_link_class")
        );
    }

    #[test]
    fn render_check_decodes_numeric_entities_and_detects_error_markup() {
        let report = analyze_rendered_page(
            &rendered(
                r#"<div class="trait-item"><span class="error">bad</span>&#91;&#91;Trait&#93;&#93;</div>"#,
            ),
            &options("trait-item"),
        );

        assert_eq!(report.literal_wikilink_count, 1);
        assert_eq!(report.parser_error_count, 1);
        assert_eq!(report.status, "failed");
    }

    #[test]
    fn render_check_reports_scope_count_and_href_contracts() {
        let mut options = options("trait-item");
        options.expected_scope_count = Some(2);
        options.required_href_substrings = vec!["(Remilio_mouth)".to_string()];
        let report = analyze_rendered_page(
            &rendered(r#"<div class="trait-item"><a href="/Other">other</a></div>"#),
            &options,
        );

        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "scope_count_mismatch")
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "scope_missing_required_href")
        );
    }
}
