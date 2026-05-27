use super::metadata::extract_html_metadata;
use super::model::HtmlMetadata;
use super::text::{
    detect_access_challenge, detect_app_shell_html, extract_readable_text,
    score_extraction_quality, summarize_text, truncate_to_byte_limit,
};
use crate::research::model::{
    ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult, ExtractionQuality, FetchMode,
};
use crate::support::{compute_hash, now_iso8601_utc};
pub(in crate::research::web_fetch) fn build_html_fetch_result(
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

pub(in crate::research::web_fetch) fn build_metadata_fallback_content(
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

pub(in crate::research::web_fetch) fn build_text_fetch_result(
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
