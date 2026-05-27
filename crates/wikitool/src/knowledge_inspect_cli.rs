use anyhow::Result;
use clap::{Args, Subcommand};

use crate::RuntimeOptions;
use crate::cli_support::OutputFormat;

mod backlinks;
mod chunks;
mod pages;
mod references;
mod templates;
#[derive(Debug, Args)]
pub(crate) struct KnowledgeInspectArgs {
    #[command(subcommand)]
    command: KnowledgeInspectSubcommand,
}

#[derive(Debug, Subcommand)]
enum KnowledgeInspectSubcommand {
    /// Show index statistics
    Stats,
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
    Orphans,
    #[command(name = "empty-categories")]
    /// Show categories with no indexed members
    EmptyCategories,
}

pub(crate) fn run_knowledge_inspect(
    runtime: &RuntimeOptions,
    args: KnowledgeInspectArgs,
) -> Result<()> {
    match args.command {
        KnowledgeInspectSubcommand::Stats => pages::run_inspect_stats(runtime),
        KnowledgeInspectSubcommand::Chunks {
            title,
            query,
            across_pages,
            limit,
            token_budget,
            max_pages,
            format,
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
            diversify,
            no_diversify,
        ),
        KnowledgeInspectSubcommand::Backlinks { title, format } => {
            backlinks::run_inspect_backlinks(runtime, &title, format)
        }
        KnowledgeInspectSubcommand::Templates {
            template,
            limit,
            all,
            format,
        } => templates::run_inspect_templates(runtime, template.as_deref(), limit, all, format),
        KnowledgeInspectSubcommand::References(args) => {
            references::run_inspect_references(runtime, args)
        }
        KnowledgeInspectSubcommand::Orphans => pages::run_inspect_orphans(runtime),
        KnowledgeInspectSubcommand::EmptyCategories => pages::run_inspect_empty_categories(runtime),
    }
}
