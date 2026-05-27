use super::discovery::{
    build_access_routes, extract_machine_surfaces_from_html, extract_xml_loc_values,
    parse_robots_machine_hints, sitemap_url_matches_source,
};
use super::html::{
    HtmlMetadata, build_html_fetch_result, build_metadata_fallback_content,
    collapse_inline_whitespace, detect_access_challenge, detect_app_shell_html,
    extract_client_redirect_url, extract_html_metadata, extract_readable_text,
    normalize_extracted_text,
};
use super::{TextHttpResponse, access_challenge_message, detect_access_challenge_response};
use super::{challenge_handoff_for_response, challenge_vendor_for_response};
use crate::research::model::{ExternalFetchAttempt, ExternalMachineSurfaceReport};
use crate::research::model::{
    ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile, ExtractionQuality, FetchMode,
};

#[test]
fn extracts_html_metadata() {
    let metadata = extract_html_metadata(
        r#"
            <html>
              <head>
                <title>Fallback Title</title>
                <meta property="og:title" content="OpenGraph Title" />
                <meta property="og:site_name" content="Example Site" />
                <meta name="author" content="Onno" />
                <meta property="article:published_time" content="2026-03-17T12:00:00Z" />
                <meta name="description" content="Readable summary" />
                <link rel="canonical" href="https://example.com/article" />
              </head>
            </html>
            "#,
        "https://example.com/fallback",
    );

    assert_eq!(metadata.title.as_deref(), Some("OpenGraph Title"));
    assert_eq!(
        metadata.canonical_url.as_deref(),
        Some("https://example.com/article")
    );
    assert_eq!(metadata.site_name.as_deref(), Some("Example Site"));
    assert_eq!(metadata.byline.as_deref(), Some("Onno"));
    assert_eq!(
        metadata.published_at.as_deref(),
        Some("2026-03-17T12:00:00Z")
    );
    assert_eq!(metadata.description.as_deref(), Some("Readable summary"));
}

#[test]
fn extracts_readable_text_from_article() {
    let text = extract_readable_text(
        r#"
            <html>
              <body>
                <header>Site navigation</header>
                <article>
                  <h1>Headline</h1>
                  <p>First paragraph.</p>
                  <p>Second <strong>paragraph</strong>.</p>
                  <ul><li>Alpha</li><li>Beta</li></ul>
                </article>
                <footer>Footer links</footer>
              </body>
            </html>
            "#,
        10_000,
    );

    assert!(text.contains("Headline"));
    assert!(text.contains("First paragraph."));
    assert!(text.contains("Second paragraph."));
    assert!(text.contains("- Alpha"));
    assert!(text.contains("- Beta"));
    assert!(text.contains("Headline\n\nFirst paragraph."));
    assert!(text.contains("First paragraph.\n\nSecond paragraph."));
    assert!(text.contains("- Alpha\n- Beta"));
    assert!(!text.contains("Site navigation"));
    assert!(!text.contains("Footer links"));
}

#[test]
fn extracts_readable_text_handles_multibyte_text_before_tags() {
    let text = extract_readable_text(
        "<html><body><main><p>Fast cat 🐆.</p><!-- note --><p>Second paragraph.</p></main></body></html>",
        10_000,
    );

    assert!(text.contains("Fast cat 🐆."));
    assert!(text.contains("Fast cat 🐆.\n\nSecond paragraph."));
    assert!(!text.contains("note"));
}

#[test]
fn access_routes_suppress_blocked_fallbacks_when_only_ancillary_attempts_failed() {
    let mut report = ExternalMachineSurfaceReport {
        source_url: "https://example.com/article".to_string(),
        origin_url: "https://example.com/".to_string(),
        content_signals: Vec::new(),
        surfaces: Vec::new(),
        access_routes: Vec::new(),
        attempts: vec![
            ExternalFetchAttempt {
                mode: "robots_txt".to_string(),
                url: "https://example.com/robots.txt".to_string(),
                outcome: "success".to_string(),
                http_status: Some(200),
                content_type: Some("text/plain".to_string()),
                message: None,
            },
            ExternalFetchAttempt {
                mode: "sitemap".to_string(),
                url: "https://example.com/sitemap.xml".to_string(),
                outcome: "http_error".to_string(),
                http_status: Some(404),
                content_type: Some("text/html".to_string()),
                message: Some("HTTP 404".to_string()),
            },
            ExternalFetchAttempt {
                mode: "source_page_static".to_string(),
                url: "https://example.com/article".to_string(),
                outcome: "success".to_string(),
                http_status: Some(200),
                content_type: Some("text/html".to_string()),
                message: None,
            },
        ],
    };
    report.access_routes = build_access_routes(&report, false);
    let kinds: Vec<&str> = report
        .access_routes
        .iter()
        .map(|route| route.kind.as_str())
        .collect();
    assert!(
        !kinds.contains(&"user_supplied_source_artifact"),
        "readable source should not recommend manual provenance"
    );
    assert!(
        !kinds.contains(&"alternate_accessible_source"),
        "readable source should not recommend alternate accessible source"
    );
    assert!(
        !kinds.contains(&"site_owner_access"),
        "ancillary HTTP error should not trigger owner-access route"
    );
}

#[test]
fn access_routes_include_blocked_fallbacks_when_source_known_blocked() {
    let mut report = ExternalMachineSurfaceReport {
        source_url: "https://example.com/article".to_string(),
        origin_url: "https://example.com/".to_string(),
        content_signals: Vec::new(),
        surfaces: Vec::new(),
        access_routes: Vec::new(),
        attempts: vec![ExternalFetchAttempt {
            mode: "robots_txt".to_string(),
            url: "https://example.com/robots.txt".to_string(),
            outcome: "success".to_string(),
            http_status: Some(200),
            content_type: Some("text/plain".to_string()),
            message: None,
        }],
    };
    report.access_routes = build_access_routes(&report, true);
    let kinds: Vec<&str> = report
        .access_routes
        .iter()
        .map(|route| route.kind.as_str())
        .collect();
    assert!(kinds.contains(&"user_supplied_source_artifact"));
    assert!(kinds.contains(&"alternate_accessible_source"));
    assert!(kinds.contains(&"site_owner_access"));
}

#[test]
fn normalize_extracted_text_merges_isolated_bullet_markers() {
    let input = "Status\n\n-\n\nNear threatened\n-\nVulnerable\n\u{2022}\nEndangered";
    let cleaned = normalize_extracted_text(input, 1_000);
    assert!(cleaned.contains("- Near threatened"));
    assert!(cleaned.contains("- Vulnerable"));
    assert!(cleaned.contains("- Endangered"));
    assert!(!cleaned.contains("\n-\n"));
    assert!(!cleaned.contains("\n\u{2022}\n"));
}

#[test]
fn normalize_extracted_text_preserves_paragraphs_and_compact_lists() {
    let cleaned = normalize_extracted_text(
        "Heading\n\nFirst paragraph.\n\nSecond paragraph.\n\n- Alpha\n\n- Beta",
        1_000,
    );

    assert!(cleaned.contains("Heading\n\nFirst paragraph."));
    assert!(cleaned.contains("First paragraph.\n\nSecond paragraph."));
    assert!(cleaned.contains("- Alpha\n- Beta"));
    assert!(!cleaned.contains("- Alpha\n\n- Beta"));
}

#[test]
fn research_profile_returns_clean_text_and_metadata() {
    let result = build_html_fetch_result(
        r#"
            <html>
              <head>
                <title>Example Article</title>
                <meta name="description" content="Summary text" />
                <meta property="og:site_name" content="Example" />
              </head>
              <body>
                <main>
                  <p>This is one long paragraph with enough words to qualify as readable content.</p>
                  <p>Another paragraph keeps the extraction meaningful and focused.</p>
                </main>
              </body>
            </html>
            "#,
        "https://example.com/article",
        "example.com",
        "article",
        &ExternalFetchOptions {
            format: ExternalFetchFormat::Html,
            max_bytes: 10_000,
            profile: ExternalFetchProfile::Research,
            session: None,
        },
    );

    assert_eq!(result.content_format, "text");
    assert_eq!(result.fetch_mode, Some(FetchMode::Static));
    assert_eq!(result.site_name.as_deref(), Some("Example"));
    assert_eq!(
        result.canonical_url.as_deref(),
        Some("https://example.com/article")
    );
    assert_eq!(result.extract.as_deref(), Some("Summary text"));
    assert_eq!(result.extraction_quality, Some(ExtractionQuality::Low));
    assert!(!result.content_hash.is_empty());
    assert!(!collapse_inline_whitespace(&result.content).contains("<html>"));
}

#[test]
fn research_profile_reports_access_challenge_pages_cleanly() {
    let result = build_html_fetch_result(
        r#"
            <html>
              <head><title></title></head>
              <body>
                <script>window.awsWafCookieDomainList = [];</script>
                <div id="challenge-container"></div>
                <noscript>
                  <h1>JavaScript is disabled</h1>
                  verify that you're not a robot
                </noscript>
              </body>
            </html>
            "#,
        "https://example.com/protected",
        "example.com",
        "protected",
        &ExternalFetchOptions {
            format: ExternalFetchFormat::Html,
            max_bytes: 10_000,
            profile: ExternalFetchProfile::Research,
            session: None,
        },
    );

    assert_eq!(result.content_format, "text");
    assert_eq!(result.fetch_mode, Some(FetchMode::Static));
    assert_eq!(result.extraction_quality, Some(ExtractionQuality::Low));
    assert!(
        result
            .content
            .contains("Access challenge detected while fetching https://example.com/protected.")
    );
    assert!(!result.content.contains("window.awsWafCookieDomainList"));
}

#[test]
fn research_profile_falls_back_to_metadata_when_static_extraction_fails() {
    let result = build_html_fetch_result(
        r#"
            <html>
              <head>
                <title>Notes on Reading Remilia's art</title>
                <meta name="description" content="A reflection on Remilia's aesthetics and reading practices." />
                <meta name="author" content="Charlotte Fang" />
                <meta property="og:site_name" content="Paragraph" />
              </head>
              <body>
                <div id="app"></div>
                <script>self.__next_f.push([1, "payload"]);</script>
                <script src="/_next/static/chunks/main.js"></script>
              </body>
            </html>
            "#,
        "https://paragraph.com/@charlemagnefang/rjACW1CDER8t7UQDDgwd",
        "paragraph.com",
        "rjACW1CDER8t7UQDDgwd",
        &ExternalFetchOptions {
            format: ExternalFetchFormat::Html,
            max_bytes: 10_000,
            profile: ExternalFetchProfile::Research,
            session: None,
        },
    );

    assert_eq!(result.content_format, "text");
    assert_eq!(result.fetch_mode, Some(FetchMode::Static));
    assert_eq!(result.extraction_quality, Some(ExtractionQuality::Low));
    assert!(
        result
            .content
            .contains("Client-rendered or app-shell page detected")
    );
    assert!(
        result
            .content
            .contains("Title: Notes on Reading Remilia's art")
    );
    assert!(
        result
            .content
            .contains("Description: A reflection on Remilia's aesthetics")
    );
    assert!(!result.content.contains("__next_f.push"));
    assert!(!result.content.contains("<html>"));
}

#[test]
fn detects_vendor_and_generic_access_challenge_signals() {
    assert!(detect_access_challenge(
        "<html><body><script>window.awsWafCookieDomainList = [];</script></body></html>"
    ));
    assert!(detect_access_challenge(
        "<html><body>making sure you're not a bot with Anubis proof-of-work challenge</body></html>"
    ));
    assert!(detect_access_challenge(
        "<html><body>JavaScript is disabled. Verify that you're not a robot.</body></html>"
    ));
    assert!(!detect_access_challenge(
        "<html><body><article><p>Readable essay text.</p></article></body></html>"
    ));
}

#[test]
fn lone_generic_signal_is_not_an_access_challenge() {
    // A single generic marker (here the "challenge-container" CSS class) must not
    // classify an otherwise readable page as an anti-bot wall; two are required.
    assert!(!detect_access_challenge(
        "<html><body><div id=\"challenge-container\"></div><p>Real content.</p></body></html>"
    ));
    assert!(detect_access_challenge(
        "<html><body><div id=\"challenge-container\"></div>access denied</body></html>"
    ));
}

#[test]
fn detects_cloudflare_challenge_header() {
    let response = TextHttpResponse {
        final_url: "https://example.com/protected".to_string(),
        http_status: 403,
        content_type: "text/html; charset=UTF-8".to_string(),
        cf_mitigated: Some("challenge".to_string()),
        crawler_price: None,
        body: "<html><body>generic challenge shell</body></html>".to_string(),
    };

    assert!(detect_access_challenge_response(&response));
    assert!(access_challenge_message(&response).contains("cf-mitigated"));
    assert_eq!(challenge_vendor_for_response(&response), "cloudflare");
}

#[test]
fn builds_structured_challenge_handoff_for_cloudflare() {
    let response = TextHttpResponse {
        final_url: "https://example.com/protected".to_string(),
        http_status: 403,
        content_type: "text/html; charset=UTF-8".to_string(),
        cf_mitigated: Some("challenge".to_string()),
        crawler_price: None,
        body: "<html><body>generic challenge shell</body></html>".to_string(),
    };

    let handoff = challenge_handoff_for_response(
        "https://example.com/protected",
        "https://example.com/protected",
        "wikitool-test/1.0",
        &response,
    );

    assert_eq!(handoff.vendor, "cloudflare");
    assert_eq!(handoff.domain, "example.com");
    assert_eq!(handoff.required_cookies, vec!["cf_clearance"]);
    assert_eq!(handoff.ttl_hint_seconds, Some(1_800));
    assert_eq!(
        handoff.suggested_argv,
        vec![
            "wikitool",
            "research",
            "session",
            "import",
            "https://example.com/protected",
            "--cookies",
            "-",
            "--user-agent",
            "wikitool-test/1.0",
            "--ttl-seconds",
            "1800"
        ]
    );
    assert!(handoff.suggested_command.contains("--cookies -"));
    assert!(
        handoff
            .notes
            .iter()
            .any(|note| note.contains("does not solve"))
    );
}

#[test]
fn parses_robots_content_signals_and_sitemaps() {
    let (signals, sitemaps) = parse_robots_machine_hints(
        r#"
            User-agent: *
            Content-Signal: search=yes, ai-train=no
            Allow: /
            Sitemap: https://example.com/sitemap.xml
            "#,
        "https://example.com/robots.txt",
    );

    assert_eq!(signals.len(), 2);
    assert_eq!(signals[0].key, "search");
    assert_eq!(signals[0].value, "yes");
    assert_eq!(signals[1].key, "ai-train");
    assert_eq!(signals[1].value, "no");
    assert_eq!(sitemaps, vec!["https://example.com/sitemap.xml"]);
}

#[test]
fn extracts_sitemap_locs_and_matches_source_token() {
    let locs = extract_xml_loc_values(
        r#"
            <urlset>
              <url><loc>https://example.com/animals/cheetah</loc></url>
              <url><loc>https://example.com/animals/lion</loc></url>
            </urlset>
            "#,
    );

    assert_eq!(locs.len(), 2);
    assert!(sitemap_url_matches_source(
        "https://example.com/animals/cheetah",
        &locs[0]
    ));
    assert!(!sitemap_url_matches_source(
        "https://example.com/animals/cheetah",
        &locs[1]
    ));
}

#[test]
fn extracts_feed_and_structured_data_surfaces_from_html() {
    let surfaces = extract_machine_surfaces_from_html(
        r#"
            <html>
              <head>
                <link rel="alternate" type="application/rss+xml" href="/feed.xml" />
                <script type="application/ld+json">{"@type":"Article"}</script>
              </head>
            </html>
            "#,
        "https://example.com/article",
    );

    assert!(surfaces.iter().any(|surface| surface.kind == "rss_feed"));
    assert!(
        surfaces
            .iter()
            .any(|surface| surface.kind == "structured_data")
    );
}

#[test]
fn detects_framework_app_shell_markup() {
    assert!(detect_app_shell_html(
        "<html><body><div id=\"app\"></div><script>self.__next_f.push([1, \"payload\"])</script><script src=\"/_next/static/chunks/main.js\"></script></body></html>"
    ));
    assert!(!detect_app_shell_html(
        "<html><body><article><p>Readable essay text.</p></article></body></html>"
    ));
}

#[test]
fn extracts_client_redirect_url_from_meta_refresh() {
    let redirect = extract_client_redirect_url(
            r#"
            <html>
              <head>
                <meta id="__next-page-redirect" http-equiv="refresh" content="1;url=../@charlemagnefang/notes-towards-a-study-of-remilia-s-art" />
              </head>
            </html>
            "#,
            "https://paragraph.com/@charlemagnefang/rjACW1CDER8t7UQDDgwd",
        )
        .expect("client redirect");

    assert_eq!(
        redirect,
        "https://paragraph.com/@charlemagnefang/notes-towards-a-study-of-remilia-s-art"
    );
}

#[test]
fn metadata_fallback_content_includes_useful_context() {
    let content = build_metadata_fallback_content(
        &HtmlMetadata {
            title: Some("Example title".to_string()),
            canonical_url: Some("https://example.com/article".to_string()),
            site_name: Some("Example".to_string()),
            byline: Some("Author".to_string()),
            published_at: Some("2026-03-17".to_string()),
            description: Some("Summary text".to_string()),
        },
        "https://example.com/article",
        true,
        10_000,
    );

    assert!(content.contains("Client-rendered or app-shell page detected"));
    assert!(content.contains("Title: Example title"));
    assert!(content.contains("Description: Summary text"));
    assert!(content.contains("Author: Author"));
    assert!(content.contains("Published: 2026-03-17"));
    assert!(content.contains("Site: Example"));
    assert!(content.contains("Canonical URL: https://example.com/article"));
}
