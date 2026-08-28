use std::fs;
use std::io::{self, Read};
use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::source::{
    SourceAccessSessionImportOptions, SourceAccessSessionSummary, clear_source_access_session,
    import_source_access_session, list_source_access_sessions, prune_source_access_sessions,
    show_source_access_session,
};

use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct SourceAccessSessionArgs {
    #[command(subcommand)]
    command: SourceAccessSessionSubcommand,
}

#[derive(Debug, Subcommand)]
enum SourceAccessSessionSubcommand {
    #[command(about = "Import source-issued browser cookies for a domain")]
    Import(SourceAccessSessionImportArgs),
    #[command(about = "List imported source access sessions without cookie values")]
    List(SourceAccessSessionListArgs),
    #[command(about = "Show one imported source access session without cookie values")]
    Show(SourceAccessSessionShowArgs),
    #[command(about = "Clear one imported source access session")]
    Clear(SourceAccessSessionClearArgs),
    #[command(about = "Remove expired source access sessions")]
    Prune(SourceAccessSessionPruneArgs),
}

#[derive(Debug, Args)]
struct SourceAccessSessionImportArgs {
    url: String,
    #[arg(
        long,
        value_name = "PATH|-",
        help = "Read cookies from stdin (-) or an existing regular, non-symlink file; literal values are rejected"
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
struct SourceAccessSessionListArgs {
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
struct SourceAccessSessionShowArgs {
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
struct SourceAccessSessionClearArgs {
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
struct SourceAccessSessionPruneArgs {
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
struct SourceAccessSessionImportOutput {
    schema_version: String,
    status: &'static str,
    session: SourceAccessSessionSummary,
}

#[derive(Debug, Serialize)]
struct SourceAccessSessionListOutput {
    schema_version: String,
    count: usize,
    sessions: Vec<SourceAccessSessionSummary>,
}

#[derive(Debug, Serialize)]
struct SourceAccessSessionShowOutput {
    schema_version: String,
    session: SourceAccessSessionSummary,
}

#[derive(Debug, Serialize)]
struct SourceAccessSessionClearOutput {
    schema_version: String,
    selector: String,
    removed: bool,
}

#[derive(Debug, Serialize)]
struct SourceAccessSessionPruneOutput {
    schema_version: String,
    removed_count: usize,
    removed: Vec<SourceAccessSessionSummary>,
}

pub(crate) fn run(runtime: &RuntimeOptions, args: SourceAccessSessionArgs) -> Result<()> {
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    match args.command {
        SourceAccessSessionSubcommand::Import(args) => {
            if args.ttl_seconds == Some(0) {
                bail!("source session import requires --ttl-seconds >= 1");
            }
            let cookie_input = read_session_cookie_input(&args.cookies)?;
            let result = import_source_access_session(
                &paths,
                &args.url,
                &cookie_input,
                &SourceAccessSessionImportOptions {
                    user_agent: args.user_agent,
                    ttl_hint_seconds: args.ttl_seconds,
                },
            )?;
            let summary = show_source_access_session(&paths, &result.session.domain)?
                .ok_or_else(|| anyhow::anyhow!("imported session was not readable after write"))?;
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&SourceAccessSessionImportOutput {
                        schema_version: "source_session_v1".to_string(),
                        status: "ok",
                        session: summary,
                    })?
                );
                return Ok(());
            }
            print_session_header("source session import", &paths);
            print_session_summary(&summary);
            println!("cookie_values: stored locally, not printed");
        }
        SourceAccessSessionSubcommand::List(args) => {
            let sessions = list_source_access_sessions(&paths)?;
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&SourceAccessSessionListOutput {
                        schema_version: "source_session_v1".to_string(),
                        count: sessions.len(),
                        sessions,
                    })?
                );
                return Ok(());
            }
            print_session_header("source session list", &paths);
            println!("sessions: {}", sessions.len());
            for session in &sessions {
                print_session_summary(session);
            }
            println!("cookie_values: not printed");
        }
        SourceAccessSessionSubcommand::Show(args) => {
            let Some(session) = show_source_access_session(&paths, &args.domain)? else {
                bail!("source session not found for {}", args.domain);
            };
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&SourceAccessSessionShowOutput {
                        schema_version: "source_session_v1".to_string(),
                        session,
                    })?
                );
                return Ok(());
            }
            print_session_header("source session show", &paths);
            print_session_summary(&session);
            println!("cookie_values: not printed");
        }
        SourceAccessSessionSubcommand::Clear(args) => {
            let removed = clear_source_access_session(&paths, &args.domain)?;
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&SourceAccessSessionClearOutput {
                        schema_version: "source_session_v1".to_string(),
                        selector: args.domain,
                        removed,
                    })?
                );
                return Ok(());
            }
            print_session_header("source session clear", &paths);
            println!("selector: {}", args.domain);
            println!("removed: {}", yes_no(removed));
        }
        SourceAccessSessionSubcommand::Prune(args) => {
            let removed = prune_source_access_sessions(&paths)?;
            if args.format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&SourceAccessSessionPruneOutput {
                        schema_version: "source_session_v1".to_string(),
                        removed_count: removed.len(),
                        removed,
                    })?
                );
                return Ok(());
            }
            print_session_header("source session prune", &paths);
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
    let mut stdin = io::stdin().lock();
    read_session_cookie_input_from(selector, &mut stdin)
}

fn read_session_cookie_input_from(selector: &str, stdin: &mut impl Read) -> Result<String> {
    if selector == "-" {
        let mut input = String::new();
        stdin
            .read_to_string(&mut input)
            .context("failed to read cookies from stdin")?;
        return Ok(input);
    }

    let path = Path::new(selector);
    let metadata = fs::symlink_metadata(path)
        .context("--cookies must be `-` for stdin or an existing regular file")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("--cookies must be `-` for stdin or an existing regular file");
    }
    fs::read_to_string(path).context("failed to read the --cookies file")
}

fn print_session_header(label: &str, paths: &wikitool_core::runtime::ResolvedPaths) {
    println!("{label}");
    println!("project_root: {}", normalize_path(&paths.project_root));
}

fn print_session_summary(session: &SourceAccessSessionSummary) {
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn cookie_input_reads_stdin_only_for_dash() {
        let mut stdin = Cursor::new(b"session_cookie=stdin-value".to_vec());

        let input = read_session_cookie_input_from("-", &mut stdin).expect("read stdin");

        assert_eq!(input, "session_cookie=stdin-value");
    }

    #[test]
    fn cookie_input_reads_an_existing_regular_file() {
        let temp = tempdir().expect("create temp directory");
        let path = temp.path().join("cookies.txt");
        fs::write(&path, "session_cookie=file-value").expect("write cookie fixture");

        let input = read_session_cookie_input_from(
            path.to_str().expect("utf-8 fixture path"),
            &mut Cursor::new(Vec::new()),
        )
        .expect("read cookie file");

        assert_eq!(input, "session_cookie=file-value");
    }

    #[test]
    fn cookie_input_rejects_literal_cookie_without_echoing_it() {
        let cookie = "session_cookie=literal-secret-sentinel";

        let error = read_session_cookie_input_from(cookie, &mut Cursor::new(Vec::new()))
            .expect_err("literal cookie must not be accepted");
        let diagnostic = format!("{error:#}");

        assert!(diagnostic.contains("existing regular file"));
        assert!(!diagnostic.contains(cookie));
        assert!(!diagnostic.contains("literal-secret-sentinel"));
    }

    #[test]
    fn cookie_input_rejects_non_regular_path_without_echoing_it() {
        let temp = tempdir().expect("create temp directory");
        let selector = temp.path().to_str().expect("utf-8 fixture path");

        let error = read_session_cookie_input_from(selector, &mut Cursor::new(Vec::new()))
            .expect_err("directory must not be accepted");
        let diagnostic = format!("{error:#}");

        assert!(diagnostic.contains("existing regular file"));
        assert!(!diagnostic.contains(selector));
    }

    #[cfg(unix)]
    #[test]
    fn cookie_input_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("create temp directory");
        let target = temp.path().join("cookies.txt");
        let link = temp.path().join("cookies.link");
        fs::write(&target, "session_cookie=file-value").expect("write cookie fixture");
        symlink(&target, &link).expect("create cookie symlink");

        let error = read_session_cookie_input_from(
            link.to_str().expect("utf-8 fixture path"),
            &mut Cursor::new(Vec::new()),
        )
        .expect_err("symlink must not be accepted");

        assert!(format!("{error:#}").contains("existing regular file"));
    }
}
