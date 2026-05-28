use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::config::{WikiConfig, load_config};
use wikitool_core::runtime::{ResolvedPaths, inspect_runtime};

use crate::RuntimeOptions;
use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_paths};

#[derive(Debug, Args)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
enum ConfigSubcommand {
    #[command(about = "Show resolved configuration, paths, and target-wiki sources")]
    Show(ConfigShowArgs),
}

#[derive(Debug, Args)]
struct ConfigShowArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct ConfigShowJson {
    schema_version: &'static str,
    project_root: String,
    config_path: String,
    config_exists: bool,
    wiki: wikitool_core::config::WikiTargetResolution,
    paths: ConfigPathsJson,
    runtime: ConfigRuntimeJson,
    notes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ConfigPathsJson {
    wiki_content_dir: String,
    templates_dir: String,
    state_dir: String,
    data_dir: String,
    db_path: String,
    parser_config_path: String,
}

#[derive(Debug, Serialize)]
struct ConfigRuntimeJson {
    db_exists: bool,
    config_exists: bool,
    parser_config_exists: bool,
    warnings: Vec<String>,
}

pub(crate) fn run_config(runtime: &RuntimeOptions, args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigSubcommand::Show(args) => run_config_show(runtime, args.format),
    }
}

fn run_config_show(runtime: &RuntimeOptions, format: OutputFormat) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let config = load_config(&paths.config_path)
        .with_context(|| format!("failed to load {}", normalize_path(&paths.config_path)))?;
    let output = build_config_show(&paths, &config)?;
    if format.is_json() {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("config show");
    println!("project_root: {}", output.project_root);
    println!("config_path: {}", output.config_path);
    print_resolved_value("wiki.url", &output.wiki.url);
    print_resolved_value("wiki.api_url", &output.wiki.api_url);
    print_resolved_value("wiki.article_path", &output.wiki.article_path);
    print_resolved_value("wiki.user_agent", &output.wiki.user_agent);
    if output.wiki.warnings.is_empty() {
        println!("warnings: <none>");
    } else {
        for warning in &output.wiki.warnings {
            println!("warning: {warning}");
        }
    }
    for note in &output.notes {
        println!("note: {note}");
    }
    Ok(())
}

fn build_config_show(paths: &ResolvedPaths, config: &WikiConfig) -> Result<ConfigShowJson> {
    let status = inspect_runtime(paths)?;
    Ok(ConfigShowJson {
        schema_version: "wikitool_config_v1",
        project_root: normalize_path(&paths.project_root),
        config_path: normalize_path(&paths.config_path),
        config_exists: status.config_exists,
        wiki: config.resolve_wiki_target(),
        paths: ConfigPathsJson {
            wiki_content_dir: normalize_path(&paths.wiki_content_dir),
            templates_dir: normalize_path(&paths.templates_dir),
            state_dir: normalize_path(&paths.state_dir),
            data_dir: normalize_path(&paths.data_dir),
            db_path: normalize_path(&paths.db_path),
            parser_config_path: normalize_path(&paths.parser_config_path),
        },
        runtime: ConfigRuntimeJson {
            db_exists: status.db_exists,
            config_exists: status.config_exists,
            parser_config_exists: status.parser_config_exists,
            warnings: status.warnings,
        },
        notes: vec![
            "project config is the durable wiki target; WIKITOOL_* env vars are temporary overrides",
            "bare WIKI_* env vars are not read; use WIKITOOL_WIKI_URL, WIKITOOL_WIKI_API_URL, WIKITOOL_USER_AGENT, and WIKITOOL_ARTICLE_PATH",
            "authoring and lint overlays are currently Remilia-specific even when the sync target is changed",
        ],
    })
}

fn print_resolved_value(label: &str, value: &wikitool_core::config::ResolvedConfigValue) {
    println!(
        "{label}: {} (source={} key={})",
        value.value.as_deref().unwrap_or("<none>"),
        value.source,
        value.source_key.as_deref().unwrap_or("<none>")
    );
}
