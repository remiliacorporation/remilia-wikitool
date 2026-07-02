use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

use crate::RuntimeOptions;
use crate::briefs::BriefView;
use crate::cli_support::OutputFormat;

mod capabilities;
mod cargo;
mod output;
mod profile;
mod rules;
mod summary;
mod surface;

#[cfg(test)]
mod tests;
#[derive(Debug, Args)]
pub(crate) struct WikiArgs {
    #[command(subcommand)]
    command: WikiSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiSubcommand {
    #[command(about = "Sync and inspect live wiki capability manifests")]
    Capabilities(WikiCapabilitiesArgs),
    #[command(about = "Query the live wiki's Cargo extension tables")]
    Cargo(WikiCargoArgs),
    #[command(about = "Show the combined live/profile-aware wiki surface")]
    Profile(WikiProfileArgs),
    #[command(about = "Show the structured local editorial rules overlay")]
    Rules(WikiRulesArgs),
    #[command(
        about = "Show the agent-facing template, module, asset, and extension authoring surface"
    )]
    Surface(WikiSurfaceArgs),
}

#[derive(Debug, Args)]
pub(crate) struct WikiCargoArgs {
    #[command(subcommand)]
    command: WikiCargoSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiCargoSubcommand {
    #[command(about = "List the live wiki's Cargo tables")]
    Tables(WikiCargoTablesArgs),
    #[command(about = "Show a live Cargo table's field schema (names, types, list markers)")]
    Fields(WikiCargoFieldsArgs),
    #[command(about = "Fetch rows from a live Cargo table")]
    Rows(WikiCargoRowsArgs),
    #[command(about = "Count rows in a live Cargo table")]
    Count(WikiCargoCountArgs),
}

#[derive(Debug, Args)]
struct WikiCargoTablesArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct WikiCargoFieldsArgs {
    #[arg(value_name = "TABLE", help = "Cargo table name")]
    table: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct WikiCargoRowsArgs {
    #[arg(value_name = "TABLE", help = "Cargo table name")]
    table: String,
    #[arg(
        long = "field",
        value_name = "FIELD",
        help = "Field to select (repeat or comma-separate); defaults to the table's full schema"
    )]
    fields: Vec<String>,
    #[arg(
        long = "where",
        value_name = "CLAUSE",
        help = "Cargo where clause, e.g. collection='Milady Maker'"
    )]
    where_clause: Option<String>,
    #[arg(
        long = "order-by",
        value_name = "CLAUSE",
        help = "Cargo order_by clause"
    )]
    order_by: Option<String>,
    #[arg(
        long,
        default_value_t = 10,
        value_name = "N",
        help = "Maximum rows to return"
    )]
    limit: usize,
    #[arg(long, default_value_t = 0, value_name = "N", help = "Row offset")]
    offset: usize,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct WikiCargoCountArgs {
    #[arg(value_name = "TABLE", help = "Cargo table name to count rows in")]
    table: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct WikiCapabilitiesArgs {
    #[command(subcommand)]
    command: WikiCapabilitiesSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiCapabilitiesSubcommand {
    #[command(about = "Fetch and store the current live wiki capability manifest")]
    Sync(WikiCapabilitiesFormatArgs),
    #[command(about = "Show the last stored wiki capability manifest")]
    Show(WikiCapabilitiesFormatArgs),
}

#[derive(Debug, Args)]
struct WikiCapabilitiesFormatArgs {
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
        default_value_t = WikiJsonView::Summary,
        value_name = "VIEW",
        help = "JSON view: summary|full"
    )]
    view: WikiJsonView,
}

#[derive(Debug, Args)]
struct WikiFormatArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum WikiJsonView {
    Summary,
    Full,
}

impl WikiJsonView {
    fn is_full(self) -> bool {
        self == Self::Full
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Full => "full",
        }
    }
}

impl std::fmt::Display for WikiJsonView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Args)]
pub(crate) struct WikiProfileArgs {
    #[command(subcommand)]
    command: WikiProfileSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiProfileSubcommand {
    #[command(about = "Refresh the local rules overlay and live capability snapshot")]
    Sync(WikiCapabilitiesFormatArgs),
    #[command(about = "Show the current combined profile snapshot")]
    Show(WikiCapabilitiesFormatArgs),
    #[command(about = "Inspect a remote target wiki capability profile without storing it locally")]
    Remote(WikiRemoteProfileArgs),
}

#[derive(Debug, Args)]
struct WikiRemoteProfileArgs {
    url: String,
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
        default_value_t = WikiJsonView::Summary,
        value_name = "VIEW",
        help = "JSON view: summary|full"
    )]
    view: WikiJsonView,
}

#[derive(Debug, Args)]
pub(crate) struct WikiRulesArgs {
    #[command(subcommand)]
    command: WikiRulesSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiRulesSubcommand {
    #[command(about = "Show the current profile rules overlay")]
    Show(WikiFormatArgs),
}

#[derive(Debug, Args)]
pub(crate) struct WikiSurfaceArgs {
    #[command(subcommand)]
    command: WikiSurfaceSubcommand,
}

#[derive(Debug, Subcommand)]
enum WikiSurfaceSubcommand {
    #[command(about = "Refresh and show the agent-facing authoring surface")]
    Sync(WikiSurfaceFormatArgs),
    #[command(about = "Show the current agent-facing authoring surface")]
    Show(WikiSurfaceFormatArgs),
}

#[derive(Debug, Args)]
struct WikiSurfaceFormatArgs {
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

pub(crate) fn run_wiki(runtime: &RuntimeOptions, args: WikiArgs) -> Result<()> {
    match args.command {
        WikiSubcommand::Capabilities(args) => capabilities::run_wiki_capabilities(runtime, args),
        WikiSubcommand::Cargo(args) => cargo::run_wiki_cargo(runtime, args),
        WikiSubcommand::Profile(args) => profile::run_wiki_profile(runtime, args),
        WikiSubcommand::Rules(args) => rules::run_wiki_rules(runtime, args),
        WikiSubcommand::Surface(args) => surface::run_wiki_surface(runtime, args),
    }
}
