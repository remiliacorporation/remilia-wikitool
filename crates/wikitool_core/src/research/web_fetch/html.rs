use std::collections::BTreeMap;
use std::io::Read;

use anyhow::{Context, Result};
use reqwest::Url;

use super::super::entities::decode_html_entities;
use super::super::model::{
    ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult, ExtractionQuality, FetchMode,
};
use super::super::url::decode_title;
use crate::support::{compute_hash, now_iso8601_utc};
#[derive(Debug, Clone)]
pub(super) struct TagMatch {
    pub(super) attrs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct HtmlMetadata {
    pub(super) title: Option<String>,
    pub(super) canonical_url: Option<String>,
    pub(super) site_name: Option<String>,
    pub(super) byline: Option<String>,
    pub(super) published_at: Option<String>,
    pub(super) description: Option<String>,
}

pub(super) fn build_html_fetch_result(
    html: &str,
    final_url: &str,
    source_domain: &str,
    fallback_title: &str,
    options: &ExternalFetchOptions,
) -> ExternalFetchResult {
    let metadata = extract_html_metadata(html, final_url);
    let title = metadata
        .title
        .clone()
        .unwrap_or_else(|| fallback_title.to_string());
    let canonical_url = metadata
        .canonical_url
        .clone()
        .or_else(|| Some(final_url.to_string()));
    let challenge_notice = detect_access_challenge(html)
        .then(|| format!(
            "Access challenge detected while fetching {final_url}. The site returned anti-bot HTML instead of readable article content."
        ));

    match options.profile {
        ExternalFetchProfile::Legacy => {
            let extract = metadata
                .description
                .clone()
                .or_else(|| summarize_text(html, 280));
            ExternalFetchResult {
                title,
                content: html.to_string(),
                fetched_at: now_iso8601_utc(),
                revision_timestamp: None,
                extract,
                url: final_url.to_string(),
                source_wiki: "web".to_string(),
                source_domain: source_domain.to_string(),
                content_format: "html".to_string(),
                content_hash: compute_hash(html),
                revision_id: None,
                display_title: None,
                rendered_fetch_mode: None,
                canonical_url,
                site_name: metadata.site_name,
                byline: metadata.byline,
                published_at: metadata.published_at,
                fetch_mode: Some(FetchMode::Static),
                extraction_quality: None,
                fetch_attempts: Vec::new(),
            }
        }
        ExternalFetchProfile::Research => {
            if let Some(note) = challenge_notice {
                let content_hash = compute_hash(&note);
                return ExternalFetchResult {
                    title,
                    content: note.clone(),
                    fetched_at: now_iso8601_utc(),
                    revision_timestamp: None,
                    extract: Some(note),
                    url: final_url.to_string(),
                    source_wiki: "web".to_string(),
                    source_domain: source_domain.to_string(),
                    content_format: "text".to_string(),
                    content_hash,
                    revision_id: None,
                    display_title: None,
                    rendered_fetch_mode: None,
                    canonical_url,
                    site_name: metadata.site_name,
                    byline: None,
                    published_at: None,
                    fetch_mode: Some(FetchMode::Static),
                    extraction_quality: Some(ExtractionQuality::Low),
                    fetch_attempts: Vec::new(),
                };
            }
            let extracted = extract_readable_text(html, options.max_bytes);
            let app_shell = detect_app_shell_html(html);
            if extracted.is_empty() {
                let content = build_metadata_fallback_content(
                    &metadata,
                    final_url,
                    app_shell,
                    options.max_bytes,
                );
                let extract = metadata
                    .description
                    .clone()
                    .or_else(|| summarize_text(&content, 280));
                let content_hash = compute_hash(&content);

                return ExternalFetchResult {
                    title,
                    content,
                    fetched_at: now_iso8601_utc(),
                    revision_timestamp: None,
                    extract,
                    url: final_url.to_string(),
                    source_wiki: "web".to_string(),
                    source_domain: source_domain.to_string(),
                    content_format: "text".to_string(),
                    content_hash,
                    revision_id: None,
                    display_title: None,
                    rendered_fetch_mode: None,
                    canonical_url,
                    site_name: metadata.site_name,
                    byline: metadata.byline,
                    published_at: metadata.published_at,
                    fetch_mode: Some(FetchMode::Static),
                    extraction_quality: Some(ExtractionQuality::Low),
                    fetch_attempts: Vec::new(),
                };
            }
            let content = extracted;
            let extract = metadata
                .description
                .clone()
                .or_else(|| summarize_text(&content, 280));
            let extraction_quality = Some(score_extraction_quality(&content, extract.as_deref()));
            let content_hash = compute_hash(&content);

            ExternalFetchResult {
                title,
                content,
                fetched_at: now_iso8601_utc(),
                revision_timestamp: None,
                extract,
                url: final_url.to_string(),
                source_wiki: "web".to_string(),
                source_domain: source_domain.to_string(),
                content_format: "text".to_string(),
                content_hash,
                revision_id: None,
                display_title: None,
                rendered_fetch_mode: None,
                canonical_url,
                site_name: metadata.site_name,
                byline: metadata.byline,
                published_at: metadata.published_at,
                fetch_mode: Some(FetchMode::Static),
                extraction_quality,
                fetch_attempts: Vec::new(),
            }
        }
    }
}

pub(super) fn build_metadata_fallback_content(
    metadata: &HtmlMetadata,
    final_url: &str,
    app_shell: bool,
    max_bytes: usize,
) -> String {
    let mut lines = Vec::new();
    if app_shell {
        lines.push(format!(
            "Client-rendered or app-shell page detected at {final_url}. Full article text could not be extracted reliably from the static HTML."
        ));
    } else {
        lines.push(format!(
            "Readable article text could not be extracted reliably from {final_url}."
        ));
    }
    if let Some(title) = metadata.title.as_deref() {
        lines.push(format!("Title: {title}"));
    }
    if let Some(description) = metadata.description.as_deref() {
        lines.push(format!("Description: {description}"));
    }
    if let Some(byline) = metadata.byline.as_deref() {
        lines.push(format!("Author: {byline}"));
    }
    if let Some(published_at) = metadata.published_at.as_deref() {
        lines.push(format!("Published: {published_at}"));
    }
    if let Some(site_name) = metadata.site_name.as_deref() {
        lines.push(format!("Site: {site_name}"));
    }
    if let Some(canonical_url) = metadata.canonical_url.as_deref() {
        lines.push(format!("Canonical URL: {canonical_url}"));
    }
    truncate_to_byte_limit(&lines.join("\n"), max_bytes)
}

pub(super) fn build_text_fetch_result(
    content: &str,
    final_url: &str,
    source_domain: &str,
    fallback_title: &str,
    content_format: &str,
    options: &ExternalFetchOptions,
) -> ExternalFetchResult {
    let extract = summarize_text(content, 280);
    let extraction_quality = match options.profile {
        ExternalFetchProfile::Legacy => None,
        ExternalFetchProfile::Research => {
            Some(score_extraction_quality(content, extract.as_deref()))
        }
    };
    let content = truncate_to_byte_limit(content, options.max_bytes);

    ExternalFetchResult {
        title: fallback_title.to_string(),
        content: content.clone(),
        fetched_at: now_iso8601_utc(),
        revision_timestamp: None,
        extract,
        url: final_url.to_string(),
        source_wiki: "web".to_string(),
        source_domain: source_domain.to_string(),
        content_format: content_format.to_string(),
        content_hash: compute_hash(&content),
        revision_id: None,
        display_title: None,
        rendered_fetch_mode: None,
        canonical_url: Some(final_url.to_string()),
        site_name: None,
        byline: None,
        published_at: None,
        fetch_mode: Some(FetchMode::Static),
        extraction_quality,
        fetch_attempts: Vec::new(),
    }
}

pub(super) fn derive_title_from_url(parsed_url: Option<&Url>, final_url: &str) -> String {
    parsed_url
        .and_then(|value| value.path_segments())
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.trim().is_empty())
        .map(decode_title)
        .unwrap_or_else(|| final_url.to_string())
}

pub(super) fn extract_html_metadata(html: &str, final_url: &str) -> HtmlMetadata {
    let head = extract_head(html);
    let title = extract_title(&head);
    let meta = collect_meta(&scan_tags(&head, "meta"));
    let canonical_url = find_canonical(&scan_tags(&head, "link"))
        .or_else(|| meta_first(&meta, &["og:url", "twitter:url"]))
        .or_else(|| Some(final_url.to_string()));

    HtmlMetadata {
        title: meta_first(&meta, &["og:title", "twitter:title"]).or(title),
        canonical_url,
        site_name: meta_first(&meta, &["og:site_name", "application-name"]),
        byline: meta_first(
            &meta,
            &[
                "author",
                "article:author",
                "parsely-author",
                "dc.creator",
                "dc.creator.creator",
            ],
        ),
        published_at: meta_first(
            &meta,
            &[
                "article:published_time",
                "og:published_time",
                "pubdate",
                "publish-date",
                "parsely-pub-date",
                "date",
            ],
        ),
        description: meta_first(
            &meta,
            &["description", "og:description", "twitter:description"],
        ),
    }
}

pub(super) fn extract_client_redirect_url(html: &str, final_url: &str) -> Option<String> {
    let head = extract_head(html);
    let base_url = Url::parse(final_url).ok()?;
    for tag in scan_tags(&head, "meta") {
        let http_equiv = tag
            .attrs
            .get("http-equiv")
            .map(|value| value.to_ascii_lowercase());
        let id = tag.attrs.get("id").map(|value| value.to_ascii_lowercase());
        if http_equiv.as_deref() != Some("refresh") && id.as_deref() != Some("__next-page-redirect")
        {
            continue;
        }
        let content = tag.attrs.get("content")?;
        let target = parse_meta_refresh_target(content)?;
        let joined = base_url.join(target).ok()?.to_string();
        return Some(joined);
    }
    None
}

fn parse_meta_refresh_target(content: &str) -> Option<&str> {
    let lowered = content.to_ascii_lowercase();
    let marker = "url=";
    let at = lowered.find(marker)?;
    let target = content[at + marker.len()..].trim();
    let target = target.trim_matches(|ch| matches!(ch, '"' | '\'' | ' '));
    if target.is_empty() {
        None
    } else {
        Some(target)
    }
}

fn meta_first(meta: &BTreeMap<String, Vec<String>>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        meta.get(*key)
            .and_then(|values| values.first())
            .cloned()
            .filter(|value| !value.trim().is_empty())
    })
}

pub(super) fn extract_readable_text(html: &str, max_bytes: usize) -> String {
    let candidate = extract_tag_contents(html, "article")
        .or_else(|| extract_tag_contents(html, "main"))
        .or_else(|| extract_tag_contents(html, "body"))
        .unwrap_or(html);
    let mut output = String::new();
    let mut index = 0usize;
    let mut skip_depth = 0usize;

    while index < candidate.len() {
        let Some(next_lt) = candidate[index..].find('<') else {
            if skip_depth == 0 {
                append_text_segment(&mut output, &candidate[index..]);
            }
            break;
        };

        let tag_start = index + next_lt;
        if skip_depth == 0 {
            append_text_segment(&mut output, &candidate[index..tag_start]);
        }

        if starts_with_at(candidate, tag_start, "<!--") {
            if let Some(end) = index_of_ignore_case(candidate, "-->", tag_start + 4) {
                index = end + 3;
            } else {
                break;
            }
            continue;
        }

        let Some(tag_end) = find_tag_end(candidate, tag_start) else {
            break;
        };
        let raw_tag = &candidate[tag_start..=tag_end];
        let Some((tag_name, is_closing, is_self_closing)) = parse_tag_descriptor(raw_tag) else {
            index = tag_end + 1;
            continue;
        };

        if is_closing {
            if is_skip_tag(tag_name) && skip_depth > 0 {
                skip_depth -= 1;
            }
            if skip_depth == 0 {
                if is_paragraph_block_tag(tag_name) {
                    append_separator(&mut output, "\n\n");
                } else if is_block_tag(tag_name) {
                    append_separator(&mut output, "\n");
                }
            }
        } else if is_skip_tag(tag_name) && !is_self_closing {
            skip_depth += 1;
        } else if skip_depth == 0 {
            if tag_name == "br" {
                append_separator(&mut output, "\n");
            } else if tag_name == "li" {
                append_separator(&mut output, "\n- ");
            } else if is_paragraph_block_tag(tag_name) {
                append_separator(&mut output, "\n\n");
            } else if is_block_tag(tag_name) {
                append_separator(&mut output, "\n");
            }
        }

        index = tag_end + 1;
    }

    normalize_extracted_text(&output, max_bytes)
}

fn append_text_segment(output: &mut String, text: &str) {
    let decoded = decode_html(text);
    if !decoded.is_empty() {
        output.push_str(&decoded);
    }
}

fn append_separator(output: &mut String, separator: &str) {
    if output.is_empty() {
        return;
    }
    match separator {
        "\n\n" => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if output.ends_with("\n\n") {
                return;
            }
            if output.ends_with('\n') {
                output.push('\n');
            } else {
                output.push_str("\n\n");
            }
        }
        "\n- " => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if output.ends_with('\n') {
                output.push_str("- ");
            } else {
                output.push_str("\n- ");
            }
        }
        "\n" => {
            while output.ends_with(' ') || output.ends_with('\t') {
                output.pop();
            }
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }
        other => output.push_str(other),
    }
}

pub(super) fn normalize_extracted_text(value: &str, max_bytes: usize) -> String {
    let mut lines = Vec::new();
    let mut blank_count = 0usize;
    for line in value.lines() {
        let collapsed = collapse_inline_whitespace(line);
        if collapsed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                lines.push(String::new());
            }
            continue;
        }
        blank_count = 0;
        lines.push(collapsed);
    }
    let mut merged = merge_isolated_bullet_markers(&lines);
    compact_adjacent_list_item_spacing(&mut merged);
    while matches!(merged.first(), Some(line) if line.is_empty()) {
        merged.remove(0);
    }
    while matches!(merged.last(), Some(line) if line.is_empty()) {
        merged.pop();
    }
    truncate_to_byte_limit(&merged.join("\n"), max_bytes)
}

/// Drop isolated single-character list markers (`-`, `*`, `•`) emitted as their own
/// line by HTML-to-text extractors, joining them with the following text line. Also
/// strips leading image/credit attribution lines (`© …`) that sit above their related
/// prose but carry no reader-facing content on their own.
fn merge_isolated_bullet_markers(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    while index < lines.len() {
        let line = &lines[index];
        let trimmed = line.trim();
        if matches!(trimmed, "-" | "*" | "\u{2022}")
            && let Some((next_index, next)) = next_nonblank_line(lines, index + 1)
            && !next.trim().is_empty()
        {
            out.push(format!("- {}", next.trim()));
            index = next_index + 1;
            continue;
        }
        index += 1;
        out.push(line.clone());
    }
    out
}

fn next_nonblank_line(lines: &[String], start: usize) -> Option<(usize, &str)> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| (index, line.as_str()))
}

fn compact_adjacent_list_item_spacing(lines: &mut Vec<String>) {
    let mut index = 1usize;
    while index + 1 < lines.len() {
        if lines[index].is_empty()
            && is_markdown_unordered_list_item(&lines[index - 1])
            && is_markdown_unordered_list_item(&lines[index + 1])
        {
            lines.remove(index);
            continue;
        }
        index += 1;
    }
}

fn is_markdown_unordered_list_item(line: &str) -> bool {
    line.trim_start().starts_with("- ")
}

pub(super) fn collapse_inline_whitespace(value: &str) -> String {
    let mut output = String::new();
    let mut pending_space = false;

    for ch in value.chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !output.is_empty() {
            output.push(' ');
        }
        output.push(ch);
        pending_space = false;
    }

    output.trim().to_string()
}

fn summarize_text(value: &str, max_chars: usize) -> Option<String> {
    let text = collapse_inline_whitespace(value);
    if text.is_empty() {
        return None;
    }
    let mut output = String::new();
    for ch in text.chars().take(max_chars) {
        output.push(ch);
    }
    Some(output)
}

fn score_extraction_quality(text: &str, extract: Option<&str>) -> ExtractionQuality {
    let word_count = text.split_whitespace().count();
    if word_count >= 350 {
        return ExtractionQuality::High;
    }
    if word_count >= 40 || extract.is_some_and(|value| value.len() >= 40) {
        return ExtractionQuality::Medium;
    }
    ExtractionQuality::Low
}

pub(super) fn detect_app_shell_html(html: &str) -> bool {
    let lowered = html.to_ascii_lowercase();
    let signals = [
        "__next_f.push(",
        "_next/static/",
        "__next_data__",
        "data-reactroot",
        "ng-version",
        "window.__nuxt__",
        "id=\"app\"",
        "id='app'",
    ];
    signals
        .iter()
        .filter(|signal| lowered.contains(**signal))
        .count()
        >= 2
}

pub(super) fn detect_access_challenge(html: &str) -> bool {
    let lowered = html.to_ascii_lowercase();
    let vendor_signals = [
        "awswafintegration",
        "awswafcookiedomainlist",
        "challenge-container",
        "__cf_chl_",
        "cf-browser-verification",
        "captcha-delivery",
        "datadome",
        "perimeterx",
        "px-captcha",
    ];
    if vendor_signals.iter().any(|signal| lowered.contains(signal)) {
        return true;
    }

    let generic_signals = [
        "verify that you're not a robot",
        "checking your browser",
        "enable javascript and then reload the page",
        "javascript is disabled",
        "access denied",
        "captcha",
        "challenge.js",
    ];
    generic_signals
        .iter()
        .filter(|signal| lowered.contains(**signal))
        .count()
        >= 2
}

pub(super) fn extract_head(html: &str) -> String {
    let Some(head_start) = find_tag_start(html, "head", 0) else {
        return html.to_string();
    };
    let Some(open_end) = find_tag_end(html, head_start) else {
        return html.to_string();
    };
    let Some(close_index) = index_of_ignore_case(html, "</head>", open_end + 1) else {
        return html[open_end + 1..].to_string();
    };
    html[open_end + 1..close_index].to_string()
}

fn extract_title(html: &str) -> Option<String> {
    let start = find_tag_start(html, "title", 0)?;
    let open_end = find_tag_end(html, start)?;
    let close = index_of_ignore_case(html, "</title>", open_end + 1)?;
    let raw = &html[open_end + 1..close];
    let decoded = decode_html(raw);
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn scan_tags(html: &str, tag_name: &str) -> Vec<TagMatch> {
    let name = tag_name.to_ascii_lowercase();
    let mut output = Vec::new();
    let mut index = 0usize;

    while index < html.len() {
        let Some(lt) = html[index..].find('<') else {
            break;
        };
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", at + 4) {
                index = end + 3;
            } else {
                index = html.len();
            }
            continue;
        }
        if is_tag_at(html, at, &name) {
            let Some(end) = find_tag_end(html, at) else {
                break;
            };
            let raw = &html[at..=end];
            output.push(TagMatch {
                attrs: parse_attributes(raw, &name),
            });
            index = end + 1;
            continue;
        }
        index = at + 1;
    }

    output
}

fn collect_meta(tags: &[TagMatch]) -> BTreeMap<String, Vec<String>> {
    let mut meta = BTreeMap::new();
    for tag in tags {
        let key = tag
            .attrs
            .get("property")
            .or_else(|| tag.attrs.get("name"))
            .map(|value| value.to_ascii_lowercase());
        let Some(key) = key else {
            continue;
        };
        let Some(content) = tag.attrs.get("content") else {
            continue;
        };
        let content = decode_html(content).trim().to_string();
        if content.is_empty() {
            continue;
        }
        meta.entry(key).or_insert_with(Vec::new).push(content);
    }
    meta
}

fn find_canonical(tags: &[TagMatch]) -> Option<String> {
    for tag in tags {
        let rel = tag
            .attrs
            .get("rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if !rel.contains("canonical") {
            continue;
        }
        if let Some(href) = tag.attrs.get("href") {
            let decoded = decode_html(href).trim().to_string();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    None
}

fn extract_tag_contents<'a>(html: &'a str, tag_name: &str) -> Option<&'a str> {
    let start = find_tag_start(html, tag_name, 0)?;
    let open_end = find_tag_end(html, start)?;
    let close_start = find_matching_close_tag(html, tag_name, open_end + 1)?;
    Some(&html[open_end + 1..close_start])
}

fn find_matching_close_tag(html: &str, tag_name: &str, start: usize) -> Option<usize> {
    let mut index = start;
    let mut depth = 1usize;

    while index < html.len() {
        let lt = html[index..].find('<')?;
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", at + 4) {
                index = end + 3;
            } else {
                return None;
            }
            continue;
        }
        if is_close_tag_at(html, at, tag_name) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(at);
            }
            index = find_tag_end(html, at)? + 1;
            continue;
        }
        if is_tag_at(html, at, tag_name) {
            let end = find_tag_end(html, at)?;
            if !is_self_closing_tag(&html[at..=end], tag_name) {
                depth += 1;
            }
            index = end + 1;
            continue;
        }
        index = at + 1;
    }

    None
}

fn parse_tag_descriptor(tag_raw: &str) -> Option<(&str, bool, bool)> {
    let bytes = tag_raw.as_bytes();
    if bytes.first().copied() != Some(b'<') {
        return None;
    }

    let mut index = 1usize;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let is_closing = bytes.get(index).copied() == Some(b'/');
    if is_closing {
        index += 1;
    }
    let name_start = index;
    while index < bytes.len() {
        let ch = bytes[index];
        if ch.is_ascii_whitespace() || ch == b'>' || ch == b'/' {
            break;
        }
        index += 1;
    }
    if name_start == index {
        return None;
    }
    let tag_name = &tag_raw[name_start..index];
    Some((tag_name, is_closing, is_self_closing_tag(tag_raw, tag_name)))
}

fn is_skip_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "script"
            | "style"
            | "noscript"
            | "template"
            | "svg"
            | "canvas"
            | "nav"
            | "header"
            | "footer"
            | "aside"
            | "form"
    )
}

fn is_block_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "p" | "div"
            | "section"
            | "article"
            | "main"
            | "li"
            | "ul"
            | "ol"
            | "table"
            | "tr"
            | "blockquote"
            | "pre"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

fn is_paragraph_block_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "p" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    )
}

fn is_self_closing_tag(tag_raw: &str, tag_name: &str) -> bool {
    let normalized = tag_name.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "br" | "hr" | "img" | "meta" | "link" | "input" | "source"
    ) {
        return true;
    }
    tag_raw.trim_end().ends_with("/>")
}

pub(super) fn read_text_body_limited<R: Read>(reader: R, max_bytes: usize) -> Result<String> {
    if max_bytes == 0 {
        return Ok(String::new());
    }

    let mut body = Vec::with_capacity(max_bytes.min(8192));
    let mut limited = reader.take(max_bytes as u64);
    limited
        .read_to_end(&mut body)
        .context("failed to read response body")?;

    let body = strip_utf8_bom(&body);
    let text = String::from_utf8_lossy(body);
    Ok(truncate_to_byte_limit(&text, max_bytes))
}

fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    const UTF8_BOM: &[u8; 3] = b"\xEF\xBB\xBF";

    bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes)
}

pub(crate) fn truncate_to_byte_limit(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn find_tag_start(html: &str, tag_name: &str, start: usize) -> Option<usize> {
    let mut index = start;
    while index < html.len() {
        let lt = html[index..].find('<')?;
        let at = index + lt;
        if is_tag_at(html, at, tag_name) {
            return Some(at);
        }
        index = at + 1;
    }
    None
}

fn is_tag_at(html: &str, at: usize, tag_name: &str) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(at).copied() != Some(b'<') {
        return false;
    }
    let mut index = at + 1;
    if index >= bytes.len() {
        return false;
    }
    if bytes[index] == b'/' {
        return false;
    }
    for expected in tag_name.as_bytes() {
        let Some(actual) = bytes.get(index) else {
            return false;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return false;
        }
        index += 1;
    }
    matches!(
        bytes.get(index).copied(),
        Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/')
    )
}

fn is_close_tag_at(html: &str, at: usize, tag_name: &str) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(at).copied() != Some(b'<') || bytes.get(at + 1).copied() != Some(b'/') {
        return false;
    }
    let mut index = at + 2;
    for expected in tag_name.as_bytes() {
        let Some(actual) = bytes.get(index) else {
            return false;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return false;
        }
        index += 1;
    }
    matches!(
        bytes.get(index).copied(),
        Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>')
    )
}

fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut index = start;
    let mut quote = None::<u8>;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if byte == b'>' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn parse_attributes(tag_raw: &str, tag_name: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = tag_raw.as_bytes();
    let mut index = tag_name.len() + 1;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'>' {
            break;
        }
        if byte == b'/' || byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        let name_start = index;
        while index < bytes.len() {
            let ch = bytes[index];
            if ch.is_ascii_whitespace() || ch == b'=' || ch == b'>' || ch == b'/' {
                break;
            }
            index += 1;
        }
        if name_start == index {
            index += 1;
            continue;
        }
        let name = tag_raw[name_start..index].trim().to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let mut value = String::new();
        if bytes.get(index).copied() == Some(b'=') {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if let Some(quote) = bytes
                .get(index)
                .copied()
                .filter(|byte| *byte == b'"' || *byte == b'\'')
            {
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                value = tag_raw[value_start..index].to_string();
                if bytes.get(index).copied() == Some(quote) {
                    index += 1;
                }
            } else {
                let value_start = index;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                {
                    index += 1;
                }
                value = tag_raw[value_start..index].to_string();
            }
        }

        if !value.is_empty() {
            attrs.insert(name, value);
        } else {
            attrs.entry(name).or_default();
        }
    }

    attrs
}

pub(super) fn index_of_ignore_case(text: &str, search: &str, start: usize) -> Option<usize> {
    if search.is_empty() {
        return Some(start);
    }
    let text_bytes = text.as_bytes();
    let search_bytes = search.as_bytes();
    if search_bytes.len() > text_bytes.len() || start >= text_bytes.len() {
        return None;
    }

    let last_start = text_bytes.len().saturating_sub(search_bytes.len());
    for index in start..=last_start {
        let mut matched = true;
        for offset in 0..search_bytes.len() {
            if !text_bytes[index + offset].eq_ignore_ascii_case(&search_bytes[offset]) {
                matched = false;
                break;
            }
        }
        if matched {
            return Some(index);
        }
    }
    None
}

fn starts_with_at(text: &str, index: usize, sequence: &str) -> bool {
    if index + sequence.len() > text.len() {
        return false;
    }
    &text[index..index + sequence.len()] == sequence
}

pub(super) fn decode_html(text: &str) -> String {
    decode_html_entities(text)
}
