use anyhow::Result;
use serde::Serialize;
use wikitool_core::profile::{
    AuthoringSurface, AuthoringSurfaceOptions, AuthoringTemplateSurface,
    build_authoring_surface_with_config, sync_authoring_surface_with_config,
};

use crate::briefs::{BriefCommand, brief_command_owned};
use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::join_or_none;
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
    if args.parser_function_limit == 0 {
        anyhow::bail!("wiki surface requires --parser-function-limit >= 1");
    }
    Ok(AuthoringSurfaceOptions {
        template_limit: args.template_limit,
        template_example_limit: args.template_example_limit,
        module_limit: args.module_limit,
        asset_limit: args.asset_limit,
        extension_limit: args.extension_limit,
        extension_tag_limit: args.extension_tag_limit,
        parser_function_limit: args.parser_function_limit,
    })
}

fn print_authoring_surface(
    heading: &str,
    runtime: &RuntimeOptions,
    paths: &wikitool_core::runtime::ResolvedPaths,
    surface: &AuthoringSurface,
    format: OutputFormat,
    view: BriefView,
) -> Result<()> {
    if format.is_json() {
        if view.is_full() {
            println!("{}", serde_json::to_string_pretty(surface)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&build_surface_brief(heading, surface))?
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
        "parser_function_count_total: {}",
        surface.parser_function_count_total
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
        "parser_functions: {}",
        join_or_none(
            &surface
                .parser_functions
                .iter()
                .map(|function| function.function_name.clone())
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

#[derive(Debug, Serialize)]
struct SurfaceBrief<'a> {
    schema_version: &'static str,
    command: &'a str,
    view: &'static str,
    status: &'static str,
    profile_id: &'a str,
    generated_at: &'a str,
    wiki_id: Option<&'a str>,
    wiki_url: Option<&'a str>,
    freshness: SurfaceFreshness<'a>,
    counts: SurfaceCounts,
    top_templates: Vec<SurfaceTemplateBrief<'a>>,
    modules: Vec<&'a str>,
    assets: Vec<&'a str>,
    extension_tags: Vec<SurfaceExtensionTagBrief<'a>>,
    parser_functions: Vec<&'a str>,
    warnings: &'a [String],
    next_commands: Vec<BriefCommand>,
    full_view_command: BriefCommand,
}

#[derive(Debug, Serialize)]
struct SurfaceExtensionTagBrief<'a> {
    tag_name: &'a str,
    paired_syntax: &'a str,
    docs_query: &'a str,
}

#[derive(Debug, Serialize)]
struct SurfaceFreshness<'a> {
    capabilities_refreshed_at: Option<&'a str>,
    template_catalog_refreshed_at: Option<&'a str>,
    template_source: &'a str,
}

#[derive(Debug, Serialize)]
struct SurfaceCounts {
    template_count_total: usize,
    template_count_returned: usize,
    module_count_total: usize,
    module_count_returned: usize,
    asset_count_total: usize,
    asset_count_returned: usize,
    extension_count_total: usize,
    extension_count_returned: usize,
    extension_tag_count_total: usize,
    extension_tag_count_returned: usize,
    parser_function_count_total: usize,
    parser_function_count_returned: usize,
}

#[derive(Debug, Serialize)]
struct SurfaceTemplateBrief<'a> {
    template_title: &'a str,
    category: &'a str,
    has_templatedata: bool,
    usage_count: usize,
    required_parameters: Vec<&'a str>,
    suggested_parameters: Vec<&'a str>,
    recommendation_tags: &'a [String],
}

const BRIEF_TEMPLATE_LIMIT: usize = 10;
const BRIEF_MODULE_LIMIT: usize = 12;
const BRIEF_ASSET_LIMIT: usize = 12;
const BRIEF_EXTENSION_TAG_LIMIT: usize = 20;
const BRIEF_PARSER_FUNCTION_LIMIT: usize = 20;

fn build_surface_brief<'a>(command: &'a str, surface: &'a AuthoringSurface) -> SurfaceBrief<'a> {
    SurfaceBrief {
        schema_version: "wikitool_brief_v1",
        command,
        view: "brief",
        status: "found",
        profile_id: &surface.profile_id,
        generated_at: &surface.generated_at,
        wiki_id: surface.wiki_id.as_deref(),
        wiki_url: surface.wiki_url.as_deref(),
        freshness: SurfaceFreshness {
            capabilities_refreshed_at: surface.capabilities_refreshed_at.as_deref(),
            template_catalog_refreshed_at: surface.template_catalog_refreshed_at.as_deref(),
            template_source: &surface.template_source,
        },
        counts: SurfaceCounts {
            template_count_total: surface.template_count_total,
            template_count_returned: surface.template_count_returned,
            module_count_total: surface.module_count_total,
            module_count_returned: surface.module_count_returned,
            asset_count_total: surface.asset_count_total,
            asset_count_returned: surface.asset_count_returned,
            extension_count_total: surface.extension_count_total,
            extension_count_returned: surface.extension_count_returned,
            extension_tag_count_total: surface.extension_tag_count_total,
            extension_tag_count_returned: surface.extension_tag_count_returned,
            parser_function_count_total: surface.parser_function_count_total,
            parser_function_count_returned: surface.parser_function_count_returned,
        },
        top_templates: surface
            .templates
            .iter()
            .take(BRIEF_TEMPLATE_LIMIT)
            .map(surface_template_brief)
            .collect(),
        modules: surface
            .modules
            .iter()
            .take(BRIEF_MODULE_LIMIT)
            .map(|module| module.module_title.as_str())
            .collect(),
        assets: surface
            .assets
            .iter()
            .map(|asset| asset.title.as_str())
            .take(BRIEF_ASSET_LIMIT)
            .collect(),
        extension_tags: surface
            .extension_tags
            .iter()
            .map(|tag| SurfaceExtensionTagBrief {
                tag_name: &tag.tag_name,
                paired_syntax: &tag.paired_syntax,
                docs_query: &tag.docs_query,
            })
            .take(BRIEF_EXTENSION_TAG_LIMIT)
            .collect(),
        parser_functions: surface
            .parser_functions
            .iter()
            .map(|function| function.function_name.as_str())
            .take(BRIEF_PARSER_FUNCTION_LIMIT)
            .collect(),
        warnings: &surface.warnings,
        next_commands: surface
            .templates
            .iter()
            .take(3)
            .map(|template| {
                brief_command_owned(vec![
                    "wikitool".to_string(),
                    "templates".to_string(),
                    "show".to_string(),
                    template.template_title.clone(),
                    "--format".to_string(),
                    "json".to_string(),
                    "--view".to_string(),
                    "brief".to_string(),
                ])
            })
            .collect(),
        full_view_command: surface_full_view_command(command),
    }
}

fn surface_full_view_command(command: &str) -> BriefCommand {
    let subcommand = if command.ends_with(" sync") {
        "sync"
    } else {
        "show"
    };
    brief_command_owned(vec![
        "wikitool".to_string(),
        "wiki".to_string(),
        "surface".to_string(),
        subcommand.to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--view".to_string(),
        "full".to_string(),
    ])
}

fn surface_template_brief(template: &AuthoringTemplateSurface) -> SurfaceTemplateBrief<'_> {
    SurfaceTemplateBrief {
        template_title: &template.template_title,
        category: &template.category,
        has_templatedata: template.has_templatedata,
        usage_count: template.usage_count,
        required_parameters: template
            .parameters
            .iter()
            .filter(|parameter| parameter.required)
            .map(|parameter| parameter.name.as_str())
            .take(6)
            .collect(),
        suggested_parameters: template
            .parameters
            .iter()
            .filter(|parameter| parameter.suggested && !parameter.required)
            .map(|parameter| parameter.name.as_str())
            .take(6)
            .collect(),
        recommendation_tags: &template.recommendation_tags,
    }
}
