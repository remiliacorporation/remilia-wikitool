use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use wikitool_core::authoring::model::{ArticleStartIntent, ArticleStartResult};
use wikitool_core::docs::DocsImportProfileReport;
use wikitool_core::knowledge::authoring::AuthoringContractProfile;
use wikitool_core::knowledge::content_index::RebuildReport;
use wikitool_core::knowledge::status::{
    DEFAULT_DOCS_PROFILE, KnowledgeReadinessLevel, KnowledgeStatusReport,
};

use crate::RuntimeOptions;
use crate::briefs::BriefView;
use crate::cli_support::OutputFormat;
use crate::knowledge_inspect_cli;

mod article_start;
mod build;
mod contracts;
mod interview;
mod shared;
mod status;
mod warm;

pub(crate) use warm::run_knowledge_warm;
#[derive(Debug, Args)]
pub(crate) struct KnowledgeArgs {
    #[command(subcommand)]
    command: KnowledgeSubcommand,
}

#[derive(Debug, Subcommand)]
enum KnowledgeSubcommand {
    #[command(about = "Rebuild the local content knowledge index")]
    Build(KnowledgeBuildArgs),
    #[command(about = "Build content knowledge and hydrate a docs profile")]
    Warm(KnowledgeWarmArgs),
    #[command(about = "Report knowledge readiness and degradations")]
    Status(KnowledgeStatusArgs),
    #[command(about = "Assemble an interpreted authoring brief for a topic")]
    ArticleStart(KnowledgeArticleStartArgs),
    #[command(about = "Plan and search token-budgeted authoring contracts")]
    Contracts(KnowledgeContractsArgs),
    #[command(about = "Create, validate, show, and audit knowledge interview briefs")]
    Interview(interview::KnowledgeInterviewArgs),
    #[command(about = "Inspect indexed knowledge structures directly")]
    Inspect(knowledge_inspect_cli::KnowledgeInspectArgs),
}

#[derive(Debug, Args)]
pub(crate) struct KnowledgeBuildArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct KnowledgeWarmArgs {
    #[arg(
        long,
        default_value = DEFAULT_DOCS_PROFILE,
        value_name = "PROFILE",
        help = "Docs profile to hydrate during warmup"
    )]
    pub(crate) docs_profile: String,
    #[arg(
        long,
        value_enum,
        default_value_t = KnowledgeWarmDocsMode::Missing,
        value_name = "MODE",
        help = "Docs hydration mode: missing|refresh|skip"
    )]
    pub(crate) docs_mode: KnowledgeWarmDocsMode,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum KnowledgeWarmDocsMode {
    Missing,
    Refresh,
    Skip,
}

#[derive(Debug, Args)]
pub(crate) struct KnowledgeStatusArgs {
    #[arg(
        long,
        default_value = DEFAULT_DOCS_PROFILE,
        value_name = "PROFILE",
        help = "Docs profile to assess for authoring readiness"
    )]
    docs_profile: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct KnowledgeArticleStartArgs {
    #[arg(
        value_name = "TOPIC",
        help = "Primary article topic/title for retrieval"
    )]
    topic: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional stub wikitext file used for link/template hint extraction"
    )]
    stub_path: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional knowledge interview brief to validate and include in the authoring brief"
    )]
    brief_path: Option<PathBuf>,
    #[arg(
        long,
        default_value_t = 45,
        value_name = "DAYS",
        help = "Age in days after which an interview brief is considered stale"
    )]
    brief_stale_days: u64,
    #[arg(
        long,
        default_value_t = 18,
        value_name = "N",
        help = "Maximum related pages in the brief"
    )]
    related_limit: usize,
    #[arg(
        long,
        default_value_t = 10,
        value_name = "N",
        help = "Maximum retrieved context chunks"
    )]
    chunk_limit: usize,
    #[arg(
        long,
        default_value_t = 1200,
        value_name = "TOKENS",
        help = "Token budget across retrieved chunks"
    )]
    token_budget: usize,
    #[arg(
        long,
        default_value_t = 8,
        value_name = "N",
        help = "Maximum distinct source pages in chunk retrieval"
    )]
    max_pages: usize,
    #[arg(
        long,
        default_value_t = 18,
        value_name = "N",
        help = "Maximum internal link suggestions"
    )]
    link_limit: usize,
    #[arg(
        long,
        default_value_t = 8,
        value_name = "N",
        help = "Maximum category suggestions"
    )]
    category_limit: usize,
    #[arg(
        long,
        default_value_t = 16,
        value_name = "N",
        help = "Maximum template summaries"
    )]
    template_limit: usize,
    #[arg(
        long,
        default_value = DEFAULT_DOCS_PROFILE,
        value_name = "PROFILE",
        help = "Docs profile to use for bridged authoring retrieval"
    )]
    docs_profile: String,
    #[arg(
        long,
        value_enum,
        default_value_t = AuthoringContractProfileArg::Author,
        value_name = "PROFILE",
        help = "Contract traversal profile: index|author|implementation"
    )]
    contract_profile: AuthoringContractProfileArg,
    #[arg(
        long,
        value_name = "QUERY",
        help = "Optional contract traversal query separate from TOPIC, such as \"species infobox taxonomy\""
    )]
    contract_query: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
    #[arg(
        long,
        value_enum,
        default_value_t = BriefView::Brief,
        value_name = "VIEW",
        help = "JSON view: brief|full"
    )]
    view: BriefView,
    #[arg(
        long,
        value_enum,
        default_value_t = ArticleStartIntentArg::New,
        value_name = "INTENT",
        help = "Authoring intent: new|expand|audit|refresh"
    )]
    intent: ArticleStartIntentArg,
    #[arg(long, help = "Enable lexical chunk de-duplication and diversification")]
    diversify: bool,
    #[arg(
        long,
        help = "Disable lexical chunk de-duplication and diversification"
    )]
    no_diversify: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct KnowledgeContractsArgs {
    #[command(subcommand)]
    command: KnowledgeContractsSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum KnowledgeContractsSubcommand {
    #[command(about = "Search the indexed authoring contract graph")]
    Search(KnowledgeContractsSearchArgs),
    #[command(about = "Plan contract traversal for a topic or draft")]
    Plan(KnowledgeContractsPlanArgs),
}

#[derive(Debug, Args, Clone)]
struct KnowledgeContractsSearchArgs {
    #[arg(value_name = "QUERY", help = "Template/module/authoring surface query")]
    query: String,
    #[arg(long, default_value_t = 16, value_name = "N")]
    limit: usize,
    #[arg(long, default_value_t = 900, value_name = "TOKENS")]
    token_budget: usize,
    #[arg(
        long,
        value_enum,
        default_value_t = AuthoringContractProfileArg::Author,
        value_name = "PROFILE",
        help = "Contract traversal profile: index|author|implementation"
    )]
    profile: AuthoringContractProfileArg,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args, Clone)]
struct KnowledgeContractsPlanArgs {
    #[arg(
        value_name = "TOPIC",
        help = "Primary article topic/title for traversal"
    )]
    topic: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional stub wikitext file used for template seeds"
    )]
    stub_path: Option<PathBuf>,
    #[arg(long, default_value_t = 16, value_name = "N")]
    limit: usize,
    #[arg(long, default_value_t = 900, value_name = "TOKENS")]
    token_budget: usize,
    #[arg(
        long,
        value_enum,
        default_value_t = AuthoringContractProfileArg::Author,
        value_name = "PROFILE",
        help = "Contract traversal profile: index|author|implementation"
    )]
    profile: AuthoringContractProfileArg,
    #[arg(
        long,
        value_name = "QUERY",
        help = "Optional contract traversal query separate from TOPIC"
    )]
    contract_query: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ArticleStartIntentArg {
    New,
    Expand,
    Audit,
    Refresh,
}

impl From<ArticleStartIntentArg> for ArticleStartIntent {
    fn from(value: ArticleStartIntentArg) -> Self {
        match value {
            ArticleStartIntentArg::New => Self::New,
            ArticleStartIntentArg::Expand => Self::Expand,
            ArticleStartIntentArg::Audit => Self::Audit,
            ArticleStartIntentArg::Refresh => Self::Refresh,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum AuthoringContractProfileArg {
    Index,
    Author,
    Implementation,
}

impl From<AuthoringContractProfileArg> for AuthoringContractProfile {
    fn from(value: AuthoringContractProfileArg) -> Self {
        match value {
            AuthoringContractProfileArg::Index => Self::Index,
            AuthoringContractProfileArg::Author => Self::Author,
            AuthoringContractProfileArg::Implementation => Self::Implementation,
        }
    }
}

#[derive(Debug, Serialize)]
struct KnowledgeBuildReport {
    rebuild: RebuildReport,
    status: KnowledgeStatusReport,
}

#[derive(Debug, Serialize)]
struct KnowledgeWarmReport {
    rebuild: RebuildReport,
    docs_action: &'static str,
    docs: DocsImportProfileReport,
    status: KnowledgeStatusReport,
}

#[derive(Debug, Serialize)]
struct KnowledgeArticleStartOutput {
    docs_profile_requested: String,
    readiness: KnowledgeReadinessLevel,
    degradations: Vec<String>,
    knowledge_generation: String,
    interview_brief: Option<wikitool_core::knowledge_interview::InterviewValidationReport>,
    result: KnowledgeArticleStartPayload,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum KnowledgeArticleStartPayload {
    IndexMissing,
    QueryMissing,
    Found {
        article_start: Box<ArticleStartResult>,
    },
}

pub(crate) fn run_knowledge(runtime: &RuntimeOptions, args: KnowledgeArgs) -> Result<()> {
    match args.command {
        KnowledgeSubcommand::Build(args) => build::run_knowledge_build(runtime, args),
        KnowledgeSubcommand::Warm(args) => run_knowledge_warm(runtime, args),
        KnowledgeSubcommand::Status(args) => status::run_knowledge_status(runtime, args),
        KnowledgeSubcommand::ArticleStart(args) => {
            article_start::run_knowledge_article_start(runtime, args)
        }
        KnowledgeSubcommand::Contracts(args) => contracts::run_knowledge_contracts(runtime, args),
        KnowledgeSubcommand::Interview(args) => interview::run_knowledge_interview(runtime, args),
        KnowledgeSubcommand::Inspect(args) => {
            knowledge_inspect_cli::run_knowledge_inspect(runtime, args)
        }
    }
}
