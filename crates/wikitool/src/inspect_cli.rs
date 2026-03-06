use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use wikitool_core::inspect::{
    LighthouseOutputFormat, LighthouseRunOptions, NetInspectOptions, find_lighthouse_binary,
    lighthouse_version, net_inspect, run_lighthouse, seo_inspect,
};

use crate::cli_support::resolve_runtime_with_config;
use crate::{MIGRATIONS_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct SeoArgs {
    #[command(subcommand)]
    command: SeoSubcommand,
}

#[derive(Debug, Subcommand)]
enum SeoSubcommand {
    Inspect {
        target: String,
        #[arg(long, help = "Output JSON for AI consumption")]
        json: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
        #[arg(long, value_name = "URL", help = "Override target URL")]
        url: Option<String>,
    },
}

#[derive(Debug, Args)]
pub(crate) struct NetArgs {
    #[command(subcommand)]
    command: NetSubcommand,
}

#[derive(Debug, Subcommand)]
enum NetSubcommand {
    Inspect {
        target: String,
        #[arg(
            long,
            default_value_t = 25,
            value_name = "N",
            help = "Limit number of resources to probe"
        )]
        limit: usize,
        #[arg(long, help = "Skip HEAD probes (faster, no size/cache info)")]
        no_probe: bool,
        #[arg(long, help = "Output JSON for AI consumption")]
        json: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
        #[arg(long, value_name = "URL", help = "Override target URL")]
        url: Option<String>,
    },
}

#[derive(Debug, Args)]
pub(crate) struct PerfArgs {
    #[command(subcommand)]
    command: PerfSubcommand,
}

#[derive(Debug, Subcommand)]
enum PerfSubcommand {
    Lighthouse {
        target: Option<String>,
        #[arg(
            long,
            default_value = "html",
            value_name = "FORMAT",
            help = "Output format: html|json"
        )]
        output: String,
        #[arg(long, value_name = "PATH", help = "Report output path")]
        out: Option<PathBuf>,
        #[arg(long, value_name = "LIST", help = "Comma-separated categories")]
        categories: Option<String>,
        #[arg(long, value_name = "FLAGS", help = "Pass Chrome flags to Lighthouse")]
        chrome_flags: Option<String>,
        #[arg(long, help = "Print resolved Lighthouse binary + version and exit")]
        show_version: bool,
        #[arg(long, help = "Output JSON summary")]
        json: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
        #[arg(long, value_name = "URL", help = "Override target URL")]
        url: Option<String>,
    },
}

pub(crate) fn run_seo(runtime: &RuntimeOptions, args: SeoArgs) -> Result<()> {
    match args.command {
        SeoSubcommand::Inspect {
            target,
            json,
            no_meta: _,
            url,
        } => run_seo_inspect(runtime, &target, json, url.as_deref()),
    }
}

pub(crate) fn run_net(runtime: &RuntimeOptions, args: NetArgs) -> Result<()> {
    match args.command {
        NetSubcommand::Inspect {
            target,
            limit,
            no_probe,
            json,
            no_meta: _,
            url,
        } => run_net_inspect(
            runtime,
            &target,
            json,
            url.as_deref(),
            &NetInspectOptions {
                limit,
                probe: !no_probe,
            },
        ),
    }
}

pub(crate) fn run_perf(runtime: &RuntimeOptions, args: PerfArgs) -> Result<()> {
    match args.command {
        PerfSubcommand::Lighthouse {
            target,
            output,
            out,
            categories,
            chrome_flags,
            show_version,
            json,
            no_meta: _,
            url,
        } => run_perf_lighthouse(
            runtime,
            target,
            output.as_str(),
            out.as_deref(),
            categories.as_deref(),
            chrome_flags.as_deref(),
            show_version,
            json,
            url.as_deref(),
        ),
    }
}

fn run_seo_inspect(
    runtime: &RuntimeOptions,
    target: &str,
    json: bool,
    override_url: Option<&str>,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let result = seo_inspect(
        target,
        override_url,
        config.wiki_url().as_deref(),
        Some(config.article_path()),
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("seo inspect");
        println!("url: {}", result.url);
        println!("title: {}", result.title.as_deref().unwrap_or("<missing>"));
        println!(
            "canonical: {}",
            result.canonical.as_deref().unwrap_or("<missing>")
        );
        print_meta_value("description", result.meta.get("description"));
        print_meta_value("og:title", result.meta.get("og:title"));
        print_meta_value("og:description", result.meta.get("og:description"));
        print_meta_value("og:type", result.meta.get("og:type"));
        print_meta_value("og:image", result.meta.get("og:image"));
        print_meta_value("og:url", result.meta.get("og:url"));
        print_meta_value("twitter:card", result.meta.get("twitter:card"));
        print_meta_value("twitter:title", result.meta.get("twitter:title"));
        print_meta_value(
            "twitter:description",
            result.meta.get("twitter:description"),
        );
        print_meta_value("twitter:image", result.meta.get("twitter:image"));
        if result.missing.is_empty() {
            println!("missing: <none>");
        } else {
            println!("missing.count: {}", result.missing.len());
            for item in &result.missing {
                println!("missing.item: {item}");
            }
        }
    }

    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_net_inspect(
    runtime: &RuntimeOptions,
    target: &str,
    json: bool,
    override_url: Option<&str>,
    options: &NetInspectOptions,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let result = net_inspect(
        target,
        override_url,
        config.wiki_url().as_deref(),
        Some(config.article_path()),
        options,
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("net inspect");
        println!("url: {}", result.url);
        println!("resources.total: {}", result.total_resources);
        println!("resources.inspected: {}", result.inspected);
        println!("known_bytes: {}", result.summary.known_bytes);
        println!("unknown_sizes: {}", result.summary.unknown_count);
        if result.summary.largest.is_empty() {
            println!("largest: <none>");
        } else {
            for entry in &result.summary.largest {
                println!(
                    "largest.resource: size={} type={} url={}",
                    entry
                        .size_bytes
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    entry.resource_type,
                    entry.url
                );
            }
        }
        if result.summary.cache_warnings.is_empty() {
            println!("cache_warnings: <none>");
        } else {
            println!(
                "cache_warnings.count: {}",
                result.summary.cache_warnings.len()
            );
            for warning in &result.summary.cache_warnings {
                println!("cache_warning: {warning}");
            }
        }
    }

    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_perf_lighthouse(
    runtime: &RuntimeOptions,
    target: Option<String>,
    output: &str,
    out: Option<&Path>,
    categories: Option<&str>,
    chrome_flags: Option<&str>,
    show_version: bool,
    json: bool,
    override_url: Option<&str>,
) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let Some(lighthouse_path) = find_lighthouse_binary(&paths.project_root) else {
        bail!("lighthouse not found on PATH. Install with: npm install -g lighthouse");
    };

    if show_version {
        let info = lighthouse_version(&lighthouse_path)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&info)?);
        } else {
            println!("perf lighthouse");
            println!("path: {}", info.path);
            println!("version: {}", info.version.as_deref().unwrap_or("unknown"));
            println!("code: {}", info.code);
            if !info.stderr.trim().is_empty() {
                println!("stderr: {}", info.stderr.trim());
            }
        }
        println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
        if info.code != 0 {
            bail!("failed to resolve lighthouse version");
        }
        return Ok(());
    }

    let output_format = LighthouseOutputFormat::parse(output)?;
    let report = run_lighthouse(
        &paths.project_root,
        &lighthouse_path,
        &LighthouseRunOptions {
            target,
            target_url_override: override_url.map(ToString::to_string),
            default_wiki_url: config.wiki_url(),
            article_path: Some(config.article_path_owned()),
            output_format,
            output_path_override: out.map(Path::to_path_buf),
            categories: parse_csv_list(categories),
            chrome_flags: chrome_flags.map(ToString::to_string),
        },
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("perf lighthouse");
        println!("url: {}", report.url);
        println!("format: {}", report.format);
        println!("report_path: {}", report.report_path);
        println!(
            "report_bytes: {}",
            report
                .report_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        );
        if report.categories.is_empty() {
            println!("categories: <default>");
        } else {
            println!("categories: {}", report.categories.join(","));
        }
        if report.ignored_windows_cleanup_failure {
            println!(
                "warning: ignored known Windows chrome-launcher cleanup failure (report generated)"
            );
        }
    }

    println!("policy: {MIGRATIONS_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn print_meta_value(label: &str, values: Option<&Vec<String>>) {
    match values {
        Some(values) if !values.is_empty() => {
            println!("meta.{label}: {}", values[0]);
            if values.len() > 1 {
                println!("meta.{label}.extra_count: {}", values.len() - 1);
            }
        }
        _ => println!("meta.{label}: <missing>"),
    }
}

fn parse_csv_list(value: Option<&str>) -> Vec<String> {
    let mut output = Vec::new();
    let Some(raw) = value else {
        return output;
    };
    for part in raw.split(',') {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            output.push(trimmed.to_string());
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::parse_csv_list;

    #[test]
    fn parse_csv_list_ignores_empty_entries() {
        assert_eq!(
            parse_csv_list(Some("seo, , performance ,, accessibility")),
            vec![
                "seo".to_string(),
                "performance".to_string(),
                "accessibility".to_string(),
            ]
        );
    }

    #[test]
    fn parse_csv_list_returns_empty_when_missing() {
        assert!(parse_csv_list(None).is_empty());
    }
}
