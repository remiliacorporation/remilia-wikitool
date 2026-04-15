use std::fs;

use anyhow::Result;
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::knowledge::content_index::load_stored_index_stats;
use wikitool_core::knowledge::status::{DEFAULT_DOCS_PROFILE, knowledge_status};
use wikitool_core::runtime::inspect_runtime;
use wikitool_core::schema::{DatabaseSchemaState, schema_state};

use crate::cli_support::{
    OutputFormat, format_flag, normalize_path, print_database_schema_status,
    print_stored_index_stats, prompt_yes_no, resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct DbArgs {
    #[command(subcommand)]
    command: DbSubcommand,
}

#[derive(Debug, Subcommand)]
enum DbSubcommand {
    #[command(
        alias = "status",
        about = "Show local database state and knowledge readiness"
    )]
    Stats(DbStatsArgs),
    #[command(about = "Delete the local runtime database")]
    Reset {
        #[arg(
            long,
            help = "Assume yes and delete the local database without prompting"
        )]
        yes: bool,
    },
}

#[derive(Debug, Args)]
struct DbStatsArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct DbStatsJson {
    db_path: String,
    data_dir: String,
    db_exists: bool,
    db_size_bytes: Option<u64>,
    content_index: Option<wikitool_core::knowledge::content_index::StoredIndexStats>,
    docs_profile_requested: String,
    readiness: wikitool_core::knowledge::status::KnowledgeReadinessLevel,
    degradations: Vec<String>,
    knowledge_generation: String,
    database_schema: DbSchemaJson,
}

#[derive(Debug, Serialize)]
struct DbSchemaJson {
    status: String,
    reason: Option<String>,
}

pub(crate) fn run_db(runtime: &RuntimeOptions, args: DbArgs) -> Result<()> {
    match args.command {
        DbSubcommand::Stats(args) => run_db_stats(runtime, args),
        DbSubcommand::Reset { yes } => run_db_reset(runtime, yes),
    }
}

fn run_db_stats(runtime: &RuntimeOptions, args: DbStatsArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let status = inspect_runtime(&paths)?;
    let stored = load_stored_index_stats(&paths)?;
    let knowledge = knowledge_status(&paths, DEFAULT_DOCS_PROFILE)?;
    if args.format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&DbStatsJson {
                db_path: normalize_path(&paths.db_path),
                data_dir: normalize_path(&paths.data_dir),
                db_exists: status.db_exists,
                db_size_bytes: status.db_size_bytes,
                content_index: stored,
                docs_profile_requested: knowledge.docs_profile_requested,
                readiness: knowledge.readiness,
                degradations: knowledge.degradations,
                knowledge_generation: knowledge.knowledge_generation,
                database_schema: db_schema_json(&paths)?,
            })?
        );
        return Ok(());
    }

    println!("db stats");
    println!("db_path: {}", normalize_path(&paths.db_path));
    println!("data_dir: {}", normalize_path(&paths.data_dir));
    println!("db_exists: {}", format_flag(status.db_exists));
    println!(
        "db_size_bytes: {}",
        status
            .db_size_bytes
            .map(|size| size.to_string())
            .unwrap_or_else(|| "n/a".to_string())
    );
    match stored {
        Some(stored) => print_stored_index_stats("content_index", &stored),
        None => println!("content_index.storage: <not built> (run `wikitool knowledge build`)"),
    }
    println!(
        "docs_profile_requested: {}",
        knowledge.docs_profile_requested
    );
    println!(
        "readiness: {}",
        match knowledge.readiness {
            wikitool_core::knowledge::status::KnowledgeReadinessLevel::NotReady => "not_ready",
            wikitool_core::knowledge::status::KnowledgeReadinessLevel::ContentReady => {
                "content_ready"
            }
            wikitool_core::knowledge::status::KnowledgeReadinessLevel::AuthoringReady => {
                "authoring_ready"
            }
        }
    );
    println!(
        "degradations: {}",
        if knowledge.degradations.is_empty() {
            "<none>".to_string()
        } else {
            knowledge.degradations.join(", ")
        }
    );
    println!("knowledge_generation: {}", knowledge.knowledge_generation);
    print_database_schema_status(&paths);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}

fn db_schema_json(paths: &wikitool_core::runtime::ResolvedPaths) -> Result<DbSchemaJson> {
    Ok(match schema_state(paths)? {
        DatabaseSchemaState::Missing => DbSchemaJson {
            status: "absent".to_string(),
            reason: None,
        },
        DatabaseSchemaState::Ready => DbSchemaJson {
            status: "ready".to_string(),
            reason: None,
        },
        DatabaseSchemaState::Incompatible { reason } => DbSchemaJson {
            status: "incompatible".to_string(),
            reason: Some(reason),
        },
    })
}

fn run_db_reset(runtime: &RuntimeOptions, yes: bool) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let normalized_path = normalize_path(&paths.db_path);
    if paths.db_path.exists()
        && !yes
        && !prompt_yes_no(&format!("Delete local database {normalized_path}? (y/N) "))?
    {
        println!("Aborted.");
        return Ok(());
    }

    let deleted = if paths.db_path.exists() {
        fs::remove_file(&paths.db_path)?;
        true
    } else {
        false
    };

    println!("db reset");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("db_path: {normalized_path}");
    println!("deleted: {}", format_flag(deleted));
    println!("next_step: run `wikitool knowledge build` or `wikitool knowledge warm`");
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}
