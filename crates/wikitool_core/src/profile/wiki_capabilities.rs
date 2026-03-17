use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
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
    let manifest = build_manifest_from_siteinfo(
        &api_url,
        &wiki_url,
        &config.article_path_owned(),
        &siteinfo,
        parser_extension_tags,
        parser_function_hooks,
        &refreshed_at,
    );
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

    manifest.has_visual_editor = manifest.supports_extension("VisualEditor");
    manifest.has_templatedata = manifest.supports_extension("TemplateData");
    manifest.has_citoid = manifest.supports_extension("Citoid");
    manifest.has_cargo = manifest.supports_extension("Cargo");
    manifest.has_page_forms = manifest.supports_extension("Page Forms");
    manifest.has_short_description = manifest.supports_extension("ShortDescription");
    manifest.has_scribunto = manifest.supports_extension("Scribunto");
    manifest.has_timed_media_handler = manifest.supports_extension("TimedMediaHandler");
    manifest
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        SiteInfoResponse, build_manifest_from_siteinfo, derive_wiki_id,
        load_latest_wiki_capabilities, load_wiki_capabilities, store_wiki_capabilities,
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
}
