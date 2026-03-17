use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use reqwest::Url;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::config::{DEFAULT_ARTICLE_PATH, WikiConfig, derive_wiki_url};
use crate::knowledge::status::KNOWLEDGE_GENERATION;
use crate::mw::client::MediaWikiClient;
use crate::mw::namespace::namespace_display_name;
use crate::mw::siteinfo::SiteInfoNamespace;
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{compute_hash, unix_timestamp};

const WIKI_CAPABILITY_ARTIFACT_KIND: &str = "wiki_capabilities";
const WIKI_CAPABILITY_SCHEMA_VERSION: &str = "wiki_capabilities_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamespaceInfo {
    pub id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_name: Option<String>,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WikiCapabilityManifest {
    pub schema_version: String,
    pub wiki_id: String,
    pub wiki_url: String,
    pub api_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest_url: Option<String>,
    pub article_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mediawiki_version: Option<String>,
    pub namespaces: Vec<NamespaceInfo>,
    pub extensions: Vec<ExtensionInfo>,
    pub parser_extension_tags: Vec<String>,
    pub parser_function_hooks: Vec<String>,
    pub special_pages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_backend_hint: Option<String>,
    pub has_visual_editor: bool,
    pub has_templatedata: bool,
    pub has_citoid: bool,
    pub has_cargo: bool,
    pub has_page_forms: bool,
    pub has_short_description: bool,
    pub has_scribunto: bool,
    pub has_timed_media_handler: bool,
    pub supports_parse_api_html: bool,
    pub supports_rest_html: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest_html_path_template: Option<String>,
    pub refreshed_at: String,
}

impl WikiCapabilityManifest {
    pub fn supports_extension(&self, name: &str) -> bool {
        self.extensions
            .iter()
            .any(|extension| extension.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Deserialize, Default)]
struct SiteInfoResponse {
    #[serde(default)]
    query: SiteInfoQuery,
}

#[derive(Debug, Deserialize, Default)]
struct SiteInfoQuery {
    #[serde(default)]
    general: SiteInfoGeneral,
    #[serde(default)]
    namespaces: BTreeMap<String, SiteInfoNamespace>,
    #[serde(default)]
    extensions: Vec<SiteInfoExtension>,
    #[serde(default)]
    specialpagealiases: Vec<SiteInfoSpecialPage>,
    #[serde(default)]
    extensiontags: Vec<String>,
    #[serde(default)]
    functionhooks: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SiteInfoGeneral {
    #[serde(default)]
    articlepath: String,
    #[serde(default)]
    generator: String,
}

#[derive(Debug, Deserialize, Default)]
struct SiteInfoExtension {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "type")]
    category: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SiteInfoSpecialPage {
    #[serde(default)]
    realname: String,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Default)]
struct SpecialVersionInfo {
    article_path: Option<String>,
    rest_url: Option<String>,
    mediawiki_version: Option<String>,
    extensions: Vec<ExtensionInfo>,
    parser_extension_tags: Vec<String>,
    parser_function_hooks: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableCell {
    text: String,
    href: Option<String>,
}

pub fn sync_wiki_capabilities_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<WikiCapabilityManifest> {
    let mut client = MediaWikiClient::from_config(config)?;
    if client.config.api_url.trim().is_empty() {
        bail!("wiki API URL is not configured (set [wiki].api_url or WIKI_API_URL)");
    }

    let api_url = client.config.api_url.clone();
    let wiki_url = resolve_wiki_url(config, &api_url)?;
    let siteinfo = fetch_siteinfo_base(&mut client)?;
    let parser_extension_tags = fetch_optional_siteinfo_list(&mut client, "extensiontags")?;
    let parser_function_hooks = fetch_optional_siteinfo_list(&mut client, "functionhooks")?;
    let refreshed_at = unix_timestamp()?.to_string();
    let mut manifest = build_manifest_from_siteinfo(
        &api_url,
        &wiki_url,
        &config.article_path_owned(),
        &siteinfo,
        parser_extension_tags,
        parser_function_hooks,
        &refreshed_at,
    );
    if let Some(special_version) =
        fetch_special_version_info(&mut client, &manifest.wiki_url, &manifest.article_path)?
    {
        apply_special_version_info(&mut manifest, special_version);
    }
    store_wiki_capabilities(paths, &manifest)?;
    Ok(manifest)
}

pub fn load_wiki_capabilities_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<Option<WikiCapabilityManifest>> {
    if let Some(wiki_url) = config.wiki_url().or_else(|| {
        config
            .api_url_owned()
            .and_then(|value| derive_wiki_url(&value))
    }) && let Some(manifest) = load_wiki_capabilities(paths, &derive_wiki_id(&wiki_url))?
    {
        return Ok(Some(manifest));
    }

    load_latest_wiki_capabilities(paths)
}

pub fn load_latest_wiki_capabilities(
    paths: &ResolvedPaths,
) -> Result<Option<WikiCapabilityManifest>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let manifest_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM knowledge_artifacts
             WHERE artifact_kind = ?1
             ORDER BY built_at_unix DESC
             LIMIT 1",
            params![WIKI_CAPABILITY_ARTIFACT_KIND],
            |row| row.get(0),
        )
        .optional()
        .context("failed to load latest wiki capability manifest")?;

    manifest_json
        .map(|value| {
            serde_json::from_str(&value).context("failed to decode wiki capability manifest")
        })
        .transpose()
}

fn load_wiki_capabilities(
    paths: &ResolvedPaths,
    wiki_id: &str,
) -> Result<Option<WikiCapabilityManifest>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let manifest_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM knowledge_artifacts
             WHERE artifact_key = ?1",
            params![wiki_capabilities_artifact_key(wiki_id)],
            |row| row.get(0),
        )
        .optional()
        .with_context(|| format!("failed to load wiki capability manifest for {wiki_id}"))?;

    manifest_json
        .map(|value| {
            serde_json::from_str(&value).context("failed to decode wiki capability manifest")
        })
        .transpose()
}

fn store_wiki_capabilities(paths: &ResolvedPaths, manifest: &WikiCapabilityManifest) -> Result<()> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let metadata_json = serde_json::to_string_pretty(manifest)
        .context("failed to serialize wiki capability manifest")?;
    let built_at_unix = unix_timestamp()?;
    let row_count = manifest
        .namespaces
        .len()
        .saturating_add(manifest.extensions.len())
        .saturating_add(manifest.special_pages.len());

    connection
        .execute(
            "INSERT INTO knowledge_artifacts (
                artifact_key,
                artifact_kind,
                profile,
                schema_generation,
                built_at_unix,
                row_count,
                metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(artifact_key) DO UPDATE SET
                artifact_kind = excluded.artifact_kind,
                profile = excluded.profile,
                schema_generation = excluded.schema_generation,
                built_at_unix = excluded.built_at_unix,
                row_count = excluded.row_count,
                metadata_json = excluded.metadata_json",
            params![
                wiki_capabilities_artifact_key(&manifest.wiki_id),
                WIKI_CAPABILITY_ARTIFACT_KIND,
                Some(manifest.wiki_id.as_str()),
                KNOWLEDGE_GENERATION,
                i64::try_from(built_at_unix).context("artifact timestamp does not fit into i64")?,
                i64::try_from(row_count).context("artifact row count does not fit into i64")?,
                metadata_json,
            ],
        )
        .with_context(|| {
            format!(
                "failed to store wiki capability manifest for {}",
                manifest.wiki_id
            )
        })?;

    Ok(())
}

fn fetch_siteinfo_base(client: &mut MediaWikiClient) -> Result<SiteInfoQuery> {
    let payload = client.request_json_get(&[
        ("action", "query".to_string()),
        ("meta", "siteinfo".to_string()),
        (
            "siprop",
            "general|namespaces|extensions|specialpagealiases".to_string(),
        ),
    ])?;
    let parsed: SiteInfoResponse = serde_json::from_value(payload)
        .context("failed to decode wiki capability siteinfo response")?;
    Ok(parsed.query)
}

fn fetch_optional_siteinfo_list(client: &mut MediaWikiClient, siprop: &str) -> Result<Vec<String>> {
    let payload = match client.request_json_get(&[
        ("action", "query".to_string()),
        ("meta", "siteinfo".to_string()),
        ("siprop", siprop.to_string()),
    ]) {
        Ok(payload) => payload,
        Err(error) if is_optional_siprop_error(&error, siprop) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let parsed: SiteInfoResponse =
        serde_json::from_value(payload).context("failed to decode optional siteinfo response")?;
    let values = match siprop {
        "extensiontags" => parsed.query.extensiontags,
        "functionhooks" => parsed.query.functionhooks,
        _ => Vec::new(),
    };
    Ok(normalize_string_list(values))
}

fn is_optional_siprop_error(error: &anyhow::Error, siprop: &str) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("siprop") && message.contains(&siprop.to_ascii_lowercase())
}

fn fetch_special_version_info(
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

fn apply_special_version_info(manifest: &mut WikiCapabilityManifest, info: SpecialVersionInfo) {
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

fn build_manifest_from_siteinfo(
    api_url: &str,
    wiki_url: &str,
    default_article_path: &str,
    siteinfo: &SiteInfoQuery,
    parser_extension_tags: Vec<String>,
    parser_function_hooks: Vec<String>,
    refreshed_at: &str,
) -> WikiCapabilityManifest {
    let rest_url = Some(format!("{}/rest.php", wiki_url.trim_end_matches('/')));
    let namespaces = build_namespaces(&siteinfo.namespaces);
    let extensions = build_extensions(&siteinfo.extensions);
    let special_pages = build_special_pages(&siteinfo.specialpagealiases);
    let mediawiki_version = parse_mediawiki_version(&siteinfo.general.generator);
    let search_backend_hint = determine_search_backend_hint(&extensions);
    let article_path = clean_label(&siteinfo.general.articlepath)
        .unwrap_or_else(|| fallback_article_path(default_article_path));

    let mut manifest = WikiCapabilityManifest {
        schema_version: WIKI_CAPABILITY_SCHEMA_VERSION.to_string(),
        wiki_id: derive_wiki_id(wiki_url),
        wiki_url: wiki_url.trim().to_string(),
        api_url: api_url.trim().to_string(),
        rest_url,
        article_path,
        mediawiki_version,
        namespaces,
        extensions,
        parser_extension_tags,
        parser_function_hooks,
        special_pages,
        search_backend_hint,
        has_visual_editor: false,
        has_templatedata: false,
        has_citoid: false,
        has_cargo: false,
        has_page_forms: false,
        has_short_description: false,
        has_scribunto: false,
        has_timed_media_handler: false,
        supports_parse_api_html: true,
        supports_rest_html: false,
        rest_html_path_template: None,
        refreshed_at: refreshed_at.to_string(),
    };

    refresh_manifest_flags(&mut manifest);
    manifest
}

fn parse_special_version_html(html: &str, wiki_url: &str) -> SpecialVersionInfo {
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

fn build_article_url(wiki_url: &str, article_path: &str, title: &str) -> Result<String> {
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

fn refresh_manifest_flags(manifest: &mut WikiCapabilityManifest) {
    manifest.search_backend_hint = determine_search_backend_hint(&manifest.extensions);
    manifest.has_visual_editor = manifest.supports_extension("VisualEditor");
    manifest.has_templatedata = manifest.supports_extension("TemplateData");
    manifest.has_citoid = manifest.supports_extension("Citoid");
    manifest.has_cargo = manifest.supports_extension("Cargo");
    manifest.has_page_forms = manifest.supports_extension("Page Forms");
    manifest.has_short_description = manifest.supports_extension("ShortDescription");
    manifest.has_scribunto = manifest.supports_extension("Scribunto");
    manifest.has_timed_media_handler = manifest.supports_extension("TimedMediaHandler");
    manifest.rest_html_path_template = manifest
        .rest_url
        .as_deref()
        .filter(|_| manifest.supports_rest_html)
        .map(|value| format!("{}/v1/page/$TITLE/html", value.trim_end_matches('/')));
}

fn merge_extensions(base: Vec<ExtensionInfo>, overlay: Vec<ExtensionInfo>) -> Vec<ExtensionInfo> {
    let mut merged = BTreeMap::<String, ExtensionInfo>::new();
    for extension in base {
        merged.insert(extension.name.to_ascii_lowercase(), extension);
    }
    for extension in overlay {
        let key = extension.name.to_ascii_lowercase();
        if let Some(existing) = merged.get_mut(&key) {
            if extension.version.is_some() {
                existing.version = extension.version.clone();
            }
            if extension.category.is_some() {
                existing.category = extension.category.clone();
            }
            if existing.name.len() < extension.name.len() {
                existing.name = extension.name.clone();
            }
        } else {
            merged.insert(key, extension);
        }
    }

    let mut out = merged.into_values().collect::<Vec<_>>();
    out.sort_by_key(|extension| extension.name.to_ascii_lowercase());
    out
}

fn resolve_wiki_url(config: &WikiConfig, api_url: &str) -> Result<String> {
    config
        .wiki_url()
        .or_else(|| derive_wiki_url(api_url))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("failed to derive wiki URL from configured API URL"))
}

fn build_namespaces(namespaces: &BTreeMap<String, SiteInfoNamespace>) -> Vec<NamespaceInfo> {
    let mut out = Vec::new();
    for (key, namespace) in namespaces {
        let display_name = namespace_display_name(namespace).or_else(|| {
            if namespace.id == 0 || key == "0" {
                return Some("Main".to_string());
            }
            clean_label(key)
        });
        let Some(display_name) = display_name else {
            continue;
        };
        out.push(NamespaceInfo {
            id: namespace.id,
            canonical_name: clean_label(namespace.canonical.as_deref().unwrap_or_default()),
            display_name,
        });
    }
    out.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    out.dedup_by_key(|namespace| namespace.id);
    out
}

fn build_extensions(extensions: &[SiteInfoExtension]) -> Vec<ExtensionInfo> {
    let mut out = extensions
        .iter()
        .filter_map(|extension| {
            let name = normalize_extension_name(&extension.name);
            if name.is_empty() {
                return None;
            }
            Some(ExtensionInfo {
                name,
                version: extension
                    .version
                    .as_deref()
                    .and_then(clean_label)
                    .filter(|value| !value.eq_ignore_ascii_case("unknown")),
                category: extension.category.as_deref().and_then(clean_label),
            })
        })
        .collect::<Vec<_>>();

    out.sort_by_key(|extension| extension.name.to_ascii_lowercase());
    out.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
    out
}

fn build_special_pages(pages: &[SiteInfoSpecialPage]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for page in pages {
        let label = clean_label(&page.realname)
            .or_else(|| page.aliases.iter().find_map(|alias| clean_label(alias)));
        let Some(label) = label else {
            continue;
        };
        if seen.insert(label.to_ascii_lowercase()) {
            out.push(label);
        }
    }
    out.sort_unstable();
    out
}

fn determine_search_backend_hint(extensions: &[ExtensionInfo]) -> Option<String> {
    if extensions
        .iter()
        .any(|extension| extension.name.eq_ignore_ascii_case("CirrusSearch"))
    {
        return Some("cirrussearch".to_string());
    }
    if extensions
        .iter()
        .any(|extension| extension.name.eq_ignore_ascii_case("AdvancedSearch"))
    {
        return Some("advancedsearch".to_string());
    }
    None
}

fn extract_table_rows_by_id(html: &str, table_id: &str) -> Vec<Vec<TableCell>> {
    let Some(table_html) = extract_element_inner_html_by_id(html, "table", table_id) else {
        return Vec::new();
    };

    extract_tag_blocks(table_html, "tr")
        .into_iter()
        .filter_map(|row| {
            let cells = extract_table_cells(row);
            if cells.is_empty() { None } else { Some(cells) }
        })
        .collect()
}

fn extract_table_cells(row_html: &str) -> Vec<TableCell> {
    extract_tag_blocks(row_html, "td")
        .into_iter()
        .filter_map(|cell| {
            let content = inner_html_from_block(cell, "td")?;
            let text = html_text(content);
            if text.is_empty() {
                return None;
            }
            Some(TableCell {
                text,
                href: extract_first_href(content),
            })
        })
        .collect()
}

fn extract_table_blocks_with_class<'a>(html: &'a str, class_name: &str) -> Vec<&'a str> {
    extract_tag_blocks(html, "table")
        .into_iter()
        .filter(|table| tag_block_has_class(table, "table", class_name))
        .collect()
}

fn extract_caption_text(table_html: &str) -> Option<String> {
    extract_tag_blocks(table_html, "caption")
        .into_iter()
        .next()
        .and_then(|caption| inner_html_from_block(caption, "caption"))
        .map(html_text)
        .and_then(|value| clean_label(&value))
}

fn extract_code_values(html: &str) -> Vec<String> {
    extract_tag_blocks(html, "code")
        .into_iter()
        .filter_map(|code| inner_html_from_block(code, "code"))
        .map(html_text)
        .filter(|value| !value.is_empty())
        .collect()
}

fn extract_first_href(html: &str) -> Option<String> {
    for tag in scan_tags(html, "a") {
        if let Some(href) = tag.attrs.get("href") {
            let decoded = decode_html(href).trim().to_string();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    None
}

fn extract_first_tag_text_with_class(html: &str, class_name: &str) -> Option<String> {
    let mut index = 0usize;
    while index < html.len() {
        let Some(lt) = html[index..].find('<') else {
            break;
        };
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", at + 4) {
                index = end + 3;
            } else {
                break;
            }
            continue;
        }
        let Some(open_end) = find_tag_end(html, at) else {
            break;
        };
        let raw = &html[at..=open_end];
        let Some((tag_name, is_closing, is_self_closing)) = parse_tag_descriptor(raw) else {
            index = open_end + 1;
            continue;
        };
        if is_closing {
            index = open_end + 1;
            continue;
        }
        let attrs = parse_attributes(raw, tag_name);
        if class_contains(attrs.get("class"), class_name) {
            if is_self_closing {
                return None;
            }
            let close = find_matching_close_tag(html, tag_name, open_end + 1)?;
            return clean_label(&html_text(&html[open_end + 1..close]));
        }
        index = open_end + 1;
    }
    None
}

fn extract_element_inner_html_by_id<'a>(
    html: &'a str,
    tag_name: &str,
    element_id: &str,
) -> Option<&'a str> {
    let (start, open_end) = find_tag_by_id(html, tag_name, element_id, 0)?;
    let close = find_matching_close_tag(html, tag_name, open_end + 1)?;
    let _ = start;
    Some(&html[open_end + 1..close])
}

fn extract_section_between_ids<'a>(html: &'a str, start_id: &str, end_id: &str) -> Option<&'a str> {
    let (_, open_end) = find_tag_by_id(html, "h2", start_id, 0)?;
    let end = find_tag_by_id(html, "h2", end_id, open_end + 1)
        .map(|(start, _)| start)
        .unwrap_or(html.len());
    Some(&html[open_end + 1..end])
}

fn find_tag_by_id(
    html: &str,
    tag_name: &str,
    element_id: &str,
    start: usize,
) -> Option<(usize, usize)> {
    let mut index = start;
    while let Some(at) = find_tag_start(html, tag_name, index) {
        let open_end = find_tag_end(html, at)?;
        let attrs = parse_attributes(&html[at..=open_end], tag_name);
        if attrs
            .get("id")
            .is_some_and(|value| value.eq_ignore_ascii_case(element_id))
        {
            return Some((at, open_end));
        }
        index = open_end + 1;
    }
    None
}

fn extract_tag_blocks<'a>(html: &'a str, tag_name: &str) -> Vec<&'a str> {
    let mut output = Vec::new();
    let mut index = 0usize;

    while let Some(at) = find_tag_start(html, tag_name, index) {
        let Some(open_end) = find_tag_end(html, at) else {
            break;
        };
        let Some(close_start) = find_matching_close_tag(html, tag_name, open_end + 1) else {
            index = open_end + 1;
            continue;
        };
        let Some(close_end) = find_tag_end(html, close_start) else {
            break;
        };
        output.push(&html[at..=close_end]);
        index = close_end + 1;
    }

    output
}

fn inner_html_from_block<'a>(html: &'a str, tag_name: &str) -> Option<&'a str> {
    let open_end = find_tag_end(html, 0)?;
    let close_start = find_matching_close_tag(html, tag_name, open_end + 1)?;
    Some(&html[open_end + 1..close_start])
}

fn tag_block_has_class(html: &str, tag_name: &str, class_name: &str) -> bool {
    let Some(open_end) = find_tag_end(html, 0) else {
        return false;
    };
    let attrs = parse_attributes(&html[..=open_end], tag_name);
    class_contains(attrs.get("class"), class_name)
}

fn class_contains(value: Option<&String>, needle: &str) -> bool {
    value.is_some_and(|value| value.split_whitespace().any(|part| part == needle))
}

fn resolve_href(wiki_url: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(href) {
        return Some(url.to_string());
    }
    let base = Url::parse(wiki_url).ok()?;
    base.join(href.trim_start_matches('/'))
        .ok()
        .map(|url| url.to_string())
}

fn clean_version_label(value: &str) -> Option<String> {
    let value = clean_label(value)?;
    if matches!(value.as_str(), "-" | "–" | "—") {
        None
    } else {
        Some(value)
    }
}

fn parse_mediawiki_version(generator: &str) -> Option<String> {
    let trimmed = generator.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(version) = trimmed.strip_prefix("MediaWiki ") {
        return clean_label(version);
    }
    clean_label(trimmed)
}

fn normalize_extension_name(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("Extension:")
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for value in values {
        let Some(normalized) = clean_label(&value) else {
            continue;
        };
        if seen.insert(normalized.to_ascii_lowercase()) {
            out.push(normalized);
        }
    }
    out.sort_unstable();
    out
}

fn normalize_preserved_string_list(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for value in values {
        let normalized = collapse_whitespace(&value);
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.to_ascii_lowercase()) {
            out.push(normalized);
        }
    }
    out.sort_unstable();
    out
}

fn fallback_article_path(value: &str) -> String {
    clean_label(value).unwrap_or_else(|| DEFAULT_ARTICLE_PATH.to_string())
}

fn clean_label(value: &str) -> Option<String> {
    let normalized = value
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn derive_wiki_id(wiki_url: &str) -> String {
    let trimmed = wiki_url.trim();
    if let Ok(url) = Url::parse(trimmed) {
        let mut parts = Vec::new();
        if let Some(host) = url.host_str() {
            parts.push(host.to_string());
        }
        let path = url.path().trim_matches('/');
        if !path.is_empty() {
            parts.extend(path.split('/').map(ToString::to_string));
        }
        if !parts.is_empty() {
            return parts.join("-");
        }
    }
    compute_hash(trimmed)
}

fn wiki_capabilities_artifact_key(wiki_id: &str) -> String {
    format!("wiki_capabilities:{wiki_id}")
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

#[derive(Debug, Clone)]
struct TagMatch {
    attrs: BTreeMap<String, String>,
}

fn extract_head(html: &str) -> String {
    let Some(head_start) = find_tag_start(html, "head", 0) else {
        return html.to_string();
    };
    let Some(open_end) = find_tag_end(html, head_start) else {
        return html.to_string();
    };
    let Some(close_index) = index_of_ignore_case(html, "</head>", open_end + 1) else {
        return html[open_end + 1..].to_string();
    };
    html[open_end + 1..close_index].to_string()
}

fn scan_tags(html: &str, tag_name: &str) -> Vec<TagMatch> {
    let mut output = Vec::new();
    let mut index = 0usize;

    while index < html.len() {
        let Some(lt) = html[index..].find('<') else {
            break;
        };
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", at + 4) {
                index = end + 3;
            } else {
                index = html.len();
            }
            continue;
        }
        if is_tag_at(html, at, tag_name) {
            let Some(end) = find_tag_end(html, at) else {
                break;
            };
            output.push(TagMatch {
                attrs: parse_attributes(&html[at..=end], tag_name),
            });
            index = end + 1;
            continue;
        }
        index = at + 1;
    }

    output
}

fn html_text(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut index = 0usize;
    while index < html.len() {
        if starts_with_at(html, index, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", index + 4) {
                index = end + 3;
            } else {
                break;
            }
            continue;
        }
        let Some(ch) = html[index..].chars().next() else {
            break;
        };
        if ch == '<' {
            let Some(end) = find_tag_end(html, index) else {
                break;
            };
            if let Some((tag_name, _, _)) = parse_tag_descriptor(&html[index..=end])
                && is_block_like_tag(tag_name)
                && !output.ends_with(' ')
            {
                output.push(' ');
            }
            index = end + 1;
            continue;
        }
        output.push(ch);
        index += ch.len_utf8();
    }
    collapse_whitespace(&decode_html(&output))
}

fn is_block_like_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "div"
            | "p"
            | "li"
            | "tr"
            | "td"
            | "th"
            | "table"
            | "caption"
            | "code"
            | "br"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

fn find_matching_close_tag(html: &str, tag_name: &str, start: usize) -> Option<usize> {
    let mut index = start;
    let mut depth = 1usize;

    while index < html.len() {
        let lt = html[index..].find('<')?;
        let at = index + lt;
        if starts_with_at(html, at, "<!--") {
            if let Some(end) = index_of_ignore_case(html, "-->", at + 4) {
                index = end + 3;
            } else {
                return None;
            }
            continue;
        }
        if is_close_tag_at(html, at, tag_name) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(at);
            }
            index = find_tag_end(html, at)? + 1;
            continue;
        }
        if is_tag_at(html, at, tag_name) {
            let end = find_tag_end(html, at)?;
            if !is_self_closing_tag(&html[at..=end], tag_name) {
                depth += 1;
            }
            index = end + 1;
            continue;
        }
        index = at + 1;
    }

    None
}

fn parse_tag_descriptor(tag_raw: &str) -> Option<(&str, bool, bool)> {
    let bytes = tag_raw.as_bytes();
    if bytes.first().copied() != Some(b'<') {
        return None;
    }

    let mut index = 1usize;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let is_closing = bytes.get(index).copied() == Some(b'/');
    if is_closing {
        index += 1;
    }
    let name_start = index;
    while index < bytes.len() {
        let ch = bytes[index];
        if ch.is_ascii_whitespace() || ch == b'>' || ch == b'/' {
            break;
        }
        index += 1;
    }
    if name_start == index {
        return None;
    }
    let tag_name = &tag_raw[name_start..index];
    Some((tag_name, is_closing, is_self_closing_tag(tag_raw, tag_name)))
}

fn find_tag_start(html: &str, tag_name: &str, start: usize) -> Option<usize> {
    let mut index = start;
    while index < html.len() {
        let lt = html[index..].find('<')?;
        let at = index + lt;
        if is_tag_at(html, at, tag_name) {
            return Some(at);
        }
        index = at + 1;
    }
    None
}

fn is_tag_at(html: &str, at: usize, tag_name: &str) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(at).copied() != Some(b'<') {
        return false;
    }
    let mut index = at + 1;
    if index >= bytes.len() || bytes[index] == b'/' {
        return false;
    }
    for expected in tag_name.as_bytes() {
        let Some(actual) = bytes.get(index) else {
            return false;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return false;
        }
        index += 1;
    }
    matches!(
        bytes.get(index).copied(),
        Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/')
    )
}

fn is_close_tag_at(html: &str, at: usize, tag_name: &str) -> bool {
    let bytes = html.as_bytes();
    if bytes.get(at).copied() != Some(b'<') || bytes.get(at + 1).copied() != Some(b'/') {
        return false;
    }
    let mut index = at + 2;
    for expected in tag_name.as_bytes() {
        let Some(actual) = bytes.get(index) else {
            return false;
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return false;
        }
        index += 1;
    }
    matches!(
        bytes.get(index).copied(),
        Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>')
    )
}

fn find_tag_end(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut index = start;
    let mut quote = None::<u8>;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if byte == b'>' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn parse_attributes(tag_raw: &str, tag_name: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = tag_raw.as_bytes();
    let mut index = tag_name.len() + 1;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'>' {
            break;
        }
        if byte == b'/' || byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }

        let name_start = index;
        while index < bytes.len() {
            let ch = bytes[index];
            if ch.is_ascii_whitespace() || ch == b'=' || ch == b'>' || ch == b'/' {
                break;
            }
            index += 1;
        }
        if name_start == index {
            index += 1;
            continue;
        }
        let name = tag_raw[name_start..index].trim().to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let mut value = String::new();
        if bytes.get(index).copied() == Some(b'=') {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if let Some(quote) = bytes
                .get(index)
                .copied()
                .filter(|byte| *byte == b'"' || *byte == b'\'')
            {
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                value = tag_raw[value_start..index].to_string();
                if bytes.get(index).copied() == Some(quote) {
                    index += 1;
                }
            } else {
                let value_start = index;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                {
                    index += 1;
                }
                value = tag_raw[value_start..index].to_string();
            }
        }

        if !value.is_empty() {
            attrs.insert(name, value);
        } else {
            attrs.entry(name).or_default();
        }
    }

    attrs
}

fn index_of_ignore_case(text: &str, search: &str, start: usize) -> Option<usize> {
    if search.is_empty() {
        return Some(start);
    }
    let text_bytes = text.as_bytes();
    let search_bytes = search.as_bytes();
    if search_bytes.len() > text_bytes.len() || start >= text_bytes.len() {
        return None;
    }

    let last_start = text_bytes.len().saturating_sub(search_bytes.len());
    for index in start..=last_start {
        let mut matched = true;
        for offset in 0..search_bytes.len() {
            if !text_bytes[index + offset].eq_ignore_ascii_case(&search_bytes[offset]) {
                matched = false;
                break;
            }
        }
        if matched {
            return Some(index);
        }
    }
    None
}

fn starts_with_at(text: &str, index: usize, sequence: &str) -> bool {
    index + sequence.len() <= text.len() && &text[index..index + sequence.len()] == sequence
}

fn decode_html(text: &str) -> String {
    let mut value = text.to_string();
    value = value.replace("&amp;", "&");
    value = value.replace("&quot;", "\"");
    value = value.replace("&#39;", "'");
    value = value.replace("&lt;", "<");
    value = value.replace("&gt;", ">");
    value = value.replace("&nbsp;", " ");
    value = value.replace("&ndash;", "–");
    value = value.replace("&mdash;", "—");
    value
}

fn collapse_whitespace(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
                previous_was_space = true;
            }
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }
    output.trim().to_string()
}

fn is_self_closing_tag(tag_raw: &str, tag_name: &str) -> bool {
    let normalized = tag_name.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "br" | "hr" | "img" | "meta" | "link" | "input" | "source"
    ) {
        return true;
    }
    tag_raw.trim_end().ends_with("/>")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        SiteInfoResponse, apply_special_version_info, build_article_url,
        build_manifest_from_siteinfo, derive_wiki_id, load_latest_wiki_capabilities,
        load_wiki_capabilities, parse_special_version_html, store_wiki_capabilities,
    };
    use crate::runtime::{ResolvedPaths, ValueSource};

    fn paths(project_root: &Path) -> ResolvedPaths {
        let state_dir = project_root.join(".wikitool");
        let data_dir = state_dir.join("data");
        ResolvedPaths {
            project_root: project_root.to_path_buf(),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: state_dir.clone(),
            data_dir: data_dir.clone(),
            db_path: data_dir.join("wikitool.db"),
            config_path: state_dir.join("config.toml"),
            parser_config_path: state_dir.join("parser-config.json"),
            root_source: ValueSource::Default,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn builds_manifest_from_siteinfo_payload() {
        let payload = serde_json::from_str::<SiteInfoResponse>(
            r#"{
                "query": {
                    "general": {
                        "sitename": "Remilia Wiki",
                        "articlepath": "/wiki/$1",
                        "generator": "MediaWiki 1.44.1"
                    },
                    "namespaces": {
                        "0": {"id": 0},
                        "10": {"id": 10, "*": "Template", "canonical": "Template"},
                        "3000": {"id": 3000, "*": "Essay", "canonical": "Essay", "content": true}
                    },
                    "extensions": [
                        {"name": "VisualEditor", "version": "1.0.0", "type": "editor"},
                        {"name": "TemplateData", "type": "other"},
                        {"name": "Cargo", "type": "other"},
                        {"name": "CirrusSearch", "type": "search"}
                    ],
                    "specialpagealiases": [
                        {"realname": "Version", "aliases": ["Version"]},
                        {"realname": "CargoTables", "aliases": ["CargoTables"]}
                    ]
                }
            }"#,
        )
        .expect("payload should parse");

        let manifest = build_manifest_from_siteinfo(
            "https://wiki.remilia.org/api.php",
            "https://wiki.remilia.org",
            "/$1",
            &payload.query,
            vec!["gallery".to_string(), "poem".to_string()],
            vec!["cargoquery".to_string(), "invoke".to_string()],
            "1234567890",
        );

        assert_eq!(manifest.schema_version, "wiki_capabilities_v1");
        assert_eq!(manifest.wiki_id, "wiki.remilia.org");
        assert_eq!(manifest.article_path, "/wiki/$1");
        assert_eq!(manifest.mediawiki_version.as_deref(), Some("1.44.1"));
        assert_eq!(manifest.namespaces.len(), 3);
        assert_eq!(manifest.namespaces[0].display_name, "Main");
        assert_eq!(manifest.namespaces[1].display_name, "Template");
        assert_eq!(manifest.namespaces[2].display_name, "Essay");
        assert_eq!(manifest.extensions.len(), 4);
        assert_eq!(
            manifest.search_backend_hint.as_deref(),
            Some("cirrussearch")
        );
        assert!(manifest.has_visual_editor);
        assert!(manifest.has_templatedata);
        assert!(manifest.has_cargo);
        assert!(!manifest.supports_rest_html);
        assert_eq!(manifest.special_pages.len(), 2);
        assert_eq!(manifest.parser_extension_tags, vec!["gallery", "poem"]);
        assert_eq!(manifest.parser_function_hooks, vec!["cargoquery", "invoke"]);
    }

    #[test]
    fn stores_and_loads_manifest_roundtrip() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        fs::create_dir_all(&paths.data_dir).expect("data dir should exist");

        let manifest = super::WikiCapabilityManifest {
            schema_version: "wiki_capabilities_v1".to_string(),
            wiki_id: "wiki.remilia.org".to_string(),
            wiki_url: "https://wiki.remilia.org".to_string(),
            api_url: "https://wiki.remilia.org/api.php".to_string(),
            rest_url: Some("https://wiki.remilia.org/rest.php".to_string()),
            article_path: "/wiki/$1".to_string(),
            mediawiki_version: Some("1.44.1".to_string()),
            namespaces: vec![super::NamespaceInfo {
                id: 0,
                canonical_name: Some("Main".to_string()),
                display_name: "Main".to_string(),
            }],
            extensions: vec![super::ExtensionInfo {
                name: "VisualEditor".to_string(),
                version: Some("1.0.0".to_string()),
                category: Some("editor".to_string()),
            }],
            parser_extension_tags: vec!["gallery".to_string()],
            parser_function_hooks: vec!["invoke".to_string()],
            special_pages: vec!["Version".to_string()],
            search_backend_hint: Some("cirrussearch".to_string()),
            has_visual_editor: true,
            has_templatedata: false,
            has_citoid: false,
            has_cargo: false,
            has_page_forms: false,
            has_short_description: false,
            has_scribunto: false,
            has_timed_media_handler: false,
            supports_parse_api_html: true,
            supports_rest_html: false,
            rest_html_path_template: None,
            refreshed_at: "1234567890".to_string(),
        };

        store_wiki_capabilities(&paths, &manifest).expect("manifest should store");

        let loaded = load_wiki_capabilities(&paths, "wiki.remilia.org")
            .expect("manifest should load")
            .expect("manifest should exist");
        assert_eq!(loaded, manifest);

        let latest = load_latest_wiki_capabilities(&paths)
            .expect("latest manifest should load")
            .expect("latest manifest should exist");
        assert_eq!(latest, manifest);
    }

    #[test]
    fn derives_wiki_id_from_url_path_when_present() {
        assert_eq!(
            derive_wiki_id("https://example.org/farm/remilia"),
            "example.org-farm-remilia"
        );
    }

    #[test]
    fn parses_special_version_html_tables_and_sections() {
        let html = r#"
            <html>
              <head>
                <meta name="generator" content="MediaWiki 1.44.3" />
              </head>
              <body>
                <h2 id="mw-version-software">Installed software</h2>
                <table id="sv-software">
                  <tr><th>Product</th><th>Version</th></tr>
                  <tr><td>MediaWiki</td><td>1.44.3</td></tr>
                </table>
                <h2 id="mw-version-entrypoints">Entry points</h2>
                <table id="mw-version-entrypoints-table">
                  <tr><td>Article path</td><td><code><a href="https://wiki.remilia.org/$1">/$1</a></code></td></tr>
                  <tr><td>rest.php</td><td><code><a href="https://wiki.remilia.org/rest.php">/rest.php</a></code></td></tr>
                </table>
                <h2 id="mw-version-ext">Installed extensions</h2>
                <table class="wikitable plainlinks mw-installed-software">
                  <caption>Editors</caption>
                  <tr class="mw-version-ext" id="mw-version-ext-editor-VisualEditor">
                    <td><a class="mw-version-ext-name external" href="https://www.mediawiki.org/wiki/Extension:VisualEditor">VisualEditor</a></td>
                    <td><span class="mw-version-ext-version">1.0.0</span></td>
                  </tr>
                </table>
                <table class="wikitable plainlinks mw-installed-software">
                  <caption>Special pages</caption>
                  <tr class="mw-version-ext" id="mw-version-ext-specialpage-PageForms">
                    <td><a class="mw-version-ext-name external" href="https://www.mediawiki.org/wiki/Extension:Page_Forms">Page Forms</a></td>
                    <td><span class="mw-version-ext-version">6.0</span></td>
                  </tr>
                </table>
                <h2 id="mw-version-libraries">Installed libraries</h2>
                <h2 id="mw-version-parser-extensiontags">Parser extension tags</h2>
                <bdi><code>&lt;gallery&gt;</code></bdi>, <bdi><code>&lt;math&gt;</code></bdi>
                <h2 id="mw-version-parser-function-hooks">Parser function hooks</h2>
                <bdi><code>{{#cargo_query}}</code></bdi>, <bdi><code>{{PAGENAME}}</code></bdi>
                <h2 id="mw-version-parsoid-modules">Parsoid extension modules</h2>
              </body>
            </html>
        "#;

        let parsed = parse_special_version_html(html, "https://wiki.remilia.org");

        assert_eq!(parsed.mediawiki_version.as_deref(), Some("1.44.3"));
        assert_eq!(parsed.article_path.as_deref(), Some("/$1"));
        assert_eq!(
            parsed.rest_url.as_deref(),
            Some("https://wiki.remilia.org/rest.php")
        );
        assert_eq!(parsed.extensions.len(), 2);
        assert_eq!(parsed.extensions[0].name, "Page Forms");
        assert_eq!(
            parsed.extensions[0].category.as_deref(),
            Some("Special pages")
        );
        assert_eq!(parsed.extensions[1].name, "VisualEditor");
        assert_eq!(parsed.extensions[1].version.as_deref(), Some("1.0.0"));
        assert_eq!(parsed.parser_extension_tags, vec!["gallery", "math"]);
        assert_eq!(
            parsed.parser_function_hooks,
            vec!["#cargo_query", "PAGENAME"]
        );
    }

    #[test]
    fn special_version_enrichment_enables_rest_and_merges_extension_metadata() {
        let payload = serde_json::from_str::<SiteInfoResponse>(
            r#"{
                "query": {
                    "general": {
                        "articlepath": "/$1",
                        "generator": "MediaWiki 1.44.1"
                    },
                    "namespaces": {
                        "0": {"id": 0}
                    },
                    "extensions": [
                        {"name": "VisualEditor", "type": "editor"},
                        {"name": "CirrusSearch", "type": "search"}
                    ],
                    "specialpagealiases": []
                }
            }"#,
        )
        .expect("payload should parse");

        let mut manifest = build_manifest_from_siteinfo(
            "https://wiki.remilia.org/api.php",
            "https://wiki.remilia.org",
            "/$1",
            &payload.query,
            Vec::new(),
            Vec::new(),
            "1234567890",
        );

        let special_html = r#"
            <h2 id="mw-version-software">Installed software</h2>
            <table id="sv-software">
              <tr><td>MediaWiki</td><td>1.44.3</td></tr>
            </table>
            <h2 id="mw-version-entrypoints">Entry points</h2>
            <table id="mw-version-entrypoints-table">
              <tr><td>Article path</td><td><code><a href="https://wiki.remilia.org/$1">/$1</a></code></td></tr>
              <tr><td>rest.php</td><td><code><a href="https://wiki.remilia.org/rest.php">/rest.php</a></code></td></tr>
            </table>
            <h2 id="mw-version-ext">Installed extensions</h2>
            <table class="wikitable plainlinks mw-installed-software">
              <caption>Editors</caption>
              <tr class="mw-version-ext" id="mw-version-ext-editor-VisualEditor">
                <td><a class="mw-version-ext-name external" href="https://www.mediawiki.org/wiki/Extension:VisualEditor">VisualEditor</a></td>
                <td><span class="mw-version-ext-version">1.0.0</span></td>
              </tr>
            </table>
            <h2 id="mw-version-libraries">Installed libraries</h2>
            <h2 id="mw-version-parser-extensiontags">Parser extension tags</h2>
            <bdi><code>&lt;gallery&gt;</code></bdi>
            <h2 id="mw-version-parser-function-hooks">Parser function hooks</h2>
            <bdi><code>{{#cargo_query}}</code></bdi>
            <h2 id="mw-version-parsoid-modules">Parsoid extension modules</h2>
        "#;

        let parsed = parse_special_version_html(special_html, "https://wiki.remilia.org");
        apply_special_version_info(&mut manifest, parsed);

        assert_eq!(manifest.mediawiki_version.as_deref(), Some("1.44.3"));
        assert!(manifest.supports_rest_html);
        assert_eq!(
            manifest.rest_html_path_template.as_deref(),
            Some("https://wiki.remilia.org/rest.php/v1/page/$TITLE/html")
        );
        assert_eq!(manifest.article_path, "/$1");
        let visual_editor = manifest
            .extensions
            .iter()
            .find(|extension| extension.name == "VisualEditor")
            .expect("visual editor should exist");
        assert_eq!(visual_editor.version.as_deref(), Some("1.0.0"));
        assert_eq!(visual_editor.category.as_deref(), Some("Editors"));
    }

    #[test]
    fn build_article_url_preserves_special_titles_with_colons() {
        let url = build_article_url("https://wiki.remilia.org", "/$1", "Special:Version")
            .expect("article url should build");
        assert_eq!(url, "https://wiki.remilia.org/Special:Version");
    }
}
