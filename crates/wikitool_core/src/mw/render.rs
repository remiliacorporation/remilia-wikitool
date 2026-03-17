#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use super::client::MediaWikiClient;

#[derive(Debug, Clone)]
pub(crate) struct RenderedPageHtml {
    pub title: String,
    pub display_title: Option<String>,
    pub revision_id: Option<i64>,
    pub html: String,
}

#[derive(Debug, Deserialize, Default)]
struct ParseResponse {
    #[serde(default)]
    parse: Option<ParsePayload>,
}

#[derive(Debug, Deserialize, Default)]
struct ParsePayload {
    title: Option<String>,
    displaytitle: Option<String>,
    revid: Option<i64>,
    text: Option<ParseText>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ParseText {
    Html(String),
    Legacy {
        #[serde(default, rename = "*")]
        html: String,
    },
}

impl ParseText {
    fn into_html(self) -> String {
        match self {
            Self::Html(html) => html,
            Self::Legacy { html } => html,
        }
    }
}

pub(crate) fn render_page_html(
    client: &mut MediaWikiClient,
    title: &str,
) -> Result<Option<RenderedPageHtml>> {
    let response = client.request_json_get(&[
        ("action", "parse".to_string()),
        ("page", title.to_string()),
        ("prop", "text|displaytitle|revid".to_string()),
    ])?;
    decode_rendered_page_payload(response, title)
}

pub(crate) fn decode_rendered_page_payload(
    response: Value,
    requested_title: &str,
) -> Result<Option<RenderedPageHtml>> {
    let parsed: ParseResponse =
        serde_json::from_value(response).context("failed to decode parse API response")?;
    let payload = match parsed.parse {
        Some(payload) => payload,
        None => return Ok(None),
    };
    let html = payload
        .text
        .map(ParseText::into_html)
        .unwrap_or_default()
        .trim()
        .to_string();
    if html.is_empty() {
        return Ok(None);
    }

    Ok(Some(RenderedPageHtml {
        title: payload.title.unwrap_or_else(|| requested_title.to_string()),
        display_title: normalize_optional_string(payload.displaytitle),
        revision_id: payload.revid,
        html,
    }))
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::decode_rendered_page_payload;

    #[test]
    fn decodes_rendered_page_metadata() {
        let rendered = decode_rendered_page_payload(
            json!({
                "parse": {
                    "title": "Main Page",
                    "displaytitle": "<i>Main Page</i>",
                    "revid": 42,
                    "text": {
                        "*": "<p>Hello</p>"
                    }
                }
            }),
            "Main Page",
        )
        .expect("parse response should decode")
        .expect("rendered page should be present");

        assert_eq!(rendered.title, "Main Page");
        assert_eq!(rendered.display_title.as_deref(), Some("<i>Main Page</i>"));
        assert_eq!(rendered.revision_id, Some(42));
        assert_eq!(rendered.html, "<p>Hello</p>");
    }

    #[test]
    fn decodes_formatversion_two_rendered_html() {
        let rendered = decode_rendered_page_payload(
            json!({
                "parse": {
                    "title": "Main Page",
                    "displaytitle": "Main Page",
                    "revid": 43,
                    "text": "<p>Hello v2</p>"
                }
            }),
            "Main Page",
        )
        .expect("parse response should decode")
        .expect("rendered page should be present");

        assert_eq!(rendered.revision_id, Some(43));
        assert_eq!(rendered.html, "<p>Hello v2</p>");
    }
}
