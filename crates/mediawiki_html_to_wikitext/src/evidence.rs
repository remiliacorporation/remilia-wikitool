use anyhow::{Context, Result, ensure};
use chrono::DateTime;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

pub const HTML_CAPTURE_RECEIPT_SCHEMA: &str = "mediawiki.html-capture-receipt.v1";

const MAX_CAPTURE_HTML_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CAPTURE_TIMEOUT_MS: u64 = 10 * 60 * 1_000;
const MAX_CAPTURE_URL_BYTES: usize = 8 * 1024;
const MAX_RESOURCE_OBSERVATIONS: usize = 1_024;
const MAX_INLINE_RESOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RESOURCE_ATTRIBUTE_BYTES: usize = 4 * 1024;
const MAX_PRODUCER_FIELD_BYTES: usize = 128;

/// Identifies whether the supplied bytes came from a direct response or a browser DOM snapshot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HtmlRepresentation {
    StaticHtml,
    RenderedDom,
}

/// Names the acquisition implementation without making its self-report an authenticity claim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HtmlCaptureProducer {
    pub name: String,
    pub version: String,
}

/// Producer-neutral identity and bounds for one exact captured HTML representation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HtmlCaptureReceipt {
    pub schema: String,
    pub source_key: String,
    pub canonical_url: String,
    pub final_url: String,
    pub captured_at: String,
    pub representation: HtmlRepresentation,
    pub producer: HtmlCaptureProducer,
    pub javascript_executed: bool,
    pub capture_timeout_ms: u64,
    pub html_sha256: String,
    pub html_bytes: u64,
    pub max_resource_observations: usize,
    pub max_inline_resource_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceDisposition {
    NotApplied,
    NotExecuted,
    DeclarativeEvidenceOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptClassification {
    Executable,
    Declarative,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceLocatorStatus {
    Resolved,
    Missing,
    Invalid,
    UnsupportedScheme,
    CredentialsRejected,
}

/// A source locator is retained even when it cannot or must not be dereferenced by the compiler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceLocator {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_url: Option<String>,
    pub status: ResourceLocatorStatus,
}

/// CSS and JavaScript observations are evidence only; their contents are never transplanted or run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceObservation {
    InlineStyle {
        ordinal: usize,
        content_sha256: String,
        content_bytes: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        media: Option<String>,
        disposition: ResourceDisposition,
    },
    ExternalStylesheet {
        ordinal: usize,
        locator: ResourceLocator,
        #[serde(skip_serializing_if = "Option::is_none")]
        media: Option<String>,
        disposition: ResourceDisposition,
    },
    InlineScript {
        ordinal: usize,
        content_sha256: String,
        content_bytes: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        script_type: Option<String>,
        classification: ScriptClassification,
        disposition: ResourceDisposition,
    },
    ExternalScript {
        ordinal: usize,
        locator: ResourceLocator,
        #[serde(skip_serializing_if = "Option::is_none")]
        script_type: Option<String>,
        classification: ScriptClassification,
        disposition: ResourceDisposition,
    },
}

pub fn validate_capture_receipt(
    receipt: &HtmlCaptureReceipt,
    html: &str,
    source_key: &str,
    canonical_url: &str,
) -> Result<()> {
    ensure!(
        receipt.schema == HTML_CAPTURE_RECEIPT_SCHEMA,
        "HTML capture receipt schema must be {HTML_CAPTURE_RECEIPT_SCHEMA}"
    );
    ensure!(
        receipt.source_key == source_key,
        "HTML capture receipt source_key {:?} does not match source evidence key {:?}",
        receipt.source_key,
        source_key
    );
    ensure!(
        receipt.canonical_url == canonical_url,
        "HTML capture receipt canonical_url {:?} does not match canonical source URL {:?}",
        receipt.canonical_url,
        canonical_url
    );
    validate_http_url(&receipt.canonical_url, "canonical_url", true)?;
    // A rendered capture may legitimately bind client-side route state in the final URL fragment.
    validate_http_url(&receipt.final_url, "final_url", false)?;
    DateTime::parse_from_rfc3339(&receipt.captured_at)
        .context("HTML capture receipt captured_at must be RFC 3339")?;
    validate_bounded_text(
        &receipt.producer.name,
        "producer name",
        MAX_PRODUCER_FIELD_BYTES,
    )?;
    validate_bounded_text(
        &receipt.producer.version,
        "producer version",
        MAX_PRODUCER_FIELD_BYTES,
    )?;
    if receipt.representation == HtmlRepresentation::StaticHtml {
        ensure!(
            !receipt.javascript_executed,
            "static_html capture receipt cannot report JavaScript execution"
        );
    }
    ensure!(
        (1..=MAX_CAPTURE_TIMEOUT_MS).contains(&receipt.capture_timeout_ms),
        "HTML capture timeout must be between 1 and {MAX_CAPTURE_TIMEOUT_MS} milliseconds"
    );
    ensure!(
        receipt.max_resource_observations <= MAX_RESOURCE_OBSERVATIONS,
        "HTML capture max_resource_observations exceeds hard limit {MAX_RESOURCE_OBSERVATIONS}"
    );
    ensure!(
        receipt.max_inline_resource_bytes <= MAX_INLINE_RESOURCE_BYTES,
        "HTML capture max_inline_resource_bytes exceeds hard limit {MAX_INLINE_RESOURCE_BYTES}"
    );
    ensure!(
        (html.len() as u64) <= MAX_CAPTURE_HTML_BYTES,
        "captured HTML exceeds hard limit {MAX_CAPTURE_HTML_BYTES} bytes"
    );
    ensure!(
        receipt.html_bytes == html.len() as u64,
        "HTML capture receipt html_bytes is {}, but supplied HTML has {} bytes",
        receipt.html_bytes,
        html.len()
    );
    ensure!(
        is_lower_hex_digest(&receipt.html_sha256),
        "HTML capture receipt html_sha256 must be a lowercase 64-character hexadecimal digest"
    );
    let actual_sha256 = sha256_hex(html.as_bytes());
    ensure!(
        receipt.html_sha256 == actual_sha256,
        "HTML capture receipt html_sha256 does not match supplied HTML (expected {actual_sha256})"
    );
    Ok(())
}

pub(crate) fn collect_resource_observations(
    html: &str,
    receipt: &HtmlCaptureReceipt,
) -> Result<Vec<ResourceObservation>> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("style, link[rel], script")
        .map_err(|_| anyhow::anyhow!("invalid static resource selector"))?;
    let final_url = Url::parse(&receipt.final_url).context("parse capture final_url")?;
    let base_url = effective_document_base(&document, &final_url)?;
    let mut observations = Vec::new();
    let mut inline_bytes = 0u64;

    for element in document.select(&selector) {
        let name = element.value().name();
        if name == "link"
            && !element
                .value()
                .attr("rel")
                .map(rel_includes_stylesheet)
                .unwrap_or(false)
        {
            continue;
        }
        ensure!(
            observations.len() < receipt.max_resource_observations,
            "captured HTML exceeds receipt max_resource_observations {}",
            receipt.max_resource_observations
        );
        let ordinal = observations.len();
        let observation = match name {
            "style" => {
                let content = element.text().collect::<String>();
                let content_bytes = content.len() as u64;
                inline_bytes = inline_bytes
                    .checked_add(content_bytes)
                    .context("inline resource byte count overflow")?;
                ResourceObservation::InlineStyle {
                    ordinal,
                    content_sha256: sha256_hex(content.as_bytes()),
                    content_bytes,
                    media: bounded_optional_attribute(
                        element.value().attr("media"),
                        "style media",
                    )?,
                    disposition: ResourceDisposition::NotApplied,
                }
            }
            "link" => ResourceObservation::ExternalStylesheet {
                ordinal,
                locator: resource_locator(element.value().attr("href"), &base_url)?,
                media: bounded_optional_attribute(
                    element.value().attr("media"),
                    "stylesheet media",
                )?,
                disposition: ResourceDisposition::NotApplied,
            },
            "script" => {
                let script_type =
                    bounded_optional_attribute(element.value().attr("type"), "script type")?
                        .map(|value| value.to_ascii_lowercase());
                let classification = classify_script(script_type.as_deref());
                let disposition = match classification {
                    ScriptClassification::Declarative => {
                        ResourceDisposition::DeclarativeEvidenceOnly
                    }
                    ScriptClassification::Executable | ScriptClassification::Unknown => {
                        ResourceDisposition::NotExecuted
                    }
                };
                if element.value().attr("src").is_some() {
                    ResourceObservation::ExternalScript {
                        ordinal,
                        locator: resource_locator(element.value().attr("src"), &base_url)?,
                        script_type,
                        classification,
                        disposition,
                    }
                } else {
                    let content = element.text().collect::<String>();
                    let content_bytes = content.len() as u64;
                    inline_bytes = inline_bytes
                        .checked_add(content_bytes)
                        .context("inline resource byte count overflow")?;
                    ResourceObservation::InlineScript {
                        ordinal,
                        content_sha256: sha256_hex(content.as_bytes()),
                        content_bytes,
                        script_type,
                        classification,
                        disposition,
                    }
                }
            }
            _ => continue,
        };
        ensure!(
            inline_bytes <= receipt.max_inline_resource_bytes,
            "captured HTML inline CSS/JavaScript evidence exceeds receipt max_inline_resource_bytes {}",
            receipt.max_inline_resource_bytes
        );
        observations.push(observation);
    }
    Ok(observations)
}

fn effective_document_base(document: &Html, final_url: &Url) -> Result<Url> {
    let selector = Selector::parse("base[href]")
        .map_err(|_| anyhow::anyhow!("invalid static base selector"))?;
    let Some(declared) = document
        .select(&selector)
        .next()
        .and_then(|element| element.value().attr("href"))
    else {
        return Ok(final_url.clone());
    };
    ensure!(
        declared.len() <= MAX_RESOURCE_ATTRIBUTE_BYTES,
        "document base href exceeds {MAX_RESOURCE_ATTRIBUTE_BYTES} bytes"
    );
    let resolved = match final_url.join(declared.trim()) {
        Ok(value) if matches!(value.scheme(), "http" | "https") => value,
        _ => return Ok(final_url.clone()),
    };
    ensure!(
        resolved.as_str().len() <= MAX_CAPTURE_URL_BYTES,
        "resolved document base URL exceeds {MAX_CAPTURE_URL_BYTES} bytes"
    );
    Ok(resolved)
}

fn resource_locator(declared: Option<&str>, base_url: &Url) -> Result<ResourceLocator> {
    let Some(declared) = declared.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(ResourceLocator {
            declared: None,
            declared_sha256: None,
            resolved_url: None,
            status: ResourceLocatorStatus::Missing,
        });
    };
    ensure!(
        declared.len() <= MAX_RESOURCE_ATTRIBUTE_BYTES,
        "resource locator exceeds {MAX_RESOURCE_ATTRIBUTE_BYTES} bytes"
    );
    let declared = declared.to_string();
    let declared_sha256 = sha256_hex(declared.as_bytes());
    let Ok(resolved) = base_url.join(&declared) else {
        return Ok(ResourceLocator {
            declared: None,
            declared_sha256: Some(declared_sha256),
            resolved_url: None,
            status: ResourceLocatorStatus::Invalid,
        });
    };
    if !matches!(resolved.scheme(), "http" | "https") {
        return Ok(ResourceLocator {
            declared: None,
            declared_sha256: Some(declared_sha256),
            resolved_url: None,
            status: ResourceLocatorStatus::UnsupportedScheme,
        });
    }
    if !resolved.username().is_empty() || resolved.password().is_some() {
        return Ok(ResourceLocator {
            declared: None,
            declared_sha256: Some(declared_sha256),
            resolved_url: None,
            status: ResourceLocatorStatus::CredentialsRejected,
        });
    }
    ensure!(
        resolved.as_str().len() <= MAX_CAPTURE_URL_BYTES,
        "resolved resource URL exceeds {MAX_CAPTURE_URL_BYTES} bytes"
    );
    Ok(ResourceLocator {
        declared: Some(declared),
        declared_sha256: Some(declared_sha256),
        resolved_url: Some(resolved.to_string()),
        status: ResourceLocatorStatus::Resolved,
    })
}

fn rel_includes_stylesheet(value: &str) -> bool {
    value
        .split_ascii_whitespace()
        .any(|token| token.eq_ignore_ascii_case("stylesheet"))
}

fn classify_script(script_type: Option<&str>) -> ScriptClassification {
    match script_type {
        Some("application/ld+json" | "application/json") => ScriptClassification::Declarative,
        None
        | Some(
            "module"
            | "text/javascript"
            | "application/javascript"
            | "text/ecmascript"
            | "application/ecmascript",
        ) => ScriptClassification::Executable,
        Some(_) => ScriptClassification::Unknown,
    }
}

fn bounded_optional_attribute(value: Option<&str>, label: &str) -> Result<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    ensure!(
        value.len() <= MAX_RESOURCE_ATTRIBUTE_BYTES,
        "{label} exceeds {MAX_RESOURCE_ATTRIBUTE_BYTES} bytes"
    );
    ensure!(
        !value.chars().any(char::is_control),
        "{label} contains control characters"
    );
    Ok(Some(value.to_string()))
}

fn validate_http_url(value: &str, label: &str, reject_fragment: bool) -> Result<()> {
    ensure!(
        value.len() <= MAX_CAPTURE_URL_BYTES,
        "HTML capture {label} exceeds {MAX_CAPTURE_URL_BYTES} bytes"
    );
    let parsed = Url::parse(value).with_context(|| format!("parse HTML capture {label}"))?;
    ensure!(
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some(),
        "HTML capture {label} must be an absolute HTTP(S) URL"
    );
    ensure!(
        parsed.username().is_empty() && parsed.password().is_none(),
        "HTML capture {label} must not contain credentials"
    );
    ensure!(
        parsed.as_str().len() <= MAX_CAPTURE_URL_BYTES,
        "normalized HTML capture {label} exceeds {MAX_CAPTURE_URL_BYTES} bytes"
    );
    if reject_fragment {
        ensure!(
            parsed.fragment().is_none(),
            "HTML capture {label} must not contain a fragment"
        );
    }
    Ok(())
}

fn validate_bounded_text(value: &str, label: &str, max_bytes: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= max_bytes,
        "HTML capture {label} must contain between 1 and {max_bytes} bytes"
    );
    ensure!(
        !value.chars().any(char::is_control),
        "HTML capture {label} contains control characters"
    );
    Ok(())
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(html: &str) -> HtmlCaptureReceipt {
        HtmlCaptureReceipt {
            schema: HTML_CAPTURE_RECEIPT_SCHEMA.to_string(),
            source_key: "fixture".to_string(),
            canonical_url: "https://source.example/article".to_string(),
            final_url: "https://source.example/article".to_string(),
            captured_at: "2026-08-30T12:34:56Z".to_string(),
            representation: HtmlRepresentation::RenderedDom,
            producer: HtmlCaptureProducer {
                name: "fixture-browser".to_string(),
                version: "1.0.0".to_string(),
            },
            javascript_executed: true,
            capture_timeout_ms: 30_000,
            html_sha256: sha256_hex(html.as_bytes()),
            html_bytes: html.len() as u64,
            max_resource_observations: 16,
            max_inline_resource_bytes: 4_096,
        }
    }

    #[test]
    fn validates_exact_static_and_rendered_capture_receipts() {
        let html = "<p>Captured</p>";
        let mut rendered = receipt(html);
        rendered.final_url = "https://source.example/article#rendered-tab".to_string();
        validate_capture_receipt(&rendered, html, "fixture", "https://source.example/article")
            .expect("validate rendered receipt");

        let mut static_receipt = rendered;
        static_receipt.final_url = "https://source.example/article".to_string();
        static_receipt.representation = HtmlRepresentation::StaticHtml;
        static_receipt.javascript_executed = false;
        validate_capture_receipt(
            &static_receipt,
            html,
            "fixture",
            "https://source.example/article",
        )
        .expect("validate static receipt");
    }

    #[test]
    fn rejects_stale_or_internally_inconsistent_capture_receipts() {
        let html = "<p>Captured</p>";
        let base = receipt(html);

        let mut stale_hash = base.clone();
        stale_hash.html_sha256 = "0".repeat(64);
        assert!(
            validate_capture_receipt(
                &stale_hash,
                html,
                "fixture",
                "https://source.example/article"
            )
            .expect_err("stale hash must fail")
            .to_string()
            .contains("does not match")
        );

        let mut stale_bytes = base.clone();
        stale_bytes.html_bytes += 1;
        assert!(
            validate_capture_receipt(
                &stale_bytes,
                html,
                "fixture",
                "https://source.example/article"
            )
            .expect_err("stale byte count must fail")
            .to_string()
            .contains("html_bytes")
        );

        let mut static_javascript = base;
        static_javascript.representation = HtmlRepresentation::StaticHtml;
        assert!(
            validate_capture_receipt(
                &static_javascript,
                html,
                "fixture",
                "https://source.example/article"
            )
            .expect_err("static JavaScript execution must fail")
            .to_string()
            .contains("cannot report JavaScript execution")
        );
    }

    #[test]
    fn rejects_capture_identity_and_bound_drift() {
        let html = "<p>Captured</p>";
        let base = receipt(html);

        assert!(
            validate_capture_receipt(
                &base,
                html,
                "other-source",
                "https://source.example/article"
            )
            .expect_err("source identity drift must fail")
            .to_string()
            .contains("source_key")
        );
        assert!(
            validate_capture_receipt(&base, html, "fixture", "https://source.example/other")
                .expect_err("canonical URL drift must fail")
                .to_string()
                .contains("canonical_url")
        );

        let mut unbounded_count = base.clone();
        unbounded_count.max_resource_observations = usize::MAX;
        assert!(
            validate_capture_receipt(
                &unbounded_count,
                html,
                "fixture",
                "https://source.example/article"
            )
            .expect_err("unbounded resource count must fail")
            .to_string()
            .contains("hard limit")
        );

        let mut unbounded_bytes = base;
        unbounded_bytes.max_inline_resource_bytes = u64::MAX;
        assert!(
            validate_capture_receipt(
                &unbounded_bytes,
                html,
                "fixture",
                "https://source.example/article"
            )
            .expect_err("unbounded inline bytes must fail")
            .to_string()
            .contains("hard limit")
        );

        let mut unbounded_final_url = receipt(html);
        unbounded_final_url.final_url = format!(
            "https://source.example/{}",
            "a".repeat(MAX_CAPTURE_URL_BYTES)
        );
        assert!(
            validate_capture_receipt(
                &unbounded_final_url,
                html,
                "fixture",
                "https://source.example/article"
            )
            .expect_err("unbounded final URL must fail")
            .to_string()
            .contains("final_url exceeds")
        );
    }

    #[test]
    fn observes_head_level_css_and_javascript_without_applying_or_executing_it() {
        let html = r#"<!doctype html><html><head>
            <base href="https://assets.example/static/">
            <link rel="preload stylesheet" href="site.css" media="screen">
            <style media="print">.chrome { display: none }</style>
            <script src="app.js" type="module"></script>
            <script type="application/ld+json">{"name":"Fixture"}</script>
            <script type="text/x-template">ignored template</script>
            </head><body><p>Durable text.</p></body></html>"#;
        let capture = receipt(html);
        let observations = collect_resource_observations(html, &capture)
            .expect("collect bounded resource observations");

        assert_eq!(observations.len(), 5);
        assert!(matches!(
            &observations[0],
            ResourceObservation::ExternalStylesheet {
                ordinal: 0,
                locator: ResourceLocator {
                    resolved_url: Some(url),
                    status: ResourceLocatorStatus::Resolved,
                    ..
                },
                media: Some(media),
                disposition: ResourceDisposition::NotApplied,
            } if url == "https://assets.example/static/site.css" && media == "screen"
        ));
        assert!(matches!(
            &observations[1],
            ResourceObservation::InlineStyle {
                ordinal: 1,
                media: Some(media),
                disposition: ResourceDisposition::NotApplied,
                ..
            } if media == "print"
        ));
        assert!(matches!(
            &observations[2],
            ResourceObservation::ExternalScript {
                ordinal: 2,
                locator: ResourceLocator {
                    resolved_url: Some(url),
                    status: ResourceLocatorStatus::Resolved,
                    ..
                },
                classification: ScriptClassification::Executable,
                disposition: ResourceDisposition::NotExecuted,
                ..
            } if url == "https://assets.example/static/app.js"
        ));
        assert!(matches!(
            &observations[3],
            ResourceObservation::InlineScript {
                ordinal: 3,
                classification: ScriptClassification::Declarative,
                disposition: ResourceDisposition::DeclarativeEvidenceOnly,
                ..
            }
        ));
        assert!(matches!(
            &observations[4],
            ResourceObservation::InlineScript {
                ordinal: 4,
                classification: ScriptClassification::Unknown,
                disposition: ResourceDisposition::NotExecuted,
                ..
            }
        ));
    }

    #[test]
    fn resource_observation_bounds_fail_closed() {
        let html = "<style>a{}</style><script>run()</script>";
        let mut capture = receipt(html);
        capture.max_resource_observations = 1;
        let error = collect_resource_observations(html, &capture)
            .expect_err("resource count must be bounded");
        assert!(error.to_string().contains("max_resource_observations"));

        capture.max_resource_observations = 2;
        capture.max_inline_resource_bytes = 3;
        let error = collect_resource_observations(html, &capture)
            .expect_err("inline evidence bytes must be bounded");
        assert!(error.to_string().contains("max_inline_resource_bytes"));

        let origin = "https://source.example/";
        let long_final_url = format!(
            "{origin}{}/",
            "a".repeat(MAX_CAPTURE_URL_BYTES - origin.len() - 1)
        );
        assert_eq!(long_final_url.len(), MAX_CAPTURE_URL_BYTES);
        let long_locator = "b".repeat(MAX_RESOURCE_ATTRIBUTE_BYTES);
        let html = format!(r#"<link rel="stylesheet" href="{long_locator}">"#);
        let mut capture = receipt(&html);
        capture.final_url = long_final_url;
        validate_capture_receipt(&capture, &html, "fixture", "https://source.example/article")
            .expect("maximum bounded final URL remains valid");
        let error = collect_resource_observations(&html, &capture)
            .expect_err("resolved resource URL amplification must fail");
        assert!(error.to_string().contains("resolved resource URL exceeds"));
    }

    #[test]
    fn unresolved_external_locators_remain_typed_evidence() {
        let html = r#"<link rel="stylesheet"><script src="data:text/javascript,run()"></script><script src="https://user:secret@source.example/app.js"></script>"#;
        let capture = receipt(html);
        let observations = collect_resource_observations(html, &capture)
            .expect("collect unresolved resource observations");

        assert!(matches!(
            &observations[0],
            ResourceObservation::ExternalStylesheet {
                locator: ResourceLocator {
                    status: ResourceLocatorStatus::Missing,
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            &observations[1],
            ResourceObservation::ExternalScript {
                locator: ResourceLocator {
                    status: ResourceLocatorStatus::UnsupportedScheme,
                    declared: None,
                    declared_sha256: Some(_),
                    ..
                },
                classification: ScriptClassification::Executable,
                disposition: ResourceDisposition::NotExecuted,
                ..
            }
        ));
        assert!(matches!(
            &observations[2],
            ResourceObservation::ExternalScript {
                locator: ResourceLocator {
                    status: ResourceLocatorStatus::CredentialsRejected,
                    declared: None,
                    declared_sha256: Some(_),
                    resolved_url: None,
                },
                ..
            }
        ));
    }
}
