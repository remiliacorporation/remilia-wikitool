use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

mod batch;
mod render;
mod subpages;

use batch::run_urls_file_export;
use clap::Args;
use render::{
    fetch_mediawiki_export_page, fetch_single_export_page, format_rendered_fetch_mode,
    render_export_page, write_or_print_export,
};
use subpages::{
    build_subpage_index, build_subpage_output_filenames, remaining_subpage_limit,
    render_combined_export,
};
use wikitool_core::external::{
    ExportFormat, ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile,
    default_export_path, fetch_page_by_url, fetch_pages_by_titles, list_subpages, parse_wiki_url,
    sanitize_filename,
};

use crate::cli_support::{
    ExportContentFormat, FetchContentFormat, normalize_path, resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::batch::{
        UrlBatchSuccess, build_url_batch_index, build_url_batch_output_filenames, read_urls_file,
    };
    use super::render::render_export_page;
    use super::subpages::{build_subpage_output_filenames, remaining_subpage_limit};
    use super::validate_export_args;
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
        let failures = vec![super::batch::UrlBatchFailure {
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
