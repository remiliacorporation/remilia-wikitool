use std::path::PathBuf;

use clap::Args;

use crate::cli_support::OutputFormat;

mod delete;
mod diff;
mod init;
mod pull;
mod push;
mod shared;
mod status;

pub(crate) use delete::run_delete;
pub(crate) use diff::run_diff;
pub(crate) use init::run_init;
pub(crate) use pull::run_pull;
pub(crate) use push::run_push;
pub(crate) use status::run_status;

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    #[arg(
        long,
        value_name = "URL",
        help = "Target wiki base URL; defaults to https://wiki.remilia.org"
    )]
    pub(crate) wiki_url: Option<String>,
    #[arg(
        long,
        value_name = "URL",
        help = "Target MediaWiki API URL; defaults to https://wiki.remilia.org/api.php"
    )]
    pub(crate) api_url: Option<String>,
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
    #[arg(long, value_name = "TEXT", help = "Edit summary for pushed changes")]
    pub(crate) summary: Option<String>,
    #[arg(long, help = "Preview push actions without writing to the wiki")]
    pub(crate) dry_run: bool,
    #[arg(long, help = "Force push even when remote timestamps diverge")]
    pub(crate) force: bool,
    #[arg(long, help = "Propagate local deletions to remote wiki pages")]
    pub(crate) delete: bool,
    #[arg(long, help = "Include template/module/mediawiki namespaces")]
    pub(crate) templates: bool,
    #[arg(long, help = "Limit push to Category namespace pages")]
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
        help = "Custom backup directory under .wikitool/"
    )]
    pub(crate) backup_dir: Option<PathBuf>,
    #[arg(long, help = "Preview deletion without making changes")]
    pub(crate) dry_run: bool,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: OutputFormat,
}
