use std::collections::BTreeMap;

use reqwest::Url;

use super::model::HtmlMetadata;
use super::tags::{collect_meta, extract_head, extract_title, find_canonical, scan_tags};
use crate::research::url::decode_title;
pub(in crate::research::web_fetch) fn derive_title_from_url(
    parsed_url: Option<&Url>,
    final_url: &str,
) -> String {
    parsed_url
        .and_then(|value| value.path_segments())
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.trim().is_empty())
        .map(decode_title)
        .unwrap_or_else(|| final_url.to_string())
}

pub(in crate::research::web_fetch) fn extract_html_metadata(
    html: &str,
    final_url: &str,
) -> HtmlMetadata {
    let head = extract_head(html);
    let title = extract_title(&head);
    let meta = collect_meta(&scan_tags(&head, "meta"));
    let canonical_url = find_canonical(&scan_tags(&head, "link"))
        .or_else(|| meta_first(&meta, &["og:url", "twitter:url"]))
        .or_else(|| Some(final_url.to_string()));

    HtmlMetadata {
        title: meta_first(&meta, &["og:title", "twitter:title"]).or(title),
        canonical_url,
        site_name: meta_first(&meta, &["og:site_name", "application-name"]),
        byline: meta_first(
            &meta,
            &[
                "author",
                "article:author",
                "parsely-author",
                "dc.creator",
                "dc.creator.creator",
            ],
        ),
        published_at: meta_first(
            &meta,
            &[
                "article:published_time",
                "og:published_time",
                "pubdate",
                "publish-date",
                "parsely-pub-date",
                "date",
            ],
        ),
        description: meta_first(
            &meta,
            &["description", "og:description", "twitter:description"],
        ),
    }
}

pub(in crate::research::web_fetch) fn extract_client_redirect_url(
    html: &str,
    final_url: &str,
) -> Option<String> {
    let head = extract_head(html);
    let base_url = Url::parse(final_url).ok()?;
    for tag in scan_tags(&head, "meta") {
        let http_equiv = tag
            .attrs
            .get("http-equiv")
            .map(|value| value.to_ascii_lowercase());
        let id = tag.attrs.get("id").map(|value| value.to_ascii_lowercase());
        if http_equiv.as_deref() != Some("refresh") && id.as_deref() != Some("__next-page-redirect")
        {
            continue;
        }
        let content = tag.attrs.get("content")?;
        let target = parse_meta_refresh_target(content)?;
        let joined = base_url.join(target).ok()?.to_string();
        return Some(joined);
    }
    None
}

fn parse_meta_refresh_target(content: &str) -> Option<&str> {
    let lowered = content.to_ascii_lowercase();
    let marker = "url=";
    let at = lowered.find(marker)?;
    let target = content[at + marker.len()..].trim();
    let target = target.trim_matches(|ch| matches!(ch, '"' | '\'' | ' '));
    if target.is_empty() {
        None
    } else {
        Some(target)
    }
}

fn meta_first(meta: &BTreeMap<String, Vec<String>>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        meta.get(*key)
            .and_then(|values| values.first())
            .cloned()
            .filter(|value| !value.trim().is_empty())
    })
}
