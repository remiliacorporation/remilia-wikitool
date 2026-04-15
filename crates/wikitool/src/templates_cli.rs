use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use wikitool_core::profile::{
    TemplateCatalog, TemplateCatalogEntry, TemplateCatalogEntryLookup, find_template_catalog_entry,
    load_or_build_remilia_profile_overlay, load_template_catalog,
    sync_template_catalog_with_overlay,
};

use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct TemplatesArgs {
    #[command(subcommand)]
    command: TemplatesSubcommand,
}

#[derive(Debug, Subcommand)]
enum TemplatesSubcommand {
    #[command(about = "Build and store the local template catalog artifact")]
    Catalog(TemplatesCatalogArgs),
    #[command(about = "Show one template catalog entry")]
    Show(TemplatesShowArgs),
    #[command(about = "Show example invocations for one template")]
    Examples(TemplatesExamplesArgs),
}

#[derive(Debug, Args)]
pub(crate) struct TemplatesCatalogArgs {
    #[command(subcommand)]
    command: TemplatesCatalogSubcommand,
}

#[derive(Debug, Subcommand)]
enum TemplatesCatalogSubcommand {
    #[command(about = "Build the catalog from tracked templates plus local index usage")]
    Build(TemplatesFormatArgs),
}

#[derive(Debug, Args)]
struct TemplatesFormatArgs {
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct TemplatesShowArgs {
    template: String,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct TemplatesExamplesArgs {
    template: String,
    #[arg(long, default_value_t = 8, value_name = "N")]
    limit: usize,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

pub(crate) fn run_templates(runtime: &RuntimeOptions, args: TemplatesArgs) -> Result<()> {
    match args.command {
        TemplatesSubcommand::Catalog(args) => run_templates_catalog(runtime, args),
        TemplatesSubcommand::Show(args) => run_templates_show(runtime, args),
        TemplatesSubcommand::Examples(args) => run_templates_examples(runtime, args),
    }
}

fn run_templates_catalog(runtime: &RuntimeOptions, args: TemplatesCatalogArgs) -> Result<()> {
    match args.command {
        TemplatesCatalogSubcommand::Build(args) => {
            run_templates_catalog_build(runtime, args.format)
        }
    }
}

fn run_templates_catalog_build(runtime: &RuntimeOptions, format: OutputFormat) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let overlay = load_or_build_remilia_profile_overlay(&paths)?;
    let catalog = sync_template_catalog_with_overlay(&paths, &overlay)?;

    if format.is_json() {
        println!("{}", serde_json::to_string_pretty(&catalog)?);
        return Ok(());
    }

    println!("templates catalog build");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_catalog_summary(&catalog);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_templates_show(runtime: &RuntimeOptions, args: TemplatesShowArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let catalog = load_or_sync_catalog(&paths)?;
    let entry = match find_template_catalog_entry(&catalog, &args.template) {
        TemplateCatalogEntryLookup::Found(entry) => *entry,
        TemplateCatalogEntryLookup::CatalogMissing => {
            bail!("template catalog is missing; run `wikitool templates catalog build`")
        }
        TemplateCatalogEntryLookup::TemplateMissing { template_title } => {
            bail!("template catalog entry not found: {template_title}")
        }
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&entry)?);
        return Ok(());
    }

    println!("templates show");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_template_entry(&entry);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn run_templates_examples(runtime: &RuntimeOptions, args: TemplatesExamplesArgs) -> Result<()> {
    if args.limit == 0 {
        bail!("templates examples requires --limit >= 1");
    }

    let paths = resolve_runtime_paths(runtime)?;
    let catalog = load_or_sync_catalog(&paths)?;
    let entry = match find_template_catalog_entry(&catalog, &args.template) {
        TemplateCatalogEntryLookup::Found(entry) => *entry,
        TemplateCatalogEntryLookup::CatalogMissing => {
            bail!("template catalog is missing; run `wikitool templates catalog build`")
        }
        TemplateCatalogEntryLookup::TemplateMissing { template_title } => {
            bail!("template catalog entry not found: {template_title}")
        }
    };

    let examples = entry
        .examples
        .into_iter()
        .take(args.limit)
        .collect::<Vec<_>>();

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&examples)?);
        return Ok(());
    }

    println!("templates examples");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("template_title: {}", entry.template_title);
    println!("example_count: {}", examples.len());
    if examples.is_empty() {
        println!("examples: <none>");
    } else {
        for example in &examples {
            println!(
                "example: source_kind={} source={} params={} text={}",
                example.source_kind,
                example.source_title.as_deref().unwrap_or("<none>"),
                if example.parameter_keys.is_empty() {
                    "<none>".to_string()
                } else {
                    example.parameter_keys.join(", ")
                },
                example.invocation_text
            );
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn load_or_sync_catalog(paths: &wikitool_core::runtime::ResolvedPaths) -> Result<TemplateCatalog> {
    let overlay = load_or_build_remilia_profile_overlay(paths)?;
    if let Some(catalog) = load_template_catalog(paths, &overlay.profile_id)? {
        return Ok(catalog);
    }
    sync_template_catalog_with_overlay(paths, &overlay)
}

fn print_catalog_summary(catalog: &TemplateCatalog) {
    let summary = catalog.summary();
    println!("profile_id: {}", summary.profile_id);
    println!("template_count: {}", summary.template_count);
    println!("templatedata_count: {}", summary.templatedata_count);
    println!("redirect_alias_count: {}", summary.redirect_alias_count);
    println!(
        "usage_index_ready: {}",
        if summary.usage_index_ready {
            "true"
        } else {
            "false"
        }
    );
    println!(
        "profile_templates: {}",
        if summary.profile_template_titles.is_empty() {
            "<none>".to_string()
        } else {
            summary.profile_template_titles.join(", ")
        }
    );
    println!("refreshed_at: {}", summary.refreshed_at);
}

fn print_template_entry(entry: &TemplateCatalogEntry) {
    println!("template_title: {}", entry.template_title);
    println!("relative_path: {}", entry.relative_path);
    println!("category: {}", entry.category);
    println!(
        "summary_text: {}",
        entry.summary_text.as_deref().unwrap_or("<none>")
    );
    println!(
        "templatedata: {}",
        if entry.templatedata.is_some() {
            "yes"
        } else {
            "no"
        }
    );
    println!("usage_count: {}", entry.usage_count);
    println!("distinct_page_count: {}", entry.distinct_page_count);
    println!(
        "redirect_aliases: {}",
        join_or_none(&entry.redirect_aliases)
    );
    println!("usage_aliases: {}", join_or_none(&entry.usage_aliases));
    println!(
        "documentation_titles: {}",
        join_or_none(&entry.documentation_titles)
    );
    println!(
        "implementation_titles: {}",
        join_or_none(&entry.implementation_titles)
    );
    println!("module_titles: {}", join_or_none(&entry.module_titles));
    println!(
        "recommendation_tags: {}",
        join_or_none(&entry.recommendation_tags)
    );
    println!(
        "declared_parameter_keys: {}",
        join_or_none(&entry.declared_parameter_keys)
    );
    println!("parameter_count: {}", entry.parameters.len());
    for parameter in &entry.parameters {
        println!(
            "parameter: {} (sources={} required={} suggested={} deprecated={} usage_count={})",
            parameter.name,
            if parameter.sources.is_empty() {
                "<none>".to_string()
            } else {
                parameter.sources.join(", ")
            },
            if parameter.required { "yes" } else { "no" },
            if parameter.suggested { "yes" } else { "no" },
            if parameter.deprecated { "yes" } else { "no" },
            parameter.usage_count
        );
        if let Some(value) = parameter.label.as_deref() {
            println!("parameter.label: {value}");
        }
        if let Some(value) = parameter.param_type.as_deref() {
            println!("parameter.type: {value}");
        }
        if let Some(value) = parameter.description.as_deref() {
            println!("parameter.description: {value}");
        }
        if !parameter.aliases.is_empty() {
            println!("parameter.aliases: {}", parameter.aliases.join(", "));
        }
        if !parameter.observed_names.is_empty() {
            println!(
                "parameter.observed_names: {}",
                parameter.observed_names.join(", ")
            );
        }
        if !parameter.example_values.is_empty() {
            println!(
                "parameter.example_values: {}",
                parameter.example_values.join(", ")
            );
        }
    }
    println!("example_count: {}", entry.examples.len());
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "<none>".to_string()
    } else {
        values.join(", ")
    }
}
