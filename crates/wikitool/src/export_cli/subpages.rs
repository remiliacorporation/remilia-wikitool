use std::collections::HashMap;

use wikitool_core::external::{
    ExportFormat, ExternalFetchResult, generate_frontmatter, source_content_to_markdown,
};
use wikitool_core::support::compute_hash;

use super::render::{export_filename_stem, now_timestamp_string};
pub(super) fn remaining_subpage_limit(limit: Option<usize>, pages_collected: usize) -> usize {
    limit
        .map(|limit| limit.saturating_sub(pages_collected))
        .unwrap_or(usize::MAX)
}

pub(super) fn render_combined_export(
    pages: &[ExternalFetchResult],
    export_format: ExportFormat,
    include_frontmatter: bool,
    code_language: Option<&str>,
    domain: &str,
    source_url: &str,
    title: &str,
) -> String {
    let mut sections = Vec::new();
    for page in pages {
        let converted = match export_format {
            ExportFormat::Wikitext => page.content.clone(),
            ExportFormat::Markdown => {
                source_content_to_markdown(&page.content, &page.content_format, code_language)
            }
        };
        let heading = match export_format {
            ExportFormat::Markdown => format!("# {}", page.title),
            ExportFormat::Wikitext => format!("== {} ==", page.title),
        };
        sections.push(format!("{heading}\n\n{converted}"));
    }
    let combined = sections.join("\n\n---\n\n");
    if !include_frontmatter {
        return combined;
    }
    let frontmatter = generate_frontmatter(
        title,
        source_url,
        domain,
        &now_timestamp_string(),
        &[("pages".to_string(), pages.len().to_string())],
    );
    format!("{frontmatter}\n{combined}")
}

pub(super) fn build_subpage_index(
    pages: &[ExternalFetchResult],
    filenames: &[String],
    domain: &str,
    source_url: &str,
    title: &str,
) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("title: \"{} - Index\"", title.replace('"', "\\\"")),
        format!("source: {source_url}"),
        format!("wiki: {domain}"),
        format!("fetched: {}", now_timestamp_string()),
        format!("pages: {}", pages.len()),
        "---".to_string(),
        String::new(),
        format!("# {title}"),
        String::new(),
        "## Pages".to_string(),
        String::new(),
    ];
    for (page, filename) in pages.iter().zip(filenames.iter()) {
        lines.push(format!("- [{}](./{})", page.title, filename));
    }
    lines.join("\n")
}

pub(super) fn build_subpage_output_filenames(
    pages: &[ExternalFetchResult],
    export_format: ExportFormat,
) -> Vec<String> {
    let extension = export_format.file_extension();
    let stems = pages
        .iter()
        .map(|page| export_filename_stem(&page.title))
        .collect::<Vec<_>>();
    let mut counts = HashMap::<String, usize>::new();
    for stem in &stems {
        let key = format!("{stem}.{extension}").to_ascii_lowercase();
        *counts.entry(key).or_default() += 1;
    }

    pages
        .iter()
        .zip(stems)
        .map(|(page, stem)| {
            let key = format!("{stem}.{extension}").to_ascii_lowercase();
            if counts.get(&key).copied().unwrap_or_default() > 1 {
                let hash = compute_hash(&page.title);
                format!("{stem}-{}.{}", &hash[..8], extension)
            } else {
                format!("{stem}.{extension}")
            }
        })
        .collect()
}
