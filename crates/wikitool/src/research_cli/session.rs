use std::fs;
use std::io::{self, Read};
use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::research::{
    ResearchSessionImportOptions, ResearchSessionSummary, clear_research_session,
    import_research_session, list_research_sessions, prune_research_sessions,
    show_research_session,
};

use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct ResearchSessionArgs {
    #[command(subcommand)]
    command: ResearchSessionSubcommand,
}

#[derive(Debug, Subcommand)]
enum ResearchSessionSubcommand {
    #[command(about = "Import source-issued browser cookies for a domain")]
    Import(ResearchSessionImportArgs),
    #[command(about = "List imported research access sessions without cookie values")]
    List(ResearchSessionListArgs),
    #[command(about = "Show one imported research access session without cookie values")]
    Show(ResearchSessionShowArgs),
    #[command(about = "Clear one imported research access session")]
    Clear(ResearchSessionClearArgs),
    #[command(about = "Remove expired research access sessions")]
    Prune(ResearchSessionPruneArgs),
}

#[derive(Debug, Args)]
struct ResearchSessionImportArgs {
    url: String,
    #[arg(
        long,
        value_name = "PATH|-|COOKIE_HEADER",
        help = "Read cookies from Netscape cookies.txt, JSON, stdin (-), or a literal Cookie header"
    )]
    cookies: String,
    #[arg(
        long,
        value_name = "UA",
        help = "Pin the browser user-agent used when the cookies were obtained"
    )]
    user_agent: Option<String>,
    #[arg(
        long,
        value_name = "SECONDS",
        help = "Expire this local session after the supplied number of seconds"
    )]
    ttl_seconds: Option<u64>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct ResearchSessionListArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct ResearchSessionShowArgs {
    domain: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct ResearchSessionClearArgs {
    domain: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct ResearchSessionPruneArgs {
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
struct ResearchSessionImportOutput {
    schema_version: String,
    status: &'static str,
    session: ResearchSessionSummary,
}

#[derive(Debug, Serialize)]
struct ResearchSessionListOutput {
    schema_version: String,
    count: usize,
    sessions: Vec<ResearchSessionSummary>,
}

#[derive(Debug, Serialize)]
struct ResearchSessionShowOutput {
    schema_version: String,
    session: ResearchSessionSummary,
}

#[derive(Debug, Serialize)]
struct ResearchSessionClearOutput {
    schema_version: String,
    selector: String,
    removed: bool,
}

#[derive(Debug, Serialize)]
struct ResearchSessionPruneOutput {
    schema_version: String,
    removed_count: usize,
    removed: Vec<ResearchSessionSummary>,
}

pub(crate) fn run(runtime: &RuntimeOptions, args: ResearchSessionArgs) -> Result<()> {
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    match args.command {
        ResearchSessionSubcommand::Import(args) => {
            if args.ttl_seconds == Some(0) {
                bail!("research session import requires --ttl-seconds >= 1");
            }
            let cookie_input = read_session_cookie_input(&args.cookies)?;
            let result = import_research_session(
                &paths,
                &args.url,
                &cookie_input,
                &ResearchSessionImportOptions {
                    user_agent: args.user_agent,
                    ttl_hint_seconds: args.ttl_seconds,
                },
            )?;
            let summary = show_research_session(&paths, &result.session.domain)?
                .ok_or_else(|| anyhow::anyhow!("imported session was not readable after write"))?;
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ResearchSessionImportOutput {
                        schema_version: "research_session_v1".to_string(),
                        status: "ok",
                        session: summary,
                    })?
                );
                return Ok(());
            }
            print_session_header("research session import", &paths);
            print_session_summary(&summary);
            println!("cookie_values: stored locally, not printed");
        }
        ResearchSessionSubcommand::List(args) => {
            let sessions = list_research_sessions(&paths)?;
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ResearchSessionListOutput {
                        schema_version: "research_session_v1".to_string(),
                        count: sessions.len(),
                        sessions,
                    })?
                );
                return Ok(());
            }
            print_session_header("research session list", &paths);
            println!("sessions: {}", sessions.len());
            for session in &sessions {
                print_session_summary(session);
            }
            println!("cookie_values: not printed");
        }
        ResearchSessionSubcommand::Show(args) => {
            let Some(session) = show_research_session(&paths, &args.domain)? else {
                bail!("research session not found for {}", args.domain);
            };
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ResearchSessionShowOutput {
                        schema_version: "research_session_v1".to_string(),
                        session,
                    })?
                );
                return Ok(());
            }
            print_session_header("research session show", &paths);
            print_session_summary(&session);
            println!("cookie_values: not printed");
        }
        ResearchSessionSubcommand::Clear(args) => {
            let removed = clear_research_session(&paths, &args.domain)?;
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ResearchSessionClearOutput {
                        schema_version: "research_session_v1".to_string(),
                        selector: args.domain,
                        removed,
                    })?
                );
                return Ok(());
            }
            print_session_header("research session clear", &paths);
            println!("selector: {}", args.domain);
            println!("removed: {}", yes_no(removed));
        }
        ResearchSessionSubcommand::Prune(args) => {
            let removed = prune_research_sessions(&paths)?;
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ResearchSessionPruneOutput {
                        schema_version: "research_session_v1".to_string(),
                        removed_count: removed.len(),
                        removed,
                    })?
                );
                return Ok(());
            }
            print_session_header("research session prune", &paths);
            println!("removed_count: {}", removed.len());
            for session in &removed {
                print_session_summary(session);
            }
            println!("cookie_values: not printed");
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn read_session_cookie_input(selector: &str) -> Result<String> {
    if selector == "-" {
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .context("failed to read cookies from stdin")?;
        return Ok(input);
    }
    let path = Path::new(selector);
    if path.exists() {
        return fs::read_to_string(path)
            .with_context(|| format!("failed to read cookies from {}", normalize_path(path)));
    }
    Ok(selector.to_string())
}

fn print_session_header(label: &str, paths: &wikitool_core::runtime::ResolvedPaths) {
    println!("{label}");
    println!("project_root: {}", normalize_path(&paths.project_root));
}

fn print_session_summary(session: &ResearchSessionSummary) {
    println!("domain: {}", session.domain);
    println!("source_url: {}", session.source_url);
    println!("cookie_count: {}", session.cookie_count);
    println!(
        "cookie_names: {}",
        if session.cookie_names.is_empty() {
            "<none>".to_string()
        } else {
            session.cookie_names.join(", ")
        }
    );
    println!("user_agent_pinned: {}", yes_no(session.user_agent_pinned));
    println!("obtained_at: {}", session.obtained_at);
    println!(
        "expires_at: {}",
        session.expires_at.as_deref().unwrap_or("<none>")
    );
    println!("expired: {}", yes_no(session.expired));
    println!("path: {}", session.path);
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
