use anyhow::Result;
use wikitool_core::profile::{
    AuthoringSurface, AuthoringSurfaceOptions, build_authoring_surface_with_config,
    sync_authoring_surface_with_config,
};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::join_or_none;
use super::summary::summarize_authoring_surface;
use super::*;
pub(super) fn run_wiki_surface(runtime: &RuntimeOptions, args: WikiSurfaceArgs) -> Result<()> {
    match args.command {
        WikiSurfaceSubcommand::Sync(args) => run_wiki_surface_sync(runtime, args),
        WikiSurfaceSubcommand::Show(args) => run_wiki_surface_show(runtime, args),
    }
}

fn run_wiki_surface_sync(runtime: &RuntimeOptions, args: WikiSurfaceFormatArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let options = surface_options(&args)?;
    let surface = sync_authoring_surface_with_config(&paths, &config, options)?;
    print_authoring_surface(
        "wiki surface sync",
        runtime,
        &paths,
        &surface,
        args.format,
        args.view,
    )
}

fn run_wiki_surface_show(runtime: &RuntimeOptions, args: WikiSurfaceFormatArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let options = surface_options(&args)?;
    let surface = build_authoring_surface_with_config(&paths, &config, options)?;
    print_authoring_surface(
        "wiki surface show",
        runtime,
        &paths,
        &surface,
        args.format,
        args.view,
    )
}

fn surface_options(args: &WikiSurfaceFormatArgs) -> Result<AuthoringSurfaceOptions> {
    if args.template_limit == 0 {
        anyhow::bail!("wiki surface requires --template-limit >= 1");
    }
    if args.extension_limit == 0 {
        anyhow::bail!("wiki surface requires --extension-limit >= 1");
    }
    if args.module_limit == 0 {
        anyhow::bail!("wiki surface requires --module-limit >= 1");
    }
    if args.asset_limit == 0 {
        anyhow::bail!("wiki surface requires --asset-limit >= 1");
    }
    if args.extension_tag_limit == 0 {
        anyhow::bail!("wiki surface requires --extension-tag-limit >= 1");
    }
    Ok(AuthoringSurfaceOptions {
        template_limit: args.template_limit,
        template_example_limit: args.template_example_limit,
        module_limit: args.module_limit,
        asset_limit: args.asset_limit,
        extension_limit: args.extension_limit,
        extension_tag_limit: args.extension_tag_limit,
    })
}

fn print_authoring_surface(
    heading: &str,
    runtime: &RuntimeOptions,
    paths: &wikitool_core::runtime::ResolvedPaths,
    surface: &AuthoringSurface,
    format: OutputFormat,
    view: WikiJsonView,
) -> Result<()> {
    if format.is_json() {
        if view.is_full() {
            println!("{}", serde_json::to_string_pretty(surface)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&summarize_authoring_surface(surface))?
            );
        }
        return Ok(());
    }

    println!("{heading}");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("profile_id: {}", surface.profile_id);
    println!(
        "wiki_id: {}",
        surface.wiki_id.as_deref().unwrap_or("<none>")
    );
    println!(
        "wiki_url: {}",
        surface.wiki_url.as_deref().unwrap_or("<none>")
    );
    println!(
        "capabilities_refreshed_at: {}",
        surface
            .capabilities_refreshed_at
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "template_catalog_refreshed_at: {}",
        surface
            .template_catalog_refreshed_at
            .as_deref()
            .unwrap_or("<none>")
    );
    println!("template_source: {}", surface.template_source);
    println!("template_count_total: {}", surface.template_count_total);
    println!(
        "template_count_returned: {}",
        surface.template_count_returned
    );
    println!("module_count_total: {}", surface.module_count_total);
    println!("module_count_returned: {}", surface.module_count_returned);
    println!("asset_count_total: {}", surface.asset_count_total);
    println!("asset_count_returned: {}", surface.asset_count_returned);
    println!("extension_count_total: {}", surface.extension_count_total);
    println!(
        "extension_tag_count_total: {}",
        surface.extension_tag_count_total
    );
    println!(
        "top_templates: {}",
        join_or_none(
            &surface
                .templates
                .iter()
                .map(|template| template.template_title.clone())
                .take(16)
                .collect::<Vec<_>>()
        )
    );
    println!(
        "extension_tags: {}",
        join_or_none(
            &surface
                .extension_tags
                .iter()
                .map(|tag| tag.tag_name.clone())
                .take(32)
                .collect::<Vec<_>>()
        )
    );
    println!(
        "modules: {}",
        join_or_none(
            &surface
                .modules
                .iter()
                .map(|module| module.module_title.clone())
                .take(32)
                .collect::<Vec<_>>()
        )
    );
    println!(
        "assets: {}",
        join_or_none(
            &surface
                .assets
                .iter()
                .map(|asset| asset.title.clone())
                .take(32)
                .collect::<Vec<_>>()
        )
    );
    if surface.warnings.is_empty() {
        println!("warnings: <none>");
    } else {
        for warning in &surface.warnings {
            println!("warning: {warning}");
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
