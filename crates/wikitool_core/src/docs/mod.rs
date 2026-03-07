use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::config::WikiConfig;
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{compute_hash, unix_timestamp};

mod fetch;
mod parse;

pub use fetch::{
    DocsApi, DocsClientConfig, MediaWikiDocsClient, RemoteDocsPage,
    discover_installed_extensions_from_wiki, discover_installed_extensions_from_wiki_with_config,
};

use parse::{
    DocsPageParseInput, ParsedDocsExample, ParsedDocsLink, ParsedDocsSection, ParsedDocsSymbol,
    estimate_tokens, is_translation_variant, normalize_retrieval_key, normalize_title,
    parse_docs_page,
};

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

pub fn search_docs(
    paths: &ResolvedPaths,
    query: &str,
    options: &DocsSearchOptions,
) -> Result<Vec<DocsSearchHit>> {
    let connection = open_docs_connection(paths)?;
    let context = SearchContext::new(query, options.limit)?;
    if context.query_lower.is_empty() {
        return Ok(Vec::new());
    }

    let scope = SearchScope::parse(options.tier.as_deref())?;
    let mut hits = Vec::new();
    if scope.include_pages {
        hits.extend(search_page_hits(
            &connection,
            &context,
            options.profile.as_deref(),
            scope.corpus_kind_filter.as_deref(),
        )?);
    }
    if scope.include_sections {
        hits.extend(search_section_hits(
            &connection,
            &context,
            options.profile.as_deref(),
            scope.corpus_kind_filter.as_deref(),
        )?);
    }
    if scope.include_symbols {
        hits.extend(
            search_symbol_hits(
                &connection,
                &context,
                options.profile.as_deref(),
                scope.corpus_kind_filter.as_deref(),
                None,
            )?
            .into_iter()
            .map(symbol_hit_to_search_hit),
        );
    }
    if scope.include_examples {
        hits.extend(search_example_hits(
            &connection,
            &context,
            options.profile.as_deref(),
            scope.corpus_kind_filter.as_deref(),
        )?);
    }

    hits.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.page_title.cmp(&right.page_title))
    });
    hits.truncate(context.limit);
    Ok(hits)
}

fn symbol_hit_to_search_hit(hit: DocsSymbolHit) -> DocsSearchHit {
    let snippet = if hit.detail_text.is_empty() {
        hit.summary_text.clone()
    } else {
        format!("{} {}", hit.summary_text, hit.detail_text)
    };
    DocsSearchHit {
        tier: "symbol".to_string(),
        title: hit.symbol_name.clone(),
        page_title: hit.page_title,
        corpus_id: hit.corpus_id,
        corpus_kind: hit.corpus_kind,
        source_profile: hit.source_profile,
        section_heading: hit.section_heading,
        retrieval_weight: hit.retrieval_weight,
        snippet,
        signals: hit.signals,
    }
}

pub fn lookup_docs_symbols(
    paths: &ResolvedPaths,
    query: &str,
    options: &DocsSymbolLookupOptions,
) -> Result<Vec<DocsSymbolHit>> {
    let connection = open_docs_connection(paths)?;
    let context = SearchContext::new(query, options.limit)?;
    if context.query_lower.is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = search_symbol_hits(
        &connection,
        &context,
        options.profile.as_deref(),
        None,
        options.kind.as_deref(),
    )?;
    hits.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.symbol_name.cmp(&right.symbol_name))
            .then_with(|| left.page_title.cmp(&right.page_title))
    });
    hits.truncate(context.limit);
    Ok(hits)
}

pub fn build_docs_context(
    paths: &ResolvedPaths,
    query: &str,
    options: &DocsContextOptions,
) -> Result<DocsContextReport> {
    let connection = open_docs_connection(paths)?;
    let limit = options.limit.max(1);
    let token_budget = options.token_budget.max(1);
    let context = SearchContext::new(query, limit.saturating_mul(3))?;
    if context.query_lower.is_empty() {
        return Ok(DocsContextReport {
            query: query.to_string(),
            profile: options.profile.clone(),
            pages: Vec::new(),
            sections: Vec::new(),
            symbols: Vec::new(),
            examples: Vec::new(),
            token_estimate: 0,
        });
    }

    let mut pages = search_page_hits(&connection, &context, options.profile.as_deref(), None)?;
    let mut sections = load_context_sections(&connection, &context, options.profile.as_deref())?;
    let mut symbols = search_symbol_hits(
        &connection,
        &context,
        options.profile.as_deref(),
        None,
        None,
    )?;
    let mut examples = load_context_examples(&connection, &context, options.profile.as_deref())?;

    pages.sort_by(|left, right| right.retrieval_weight.cmp(&left.retrieval_weight));
    sections.sort_by(|left, right| right.retrieval_weight.cmp(&left.retrieval_weight));
    symbols.sort_by(|left, right| right.retrieval_weight.cmp(&left.retrieval_weight));
    examples.sort_by(|left, right| right.retrieval_weight.cmp(&left.retrieval_weight));

    let mut used_tokens = 0usize;
    let mut selected_pages = Vec::new();
    let mut selected_sections = Vec::new();
    let mut selected_symbols = Vec::new();
    let mut selected_examples = Vec::new();

    for symbol in symbols.into_iter().take(limit) {
        let estimated =
            estimate_tokens(&format!("{} {}", symbol.summary_text, symbol.detail_text)).max(1);
        if !selected_symbols.is_empty() && used_tokens + estimated > token_budget {
            continue;
        }
        used_tokens += estimated;
        selected_symbols.push(symbol);
    }
    for section in sections.into_iter().take(limit) {
        if !selected_sections.is_empty() && used_tokens + section.token_estimate > token_budget {
            continue;
        }
        used_tokens += section.token_estimate;
        selected_sections.push(section);
    }
    for example in examples.into_iter().take(limit) {
        if !selected_examples.is_empty() && used_tokens + example.token_estimate > token_budget {
            continue;
        }
        used_tokens += example.token_estimate;
        selected_examples.push(example);
    }
    for page in pages.into_iter().take(limit) {
        let estimated = estimate_tokens(&page.snippet).max(1);
        if !selected_pages.is_empty() && used_tokens + estimated > token_budget {
            continue;
        }
        used_tokens += estimated;
        selected_pages.push(page);
    }

    Ok(DocsContextReport {
        query: normalize_title(query),
        profile: options.profile.clone(),
        pages: selected_pages,
        sections: selected_sections,
        symbols: selected_symbols,
        examples: selected_examples,
        token_estimate: used_tokens,
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

#[derive(Debug, Clone)]
struct SearchContext {
    query_lower: String,
    query_key: String,
    like_pattern: String,
    fts_query: Option<String>,
    limit: usize,
}

impl SearchContext {
    fn new(query: &str, limit: usize) -> Result<Self> {
        let normalized = normalize_title(query);
        let lowered = normalized.to_ascii_lowercase();
        let query_key = normalize_retrieval_key(&normalized);
        if limit == 0 {
            bail!("search limit must be greater than zero");
        }
        Ok(Self {
            query_lower: lowered.clone(),
            query_key: query_key.clone(),
            like_pattern: format!("%{lowered}%"),
            fts_query: build_docs_fts_query(&normalized, &query_key),
            limit,
        })
    }
}

fn build_docs_fts_query(normalized_query: &str, query_key: &str) -> Option<String> {
    let terms = collect_docs_fts_terms(normalized_query, query_key);
    if terms.is_empty() {
        return None;
    }

    let phrase = format!("\"{}\"", terms.join(" "));
    if terms.len() == 1 {
        return Some(format!("{phrase} OR {}*", terms[0]));
    }

    let conjunction = terms
        .iter()
        .map(|term| format!("{term}*"))
        .collect::<Vec<_>>()
        .join(" AND ");
    Some(format!("{phrase} OR ({conjunction})"))
}

fn collect_docs_fts_terms(normalized_query: &str, query_key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    let push_current = |out: &mut Vec<String>, current: &mut String| {
        if current.is_empty() {
            return;
        }
        if !out.iter().any(|value| value.as_str() == current.as_str()) {
            out.push(current.clone());
        }
        current.clear();
    };

    for ch in normalized_query.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else {
            push_current(&mut out, &mut current);
        }
    }
    push_current(&mut out, &mut current);

    if out.is_empty() {
        for part in query_key.split(' ') {
            let term = part.trim();
            if term.is_empty() || out.iter().any(|value| value == term) {
                continue;
            }
            out.push(term.to_string());
        }
    }

    out
}

fn fts_position_bonus(index: usize, base: usize) -> usize {
    base.saturating_sub(index.saturating_mul(4)).max(8)
}

#[derive(Debug, Clone)]
struct SearchScope {
    include_pages: bool,
    include_sections: bool,
    include_symbols: bool,
    include_examples: bool,
    corpus_kind_filter: Option<String>,
}

impl SearchScope {
    fn parse(value: Option<&str>) -> Result<Self> {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(Self {
                include_pages: true,
                include_sections: true,
                include_symbols: true,
                include_examples: true,
                corpus_kind_filter: None,
            });
        };
        let lowered = value.to_ascii_lowercase();
        match lowered.as_str() {
            "page" => Ok(Self {
                include_pages: true,
                include_sections: false,
                include_symbols: false,
                include_examples: false,
                corpus_kind_filter: None,
            }),
            "section" => Ok(Self {
                include_pages: false,
                include_sections: true,
                include_symbols: false,
                include_examples: false,
                corpus_kind_filter: None,
            }),
            "symbol" => Ok(Self {
                include_pages: false,
                include_sections: false,
                include_symbols: true,
                include_examples: false,
                corpus_kind_filter: None,
            }),
            "example" => Ok(Self {
                include_pages: false,
                include_sections: false,
                include_symbols: false,
                include_examples: true,
                corpus_kind_filter: None,
            }),
            "extension" | "technical" | "profile" => Ok(Self {
                include_pages: true,
                include_sections: true,
                include_symbols: true,
                include_examples: true,
                corpus_kind_filter: Some(lowered),
            }),
            _ => bail!(
                "unsupported docs tier `{value}`; expected page|section|symbol|example|extension|technical|profile"
            ),
        }
    }
}

fn resolve_docs_profile(value: &str) -> Result<&'static DocsProfileDefinition> {
    let normalized = normalize_title(value);
    DOCS_PROFILES
        .iter()
        .find(|profile| profile.id.eq_ignore_ascii_case(&normalized))
        .ok_or_else(|| anyhow::anyhow!("unsupported docs profile `{normalized}`"))
}

fn collect_profile_pages<A: DocsApi>(
    api: &mut A,
    definition: &DocsProfileDefinition,
    limit: usize,
) -> Result<Vec<FetchedDocsPage>> {
    let mut pages = Vec::new();
    let mut seen = BTreeSet::new();
    for seed in definition.page_seeds {
        let doc_type = infer_doc_type_from_title(seed.title);
        let mut task = TechnicalImportTask {
            doc_type,
            page_title: Some(seed.title.to_string()),
            include_subpages: seed.include_subpages,
        };
        let mut fetched = collect_pages_for_technical_task(api, &mut task, limit)?;
        for page in fetched.drain(..) {
            let key = page.page_title.to_ascii_lowercase();
            if seen.insert(key) {
                pages.push(page);
            }
        }
    }
    if pages.is_empty() {
        bail!("docs profile `{}` fetched no pages", definition.id);
    }
    Ok(pages)
}

fn import_extension_docs_with_api_internal<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportOptions,
    api: &mut A,
    source_profile: &str,
    source_version: &str,
) -> Result<DocsImportReport> {
    if options.extensions.is_empty() {
        bail!("no extensions specified for docs import");
    }

    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let requested_extensions = options.extensions.len();
    let mut imported_extensions = 0usize;
    let mut stats = PersistStats::default();
    let mut failures = Vec::new();

    for extension in normalize_extensions(&options.extensions) {
        let main_page = format!("Extension:{extension}");
        let mut pages_to_fetch = vec![main_page.clone()];
        if options.include_subpages {
            match api.get_subpages(
                &format!("Extension:{extension}/"),
                DOCS_NAMESPACE_EXTENSION,
                usize::MAX,
            ) {
                Ok(mut subpages) => pages_to_fetch.append(&mut subpages),
                Err(error) => {
                    failures.push(format!("{extension}: failed to list subpages: {error}"));
                    continue;
                }
            }
        }
        filter_translation_titles(&mut pages_to_fetch);
        dedupe_titles_in_order(&mut pages_to_fetch);

        let mut fetched_pages = Vec::new();
        let mut page_failed = false;
        for title in pages_to_fetch {
            if is_translation_variant(&title) {
                continue;
            }
            match api.get_page(&title) {
                Ok(Some(page)) => {
                    if is_translation_variant(&page.title) {
                        continue;
                    }
                    let mut alias_titles = Vec::new();
                    if !page.requested_title.eq_ignore_ascii_case(&page.title) {
                        alias_titles.push(page.requested_title);
                    }
                    fetched_pages.push(FetchedDocsPage {
                        page_title: page.title.clone(),
                        alias_titles,
                        local_path: extension_local_path(&extension, &page.title),
                        content: page.content,
                    });
                }
                Ok(None) => {
                    failures.push(format!("{extension}: page missing during refresh: {title}"));
                    page_failed = true;
                    break;
                }
                Err(error) => {
                    failures.push(format!("{extension}: failed to fetch {title}: {error}"));
                    page_failed = true;
                    break;
                }
            }
        }
        if page_failed || fetched_pages.is_empty() {
            continue;
        }

        let descriptor = CorpusDescriptor {
            corpus_id: extension_corpus_id(&extension, source_profile),
            corpus_kind: "extension".to_string(),
            label: format!("Extension:{extension}"),
            source_wiki: "mediawiki.org".to_string(),
            source_version: source_version.to_string(),
            source_profile: source_profile.to_string(),
            technical_type: String::new(),
            refresh_kind: "extension".to_string(),
            refresh_spec: serde_json::to_string(&ExtensionRefreshSpec {
                extension_name: extension.clone(),
                include_subpages: options.include_subpages,
                source_profile: source_profile.to_string(),
                source_version: source_version.to_string(),
            })?,
            fetched_at_unix: now_unix,
            expires_at_unix,
        };
        let persisted = persist_docs_corpus(paths, &descriptor, &fetched_pages)?;
        imported_extensions += 1;
        accumulate_stats(&mut stats, &persisted);
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsImportReport {
        requested_extensions,
        imported_extensions,
        imported_pages: stats.pages,
        imported_sections: stats.sections,
        imported_symbols: stats.symbols,
        imported_examples: stats.examples,
        failures,
        request_count: api.request_count(),
    })
}

fn import_technical_docs_with_api_internal<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportTechnicalOptions,
    api: &mut A,
    source_profile: &str,
    source_version: &str,
) -> Result<DocsImportTechnicalReport> {
    if options.tasks.is_empty() {
        bail!("no technical docs tasks specified");
    }

    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let mut imported_corpora = 0usize;
    let mut imported_by_type = BTreeMap::new();
    let mut stats = PersistStats::default();
    let mut failures = Vec::new();

    for task in &options.tasks {
        let mut task_clone = task.clone();
        match collect_pages_for_technical_task(api, &mut task_clone, options.limit.max(1)) {
            Ok(fetched_pages) => {
                if fetched_pages.is_empty() {
                    failures.push(format!(
                        "{}: no pages fetched for task",
                        task.doc_type.as_str()
                    ));
                    continue;
                }
                let descriptor = CorpusDescriptor {
                    corpus_id: technical_corpus_id(
                        task.doc_type,
                        task.page_title.as_deref(),
                        source_profile,
                    ),
                    corpus_kind: "technical".to_string(),
                    label: task
                        .page_title
                        .clone()
                        .unwrap_or_else(|| task.doc_type.main_page().to_string()),
                    source_wiki: "mediawiki.org".to_string(),
                    source_version: source_version.to_string(),
                    source_profile: source_profile.to_string(),
                    technical_type: task.doc_type.as_str().to_string(),
                    refresh_kind: "technical".to_string(),
                    refresh_spec: serde_json::to_string(&TechnicalRefreshSpec {
                        doc_type: task.doc_type.as_str().to_string(),
                        page_title: task.page_title.clone(),
                        include_subpages: task.include_subpages,
                        limit: options.limit.max(1),
                        source_profile: source_profile.to_string(),
                        source_version: source_version.to_string(),
                    })?,
                    fetched_at_unix: now_unix,
                    expires_at_unix,
                };
                let persisted = persist_docs_corpus(paths, &descriptor, &fetched_pages)?;
                imported_corpora += 1;
                *imported_by_type
                    .entry(task.doc_type.as_str().to_string())
                    .or_insert(0) += persisted.pages;
                accumulate_stats(&mut stats, &persisted);
            }
            Err(error) => failures.push(format!("{}: {error}", task.doc_type.as_str())),
        }
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsImportTechnicalReport {
        requested_tasks: options.tasks.len(),
        imported_corpora,
        imported_pages: stats.pages,
        imported_sections: stats.sections,
        imported_symbols: stats.symbols,
        imported_examples: stats.examples,
        imported_by_type,
        failures,
        request_count: api.request_count(),
    })
}

fn collect_pages_for_technical_task<A: DocsApi>(
    api: &mut A,
    task: &mut TechnicalImportTask,
    limit: usize,
) -> Result<Vec<FetchedDocsPage>> {
    let mut pages_to_fetch = Vec::new();
    if let Some(page_title) = task.page_title.as_deref() {
        let normalized = normalize_title(page_title);
        if !normalized.is_empty() {
            pages_to_fetch.push(normalized.clone());
            if task.include_subpages {
                let prefix = if normalized.ends_with('/') {
                    normalized.clone()
                } else {
                    format!("{normalized}/")
                };
                let mut subpages = api.get_subpages(
                    &prefix,
                    infer_doc_type_from_title(&normalized).namespace(),
                    limit.max(1),
                )?;
                pages_to_fetch.append(&mut subpages);
            }
        }
    } else {
        pages_to_fetch.push(task.doc_type.main_page().to_string());
        if task.include_subpages {
            let mut subpages = api.get_subpages(
                task.doc_type.subpage_prefix(),
                task.doc_type.namespace(),
                limit.max(1),
            )?;
            pages_to_fetch.append(&mut subpages);
        }
    }
    filter_translation_titles(&mut pages_to_fetch);
    dedupe_titles_in_order(&mut pages_to_fetch);

    let mut fetched_pages = Vec::new();
    for title in pages_to_fetch {
        if is_translation_variant(&title) {
            continue;
        }
        match api.get_page(&title)? {
            Some(page) => {
                if is_translation_variant(&page.title) {
                    continue;
                }
                let mut alias_titles = Vec::new();
                if !page.requested_title.eq_ignore_ascii_case(&page.title) {
                    alias_titles.push(page.requested_title);
                }
                fetched_pages.push(FetchedDocsPage {
                    page_title: page.title.clone(),
                    alias_titles,
                    local_path: technical_local_path(
                        infer_doc_type_from_title(&page.title),
                        &page.title,
                    ),
                    content: page.content,
                });
            }
            None => bail!("page missing during refresh: {title}"),
        }
    }
    Ok(fetched_pages)
}

fn persist_docs_corpus(
    paths: &ResolvedPaths,
    descriptor: &CorpusDescriptor,
    pages: &[FetchedDocsPage],
) -> Result<PersistStats> {
    let parsed_pages = pages
        .iter()
        .map(|page| {
            let parsed = parse_docs_page(DocsPageParseInput {
                page_title: page.page_title.clone(),
                local_path: page.local_path.clone(),
                content: page.content.clone(),
                source_revision_id: None,
                source_parent_revision_id: None,
                source_timestamp: None,
            });
            let mut alias_titles = page.alias_titles.clone();
            alias_titles.extend(parsed.alias_titles.clone());
            dedupe_titles_in_order(&mut alias_titles);
            (page, alias_titles, parsed)
        })
        .collect::<Vec<_>>();

    let mut stats = PersistStats::default();
    let mut connection = open_docs_connection(paths)?;
    let transaction = connection
        .transaction()
        .context("failed to start docs corpus transaction")?;

    transaction.execute(
        "DELETE FROM docs_corpora WHERE corpus_id = ?1",
        params![descriptor.corpus_id],
    )?;
    transaction.execute(
        "INSERT INTO docs_corpora (
            corpus_id, corpus_kind, label, source_wiki, source_version, source_profile,
            technical_type, refresh_kind, refresh_spec, pages_count, sections_count,
            symbols_count, examples_count, fetched_at_unix, expires_at_unix
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, 0, 0, ?10, ?11)",
        params![
            descriptor.corpus_id,
            descriptor.corpus_kind,
            descriptor.label,
            descriptor.source_wiki,
            descriptor.source_version,
            descriptor.source_profile,
            descriptor.technical_type,
            descriptor.refresh_kind,
            descriptor.refresh_spec,
            i64::try_from(descriptor.fetched_at_unix)?,
            i64::try_from(descriptor.expires_at_unix)?,
        ],
    )?;

    let mut page_statement = transaction.prepare(
        "INSERT INTO docs_pages (
            corpus_id, page_title, normalized_title_key, page_namespace, doc_type, title_aliases,
            local_path, raw_content, normalized_content, content_hash, summary_text,
            semantic_text, fetched_at_unix, token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    let mut section_statement = transaction.prepare(
        "INSERT INTO docs_sections (
            corpus_id, page_title, section_index, section_level, section_heading, summary_text,
            section_text, semantic_text, token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    let mut symbol_statement = transaction.prepare(
        "INSERT INTO docs_symbols (
            corpus_id, page_title, symbol_index, symbol_kind, symbol_name, normalized_symbol_key,
            aliases, section_heading, signature_text, summary_text, detail_text, retrieval_text,
            token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    let mut example_statement = transaction.prepare(
        "INSERT INTO docs_examples (
            corpus_id, page_title, example_index, example_kind, section_heading, language_hint,
            summary_text, example_text, retrieval_text, token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    let mut link_statement = transaction.prepare(
        "INSERT INTO docs_links (
            corpus_id, page_title, link_index, target_title, relation_kind, display_text
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    for (page, alias_titles, parsed) in &parsed_pages {
        page_statement.execute(params![
            descriptor.corpus_id,
            page.page_title,
            normalize_retrieval_key(&page.page_title),
            parsed.page_namespace,
            parsed.page_kind,
            serialize_string_list(alias_titles),
            page.local_path,
            page.content,
            parsed.normalized_content,
            compute_hash(&page.content),
            parsed.summary_text,
            parsed.semantic_text,
            i64::try_from(descriptor.fetched_at_unix)?,
            i64::try_from(parsed.token_estimate)?,
        ])?;
        stats.pages += 1;

        insert_sections(
            &mut section_statement,
            &descriptor.corpus_id,
            &page.page_title,
            &parsed.sections,
            &mut stats,
        )?;
        insert_symbols(
            &mut symbol_statement,
            &descriptor.corpus_id,
            &page.page_title,
            &parsed.symbols,
            &mut stats,
        )?;
        insert_examples(
            &mut example_statement,
            &descriptor.corpus_id,
            &page.page_title,
            &parsed.examples,
            &mut stats,
        )?;
        insert_links(
            &mut link_statement,
            &descriptor.corpus_id,
            &page.page_title,
            &parsed.links,
        )?;
    }

    transaction.execute(
        "UPDATE docs_corpora
         SET pages_count = ?2, sections_count = ?3, symbols_count = ?4, examples_count = ?5
         WHERE corpus_id = ?1",
        params![
            descriptor.corpus_id,
            i64::try_from(stats.pages)?,
            i64::try_from(stats.sections)?,
            i64::try_from(stats.symbols)?,
            i64::try_from(stats.examples)?,
        ],
    )?;

    drop(link_statement);
    drop(example_statement);
    drop(symbol_statement);
    drop(section_statement);
    drop(page_statement);
    transaction
        .commit()
        .context("failed to commit docs corpus transaction")?;
    Ok(stats)
}

fn insert_sections(
    statement: &mut rusqlite::Statement<'_>,
    corpus_id: &str,
    page_title: &str,
    sections: &[ParsedDocsSection],
    stats: &mut PersistStats,
) -> Result<()> {
    for section in sections {
        statement.execute(params![
            corpus_id,
            page_title,
            i64::try_from(section.section_index)?,
            i64::from(section.section_level),
            section.section_heading,
            section.summary_text,
            section.section_text,
            section.semantic_text,
            i64::try_from(section.token_estimate)?,
        ])?;
        stats.sections += 1;
    }
    Ok(())
}

fn insert_symbols(
    statement: &mut rusqlite::Statement<'_>,
    corpus_id: &str,
    page_title: &str,
    symbols: &[ParsedDocsSymbol],
    stats: &mut PersistStats,
) -> Result<()> {
    for (index, symbol) in symbols.iter().enumerate() {
        statement.execute(params![
            corpus_id,
            page_title,
            i64::try_from(index)?,
            symbol.symbol_kind,
            symbol.symbol_name,
            symbol.normalized_symbol_key,
            serialize_string_list(&symbol.aliases),
            symbol.section_heading,
            symbol.signature_text,
            symbol.summary_text,
            symbol.detail_text,
            symbol.retrieval_text,
            i64::try_from(symbol.token_estimate)?,
        ])?;
        stats.symbols += 1;
    }
    Ok(())
}

fn insert_examples(
    statement: &mut rusqlite::Statement<'_>,
    corpus_id: &str,
    page_title: &str,
    examples: &[ParsedDocsExample],
    stats: &mut PersistStats,
) -> Result<()> {
    for (index, example) in examples.iter().enumerate() {
        statement.execute(params![
            corpus_id,
            page_title,
            i64::try_from(index)?,
            example.example_kind,
            example.section_heading,
            example.language_hint,
            example.summary_text,
            example.example_text,
            example.retrieval_text,
            i64::try_from(example.token_estimate)?,
        ])?;
        stats.examples += 1;
    }
    Ok(())
}

fn insert_links(
    statement: &mut rusqlite::Statement<'_>,
    corpus_id: &str,
    page_title: &str,
    links: &[ParsedDocsLink],
) -> Result<()> {
    for (index, link) in links.iter().enumerate() {
        statement.execute(params![
            corpus_id,
            page_title,
            i64::try_from(index)?,
            link.target_title,
            link.relation_kind,
            link.display_text,
        ])?;
    }
    Ok(())
}

fn accumulate_stats(target: &mut PersistStats, incoming: &PersistStats) {
    target.pages += incoming.pages;
    target.sections += incoming.sections;
    target.symbols += incoming.symbols;
    target.examples += incoming.examples;
}

fn load_docs_stats(connection: &Connection) -> Result<DocsStats> {
    let corpora_count = count_query(connection, "SELECT COUNT(*) FROM docs_corpora")?;
    let pages_count = count_query(connection, "SELECT COUNT(*) FROM docs_pages")?;
    let sections_count = count_query(connection, "SELECT COUNT(*) FROM docs_sections")?;
    let symbols_count = count_query(connection, "SELECT COUNT(*) FROM docs_symbols")?;
    let examples_count = count_query(connection, "SELECT COUNT(*) FROM docs_examples")?;

    let mut corpora_by_kind = BTreeMap::new();
    let mut statement = connection.prepare(
        "SELECT corpus_kind, COUNT(*) FROM docs_corpora GROUP BY corpus_kind ORDER BY corpus_kind ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (kind, count) = row?;
        corpora_by_kind.insert(kind, usize::try_from(count).unwrap_or(0));
    }

    let mut technical_by_type = BTreeMap::new();
    let mut typed_statement = connection.prepare(
        "SELECT technical_type, COUNT(*) FROM docs_corpora
         WHERE technical_type != ''
         GROUP BY technical_type
         ORDER BY technical_type ASC",
    )?;
    let typed_rows = typed_statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in typed_rows {
        let (doc_type, count) = row?;
        technical_by_type.insert(doc_type, usize::try_from(count).unwrap_or(0));
    }

    Ok(DocsStats {
        corpora_count,
        pages_count,
        sections_count,
        symbols_count,
        examples_count,
        corpora_by_kind,
        technical_by_type,
    })
}

fn load_docs_corpora(
    connection: &Connection,
    corpus_kind: Option<&str>,
    technical_type: Option<&str>,
    profile: Option<&str>,
    now_unix: u64,
) -> Result<Vec<DocsCorpusSummary>> {
    let mut out = Vec::new();
    let corpus_kind = corpus_kind.unwrap_or_default().to_string();
    let technical_type = technical_type.unwrap_or_default().to_string();
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT corpus_id, corpus_kind, label, source_wiki, source_version, source_profile,
                technical_type, pages_count, sections_count, symbols_count, examples_count,
                fetched_at_unix, expires_at_unix
         FROM docs_corpora
         WHERE (?1 = '' OR lower(corpus_kind) = lower(?1))
           AND (?2 = '' OR lower(technical_type) = lower(?2))
           AND (?3 = '' OR lower(source_profile) = lower(?3))
         ORDER BY corpus_kind ASC, label ASC",
    )?;
    let rows = statement.query_map(params![corpus_kind, technical_type, profile], |row| {
        let pages_count: i64 = row.get(7)?;
        let sections_count: i64 = row.get(8)?;
        let symbols_count: i64 = row.get(9)?;
        let examples_count: i64 = row.get(10)?;
        let fetched_at_unix: i64 = row.get(11)?;
        let expires_at_unix: i64 = row.get(12)?;
        Ok(DocsCorpusSummary {
            corpus_id: row.get(0)?,
            corpus_kind: row.get(1)?,
            label: row.get(2)?,
            source_wiki: row.get(3)?,
            source_version: row.get(4)?,
            source_profile: row.get(5)?,
            technical_type: row.get(6)?,
            pages_count: usize::try_from(pages_count).unwrap_or(0),
            sections_count: usize::try_from(sections_count).unwrap_or(0),
            symbols_count: usize::try_from(symbols_count).unwrap_or(0),
            examples_count: usize::try_from(examples_count).unwrap_or(0),
            fetched_at_unix: u64::try_from(fetched_at_unix).unwrap_or(0),
            expires_at_unix: u64::try_from(expires_at_unix).unwrap_or(0),
            expired: u64::try_from(expires_at_unix).unwrap_or(0) <= now_unix,
        })
    })?;
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn load_outdated_docs(connection: &Connection, now_unix: u64) -> Result<DocsOutdatedReport> {
    let now_i64 = i64::try_from(now_unix)?;
    let mut statement = connection.prepare(
        "SELECT corpus_id, corpus_kind, label, source_profile, expires_at_unix
         FROM docs_corpora
         WHERE refresh_kind != 'static' AND expires_at_unix <= ?1
         ORDER BY corpus_kind ASC, label ASC",
    )?;
    let rows = statement.query_map(params![now_i64], |row| {
        let expires_at_unix: i64 = row.get(4)?;
        Ok(DocsOutdatedCorpus {
            corpus_id: row.get(0)?,
            corpus_kind: row.get(1)?,
            label: row.get(2)?,
            source_profile: row.get(3)?,
            expires_at_unix: u64::try_from(expires_at_unix).unwrap_or(0),
        })
    })?;
    let mut corpora = Vec::new();
    for row in rows {
        corpora.push(row?);
    }
    Ok(DocsOutdatedReport { corpora })
}

fn load_outdated_refresh_rows(paths: &ResolvedPaths) -> Result<Vec<OutdatedRefreshRow>> {
    let connection = open_docs_connection(paths)?;
    let now_unix = unix_timestamp()?;
    let now_i64 = i64::try_from(now_unix)?;
    let mut statement = connection.prepare(
        "SELECT label, refresh_kind, refresh_spec, source_profile, corpus_kind
         FROM docs_corpora
         WHERE refresh_kind != 'static' AND expires_at_unix <= ?1
         ORDER BY CASE refresh_kind WHEN 'profile' THEN 0 ELSE 1 END, label ASC",
    )?;
    let rows = statement.query_map(params![now_i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    let mut profile_refreshes = BTreeSet::new();
    for row in rows {
        let (label, refresh_kind, refresh_spec, source_profile, corpus_kind) = row?;
        if refresh_kind == "profile" {
            profile_refreshes.insert(source_profile.clone());
        }
        out.push((
            label,
            refresh_kind,
            refresh_spec,
            source_profile,
            corpus_kind,
        ));
    }

    Ok(out
        .into_iter()
        .filter(|(_, refresh_kind, _, source_profile, corpus_kind)| {
            !(refresh_kind == "extension"
                && corpus_kind == "extension"
                && !source_profile.is_empty()
                && profile_refreshes.contains(source_profile))
        })
        .map(
            |(label, refresh_kind, refresh_spec, _, _)| OutdatedRefreshRow {
                label,
                refresh_kind,
                refresh_spec,
            },
        )
        .collect())
}

fn search_page_hits(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let hits = search_page_hits_fts(connection, context, profile, corpus_kind_filter)?;
    if hits.is_empty() {
        return search_page_hits_like(connection, context, profile, corpus_kind_filter);
    }
    Ok(hits)
}

fn search_page_hits_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(2))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                p.page_title, p.title_aliases, p.summary_text, p.normalized_content, p.semantic_text
         FROM docs_pages_fts
         JOIN docs_pages p ON p.rowid = docs_pages_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = p.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND docs_pages_fts MATCH ?3
         ORDER BY bm25(docs_pages_fts, 8.0, 6.0, 2.0, 1.5, 1.0) ASC, p.page_title ASC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, match_query, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            aliases,
            summary_text,
            content,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 70usize;
        weight += fts_position_bonus(out.len(), 64);
        signals.push("fts-match".to_string());
        if normalize_retrieval_key(&page_title) == context.query_key {
            weight += 120;
            signals.push("exact-page-title".to_string());
        }
        if page_title.to_ascii_lowercase() == context.query_lower {
            weight += 90;
            signals.push("page-title-match".to_string());
        }
        if aliases.to_ascii_lowercase().contains(&context.query_lower) {
            weight += 50;
            signals.push("page-alias-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("page-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("page-semantic-match".to_string());
        }
        let snippet = if summary_text.is_empty() {
            make_snippet(&content, &context.query_lower)
        } else {
            make_snippet(&summary_text, &context.query_lower)
        };
        out.push(DocsSearchHit {
            tier: "page".to_string(),
            title: page_title.clone(),
            page_title,
            corpus_id,
            corpus_kind,
            source_profile,
            section_heading: None,
            retrieval_weight: weight,
            snippet,
            signals,
        });
    }
    Ok(out)
}

fn search_page_hits_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(2))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                p.page_title, p.title_aliases, p.summary_text, p.normalized_content, p.semantic_text
         FROM docs_pages p
         JOIN docs_corpora c ON c.corpus_id = p.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND (
                lower(p.page_title) LIKE ?3
             OR lower(p.title_aliases) LIKE ?3
             OR lower(p.summary_text) LIKE ?3
             OR lower(p.normalized_content) LIKE ?3
             OR lower(p.semantic_text) LIKE ?3
           )
         ORDER BY p.page_title ASC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, context.like_pattern, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            aliases,
            summary_text,
            content,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 70usize;
        if normalize_retrieval_key(&page_title) == context.query_key {
            weight += 120;
            signals.push("exact-page-title".to_string());
        }
        if page_title.to_ascii_lowercase() == context.query_lower {
            weight += 90;
            signals.push("page-title-match".to_string());
        }
        if aliases.to_ascii_lowercase().contains(&context.query_lower) {
            weight += 50;
            signals.push("page-alias-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("page-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("page-semantic-match".to_string());
        }
        let snippet = if summary_text.is_empty() {
            make_snippet(&content, &context.query_lower)
        } else {
            make_snippet(&summary_text, &context.query_lower)
        };
        out.push(DocsSearchHit {
            tier: "page".to_string(),
            title: page_title.clone(),
            page_title,
            corpus_id,
            corpus_kind,
            source_profile,
            section_heading: None,
            retrieval_weight: weight,
            snippet,
            signals,
        });
    }
    Ok(out)
}

fn search_section_hits(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let hits = search_section_hits_fts(connection, context, profile, corpus_kind_filter)?;
    if hits.is_empty() {
        return search_section_hits_like(connection, context, profile, corpus_kind_filter);
    }
    Ok(hits)
}

fn search_section_hits_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(2))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                s.page_title, s.section_heading, s.summary_text, s.section_text, s.semantic_text
         FROM docs_sections_fts
         JOIN docs_sections s ON s.rowid = docs_sections_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND docs_sections_fts MATCH ?3
         ORDER BY bm25(docs_sections_fts, 7.0, 7.0, 2.0, 1.0, 1.0) ASC, s.page_title ASC, s.section_index ASC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, match_query, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            section_heading,
            summary_text,
            section_text,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 90usize;
        weight += fts_position_bonus(out.len(), 58);
        signals.push("fts-match".to_string());
        if let Some(heading) = &section_heading {
            if normalize_retrieval_key(heading) == context.query_key {
                weight += 110;
                signals.push("exact-section-heading".to_string());
            }
            if heading.to_ascii_lowercase().contains(&context.query_lower) {
                weight += 60;
                signals.push("section-heading-match".to_string());
            }
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("section-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 20;
            signals.push("section-semantic-match".to_string());
        }
        out.push(DocsSearchHit {
            tier: "section".to_string(),
            title: section_heading
                .clone()
                .unwrap_or_else(|| page_title.clone()),
            page_title,
            corpus_id,
            corpus_kind,
            source_profile,
            section_heading: section_heading.clone(),
            retrieval_weight: weight,
            snippet: make_snippet(&section_text, &context.query_lower),
            signals,
        });
    }
    Ok(out)
}

fn search_section_hits_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(2))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                s.page_title, s.section_heading, s.summary_text, s.section_text, s.semantic_text
         FROM docs_sections s
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND (
                lower(COALESCE(s.section_heading, '')) LIKE ?3
             OR lower(s.summary_text) LIKE ?3
             OR lower(s.section_text) LIKE ?3
             OR lower(s.semantic_text) LIKE ?3
           )
         ORDER BY s.page_title ASC, s.section_index ASC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, context.like_pattern, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            section_heading,
            summary_text,
            section_text,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 90usize;
        if let Some(heading) = &section_heading {
            if normalize_retrieval_key(heading) == context.query_key {
                weight += 110;
                signals.push("exact-section-heading".to_string());
            }
            if heading.to_ascii_lowercase().contains(&context.query_lower) {
                weight += 60;
                signals.push("section-heading-match".to_string());
            }
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("section-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 20;
            signals.push("section-semantic-match".to_string());
        }
        out.push(DocsSearchHit {
            tier: "section".to_string(),
            title: section_heading
                .clone()
                .unwrap_or_else(|| page_title.clone()),
            page_title,
            corpus_id,
            corpus_kind,
            source_profile,
            section_heading: section_heading.clone(),
            retrieval_weight: weight,
            snippet: make_snippet(&section_text, &context.query_lower),
            signals,
        });
    }
    Ok(out)
}

fn search_symbol_hits(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
    symbol_kind_filter: Option<&str>,
) -> Result<Vec<DocsSymbolHit>> {
    let hits = search_symbol_hits_fts(
        connection,
        context,
        profile,
        corpus_kind_filter,
        symbol_kind_filter,
    )?;
    if hits.is_empty() {
        return search_symbol_hits_like(
            connection,
            context,
            profile,
            corpus_kind_filter,
            symbol_kind_filter,
        );
    }
    Ok(hits)
}

fn search_symbol_hits_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
    symbol_kind_filter: Option<&str>,
) -> Result<Vec<DocsSymbolHit>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let symbol_kind = symbol_kind_filter.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                s.page_title, s.symbol_kind, s.symbol_name, s.aliases, s.section_heading,
                s.signature_text, s.summary_text, s.detail_text, s.retrieval_text,
                s.normalized_symbol_key
         FROM docs_symbols_fts
         JOIN docs_symbols s ON s.rowid = docs_symbols_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND (?3 = '' OR lower(s.symbol_kind) = lower(?3))
           AND docs_symbols_fts MATCH ?4
         ORDER BY bm25(docs_symbols_fts, 7.0, 6.0, 3.0, 8.0, 5.0, 2.0, 1.0, 1.0, 1.0) ASC,
                  s.page_title ASC,
                  s.symbol_index ASC
         LIMIT ?5",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, symbol_kind, match_query, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            symbol_kind,
            symbol_name,
            aliases_blob,
            section_heading,
            signature_text,
            summary_text,
            detail_text,
            retrieval_text,
            normalized_symbol_key,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 120usize;
        weight += fts_position_bonus(out.len(), 72);
        signals.push("fts-match".to_string());
        if normalized_symbol_key == context.query_key {
            weight += 140;
            signals.push("exact-symbol-key".to_string());
        }
        if symbol_name.to_ascii_lowercase() == context.query_lower {
            weight += 100;
            signals.push("symbol-name-match".to_string());
        }
        if aliases_blob
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 65;
            signals.push("symbol-alias-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 40;
            signals.push("symbol-summary-match".to_string());
        }
        if retrieval_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("symbol-retrieval-match".to_string());
        }
        out.push(DocsSymbolHit {
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            symbol_kind,
            symbol_name,
            aliases: deserialize_string_list(&aliases_blob),
            section_heading,
            signature_text,
            summary_text,
            detail_text,
            retrieval_weight: weight,
            signals,
        });
    }
    Ok(out)
}

fn search_symbol_hits_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
    symbol_kind_filter: Option<&str>,
) -> Result<Vec<DocsSymbolHit>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let symbol_kind = symbol_kind_filter.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                s.page_title, s.symbol_kind, s.symbol_name, s.aliases, s.section_heading,
                s.signature_text, s.summary_text, s.detail_text, s.retrieval_text,
                s.normalized_symbol_key
         FROM docs_symbols s
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND (?3 = '' OR lower(s.symbol_kind) = lower(?3))
           AND (
                s.normalized_symbol_key = ?4
             OR lower(s.symbol_name) LIKE ?5
             OR lower(s.aliases) LIKE ?5
             OR lower(s.signature_text) LIKE ?5
             OR lower(s.summary_text) LIKE ?5
             OR lower(s.detail_text) LIKE ?5
             OR lower(s.retrieval_text) LIKE ?5
           )
         ORDER BY s.page_title ASC, s.symbol_index ASC
         LIMIT ?6",
    )?;
    let rows = statement.query_map(
        params![
            profile,
            corpus_kind,
            symbol_kind,
            context.query_key,
            context.like_pattern,
            limit_i64
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            symbol_kind,
            symbol_name,
            aliases_blob,
            section_heading,
            signature_text,
            summary_text,
            detail_text,
            retrieval_text,
            normalized_symbol_key,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 120usize;
        if normalized_symbol_key == context.query_key {
            weight += 140;
            signals.push("exact-symbol-key".to_string());
        }
        if symbol_name.to_ascii_lowercase() == context.query_lower {
            weight += 100;
            signals.push("symbol-name-match".to_string());
        }
        if aliases_blob
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 65;
            signals.push("symbol-alias-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 40;
            signals.push("symbol-summary-match".to_string());
        }
        if retrieval_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("symbol-retrieval-match".to_string());
        }
        out.push(DocsSymbolHit {
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            symbol_kind,
            symbol_name,
            aliases: deserialize_string_list(&aliases_blob),
            section_heading,
            signature_text,
            summary_text,
            detail_text,
            retrieval_weight: weight,
            signals,
        });
    }
    Ok(out)
}

fn search_example_hits(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    _corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let examples = load_context_examples(connection, context, profile)?;
    Ok(examples
        .into_iter()
        .map(|example| DocsSearchHit {
            tier: "example".to_string(),
            title: example.summary_text.clone(),
            page_title: example.page_title,
            corpus_id: example.corpus_id,
            corpus_kind: example.corpus_kind,
            source_profile: example.source_profile,
            section_heading: example.section_heading,
            retrieval_weight: example.retrieval_weight,
            snippet: make_snippet(&example.example_text, &context.query_lower),
            signals: example.signals,
        })
        .take(context.limit.saturating_mul(2))
        .collect())
}

fn load_context_sections(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextSection>> {
    let sections = load_context_sections_fts(connection, context, profile)?;
    if sections.is_empty() {
        return load_context_sections_like(connection, context, profile);
    }
    Ok(sections)
}

fn load_context_sections_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextSection>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, s.page_title, s.section_heading, s.summary_text, s.section_text,
                s.token_estimate, s.semantic_text
         FROM docs_sections_fts
         JOIN docs_sections s ON s.rowid = docs_sections_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND docs_sections_fts MATCH ?2
         ORDER BY bm25(docs_sections_fts, 7.0, 7.0, 2.0, 1.0, 1.0) ASC, s.page_title ASC, s.section_index ASC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(params![profile, match_query, limit_i64], |row| {
        let token_estimate: i64 = row.get(5)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            usize::try_from(token_estimate).unwrap_or(0),
            row.get::<_, String>(6)?,
        ))
    })?;
    for row in rows {
        let (
            corpus_id,
            page_title,
            section_heading,
            summary_text,
            section_text,
            token_estimate,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 90usize;
        weight += fts_position_bonus(out.len(), 58);
        signals.push("fts-match".to_string());
        if let Some(heading) = &section_heading {
            if normalize_retrieval_key(heading) == context.query_key {
                weight += 110;
                signals.push("exact-section-heading".to_string());
            }
            if heading.to_ascii_lowercase().contains(&context.query_lower) {
                weight += 60;
                signals.push("section-heading-match".to_string());
            }
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("section-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 20;
            signals.push("section-semantic-match".to_string());
        }
        out.push(DocsContextSection {
            corpus_id,
            page_title,
            section_heading,
            summary_text,
            section_text,
            retrieval_weight: weight,
            token_estimate,
            signals,
        });
    }
    Ok(out)
}

fn load_context_sections_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextSection>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, s.page_title, s.section_heading, s.summary_text, s.section_text,
                s.token_estimate, s.semantic_text
         FROM docs_sections s
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (
                lower(COALESCE(s.section_heading, '')) LIKE ?2
             OR lower(s.summary_text) LIKE ?2
             OR lower(s.section_text) LIKE ?2
             OR lower(s.semantic_text) LIKE ?2
           )
         ORDER BY s.page_title ASC, s.section_index ASC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(params![profile, context.like_pattern, limit_i64], |row| {
        let token_estimate: i64 = row.get(5)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            usize::try_from(token_estimate).unwrap_or(0),
            row.get::<_, String>(6)?,
        ))
    })?;
    for row in rows {
        let (
            corpus_id,
            page_title,
            section_heading,
            summary_text,
            section_text,
            token_estimate,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 90usize;
        if let Some(heading) = &section_heading {
            if normalize_retrieval_key(heading) == context.query_key {
                weight += 110;
                signals.push("exact-section-heading".to_string());
            }
            if heading.to_ascii_lowercase().contains(&context.query_lower) {
                weight += 60;
                signals.push("section-heading-match".to_string());
            }
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("section-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 20;
            signals.push("section-semantic-match".to_string());
        }
        out.push(DocsContextSection {
            corpus_id,
            page_title,
            section_heading,
            summary_text,
            section_text,
            retrieval_weight: weight,
            token_estimate,
            signals,
        });
    }
    Ok(out)
}

fn load_context_examples(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextExample>> {
    let examples = load_context_examples_fts(connection, context, profile)?;
    if examples.is_empty() {
        return load_context_examples_like(connection, context, profile);
    }
    Ok(examples)
}

fn load_context_examples_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextExample>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                e.page_title, e.example_kind, e.section_heading, e.language_hint,
                e.summary_text, e.example_text, e.token_estimate, e.retrieval_text
         FROM docs_examples_fts
         JOIN docs_examples e ON e.rowid = docs_examples_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = e.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND docs_examples_fts MATCH ?2
         ORDER BY bm25(docs_examples_fts, 5.0, 5.0, 2.0, 4.0, 2.0, 1.0, 1.0) ASC,
                  e.page_title ASC,
                  e.example_index ASC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(params![profile, match_query, limit_i64], |row| {
        let token_estimate: i64 = row.get(9)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            usize::try_from(token_estimate).unwrap_or(0),
            row.get::<_, String>(10)?,
        ))
    })?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            example_kind,
            section_heading,
            language_hint,
            summary_text,
            example_text,
            token_estimate,
            retrieval_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 80usize;
        weight += fts_position_bonus(out.len(), 54);
        signals.push("fts-match".to_string());
        if let Some(heading) = &section_heading
            && heading.to_ascii_lowercase().contains(&context.query_lower)
        {
            weight += 40;
            signals.push("example-heading-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 40;
            signals.push("example-summary-match".to_string());
        }
        if retrieval_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("example-retrieval-match".to_string());
        }
        out.push(DocsContextExample {
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            example_kind,
            section_heading,
            language_hint,
            summary_text,
            example_text,
            retrieval_weight: weight,
            token_estimate,
            signals,
        });
    }
    Ok(out)
}

fn load_context_examples_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextExample>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                e.page_title, e.example_kind, e.section_heading, e.language_hint,
                e.summary_text, e.example_text, e.token_estimate, e.retrieval_text
         FROM docs_examples e
         JOIN docs_corpora c ON c.corpus_id = e.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (
                lower(COALESCE(e.section_heading, '')) LIKE ?2
             OR lower(e.language_hint) LIKE ?2
             OR lower(e.summary_text) LIKE ?2
             OR lower(e.example_text) LIKE ?2
             OR lower(e.retrieval_text) LIKE ?2
           )
         ORDER BY e.page_title ASC, e.example_index ASC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(params![profile, context.like_pattern, limit_i64], |row| {
        let token_estimate: i64 = row.get(9)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            usize::try_from(token_estimate).unwrap_or(0),
            row.get::<_, String>(10)?,
        ))
    })?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            example_kind,
            section_heading,
            language_hint,
            summary_text,
            example_text,
            token_estimate,
            retrieval_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 80usize;
        if let Some(heading) = &section_heading
            && heading.to_ascii_lowercase().contains(&context.query_lower)
        {
            weight += 40;
            signals.push("example-heading-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 40;
            signals.push("example-summary-match".to_string());
        }
        if retrieval_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("example-retrieval-match".to_string());
        }
        out.push(DocsContextExample {
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            example_kind,
            section_heading,
            language_hint,
            summary_text,
            example_text,
            retrieval_weight: weight,
            token_estimate,
            signals,
        });
    }
    Ok(out)
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

fn make_snippet(content: &str, lowered_query: &str) -> String {
    let normalized = normalize_title(content);
    if normalized.is_empty() {
        return "<empty>".to_string();
    }
    let lowered = normalized.to_ascii_lowercase();
    let Some(index) = lowered.find(lowered_query) else {
        return truncate_text(&normalized, 200);
    };

    let start = clamp_to_char_boundary(&normalized, index.saturating_sub(80));
    let end = clamp_to_char_boundary(
        &normalized,
        index
            .saturating_add(lowered_query.len())
            .saturating_add(120)
            .min(normalized.len()),
    );
    let mut snippet = normalized[start..end].trim().to_string();
    if start > 0 {
        snippet = format!("...{snippet}");
    }
    if end < normalized.len() {
        snippet.push_str("...");
    }
    snippet
}

fn truncate_text(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let end = clamp_to_char_boundary(value, max_len);
    format!("{}...", &value[..end])
}

fn clamp_to_char_boundary(value: &str, mut index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    while !value.is_char_boundary(index) {
        index = index.saturating_sub(1);
    }
    index
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        DOCS_NAMESPACE_MANUAL, DocsApi, RemoteDocsPage, TechnicalDocType, TechnicalImportTask,
        collect_pages_for_technical_task,
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
