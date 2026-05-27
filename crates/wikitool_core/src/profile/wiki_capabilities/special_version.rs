use super::html::{
    clean_label, clean_version_label, collapse_whitespace, extract_caption_text,
    extract_code_values, extract_first_tag_text_with_class, extract_head,
    extract_section_between_ids, extract_table_blocks_with_class, extract_table_rows_by_id,
    extract_tag_blocks, normalize_extension_name, normalize_preserved_string_list,
    normalize_string_list, parse_mediawiki_version, resolve_href, scan_tags, tag_block_has_class,
};
use super::*;

#[derive(Debug, Default)]
pub(super) struct SpecialVersionInfo {
    pub(super) article_path: Option<String>,
    pub(super) rest_url: Option<String>,
    pub(super) mediawiki_version: Option<String>,
    pub(super) extensions: Vec<ExtensionInfo>,
    pub(super) parser_extension_tags: Vec<String>,
    pub(super) parser_function_hooks: Vec<String>,
}

impl SpecialVersionInfo {
    fn is_empty(&self) -> bool {
        self.article_path.is_none()
            && self.rest_url.is_none()
            && self.mediawiki_version.is_none()
            && self.extensions.is_empty()
            && self.parser_extension_tags.is_empty()
            && self.parser_function_hooks.is_empty()
    }
}

pub(super) fn fetch_special_version_info(
    client: &mut MediaWikiClient,
    wiki_url: &str,
    article_path: &str,
) -> Result<Option<SpecialVersionInfo>> {
    let special_version_url = build_article_url(wiki_url, article_path, "Special:Version")?;
    for attempt in 0..=client.config.max_retries {
        client.apply_rate_limit(false);
        let response = client
            .client
            .get(&special_version_url)
            .header("User-Agent", client.config.user_agent.clone())
            .send();
        match response {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    if attempt < client.config.max_retries && is_retryable_status(status) {
                        client.wait_before_retry(attempt, false);
                        continue;
                    }
                    return Ok(None);
                }

                let html = response
                    .text()
                    .context("failed to read Special:Version response body")?;
                let parsed = parse_special_version_html(&html, wiki_url);
                return if parsed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(parsed))
                };
            }
            Err(error) => {
                if attempt < client.config.max_retries && is_retryable_error(&error) {
                    client.wait_before_retry(attempt, false);
                    continue;
                }
                return Ok(None);
            }
        }
    }

    Ok(None)
}

pub(super) fn apply_special_version_info(
    manifest: &mut WikiCapabilityManifest,
    info: SpecialVersionInfo,
) {
    if let Some(value) = info.mediawiki_version {
        manifest.mediawiki_version = Some(value);
    }
    if let Some(value) = info.article_path {
        manifest.article_path = value;
    }
    if let Some(value) = info.rest_url {
        manifest.rest_url = Some(value);
        manifest.supports_rest_html = true;
    }
    manifest.extensions =
        merge_extensions(std::mem::take(&mut manifest.extensions), info.extensions);
    if manifest.parser_extension_tags.is_empty() && !info.parser_extension_tags.is_empty() {
        manifest.parser_extension_tags = info.parser_extension_tags;
    }
    if manifest.parser_function_hooks.is_empty() && !info.parser_function_hooks.is_empty() {
        manifest.parser_function_hooks = info.parser_function_hooks;
    }
    refresh_manifest_flags(manifest);
}

pub(super) fn parse_special_version_html(html: &str, wiki_url: &str) -> SpecialVersionInfo {
    let mut info = SpecialVersionInfo {
        mediawiki_version: extract_table_value_by_label(html, "sv-software", "MediaWiki"),
        article_path: extract_entrypoint_value(html, wiki_url, "Article path", false),
        rest_url: extract_entrypoint_value(html, wiki_url, "rest.php", true),
        extensions: extract_special_version_extensions(html),
        parser_extension_tags: extract_special_version_tags(html),
        parser_function_hooks: extract_special_version_hooks(html),
    };

    if info.mediawiki_version.is_none() {
        info.mediawiki_version = extract_meta_generator_version(html);
    }

    info
}

fn extract_entrypoint_value(
    html: &str,
    wiki_url: &str,
    label: &str,
    prefer_href: bool,
) -> Option<String> {
    for row in extract_table_rows_by_id(html, "mw-version-entrypoints-table") {
        if row.len() < 2 || !row[0].text.eq_ignore_ascii_case(label) {
            continue;
        }
        if prefer_href
            && let Some(value) = row[1]
                .href
                .as_deref()
                .and_then(|href| resolve_href(wiki_url, href))
        {
            return Some(value);
        }
        if let Some(value) = clean_label(&row[1].text) {
            return Some(value);
        }
    }
    None
}

fn extract_table_value_by_label(html: &str, table_id: &str, label: &str) -> Option<String> {
    for row in extract_table_rows_by_id(html, table_id) {
        if row.len() < 2 || !row[0].text.eq_ignore_ascii_case(label) {
            continue;
        }
        if let Some(value) = clean_version_label(&row[1].text) {
            return Some(value);
        }
    }
    None
}

fn extract_meta_generator_version(html: &str) -> Option<String> {
    let head = extract_head(html);
    for tag in scan_tags(&head, "meta") {
        let name = tag
            .attrs
            .get("name")
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if name != "generator" {
            continue;
        }
        let content = tag.attrs.get("content")?;
        return parse_mediawiki_version(content);
    }
    None
}

fn extract_special_version_extensions(html: &str) -> Vec<ExtensionInfo> {
    let Some(section) = extract_section_between_ids(html, "mw-version-ext", "mw-version-libraries")
    else {
        return Vec::new();
    };

    let mut extensions = Vec::new();
    for table in extract_table_blocks_with_class(section, "mw-installed-software") {
        let category = extract_caption_text(table);
        for row in extract_tag_blocks(table, "tr") {
            if !tag_block_has_class(row, "tr", "mw-version-ext") {
                continue;
            }
            let Some(name) = extract_first_tag_text_with_class(row, "mw-version-ext-name")
                .and_then(|value| clean_label(&normalize_extension_name(&value)))
            else {
                continue;
            };
            let version = extract_first_tag_text_with_class(row, "mw-version-ext-version")
                .and_then(|value| clean_version_label(&value));
            extensions.push(ExtensionInfo {
                name,
                version,
                category: category.clone(),
            });
        }
    }

    extensions.sort_by_key(|extension| extension.name.to_ascii_lowercase());
    extensions.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
    extensions
}

fn extract_special_version_tags(html: &str) -> Vec<String> {
    let Some(section) = extract_section_between_ids(
        html,
        "mw-version-parser-extensiontags",
        "mw-version-parser-function-hooks",
    ) else {
        return Vec::new();
    };

    normalize_string_list(
        extract_code_values(section)
            .into_iter()
            .filter_map(|value| {
                clean_label(value.trim_matches(['<', '>'])).map(|value| value.to_ascii_lowercase())
            })
            .collect(),
    )
}

fn extract_special_version_hooks(html: &str) -> Vec<String> {
    let Some(section) = extract_section_between_ids(
        html,
        "mw-version-parser-function-hooks",
        "mw-version-parsoid-modules",
    ) else {
        return Vec::new();
    };

    normalize_preserved_string_list(
        extract_code_values(section)
            .into_iter()
            .filter_map(|value| {
                let trimmed = value.trim();
                let inner = trimmed
                    .strip_prefix("{{")
                    .and_then(|value| value.strip_suffix("}}"))
                    .unwrap_or(trimmed);
                let collapsed = collapse_whitespace(inner);
                if collapsed.is_empty() {
                    None
                } else {
                    Some(collapsed)
                }
            })
            .collect(),
    )
}

pub(super) fn build_article_url(wiki_url: &str, article_path: &str, title: &str) -> Result<String> {
    let wiki_url = Url::parse(wiki_url)
        .with_context(|| format!("invalid wiki URL for Special:Version fetch: {wiki_url}"))?;
    let title = title.replace(' ', "_");
    let relative = if article_path.contains("$1") {
        article_path.replace("$1", &title)
    } else {
        let base = article_path.trim_end_matches('/');
        if base.is_empty() {
            format!("/{title}")
        } else {
            format!("{base}/{title}")
        }
    };
    let join_target = if relative.starts_with('/') || relative.starts_with('?') {
        relative
    } else if needs_relative_path_prefix(&relative) {
        format!("./{relative}")
    } else {
        relative
    };
    wiki_url
        .join(&join_target)
        .map(|url| url.to_string())
        .with_context(|| format!("failed to build Special:Version URL from {}", article_path))
}

fn needs_relative_path_prefix(value: &str) -> bool {
    value
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|segment| segment.contains(':'))
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn is_retryable_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}
