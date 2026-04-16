use anyhow::{Result, bail};
use serde_json::Value;

use super::model::{
    ExternalFetchFormat, ExternalFetchOptions, ExternalFetchResult, ParsedWikiUrl,
    RenderedFetchMode,
};
use super::url::encode_title;
use super::web_fetch::{ExternalClient, external_client, truncate_to_byte_limit};
use crate::mw::render::{RenderedPageHtml, decode_rendered_page_payload};
use crate::support::compute_hash;
use crate::support::now_iso8601_utc;

const DEFAULT_MEDIAWIKI_TITLE_BATCH_SIZE: usize = 50;

#[derive(Clone)]
enum MediaWikiFetchOutcome {
    Found(Box<ExternalFetchResult>),
    Missing,
    NotExportable,
}

pub fn fetch_mediawiki_page(
    title: &str,
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<Option<ExternalFetchResult>> {
    let mut client = external_client()?;
    match fetch_mediawiki_page_with_client(&mut client, title, parsed, options)? {
        MediaWikiFetchOutcome::Found(result) => Ok(Some(*result)),
        MediaWikiFetchOutcome::Missing | MediaWikiFetchOutcome::NotExportable => Ok(None),
    }
}

pub fn list_subpages(
    parent_title: &str,
    parsed: &ParsedWikiUrl,
    limit: usize,
) -> Result<Vec<String>> {
    let mut client = external_client()?;
    let prefix = format!("{}/", parent_title.trim_end_matches('/'));
    let mut candidate_errors = Vec::new();
    for api_url in &parsed.api_candidates {
        let response = mediawiki_query_allpages(&mut client, api_url, &prefix, limit.max(1));
        match response {
            Ok(value) => return Ok(value),
            Err(error) => candidate_errors.push(format!("{api_url}: {error:#}")),
        }
    }
    if !candidate_errors.is_empty() {
        bail!(
            "all MediaWiki API candidates failed while listing subpages for `{parent_title}` on {}:\n  - {}",
            parsed.domain,
            candidate_errors.join("\n  - ")
        );
    }
    Ok(Vec::new())
}

pub fn fetch_pages_by_titles(
    titles: &[String],
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<Vec<ExternalFetchResult>> {
    let mut client = external_client()?;
    let mut output = Vec::new();
    let mut failures = Vec::new();
    for batch in titles.chunks(DEFAULT_MEDIAWIKI_TITLE_BATCH_SIZE) {
        match fetch_mediawiki_pages_with_client(&mut client, batch, parsed, options) {
            Ok(batch_results) => {
                for (title, outcome) in batch.iter().zip(batch_results) {
                    match outcome {
                        MediaWikiFetchOutcome::Found(page) => output.push(*page),
                        MediaWikiFetchOutcome::Missing | MediaWikiFetchOutcome::NotExportable => {}
                    }
                    let _ = title;
                }
            }
            Err(error) => failures.push(error.to_string()),
        }
    }
    if output.is_empty() && !failures.is_empty() {
        bail!("{}", failures.join("\n"));
    }
    Ok(output)
}

fn fetch_mediawiki_page_with_client(
    client: &mut ExternalClient,
    title: &str,
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<MediaWikiFetchOutcome> {
    let mut candidate_errors = Vec::new();
    let mut saw_not_exportable = false;
    for api_url in &parsed.api_candidates {
        let response = mediawiki_query_content(client, api_url, title, options);
        match response {
            Ok(MediaWikiFetchOutcome::Found(result)) => {
                let result = *result;
                let url = format!("{}{}", parsed.base_url, encode_title(&result.title));
                return Ok(MediaWikiFetchOutcome::Found(Box::new(
                    ExternalFetchResult {
                        source_wiki: "mediawiki".to_string(),
                        source_domain: parsed.domain.clone(),
                        url: url.clone(),
                        canonical_url: Some(url),
                        ..result
                    },
                )));
            }
            Ok(MediaWikiFetchOutcome::Missing) => return Ok(MediaWikiFetchOutcome::Missing),
            Ok(MediaWikiFetchOutcome::NotExportable) => saw_not_exportable = true,
            Err(error) => candidate_errors.push(format!("{api_url}: {error:#}")),
        }
    }
    if saw_not_exportable {
        return Ok(MediaWikiFetchOutcome::NotExportable);
    }
    if !candidate_errors.is_empty() {
        bail!(
            "all MediaWiki API candidates failed for `{title}` on {}:\n  - {}",
            parsed.domain,
            candidate_errors.join("\n  - ")
        );
    }
    Ok(MediaWikiFetchOutcome::NotExportable)
}

fn fetch_mediawiki_pages_with_client(
    client: &mut ExternalClient,
    titles: &[String],
    parsed: &ParsedWikiUrl,
    options: &ExternalFetchOptions,
) -> Result<Vec<MediaWikiFetchOutcome>> {
    if options.format == ExternalFetchFormat::Html {
        let mut results = Vec::with_capacity(titles.len());
        for title in titles {
            results.push(fetch_mediawiki_page_with_client(
                client, title, parsed, options,
            )?);
        }
        return Ok(results);
    }

    let mut candidate_errors = Vec::new();
    for api_url in &parsed.api_candidates {
        let response = mediawiki_query_content_batch(client, api_url, titles, options);
        match response {
            Ok(results) => {
                return Ok(results
                    .into_iter()
                    .map(|outcome| match outcome {
                        MediaWikiFetchOutcome::Found(result) => {
                            let result = *result;
                            let url = format!("{}{}", parsed.base_url, encode_title(&result.title));
                            MediaWikiFetchOutcome::Found(Box::new(ExternalFetchResult {
                                source_wiki: "mediawiki".to_string(),
                                source_domain: parsed.domain.clone(),
                                url: url.clone(),
                                canonical_url: Some(url),
                                ..result
                            }))
                        }
                        other => other,
                    })
                    .collect());
            }
            Err(error) => candidate_errors.push(format!("{api_url}: {error:#}")),
        }
    }

    bail!(
        "all MediaWiki API candidates failed for titles on {}:\n  - {}",
        parsed.domain,
        candidate_errors.join("\n  - ")
    )
}

fn mediawiki_query_content(
    client: &mut ExternalClient,
    api_url: &str,
    title: &str,
    options: &ExternalFetchOptions,
) -> Result<MediaWikiFetchOutcome> {
    let wikitext_payload = client.request_json(
        api_url,
        &[
            ("action", "query".to_string()),
            ("titles", title.to_string()),
            ("prop", "revisions|extracts".to_string()),
            ("rvprop", "ids|content|timestamp".to_string()),
            ("rvslots", "main".to_string()),
            ("exintro", "1".to_string()),
            ("explaintext", "1".to_string()),
        ],
    )?;

    match options.format {
        ExternalFetchFormat::Wikitext => {
            parse_mediawiki_content_payload(&wikitext_payload, title, options)
        }
        ExternalFetchFormat::Html => {
            let base = parse_mediawiki_content_payload(&wikitext_payload, title, options)?;
            let MediaWikiFetchOutcome::Found(base) = base else {
                return Ok(base);
            };
            let rendered = mediawiki_query_rendered_html(client, api_url, title)?;
            Ok(MediaWikiFetchOutcome::Found(Box::new(apply_rendered_page(
                *base,
                rendered,
                options.max_bytes,
            ))))
        }
    }
}

fn mediawiki_query_content_batch(
    client: &mut ExternalClient,
    api_url: &str,
    titles: &[String],
    options: &ExternalFetchOptions,
) -> Result<Vec<MediaWikiFetchOutcome>> {
    let payload = client.request_json(
        api_url,
        &[
            ("action", "query".to_string()),
            ("titles", titles.join("|")),
            ("prop", "revisions|extracts".to_string()),
            ("rvprop", "ids|content|timestamp".to_string()),
            ("rvslots", "main".to_string()),
            ("exintro", "1".to_string()),
            ("explaintext", "1".to_string()),
        ],
    )?;
    parse_mediawiki_batch_content_payload(&payload, titles, options)
}

fn mediawiki_query_rendered_html(
    client: &mut ExternalClient,
    api_url: &str,
    title: &str,
) -> Result<Option<RenderedPageHtml>> {
    let payload = client.request_json(
        api_url,
        &[
            ("action", "parse".to_string()),
            ("page", title.to_string()),
            ("prop", "text|displaytitle|revid".to_string()),
        ],
    )?;
    decode_rendered_page_payload(payload, title)
}

fn apply_rendered_page(
    mut base: ExternalFetchResult,
    rendered: Option<RenderedPageHtml>,
    max_bytes: usize,
) -> ExternalFetchResult {
    let Some(rendered) = rendered else {
        return base;
    };

    base.title = rendered.title;
    base.content = truncate_to_byte_limit(&rendered.html, max_bytes);
    base.content_format = "html".to_string();
    base.content_hash = compute_hash(&base.content);
    base.display_title = rendered.display_title;
    base.revision_id = rendered.revision_id.or(base.revision_id);
    base.rendered_fetch_mode = Some(RenderedFetchMode::ParseApi);
    base
}

fn parse_mediawiki_content_payload(
    payload: &Value,
    requested_title: &str,
    options: &ExternalFetchOptions,
) -> Result<MediaWikiFetchOutcome> {
    let page = payload
        .get("query")
        .and_then(|value| value.get("pages"))
        .and_then(Value::as_array)
        .and_then(|pages| pages.first())
        .ok_or_else(|| anyhow::anyhow!("invalid MediaWiki response shape"))?;

    parse_mediawiki_content_page(page, requested_title, options)
}

fn parse_mediawiki_batch_content_payload(
    payload: &Value,
    requested_titles: &[String],
    options: &ExternalFetchOptions,
) -> Result<Vec<MediaWikiFetchOutcome>> {
    let pages = payload
        .get("query")
        .and_then(|value| value.get("pages"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("invalid MediaWiki response shape"))?;

    let mut page_outcomes = std::collections::HashMap::new();
    for page in pages {
        let title = page
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if title.trim().is_empty() {
            continue;
        }
        let outcome = parse_mediawiki_content_page(page, &title, options)?;
        page_outcomes.insert(title.clone(), outcome.clone());
        page_outcomes.insert(title.replace(' ', "_"), outcome);
    }

    Ok(requested_titles
        .iter()
        .map(|title| {
            page_outcomes
                .get(title)
                .or_else(|| {
                    let normalized = title.replace('_', " ");
                    page_outcomes.get(&normalized)
                })
                .cloned()
                .unwrap_or(MediaWikiFetchOutcome::Missing)
        })
        .collect())
}

fn parse_mediawiki_content_page(
    page: &Value,
    requested_title: &str,
    options: &ExternalFetchOptions,
) -> Result<MediaWikiFetchOutcome> {
    if page.get("missing").is_some() {
        return Ok(MediaWikiFetchOutcome::Missing);
    }

    let title = page
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(requested_title)
        .to_string();
    let extract = page
        .get("extract")
        .and_then(Value::as_str)
        .map(|value| truncate_to_byte_limit(value, options.max_bytes));

    let revision = match page
        .get("revisions")
        .and_then(Value::as_array)
        .and_then(|revisions| revisions.first())
    {
        Some(revision) => revision,
        None => return Ok(MediaWikiFetchOutcome::NotExportable),
    };
    let content = match revision
        .get("slots")
        .and_then(|value| value.get("main"))
        .and_then(|value| value.get("content"))
        .and_then(Value::as_str)
    {
        Some(content) => truncate_to_byte_limit(content, options.max_bytes),
        None => return Ok(MediaWikiFetchOutcome::NotExportable),
    };
    let revision_timestamp = revision
        .get("timestamp")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let revision_id = revision.get("revid").and_then(Value::as_i64);
    let content_hash = compute_hash(&content);

    Ok(MediaWikiFetchOutcome::Found(Box::new(
        ExternalFetchResult {
            title,
            content,
            fetched_at: now_iso8601_utc(),
            revision_timestamp,
            extract,
            url: String::new(),
            source_wiki: String::new(),
            source_domain: String::new(),
            content_format: "wikitext".to_string(),
            content_hash,
            revision_id,
            display_title: None,
            rendered_fetch_mode: None,
            canonical_url: None,
            site_name: None,
            byline: None,
            published_at: None,
            fetch_mode: None,
            extraction_quality: None,
            fetch_attempts: Vec::new(),
        },
    )))
}

fn mediawiki_query_allpages(
    client: &mut ExternalClient,
    api_url: &str,
    prefix: &str,
    limit: usize,
) -> Result<Vec<String>> {
    let target = limit.max(1);
    let mut titles = Vec::new();
    let mut continuation = None::<String>;

    while titles.len() < target {
        let mut params = vec![
            ("action", "query".to_string()),
            ("list", "allpages".to_string()),
            ("apprefix", prefix.to_string()),
            (
                "aplimit",
                target.saturating_sub(titles.len()).min(500).to_string(),
            ),
        ];
        if let Some(token) = &continuation {
            params.push(("apcontinue", token.clone()));
        }

        let payload = client.request_json(api_url, &params)?;
        let (page_titles, next_continue) = parse_allpages_payload(&payload);
        titles.extend(page_titles);
        continuation = next_continue;
        if continuation.is_none() {
            break;
        }
    }

    titles.truncate(target);
    Ok(titles)
}

fn parse_allpages_payload(payload: &Value) -> (Vec<String>, Option<String>) {
    let titles = payload
        .get("query")
        .and_then(|value| value.get("allpages"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("title").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let continuation = payload
        .get("continue")
        .and_then(|value| value.get("apcontinue"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    (titles, continuation)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{apply_rendered_page, parse_mediawiki_content_page};
    use crate::mw::render::RenderedPageHtml;
    use crate::research::model::{
        ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult,
        RenderedFetchMode,
    };

    #[test]
    fn parse_mediawiki_content_page_preserves_revision_metadata() {
        let page = json!({
            "title": "Main Page",
            "extract": "Lead summary",
            "revisions": [
                {
                    "revid": 55,
                    "timestamp": "2026-03-17T10:00:00Z",
                    "slots": {
                        "main": {
                            "content": "== Heading ==\nBody"
                        }
                    }
                }
            ]
        });

        let outcome = parse_mediawiki_content_page(
            &page,
            "Main Page",
            &ExternalFetchOptions {
                format: ExternalFetchFormat::Wikitext,
                max_bytes: 10_000,
                profile: ExternalFetchProfile::Legacy,
            },
        )
        .expect("page should parse");
        let super::MediaWikiFetchOutcome::Found(result) = outcome else {
            panic!("expected found page");
        };
        let result = *result;

        assert_eq!(result.title, "Main Page");
        assert_eq!(result.revision_id, Some(55));
        assert_eq!(
            result.revision_timestamp.as_deref(),
            Some("2026-03-17T10:00:00Z")
        );
        assert!(
            !result.fetched_at.is_empty(),
            "fetched_at should be populated"
        );
        assert_eq!(result.extract.as_deref(), Some("Lead summary"));
        assert_eq!(result.content_format, "wikitext");
        assert!(!result.content_hash.is_empty());
        assert!(result.display_title.is_none());
        assert!(result.rendered_fetch_mode.is_none());
    }

    #[test]
    fn apply_rendered_page_overlays_parse_metadata() {
        let base = ExternalFetchResult {
            title: "Main Page".to_string(),
            content: "wikitext".to_string(),
            fetched_at: "2026-03-17T10:00:00Z".to_string(),
            revision_timestamp: Some("2026-03-17T10:00:00Z".to_string()),
            extract: None,
            url: "https://wiki.example.org/Main_Page".to_string(),
            source_wiki: "mediawiki".to_string(),
            source_domain: "wiki.example.org".to_string(),
            content_format: "wikitext".to_string(),
            content_hash: "old-hash".to_string(),
            revision_id: Some(55),
            display_title: None,
            rendered_fetch_mode: None,
            canonical_url: Some("https://wiki.example.org/Main_Page".to_string()),
            site_name: None,
            byline: None,
            published_at: None,
            fetch_mode: None,
            extraction_quality: None,
            fetch_attempts: Vec::new(),
        };

        let rendered = RenderedPageHtml {
            title: "Main Page".to_string(),
            display_title: Some("<i>Main Page</i>".to_string()),
            revision_id: Some(56),
            html: "<p>Hello</p>".to_string(),
        };

        let merged = apply_rendered_page(base, Some(rendered), 10_000);

        assert_eq!(merged.content, "<p>Hello</p>");
        assert_eq!(merged.content_format, "html");
        assert_ne!(merged.content_hash, "old-hash");
        assert_eq!(merged.revision_id, Some(56));
        assert_eq!(merged.display_title.as_deref(), Some("<i>Main Page</i>"));
        assert_eq!(
            merged.rendered_fetch_mode,
            Some(RenderedFetchMode::ParseApi)
        );
    }
}
