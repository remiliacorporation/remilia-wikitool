use std::collections::BTreeSet;

use anyhow::{Context, Result};
use reqwest::Url;

use super::html::{decode_html, extract_head, index_of_ignore_case, scan_tags};
use super::{
    ExternalClient, TextHttpResponse, access_challenge_message, detect_access_challenge_response,
    external_client, is_supported_text_content_type, payment_required_message,
    request_text_candidate,
};
use crate::research::model::{
    ExternalAccessRoute, ExternalContentSignal, ExternalFetchAttempt, ExternalMachineSurface,
    ExternalMachineSurfaceReport,
};
#[derive(Debug, Clone, Copy)]
pub struct MachineSurfaceDiscoveryOptions {
    pub max_bytes: usize,
    pub surface_limit: usize,
    pub probe_source_page: bool,
    /// Hint that the caller already knows the source page is blocked (for example,
    /// discovery is being run from a fetch-error handler that saw the 403 upstream).
    /// When true, access-route synthesis emits the blocked-source fallbacks even if
    /// the discovery itself did not probe the source page.
    pub source_known_blocked: bool,
}

impl Default for MachineSurfaceDiscoveryOptions {
    fn default() -> Self {
        Self {
            max_bytes: 1_000_000,
            surface_limit: 20,
            probe_source_page: true,
            source_known_blocked: false,
        }
    }
}

pub(crate) fn discover_machine_surfaces(
    source_url: &str,
    options: MachineSurfaceDiscoveryOptions,
) -> Result<ExternalMachineSurfaceReport> {
    let parsed = Url::parse(source_url)
        .with_context(|| format!("failed to parse source URL for discovery: {source_url}"))?;
    let origin_url = origin_url(&parsed)?;
    let client = external_client()?;
    let mut report = ExternalMachineSurfaceReport {
        source_url: source_url.to_string(),
        origin_url: origin_url.clone(),
        content_signals: Vec::new(),
        surfaces: Vec::new(),
        access_routes: Vec::new(),
        attempts: Vec::new(),
    };
    let mut sitemap_candidates = BTreeSet::new();

    let robots_url = join_origin_path(&origin_url, "/robots.txt")?;
    if let Some(response) = fetch_discovery_candidate(
        &client,
        "robots_txt",
        &robots_url,
        options.max_bytes,
        &mut report.attempts,
    )? && response_is_readable(&response)
    {
        push_machine_surface(
            &mut report.surfaces,
            options.surface_limit,
            ExternalMachineSurface {
                kind: "robots_txt".to_string(),
                url: response.final_url.clone(),
                source: "well_known".to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                description: Some("Robots directives and optional content signals.".to_string()),
                notes: Vec::new(),
            },
        );
        let (signals, sitemaps) = parse_robots_machine_hints(&response.body, &response.final_url);
        report.content_signals.extend(signals);
        sitemap_candidates.extend(sitemaps);
    }

    sitemap_candidates.insert(join_origin_path(&origin_url, "/sitemap.xml")?);
    discover_sitemap_surfaces(
        &client,
        source_url,
        options,
        &mut report,
        &mut sitemap_candidates,
    )?;

    if options.probe_source_page
        && let Some(response) = fetch_discovery_candidate(
            &client,
            "source_page_static",
            source_url,
            options.max_bytes,
            &mut report.attempts,
        )?
        && response_is_readable(&response)
        && response.content_type.contains("text/html")
    {
        push_machine_surface(
            &mut report.surfaces,
            options.surface_limit,
            ExternalMachineSurface {
                kind: "source_html".to_string(),
                url: response.final_url.clone(),
                source: "source_page".to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                description: Some(
                    "Source page static HTML was readable for metadata discovery.".to_string(),
                ),
                notes: Vec::new(),
            },
        );
        for surface in extract_machine_surfaces_from_html(&response.body, &response.final_url) {
            push_machine_surface(&mut report.surfaces, options.surface_limit, surface);
        }
    }

    report.access_routes = build_access_routes(&report, options.source_known_blocked);
    Ok(report)
}

fn discover_sitemap_surfaces(
    client: &ExternalClient,
    source_url: &str,
    options: MachineSurfaceDiscoveryOptions,
    report: &mut ExternalMachineSurfaceReport,
    sitemap_candidates: &mut BTreeSet<String>,
) -> Result<()> {
    let mut attempted = BTreeSet::new();
    let mut sitemap_fetches = 0usize;
    while let Some(sitemap_url) = sitemap_candidates.pop_first() {
        if sitemap_fetches >= 8 || !attempted.insert(sitemap_url.clone()) {
            continue;
        }
        sitemap_fetches += 1;
        let Some(response) = fetch_discovery_candidate(
            client,
            "sitemap",
            &sitemap_url,
            options.max_bytes,
            &mut report.attempts,
        )?
        else {
            continue;
        };
        if !response_is_readable(&response) {
            continue;
        }
        let locs = extract_xml_loc_values(&response.body);
        let is_index = xml_contains_tag(&response.body, "sitemapindex");
        push_machine_surface(
            &mut report.surfaces,
            options.surface_limit,
            ExternalMachineSurface {
                kind: if is_index {
                    "sitemap_index".to_string()
                } else {
                    "sitemap".to_string()
                },
                url: response.final_url.clone(),
                source: "robots_or_well_known".to_string(),
                http_status: Some(response.http_status),
                content_type: Some(response.content_type.clone()),
                description: Some(format!("Sitemap document with {} URL entries.", locs.len())),
                notes: Vec::new(),
            },
        );
        if is_index {
            for loc in locs {
                if Url::parse(&loc).is_ok() {
                    sitemap_candidates.insert(loc);
                }
            }
            continue;
        }
        let mut relevant = Vec::new();
        let mut samples = Vec::new();
        for loc in locs {
            if sitemap_url_matches_source(source_url, &loc) {
                relevant.push(loc);
            } else if samples.len() < 3 {
                samples.push(loc);
            }
        }
        for loc in relevant.into_iter().chain(samples.into_iter()) {
            push_machine_surface(
                &mut report.surfaces,
                options.surface_limit,
                ExternalMachineSurface {
                    kind: "sitemap_url".to_string(),
                    url: loc,
                    source: response.final_url.clone(),
                    http_status: None,
                    content_type: None,
                    description: Some("URL declared by a readable sitemap.".to_string()),
                    notes: Vec::new(),
                },
            );
        }
    }
    Ok(())
}

fn fetch_discovery_candidate(
    client: &ExternalClient,
    mode: &str,
    url: &str,
    max_bytes: usize,
    attempts: &mut Vec<ExternalFetchAttempt>,
) -> Result<Option<TextHttpResponse>> {
    let response = match request_text_candidate(client, mode, url, max_bytes) {
        Ok(response) => response,
        Err(error) => {
            attempts.push(ExternalFetchAttempt {
                mode: mode.to_string(),
                url: url.to_string(),
                outcome: "request_error".to_string(),
                http_status: None,
                content_type: None,
                message: Some(error.to_string()),
            });
            return Ok(None);
        }
    };
    let outcome = if response.http_status == 402 || response.crawler_price.is_some() {
        "payment_required"
    } else if detect_access_challenge_response(&response) {
        "access_challenge"
    } else if response.http_status >= 400 {
        "http_error"
    } else if !is_supported_text_content_type(&response.content_type) {
        "unsupported_content_type"
    } else {
        "success"
    };
    attempts.push(ExternalFetchAttempt {
        mode: mode.to_string(),
        url: url.to_string(),
        outcome: outcome.to_string(),
        http_status: Some(response.http_status),
        content_type: Some(response.content_type.clone()),
        message: discovery_attempt_message(outcome, &response),
    });
    Ok(Some(response))
}

fn discovery_attempt_message(outcome: &str, response: &TextHttpResponse) -> Option<String> {
    match outcome {
        "access_challenge" => Some(access_challenge_message(response)),
        "payment_required" => Some(payment_required_message(response)),
        "http_error" => Some(format!("HTTP {}", response.http_status)),
        "unsupported_content_type" => Some(format!(
            "unsupported content-type: {}",
            response.content_type
        )),
        _ => None,
    }
}

fn response_is_readable(response: &TextHttpResponse) -> bool {
    response.http_status < 400
        && !detect_access_challenge_response(response)
        && is_supported_text_content_type(&response.content_type)
}

fn origin_url(parsed: &Url) -> Result<String> {
    let mut origin = parsed.clone();
    origin.set_path("/");
    origin.set_query(None);
    origin.set_fragment(None);
    Ok(origin.to_string())
}

fn join_origin_path(origin_url: &str, path: &str) -> Result<String> {
    Url::parse(origin_url)
        .with_context(|| format!("failed to parse origin URL: {origin_url}"))?
        .join(path)
        .with_context(|| format!("failed to build discovery URL for path: {path}"))
        .map(|value| value.to_string())
}

fn push_machine_surface(
    surfaces: &mut Vec<ExternalMachineSurface>,
    limit: usize,
    surface: ExternalMachineSurface,
) {
    if surfaces.len() >= limit {
        return;
    }
    if surfaces
        .iter()
        .any(|existing| existing.kind == surface.kind && existing.url == surface.url)
    {
        return;
    }
    surfaces.push(surface);
}

pub(super) fn parse_robots_machine_hints(
    robots_text: &str,
    robots_url: &str,
) -> (Vec<ExternalContentSignal>, Vec<String>) {
    let mut signals = Vec::new();
    let mut sitemaps = Vec::new();
    for (index, line) in robots_text.lines().enumerate() {
        if let Some(value) = directive_value(line, "content-signal") {
            for pair in value.split(',') {
                let Some((key, value)) = pair.split_once('=') else {
                    continue;
                };
                let key = key.trim().to_ascii_lowercase();
                let value = value.trim().to_ascii_lowercase();
                if key.is_empty() || value.is_empty() {
                    continue;
                }
                signals.push(ExternalContentSignal {
                    key,
                    value,
                    source_url: robots_url.to_string(),
                    line: index + 1,
                });
            }
        }
        if let Some(value) = directive_value(line, "sitemap")
            && Url::parse(value).is_ok()
        {
            sitemaps.push(value.to_string());
        }
    }
    (signals, sitemaps)
}

fn directive_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return None;
    }
    let (name, value) = trimmed.split_once(':')?;
    if name.trim().eq_ignore_ascii_case(key) {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

pub(super) fn extract_xml_loc_values(xml: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut index = 0usize;
    while let Some(start) = index_of_ignore_case(xml, "<loc", index) {
        let Some(tag_end_offset) = xml[start..].find('>') else {
            break;
        };
        let content_start = start + tag_end_offset + 1;
        let Some(end) = index_of_ignore_case(xml, "</loc>", content_start) else {
            break;
        };
        let value = decode_html(xml[content_start..end].trim());
        if !value.is_empty() {
            values.push(value);
        }
        index = end + "</loc>".len();
    }
    values
}

fn xml_contains_tag(xml: &str, tag: &str) -> bool {
    let open = format!("<{tag}");
    index_of_ignore_case(xml, &open, 0).is_some()
}

pub(super) fn sitemap_url_matches_source(source_url: &str, sitemap_url: &str) -> bool {
    let normalized_source = source_url.trim_end_matches('/');
    let normalized_sitemap = sitemap_url.trim_end_matches('/');
    if normalized_source.eq_ignore_ascii_case(normalized_sitemap) {
        return true;
    }
    let Some(token) = Url::parse(source_url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back().map(str::to_string))
        })
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| value.len() >= 4)
    else {
        return false;
    };
    sitemap_url.to_ascii_lowercase().contains(&token)
}

pub(super) fn extract_machine_surfaces_from_html(
    html: &str,
    final_url: &str,
) -> Vec<ExternalMachineSurface> {
    let mut surfaces = Vec::new();
    for tag in scan_tags(&extract_head(html), "link") {
        let rel = tag
            .attrs
            .get("rel")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let content_type = tag
            .attrs
            .get("type")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let href = tag.attrs.get("href").map(String::as_str).unwrap_or("");
        if href.is_empty() || !rel.contains("alternate") {
            continue;
        }
        let kind = if content_type.contains("rss") {
            Some("rss_feed")
        } else if content_type.contains("atom") {
            Some("atom_feed")
        } else if content_type.contains("json") && content_type.contains("feed") {
            Some("json_feed")
        } else {
            None
        };
        let Some(kind) = kind else {
            continue;
        };
        let Ok(url) = Url::parse(final_url).and_then(|base| base.join(href)) else {
            continue;
        };
        surfaces.push(ExternalMachineSurface {
            kind: kind.to_string(),
            url: url.to_string(),
            source: final_url.to_string(),
            http_status: None,
            content_type: Some(content_type),
            description: Some("Feed link declared by source page HTML.".to_string()),
            notes: Vec::new(),
        });
    }
    if index_of_ignore_case(html, "application/ld+json", 0).is_some() {
        surfaces.push(ExternalMachineSurface {
            kind: "structured_data".to_string(),
            url: final_url.to_string(),
            source: "source_page".to_string(),
            http_status: None,
            content_type: Some("application/ld+json".to_string()),
            description: Some("Source page declares JSON-LD structured data.".to_string()),
            notes: Vec::new(),
        });
    }
    surfaces
}

pub(super) fn build_access_routes(
    report: &ExternalMachineSurfaceReport,
    source_known_blocked: bool,
) -> Vec<ExternalAccessRoute> {
    // The source-page attempt is the only signal from the discovery report itself
    // that determines whether the *source* is blocked. Ancillary blocks (an incidental
    // sitemap 404 on a readable site) should not recommend "blocked source" routes.
    // When a caller already knows the source is blocked (fetch-error path), the
    // `source_known_blocked` flag supplies that signal directly.
    let source_page_attempt = report
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.mode == "source_page_static");
    let source_page_blocked = source_known_blocked
        || source_page_attempt.is_some_and(|attempt| {
            matches!(
                attempt.outcome.as_str(),
                "access_challenge" | "payment_required" | "http_error" | "request_error"
            )
        });
    let any_attempt_blocked = source_known_blocked
        || report.attempts.iter().any(|attempt| {
            matches!(
                attempt.outcome.as_str(),
                "access_challenge" | "payment_required"
            )
        });

    let mut routes = Vec::new();
    if !report.surfaces.is_empty() {
        routes.push(ExternalAccessRoute {
            kind: "machine_surfaces".to_string(),
            status: "available".to_string(),
            description: "Inspect declared robots, sitemap, feed, or structured-data surfaces before selecting alternate citable sources.".to_string(),
        });
    }
    if !report.content_signals.is_empty() {
        routes.push(ExternalAccessRoute {
            kind: "robots_content_signals".to_string(),
            status: "detected".to_string(),
            description: "Robots.txt declares content-use signals; agents should preserve these as source-access context rather than treating them as article evidence.".to_string(),
        });
    }
    if any_attempt_blocked {
        // Challenge/payment hints apply whenever the server sends them, even for
        // ancillary surfaces, because they signal the owner's posture on automated
        // access as a whole.
        routes.push(ExternalAccessRoute {
            kind: "site_owner_access".to_string(),
            status: "requires_source_owner".to_string(),
            description: "If the source owner wants agentic access, use an explicit API/feed/export, allowlist, service token, or documented crawler policy. Wikitool does not solve browser challenges.".to_string(),
        });
        routes.push(ExternalAccessRoute {
            kind: "verified_bot_program".to_string(),
            status: "not_applicable_to_local_one_off_fetches".to_string(),
            description: "Cloudflare Verified Bots are for documented crawler operators with owner consent, robots etiquette, public behavior docs, and stable IP validation; this does not make local ad-hoc fetches eligible.".to_string(),
        });
    }
    if source_page_blocked {
        // The "blocked source" fallbacks are only recommended when the source page
        // itself could not be fetched — not when only ancillary surfaces fail.
        routes.push(ExternalAccessRoute {
            kind: "user_supplied_source_artifact".to_string(),
            status: "manual_provenance_required".to_string(),
            description: "If a human has lawful access, they may provide a saved HTML, PDF, text excerpt, or other artifact; wikitool should preserve that provenance explicitly.".to_string(),
        });
        routes.push(ExternalAccessRoute {
            kind: "alternate_accessible_source".to_string(),
            status: "recommended_when_blocked".to_string(),
            description: "When readable source text remains unavailable, use an accessible source with equivalent authority instead of citing fetch diagnostics or challenge pages.".to_string(),
        });
    }
    routes
}
