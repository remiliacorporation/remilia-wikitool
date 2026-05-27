use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use wikitool_core::article_lint::ArticleFixApplyMode;

use crate::RuntimeOptions;
use crate::cli_support::OutputFormat;

mod fix;
mod lint;
mod output;
mod promote;
mod selection;

#[cfg(test)]
mod tests;

#[derive(Debug, Args)]
pub(crate) struct ArticleArgs {
    #[command(subcommand)]
    command: ArticleSubcommand,
}

#[derive(Debug, Subcommand)]
enum ArticleSubcommand {
    #[command(about = "Lint article wikitext against wiki/profile rules")]
    Lint(ArticleLintArgs),
    #[command(about = "Apply safe mechanical fixes to article wikitext")]
    Fix(ArticleFixArgs),
    #[command(about = "Copy a reviewed state draft into the sync tree")]
    Promote(ArticlePromoteArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ArticleLintArgs {
    #[arg(
        help = "Article path; state-draft paths under .wikitool/drafts/ may use --title override"
    )]
    path: Option<PathBuf>,
    #[arg(long, default_value = "remilia", value_name = "PROFILE")]
    profile: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
    #[arg(long, help = "Treat warnings as errors")]
    strict: bool,
    #[arg(
        long = "title",
        value_name = "TITLE",
        help = "Select a canonical article title; with one .wikitool/drafts/ PATH, override the draft title"
    )]
    titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    titles_file: Option<PathBuf>,
    #[arg(long, help = "Lint the current changed main-namespace article set")]
    changed: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ArticleFixArgs {
    #[arg(
        help = "Article path; state-draft paths under .wikitool/drafts/ may use --title override"
    )]
    path: Option<PathBuf>,
    #[arg(long, default_value = "remilia", value_name = "PROFILE")]
    profile: String,
    #[arg(
        long,
        value_enum,
        default_value_t = ArticleFixApplyArg::None,
        value_name = "MODE",
        help = "Apply mode: none|safe"
    )]
    apply: ArticleFixApplyArg,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
    #[arg(
        long = "title",
        value_name = "TITLE",
        help = "Select a canonical article title; with one .wikitool/drafts/ PATH, override the draft title"
    )]
    titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    titles_file: Option<PathBuf>,
    #[arg(long, help = "Fix the current changed main-namespace article set")]
    changed: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ArticlePromoteArgs {
    #[arg(help = "State-draft path under the canonical .wikitool/drafts/ directory")]
    path: PathBuf,
    #[arg(
        long,
        value_name = "TITLE",
        help = "Canonical article title for the destination under wiki_content/"
    )]
    title: String,
    #[arg(long, help = "Overwrite the destination file if it already exists")]
    overwrite: bool,
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
enum ArticleFixApplyArg {
    None,
    Safe,
}

impl ArticleFixApplyArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Safe => "safe",
        }
    }
}

impl From<ArticleFixApplyArg> for ArticleFixApplyMode {
    fn from(value: ArticleFixApplyArg) -> Self {
        match value {
            ArticleFixApplyArg::None => Self::None,
            ArticleFixApplyArg::Safe => Self::Safe,
        }
    }
}

impl std::fmt::Display for ArticleFixApplyArg {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub(crate) fn run_article(runtime: &RuntimeOptions, args: ArticleArgs) -> Result<()> {
    match args.command {
        ArticleSubcommand::Lint(args) => lint::run_article_lint(runtime, args),
        ArticleSubcommand::Fix(args) => fix::run_article_fix(runtime, args),
        ArticleSubcommand::Promote(args) => promote::run_article_promote(runtime, args),
    }
}
