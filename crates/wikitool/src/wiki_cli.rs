use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};

use crate::RuntimeOptions;
use crate::briefs::BriefView;
use crate::cli_support::OutputFormat;

mod capabilities;
mod cargo;
mod output;
mod render_check;
mod summary;

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
    #[command(about = "Validate rendered live HTML and scoped link contracts")]
    RenderCheck(WikiRenderCheckArgs),
}

#[derive(Debug, Args)]
struct WikiRenderCheckArgs {
    #[arg(
        value_name = "TITLE",
        help = "Live wiki page title to render and inspect"
    )]
    title: String,
    #[arg(
        long,
        value_name = "CLASS",
        help = "Inspect each rendered element carrying this CSS class as one scope"
    )]
    scope_class: Option<String>,
    #[arg(
        long = "expect-scopes",
        value_name = "N",
        help = "Require exactly N matching scope elements"
    )]
    expected_scope_count: Option<usize>,
    #[arg(
        long,
        help = "Require every scope to contain a non-crawler interactive link"
    )]
    require_interactive_link: bool,
    #[arg(
        long = "require-href-contains",
        value_name = "TEXT",
        help = "Require every scope to contain an interactive href with this text (repeatable)"
    )]
    required_href_substrings: Vec<String>,
    #[arg(
        long = "require-link-class",
        value_name = "CLASS",
        help = "Require every scope to contain an interactive link with this CSS class (repeatable)"
    )]
    required_link_classes: Vec<String>,
    #[arg(
        long = "require-page-image",
        value_name = "FILE",
        help = "Require the live PageImages/Popups representative file"
    )]
    required_page_image: Option<String>,
    #[arg(
        long,
        help = "Do not fail when rendered page text contains literal [[...]] wikitext"
    )]
    allow_literal_wikilinks: bool,
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
        help = "Cargo where clause, e.g. collection='Example Collection'"
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
    #[command(about = "Inspect a remote MediaWiki capability surface without storing it")]
    Remote(WikiRemoteCapabilitiesArgs),
}

#[derive(Debug, Args)]
struct WikiRemoteCapabilitiesArgs {
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

pub(crate) fn run_wiki(runtime: &RuntimeOptions, args: WikiArgs) -> Result<()> {
    match args.command {
        WikiSubcommand::Capabilities(args) => capabilities::run_wiki_capabilities(runtime, args),
        WikiSubcommand::Cargo(args) => cargo::run_wiki_cargo(runtime, args),
        WikiSubcommand::RenderCheck(args) => render_check::run_wiki_render_check(runtime, args),
    }
}
