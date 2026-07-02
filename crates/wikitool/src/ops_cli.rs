use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::Serialize;
use sha2::{Digest, Sha256};
use wikitool_core::mw::{
    MediaWikiClient, MovePageOptions, MoveReport, ProtectPageOptions, ProtectReport, PurgeOptions,
    PurgeReport, UndeletePageOptions, UndeleteReport, UploadOptions, UploadReport, WikiWriteApi,
};

use crate::RuntimeOptions;
use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_with_config};

#[derive(Debug, Args)]
pub(crate) struct PurgeArgs {
    #[arg(value_name = "TITLE")]
    pub(crate) positional_titles: Vec<String>,
    #[arg(long = "title", value_name = "TITLE")]
    pub(crate) titles: Vec<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Read one canonical page title per line"
    )]
    pub(crate) titles_file: Option<PathBuf>,
    #[arg(long, help = "Force link table update while purging")]
    pub(crate) forcelinkupdate: bool,
    #[arg(long, help = "Force recursive link table update while purging")]
    pub(crate) forcerecursivelinkupdate: bool,
    #[arg(long, help = "Preview purge without writing to the wiki")]
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

#[derive(Debug, Args)]
pub(crate) struct UploadArgs {
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,
    #[arg(long, value_name = "FILENAME", help = "Target wiki filename")]
    pub(crate) filename: Option<String>,
    #[arg(
        long,
        value_name = "TEXT",
        default_value = "Upload via wikitool",
        help = "Upload comment"
    )]
    pub(crate) comment: String,
    #[arg(long, value_name = "WIKITEXT", help = "Initial file description text")]
    pub(crate) text: Option<String>,
    #[arg(long, help = "Pass ignorewarnings=1 to MediaWiki upload")]
    pub(crate) ignore_warnings: bool,
    #[arg(long, help = "Preview upload without writing to the wiki")]
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

#[derive(Debug, Args)]
pub(crate) struct MoveArgs {
    #[arg(value_name = "FROM")]
    pub(crate) from: String,
    #[arg(value_name = "TO")]
    pub(crate) to: String,
    #[arg(
        long,
        value_name = "TEXT",
        default_value = "Move via wikitool",
        help = "Move reason"
    )]
    pub(crate) reason: String,
    #[arg(
        long = "no-redirect",
        help = "Do not leave a redirect at the old title (default leaves one)"
    )]
    pub(crate) no_redirect: bool,
    #[arg(long = "move-talk", help = "Also move the associated talk page")]
    pub(crate) move_talk: bool,
    #[arg(
        long = "move-subpages",
        help = "Also move subpages (up to the API limit)"
    )]
    pub(crate) move_subpages: bool,
    #[arg(long, help = "Pass ignorewarnings=1 to MediaWiki move")]
    pub(crate) ignore_warnings: bool,
    #[arg(long, help = "Preview move without writing to the wiki")]
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

#[derive(Debug, Args)]
pub(crate) struct ProtectArgs {
    #[arg(value_name = "TITLE")]
    pub(crate) title: String,
    #[arg(
        long = "protection",
        value_name = "TYPE=LEVEL",
        required = true,
        help = "Restriction to apply, e.g. edit=sysop or move=autoconfirmed; repeat for multiple. An empty level (edit=) clears the restriction"
    )]
    pub(crate) protections: Vec<String>,
    #[arg(
        long,
        value_name = "EXPIRY",
        default_value = "infinite",
        help = "Protection expiry (MediaWiki timestamp or relative expression)"
    )]
    pub(crate) expiry: String,
    #[arg(
        long,
        value_name = "TEXT",
        default_value = "Protect via wikitool",
        help = "Protection reason"
    )]
    pub(crate) reason: String,
    #[arg(long, help = "Preview protect without writing to the wiki")]
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

#[derive(Debug, Args)]
pub(crate) struct UndeleteArgs {
    #[arg(value_name = "TITLE")]
    pub(crate) title: String,
    #[arg(
        long,
        value_name = "TEXT",
        default_value = "Undelete via wikitool",
        help = "Undelete reason"
    )]
    pub(crate) reason: String,
    #[arg(long, help = "Preview undelete without writing to the wiki")]
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

#[derive(Debug, Serialize)]
struct ProtectDryRunReport {
    title: String,
    protections: Vec<String>,
    expiry: String,
    reason: String,
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct UndeleteDryRunReport {
    title: String,
    reason: String,
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct PurgeDryRunReport {
    titles: Vec<String>,
    forcelinkupdate: bool,
    forcerecursivelinkupdate: bool,
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct MoveDryRunReport {
    from: String,
    to: String,
    reason: String,
    no_redirect: bool,
    move_talk: bool,
    move_subpages: bool,
    ignore_warnings: bool,
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct UploadDryRunReport {
    filename: String,
    source_path: String,
    bytes: u64,
    sha256: String,
    comment: String,
    ignore_warnings: bool,
    dry_run: bool,
}

pub(crate) fn run_purge(runtime: &RuntimeOptions, args: PurgeArgs) -> Result<()> {
    let (_paths, config) = resolve_runtime_with_config(runtime)?;
    let titles = collect_titles(&args)?;
    if titles.is_empty() {
        bail!("purge requires at least one title");
    }

    if args.dry_run {
        let report = PurgeDryRunReport {
            titles,
            forcelinkupdate: args.forcelinkupdate,
            forcerecursivelinkupdate: args.forcerecursivelinkupdate,
            dry_run: true,
        };
        if args.format.is_json() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_purge_dry_run(&report);
        }
        return Ok(());
    }

    let mut client = MediaWikiClient::from_config(&config)?;
    login_with_bot_credentials(&mut client, "purge")?;
    let report = client.purge_pages(
        &titles,
        &PurgeOptions {
            forcelinkupdate: args.forcelinkupdate,
            forcerecursivelinkupdate: args.forcerecursivelinkupdate,
        },
    )?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_purge_report(&report);
    }
    Ok(())
}

pub(crate) fn run_upload(runtime: &RuntimeOptions, args: UploadArgs) -> Result<()> {
    let (_paths, config) = resolve_runtime_with_config(runtime)?;
    let filename = resolve_upload_filename(&args)?;
    let options = UploadOptions {
        path: args.path.clone(),
        filename: filename.clone(),
        comment: args.comment.clone(),
        text: args.text.clone(),
        ignore_warnings: args.ignore_warnings,
    };

    if args.dry_run {
        let report = plan_upload(&options)?;
        if args.format.is_json() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_upload_dry_run(&report);
        }
        return Ok(());
    }

    let mut client = MediaWikiClient::from_config(&config)?;
    login_with_bot_credentials(&mut client, "upload")?;
    let report = client.upload_file(&options)?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_upload_report(&report);
    }
    if !report.uploaded {
        bail!("upload did not complete successfully: {}", report.result);
    }
    Ok(())
}

pub(crate) fn run_move(runtime: &RuntimeOptions, args: MoveArgs) -> Result<()> {
    let (_paths, config) = resolve_runtime_with_config(runtime)?;
    let from = args.from.replace('_', " ").trim().to_string();
    let to = args.to.replace('_', " ").trim().to_string();
    if from.is_empty() || to.is_empty() {
        bail!("move requires non-empty FROM and TO titles");
    }

    if args.dry_run {
        let report = MoveDryRunReport {
            from,
            to,
            reason: args.reason.clone(),
            no_redirect: args.no_redirect,
            move_talk: args.move_talk,
            move_subpages: args.move_subpages,
            ignore_warnings: args.ignore_warnings,
            dry_run: true,
        };
        if args.format.is_json() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_move_dry_run(&report);
        }
        return Ok(());
    }

    let options = MovePageOptions {
        from,
        to,
        reason: args.reason.clone(),
        no_redirect: args.no_redirect,
        move_talk: args.move_talk,
        move_subpages: args.move_subpages,
        ignore_warnings: args.ignore_warnings,
    };
    let mut client = MediaWikiClient::from_config(&config)?;
    login_with_bot_credentials(&mut client, "move")?;
    let report = client.move_page(&options)?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_move_report(&report);
    }
    Ok(())
}

pub(crate) fn run_protect(runtime: &RuntimeOptions, args: ProtectArgs) -> Result<()> {
    let (_paths, config) = resolve_runtime_with_config(runtime)?;
    let title = args.title.replace('_', " ").trim().to_string();
    if title.is_empty() {
        bail!("protect requires a non-empty TITLE");
    }
    let protections = parse_protection_pairs(&args.protections)?;

    if args.dry_run {
        let report = ProtectDryRunReport {
            title,
            protections: protections
                .iter()
                .map(|(restriction, level)| format!("{restriction}={level}"))
                .collect(),
            expiry: args.expiry.clone(),
            reason: args.reason.clone(),
            dry_run: true,
        };
        if args.format.is_json() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_protect_dry_run(&report);
        }
        return Ok(());
    }

    let options = ProtectPageOptions {
        title,
        protections,
        expiry: args.expiry.clone(),
        reason: args.reason.clone(),
    };
    let mut client = MediaWikiClient::from_config(&config)?;
    login_with_bot_credentials(&mut client, "protect")?;
    let report = client.protect_page(&options)?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_protect_report(&report);
    }
    Ok(())
}

pub(crate) fn run_undelete(runtime: &RuntimeOptions, args: UndeleteArgs) -> Result<()> {
    let (_paths, config) = resolve_runtime_with_config(runtime)?;
    let title = args.title.replace('_', " ").trim().to_string();
    if title.is_empty() {
        bail!("undelete requires a non-empty TITLE");
    }

    if args.dry_run {
        let report = UndeleteDryRunReport {
            title,
            reason: args.reason.clone(),
            dry_run: true,
        };
        if args.format.is_json() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_undelete_dry_run(&report);
        }
        return Ok(());
    }

    let options = UndeletePageOptions {
        title,
        reason: args.reason.clone(),
    };
    let mut client = MediaWikiClient::from_config(&config)?;
    login_with_bot_credentials(&mut client, "undelete")?;
    let report = client.undelete_page(&options)?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_undelete_report(&report);
    }
    Ok(())
}

fn parse_protection_pairs(raw: &[String]) -> Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    for entry in raw {
        let Some((restriction, level)) = entry.split_once('=') else {
            bail!("--protection expects TYPE=LEVEL, got `{entry}`");
        };
        let restriction = restriction.trim();
        if restriction.is_empty() {
            bail!("--protection expects a non-empty restriction type in `{entry}`");
        }
        pairs.push((restriction.to_string(), level.trim().to_string()));
    }
    Ok(pairs)
}

fn collect_titles(args: &PurgeArgs) -> Result<Vec<String>> {
    let mut titles = Vec::new();
    titles.extend(args.positional_titles.iter().cloned());
    titles.extend(args.titles.iter().cloned());
    if let Some(path) = &args.titles_file {
        let body = fs::read_to_string(path)
            .with_context(|| format!("failed to read titles file {}", normalize_path(path)))?;
        for line in body.lines() {
            let title = line.trim();
            if !title.is_empty() && !title.starts_with('#') {
                titles.push(title.to_string());
            }
        }
    }
    Ok(titles
        .into_iter()
        .map(|title| title.replace('_', " ").trim().to_string())
        .filter(|title| !title.is_empty())
        .collect())
}

fn resolve_upload_filename(args: &UploadArgs) -> Result<String> {
    if let Some(filename) = &args.filename {
        let normalized = filename.trim();
        if normalized.is_empty() {
            bail!("--filename must be non-empty");
        }
        return Ok(normalized.to_string());
    }
    let name = args
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow::anyhow!("could not derive upload filename from path"))?;
    Ok(name.to_string())
}

fn plan_upload(options: &UploadOptions) -> Result<UploadDryRunReport> {
    let bytes = fs::read(&options.path).with_context(|| {
        format!(
            "failed to read upload source {}",
            normalize_path(&options.path)
        )
    })?;
    Ok(UploadDryRunReport {
        filename: options.filename.clone(),
        source_path: normalize_path(&options.path),
        bytes: u64::try_from(bytes.len()).context("upload source is too large")?,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        comment: options.comment.clone(),
        ignore_warnings: options.ignore_warnings,
        dry_run: true,
    })
}

fn login_with_bot_credentials(client: &mut MediaWikiClient, action: &str) -> Result<()> {
    let username = env::var("WIKITOOL_BOT_USER")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{action} requires WIKITOOL_BOT_USER"))?;
    let password = env::var("WIKITOOL_BOT_PASS")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{action} requires WIKITOOL_BOT_PASS"))?;
    client.login(&username, &password)
}

fn print_protect_dry_run(report: &ProtectDryRunReport) {
    println!("protect");
    println!("dry_run: true");
    println!("title: {}", report.title);
    for protection in &report.protections {
        println!("protection: {protection}");
    }
    println!("expiry: {}", report.expiry);
    println!("reason: {}", report.reason);
}

fn print_protect_report(report: &ProtectReport) {
    println!("protect");
    println!("title: {}", report.title);
    println!("reason: {}", report.reason);
    for protection in &report.protections {
        println!(
            "protection: {}={} expiry={}",
            protection.restriction, protection.level, protection.expiry
        );
    }
    println!("request_count: {}", report.request_count);
}

fn print_undelete_dry_run(report: &UndeleteDryRunReport) {
    println!("undelete");
    println!("dry_run: true");
    println!("title: {}", report.title);
    println!("reason: {}", report.reason);
}

fn print_undelete_report(report: &UndeleteReport) {
    println!("undelete");
    println!("title: {}", report.title);
    println!("reason: {}", report.reason);
    println!("revisions: {}", report.revisions);
    println!("file_versions: {}", report.file_versions);
    println!("request_count: {}", report.request_count);
}

fn print_move_dry_run(report: &MoveDryRunReport) {
    println!("move");
    println!("dry_run: true");
    println!("from: {}", report.from);
    println!("to: {}", report.to);
    println!("reason: {}", report.reason);
    println!("no_redirect: {}", report.no_redirect);
    println!("move_talk: {}", report.move_talk);
    println!("move_subpages: {}", report.move_subpages);
    println!("ignore_warnings: {}", report.ignore_warnings);
}

fn print_move_report(report: &MoveReport) {
    println!("move");
    println!("requested_from: {}", report.requested_from);
    println!("requested_to: {}", report.requested_to);
    println!("from: {}", report.from);
    println!("to: {}", report.to);
    println!("reason: {}", report.reason);
    println!("redirect_created: {}", report.redirect_created);
    println!("ignore_warnings: {}", report.ignore_warnings);
    println!("talk_moved: {}", report.talk_moved);
    if let Some(talk_from) = &report.talk_from {
        println!("talk_from: {talk_from}");
    }
    if let Some(talk_to) = &report.talk_to {
        println!("talk_to: {talk_to}");
    }
    if let Some(warnings) = &report.warnings {
        println!("warnings: {warnings}");
    }
    println!("request_count: {}", report.request_count);
}

fn print_purge_dry_run(report: &PurgeDryRunReport) {
    println!("purge");
    println!("dry_run: true");
    println!("titles.count: {}", report.titles.len());
    for title in &report.titles {
        println!("titles.item: {title}");
    }
    println!("forcelinkupdate: {}", report.forcelinkupdate);
    println!(
        "forcerecursivelinkupdate: {}",
        report.forcerecursivelinkupdate
    );
}

fn print_purge_report(report: &PurgeReport) {
    println!("purge");
    println!("titles.count: {}", report.titles.len());
    println!("forcelinkupdate: {}", report.forcelinkupdate);
    println!(
        "forcerecursivelinkupdate: {}",
        report.forcerecursivelinkupdate
    );
    println!("request_count: {}", report.request_count);
    for page in &report.pages {
        println!(
            "page: {} status={} purged={} linkupdate={} missing={} invalid={}",
            page.title, page.status, page.purged, page.linkupdate, page.missing, page.invalid
        );
    }
}

fn print_upload_dry_run(report: &UploadDryRunReport) {
    println!("upload");
    println!("dry_run: true");
    println!("filename: {}", report.filename);
    println!("source_path: {}", report.source_path);
    println!("bytes: {}", report.bytes);
    println!("sha256: {}", report.sha256);
    println!("comment: {}", report.comment);
    println!("ignore_warnings: {}", report.ignore_warnings);
}

fn print_upload_report(report: &UploadReport) {
    println!("upload");
    println!("filename: {}", report.filename);
    println!("source_path: {}", report.source_path);
    println!("bytes: {}", report.bytes);
    println!("sha256: {}", report.sha256);
    println!("comment: {}", report.comment);
    println!("ignore_warnings: {}", report.ignore_warnings);
    println!("request_count: {}", report.request_count);
    println!("result: {}", report.result);
    println!("uploaded: {}", report.uploaded);
    if let Some(warnings) = &report.warnings {
        println!("warnings: {warnings}");
    }
}
