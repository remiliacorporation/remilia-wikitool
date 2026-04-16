use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use wikitool_core::external::{
    ExportFormat, ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile,
    ExternalFetchResult, ParsedWikiUrl, default_export_path, fetch_mediawiki_page,
    fetch_page_by_url, fetch_pages_by_titles, generate_frontmatter, list_subpages, parse_wiki_url,
    sanitize_filename, source_content_to_markdown,
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
    url: String,
    #[arg(
        short = 'o',
        long,
        value_name = "PATH",
        help = "Output file or directory path"
    )]
    output: Option<PathBuf>,
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

    if args.subpages {
        let parsed = parse_wiki_url(&args.url).ok_or_else(|| {
            anyhow::anyhow!("subpages export requires a recognizable MediaWiki URL")
        })?;
        let parent_title = parsed.title.trim_end_matches('/').to_string();
        let mut all_pages = Vec::new();

        if let Some(main_page) =
            fetch_mediawiki_export_page(&parent_title, &parsed, &fetch_options)?
        {
            all_pages.push(main_page);
        }

        // Tree exports should walk the full MediaWiki allpages continuation chain.
        let subpage_titles = list_subpages(&parent_title, &parsed, usize::MAX)?;
        let subpages = fetch_pages_by_titles(&subpage_titles, &parsed, &fetch_options)?;
        all_pages.extend(subpages);
        if all_pages.is_empty() {
            bail!("no pages found for export target: {}", args.url);
        }

        if args.combined {
            let combined = render_combined_export(
                &all_pages,
                export_format,
                !args.no_frontmatter,
                args.code_language.as_deref(),
                &parsed.domain,
                &args.url,
                &parent_title,
            );
            let output_path = args.output.clone().or_else(|| {
                default_export_path(&paths.project_root, &parent_title, false, export_format)
            });
            write_or_print_export(&combined, output_path.as_deref())?;

            println!("export");
            println!("mode: subpages_combined");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("source_url: {}", args.url);
            println!("pages_exported: {}", all_pages.len());
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

            let index_content = build_subpage_index(
                &all_pages,
                &filenames,
                &parsed.domain,
                &args.url,
                &parent_title,
            );
            let index_path = output_dir.join("_index.md");
            fs::write(&index_path, index_content.as_bytes())
                .with_context(|| format!("failed to write {}", normalize_path(&index_path)))?;

            println!("export");
            println!("mode: subpages_separate");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("source_url: {}", args.url);
            println!("pages_exported: {}", all_pages.len());
            println!("format: {}", args.format.as_str());
            println!("output_dir: {}", normalize_path(&output_dir));
            println!("index_path: {}", normalize_path(&index_path));
        }
    } else {
        let page = fetch_single_export_page(&args.url, export_format)?;
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
        println!("source_url: {}", args.url);
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
    use super::{build_subpage_output_filenames, render_export_page};
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
}
