use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::config::WikiConfig;
use crate::knowledge::status::record_docs_profile_artifact;
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{compute_hash, unix_timestamp};

mod catalog;
mod fetch;
mod import;
mod parse;
mod persist;
mod profiles;
mod query;

pub use fetch::{
    DocsApi, DocsClientConfig, MediaWikiDocsClient, RemoteDocsPage,
    discover_installed_extensions_from_wiki, discover_installed_extensions_from_wiki_with_config,
};
pub use query::{build_docs_context, lookup_docs_symbols, search_docs};

use catalog::{load_docs_corpora, load_docs_stats, load_outdated_docs, load_outdated_refresh_rows};
use import::{import_extension_docs_with_api_internal, import_technical_docs_with_api_internal};
use parse::{
    DocsPageParseInput, ParsedDocsExample, ParsedDocsLink, ParsedDocsSection, ParsedDocsSymbol,
    estimate_tokens, is_translation_variant, normalize_retrieval_key, normalize_title,
    parse_docs_page,
};
use persist::{accumulate_stats, persist_docs_corpus};
use profiles::{collect_profile_pages, resolve_docs_profile};

const DOCS_NAMESPACE_HELP: i32 = 12;
const DOCS_NAMESPACE_MANUAL: i32 = 100;
const DOCS_NAMESPACE_EXTENSION: i32 = 102;
const DOCS_NAMESPACE_API: i32 = 104;
const DOCS_CACHE_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
const DOCS_SUBPAGE_LIMIT_DEFAULT: usize = 100;
const DOCS_BUNDLE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TechnicalDocType {
    Hooks,
    Config,
    Api,
    Manual,
    Help,
}

impl TechnicalDocType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hooks => "hooks",
            Self::Config => "config",
            Self::Api => "api",
            Self::Manual => "manual",
            Self::Help => "help",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("hooks") {
            return Some(Self::Hooks);
        }
        if value.eq_ignore_ascii_case("config") {
            return Some(Self::Config);
        }
        if value.eq_ignore_ascii_case("api") {
            return Some(Self::Api);
        }
        if value.eq_ignore_ascii_case("manual") {
            return Some(Self::Manual);
        }
        if value.eq_ignore_ascii_case("help") {
            return Some(Self::Help);
        }
        None
    }

    fn main_page(self) -> &'static str {
        match self {
            Self::Hooks => "Manual:Hooks",
            Self::Config => "Manual:Configuration settings",
            Self::Api => "API:Main page",
            Self::Manual => "Manual:Contents",
            Self::Help => "Help:Contents",
        }
    }

    fn subpage_prefix(self) -> &'static str {
        match self {
            Self::Hooks => "Manual:Hooks/",
            Self::Config => "Manual:$wg",
            Self::Api => "API:",
            Self::Manual => "Manual:",
            Self::Help => "Help:",
        }
    }

    fn namespace(self) -> i32 {
        match self {
            Self::Hooks | Self::Config | Self::Manual => DOCS_NAMESPACE_MANUAL,
            Self::Api => DOCS_NAMESPACE_API,
            Self::Help => DOCS_NAMESPACE_HELP,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DocsImportOptions {
    pub extensions: Vec<String>,
    pub include_subpages: bool,
}

impl Default for DocsImportOptions {
    fn default() -> Self {
        Self {
            extensions: Vec::new(),
            include_subpages: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsImportReport {
    pub requested_extensions: usize,
    pub imported_extensions: usize,
    pub imported_pages: usize,
    pub imported_sections: usize,
    pub imported_symbols: usize,
    pub imported_examples: usize,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone)]
pub struct TechnicalImportTask {
    pub doc_type: TechnicalDocType,
    pub page_title: Option<String>,
    pub include_subpages: bool,
}

#[derive(Debug, Clone)]
pub struct DocsImportTechnicalOptions {
    pub tasks: Vec<TechnicalImportTask>,
    pub limit: usize,
}

impl Default for DocsImportTechnicalOptions {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            limit: DOCS_SUBPAGE_LIMIT_DEFAULT,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsImportTechnicalReport {
    pub requested_tasks: usize,
    pub imported_corpora: usize,
    pub imported_pages: usize,
    pub imported_sections: usize,
    pub imported_symbols: usize,
    pub imported_examples: usize,
    pub imported_by_type: BTreeMap<String, usize>,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone)]
pub struct DocsImportProfileOptions {
    pub profile: String,
    pub include_installed_extensions: bool,
    pub include_extension_subpages: bool,
    pub extra_extensions: Vec<String>,
    pub limit: usize,
}

impl Default for DocsImportProfileOptions {
    fn default() -> Self {
        Self {
            profile: "remilia-mw-1.44".to_string(),
            include_installed_extensions: false,
            include_extension_subpages: true,
            extra_extensions: Vec::new(),
            limit: DOCS_SUBPAGE_LIMIT_DEFAULT,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsImportProfileReport {
    pub profile: String,
    pub imported_corpora: usize,
    pub imported_extensions: usize,
    pub imported_pages: usize,
    pub imported_sections: usize,
    pub imported_symbols: usize,
    pub imported_examples: usize,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsStats {
    pub corpora_count: usize,
    pub pages_count: usize,
    pub sections_count: usize,
    pub symbols_count: usize,
    pub examples_count: usize,
    pub corpora_by_kind: BTreeMap<String, usize>,
    pub technical_by_type: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsCorpusSummary {
    pub corpus_id: String,
    pub corpus_kind: String,
    pub label: String,
    pub source_wiki: String,
    pub source_version: String,
    pub source_profile: String,
    pub technical_type: String,
    pub pages_count: usize,
    pub sections_count: usize,
    pub symbols_count: usize,
    pub examples_count: usize,
    pub fetched_at_unix: u64,
    pub expires_at_unix: u64,
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsOutdatedCorpus {
    pub corpus_id: String,
    pub corpus_kind: String,
    pub label: String,
    pub source_profile: String,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DocsOutdatedReport {
    pub corpora: Vec<DocsOutdatedCorpus>,
}

#[derive(Debug, Clone, Default)]
pub struct DocsListOptions {
    pub technical_type: Option<String>,
    pub corpus_kind: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsListReport {
    pub now_unix: u64,
    pub stats: DocsStats,
    pub corpora: Vec<DocsCorpusSummary>,
    pub outdated: DocsOutdatedReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsUpdateReport {
    pub updated_corpora: usize,
    pub updated_pages: usize,
    pub updated_sections: usize,
    pub updated_symbols: usize,
    pub updated_examples: usize,
    pub failures: Vec<String>,
    pub request_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocsRemoveKind {
    Corpus,
    TechnicalType,
    Page,
    NotFound,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsRemoveReport {
    pub kind: DocsRemoveKind,
    pub target: String,
    pub removed_rows: usize,
}

#[derive(Debug, Clone, Default)]
pub struct DocsSearchOptions {
    pub tier: Option<String>,
    pub profile: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocsSearchHit {
    pub tier: String,
    pub title: String,
    pub page_title: String,
    pub corpus_id: String,
    pub corpus_kind: String,
    pub source_profile: String,
    pub section_heading: Option<String>,
    pub retrieval_weight: usize,
    pub snippet: String,
    pub signals: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DocsSymbolLookupOptions {
    pub kind: Option<String>,
    pub profile: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocsSymbolHit {
    pub corpus_id: String,
    pub corpus_kind: String,
    pub source_profile: String,
    pub page_title: String,
    pub symbol_kind: String,
    pub symbol_name: String,
    pub aliases: Vec<String>,
    pub section_heading: Option<String>,
    pub signature_text: String,
    pub summary_text: String,
    pub detail_text: String,
    pub retrieval_weight: usize,
    pub signals: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DocsContextOptions {
    pub profile: Option<String>,
    pub limit: usize,
    pub token_budget: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocsContextSection {
    pub corpus_id: String,
    pub page_title: String,
    pub section_heading: Option<String>,
    pub summary_text: String,
    pub section_text: String,
    pub retrieval_weight: usize,
    pub token_estimate: usize,
    pub signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocsContextExample {
    pub corpus_id: String,
    pub corpus_kind: String,
    pub source_profile: String,
    pub page_title: String,
    pub example_kind: String,
    pub section_heading: Option<String>,
    pub language_hint: String,
    pub summary_text: String,
    pub example_text: String,
    pub retrieval_weight: usize,
    pub token_estimate: usize,
    pub signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocsContextReport {
    pub query: String,
    pub profile: Option<String>,
    pub pages: Vec<DocsSearchHit>,
    pub sections: Vec<DocsContextSection>,
    pub symbols: Vec<DocsSymbolHit>,
    pub examples: Vec<DocsContextExample>,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocsBundle {
    pub schema_version: u32,
    pub generated_at_unix: Option<u64>,
    pub source: Option<String>,
    #[serde(default)]
    pub extensions: Vec<DocsBundleExtension>,
    #[serde(default)]
    pub technical: Vec<DocsBundleTechnical>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocsBundleExtension {
    pub extension_name: String,
    pub source_wiki: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub pages: Vec<DocsBundlePage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocsBundleTechnical {
    pub doc_type: String,
    #[serde(default)]
    pub pages: Vec<DocsBundlePage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocsBundlePage {
    pub page_title: String,
    pub content: String,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocsBundleImportReport {
    pub schema_version: u32,
    pub source: String,
    pub imported_extensions: usize,
    pub imported_technical_types: usize,
    pub imported_pages: usize,
    pub imported_sections: usize,
    pub imported_symbols: usize,
    pub imported_examples: usize,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone)]
struct FetchedDocsPage {
    page_title: String,
    alias_titles: Vec<String>,
    local_path: String,
    content: String,
}

#[derive(Debug, Clone)]
struct CorpusDescriptor {
    corpus_id: String,
    corpus_kind: String,
    label: String,
    source_wiki: String,
    source_version: String,
    source_profile: String,
    technical_type: String,
    refresh_kind: String,
    refresh_spec: String,
    fetched_at_unix: u64,
    expires_at_unix: u64,
}

#[derive(Debug, Clone, Default)]
struct PersistStats {
    pages: usize,
    sections: usize,
    symbols: usize,
    examples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtensionRefreshSpec {
    extension_name: String,
    include_subpages: bool,
    source_profile: String,
    source_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TechnicalRefreshSpec {
    doc_type: String,
    page_title: Option<String>,
    include_subpages: bool,
    limit: usize,
    source_profile: String,
    source_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfileRefreshSpec {
    profile: String,
    include_installed_extensions: bool,
    include_extension_subpages: bool,
    extra_extensions: Vec<String>,
    limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StaticRefreshSpec {
    source: String,
}

struct DocsProfileDefinition {
    id: &'static str,
    label: &'static str,
    source_version: &'static str,
    include_installed_extensions_by_default: bool,
    page_seeds: &'static [ProfilePageSeed],
    extension_seeds: &'static [&'static str],
}

struct ProfilePageSeed {
    title: &'static str,
    include_subpages: bool,
}

const AUTHORING_PAGE_SEEDS: &[ProfilePageSeed] = &[
    ProfilePageSeed {
        title: "Help:Formatting",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Help:Images",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Help:Links",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Help:Categories",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Help:Tables",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Help:Templates",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Help:Magic words",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Help:Tags",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Help:Extension:ParserFunctions",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Manual:Parser functions",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Manual:Template expansion process",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Manual:Tag extensions",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "Manual:Tag extensions/Example",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "API:Parsing wikitext",
        include_subpages: false,
    },
    ProfilePageSeed {
        title: "API:Expandtemplates",
        include_subpages: false,
    },
];

const AUTHORING_EXTENSION_SEEDS: &[&str] = &[
    "Scribunto",
    "ParserFunctions",
    "Cite",
    "TemplateStyles",
    "Cargo",
];

const DOCS_PROFILES: &[DocsProfileDefinition] = &[
    DocsProfileDefinition {
        id: "mw-1.44-authoring",
        label: "MediaWiki 1.44 authoring reference",
        source_version: "1.44",
        include_installed_extensions_by_default: false,
        page_seeds: AUTHORING_PAGE_SEEDS,
        extension_seeds: AUTHORING_EXTENSION_SEEDS,
    },
    DocsProfileDefinition {
        id: "remilia-mw-1.44",
        label: "Remilia MediaWiki 1.44 authoring reference",
        source_version: "1.44",
        include_installed_extensions_by_default: true,
        page_seeds: AUTHORING_PAGE_SEEDS,
        extension_seeds: AUTHORING_EXTENSION_SEEDS,
    },
];

pub fn import_docs_bundle(
    paths: &ResolvedPaths,
    bundle_path: &Path,
) -> Result<DocsBundleImportReport> {
    let bundle_data = fs::read_to_string(bundle_path)
        .with_context(|| format!("failed to read docs bundle {}", bundle_path.display()))?;
    let bundle: DocsBundle =
        serde_json::from_str(&bundle_data).context("failed to parse docs bundle JSON")?;
    if bundle.schema_version != DOCS_BUNDLE_SCHEMA_VERSION {
        bail!(
            "unsupported docs bundle schema version {} (expected {})",
            bundle.schema_version,
            DOCS_BUNDLE_SCHEMA_VERSION
        );
    }

    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let source = bundle
        .source
        .clone()
        .unwrap_or_else(|| "precomposed_bundle".to_string());

    let mut imported_extensions = 0usize;
    let mut imported_technical_types = 0usize;
    let mut stats = PersistStats::default();
    let mut failures = Vec::new();

    for extension in &bundle.extensions {
        let extension_name = normalize_extension_name(&extension.extension_name);
        if extension_name.is_empty() {
            failures.push("bundle extension entry with empty extension_name".to_string());
            continue;
        }
        let mut pages = Vec::new();
        for page in &extension.pages {
            let page_title = normalize_title(&page.page_title);
            if page_title.is_empty() || page.content.trim().is_empty() {
                continue;
            }
            let local_path = page
                .local_path
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| extension_local_path(&extension_name, &page_title));
            pages.push(FetchedDocsPage {
                page_title,
                alias_titles: Vec::new(),
                local_path,
                content: page.content.clone(),
            });
        }
        if pages.is_empty() {
            failures.push(format!(
                "{extension_name}: bundle entry has no usable pages"
            ));
            continue;
        }
        let descriptor = CorpusDescriptor {
            corpus_id: extension_corpus_id(&extension_name, ""),
            corpus_kind: "extension".to_string(),
            label: format!("Extension:{extension_name}"),
            source_wiki: extension
                .source_wiki
                .clone()
                .unwrap_or_else(|| source.clone()),
            source_version: extension.version.clone().unwrap_or_default(),
            source_profile: String::new(),
            technical_type: String::new(),
            refresh_kind: "static".to_string(),
            refresh_spec: serde_json::to_string(&StaticRefreshSpec {
                source: source.clone(),
            })?,
            fetched_at_unix: now_unix,
            expires_at_unix,
        };
        let persisted = persist_docs_corpus(paths, &descriptor, &pages)?;
        imported_extensions += 1;
        accumulate_stats(&mut stats, &persisted);
    }

    for technical in &bundle.technical {
        let Some(doc_type) = TechnicalDocType::parse(&technical.doc_type) else {
            failures.push(format!(
                "bundle technical entry has unsupported doc_type `{}`",
                technical.doc_type
            ));
            continue;
        };
        let mut pages = Vec::new();
        for page in &technical.pages {
            let page_title = normalize_title(&page.page_title);
            if page_title.is_empty() || page.content.trim().is_empty() {
                continue;
            }
            let local_path = page
                .local_path
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| technical_local_path(doc_type, &page_title));
            pages.push(FetchedDocsPage {
                page_title,
                alias_titles: Vec::new(),
                local_path,
                content: page.content.clone(),
            });
        }
        if pages.is_empty() {
            continue;
        }
        let descriptor = CorpusDescriptor {
            corpus_id: technical_corpus_id(doc_type, None, ""),
            corpus_kind: "technical".to_string(),
            label: doc_type.main_page().to_string(),
            source_wiki: source.clone(),
            source_version: String::new(),
            source_profile: String::new(),
            technical_type: doc_type.as_str().to_string(),
            refresh_kind: "static".to_string(),
            refresh_spec: serde_json::to_string(&StaticRefreshSpec {
                source: source.clone(),
            })?,
            fetched_at_unix: now_unix,
            expires_at_unix,
        };
        let persisted = persist_docs_corpus(paths, &descriptor, &pages)?;
        imported_technical_types += 1;
        accumulate_stats(&mut stats, &persisted);
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsBundleImportReport {
        schema_version: bundle.schema_version,
        source,
        imported_extensions,
        imported_technical_types,
        imported_pages: stats.pages,
        imported_sections: stats.sections,
        imported_symbols: stats.symbols,
        imported_examples: stats.examples,
        failures,
    })
}

pub fn import_extension_docs(
    paths: &ResolvedPaths,
    options: &DocsImportOptions,
) -> Result<DocsImportReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    import_extension_docs_with_api(paths, options, &mut api)
}

pub fn import_extension_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportOptions,
    api: &mut A,
) -> Result<DocsImportReport> {
    import_extension_docs_with_api_internal(paths, options, api, "", "")
}

pub fn import_technical_docs(
    paths: &ResolvedPaths,
    options: &DocsImportTechnicalOptions,
) -> Result<DocsImportTechnicalReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    import_technical_docs_with_api(paths, options, &mut api)
}

pub fn import_technical_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportTechnicalOptions,
    api: &mut A,
) -> Result<DocsImportTechnicalReport> {
    import_technical_docs_with_api_internal(paths, options, api, "", "")
}

pub fn import_docs_profile(
    paths: &ResolvedPaths,
    options: &DocsImportProfileOptions,
) -> Result<DocsImportProfileReport> {
    import_docs_profile_with_config(paths, options, &WikiConfig::default())
}

pub fn import_docs_profile_with_config(
    paths: &ResolvedPaths,
    options: &DocsImportProfileOptions,
    config: &WikiConfig,
) -> Result<DocsImportProfileReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    import_docs_profile_with_api(paths, options, config, &mut api)
}

pub fn import_docs_profile_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportProfileOptions,
    config: &WikiConfig,
    api: &mut A,
) -> Result<DocsImportProfileReport> {
    let definition = resolve_docs_profile(&options.profile)?;
    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let include_installed_extensions =
        options.include_installed_extensions || definition.include_installed_extensions_by_default;

    let profile_pages = collect_profile_pages(api, definition, options.limit)?;
    let descriptor = CorpusDescriptor {
        corpus_id: profile_corpus_id(definition.id),
        corpus_kind: "profile".to_string(),
        label: definition.label.to_string(),
        source_wiki: "mediawiki.org".to_string(),
        source_version: definition.source_version.to_string(),
        source_profile: definition.id.to_string(),
        technical_type: "profile".to_string(),
        refresh_kind: "profile".to_string(),
        refresh_spec: serde_json::to_string(&ProfileRefreshSpec {
            profile: definition.id.to_string(),
            include_installed_extensions,
            include_extension_subpages: options.include_extension_subpages,
            extra_extensions: options.extra_extensions.clone(),
            limit: options.limit.max(1),
        })?,
        fetched_at_unix: now_unix,
        expires_at_unix,
    };
    let mut stats = persist_docs_corpus(paths, &descriptor, &profile_pages)?;
    let mut failures = Vec::new();
    let mut imported_extensions = 0usize;

    let mut extension_names = definition
        .extension_seeds
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    extension_names.extend(options.extra_extensions.clone());
    if include_installed_extensions {
        match discover_installed_extensions_from_wiki_with_config(config) {
            Ok(discovered) => extension_names.extend(discovered),
            Err(error) => failures.push(format!(
                "profile {}: installed extension discovery skipped: {error}",
                definition.id
            )),
        }
    }
    normalize_extension_list(&mut extension_names);

    if !extension_names.is_empty() {
        let extension_report = import_extension_docs_with_api_internal(
            paths,
            &DocsImportOptions {
                extensions: extension_names,
                include_subpages: options.include_extension_subpages,
            },
            api,
            definition.id,
            definition.source_version,
        )?;
        imported_extensions = extension_report.imported_extensions;
        stats.pages += extension_report.imported_pages;
        stats.sections += extension_report.imported_sections;
        stats.symbols += extension_report.imported_symbols;
        stats.examples += extension_report.imported_examples;
        failures.extend(extension_report.failures);
    }

    rebuild_docs_fts_indexes(paths)?;
    let connection = open_initialized_database_connection(&paths.db_path)?;
    record_docs_profile_artifact(
        &connection,
        definition.id,
        stats.pages,
        &serde_json::json!({
            "profile": definition.id,
            "imported_corpora": 1 + imported_extensions,
            "imported_extensions": imported_extensions,
            "imported_pages": stats.pages,
            "imported_sections": stats.sections,
            "imported_symbols": stats.symbols,
            "imported_examples": stats.examples,
            "failures": failures.clone(),
        })
        .to_string(),
    )?;

    Ok(DocsImportProfileReport {
        profile: definition.id.to_string(),
        imported_corpora: 1 + imported_extensions,
        imported_extensions,
        imported_pages: stats.pages,
        imported_sections: stats.sections,
        imported_symbols: stats.symbols,
        imported_examples: stats.examples,
        failures,
        request_count: api.request_count(),
    })
}

pub fn list_docs(paths: &ResolvedPaths, options: &DocsListOptions) -> Result<DocsListReport> {
    let connection = open_docs_connection(paths)?;
    let now_unix = unix_timestamp()?;
    let stats = load_docs_stats(&connection)?;
    let corpora = load_docs_corpora(
        &connection,
        options.corpus_kind.as_deref(),
        options.technical_type.as_deref(),
        options.profile.as_deref(),
        now_unix,
    )?;
    let outdated = load_outdated_docs(&connection, now_unix)?;

    Ok(DocsListReport {
        now_unix,
        stats,
        corpora,
        outdated,
    })
}

pub fn update_outdated_docs(paths: &ResolvedPaths) -> Result<DocsUpdateReport> {
    update_outdated_docs_with_config(paths, &WikiConfig::default())
}

pub fn update_outdated_docs_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<DocsUpdateReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    update_outdated_docs_with_api(paths, config, &mut api)
}

pub fn update_outdated_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    config: &WikiConfig,
    api: &mut A,
) -> Result<DocsUpdateReport> {
    let refresh_rows = load_outdated_refresh_rows(paths)?;
    let mut updated_corpora = 0usize;
    let mut updated_pages = 0usize;
    let mut updated_sections = 0usize;
    let mut updated_symbols = 0usize;
    let mut updated_examples = 0usize;
    let mut failures = Vec::new();

    for row in refresh_rows {
        match row.refresh_kind.as_str() {
            "extension" => {
                let spec: ExtensionRefreshSpec = serde_json::from_str(&row.refresh_spec)
                    .context("invalid extension refresh spec")?;
                match import_extension_docs_with_api_internal(
                    paths,
                    &DocsImportOptions {
                        extensions: vec![spec.extension_name.clone()],
                        include_subpages: spec.include_subpages,
                    },
                    api,
                    &spec.source_profile,
                    &spec.source_version,
                ) {
                    Ok(report) => {
                        if report.imported_extensions > 0 {
                            updated_corpora += report.imported_extensions;
                            updated_pages += report.imported_pages;
                            updated_sections += report.imported_sections;
                            updated_symbols += report.imported_symbols;
                            updated_examples += report.imported_examples;
                        }
                        failures.extend(report.failures);
                    }
                    Err(error) => failures.push(format!("{}: {error}", row.label)),
                }
            }
            "technical" => {
                let spec: TechnicalRefreshSpec = serde_json::from_str(&row.refresh_spec)
                    .context("invalid technical refresh spec")?;
                let Some(doc_type) = TechnicalDocType::parse(&spec.doc_type) else {
                    failures.push(format!(
                        "{}: unsupported technical doc type {}",
                        row.label, spec.doc_type
                    ));
                    continue;
                };
                match import_technical_docs_with_api_internal(
                    paths,
                    &DocsImportTechnicalOptions {
                        tasks: vec![TechnicalImportTask {
                            doc_type,
                            page_title: spec.page_title.clone(),
                            include_subpages: spec.include_subpages,
                        }],
                        limit: spec.limit.max(1),
                    },
                    api,
                    &spec.source_profile,
                    &spec.source_version,
                ) {
                    Ok(report) => {
                        if report.imported_corpora > 0 {
                            updated_corpora += report.imported_corpora;
                            updated_pages += report.imported_pages;
                            updated_sections += report.imported_sections;
                            updated_symbols += report.imported_symbols;
                            updated_examples += report.imported_examples;
                        }
                        failures.extend(report.failures);
                    }
                    Err(error) => failures.push(format!("{}: {error}", row.label)),
                }
            }
            "profile" => {
                let spec: ProfileRefreshSpec = serde_json::from_str(&row.refresh_spec)
                    .context("invalid profile refresh spec")?;
                match import_docs_profile_with_api(
                    paths,
                    &DocsImportProfileOptions {
                        profile: spec.profile.clone(),
                        include_installed_extensions: spec.include_installed_extensions,
                        include_extension_subpages: spec.include_extension_subpages,
                        extra_extensions: spec.extra_extensions.clone(),
                        limit: spec.limit.max(1),
                    },
                    config,
                    api,
                ) {
                    Ok(report) => {
                        updated_corpora += report.imported_corpora;
                        updated_pages += report.imported_pages;
                        updated_sections += report.imported_sections;
                        updated_symbols += report.imported_symbols;
                        updated_examples += report.imported_examples;
                        failures.extend(report.failures);
                    }
                    Err(error) => failures.push(format!("{}: {error}", row.label)),
                }
            }
            _ => {}
        }
    }

    Ok(DocsUpdateReport {
        updated_corpora,
        updated_pages,
        updated_sections,
        updated_symbols,
        updated_examples,
        failures,
        request_count: api.request_count(),
    })
}

pub fn remove_docs(paths: &ResolvedPaths, target: &str) -> Result<DocsRemoveReport> {
    let connection = open_docs_connection(paths)?;
    let normalized_target = normalize_title(target);
    if normalized_target.is_empty() {
        bail!("docs remove target is empty");
    }

    if let Some(doc_type) = TechnicalDocType::parse(&normalized_target) {
        let removed = connection.execute(
            "DELETE FROM docs_corpora WHERE corpus_kind = 'technical' AND technical_type = ?1",
            params![doc_type.as_str()],
        )?;
        if removed > 0 {
            rebuild_docs_fts_indexes(paths)?;
            return Ok(DocsRemoveReport {
                kind: DocsRemoveKind::TechnicalType,
                target: normalized_target,
                removed_rows: removed,
            });
        }
    }

    let removed = connection.execute(
        "DELETE FROM docs_corpora
         WHERE lower(corpus_id) = lower(?1)
            OR lower(label) = lower(?1)
            OR lower(replace(label, 'Extension:', '')) = lower(?1)",
        params![normalized_target],
    )?;
    if removed > 0 {
        rebuild_docs_fts_indexes(paths)?;
        return Ok(DocsRemoveReport {
            kind: DocsRemoveKind::Corpus,
            target: normalized_target,
            removed_rows: removed,
        });
    }

    let removed_pages = connection.execute(
        "DELETE FROM docs_pages WHERE lower(page_title) = lower(?1)",
        params![normalized_target],
    )?;
    if removed_pages > 0 {
        rebuild_docs_fts_indexes(paths)?;
        cleanup_empty_corpora(&connection)?;
        return Ok(DocsRemoveReport {
            kind: DocsRemoveKind::Page,
            target: normalized_target,
            removed_rows: removed_pages,
        });
    }

    Ok(DocsRemoveReport {
        kind: DocsRemoveKind::NotFound,
        target: normalized_target,
        removed_rows: 0,
    })
}

pub fn format_expiration(now_unix: u64, expires_at_unix: u64) -> String {
    if expires_at_unix <= now_unix {
        return "expired".to_string();
    }
    let delta = expires_at_unix - now_unix;
    let day = 24 * 60 * 60;
    let hour = 60 * 60;
    if delta >= day {
        let days = delta / day;
        return format!("expires in {days} day{}", if days == 1 { "" } else { "s" });
    }
    if delta >= hour {
        let hours = delta / hour;
        return format!(
            "expires in {hours} hour{}",
            if hours == 1 { "" } else { "s" }
        );
    }
    "expires soon".to_string()
}

#[derive(Debug, Clone)]
struct OutdatedRefreshRow {
    label: String,
    refresh_kind: String,
    refresh_spec: String,
}

fn open_docs_connection(paths: &ResolvedPaths) -> Result<Connection> {
    open_initialized_database_connection(&paths.db_path)
}

fn rebuild_docs_fts_indexes(paths: &ResolvedPaths) -> Result<()> {
    let connection = open_docs_connection(paths)?;
    for table_name in [
        "docs_pages_fts",
        "docs_sections_fts",
        "docs_symbols_fts",
        "docs_examples_fts",
    ] {
        connection.execute_batch(&format!(
            "INSERT INTO {table_name}({table_name}) VALUES('rebuild')"
        ))?;
    }
    Ok(())
}

fn cleanup_empty_corpora(connection: &Connection) -> Result<()> {
    connection.execute(
        "DELETE FROM docs_corpora
         WHERE NOT EXISTS (
             SELECT 1 FROM docs_pages WHERE docs_pages.corpus_id = docs_corpora.corpus_id
         )",
        [],
    )?;
    Ok(())
}

fn count_query(connection: &Connection, sql: &str) -> Result<usize> {
    let value: i64 = connection.query_row(sql, [], |row| row.get(0))?;
    Ok(usize::try_from(value).unwrap_or(0))
}

fn extension_corpus_id(extension_name: &str, source_profile: &str) -> String {
    if source_profile.is_empty() {
        return format!("extension:{}", sanitize_id(extension_name));
    }
    format!(
        "extension:{}:{}",
        sanitize_id(source_profile),
        sanitize_id(extension_name)
    )
}

fn technical_corpus_id(
    doc_type: TechnicalDocType,
    page_title: Option<&str>,
    source_profile: &str,
) -> String {
    let scope = page_title.unwrap_or(doc_type.main_page());
    if source_profile.is_empty() {
        return format!("technical:{}:{}", doc_type.as_str(), sanitize_id(scope));
    }
    format!(
        "technical:{}:{}:{}",
        sanitize_id(source_profile),
        doc_type.as_str(),
        sanitize_id(scope)
    )
}

fn profile_corpus_id(profile: &str) -> String {
    format!("profile:{}", sanitize_id(profile))
}

fn normalize_corpus_kind_filter(value: Option<&str>) -> String {
    value.unwrap_or_default().trim().to_ascii_lowercase()
}

fn infer_doc_type_from_title(title: &str) -> TechnicalDocType {
    if title.starts_with("Manual:Hooks") {
        return TechnicalDocType::Hooks;
    }
    if title.starts_with("Manual:$wg") {
        return TechnicalDocType::Config;
    }
    if title.starts_with("API:") {
        return TechnicalDocType::Api;
    }
    if title.starts_with("Help:") {
        return TechnicalDocType::Help;
    }
    TechnicalDocType::Manual
}

fn normalize_extensions(extensions: &[String]) -> Vec<String> {
    let mut out = extensions
        .iter()
        .map(|value| normalize_extension_name(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalize_extension_list(&mut out);
    out
}

fn normalize_extension_list(extensions: &mut Vec<String>) {
    extensions.sort_unstable_by_key(|value| value.to_ascii_lowercase());
    extensions.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
}

fn filter_translation_titles(titles: &mut Vec<String>) {
    titles.retain(|title| !is_translation_variant(title));
}

fn normalize_extension_name(value: &str) -> String {
    normalize_title(value.trim().trim_start_matches("Extension:"))
}

fn dedupe_titles_in_order(titles: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    titles.retain(|title| seen.insert(title.to_ascii_lowercase()));
}

fn extension_local_path(extension: &str, title: &str) -> String {
    format!(
        "docs/extensions/{}/{}.wiki",
        sanitize_path_segment(extension),
        sanitize_title_for_filename(title),
    )
}

fn technical_local_path(doc_type: TechnicalDocType, title: &str) -> String {
    format!(
        "docs/technical/{}/{}.wiki",
        doc_type.as_str(),
        sanitize_title_for_filename(title),
    )
}

fn sanitize_path_segment(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "_".to_string()
    } else {
        output
    }
}

fn sanitize_title_for_filename(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            output.push('_');
        } else {
            output.push(ch);
        }
    }
    if output.is_empty() {
        "_".to_string()
    } else {
        output
    }
}

fn sanitize_id(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            output.push('-');
            previous_dash = true;
        }
    }
    output.trim_matches('-').to_string()
}

fn serialize_string_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| normalize_title(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n")
}

fn deserialize_string_list(value: &str) -> Vec<String> {
    value
        .lines()
        .map(normalize_title)
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        DOCS_NAMESPACE_MANUAL, DocsApi, RemoteDocsPage, TechnicalDocType, TechnicalImportTask,
        import::collect_pages_for_technical_task,
    };

    #[derive(Default)]
    struct MockDocsApi {
        subpages: Vec<String>,
        pages: BTreeMap<String, RemoteDocsPage>,
        default_page_content: Option<String>,
        subpage_calls: Vec<(String, i32, usize)>,
        page_calls: Vec<String>,
    }

    impl DocsApi for MockDocsApi {
        fn get_subpages(
            &mut self,
            prefix: &str,
            namespace: i32,
            limit: usize,
        ) -> anyhow::Result<Vec<String>> {
            self.subpage_calls
                .push((prefix.to_string(), namespace, limit));
            Ok(self.subpages.clone())
        }

        fn get_page(&mut self, title: &str) -> anyhow::Result<Option<RemoteDocsPage>> {
            self.page_calls.push(title.to_string());
            Ok(self.pages.get(title).cloned().or_else(|| {
                self.default_page_content
                    .as_ref()
                    .map(|content| RemoteDocsPage {
                        requested_title: title.to_string(),
                        title: title.to_string(),
                        timestamp: String::new(),
                        content: format!("{content} {title}"),
                    })
            }))
        }

        fn request_count(&self) -> usize {
            self.page_calls.len() + self.subpage_calls.len()
        }
    }

    #[test]
    fn collect_pages_for_technical_task_uses_mediawiki_namespace_and_skips_translation_variants() {
        let mut api = MockDocsApi {
            subpages: vec![
                "Manual:Hooks/PageSaveComplete/en".to_string(),
                "Manual:Hooks/PageSaveComplete".to_string(),
            ],
            pages: BTreeMap::from([
                (
                    "Manual:Hooks".to_string(),
                    RemoteDocsPage {
                        requested_title: "Manual:Hooks".to_string(),
                        title: "Manual:Hooks".to_string(),
                        timestamp: String::new(),
                        content: "Hooks index".to_string(),
                    },
                ),
                (
                    "Manual:Hooks/PageSaveComplete".to_string(),
                    RemoteDocsPage {
                        requested_title: "Manual:Hooks/PageSaveComplete".to_string(),
                        title: "Manual:Hooks/PageSaveComplete".to_string(),
                        timestamp: String::new(),
                        content: "PageSaveComplete docs".to_string(),
                    },
                ),
            ]),
            default_page_content: None,
            subpage_calls: Vec::new(),
            page_calls: Vec::new(),
        };
        let mut task = TechnicalImportTask {
            doc_type: TechnicalDocType::Hooks,
            page_title: None,
            include_subpages: true,
        };

        let pages = collect_pages_for_technical_task(&mut api, &mut task, 25).unwrap();

        assert_eq!(api.subpage_calls.len(), 1);
        assert_eq!(api.subpage_calls[0].0, "Manual:Hooks/");
        assert_eq!(api.subpage_calls[0].1, DOCS_NAMESPACE_MANUAL);
        assert!(
            !api.page_calls
                .iter()
                .any(|title| title == "Manual:Hooks/PageSaveComplete/en")
        );
        assert_eq!(pages.len(), 2);
        assert!(
            pages
                .iter()
                .any(|page| page.page_title == "Manual:Hooks/PageSaveComplete")
        );
    }

    #[test]
    fn import_docs_profile_skips_installed_extension_discovery_failures() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        std::fs::create_dir_all(&project_root).expect("create project root");
        let context = crate::runtime::ResolutionContext {
            cwd: project_root.clone(),
            executable_dir: None,
        };
        let overrides = crate::runtime::PathOverrides {
            project_root: Some(project_root.clone()),
            ..crate::runtime::PathOverrides::default()
        };
        let paths = crate::runtime::resolve_paths(&context, &overrides).expect("resolve runtime");
        crate::runtime::init_layout(&paths, &crate::runtime::InitOptions::default())
            .expect("init runtime");

        let mut api = MockDocsApi {
            subpages: Vec::new(),
            pages: BTreeMap::new(),
            default_page_content: Some("Profile docs fixture".to_string()),
            subpage_calls: Vec::new(),
            page_calls: Vec::new(),
        };
        let report = super::import_docs_profile_with_api(
            &paths,
            &super::DocsImportProfileOptions {
                profile: "remilia-mw-1.44".to_string(),
                include_installed_extensions: false,
                include_extension_subpages: false,
                extra_extensions: Vec::new(),
                limit: 2,
            },
            &crate::config::WikiConfig::default(),
            &mut api,
        )
        .expect("profile import should degrade cleanly");

        assert_eq!(report.profile, "remilia-mw-1.44");
        assert!(report.imported_pages > 0);
        assert!(
            report
                .failures
                .iter()
                .any(|entry| entry.contains("installed extension discovery skipped"))
        );
    }
}
