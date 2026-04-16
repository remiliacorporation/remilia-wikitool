use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use wikitool_core::external::{
    ExportFormat, ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile,
    ExternalFetchResult, ParsedWikiUrl, fetch_mediawiki_page, fetch_page_by_url,
    generate_frontmatter, parse_wiki_url, sanitize_filename, source_content_to_markdown,
};
use wikitool_core::support::compute_hash;

use crate::cli_support::normalize_path;
pub(super) fn fetch_mediawiki_export_page(
    title: &str,
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<Option<ExternalFetchResult>> {
    fetch_mediawiki_page(title, parsed, options)
}

pub(super) fn fetch_single_export_page(
    url: &str,
    export_format: ExportFormat,
) -> Result<ExternalFetchResult> {
    let is_mediawiki = parse_wiki_url(url).is_some();
    if !is_mediawiki && export_format == ExportFormat::Wikitext {
        bail!(
            "wikitext export requires a recognizable MediaWiki URL; use markdown for arbitrary web pages"
        );
    }
    let options = if is_mediawiki {
        ExternalFetchOptions {
            format: ExternalFetchFormat::Wikitext,
            max_bytes: 1_000_000,
            profile: ExternalFetchProfile::Legacy,
        }
    } else {
        ExternalFetchOptions {
            format: ExternalFetchFormat::Html,
            max_bytes: 1_000_000,
            profile: ExternalFetchProfile::Research,
        }
    };
    fetch_page_by_url(url, &options)?.ok_or_else(|| anyhow::anyhow!("page not found: {url}"))
}

pub(super) fn render_export_page(
    page: &ExternalFetchResult,
    export_format: ExportFormat,
    include_frontmatter: bool,
    code_language: Option<&str>,
    domain: &str,
) -> String {
    let converted = match export_format {
        ExportFormat::Wikitext => page.content.clone(),
        ExportFormat::Markdown => {
            source_content_to_markdown(&page.content, &page.content_format, code_language)
        }
    };
    if !include_frontmatter {
        return converted;
    }
    let frontmatter = generate_frontmatter(
        &page.title,
        &page.url,
        domain,
        &page.fetched_at,
        &export_frontmatter_fields(page, export_format),
    );
    format!("{frontmatter}\n{converted}")
}

fn export_frontmatter_fields(
    page: &ExternalFetchResult,
    export_format: ExportFormat,
) -> Vec<(String, String)> {
    let mut fields = vec![
        (
            "format".to_string(),
            export_format.file_extension().to_string(),
        ),
        ("source_wiki".to_string(), page.source_wiki.clone()),
        ("content_format".to_string(), page.content_format.clone()),
        ("content_hash".to_string(), page.content_hash.clone()),
    ];
    if let Some(value) = page.revision_timestamp.as_deref() {
        fields.push(("revision_timestamp".to_string(), value.to_string()));
    }
    if let Some(value) = page.revision_id {
        fields.push(("revision_id".to_string(), value.to_string()));
    }
    if let Some(value) = page.display_title.as_deref() {
        fields.push(("display_title".to_string(), value.to_string()));
    }
    if let Some(value) = page.canonical_url.as_deref() {
        fields.push(("canonical_url".to_string(), value.to_string()));
    }
    if let Some(value) = page.site_name.as_deref() {
        fields.push(("site_name".to_string(), value.to_string()));
    }
    if let Some(value) = page.byline.as_deref() {
        fields.push(("byline".to_string(), value.to_string()));
    }
    if let Some(value) = page.published_at.as_deref() {
        fields.push(("published_at".to_string(), value.to_string()));
    }
    if let Some(value) = page.fetch_mode {
        fields.push((
            "fetch_mode".to_string(),
            format_fetch_mode(value).to_string(),
        ));
    }
    if let Some(value) = page.extraction_quality {
        fields.push((
            "extraction_quality".to_string(),
            format_extraction_quality(value).to_string(),
        ));
    }
    if let Some(value) = page.rendered_fetch_mode {
        fields.push((
            "rendered_fetch_mode".to_string(),
            format_rendered_fetch_mode(value).to_string(),
        ));
    }
    fields
}

pub(super) fn export_filename_stem(title: &str) -> String {
    let stem = sanitize_filename(title);
    if !stem.is_empty() {
        return stem;
    }

    let hash = compute_hash(title);
    format!("page-{}", &hash[..8])
}

pub(super) fn write_or_print_export(content: &str, output_path: Option<&Path>) -> Result<()> {
    if let Some(path) = output_path {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
        }
        fs::write(path, content.as_bytes())
            .with_context(|| format!("failed to write {}", normalize_path(path)))?;
    } else {
        println!("{content}");
    }
    Ok(())
}

pub(super) fn now_timestamp_string() -> String {
    wikitool_core::support::now_iso8601_utc()
}

pub(super) fn format_rendered_fetch_mode(
    mode: wikitool_core::external::RenderedFetchMode,
) -> &'static str {
    match mode {
        wikitool_core::external::RenderedFetchMode::ParseApi => "parse_api",
    }
}

pub(super) fn format_fetch_mode(mode: wikitool_core::external::FetchMode) -> &'static str {
    match mode {
        wikitool_core::external::FetchMode::Static => "static",
    }
}

fn format_extraction_quality(quality: wikitool_core::external::ExtractionQuality) -> &'static str {
    match quality {
        wikitool_core::external::ExtractionQuality::Low => "low",
        wikitool_core::external::ExtractionQuality::Medium => "medium",
        wikitool_core::external::ExtractionQuality::High => "high",
    }
}
