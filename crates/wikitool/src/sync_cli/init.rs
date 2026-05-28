use anyhow::{Context, Result};
use wikitool_core::config::{
    DEFAULT_WIKI_API_URL, DEFAULT_WIKI_URL, WikiConfigPatch, derive_wiki_url, load_config,
    patch_wiki_config,
};
use wikitool_core::runtime::{InitOptions, init_layout};
use wikitool_core::sync::discover_custom_namespaces;

use crate::cli_support::{normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::InitArgs;
use super::shared::materialize_custom_namespace_dirs;

pub(crate) fn run_init(runtime: &RuntimeOptions, args: InitArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let report = init_layout(
        &paths,
        &InitOptions {
            include_templates: args.templates,
            materialize_config: !args.no_config,
            materialize_parser_config: !args.no_parser_config,
            force: args.force,
        },
    )?;
    let mut wrote_namespace_config = false;
    let mut discovered_namespaces = 0usize;
    let mut created_namespace_dirs = 0usize;
    let mut namespace_discovery_status = "skipped (--no-config)".to_string();
    let mut persisted_api_url = false;
    let mut persisted_wiki_url = false;

    if !args.no_config {
        let config = load_config(&paths.config_path)
            .with_context(|| format!("failed to load {}", normalize_path(&paths.config_path)))?;
        let resolved_api_url = args
            .api_url
            .clone()
            .or_else(|| config.api_url_owned())
            .or_else(|| Some(DEFAULT_WIKI_API_URL.to_string()));
        let resolved_wiki_url = args
            .wiki_url
            .clone()
            .or_else(|| args.api_url.as_deref().and_then(derive_wiki_url))
            .or_else(|| config.wiki_url())
            .or_else(|| resolved_api_url.as_deref().and_then(derive_wiki_url))
            .or_else(|| Some(DEFAULT_WIKI_URL.to_string()));
        let mut discovery_config = config.clone();
        if let Some(api_url) = &resolved_api_url {
            discovery_config.wiki.api_url = Some(api_url.clone());
        }
        if let Some(wiki_url) = &resolved_wiki_url {
            discovery_config.wiki.url = Some(wiki_url.clone());
        }
        namespace_discovery_status = "pending".to_string();
        let discovered = match discover_custom_namespaces(&discovery_config) {
            Ok(ns) => ns,
            Err(_) if resolved_api_url.is_none() => {
                namespace_discovery_status = "skipped (no API URL configured)".to_string();
                Vec::new()
            }
            Err(err) => {
                namespace_discovery_status = format!("failed: {err:#}");
                Vec::new()
            }
        };
        let mut patch = WikiConfigPatch {
            set_url: None,
            set_api_url: None,
            set_custom_namespaces: Some(discovered.clone()),
        };
        if args.api_url.is_some() || config.wiki.api_url.is_none() {
            patch.set_api_url = resolved_api_url;
        }
        if args.wiki_url.is_some() || args.api_url.is_some() || config.wiki.url.is_none() {
            patch.set_url = resolved_wiki_url;
        }
        wrote_namespace_config = patch_wiki_config(&paths.config_path, &patch)
            .with_context(|| format!("failed to update {}", normalize_path(&paths.config_path)))?;

        let refreshed = load_config(&paths.config_path)
            .with_context(|| format!("failed to load {}", normalize_path(&paths.config_path)))?;
        persisted_api_url = refreshed.wiki.api_url.is_some();
        persisted_wiki_url = refreshed.wiki.url.is_some();
        let created = materialize_custom_namespace_dirs(&paths, &refreshed)?;
        created_namespace_dirs = created.len();
        discovered_namespaces = discovered.len();
        if namespace_discovery_status.starts_with("skipped")
            || namespace_discovery_status.starts_with("failed")
        {
            // Keep the status set by the error handler above.
        } else {
            namespace_discovery_status = "ok".to_string();
        }
    }

    println!("Initialized wikitool runtime layout");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("wiki_content: {}", normalize_path(&paths.wiki_content_dir));
    println!("templates: {}", normalize_path(&paths.templates_dir));
    println!("state_dir: {}", normalize_path(&paths.state_dir));
    println!("data_dir: {}", normalize_path(&paths.data_dir));
    println!("db_path: {}", normalize_path(&paths.db_path));
    println!("config_path: {}", normalize_path(&paths.config_path));
    println!(
        "parser_config_path: {}",
        normalize_path(&paths.parser_config_path)
    );
    println!("created_dirs: {}", report.created_dirs.len());
    println!("wrote_config: {}", report.wrote_config);
    println!("wrote_parser_config: {}", report.wrote_parser_config);
    println!("namespace_discovery: {namespace_discovery_status}");
    println!("discovered_custom_namespaces: {discovered_namespaces}");
    println!("wrote_namespace_config: {wrote_namespace_config}");
    println!("created_namespace_dirs: {created_namespace_dirs}");
    println!("persisted_wiki_api_url: {persisted_api_url}");
    println!("persisted_wiki_url: {persisted_wiki_url}");
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    Ok(())
}
