#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::Deserialize;

use super::client::MediaWikiClient;

#[derive(Debug, Deserialize, Default)]
struct ParseResponse {
    #[serde(default)]
    parse: Option<ParsePayload>,
}

#[derive(Debug, Deserialize, Default)]
struct ParsePayload {
    title: Option<String>,
    text: Option<ParseText>,
}

#[derive(Debug, Deserialize, Default)]
struct ParseText {
    #[serde(default, rename = "*")]
    html: String,
}

pub(crate) fn render_page_html(
    client: &mut MediaWikiClient,
    title: &str,
) -> Result<Option<(String, String)>> {
    let response = client.request_json_get(&[
        ("action", "parse".to_string()),
        ("page", title.to_string()),
        ("prop", "text".to_string()),
    ])?;
    let parsed: ParseResponse =
        serde_json::from_value(response).context("failed to decode parse API response")?;
    let payload = match parsed.parse {
        Some(payload) => payload,
        None => return Ok(None),
    };
    let html = payload
        .text
        .map(|text| text.html)
        .unwrap_or_default()
        .trim()
        .to_string();
    if html.is_empty() {
        return Ok(None);
    }
    Ok(Some((
        payload.title.unwrap_or_else(|| title.to_string()),
        html,
    )))
}
