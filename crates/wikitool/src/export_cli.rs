use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use wikitool_core::external::{
    ExportFormat, ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile,
    ExternalFetchResult, ParsedWikiUrl, default_export_path, fetch_mediawiki_page,
    fetch_page_by_url, fetch_pages_by_titles, generate_frontmatter, list_subpages, parse_wiki_url,
    sanitize_filename, wikitext_to_markdown,
};

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct FetchArgs {
    url: String,
    #[arg(
        long,
        default_value = "wikitext",
        value_name = "FORMAT",
        help = "Output format: wikitext|html|rendered-html"
    )]
    format: String,
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
        default_value = "markdown",
        value_name = "FORMAT",
        help = "Output format: markdown|wikitext"
    )]
    format: String,
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
    let format = ExternalFetchFormat::parse(&args.format)?;
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
    let export_format = ExportFormat::parse(&args.format)?;
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

        let subpage_titles = list_subpages(&parent_title, &parsed, 500)?;
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
            println!("format: {}", args.format.to_ascii_lowercase());
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

            for page in &all_pages {
                let rendered = render_export_page(
                    page,
                    export_format,
                    !args.no_frontmatter,
                    args.code_language.as_deref(),
                    &parsed.domain,
                );
                let filename = format!(
                    "{}.{}",
                    sanitize_filename(&page.title),
                    export_format.file_extension()
                );
                let output_file = output_dir.join(filename);
                fs::write(&output_file, rendered.as_bytes())
                    .with_context(|| format!("failed to write {}", normalize_path(&output_file)))?;
            }

            let index_content = build_subpage_index(
                &all_pages,
                &parsed.domain,
                &args.url,
                &parent_title,
                export_format,
            );
            let index_path = output_dir.join("_index.md");
            fs::write(&index_path, index_content.as_bytes())
                .with_context(|| format!("failed to write {}", normalize_path(&index_path)))?;

            println!("export");
            println!("mode: subpages_separate");
            println!("project_root: {}", normalize_path(&paths.project_root));
            println!("source_url: {}", args.url);
            println!("pages_exported: {}", all_pages.len());
            println!("format: {}", args.format.to_ascii_lowercase());
            println!("output_dir: {}", normalize_path(&output_dir));
            println!("index_path: {}", normalize_path(&index_path));
        }
    } else {
        let page = fetch_page_by_url(&args.url, &fetch_options)?
            .ok_or_else(|| anyhow::anyhow!("page not found: {}", args.url))?;
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
        println!("format: {}", args.format.to_ascii_lowercase());
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

fn render_export_page(
    page: &ExternalFetchResult,
    export_format: ExportFormat,
    include_frontmatter: bool,
    code_language: Option<&str>,
    domain: &str,
) -> String {
    let converted = match export_format {
        ExportFormat::Wikitext => page.content.clone(),
        ExportFormat::Markdown => wikitext_to_markdown(&page.content, code_language),
    };
    if !include_frontmatter {
        return converted;
    }
    let frontmatter = generate_frontmatter(
        &page.title,
        &page.url,
        domain,
        &page.timestamp,
        &[(
            "format".to_string(),
            export_format.file_extension().to_string(),
        )],
    );
    format!("{frontmatter}\n{converted}")
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
            ExportFormat::Markdown => wikitext_to_markdown(&page.content, code_language),
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
    domain: &str,
    source_url: &str,
    title: &str,
    export_format: ExportFormat,
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
    for page in pages {
        let filename = format!(
            "{}.{}",
            sanitize_filename(&page.title),
            export_format.file_extension()
        );
        lines.push(format!("- [{}](./{})", page.title, filename));
    }
    lines.join("\n")
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
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn format_rendered_fetch_mode(mode: wikitool_core::external::RenderedFetchMode) -> &'static str {
    match mode {
        wikitool_core::external::RenderedFetchMode::ParseApi => "parse_api",
    }
}
