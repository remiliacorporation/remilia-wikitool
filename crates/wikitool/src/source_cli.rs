use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::source::{
    ChallengeHandoff, ExternalFetchAttempt, ExternalFetchFailureError, ExternalFetchFormat,
    ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult, ExternalMachineSurfaceReport,
    MachineSurfaceDiscoveryOptions, SourceCacheOptions, SourceCacheStatus, WebArchiveOptions,
    archive_web_site, discover_machine_surfaces, fetch_page_by_url_cached,
};
use wikitool_core::source::{ExternalSearchHit, ExternalSearchReport, MediaWikiSearchWhat};

use crate::cli_support::{
    FetchContentFormat, OutputFormat, normalize_path, resolve_runtime_with_config,
};
use crate::query_cli::{
    RemoteSearchScope, RemoteWikiSearchRequest, print_external_search_report,
    remote_wiki_search_report,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

mod mediawiki_templates;
mod session;

#[derive(Debug, Args)]
pub(crate) struct SourceArgs {
    #[command(subcommand)]
    command: SourceSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SourceSubcommand {
    #[command(
        name = "wiki-search",
        about = "Search the configured wiki API for subject evidence"
    )]
    WikiSearch(SourceSearchArgs),
    #[command(about = "Fetch readable reference material from a URL")]
    Fetch(SourceFetchArgs),
    #[command(about = "Mirror raw web pages and requisites into a local manifest archive")]
    Archive(SourceArchiveArgs),
    #[command(about = "Discover public machine-readable source surfaces for a URL")]
    Discover(SourceDiscoverArgs),
    #[command(about = "Manage human-solved source access sessions")]
    Session(session::SourceAccessSessionArgs),
    #[command(about = "Inspect live template contracts used by a source MediaWiki page")]
    MediawikiTemplates(mediawiki_templates::SourceMediaWikiTemplatesArgs),
}

#[derive(Debug, Args)]
pub(crate) struct SourceSearchArgs {
    query: String,
    #[arg(long, default_value_t = 20, value_name = "N")]
    limit: usize,
    #[arg(
        long,
        value_enum,
        default_value_t = RemoteSearchScope::Text,
        value_name = "SCOPE",
        help = "Search scope: text|title|nearmatch"
    )]
    what: RemoteSearchScope,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct SourceFetchArgs {
    url: String,
    #[arg(
        long,
        value_enum,
        default_value_t = FetchContentFormat::Html,
        value_name = "FORMAT",
        help = "Output format: wikitext|html|rendered-html"
    )]
    format: FetchContentFormat,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output wrapper: text|json"
    )]
    output: OutputFormat,
    #[arg(long, help = "Refresh the source cache entry before returning output")]
    refresh: bool,
    #[arg(long, help = "Bypass the source cache for this fetch")]
    no_cache: bool,
    #[arg(
        long,
        value_name = "CHARS",
        help = "Limit returned content characters; cached source content remains complete"
    )]
    content_limit: Option<usize>,
    #[arg(
        long,
        help = "Omit fetched content from output while keeping metadata and extract"
    )]
    no_content: bool,
    #[arg(long, help = "Skip machine-surface discovery when a fetch fails")]
    no_discover: bool,
    #[arg(
        long,
        default_value_t = 12,
        value_name = "N",
        help = "Limit machine-surface entries included with failed fetch diagnostics"
    )]
    discover_limit: usize,
}

#[derive(Debug, Args)]
pub(crate) struct SourceDiscoverArgs {
    url: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
    #[arg(
        long,
        default_value_t = 20,
        value_name = "N",
        help = "Limit machine-surface entries"
    )]
    limit: usize,
}

#[derive(Debug, Args)]
pub(crate) struct SourceArchiveArgs {
    url: String,
    #[arg(
        long,
        value_name = "PATH",
        help = "Write archive files to this directory"
    )]
    output_dir: Option<std::path::PathBuf>,
    #[arg(
        long,
        default_value_t = 1_000,
        value_name = "N",
        help = "Maximum URLs to attempt"
    )]
    max_pages: usize,
    #[arg(
        long,
        default_value_t = 50_000_000,
        value_name = "BYTES",
        help = "Maximum bytes to store for a single response"
    )]
    max_bytes: usize,
    #[arg(
        long,
        default_value_t = 8,
        value_name = "N",
        help = "Maximum link depth from the seed URL (seed is depth 0)"
    )]
    max_depth: usize,
    #[arg(
        long,
        default_value_t = 1_000_000_000,
        value_name = "BYTES",
        help = "Maximum total bytes to store across the whole crawl"
    )]
    max_total_bytes: usize,
    #[arg(long, help = "Allow crawling linked URLs outside the source host")]
    span_hosts: bool,
    #[arg(
        long,
        help = "Do not enqueue linked page requisites such as CSS image URLs"
    )]
    no_page_requisites: bool,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct SourceSearchOutput {
    schema_version: String,
    source_scope: String,
    query: String,
    what: MediaWikiSearchWhat,
    namespaces: Vec<i32>,
    count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_hits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rewritten_query: Option<String>,
    hits: Vec<ExternalSearchHit>,
}

#[derive(Debug, Serialize)]
struct SourceFetchOutput {
    schema_version: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_status: Option<SourceCacheStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<SourceFetchContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<ExternalFetchResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<SourceFetchErrorOutput>,
}

#[derive(Debug, Serialize)]
struct SourceFetchErrorOutput {
    source_url: String,
    kind: String,
    message: String,
    attempts: Vec<ExternalFetchAttempt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    challenge_handoffs: Vec<ChallengeHandoff>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery: Option<ExternalMachineSurfaceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct SourceFetchContent {
    full_length: usize,
    returned_length: usize,
    truncated: bool,
    omitted: bool,
    limit: Option<usize>,
}

pub(crate) fn run_source(runtime: &RuntimeOptions, args: SourceArgs) -> Result<()> {
    match args.command {
        SourceSubcommand::WikiSearch(args) => {
            run_source_wiki_search(runtime, args, "source wiki-search")
        }
        SourceSubcommand::Fetch(args) => run_source_fetch(runtime, args),
        SourceSubcommand::Archive(args) => run_source_archive(runtime, args),
        SourceSubcommand::Discover(args) => run_source_discover(runtime, args),
        SourceSubcommand::Session(args) => session::run(runtime, args),
        SourceSubcommand::MediawikiTemplates(args) => mediawiki_templates::run(runtime, args),
    }
}

fn run_source_archive(runtime: &RuntimeOptions, args: SourceArchiveArgs) -> Result<()> {
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    let report = archive_web_site(
        &paths,
        &args.url,
        WebArchiveOptions {
            max_pages: args.max_pages,
            max_bytes: args.max_bytes,
            max_depth: args.max_depth,
            max_total_bytes: args.max_total_bytes,
            same_host_only: !args.span_hosts,
            include_page_requisites: !args.no_page_requisites,
            output_dir: args.output_dir,
        },
    )?;

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("source archive");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("source_url: {}", report.source_url);
    println!("origin_host: {}", report.origin_host);
    println!("output_dir: {}", report.output_dir);
    println!("attempted: {}", report.attempted);
    println!("succeeded: {}", report.succeeded);
    println!("failed: {}", report.failed);
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_source_wiki_search(
    runtime: &RuntimeOptions,
    args: SourceSearchArgs,
    command_name: &'static str,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let report = remote_wiki_search_report(
        &config,
        RemoteWikiSearchRequest {
            command_name,
            query: &args.query,
            limit: args.limit,
            what: args.what,
        },
    )?;

    if args.format.is_json() {
        let output = SourceSearchOutput::from(report);
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("{command_name}");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("source_scope: configured_wiki_api");
    println!("note: this searches the configured target wiki, not the open web");
    println!("query: {}", report.query);
    print_external_search_report("source_search", &report);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_source_discover(runtime: &RuntimeOptions, args: SourceDiscoverArgs) -> Result<()> {
    if args.limit == 0 {
        bail!("source discover requires --limit >= 1");
    }
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    let report = discover_machine_surfaces(
        &args.url,
        MachineSurfaceDiscoveryOptions {
            max_bytes: 1_000_000,
            surface_limit: args.limit,
            probe_source_page: true,
            source_known_blocked: false,
        },
    )?;

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("source discover");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("source_url: {}", report.source_url);
    println!("origin_url: {}", report.origin_url);
    println!("content_signals: {}", report.content_signals.len());
    for signal in &report.content_signals {
        println!(
            "content_signal: {}={} ({}, line {})",
            signal.key, signal.value, signal.source_url, signal.line
        );
    }
    println!("surfaces: {}", report.surfaces.len());
    for surface in &report.surfaces {
        println!(
            "surface: {} {} [{}]",
            surface.kind, surface.url, surface.source
        );
    }
    println!("access_routes: {}", report.access_routes.len());
    for route in &report.access_routes {
        println!(
            "access_route: {} {} - {}",
            route.kind, route.status, route.description
        );
    }
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_source_fetch(runtime: &RuntimeOptions, args: SourceFetchArgs) -> Result<()> {
    if args.refresh && args.no_cache {
        bail!("source fetch does not allow --refresh together with --no-cache");
    }
    if args.no_content && args.content_limit.is_some() {
        bail!("source fetch does not allow --no-content together with --content-limit");
    }
    if matches!(args.content_limit, Some(0)) {
        bail!("source fetch requires --content-limit >= 1");
    }
    if args.discover_limit == 0 {
        bail!("source fetch requires --discover-limit >= 1");
    }
    let fetch_format = ExternalFetchFormat::from(args.format);
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    let cached = match fetch_page_by_url_cached(
        &paths,
        &args.url,
        &ExternalFetchOptions {
            format: fetch_format,
            max_bytes: 1_000_000,
            profile: ExternalFetchProfile::ReadableDocument,
            session: None,
        },
        &SourceCacheOptions {
            use_cache: !args.no_cache,
            refresh: args.refresh,
        },
    ) {
        Ok(Some(cached)) => cached,
        Ok(None) => {
            return handle_source_fetch_error(
                runtime,
                &paths,
                &args,
                SourceFetchErrorOutput {
                    source_url: args.url.clone(),
                    kind: "not_found".to_string(),
                    message: format!("page not found: {}", args.url),
                    attempts: Vec::new(),
                    challenge_handoffs: Vec::new(),
                    discovery: None,
                    discovery_error: None,
                },
            );
        }
        Err(error) => {
            return handle_source_fetch_error(
                runtime,
                &paths,
                &args,
                source_fetch_error_output(&args.url, &error),
            );
        }
    };
    let cache_status = cached.status;
    let cache_path = cached.cache_path.as_deref().map(normalize_path);
    let (result, content) =
        prepare_fetch_result(cached.result, args.content_limit, args.no_content);

    if args.output.is_json() {
        let output = SourceFetchOutput {
            schema_version: "source_document_v2".to_string(),
            status: "ok",
            cache_status: Some(cache_status),
            cache_path,
            content: Some(content),
            result: Some(result),
            error: None,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("source fetch");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("source_url: {}", args.url);
    println!("resolved_url: {}", result.url);
    println!("title: {}", result.title);
    println!("content_format: {}", result.content_format);
    println!("cache_status: {}", format_cache_status(cache_status));
    if let Some(value) = cache_path.as_deref() {
        println!("cache_path: {value}");
    }
    println!("content_hash: {}", result.content_hash);
    if let Some(value) = result.revision_id {
        println!("revision_id: {value}");
    }
    if let Some(value) = result.display_title.as_deref() {
        println!("display_title: {value}");
    }
    if let Some(value) = result.rendered_fetch_mode {
        println!("rendered_fetch_mode: {}", format_rendered_fetch_mode(value));
    }
    if let Some(value) = result.canonical_url.as_deref() {
        println!("canonical_url: {value}");
    }
    if let Some(value) = result.site_name.as_deref() {
        println!("site_name: {value}");
    }
    if let Some(value) = result.byline.as_deref() {
        println!("byline: {value}");
    }
    if let Some(value) = result.published_at.as_deref() {
        println!("published_at: {value}");
    }
    if let Some(value) = result.fetch_mode {
        println!("fetch_mode: {}", format_fetch_mode(value));
    }
    if let Some(value) = result.extraction_quality {
        println!("extraction_quality: {}", format_extraction_quality(value));
    }
    println!("content_full_length: {}", content.full_length);
    println!("content_returned_length: {}", content.returned_length);
    println!("content_truncated: {}", content.truncated);
    println!("content_omitted: {}", content.omitted);
    if !content.omitted {
        println!("content:");
        println!("{}", result.content);
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn handle_source_fetch_error(
    runtime: &RuntimeOptions,
    paths: &wikitool_core::runtime::ResolvedPaths,
    args: &SourceFetchArgs,
    mut error: SourceFetchErrorOutput,
) -> Result<()> {
    if args.output.is_json() {
        if !args.no_discover {
            match discover_machine_surfaces(
                &args.url,
                MachineSurfaceDiscoveryOptions {
                    max_bytes: 1_000_000,
                    surface_limit: args.discover_limit,
                    probe_source_page: false,
                    source_known_blocked: true,
                },
            ) {
                Ok(report) => error.discovery = Some(report),
                Err(discovery_error) => error.discovery_error = Some(discovery_error.to_string()),
            }
        }
        let output = SourceFetchOutput {
            schema_version: "source_document_v2".to_string(),
            status: "error",
            cache_status: None,
            cache_path: None,
            content: None,
            result: None,
            error: Some(error),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        if runtime.diagnostics {
            eprintln!("\n[diagnostics]\n{}", paths.diagnostics());
        }
        return Ok(());
    }
    bail!("{}", error.message)
}

fn source_fetch_error_output(source_url: &str, error: &anyhow::Error) -> SourceFetchErrorOutput {
    if let Some(failure) = error.downcast_ref::<ExternalFetchFailureError>() {
        return SourceFetchErrorOutput {
            source_url: failure.failure.source_url.clone(),
            kind: failure.failure.kind.clone(),
            message: failure.failure.message.clone(),
            attempts: failure.failure.attempts.clone(),
            challenge_handoffs: failure.failure.challenge_handoffs.clone(),
            discovery: None,
            discovery_error: None,
        };
    }
    SourceFetchErrorOutput {
        source_url: source_url.to_string(),
        kind: "fetch_failed".to_string(),
        message: error.to_string(),
        attempts: Vec::new(),
        challenge_handoffs: Vec::new(),
        discovery: None,
        discovery_error: None,
    }
}

impl From<ExternalSearchReport> for SourceSearchOutput {
    fn from(report: ExternalSearchReport) -> Self {
        let ExternalSearchReport {
            query,
            what,
            namespaces,
            total_hits,
            suggestion,
            rewritten_query,
            hits,
        } = report;
        let count = hits.len();

        Self {
            schema_version: "source_search_v1".to_string(),
            source_scope: "configured_wiki_api".to_string(),
            query,
            what,
            namespaces,
            count,
            total_hits,
            suggestion,
            rewritten_query,
            hits,
        }
    }
}

fn prepare_fetch_result(
    mut result: ExternalFetchResult,
    content_limit: Option<usize>,
    no_content: bool,
) -> (ExternalFetchResult, SourceFetchContent) {
    let full_length = result.content.chars().count();
    if no_content {
        result.content.clear();
        return (
            result,
            SourceFetchContent {
                full_length,
                returned_length: 0,
                truncated: false,
                omitted: true,
                limit: None,
            },
        );
    }

    let mut truncated = false;
    if let Some(limit) = content_limit {
        let (limited, was_truncated) = truncate_to_chars(&result.content, limit);
        result.content = limited;
        truncated = was_truncated;
    }
    let returned_length = result.content.chars().count();
    (
        result,
        SourceFetchContent {
            full_length,
            returned_length,
            truncated,
            omitted: false,
            limit: content_limit,
        },
    )
}

fn truncate_to_chars(value: &str, limit: usize) -> (String, bool) {
    if value.chars().count() <= limit {
        return (value.to_string(), false);
    }
    (value.chars().take(limit).collect(), true)
}

fn format_rendered_fetch_mode(mode: wikitool_core::source::RenderedFetchMode) -> &'static str {
    match mode {
        wikitool_core::source::RenderedFetchMode::ParseApi => "parse_api",
    }
}

fn format_fetch_mode(mode: wikitool_core::source::FetchMode) -> &'static str {
    match mode {
        wikitool_core::source::FetchMode::Static => "static",
    }
}

fn format_extraction_quality(quality: wikitool_core::source::ExtractionQuality) -> &'static str {
    match quality {
        wikitool_core::source::ExtractionQuality::Low => "low",
        wikitool_core::source::ExtractionQuality::Medium => "medium",
        wikitool_core::source::ExtractionQuality::High => "high",
    }
}

fn format_cache_status(status: SourceCacheStatus) -> &'static str {
    match status {
        SourceCacheStatus::Hit => "hit",
        SourceCacheStatus::Miss => "miss",
        SourceCacheStatus::Refresh => "refresh",
        SourceCacheStatus::Bypass => "bypass",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wikitool_core::source::ExternalFetchFailure;

    #[test]
    fn prepare_fetch_result_limits_returned_content_without_touching_metadata() {
        let result = sample_fetch_result("abcdef");

        let (result, content) = prepare_fetch_result(result, Some(3), false);

        assert_eq!(result.content, "abc");
        assert_eq!(result.content_hash, "hash");
        assert_eq!(content.full_length, 6);
        assert_eq!(content.returned_length, 3);
        assert!(content.truncated);
        assert!(!content.omitted);
    }

    #[test]
    fn prepare_fetch_result_can_omit_content() {
        let result = sample_fetch_result("abcdef");

        let (result, content) = prepare_fetch_result(result, None, true);

        assert!(result.content.is_empty());
        assert_eq!(content.full_length, 6);
        assert_eq!(content.returned_length, 0);
        assert!(!content.truncated);
        assert!(content.omitted);
    }

    #[test]
    fn source_fetch_error_output_preserves_structured_failure() {
        let error = anyhow::Error::new(ExternalFetchFailureError {
            failure: ExternalFetchFailure {
                source_url: "https://example.com/protected".to_string(),
                kind: "access_challenge".to_string(),
                message: "access challenge prevented readable fetch".to_string(),
                attempts: vec![ExternalFetchAttempt {
                    mode: "direct_static".to_string(),
                    url: "https://example.com/protected".to_string(),
                    outcome: "access_challenge".to_string(),
                    http_status: Some(403),
                    content_type: Some("text/html; charset=UTF-8".to_string()),
                    message: Some("cf-mitigated: challenge".to_string()),
                }],
                challenge_handoffs: vec![ChallengeHandoff {
                    vendor: "cloudflare".to_string(),
                    url: "https://example.com/protected".to_string(),
                    domain: "example.com".to_string(),
                    required_cookies: vec!["cf_clearance".to_string()],
                    user_agent_pin: "wikitool-test/1.0".to_string(),
                    suggested_argv: vec![
                        "wikitool".to_string(),
                        "source".to_string(),
                        "session".to_string(),
                        "import".to_string(),
                        "https://example.com/protected".to_string(),
                        "--cookies".to_string(),
                        "-".to_string(),
                    ],
                    suggested_command:
                        "wikitool source session import https://example.com/protected --cookies -"
                            .to_string(),
                    ttl_hint_seconds: Some(1_800),
                    notes: Vec::new(),
                }],
            },
        });

        let output = source_fetch_error_output("https://example.com/protected", &error);

        assert_eq!(output.source_url, "https://example.com/protected");
        assert_eq!(output.kind, "access_challenge");
        assert_eq!(output.attempts.len(), 1);
        assert_eq!(output.attempts[0].outcome, "access_challenge");
        assert_eq!(output.attempts[0].http_status, Some(403));
        assert_eq!(output.challenge_handoffs.len(), 1);
        assert_eq!(output.challenge_handoffs[0].vendor, "cloudflare");
        assert_eq!(
            output.challenge_handoffs[0].required_cookies,
            vec!["cf_clearance"]
        );
        assert!(output.discovery.is_none());
        assert!(output.discovery_error.is_none());
    }

    fn sample_fetch_result(content: &str) -> ExternalFetchResult {
        ExternalFetchResult {
            title: "Source".to_string(),
            content: content.to_string(),
            fetched_at: "2026-04-15T00:00:00Z".to_string(),
            revision_timestamp: None,
            extract: Some("Extract".to_string()),
            url: "https://example.org/source".to_string(),
            source_wiki: "example".to_string(),
            source_domain: "example.org".to_string(),
            content_format: "html".to_string(),
            content_hash: "hash".to_string(),
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
}
