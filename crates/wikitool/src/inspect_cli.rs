use std::collections::BTreeMap;

use anyhow::Result;
use clap::Args;
use serde::Serialize;
use wikitool_core::inspect::{
    NetInspectOptions, NetInspectResult, NetResource, NetSummary, SeoInspectResult, net_inspect,
    seo_inspect,
};

use crate::cli_support::resolve_runtime_with_config;
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct SeoArgs {
    target: String,
    #[arg(long, help = "Output JSON for AI consumption")]
    json: bool,
    #[arg(long, help = "Omit metadata from JSON output")]
    no_meta: bool,
    #[arg(long, value_name = "URL", help = "Override target URL")]
    url: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct NetArgs {
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

#[derive(Debug, Serialize)]
struct SeoInspectJson<'a> {
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<&'a BTreeMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical: Option<&'a str>,
    missing: &'a [String],
}

#[derive(Debug, Serialize)]
struct NetInspectJson<'a> {
    url: &'a str,
    total_resources: usize,
    inspected: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<NetSummaryJson<'a>>,
    resources: Vec<NetResourceJson<'a>>,
}

#[derive(Debug, Serialize)]
struct NetSummaryJson<'a> {
    known_bytes: u64,
    unknown_count: usize,
    largest: Vec<NetResourceJson<'a>>,
    cache_warnings: &'a [String],
}

#[derive(Debug, Serialize)]
struct NetResourceJson<'a> {
    url: &'a str,
    resource_type: &'a str,
    tag: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    age: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    x_cache: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    x_varnish: Option<&'a str>,
}

pub(crate) fn run_seo(runtime: &RuntimeOptions, args: SeoArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let result = seo_inspect(
        &args.target,
        args.url.as_deref(),
        config.wiki_url().as_deref(),
        Some(config.article_path()),
    )?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&seo_json_output(&result, args.no_meta))?
        );
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

pub(crate) fn run_net(runtime: &RuntimeOptions, args: NetArgs) -> Result<()> {
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

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&net_json_output(&result, args.no_meta))?
        );
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

fn seo_json_output<'a>(result: &'a SeoInspectResult, no_meta: bool) -> SeoInspectJson<'a> {
    SeoInspectJson {
        url: &result.url,
        title: result.title.as_deref(),
        meta: if no_meta { None } else { Some(&result.meta) },
        canonical: result.canonical.as_deref(),
        missing: &result.missing,
    }
}

fn net_json_output<'a>(result: &'a NetInspectResult, no_meta: bool) -> NetInspectJson<'a> {
    NetInspectJson {
        url: &result.url,
        total_resources: result.total_resources,
        inspected: result.inspected,
        summary: if no_meta {
            None
        } else {
            Some(net_summary_json(&result.summary))
        },
        resources: result
            .resources
            .iter()
            .map(|resource| net_resource_json(resource, no_meta))
            .collect(),
    }
}

fn net_summary_json<'a>(summary: &'a NetSummary) -> NetSummaryJson<'a> {
    NetSummaryJson {
        known_bytes: summary.known_bytes,
        unknown_count: summary.unknown_count,
        largest: summary
            .largest
            .iter()
            .map(|resource| net_resource_json(resource, false))
            .collect(),
        cache_warnings: &summary.cache_warnings,
    }
}

fn net_resource_json<'a>(resource: &'a NetResource, no_meta: bool) -> NetResourceJson<'a> {
    NetResourceJson {
        url: &resource.url,
        resource_type: &resource.resource_type,
        tag: &resource.tag,
        size_bytes: if no_meta { None } else { resource.size_bytes },
        content_type: if no_meta {
            None
        } else {
            resource.content_type.as_deref()
        },
        cache_control: if no_meta {
            None
        } else {
            resource.cache_control.as_deref()
        },
        age: if no_meta {
            None
        } else {
            resource.age.as_deref()
        },
        x_cache: if no_meta {
            None
        } else {
            resource.x_cache.as_deref()
        },
        x_varnish: if no_meta {
            None
        } else {
            resource.x_varnish.as_deref()
        },
    }
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
    use clap::{Parser, Subcommand};
    use serde_json::json;

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
                assert_eq!(args.target, "Main Page");
                assert!(args.json);
            }
            InspectCommand::Net(_) => panic!("expected seo command"),
        }
    }

    #[test]
    fn seo_inspect_alias_no_longer_parses() {
        assert!(
            InspectCli::try_parse_from(["inspect-cli", "seo", "inspect", "Main Page"]).is_err()
        );
    }

    #[test]
    fn net_direct_form_parses_without_subcommand() {
        let cli = InspectCli::try_parse_from(["inspect-cli", "net", "Main Page", "--limit", "10"])
            .expect("parse net direct form");

        match cli.command {
            InspectCommand::Net(args) => {
                assert_eq!(args.target, "Main Page");
                assert_eq!(args.limit, 10);
            }
            InspectCommand::Seo(_) => panic!("expected net command"),
        }
    }

    #[test]
    fn net_inspect_alias_no_longer_parses() {
        assert!(
            InspectCli::try_parse_from(["inspect-cli", "net", "inspect", "Main Page"]).is_err()
        );
    }

    #[test]
    fn seo_no_meta_json_omits_meta_map() {
        let result = SeoInspectResult {
            url: "https://wiki.example.org/Alpha".to_string(),
            title: Some("Alpha".to_string()),
            meta: BTreeMap::from([("description".to_string(), vec!["Summary".to_string()])]),
            canonical: Some("https://wiki.example.org/Alpha".to_string()),
            missing: vec!["twitter:card".to_string()],
        };

        let value = serde_json::to_value(seo_json_output(&result, true)).expect("serialize seo");

        assert_eq!(value["url"], json!("https://wiki.example.org/Alpha"));
        assert_eq!(value["canonical"], json!("https://wiki.example.org/Alpha"));
        assert!(value.get("meta").is_none());
    }

    #[test]
    fn net_no_meta_json_omits_summary_and_probe_fields() {
        let resource = NetResource {
            url: "https://wiki.example.org/load.js".to_string(),
            resource_type: "script".to_string(),
            tag: "script".to_string(),
            size_bytes: Some(42),
            content_type: Some("text/javascript".to_string()),
            cache_control: Some("max-age=60".to_string()),
            age: Some("12".to_string()),
            x_cache: Some("hit".to_string()),
            x_varnish: Some("123".to_string()),
        };
        let result = NetInspectResult {
            url: "https://wiki.example.org/Alpha".to_string(),
            total_resources: 3,
            inspected: 1,
            summary: NetSummary {
                known_bytes: 42,
                unknown_count: 0,
                largest: vec![resource.clone()],
                cache_warnings: vec![],
            },
            resources: vec![resource],
        };

        let value = serde_json::to_value(net_json_output(&result, true)).expect("serialize net");
        let first = &value["resources"][0];

        assert!(value.get("summary").is_none());
        assert_eq!(first["url"], json!("https://wiki.example.org/load.js"));
        assert_eq!(first["resource_type"], json!("script"));
        assert_eq!(first["tag"], json!("script"));
        assert!(first.get("size_bytes").is_none());
        assert!(first.get("content_type").is_none());
    }
}
