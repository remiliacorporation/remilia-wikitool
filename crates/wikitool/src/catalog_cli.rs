use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use wikitool_core::catalog::authoring::AuthoringContractProfile;
use wikitool_core::catalog::content_index::RebuildReport;
use wikitool_core::catalog::status::CatalogStatus;
use wikitool_core::docs::DocsImportProfileReport;

use crate::RuntimeOptions;
use crate::briefs::BriefView;
use crate::catalog_inspect_cli;
use crate::cli_support::OutputFormat;

mod build;
mod contracts;
pub(crate) mod shared;
mod status;
mod surface;
mod warm;

use warm::run_catalog_warm;
#[derive(Debug, Args)]
pub(crate) struct CatalogArgs {
    #[command(subcommand)]
    command: CatalogSubcommand,
}

#[derive(Debug, Subcommand)]
enum CatalogSubcommand {
    #[command(about = "Rebuild the local content catalog")]
    Build(CatalogBuildArgs),
    #[command(about = "Build the content catalog and hydrate a docs profile")]
    Warm(CatalogWarmArgs),
    #[command(about = "Report catalog readiness and degradations")]
    Status(CatalogStatusArgs),
    #[command(about = "Plan and search token-budgeted authoring contracts")]
    Contracts(CatalogContractsArgs),
    #[command(about = "Inspect indexed catalog structures directly")]
    Inspect(catalog_inspect_cli::CatalogInspectArgs),
    #[command(
        about = "Build the derived agent-facing template, module, asset, and extension surface"
    )]
    Surface(CatalogSurfaceArgs),
}

#[derive(Debug, Args)]
pub(crate) struct CatalogSurfaceArgs {
    #[command(subcommand)]
    command: CatalogSurfaceSubcommand,
}

#[derive(Debug, Subcommand)]
enum CatalogSurfaceSubcommand {
    #[command(about = "Refresh and show the derived authoring surface")]
    Sync(CatalogSurfaceFormatArgs),
    #[command(about = "Show the current derived authoring surface")]
    Show(CatalogSurfaceFormatArgs),
}

#[derive(Debug, Args)]
struct CatalogSurfaceFormatArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
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
    #[arg(long = "template-limit", default_value_t = 64, value_name = "N")]
    template_limit: usize,
    #[arg(long = "template-example-limit", default_value_t = 2, value_name = "N")]
    template_example_limit: usize,
    #[arg(long = "module-limit", default_value_t = 128, value_name = "N")]
    module_limit: usize,
    #[arg(long = "asset-limit", default_value_t = 128, value_name = "N")]
    asset_limit: usize,
    #[arg(long = "extension-limit", default_value_t = 128, value_name = "N")]
    extension_limit: usize,
    #[arg(long = "extension-tag-limit", default_value_t = 128, value_name = "N")]
    extension_tag_limit: usize,
    #[arg(
        long = "parser-function-limit",
        default_value_t = 128,
        value_name = "N"
    )]
    parser_function_limit: usize,
}

#[derive(Debug, Args)]
pub(crate) struct CatalogBuildArgs {
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
pub(crate) struct CatalogWarmArgs {
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Docs profile to hydrate (default: configured site adapter)"
    )]
    pub(crate) docs_profile: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = CatalogWarmDocsMode::Missing,
        value_name = "MODE",
        help = "Docs hydration mode: missing|refresh|skip"
    )]
    pub(crate) docs_mode: CatalogWarmDocsMode,
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
pub(crate) enum CatalogWarmDocsMode {
    Missing,
    Refresh,
    Skip,
}

#[derive(Debug, Args)]
pub(crate) struct CatalogStatusArgs {
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Docs profile to assess (default: configured site adapter)"
    )]
    docs_profile: Option<String>,
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
pub(crate) struct CatalogContractsArgs {
    #[command(subcommand)]
    command: CatalogContractsSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum CatalogContractsSubcommand {
    #[command(about = "Search the indexed authoring contract graph")]
    Search(CatalogContractsSearchArgs),
    #[command(about = "Plan contract traversal for a topic or draft")]
    Plan(CatalogContractsPlanArgs),
}

#[derive(Debug, Args, Clone)]
struct CatalogContractsSearchArgs {
    #[arg(value_name = "QUERY", help = "Template/module/authoring surface query")]
    query: String,
    #[arg(long, default_value_t = 16, value_name = "N")]
    limit: usize,
    #[arg(long, default_value_t = 900, value_name = "TOKENS")]
    token_budget: usize,
    #[arg(
        long,
        value_enum,
        default_value_t = CatalogContractProfileArg::Author,
        value_name = "PROFILE",
        help = "Contract traversal profile: index|author|implementation"
    )]
    profile: CatalogContractProfileArg,
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
struct CatalogContractsPlanArgs {
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
        default_value_t = CatalogContractProfileArg::Author,
        value_name = "PROFILE",
        help = "Contract traversal profile: index|author|implementation"
    )]
    profile: CatalogContractProfileArg,
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
enum CatalogContractProfileArg {
    Index,
    Author,
    Implementation,
}

impl From<CatalogContractProfileArg> for AuthoringContractProfile {
    fn from(value: CatalogContractProfileArg) -> Self {
        match value {
            CatalogContractProfileArg::Index => Self::Index,
            CatalogContractProfileArg::Author => Self::Author,
            CatalogContractProfileArg::Implementation => Self::Implementation,
        }
    }
}

#[derive(Debug, Serialize)]
struct CatalogBuildReport {
    rebuild: RebuildReport,
    status: CatalogStatus,
}

#[derive(Debug, Serialize)]
struct CatalogWarmReport {
    rebuild: RebuildReport,
    docs_action: &'static str,
    docs: DocsImportProfileReport,
    status: CatalogStatus,
}

pub(crate) fn run_catalog(runtime: &RuntimeOptions, args: CatalogArgs) -> Result<()> {
    match args.command {
        CatalogSubcommand::Build(args) => build::run_catalog_build(runtime, args),
        CatalogSubcommand::Warm(args) => run_catalog_warm(runtime, args),
        CatalogSubcommand::Status(args) => status::run_catalog_status(runtime, args),
        CatalogSubcommand::Contracts(args) => contracts::run_catalog_contracts(runtime, args),
        CatalogSubcommand::Inspect(args) => catalog_inspect_cli::run_catalog_inspect(runtime, args),
        CatalogSubcommand::Surface(args) => surface::run_catalog_surface(runtime, args),
    }
}
