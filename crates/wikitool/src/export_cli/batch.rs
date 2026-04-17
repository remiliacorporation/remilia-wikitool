use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use wikitool_core::external::{
    DEFAULT_EXPORTS_DIR, ExportFormat, ExternalFetchResult, sanitize_filename,
};
use wikitool_core::support::compute_hash;

use crate::cli_support::normalize_path;

use super::render::{
    export_filename_stem, fetch_single_export_page, now_timestamp_string, render_export_page,
    write_export_file,
};
pub(super) fn run_urls_file_export(
    project_root: &Path,
    urls_file: &Path,
    output_dir: Option<&Path>,
    include_frontmatter: bool,
    code_language: Option<&str>,
) -> Result<()> {
    let urls = read_urls_file(urls_file)?;
    let output_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| default_urls_file_output_dir(project_root, urls_file));
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", normalize_path(&output_dir)))?;

    let mut successes = Vec::new();
    let mut failures = Vec::new();

    for url in urls {
        match fetch_single_export_page(&url, ExportFormat::Markdown) {
            Ok(page) => successes.push(UrlBatchSuccess {
                source_url: url,
                page,
                output_file: String::new(),
            }),
            Err(error) => failures.push(UrlBatchFailure {
                source_url: url.clone(),
                source_domain: domain_from_url(&url),
                error: error.to_string(),
            }),
        }
    }

    let filenames = build_url_batch_output_filenames(&successes);
    for (success, filename) in successes.iter_mut().zip(filenames) {
        let rendered = render_export_page(
            &success.page,
            ExportFormat::Markdown,
            include_frontmatter,
            code_language,
            &success.page.source_domain,
        );
        let output_file = output_dir.join(&filename);
        write_export_file(&output_file, &rendered)?;
        success.output_file = filename;
    }

    let index_content = build_url_batch_index(&successes, &failures, urls_file);
    let index_path = output_dir.join("_index.md");
    write_export_file(&index_path, &index_content)?;

    println!("export");
    println!("mode: urls_file");
    println!("project_root: {}", normalize_path(project_root));
    println!("urls_file: {}", normalize_path(urls_file));
    println!("urls_total: {}", successes.len() + failures.len());
    println!("pages_exported: {}", successes.len());
    println!("failed_count: {}", failures.len());
    println!("format: markdown");
    println!("output_dir: {}", normalize_path(&output_dir));
    println!("index_path: {}", normalize_path(&index_path));

    if !failures.is_empty() {
        io::stdout()
            .flush()
            .context("failed to flush export summary")?;
        bail!(
            "batch export completed with {} failed URL(s); see {}",
            failures.len(),
            normalize_path(&index_path)
        );
    }
    Ok(())
}

pub(super) fn read_urls_file(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", normalize_path(path)))?;
    let urls = raw
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches('\u{feff}').trim();
            (!line.is_empty() && !line.starts_with('#')).then(|| line.to_string())
        })
        .collect::<Vec<_>>();
    if urls.is_empty() {
        bail!("URL file contains no URLs: {}", normalize_path(path));
    }
    Ok(urls)
}

fn default_urls_file_output_dir(project_root: &Path, urls_file: &Path) -> PathBuf {
    let stem = urls_file
        .file_stem()
        .and_then(|value| value.to_str())
        .map(sanitize_filename)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "url-batch".to_string());
    project_root.join(DEFAULT_EXPORTS_DIR).join(stem)
}

#[derive(Debug)]
pub(super) struct UrlBatchSuccess {
    pub(super) source_url: String,
    pub(super) page: ExternalFetchResult,
    pub(super) output_file: String,
}

#[derive(Debug)]
pub(super) struct UrlBatchFailure {
    pub(super) source_url: String,
    pub(super) source_domain: String,
    pub(super) error: String,
}

pub(super) fn build_url_batch_output_filenames(successes: &[UrlBatchSuccess]) -> Vec<String> {
    let extension = ExportFormat::Markdown.file_extension();
    let stems = successes
        .iter()
        .map(|success| export_filename_stem(&success.page.title))
        .collect::<Vec<_>>();
    let mut counts = HashMap::<String, usize>::new();
    for stem in &stems {
        let key = format!("{stem}.{extension}").to_ascii_lowercase();
        *counts.entry(key).or_default() += 1;
    }

    let mut used = HashMap::<String, usize>::new();
    successes
        .iter()
        .zip(stems)
        .enumerate()
        .map(|(index, (success, stem))| {
            let key = format!("{stem}.{extension}").to_ascii_lowercase();
            let base = if counts.get(&key).copied().unwrap_or_default() > 1 {
                let hash = compute_hash(&format!("{}#{index}", success.source_url));
                format!("{stem}-{}", &hash[..8])
            } else {
                stem
            };
            let mut filename = format!("{base}.{extension}");
            let mut normalized = filename.to_ascii_lowercase();
            let mut suffix = 2usize;
            while used.contains_key(&normalized) {
                filename = format!("{base}-{suffix}.{extension}");
                normalized = filename.to_ascii_lowercase();
                suffix += 1;
            }
            used.insert(normalized, 1);
            filename
        })
        .collect()
}

pub(super) fn build_url_batch_index(
    successes: &[UrlBatchSuccess],
    failures: &[UrlBatchFailure],
    urls_file: &Path,
) -> String {
    let mut lines = vec![
        "---".to_string(),
        "title: \"URL Export Batch - Index\"".to_string(),
        format!(
            "urls_file: \"{}\"",
            normalize_path(urls_file).replace('"', "\\\"")
        ),
        format!("fetched_at: \"{}\"", now_timestamp_string()),
        format!("exported: {}", successes.len()),
        format!("failed: {}", failures.len()),
        "---".to_string(),
        String::new(),
        "# URL Export Batch".to_string(),
        String::new(),
        "## Exported".to_string(),
        String::new(),
    ];

    if successes.is_empty() {
        lines.push("No URLs exported successfully.".to_string());
    } else {
        lines.push("| Source URL | Output file | Title | Source domain |".to_string());
        lines.push("| --- | --- | --- | --- |".to_string());
        for success in successes {
            lines.push(format!(
                "| {} | [{}](./{}) | {} | {} |",
                markdown_table_cell(&success.source_url),
                markdown_table_cell(&success.output_file),
                success.output_file,
                markdown_table_cell(&success.page.title),
                markdown_table_cell(&success.page.source_domain)
            ));
        }
    }

    lines.push(String::new());
    lines.push("## Failed".to_string());
    lines.push(String::new());
    if failures.is_empty() {
        lines.push("No URL failures.".to_string());
    } else {
        lines.push("| Source URL | Source domain | Error |".to_string());
        lines.push("| --- | --- | --- |".to_string());
        for failure in failures {
            lines.push(format!(
                "| {} | {} | {} |",
                markdown_table_cell(&failure.source_url),
                markdown_table_cell(&failure.source_domain),
                markdown_table_cell(&failure.error)
            ));
        }
    }

    lines.join("\n")
}

fn markdown_table_cell(value: &str) -> String {
    value
        .replace('|', "\\|")
        .replace('\n', " ")
        .trim()
        .to_string()
}

fn domain_from_url(url: &str) -> String {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim();
    if host.is_empty() {
        "web".to_string()
    } else {
        host.to_string()
    }
}
