use anyhow::{Context, Result};
use wikitool_core::config::{WikiConfigPatch, load_config, patch_wiki_config};
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
        let resolved_api_url = config.api_url_owned();
        let resolved_wiki_url = config.wiki_url();
        let discovered = match discover_custom_namespaces(&config) {
            Ok(ns) => ns,
            Err(_) if config.api_url_owned().is_none() => {
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
        if config.wiki.api_url.is_none() {
            patch.set_api_url = resolved_api_url;
            persisted_api_url = patch.set_api_url.is_some();
        }
        if config.wiki.url.is_none() {
            patch.set_url = resolved_wiki_url;
            persisted_wiki_url = patch.set_url.is_some();
        }
        wrote_namespace_config = patch_wiki_config(&paths.config_path, &patch)
            .with_context(|| format!("failed to update {}", normalize_path(&paths.config_path)))?;

        let refreshed = load_config(&paths.config_path)
            .with_context(|| format!("failed to load {}", normalize_path(&paths.config_path)))?;
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
