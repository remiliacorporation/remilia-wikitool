use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use wikitool_core::external::{
    DEFAULT_EXPORTS_DIR, ExportFormat, ExternalFetchFormat, ExternalFetchOptions,
    ExternalFetchProfile, ExternalFetchResult, ParsedWikiUrl, default_export_path,
    fetch_mediawiki_page, fetch_page_by_url, fetch_pages_by_titles, generate_frontmatter,
    list_subpages, parse_wiki_url, sanitize_filename, source_content_to_markdown,
};

use crate::cli_support::{
    ExportContentFormat, FetchContentFormat, normalize_path, resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};
use wikitool_core::support::compute_hash;

#[derive(Debug, Args)]
pub(crate) struct FetchArgs {
    url: String,
    #[arg(
        long,
        value_enum,
        default_value_t = FetchContentFormat::Wikitext,
        value_name = "FORMAT",
        help = "Output format: wikitext|html|rendered-html"
    )]
    format: FetchContentFormat,
    #[arg(long, help = "Save output under reference/<source>/ in project root")]
    save: bool,
    #[arg(
        long,
        value_name = "NAME",
        help = "Custom name for saved reference file"
    )]
    name: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    url: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read arbitrary source URLs from a newline-delimited file"
    )]
    urls_file: Option<PathBuf>,
    #[arg(
        short = 'o',
        long,
        value_name = "PATH",
        help = "Output file or directory path"
    )]
    output: Option<PathBuf>,
    #[arg(
        long,
        value_name = "DIR",
        help = "Output directory for --urls-file markdown exports"
    )]
    output_dir: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = ExportContentFormat::Markdown,
        value_name = "FORMAT",
        help = "Output format: markdown|wikitext"
    )]
    format: ExportContentFormat,
    #[arg(
        long,
        value_name = "LANG",
        help = "Code language hint (reserved for markdown export)"
    )]
    code_language: Option<String>,
    #[arg(long, help = "Skip YAML frontmatter")]
    no_frontmatter: bool,
    #[arg(long, help = "Include subpages for MediaWiki page exports")]
    subpages: bool,
    #[arg(long, help = "With --subpages, combine all pages into one output")]
    combined: bool,
    #[arg(
        long,
        value_name = "N",
        help = "Maximum total pages to export with --subpages, including the parent page"
    )]
    limit: Option<usize>,
}

pub(crate) fn run_fetch(runtime: &RuntimeOptions, args: FetchArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let format = ExternalFetchFormat::from(args.format);
    let result = fetch_page_by_url(
        &args.url,
        &ExternalFetchOptions {
            format,
            max_bytes: 1_000_000,
            profile: ExternalFetchProfile::Legacy,
        },
    )?
    .ok_or_else(|| anyhow::anyhow!("page not found: {}", args.url))?;

    println!("fetch");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("source_url: {}", args.url);
    println!("resolved_url: {}", result.url);
    println!("title: {}", result.title);
    println!("source_wiki: {}", result.source_wiki);
    println!("source_domain: {}", result.source_domain);
    println!("content_format: {}", result.content_format);
    if let Some(value) = result.revision_id {
        println!("revision_id: {value}");
    }
    if let Some(value) = result.display_title.as_deref() {
        println!("display_title: {value}");
    }
    if let Some(value) = result.rendered_fetch_mode {
        println!("rendered_fetch_mode: {}", format_rendered_fetch_mode(value));
    }
    println!("content_length: {}", result.content.len());

    if args.save {
        let safe_name = args
            .name
            .as_deref()
            .map(sanitize_filename)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                let fallback = sanitize_filename(&result.title);
                if fallback.is_empty() {
                    "external-page".to_string()
                } else {
                    fallback
                }
            });
        let relative_path = format!("reference/{}/{}.wiki", result.source_wiki, safe_name);
        let absolute_path = paths.project_root.join(relative_path.replace('/', "\\"));
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
        }
        fs::write(&absolute_path, result.content.as_bytes())
            .with_context(|| format!("failed to write {}", normalize_path(&absolute_path)))?;
        println!("saved: yes");
        println!("saved_path: {}", normalize_path(&absolute_path));
    } else {
        println!("saved: no");
        println!("content:");
        println!("{}", result.content);
    }

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

pub(crate) fn run_export(runtime: &RuntimeOptions, args: ExportArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let export_format = ExportFormat::from(args.format);
    let fetch_options = ExternalFetchOptions {
        format: ExternalFetchFormat::Wikitext,
        max_bytes: 1_000_000,
        profile: ExternalFetchProfile::Legacy,
    };
    validate_export_args(&args, export_format)?;

    if let Some(urls_file) = args.urls_file.as_deref() {
        run_urls_file_export(
            &paths.project_root,
            urls_file,
            args.output_dir.as_deref(),
            !args.no_frontmatter,
            args.code_language.as_deref(),
        )?;

        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
        return Ok(());
    }

    let url = args
        .url
        .as_deref()
        .expect("validated URL positional is present");

    if args.subpages {
        let parsed = parse_wiki_url(url).ok_or_else(|| {
            anyhow::anyhow!("subpages export requires a recognizable MediaWiki URL")
        })?;
        let parent_title = parsed.title.trim_end_matches('/').to_string();
        let mut all_pages = Vec::new();

        if let Some(main_page) =
            fetch_mediawiki_export_page(&parent_title, &parsed, &fetch_options)?
        {
            all_pages.push(main_page);
        }

        let remaining = remaining_subpage_limit(args.limit, all_pages.len());
        if remaining > 0 {
            // Tree exports should walk the full MediaWiki allpages continuation chain
            // unless the caller supplies a bounded stress-test limit.
            let subpage_titles = list_subpages(&parent_title, &parsed, remaining)?;
            let subpages = fetch_pages_by_titles(&subpage_titles, &parsed, &fetch_options)?;
            all_pages.extend(subpages);
        }
        if all_pages.is_empty() {
            bail!("no pages found for export target: {}", url);
        }

        if args.combined {
            let combined = render_combined_export(
                &all_pages,
                export_format,
                !args.no_frontmatter,
                args.code_language.as_deref(),
                &parsed.domain,
                url,
                &parent_title,
            );
            let output_path = args.output.clone().or_else(|| {
                default_export_path(&paths.project_root, &parent_title, false, export_format)
            });
            write_or_print_export(&combined, output_path.as_deref())?;

            println!("export");
            println!("mode: subpages_combined");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("source_url: {}", url);
            println!("pages_exported: {}", all_pages.len());
            if let Some(limit) = args.limit {
                println!("page_limit: {limit}");
            }
            println!("format: {}", args.format.as_str());
            if let Some(path) = output_path {
                println!("output_path: {}", normalize_path(&path));
            } else {
                println!("output_path: <stdout>");
            }
        } else {
            let output_dir = args
                .output
                .clone()
                .or_else(|| {
                    default_export_path(&paths.project_root, &parent_title, true, export_format)
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("output directory is required for subpage export")
                })?;
            fs::create_dir_all(&output_dir)
                .with_context(|| format!("failed to create {}", normalize_path(&output_dir)))?;

            let filenames = build_subpage_output_filenames(&all_pages, export_format);

            for (page, filename) in all_pages.iter().zip(filenames.iter()) {
                let rendered = render_export_page(
                    page,
                    export_format,
                    !args.no_frontmatter,
                    args.code_language.as_deref(),
                    &parsed.domain,
                );
                let output_file = output_dir.join(filename);
                fs::write(&output_file, rendered.as_bytes())
                    .with_context(|| format!("failed to write {}", normalize_path(&output_file)))?;
            }

            let index_content =
                build_subpage_index(&all_pages, &filenames, &parsed.domain, url, &parent_title);
            let index_path = output_dir.join("_index.md");
            fs::write(&index_path, index_content.as_bytes())
                .with_context(|| format!("failed to write {}", normalize_path(&index_path)))?;

            println!("export");
            println!("mode: subpages_separate");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("source_url: {}", url);
            println!("pages_exported: {}", all_pages.len());
            if let Some(limit) = args.limit {
                println!("page_limit: {limit}");
            }
            println!("format: {}", args.format.as_str());
            println!("output_dir: {}", normalize_path(&output_dir));
            println!("index_path: {}", normalize_path(&index_path));
        }
    } else {
        let page = fetch_single_export_page(url, export_format)?;
        let rendered = render_export_page(
            &page,
            export_format,
            !args.no_frontmatter,
            args.code_language.as_deref(),
            &page.source_domain,
        );
        let output_path = args.output.clone().or_else(|| {
            default_export_path(&paths.project_root, &page.title, false, export_format)
        });
        write_or_print_export(&rendered, output_path.as_deref())?;

        println!("export");
        println!("mode: single");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("source_url: {}", url);
        println!("resolved_url: {}", page.url);
        println!("title: {}", page.title);
        println!("format: {}", args.format.as_str());
        println!("source_domain: {}", page.source_domain);
        println!("content_length: {}", page.content.len());
        if let Some(path) = output_path {
            println!("output_path: {}", normalize_path(&path));
        } else {
            println!("output_path: <stdout>");
        }
    }

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn validate_export_args(args: &ExportArgs, export_format: ExportFormat) -> Result<()> {
    let has_url = args
        .url
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let has_urls_file = args.urls_file.is_some();

    if has_url == has_urls_file {
        bail!("exactly one of URL or --urls-file is required");
    }
    if args.limit == Some(0) {
        bail!("--limit must be greater than 0");
    }

    if has_urls_file {
        if args.subpages {
            bail!("--urls-file conflicts with --subpages");
        }
        if args.combined {
            bail!("--urls-file conflicts with --combined");
        }
        if args.output.is_some() {
            bail!("--urls-file uses --output-dir; --output is for single-page and subpage exports");
        }
        if export_format == ExportFormat::Wikitext {
            bail!("--urls-file supports markdown export only");
        }
    } else if args.output_dir.is_some() {
        bail!("--output-dir requires --urls-file");
    }
    if args.combined && !args.subpages {
        bail!("--combined requires --subpages");
    }
    if args.limit.is_some() && !args.subpages {
        bail!("--limit requires --subpages");
    }

    Ok(())
}

fn remaining_subpage_limit(limit: Option<usize>, pages_collected: usize) -> usize {
    limit
        .map(|limit| limit.saturating_sub(pages_collected))
        .unwrap_or(usize::MAX)
}

fn run_urls_file_export(
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
        fs::write(&output_file, rendered.as_bytes())
            .with_context(|| format!("failed to write {}", normalize_path(&output_file)))?;
        success.output_file = filename;
    }

    let index_content = build_url_batch_index(&successes, &failures, urls_file);
    let index_path = output_dir.join("_index.md");
    fs::write(&index_path, index_content.as_bytes())
        .with_context(|| format!("failed to write {}", normalize_path(&index_path)))?;

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

fn read_urls_file(path: &Path) -> Result<Vec<String>> {
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
struct UrlBatchSuccess {
    source_url: String,
    page: ExternalFetchResult,
    output_file: String,
}

#[derive(Debug)]
struct UrlBatchFailure {
    source_url: String,
    source_domain: String,
    error: String,
}

fn build_url_batch_output_filenames(successes: &[UrlBatchSuccess]) -> Vec<String> {
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

fn build_url_batch_index(
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

fn fetch_mediawiki_export_page(
    title: &str,
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<Option<ExternalFetchResult>> {
    fetch_mediawiki_page(title, parsed, options)
}

fn fetch_single_export_page(url: &str, export_format: ExportFormat) -> Result<ExternalFetchResult> {
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

fn render_export_page(
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

fn render_combined_export(
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

fn build_subpage_index(
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

fn build_subpage_output_filenames(
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

fn export_filename_stem(title: &str) -> String {
    let stem = sanitize_filename(title);
    if !stem.is_empty() {
        return stem;
    }

    let hash = compute_hash(title);
    format!("page-{}", &hash[..8])
}

fn write_or_print_export(content: &str, output_path: Option<&Path>) -> Result<()> {
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

fn now_timestamp_string() -> String {
    wikitool_core::support::now_iso8601_utc()
}

fn format_rendered_fetch_mode(mode: wikitool_core::external::RenderedFetchMode) -> &'static str {
    match mode {
        wikitool_core::external::RenderedFetchMode::ParseApi => "parse_api",
    }
}

fn format_fetch_mode(mode: wikitool_core::external::FetchMode) -> &'static str {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        UrlBatchSuccess, build_subpage_output_filenames, build_url_batch_index,
        build_url_batch_output_filenames, read_urls_file, remaining_subpage_limit,
        render_export_page, validate_export_args,
    };
    use crate::cli_support::ExportContentFormat;
    use wikitool_core::external::{
        ExportFormat, ExternalFetchResult, ExtractionQuality, FetchMode,
    };

    fn page(title: &str) -> ExternalFetchResult {
        ExternalFetchResult {
            title: title.to_string(),
            content: String::new(),
            fetched_at: String::new(),
            revision_timestamp: None,
            extract: None,
            url: String::new(),
            source_wiki: String::new(),
            source_domain: String::new(),
            content_format: String::new(),
            content_hash: String::new(),
            revision_id: None,
            display_title: None,
            rendered_fetch_mode: None,
            canonical_url: None,
            site_name: None,
            byline: None,
            published_at: None,
            fetch_mode: None,
            extraction_quality: None,
            fetch_attempts: Vec::new(),
        }
    }

    fn export_args() -> super::ExportArgs {
        super::ExportArgs {
            url: Some("https://example.org/page".to_string()),
            urls_file: None,
            output: None,
            output_dir: None,
            format: ExportContentFormat::Markdown,
            code_language: None,
            no_frontmatter: false,
            subpages: false,
            combined: false,
            limit: None,
        }
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "wikitool-export-cli-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp test dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn build_subpage_output_filenames_disambiguates_case_collisions() {
        let filenames = build_subpage_output_filenames(
            &[
                page("DB/GtArmorMitigationByLvl"),
                page("DB/gtArmorMitigationByLvl"),
            ],
            ExportFormat::Markdown,
        );

        assert_eq!(filenames.len(), 2);
        assert_ne!(
            filenames[0].to_ascii_lowercase(),
            filenames[1].to_ascii_lowercase()
        );
        assert!(filenames[0].ends_with(".md"));
        assert!(filenames[1].ends_with(".md"));
    }

    #[test]
    fn render_export_page_uses_content_format_and_agent_metadata() {
        let mut page = page("Readable Source");
        page.content = "Readable paragraph.\n\nSecond paragraph.".to_string();
        page.content_format = "text".to_string();
        page.content_hash = "hash".to_string();
        page.source_wiki = "web".to_string();
        page.source_domain = "example.org".to_string();
        page.url = "https://example.org/source".to_string();
        page.canonical_url = Some("https://example.org/source".to_string());
        page.site_name = Some("Example".to_string());
        page.fetch_mode = Some(FetchMode::Static);
        page.extraction_quality = Some(ExtractionQuality::High);

        let rendered = render_export_page(
            &page,
            ExportFormat::Markdown,
            true,
            None,
            &page.source_domain,
        );

        assert!(rendered.contains("content_format: \"text\""));
        assert!(rendered.contains("canonical_url: \"https://example.org/source\""));
        assert!(rendered.contains("extraction_quality: \"high\""));
        assert!(rendered.contains("Readable paragraph.\n\nSecond paragraph."));
    }

    #[test]
    fn validate_export_args_rejects_zero_subpage_limit() {
        let mut args = export_args();
        args.subpages = true;
        args.limit = Some(0);

        let error = validate_export_args(&args, ExportFormat::Markdown).unwrap_err();

        assert!(error.to_string().contains("--limit must be greater than 0"));
    }

    #[test]
    fn validate_export_args_rejects_urls_file_wikitext_format() {
        let mut args = export_args();
        args.url = None;
        args.urls_file = Some(PathBuf::from("urls.txt"));

        let error = validate_export_args(&args, ExportFormat::Wikitext).unwrap_err();

        assert!(error.to_string().contains("markdown export only"));
    }

    #[test]
    fn remaining_subpage_limit_caps_total_pages_including_parent() {
        assert_eq!(remaining_subpage_limit(Some(25), 1), 24);
        assert_eq!(remaining_subpage_limit(Some(1), 1), 0);
        assert_eq!(remaining_subpage_limit(Some(1), 0), 1);
        assert_eq!(remaining_subpage_limit(None, 12), usize::MAX);
    }

    #[test]
    fn read_urls_file_ignores_comments_and_blank_lines() {
        let temp = TestDir::new("urls-file");
        let path = temp.path.join("sources.txt");
        fs::write(
            &path,
            "\u{feff}# comment\nhttps://example.org/a\n  \nhttps://example.org/b  \n",
        )
        .expect("write urls file");

        let urls = read_urls_file(&path).expect("read urls file");

        assert_eq!(
            urls,
            vec![
                "https://example.org/a".to_string(),
                "https://example.org/b".to_string()
            ]
        );
    }

    #[test]
    fn build_url_batch_output_filenames_disambiguates_duplicate_titles() {
        let mut first = page("Shared Title");
        first.url = "https://example.org/one".to_string();
        first.source_domain = "example.org".to_string();
        let mut second = page("Shared Title");
        second.url = "https://example.net/two".to_string();
        second.source_domain = "example.net".to_string();
        let successes = vec![
            UrlBatchSuccess {
                source_url: first.url.clone(),
                page: first,
                output_file: String::new(),
            },
            UrlBatchSuccess {
                source_url: second.url.clone(),
                page: second,
                output_file: String::new(),
            },
        ];

        let filenames = build_url_batch_output_filenames(&successes);

        assert_eq!(filenames.len(), 2);
        assert_ne!(filenames[0], filenames[1]);
        assert!(filenames[0].starts_with("Shared-Title-"));
        assert!(filenames[1].starts_with("Shared-Title-"));
        assert!(filenames.iter().all(|filename| filename.ends_with(".md")));
    }

    #[test]
    fn build_url_batch_index_lists_successes_and_failures() {
        let mut page = page("Readable Source");
        page.source_domain = "example.org".to_string();
        let successes = vec![UrlBatchSuccess {
            source_url: "https://example.org/source".to_string(),
            page,
            output_file: "Readable-Source.md".to_string(),
        }];
        let failures = vec![super::UrlBatchFailure {
            source_url: "https://example.com/blocked".to_string(),
            source_domain: "example.com".to_string(),
            error: "access challenge prevented readable fetch".to_string(),
        }];

        let index = build_url_batch_index(&successes, &failures, Path::new("sources.txt"));

        assert!(index.contains("https://example.org/source"));
        assert!(index.contains("[Readable-Source.md](./Readable-Source.md)"));
        assert!(index.contains("Readable Source"));
        assert!(index.contains("example.org"));
        assert!(index.contains("https://example.com/blocked"));
        assert!(index.contains("access challenge prevented readable fetch"));
    }
}
