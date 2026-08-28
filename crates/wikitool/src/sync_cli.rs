use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use crate::cli_support::OutputFormat;

mod delete;
mod diff;
mod init;
mod mutation;
mod pull;
mod push;
mod shared;
mod status;

pub(crate) use delete::run_delete;
pub(crate) use diff::run_diff;
pub(crate) use init::run_init;
pub(crate) use mutation::run_mutation;
pub(crate) use pull::run_pull;
pub(crate) use push::run_push;
pub(crate) use status::run_status;

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    #[arg(long, value_name = "URL", help = "Target wiki base URL")]
    pub(crate) wiki_url: Option<String>,
    #[arg(long, value_name = "URL", help = "Target MediaWiki API URL")]
    pub(crate) api_url: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Project-relative site-adapter path to validate and store in project config"
    )]
    pub(crate) adapter_path: Option<PathBuf>,
    #[arg(long, help = "Create templates/ during initialization")]
    pub(crate) templates: bool,
    #[arg(long, help = "Overwrite existing config/parser files")]
    pub(crate) force: bool,
    #[arg(long, help = "Skip writing .wikitool/config.toml")]
    pub(crate) no_config: bool,
    #[arg(long, help = "Skip writing parser config")]
    pub(crate) no_parser_config: bool,
    #[arg(long, help = "Skip network namespace discovery during initialization")]
    pub(crate) no_network: bool,
}

#[derive(Debug, Args)]
pub(crate) struct PullArgs {
    #[arg(long, help = "Full refresh (ignore last pull timestamp)")]
    pub(crate) full: bool,
    #[arg(long, help = "Overwrite locally modified files during pull")]
    pub(crate) overwrite_local: bool,
    #[arg(short = 'c', long, value_name = "NAME", help = "Filter by category")]
    pub(crate) category: Option<String>,
    #[arg(long, help = "Pull templates instead of articles")]
    pub(crate) templates: bool,
    #[arg(long, help = "Pull Category: namespace pages")]
    pub(crate) categories: bool,
    #[arg(long, help = "Pull everything (articles, categories, and templates)")]
    pub(crate) all: bool,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct PushArgs {
    #[arg(
        long,
        value_name = "TEXT",
        help = "Edit summary for the bound push plan"
    )]
    pub(crate) summary: String,
    #[arg(
        long,
        value_name = "PLAN_ID",
        help = "Apply the exact plan ID returned by a current preview; without this option the command only previews"
    )]
    pub(crate) apply: Option<String>,
    #[arg(long, help = "Force push even when remote timestamps diverge")]
    pub(crate) force: bool,
    #[arg(long, help = "Propagate local deletions to remote wiki pages")]
    pub(crate) delete: bool,
    #[arg(long, help = "Include template/module/mediawiki namespaces")]
    pub(crate) templates: bool,
    #[arg(long, help = "Limit push to Category namespace pages")]
    pub(crate) categories: bool,
    #[arg(
        long,
        help = "Explicitly include every eligible current change (cannot be combined with title/path selection)"
    )]
    pub(crate) all: bool,
    #[arg(long = "title", value_name = "TITLE")]
    pub(crate) titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    pub(crate) paths: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    pub(crate) titles_file: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct DiffArgs {
    #[arg(long, help = "Include template/module/mediawiki namespaces")]
    pub(crate) templates: bool,
    #[arg(long, help = "Limit diff to Category namespace pages")]
    pub(crate) categories: bool,
    #[arg(long, help = "Show hash-level details for modified entries")]
    pub(crate) verbose: bool,
    #[arg(
        long,
        help = "Render unified textual diffs against the last synced baseline"
    )]
    pub(crate) content: bool,
    #[arg(long = "title", value_name = "TITLE")]
    pub(crate) titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    pub(crate) paths: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    pub(crate) titles_file: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct StatusArgs {
    #[arg(long, help = "Only show modified")]
    pub(crate) modified: bool,
    #[arg(long, help = "Only show conflicts")]
    pub(crate) conflicts: bool,
    #[arg(long, help = "Include templates")]
    pub(crate) templates: bool,
    #[arg(long, help = "Limit status to Category namespace pages")]
    pub(crate) categories: bool,
    #[arg(long = "title", value_name = "TITLE")]
    pub(crate) titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    pub(crate) paths: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    pub(crate) titles_file: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    pub(crate) title: String,
    #[arg(long, value_name = "TEXT", help = "Reason for deletion (required)")]
    pub(crate) reason: String,
    #[arg(long, help = "Skip backup (not recommended)")]
    pub(crate) no_backup: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Custom backup directory under .wikitool/sync/"
    )]
    pub(crate) backup_dir: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PLAN_ID",
        help = "Apply the exact target/content/revision-bound plan ID; without this option the command only previews"
    )]
    pub(crate) apply: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct MutationArgs {
    #[command(subcommand)]
    pub(crate) command: MutationCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum MutationCommand {
    #[command(about = "List target-bound durable remote mutation receipts")]
    List {
        #[arg(
            long,
            help = "Include terminal receipts as well as unresolved mutations"
        )]
        all: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    #[command(about = "Show one target-bound remote mutation receipt")]
    Show {
        #[arg(value_enum)]
        operation: MutationOperationArg,
        mutation_id: i64,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    #[command(about = "Reconcile one mutation without replaying its write")]
    Reconcile {
        #[arg(value_enum)]
        operation: MutationOperationArg,
        mutation_id: i64,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    #[command(
        about = "Close an irreconcilable mutation without claiming a remote outcome",
        long_about = "Close an irreconcilable target-bound mutation, retain its evidence, and invalidate the title's sync baseline. A fresh target-bound pull is required before another write."
    )]
    Close {
        #[arg(value_enum)]
        operation: MutationOperationArg,
        mutation_id: i64,
        #[arg(long, value_name = "ACTOR", help = "Operator recording the closure")]
        actor: String,
        #[arg(
            long,
            value_name = "REASON",
            help = "Reason remote truth cannot be proved"
        )]
        reason: String,
        #[arg(
            long,
            help = "Confirm evidence-preserving closure and sync-baseline invalidation"
        )]
        confirm: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum MutationOperationArg {
    Edit,
    Delete,
}

impl From<MutationOperationArg> for wikitool_core::sync::RemoteMutationOperation {
    fn from(value: MutationOperationArg) -> Self {
        match value {
            MutationOperationArg::Edit => Self::Edit,
            MutationOperationArg::Delete => Self::Delete,
        }
    }
}
