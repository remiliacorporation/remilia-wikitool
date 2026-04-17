use anyhow::{Result, bail};
use clap::{ArgAction, Args, Subcommand};
use serde::Serialize;
use wikitool_core::research::{
    ExternalFetchAttempt, ExternalFetchFailureError, ExternalFetchFormat, ExternalFetchOptions,
    ExternalFetchProfile, ExternalFetchResult, ExternalMachineSurfaceReport,
    MachineSurfaceDiscoveryOptions, MediaWikiTemplatePage, MediaWikiTemplateQueryOptions,
    MediaWikiTemplateReport, ResearchCacheOptions, ResearchCacheStatus, discover_machine_surfaces,
    fetch_mediawiki_template_report, fetch_page_by_url_cached,
};
use wikitool_core::sync::{ExternalSearchHit, ExternalSearchReport, MediaWikiSearchWhat};

use crate::cli_support::{
    FetchContentFormat, OutputFormat, normalize_path, resolve_runtime_with_config,
};
use crate::query_cli::{
    RemoteSearchScope, RemoteWikiSearchRequest, print_external_search_report,
    remote_wiki_search_report,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct ResearchArgs {
    #[command(subcommand)]
    command: ResearchSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ResearchSubcommand {
    #[command(about = "Search the remote wiki API for subject evidence")]
    Search(ResearchSearchArgs),
    #[command(about = "Fetch readable reference material from a URL")]
    Fetch(ResearchFetchArgs),
    #[command(about = "Discover public machine-readable source surfaces for a URL")]
    Discover(ResearchDiscoverArgs),
    #[command(about = "Inspect live template contracts used by a source MediaWiki page")]
    MediawikiTemplates(ResearchMediaWikiTemplatesArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ResearchSearchArgs {
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
pub(crate) struct ResearchFetchArgs {
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
    #[arg(
        long,
        help = "Refresh the research cache entry before returning output"
    )]
    refresh: bool,
    #[arg(long, help = "Bypass the research cache for this fetch")]
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
pub(crate) struct ResearchDiscoverArgs {
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
pub(crate) struct ResearchMediaWikiTemplatesArgs {
    url: String,
    #[arg(
        long,
        default_value_t = 16,
        value_name = "N",
        help = "Maximum selected template pages and invocation samples to return"
    )]
    limit: usize,
    #[arg(
        long,
        default_value_t = 2400,
        value_name = "BYTES",
        help = "Maximum source bytes per selected template page preview"
    )]
    content_limit: usize,
    #[arg(
        long,
        default_value_t = 64,
        value_name = "N",
        help = "Maximum TemplateData parameters returned per selected template"
    )]
    parameter_limit: usize,
    #[arg(
        long = "template",
        value_name = "TITLE",
        action = ArgAction::Append,
        help = "Fetch an exact template page from the source wiki; may be repeated"
    )]
    template: Vec<String>,
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
struct ResearchSearchOutput {
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
struct ResearchFetchOutput {
    schema_version: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_status: Option<ResearchCacheStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<ResearchFetchContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<ExternalFetchResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ResearchFetchErrorOutput>,
}

#[derive(Debug, Serialize)]
struct ResearchFetchErrorOutput {
    source_url: String,
    kind: String,
    message: String,
    attempts: Vec<ExternalFetchAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery: Option<ExternalMachineSurfaceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ResearchFetchContent {
    full_length: usize,
    returned_length: usize,
    truncated: bool,
    omitted: bool,
    limit: Option<usize>,
}

pub(crate) fn run_research(runtime: &RuntimeOptions, args: ResearchArgs) -> Result<()> {
    match args.command {
        ResearchSubcommand::Search(args) => run_research_search(runtime, args),
        ResearchSubcommand::Fetch(args) => run_research_fetch(runtime, args),
        ResearchSubcommand::Discover(args) => run_research_discover(runtime, args),
        ResearchSubcommand::MediawikiTemplates(args) => {
            run_research_mediawiki_templates(runtime, args)
        }
    }
}

fn run_research_search(runtime: &RuntimeOptions, args: ResearchSearchArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let report = remote_wiki_search_report(
        &config,
        RemoteWikiSearchRequest {
            command_name: "research search",
            query: &args.query,
            limit: args.limit,
            what: args.what,
        },
    )?;

    if args.format.is_json() {
        let output = ResearchSearchOutput::from(report);
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("research search");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("source_scope: configured_wiki_api");
    println!("query: {}", report.query);
    print_external_search_report("research_search", &report);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_research_discover(runtime: &RuntimeOptions, args: ResearchDiscoverArgs) -> Result<()> {
    if args.limit == 0 {
        bail!("research discover requires --limit >= 1");
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

    println!("research discover");
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

fn run_research_mediawiki_templates(
    runtime: &RuntimeOptions,
    args: ResearchMediaWikiTemplatesArgs,
) -> Result<()> {
    if args.limit == 0 {
        bail!("research mediawiki-templates requires --limit >= 1");
    }
    if args.content_limit == 0 {
        bail!("research mediawiki-templates requires --content-limit >= 1");
    }
    if args.parameter_limit == 0 {
        bail!("research mediawiki-templates requires --parameter-limit >= 1");
    }
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    let report = fetch_mediawiki_template_report(
        &args.url,
        &MediaWikiTemplateQueryOptions {
            limit: args.limit,
            content_limit: args.content_limit,
            parameter_limit: args.parameter_limit,
            template_titles: args.template,
        },
    )?;

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_mediawiki_template_report_text(&paths, &report);
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn print_mediawiki_template_report_text(
    paths: &wikitool_core::runtime::ResolvedPaths,
    report: &MediaWikiTemplateReport,
) {
    println!("research mediawiki-templates");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("contract_scope: {}", report.contract_scope);
    println!("target_compatibility: {}", report.target_compatibility);
    println!(
        "target_compatibility_note: {}",
        report.target_compatibility_note
    );
    println!("source_url: {}", report.source_url);
    println!("source_domain: {}", report.source_domain);
    println!("api_endpoint: {}", report.api_endpoint);
    println!("page_title: {}", report.page_title);
    println!("canonical_url: {}", report.canonical_url);
    println!("fetched_at: {}", report.fetched_at);
    if let Some(value) = report.page_revision_id {
        println!("page_revision_id: {value}");
    }
    if let Some(value) = report.page_revision_timestamp.as_deref() {
        println!("page_revision_timestamp: {value}");
    }
    println!("api_template_count: {}", report.api_template_count);
    println!(
        "page_template_count_returned: {}",
        report.page_template_count_returned
    );
    println!("invocation_count: {}", report.invocation_count);
    println!(
        "selected_template_count: {}",
        report.selected_template_count
    );
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
    for invocation in &report.template_invocations {
        println!(
            "template_invocation: title={} keys={} tokens={} text={}",
            invocation.template_title,
            if invocation.parameter_keys.is_empty() {
                "<none>".to_string()
            } else {
                invocation.parameter_keys.join(", ")
            },
            invocation.token_estimate,
            single_line(&invocation.raw_wikitext)
        );
    }
    for page in &report.template_pages {
        print_mediawiki_template_page(page);
    }
}

fn print_mediawiki_template_page(page: &MediaWikiTemplatePage) {
    println!(
        "template_page: title={} exists={} revision_id={} revision_timestamp={} hash={} truncated={}",
        page.title,
        if page.exists { "yes" } else { "no" },
        page.revision_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        page.revision_timestamp.as_deref().unwrap_or("<none>"),
        page.content_hash.as_deref().unwrap_or("<none>"),
        if page.content_truncated { "yes" } else { "no" }
    );
    if let Some(templatedata) = page.templatedata.as_ref() {
        println!(
            "template_page.templatedata: title={} params={} description={}",
            page.title,
            templatedata.parameter_count,
            templatedata.description.as_deref().unwrap_or("<none>")
        );
        for parameter in &templatedata.parameters {
            println!(
                "template_page.templatedata.param: template={} name={} type={} required={} suggested={} deprecated={} aliases={} label={} description={}",
                page.title,
                parameter.name,
                parameter.param_type.as_deref().unwrap_or("<none>"),
                if parameter.required { "yes" } else { "no" },
                if parameter.suggested { "yes" } else { "no" },
                if parameter.deprecated { "yes" } else { "no" },
                if parameter.aliases.is_empty() {
                    "<none>".to_string()
                } else {
                    parameter.aliases.join(", ")
                },
                parameter.label.as_deref().unwrap_or("<none>"),
                parameter.description.as_deref().unwrap_or("<none>")
            );
        }
    }
    if let Some(preview) = page.content_preview.as_deref() {
        println!("template_page.preview: {}", single_line(preview));
    }
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn run_research_fetch(runtime: &RuntimeOptions, args: ResearchFetchArgs) -> Result<()> {
    if args.refresh && args.no_cache {
        bail!("research fetch does not allow --refresh together with --no-cache");
    }
    if args.no_content && args.content_limit.is_some() {
        bail!("research fetch does not allow --no-content together with --content-limit");
    }
    if matches!(args.content_limit, Some(0)) {
        bail!("research fetch requires --content-limit >= 1");
    }
    if args.discover_limit == 0 {
        bail!("research fetch requires --discover-limit >= 1");
    }
    let fetch_format = ExternalFetchFormat::from(args.format);
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    let cached = match fetch_page_by_url_cached(
        &paths,
        &args.url,
        &ExternalFetchOptions {
            format: fetch_format,
            max_bytes: 1_000_000,
            profile: ExternalFetchProfile::Research,
        },
        &ResearchCacheOptions {
            use_cache: !args.no_cache,
            refresh: args.refresh,
        },
    ) {
        Ok(Some(cached)) => cached,
        Ok(None) => {
            return handle_research_fetch_error(
                runtime,
                &paths,
                &args,
                ResearchFetchErrorOutput {
                    source_url: args.url.clone(),
                    kind: "not_found".to_string(),
                    message: format!("page not found: {}", args.url),
                    attempts: Vec::new(),
                    discovery: None,
                    discovery_error: None,
                },
            );
        }
        Err(error) => {
            return handle_research_fetch_error(
                runtime,
                &paths,
                &args,
                research_fetch_error_output(&args.url, &error),
            );
        }
    };
    let cache_status = cached.status;
    let cache_path = cached.cache_path.as_deref().map(normalize_path);
    let (result, content) =
        prepare_fetch_result(cached.result, args.content_limit, args.no_content);

    if args.output.is_json() {
        let output = ResearchFetchOutput {
            schema_version: "research_document_v2".to_string(),
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

    println!("research fetch");
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

fn handle_research_fetch_error(
    runtime: &RuntimeOptions,
    paths: &wikitool_core::runtime::ResolvedPaths,
    args: &ResearchFetchArgs,
    mut error: ResearchFetchErrorOutput,
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
        let output = ResearchFetchOutput {
            schema_version: "research_document_v2".to_string(),
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

fn research_fetch_error_output(
    source_url: &str,
    error: &anyhow::Error,
) -> ResearchFetchErrorOutput {
    if let Some(failure) = error.downcast_ref::<ExternalFetchFailureError>() {
        return ResearchFetchErrorOutput {
            source_url: failure.failure.source_url.clone(),
            kind: failure.failure.kind.clone(),
            message: failure.failure.message.clone(),
            attempts: failure.failure.attempts.clone(),
            discovery: None,
            discovery_error: None,
        };
    }
    ResearchFetchErrorOutput {
        source_url: source_url.to_string(),
        kind: "fetch_failed".to_string(),
        message: error.to_string(),
        attempts: Vec::new(),
        discovery: None,
        discovery_error: None,
    }
}

impl From<ExternalSearchReport> for ResearchSearchOutput {
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
            schema_version: "research_search_v1".to_string(),
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
) -> (ExternalFetchResult, ResearchFetchContent) {
    let full_length = result.content.chars().count();
    if no_content {
        result.content.clear();
        return (
            result,
            ResearchFetchContent {
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
        ResearchFetchContent {
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

fn format_rendered_fetch_mode(mode: wikitool_core::research::RenderedFetchMode) -> &'static str {
    match mode {
        wikitool_core::research::RenderedFetchMode::ParseApi => "parse_api",
    }
}

fn format_fetch_mode(mode: wikitool_core::research::FetchMode) -> &'static str {
    match mode {
        wikitool_core::research::FetchMode::Static => "static",
    }
}

fn format_extraction_quality(quality: wikitool_core::research::ExtractionQuality) -> &'static str {
    match quality {
        wikitool_core::research::ExtractionQuality::Low => "low",
        wikitool_core::research::ExtractionQuality::Medium => "medium",
        wikitool_core::research::ExtractionQuality::High => "high",
    }
}

fn format_cache_status(status: ResearchCacheStatus) -> &'static str {
    match status {
        ResearchCacheStatus::Hit => "hit",
        ResearchCacheStatus::Miss => "miss",
        ResearchCacheStatus::Refresh => "refresh",
        ResearchCacheStatus::Bypass => "bypass",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wikitool_core::research::ExternalFetchFailure;

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
    fn research_fetch_error_output_preserves_structured_failure() {
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
            },
        });

        let output = research_fetch_error_output("https://example.com/protected", &error);

        assert_eq!(output.source_url, "https://example.com/protected");
        assert_eq!(output.kind, "access_challenge");
        assert_eq!(output.attempts.len(), 1);
        assert_eq!(output.attempts[0].outcome, "access_challenge");
        assert_eq!(output.attempts[0].http_status, Some(403));
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
