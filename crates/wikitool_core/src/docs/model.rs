use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(super) const DOCS_NAMESPACE_HELP: i32 = 12;
pub(super) const DOCS_NAMESPACE_MANUAL: i32 = 100;
pub(super) const DOCS_NAMESPACE_EXTENSION: i32 = 102;
pub(super) const DOCS_NAMESPACE_API: i32 = 104;
pub(super) const DOCS_CACHE_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
pub(super) const DOCS_SUBPAGE_LIMIT_DEFAULT: usize = 100;
pub(super) const DOCS_BUNDLE_SCHEMA_VERSION: u32 = 1;

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

    pub(super) fn main_page(self) -> &'static str {
        match self {
            Self::Hooks => "Manual:Hooks",
            Self::Config => "Manual:Configuration settings",
            Self::Api => "API:Main page",
            Self::Manual => "Manual:Contents",
            Self::Help => "Help:Contents",
        }
    }

    pub(super) fn subpage_prefix(self) -> &'static str {
        match self {
            Self::Hooks => "Manual:Hooks/",
            Self::Config => "Manual:$wg",
            Self::Api => "API:",
            Self::Manual => "Manual:",
            Self::Help => "Help:",
        }
    }

    pub(super) fn namespace(self) -> i32 {
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
pub(super) struct FetchedDocsPage {
    pub(super) page_title: String,
    pub(super) alias_titles: Vec<String>,
    pub(super) local_path: String,
    pub(super) content: String,
}

#[derive(Debug, Clone)]
pub(super) struct CorpusDescriptor {
    pub(super) corpus_id: String,
    pub(super) corpus_kind: String,
    pub(super) label: String,
    pub(super) source_wiki: String,
    pub(super) source_version: String,
    pub(super) source_profile: String,
    pub(super) technical_type: String,
    pub(super) refresh_kind: String,
    pub(super) refresh_spec: String,
    pub(super) fetched_at_unix: u64,
    pub(super) expires_at_unix: u64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct PersistStats {
    pub(super) pages: usize,
    pub(super) sections: usize,
    pub(super) symbols: usize,
    pub(super) examples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ExtensionRefreshSpec {
    pub(super) extension_name: String,
    pub(super) include_subpages: bool,
    pub(super) source_profile: String,
    pub(super) source_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TechnicalRefreshSpec {
    pub(super) doc_type: String,
    pub(super) page_title: Option<String>,
    pub(super) include_subpages: bool,
    pub(super) limit: usize,
    pub(super) source_profile: String,
    pub(super) source_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ProfileRefreshSpec {
    pub(super) profile: String,
    pub(super) include_installed_extensions: bool,
    pub(super) include_extension_subpages: bool,
    pub(super) extra_extensions: Vec<String>,
    pub(super) limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StaticRefreshSpec {
    pub(super) source: String,
}

pub(super) struct DocsProfileDefinition {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) source_version: &'static str,
    pub(super) include_installed_extensions_by_default: bool,
    pub(super) page_seeds: &'static [ProfilePageSeed],
    pub(super) extension_seeds: &'static [&'static str],
}

pub(super) struct ProfilePageSeed {
    pub(super) title: &'static str,
    pub(super) include_subpages: bool,
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

pub(super) const DOCS_PROFILES: &[DocsProfileDefinition] = &[
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
