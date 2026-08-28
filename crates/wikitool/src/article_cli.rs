use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use wikitool_core::article_acceptance::ArticleProseOrigin;
use wikitool_core::article_changeset::ArticleChangesetWarningPolicy;
use wikitool_core::article_lint::ArticleFixApplyMode;

use crate::RuntimeOptions;
use crate::cli_support::OutputFormat;

mod accept;
mod changeset;
mod fix;
mod lint;
mod output;
mod promote;
mod scout;
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
    #[command(about = "Assemble a typed retrieval-context packet for an article topic")]
    Scout(scout::ArticleScoutArgs),
    #[command(about = "Record a hash-bound acceptance decision in the local ledger")]
    Accept(ArticleAcceptArgs),
    #[command(about = "Prepare or accept an exact-content multi-article review changeset")]
    Changeset(ArticleChangesetArgs),
    #[command(about = "Lint article wikitext against MediaWiki and site-adapter rules")]
    Lint(ArticleLintArgs),
    #[command(about = "Apply safe mechanical fixes to article wikitext")]
    Fix(ArticleFixArgs),
    #[command(about = "Promote a draft with current transactional publication acceptance")]
    Promote(ArticlePromoteArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ArticleChangesetArgs {
    #[command(subcommand)]
    command: ArticleChangesetSubcommand,
}

#[derive(Debug, Subcommand)]
enum ArticleChangesetSubcommand {
    #[command(
        about = "Freeze selected articles, lint evidence, and prose origin in a JSON manifest"
    )]
    Prepare(ArticleChangesetPrepareArgs),
    #[command(about = "Bind one named human decision to every exact item in a prepared manifest")]
    Accept(ArticleChangesetAcceptArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ArticleChangesetPrepareArgs {
    #[arg(
        help = "One Main-namespace article path, or one state-draft path paired with exactly one --title"
    )]
    path: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Write the prepared JSON manifest to this project-scoped path"
    )]
    output: PathBuf,
    #[arg(
        long,
        value_enum,
        value_name = "ORIGIN",
        help = "Truthful prose origin shared by every item in this changeset"
    )]
    prose_origin: ArticleProseOriginArg,
    #[arg(long, help = "Replace an existing manifest after reviewing its path")]
    replace: bool,
    #[arg(long = "title", value_name = "TITLE")]
    titles: Vec<String>,
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    titles_file: Option<PathBuf>,
    #[arg(long, help = "Prepare the current changed Main-namespace article set")]
    changed: bool,
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
pub(crate) struct ArticleChangesetAcceptArgs {
    #[arg(help = "Prepared article review changeset JSON manifest")]
    manifest: PathBuf,
    #[arg(
        long,
        value_name = "IDENTITY",
        help = "Self-reported name or handle of the human editor; Wikitool does not authenticate it"
    )]
    human_editor: String,
    #[arg(
        long,
        value_enum,
        default_value_t = ArticleChangesetWarningPolicyArg::RequireNone,
        value_name = "DECISION",
        help = "Warning decision: require-none|accept"
    )]
    warnings: ArticleChangesetWarningPolicyArg,
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
pub(crate) struct ArticleAcceptArgs {
    #[arg(help = "Draft or Main-namespace article path whose exact prose was read")]
    path: PathBuf,
    #[arg(
        long,
        value_name = "TITLE",
        help = "Canonical Main-namespace article title"
    )]
    title: String,
    #[arg(
        long,
        value_name = "IDENTITY",
        help = "Self-reported name or handle of the human editor; Wikitool does not authenticate it"
    )]
    human_editor: String,
    #[arg(
        long,
        value_enum,
        value_name = "ORIGIN",
        help = "Prose origin: human-draft|human-revision|agent-draft|collaborative-draft|mechanical-conversion-of-human-prose|human-reviewed-legacy"
    )]
    prose_origin: ArticleProseOriginArg,
    #[arg(
        long,
        help = "Record explicit caller acceptance of remaining lint warnings"
    )]
    allow_warnings: bool,
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
pub(crate) struct ArticleLintArgs {
    #[arg(
        help = "Article path; state-draft paths under .wikitool/drafts/ may use --title override"
    )]
    path: Option<PathBuf>,
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
    #[arg(
        help = "Human-accepted state-draft path under the canonical .wikitool/drafts/ directory"
    )]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ArticleProseOriginArg {
    HumanDraft,
    HumanRevision,
    AgentDraft,
    CollaborativeDraft,
    MechanicalConversionOfHumanProse,
    HumanReviewedLegacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ArticleChangesetWarningPolicyArg {
    RequireNone,
    Accept,
}

impl From<ArticleProseOriginArg> for ArticleProseOrigin {
    fn from(value: ArticleProseOriginArg) -> Self {
        match value {
            ArticleProseOriginArg::HumanDraft => Self::HumanDraft,
            ArticleProseOriginArg::HumanRevision => Self::HumanRevision,
            ArticleProseOriginArg::AgentDraft => Self::AgentDraft,
            ArticleProseOriginArg::CollaborativeDraft => Self::CollaborativeDraft,
            ArticleProseOriginArg::MechanicalConversionOfHumanProse => {
                Self::MechanicalConversionOfHumanProse
            }
            ArticleProseOriginArg::HumanReviewedLegacy => Self::HumanReviewedLegacy,
        }
    }
}

impl From<ArticleChangesetWarningPolicyArg> for ArticleChangesetWarningPolicy {
    fn from(value: ArticleChangesetWarningPolicyArg) -> Self {
        match value {
            ArticleChangesetWarningPolicyArg::RequireNone => Self::RequireNone,
            ArticleChangesetWarningPolicyArg::Accept => Self::Accept,
        }
    }
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
        ArticleSubcommand::Scout(args) => scout::run_article_scout(runtime, args),
        ArticleSubcommand::Accept(args) => accept::run_article_accept(runtime, args),
        ArticleSubcommand::Changeset(args) => match args.command {
            ArticleChangesetSubcommand::Prepare(args) => {
                changeset::run_article_changeset_prepare(runtime, args)
            }
            ArticleChangesetSubcommand::Accept(args) => {
                changeset::run_article_changeset_accept(runtime, args)
            }
        },
        ArticleSubcommand::Lint(args) => lint::run_article_lint(runtime, args),
        ArticleSubcommand::Fix(args) => fix::run_article_fix(runtime, args),
        ArticleSubcommand::Promote(args) => promote::run_article_promote(runtime, args),
    }
}
