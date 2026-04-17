use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Result, bail};
use serde_json::Value;

use super::{MediaWikiFetchOutcome, content::parse_mediawiki_content_payload};
use crate::content_store::parsing::extract_template_invocations;
use crate::research::model::{
    ExternalFetchFormat, ExternalFetchOptions, ExternalFetchProfile, ExternalFetchResult,
    MediaWikiPageTemplate, MediaWikiTemplateDataParameter, MediaWikiTemplateDataSummary,
    MediaWikiTemplateInvocation, MediaWikiTemplatePage, MediaWikiTemplateQueryOptions,
    MediaWikiTemplateReport,
};
use crate::research::url::{encode_title, parse_wiki_url};
use crate::research::web_fetch::{ExternalClient, external_client, truncate_to_byte_limit};
use crate::support::compute_hash;

const DEFAULT_MEDIAWIKI_TEMPLATE_BATCH_SIZE: usize = 50;
const MEDIAWIKI_TEMPLATE_QUERY_LIMIT: usize = 500;

pub fn fetch_mediawiki_template_report(
    source_url: &str,
    options: &MediaWikiTemplateQueryOptions,
) -> Result<MediaWikiTemplateReport> {
    if options.limit == 0 {
        bail!("mediawiki template report requires limit >= 1");
    }
    if options.content_limit == 0 {
        bail!("mediawiki template report requires content_limit >= 1");
    }
    if options.parameter_limit == 0 {
        bail!("mediawiki template report requires parameter_limit >= 1");
    }
    let parsed = parse_wiki_url(source_url)
        .ok_or_else(|| anyhow::anyhow!("URL is not a recognized MediaWiki page: {source_url}"))?;
    let mut client = external_client()?;
    let mut candidate_errors = Vec::new();
    let mut saw_missing = false;

    for api_url in &parsed.api_candidates {
        match mediawiki_query_source_template_page(&mut client, api_url, &parsed.title) {
            Ok(Some((page, page_templates))) => {
                let mut warnings = Vec::new();
                let all_invocations = collect_template_invocations(&page.content);
                let selected_titles =
                    select_template_titles(&page_templates, &all_invocations, options);
                let selected_key_set = selected_titles
                    .iter()
                    .map(|title| normalize_title_key(title))
                    .collect::<BTreeSet<_>>();
                let template_invocations = select_template_invocation_samples(
                    &all_invocations,
                    &selected_key_set,
                    options.limit,
                );
                let mut template_pages = match mediawiki_query_template_pages(
                    &mut client,
                    api_url,
                    &selected_titles,
                    options.content_limit,
                ) {
                    Ok(pages) => pages,
                    Err(error) => {
                        warnings.push(format!("template page content query failed: {error:#}"));
                        selected_titles
                            .iter()
                            .map(|title| missing_template_page(title))
                            .collect()
                    }
                };
                match mediawiki_query_templatedata(
                    &mut client,
                    api_url,
                    &selected_titles,
                    options.parameter_limit,
                ) {
                    Ok(template_data) => attach_templatedata(&mut template_pages, template_data),
                    Err(error) => warnings.push(format!("templatedata query failed: {error:#}")),
                }

                let canonical_url = format!("{}{}", parsed.base_url, encode_title(&page.title));
                let returned_page_templates =
                    sample_page_templates(&page_templates, &selected_titles, options.limit);
                return Ok(MediaWikiTemplateReport {
                    contract_scope: "source_mediawiki_api".to_string(),
                    target_compatibility: "not_evaluated".to_string(),
                    target_compatibility_note:
                        "Templates and modules in this report are valid only on the source wiki that served the API response; use the target wiki's local contract, template, and lint surfaces before adding them to a draft."
                            .to_string(),
                    source_url: source_url.to_string(),
                    source_domain: parsed.domain.clone(),
                    api_endpoint: api_url.clone(),
                    page_title: page.title.clone(),
                    canonical_url,
                    fetched_at: page.fetched_at.clone(),
                    page_revision_id: page.revision_id,
                    page_revision_timestamp: page.revision_timestamp.clone(),
                    api_template_count: page_templates.len(),
                    page_template_count_returned: returned_page_templates.len(),
                    invocation_count: all_invocations.len(),
                    selected_template_count: template_pages.len(),
                    page_templates: returned_page_templates,
                    template_invocations,
                    template_pages,
                    warnings,
                });
            }
            Ok(None) => saw_missing = true,
            Err(error) => candidate_errors.push(format!("{api_url}: {error:#}")),
        }
    }

    if saw_missing {
        bail!(
            "MediaWiki page not found through API for `{}` on {}",
            parsed.title,
            parsed.domain
        );
    }
    bail!(
        "all MediaWiki API candidates failed while querying template surface for `{}` on {}:\n  - {}",
        parsed.title,
        parsed.domain,
        candidate_errors.join("\n  - ")
    )
}

fn mediawiki_query_source_template_page(
    client: &mut ExternalClient,
    api_url: &str,
    title: &str,
) -> Result<Option<(ExternalFetchResult, Vec<MediaWikiPageTemplate>)>> {
    let options = ExternalFetchOptions {
        format: ExternalFetchFormat::Wikitext,
        max_bytes: 1_000_000,
        profile: ExternalFetchProfile::Research,
    };
    let mut continuation = None::<String>;
    let mut page = None::<ExternalFetchResult>;
    let mut templates = BTreeMap::<String, MediaWikiPageTemplate>::new();

    loop {
        let mut params = vec![
            ("action", "query".to_string()),
            ("titles", title.to_string()),
            ("prop", "revisions|templates".to_string()),
            ("rvprop", "ids|content|timestamp".to_string()),
            ("rvslots", "main".to_string()),
            ("tllimit", MEDIAWIKI_TEMPLATE_QUERY_LIMIT.to_string()),
        ];
        if let Some(token) = continuation.as_ref() {
            params.push(("tlcontinue", token.clone()));
        }

        let payload = client.request_json(api_url, &params)?;
        if page.is_none() {
            match parse_mediawiki_content_payload(&payload, title, &options)? {
                MediaWikiFetchOutcome::Found(result) => page = Some(*result),
                MediaWikiFetchOutcome::Missing => return Ok(None),
                MediaWikiFetchOutcome::NotExportable => {
                    bail!("MediaWiki page `{title}` is not exportable through the revisions API")
                }
            }
        }
        for template in parse_page_templates_from_payload(&payload) {
            templates
                .entry(normalize_title_key(&template.title))
                .or_insert(template);
        }
        continuation = payload
            .get("continue")
            .and_then(|value| value.get("tlcontinue"))
            .and_then(Value::as_str)
            .map(ToString::to_string);
        if continuation.is_none() {
            break;
        }
    }

    let Some(page) = page else {
        bail!("invalid MediaWiki response shape: page content was absent");
    };
    Ok(Some((page, templates.into_values().collect())))
}

fn parse_page_templates_from_payload(payload: &Value) -> Vec<MediaWikiPageTemplate> {
    let Some(pages) = payload
        .get("query")
        .and_then(|value| value.get("pages"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for page in pages {
        let Some(templates) = page.get("templates").and_then(Value::as_array) else {
            continue;
        };
        for template in templates {
            let Some(title) = template.get("title").and_then(Value::as_str) else {
                continue;
            };
            let namespace = template
                .get("ns")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok());
            out.push(MediaWikiPageTemplate {
                title: title.to_string(),
                namespace,
            });
        }
    }
    out
}

fn collect_template_invocations(content: &str) -> Vec<MediaWikiTemplateInvocation> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for invocation in extract_template_invocations(content) {
        let signature = format!(
            "{}|{}",
            normalize_title_key(&invocation.template_title),
            invocation.parameter_keys.join(",")
        );
        if !seen.insert(signature) {
            continue;
        }
        out.push(MediaWikiTemplateInvocation {
            template_title: invocation.template_title,
            parameter_keys: invocation.parameter_keys,
            raw_wikitext: invocation.raw_wikitext,
            token_estimate: invocation.token_estimate,
        });
    }
    out
}

fn select_template_titles(
    page_templates: &[MediaWikiPageTemplate],
    invocations: &[MediaWikiTemplateInvocation],
    options: &MediaWikiTemplateQueryOptions,
) -> Vec<String> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    if !options.template_titles.is_empty() {
        for title in &options.template_titles {
            let normalized = normalize_requested_template_title(title);
            if !normalized.is_empty() && seen.insert(normalize_title_key(&normalized)) {
                selected.push(normalized);
            }
            if selected.len() >= options.limit {
                break;
            }
        }
        return selected;
    }

    for invocation in invocations {
        if seen.insert(normalize_title_key(&invocation.template_title)) {
            selected.push(invocation.template_title.clone());
        }
        if selected.len() >= options.limit {
            return selected;
        }
    }

    for template in page_templates {
        if seen.insert(normalize_title_key(&template.title)) {
            selected.push(template.title.clone());
        }
        if selected.len() >= options.limit {
            break;
        }
    }
    selected
}

fn select_template_invocation_samples(
    invocations: &[MediaWikiTemplateInvocation],
    selected_key_set: &BTreeSet<String>,
    limit: usize,
) -> Vec<MediaWikiTemplateInvocation> {
    let mut out = Vec::new();
    for invocation in invocations {
        if selected_key_set.is_empty()
            || selected_key_set.contains(&normalize_title_key(&invocation.template_title))
        {
            out.push(invocation.clone());
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn sample_page_templates(
    page_templates: &[MediaWikiPageTemplate],
    selected_titles: &[String],
    limit: usize,
) -> Vec<MediaWikiPageTemplate> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for selected in selected_titles {
        let selected_key = normalize_title_key(selected);
        if let Some(template) = page_templates
            .iter()
            .find(|template| normalize_title_key(&template.title) == selected_key)
            && seen.insert(normalize_title_key(&template.title))
        {
            out.push(template.clone());
        }
        if out.len() >= limit {
            return out;
        }
    }
    for template in page_templates {
        if seen.insert(normalize_title_key(&template.title)) {
            out.push(template.clone());
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn normalize_requested_template_title(value: &str) -> String {
    let trimmed = value.replace('_', " ");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains(':') {
        return trimmed.to_string();
    }
    format!("Template:{trimmed}")
}

fn normalize_title_key(value: &str) -> String {
    value
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn mediawiki_query_template_pages(
    client: &mut ExternalClient,
    api_url: &str,
    titles: &[String],
    content_limit: usize,
) -> Result<Vec<MediaWikiTemplatePage>> {
    let mut pages_by_title = HashMap::<String, MediaWikiTemplatePage>::new();
    for batch in titles.chunks(DEFAULT_MEDIAWIKI_TEMPLATE_BATCH_SIZE) {
        let payload = client.request_json(
            api_url,
            &[
                ("action", "query".to_string()),
                ("titles", batch.join("|")),
                ("prop", "revisions".to_string()),
                ("rvprop", "ids|content|timestamp".to_string()),
                ("rvslots", "main".to_string()),
            ],
        )?;
        for page in parse_template_pages_payload(&payload, content_limit) {
            pages_by_title.insert(normalize_title_key(&page.title), page);
        }
    }

    Ok(titles
        .iter()
        .map(|title| {
            pages_by_title
                .remove(&normalize_title_key(title))
                .unwrap_or_else(|| missing_template_page(title))
        })
        .collect())
}

fn parse_template_pages_payload(
    payload: &Value,
    content_limit: usize,
) -> Vec<MediaWikiTemplatePage> {
    let Some(pages) = payload
        .get("query")
        .and_then(|value| value.get("pages"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for page in pages {
        let title = page
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if title.is_empty() {
            continue;
        }
        if page.get("missing").is_some() {
            out.push(missing_template_page(&title));
            continue;
        }
        let revision = page
            .get("revisions")
            .and_then(Value::as_array)
            .and_then(|revisions| revisions.first());
        let content = revision
            .and_then(|revision| revision.get("slots"))
            .and_then(|value| value.get("main"))
            .and_then(|value| value.get("content"))
            .and_then(Value::as_str);
        let revision_id = revision
            .and_then(|revision| revision.get("revid"))
            .and_then(Value::as_i64);
        let revision_timestamp = revision
            .and_then(|revision| revision.get("timestamp"))
            .and_then(Value::as_str)
            .map(ToString::to_string);
        match content {
            Some(content) => {
                let content_preview = truncate_to_byte_limit(content, content_limit);
                out.push(MediaWikiTemplatePage {
                    title,
                    exists: true,
                    revision_id,
                    revision_timestamp,
                    content_hash: Some(compute_hash(content)),
                    content_truncated: content_preview.len() < content.len(),
                    content_preview: Some(content_preview),
                    templatedata: None,
                });
            }
            None => out.push(MediaWikiTemplatePage {
                title,
                exists: true,
                revision_id,
                revision_timestamp,
                content_hash: None,
                content_preview: None,
                content_truncated: false,
                templatedata: None,
            }),
        }
    }
    out
}

fn missing_template_page(title: &str) -> MediaWikiTemplatePage {
    MediaWikiTemplatePage {
        title: title.to_string(),
        exists: false,
        revision_id: None,
        revision_timestamp: None,
        content_hash: None,
        content_preview: None,
        content_truncated: false,
        templatedata: None,
    }
}

fn mediawiki_query_templatedata(
    client: &mut ExternalClient,
    api_url: &str,
    titles: &[String],
    parameter_limit: usize,
) -> Result<BTreeMap<String, MediaWikiTemplateDataSummary>> {
    let mut out = BTreeMap::new();
    for batch in titles.chunks(DEFAULT_MEDIAWIKI_TEMPLATE_BATCH_SIZE) {
        let payload = client.request_json(
            api_url,
            &[
                ("action", "templatedata".to_string()),
                ("titles", batch.join("|")),
            ],
        )?;
        for (title, summary) in parse_templatedata_payload(&payload, parameter_limit) {
            out.insert(normalize_title_key(&title), summary);
        }
    }
    Ok(out)
}

fn parse_templatedata_payload(
    payload: &Value,
    parameter_limit: usize,
) -> Vec<(String, MediaWikiTemplateDataSummary)> {
    let Some(pages) = payload
        .get("pages")
        .or_else(|| payload.get("query").and_then(|value| value.get("pages")))
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    if let Some(items) = pages.as_array() {
        for page in items {
            if let Some((title, summary)) = parse_templatedata_page(page, parameter_limit) {
                out.push((title, summary));
            }
        }
        return out;
    }
    if let Some(items) = pages.as_object() {
        for page in items.values() {
            if let Some((title, summary)) = parse_templatedata_page(page, parameter_limit) {
                out.push((title, summary));
            }
        }
    }
    out
}

fn parse_templatedata_page(
    page: &Value,
    parameter_limit: usize,
) -> Option<(String, MediaWikiTemplateDataSummary)> {
    if page.get("missing").is_some() {
        return None;
    }
    let title = page.get("title").and_then(Value::as_str)?.to_string();
    let params = page.get("params").and_then(Value::as_object);
    let mut parameters = Vec::new();
    if let Some(params) = params {
        for (name, value) in params {
            parameters.push(MediaWikiTemplateDataParameter {
                name: name.to_string(),
                aliases: value
                    .get("aliases")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToString::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                label: localized_string(value.get("label")),
                description: localized_string(value.get("description")),
                param_type: value
                    .get("type")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                required: value
                    .get("required")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                suggested: value
                    .get("suggested")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                deprecated: deprecated_templatedata_value(value.get("deprecated")),
            });
        }
    }
    parameters.sort_by(|left, right| left.name.cmp(&right.name));
    let parameter_count = parameters.len();
    parameters.truncate(parameter_limit);
    Some((
        title,
        MediaWikiTemplateDataSummary {
            description: localized_string(page.get("description")),
            format: page
                .get("format")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            parameter_count,
            parameters,
        },
    ))
}

fn localized_string(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str() {
        return non_empty_string(text);
    }
    let object = value.as_object()?;
    if let Some(text) = object.get("en").and_then(Value::as_str) {
        return non_empty_string(text);
    }
    object
        .values()
        .find_map(Value::as_str)
        .and_then(non_empty_string)
}

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn deprecated_templatedata_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => !value.trim().is_empty(),
        _ => false,
    }
}

fn attach_templatedata(
    template_pages: &mut [MediaWikiTemplatePage],
    mut template_data: BTreeMap<String, MediaWikiTemplateDataSummary>,
) {
    for page in template_pages {
        page.templatedata = template_data.remove(&normalize_title_key(&page.title));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse_template_pages_payload, parse_templatedata_payload};

    #[test]
    fn parse_template_pages_payload_preserves_preview_and_missing_pages() {
        let payload = json!({
            "query": {
                "pages": [
                    {
                        "title": "Template:Speciesbox",
                        "revisions": [
                            {
                                "revid": 12,
                                "timestamp": "2026-04-01T00:00:00Z",
                                "slots": {
                                    "main": {
                                        "content": "abcdef"
                                    }
                                }
                            }
                        ]
                    },
                    {
                        "title": "Template:Missing",
                        "missing": true
                    }
                ]
            }
        });

        let pages = parse_template_pages_payload(&payload, 3);

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].title, "Template:Speciesbox");
        assert!(pages[0].exists);
        assert_eq!(pages[0].revision_id, Some(12));
        assert_eq!(
            pages[0].revision_timestamp.as_deref(),
            Some("2026-04-01T00:00:00Z")
        );
        assert_eq!(pages[0].content_preview.as_deref(), Some("abc"));
        assert!(pages[0].content_truncated);
        assert_eq!(pages[1].title, "Template:Missing");
        assert!(!pages[1].exists);
    }

    #[test]
    fn parse_templatedata_payload_extracts_parameter_contracts() {
        let payload = json!({
            "pages": {
                "123": {
                    "title": "Template:Speciesbox",
                    "description": {"en": "Species infobox"},
                    "format": "block",
                    "params": {
                        "taxon": {
                            "label": "Taxon",
                            "description": {"en": "Scientific taxon"},
                            "type": "string",
                            "required": true,
                            "suggested": true,
                            "aliases": ["species"]
                        },
                        "status": {
                            "type": "string",
                            "deprecated": "Use status_system with status"
                        }
                    }
                }
            }
        });

        let capped_pages = parse_templatedata_payload(&payload, 1);
        assert_eq!(capped_pages[0].1.parameter_count, 2);
        assert_eq!(capped_pages[0].1.parameters.len(), 1);

        let pages = parse_templatedata_payload(&payload, 64);

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].0, "Template:Speciesbox");
        let summary = &pages[0].1;
        assert_eq!(summary.description.as_deref(), Some("Species infobox"));
        assert_eq!(summary.format.as_deref(), Some("block"));
        assert_eq!(summary.parameter_count, 2);
        let status = summary
            .parameters
            .iter()
            .find(|parameter| parameter.name == "status")
            .expect("status parameter");
        assert!(status.deprecated);
        let taxon = summary
            .parameters
            .iter()
            .find(|parameter| parameter.name == "taxon")
            .expect("taxon parameter");
        assert_eq!(taxon.aliases, vec!["species"]);
        assert!(taxon.required);
        assert!(taxon.suggested);
    }
}
