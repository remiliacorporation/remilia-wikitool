use anyhow::Result;
use clap::{Args, Subcommand};
use wikitool_core::inspect::{NetInspectOptions, net_inspect, seo_inspect};

use crate::cli_support::resolve_runtime_with_config;
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub(crate) struct SeoArgs {
    target: Option<String>,
    #[arg(long, help = "Output JSON for AI consumption")]
    json: bool,
    #[arg(long, help = "Omit metadata from JSON output")]
    no_meta: bool,
    #[arg(long, value_name = "URL", help = "Override target URL")]
    url: Option<String>,
    #[command(subcommand)]
    command: Option<SeoSubcommand>,
}

#[derive(Debug, Subcommand)]
enum SeoSubcommand {
    #[command(about = "Deprecated alias for `wikitool seo`", hide = true)]
    Inspect(SeoInspectArgs),
}

#[derive(Debug, Clone, Args)]
struct SeoInspectArgs {
    target: String,
    #[arg(long, help = "Output JSON for AI consumption")]
    json: bool,
    #[arg(long, help = "Omit metadata from JSON output")]
    no_meta: bool,
    #[arg(long, value_name = "URL", help = "Override target URL")]
    url: Option<String>,
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub(crate) struct NetArgs {
    target: Option<String>,
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
    #[command(subcommand)]
    command: Option<NetSubcommand>,
}

#[derive(Debug, Subcommand)]
enum NetSubcommand {
    #[command(about = "Deprecated alias for `wikitool net`", hide = true)]
    Inspect(NetInspectArgs),
}

#[derive(Debug, Clone, Args)]
struct NetInspectArgs {
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
}

pub(crate) fn run_seo(runtime: &RuntimeOptions, args: SeoArgs) -> Result<()> {
    match args.command {
        Some(SeoSubcommand::Inspect(args)) => {
            print_deprecated_inspect_warning("wikitool seo inspect", "wikitool seo");
            run_seo_inspect(runtime, &args)
        }
        None => run_seo_inspect(runtime, &seo_request_from_direct_args(args)?),
    }
}

pub(crate) fn run_net(runtime: &RuntimeOptions, args: NetArgs) -> Result<()> {
    match args.command {
        Some(NetSubcommand::Inspect(args)) => {
            print_deprecated_inspect_warning("wikitool net inspect", "wikitool net");
            run_net_inspect(runtime, &args)
        }
        None => run_net_inspect(runtime, &net_request_from_direct_args(args)?),
    }
}

fn seo_request_from_direct_args(args: SeoArgs) -> Result<SeoInspectArgs> {
    let target = args
        .target
        .ok_or_else(|| anyhow::anyhow!("seo requires a target"))?;
    Ok(SeoInspectArgs {
        target,
        json: args.json,
        no_meta: args.no_meta,
        url: args.url,
    })
}

fn net_request_from_direct_args(args: NetArgs) -> Result<NetInspectArgs> {
    let target = args
        .target
        .ok_or_else(|| anyhow::anyhow!("net requires a target"))?;
    Ok(NetInspectArgs {
        target,
        limit: args.limit,
        no_probe: args.no_probe,
        json: args.json,
        no_meta: args.no_meta,
        url: args.url,
    })
}

fn run_seo_inspect(runtime: &RuntimeOptions, args: &SeoInspectArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let result = seo_inspect(
        &args.target,
        args.url.as_deref(),
        config.wiki_url().as_deref(),
        Some(config.article_path()),
    )?;

    let _ = args.no_meta;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("seo");
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

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_net_inspect(runtime: &RuntimeOptions, args: &NetInspectArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let options = NetInspectOptions {
        limit: args.limit,
        probe: !args.no_probe,
    };
    let result = net_inspect(
        &args.target,
        args.url.as_deref(),
        config.wiki_url().as_deref(),
        Some(config.article_path()),
        &options,
    )?;

    let _ = args.no_meta;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("net");
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

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn print_deprecated_inspect_warning(current: &str, preferred: &str) {
    eprintln!("warning: `{current}` is deprecated; use `{preferred}`");
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

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct InspectCli {
        #[command(subcommand)]
        command: InspectCommand,
    }

    #[derive(Debug, Subcommand)]
    enum InspectCommand {
        Seo(SeoArgs),
        Net(NetArgs),
    }

    #[test]
    fn seo_direct_form_parses_without_subcommand() {
        let cli = InspectCli::try_parse_from(["inspect-cli", "seo", "Main Page", "--json"])
            .expect("parse seo direct form");

        match cli.command {
            InspectCommand::Seo(args) => {
                assert!(args.command.is_none());
                assert_eq!(args.target.as_deref(), Some("Main Page"));
                assert!(args.json);
            }
            InspectCommand::Net(_) => panic!("expected seo command"),
        }
    }

    #[test]
    fn seo_legacy_inspect_alias_parses() {
        let cli = InspectCli::try_parse_from(["inspect-cli", "seo", "inspect", "Main Page"])
            .expect("parse seo inspect alias");

        match cli.command {
            InspectCommand::Seo(args) => match args.command {
                Some(SeoSubcommand::Inspect(args)) => assert_eq!(args.target, "Main Page"),
                None => panic!("expected legacy seo inspect alias"),
            },
            InspectCommand::Net(_) => panic!("expected seo command"),
        }
    }

    #[test]
    fn net_direct_form_parses_without_subcommand() {
        let cli = InspectCli::try_parse_from(["inspect-cli", "net", "Main Page", "--limit", "10"])
            .expect("parse net direct form");

        match cli.command {
            InspectCommand::Net(args) => {
                assert!(args.command.is_none());
                assert_eq!(args.target.as_deref(), Some("Main Page"));
                assert_eq!(args.limit, 10);
            }
            InspectCommand::Seo(_) => panic!("expected net command"),
        }
    }

    #[test]
    fn net_legacy_inspect_alias_parses() {
        let cli = InspectCli::try_parse_from(["inspect-cli", "net", "inspect", "Main Page"])
            .expect("parse net inspect alias");

        match cli.command {
            InspectCommand::Net(args) => match args.command {
                Some(NetSubcommand::Inspect(args)) => assert_eq!(args.target, "Main Page"),
                None => panic!("expected legacy net inspect alias"),
            },
            InspectCommand::Seo(_) => panic!("expected net command"),
        }
    }
}
