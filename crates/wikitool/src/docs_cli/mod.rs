use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use wikitool_core::docs::{
    DocsContextOptions, DocsImportOptions, DocsImportProfileOptions, DocsImportTechnicalOptions,
    DocsListOptions, DocsRemoveKind, DocsSearchOptions, DocsSymbolLookupOptions, TechnicalDocType,
    TechnicalImportTask, build_docs_context, format_expiration, import_docs_bundle,
    import_docs_profile_with_config, import_extension_docs, import_technical_docs, list_docs,
    lookup_docs_symbols, remove_docs, search_docs, update_outdated_docs_with_config,
};

use crate::{
    LOCAL_DB_POLICY_MESSAGE, RuntimeOptions,
    cli_support::{
        OutputFormat, collapse_whitespace, format_flag, normalize_path, normalize_title_query,
        resolve_runtime_paths, resolve_runtime_with_config,
    },
};

#[cfg(any(test, feature = "maintainer-surface"))]
use crate::Cli;
#[cfg(any(test, feature = "maintainer-surface"))]
use clap::{Command, CommandFactory, error::ErrorKind};
#[cfg(feature = "maintainer-surface")]
use std::fs;

mod admin;
mod import;
mod query;
mod reference;

#[cfg(feature = "maintainer-surface")]
pub(crate) use reference::{DocsGenerateReferenceArgs, run_docs_generate_reference};

#[derive(Debug, Args)]
pub(crate) struct DocsArgs {
    #[command(subcommand)]
    command: DocsSubcommand,
}

#[derive(Debug, Subcommand)]
enum DocsSubcommand {
    #[command(about = "Import docs from a bundle or extension source")]
    Import(import::DocsImportArgs),
    #[command(
        name = "import-technical",
        about = "Import a targeted technical docs slice"
    )]
    ImportTechnical(import::DocsImportTechnicalArgs),
    #[command(name = "import-profile", about = "Hydrate a named docs profile")]
    ImportProfile(import::DocsImportProfileArgs),
    #[command(
        name = "generate-reference",
        about = "Generate CLI reference docs from help text",
        hide = true
    )]
    #[cfg(feature = "maintainer-surface")]
    GenerateReference(reference::DocsGenerateReferenceArgs),
    #[command(about = "List imported docs corpora")]
    List(admin::DocsListArgs),
    #[command(about = "Refresh outdated imported docs corpora")]
    Update,
    #[command(about = "Remove an imported docs corpus")]
    Remove { target: String },
    #[command(about = "Search pinned docs corpora by text")]
    Search(query::DocsSearchArgs),
    #[command(about = "Build focused docs context from pinned corpora")]
    Context(query::DocsContextArgs),
    #[command(about = "Lookup docs symbols such as hooks, config vars, and APIs")]
    Symbols(query::DocsSymbolsArgs),
}

pub(crate) fn run_docs(runtime: &RuntimeOptions, args: DocsArgs) -> Result<()> {
    match args.command {
        DocsSubcommand::Import(args) => import::run_docs_import(runtime, args),
        DocsSubcommand::ImportTechnical(args) => import::run_docs_import_technical(runtime, args),
        DocsSubcommand::ImportProfile(args) => import::run_docs_import_profile(runtime, args),
        #[cfg(feature = "maintainer-surface")]
        DocsSubcommand::GenerateReference(args) => reference::run_docs_generate_reference(args),
        DocsSubcommand::List(args) => admin::run_docs_list(runtime, args),
        DocsSubcommand::Update => admin::run_docs_update(runtime),
        DocsSubcommand::Remove { target } => admin::run_docs_remove(runtime, &target),
        DocsSubcommand::Search(args) => query::run_docs_search(runtime, args),
        DocsSubcommand::Context(args) => query::run_docs_context(runtime, args),
        DocsSubcommand::Symbols(args) => query::run_docs_symbols(runtime, args),
    }
}

fn normalize_title_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = values
        .into_iter()
        .map(|value| normalize_title_query(&value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort_unstable_by_key(|value| value.to_ascii_lowercase());
    normalized.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    normalized
}

fn infer_doc_type_from_title(title: &str) -> TechnicalDocType {
    if title.starts_with("Manual:Hooks") {
        return TechnicalDocType::Hooks;
    }
    if title.starts_with("Manual:$wg") {
        return TechnicalDocType::Config;
    }
    if title.starts_with("API:") {
        return TechnicalDocType::Api;
    }
    if title.starts_with("Help:") {
        return TechnicalDocType::Help;
    }
    TechnicalDocType::Manual
}
