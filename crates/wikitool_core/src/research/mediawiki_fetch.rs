use anyhow::{Result, bail};
use serde_json::Value;

use super::model::{ExternalFetchFormat, ExternalFetchOptions, ExternalFetchResult, ParsedWikiUrl};
use super::url::encode_title;
use super::web_fetch::{
    ExternalClient, external_client, now_timestamp_string, truncate_to_byte_limit,
};

const DEFAULT_MEDIAWIKI_TITLE_BATCH_SIZE: usize = 50;

#[derive(Clone)]
enum MediaWikiFetchOutcome {
    Found(ExternalFetchResult),
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
        MediaWikiFetchOutcome::Found(result) => Ok(Some(result)),
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
                        MediaWikiFetchOutcome::Found(page) => output.push(page),
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
                return Ok(MediaWikiFetchOutcome::Found(ExternalFetchResult {
                    source_wiki: "mediawiki".to_string(),
                    source_domain: parsed.domain.clone(),
                    url: format!("{}{}", parsed.base_url, encode_title(&result.title)),
                    ..result
                }));
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
                            MediaWikiFetchOutcome::Found(ExternalFetchResult {
                                source_wiki: "mediawiki".to_string(),
                                source_domain: parsed.domain.clone(),
                                url: format!("{}{}", parsed.base_url, encode_title(&result.title)),
                                ..result
                            })
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
            ("rvprop", "content|timestamp".to_string()),
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
            let html = mediawiki_query_rendered_html(client, api_url, title, options.max_bytes)?;
            Ok(MediaWikiFetchOutcome::Found(ExternalFetchResult {
                content: html.unwrap_or(base.content),
                content_format: "html".to_string(),
                ..base
            }))
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
            ("rvprop", "content|timestamp".to_string()),
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
    max_bytes: usize,
) -> Result<Option<String>> {
    let payload = client.request_json(
        api_url,
        &[
            ("action", "parse".to_string()),
            ("page", title.to_string()),
            ("prop", "text".to_string()),
        ],
    )?;
    let html = payload
        .get("parse")
        .and_then(|value| value.get("text"))
        .and_then(|value| value.get("*"))
        .and_then(Value::as_str)
        .map(|value| truncate_to_byte_limit(value, max_bytes))
        .filter(|value| !value.trim().is_empty());
    Ok(html)
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
    let timestamp = revision
        .get("timestamp")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(now_timestamp_string);

    Ok(MediaWikiFetchOutcome::Found(ExternalFetchResult {
        title,
        content,
        timestamp,
        extract,
        url: String::new(),
        source_wiki: String::new(),
        source_domain: String::new(),
        content_format: match options.format {
            ExternalFetchFormat::Wikitext => "wikitext".to_string(),
            ExternalFetchFormat::Html => "html".to_string(),
        },
    }))
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
