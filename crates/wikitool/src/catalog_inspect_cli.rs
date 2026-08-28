use anyhow::Result;
use clap::{Args, Subcommand};

use crate::RuntimeOptions;
use crate::briefs::BriefView;
use crate::cli_support::OutputFormat;

mod backlinks;
mod chunks;
mod pages;
mod references;
mod templates;
#[derive(Debug, Args)]
pub(crate) struct CatalogInspectArgs {
    #[command(subcommand)]
    command: CatalogInspectSubcommand,
}

#[derive(Debug, Subcommand)]
enum CatalogInspectSubcommand {
    /// Show index statistics
    Stats {
        #[arg(
            long,
            value_enum,
            default_value_t = OutputFormat::Text,
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: OutputFormat,
    },
    /// Retrieve token-budgeted content chunks from indexed pages
    Chunks {
        title: Option<String>,
        #[arg(
            long,
            value_name = "QUERY",
            help = "Optional relevance query applied to chunk retrieval"
        )]
        query: Option<String>,
        #[arg(
            long,
            help = "Retrieve chunks across indexed pages (query required, omit TITLE)"
        )]
        across_pages: bool,
        #[arg(
            long,
            default_value_t = 8,
            value_name = "N",
            help = "Maximum number of chunks to return"
        )]
        limit: usize,
        #[arg(
            long,
            default_value_t = 720,
            value_name = "TOKENS",
            help = "Token budget across returned chunks"
        )]
        token_budget: usize,
        #[arg(
            long,
            default_value_t = 12,
            value_name = "N",
            help = "Maximum distinct source pages in across-pages mode"
        )]
        max_pages: usize,
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
        #[arg(long, help = "Enable lexical de-duplication and diversification")]
        diversify: bool,
        #[arg(long, help = "Disable lexical de-duplication and diversification")]
        no_diversify: bool,
    },
    /// Show indexed pages that link to a title
    Backlinks {
        title: String,
        #[arg(
            long,
            value_enum,
            default_value_t = OutputFormat::Text,
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: OutputFormat,
    },
    /// Inspect active template usage and implementation references
    Templates {
        #[arg(value_name = "TEMPLATE", help = "Optional specific template title")]
        template: Option<String>,
        #[arg(
            long,
            default_value_t = 40,
            value_name = "N",
            help = "Maximum templates to return in catalog mode"
        )]
        limit: usize,
        #[arg(long, help = "Return the full active template catalog")]
        all: bool,
        #[arg(
            long,
            value_enum,
            default_value_t = OutputFormat::Text,
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: OutputFormat,
    },
    /// Audit indexed references for cleanup work
    References(references::ReferenceInspectArgs),
    /// Show indexed pages with no backlinks
    Orphans {
        #[arg(
            long,
            value_enum,
            default_value_t = OutputFormat::Text,
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: OutputFormat,
    },
    #[command(name = "empty-categories")]
    /// Show categories with no indexed members
    EmptyCategories {
        #[arg(
            long,
            value_enum,
            default_value_t = OutputFormat::Text,
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: OutputFormat,
    },
}

pub(crate) fn run_catalog_inspect(
    runtime: &RuntimeOptions,
    args: CatalogInspectArgs,
) -> Result<()> {
    match args.command {
        CatalogInspectSubcommand::Stats { format } => pages::run_inspect_stats(runtime, format),
        CatalogInspectSubcommand::Chunks {
            title,
            query,
            across_pages,
            limit,
            token_budget,
            max_pages,
            format,
            view,
            diversify,
            no_diversify,
        } => chunks::run_inspect_chunks(
            runtime,
            title.as_deref(),
            query.as_deref(),
            across_pages,
            limit,
            token_budget,
            max_pages,
            format,
            view,
            diversify,
            no_diversify,
        ),
        CatalogInspectSubcommand::Backlinks { title, format } => {
            backlinks::run_inspect_backlinks(runtime, &title, format)
        }
        CatalogInspectSubcommand::Templates {
            template,
            limit,
            all,
            format,
        } => templates::run_inspect_templates(runtime, template.as_deref(), limit, all, format),
        CatalogInspectSubcommand::References(args) => {
            references::run_inspect_references(runtime, args)
        }
        CatalogInspectSubcommand::Orphans { format } => pages::run_inspect_orphans(runtime, format),
        CatalogInspectSubcommand::EmptyCategories { format } => {
            pages::run_inspect_empty_categories(runtime, format)
        }
    }
}
