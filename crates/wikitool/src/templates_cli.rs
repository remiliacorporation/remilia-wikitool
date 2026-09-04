use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde::Serialize;
use wikitool_core::site::{
    TemplateCatalog, TemplateCatalogEntry, TemplateCatalogEntryLookup,
    build_template_dependency_closure_with_capabilities, find_template_catalog_entry,
    load_site_adapter, load_template_catalog, load_wiki_capabilities_with_config,
    sync_template_catalog_with_adapter,
};

use crate::briefs::{BriefCommand, BriefView, brief_command_owned, capped_strings, text_preview};
use crate::cli_support::{
    OutputFormat, normalize_path, resolve_runtime_paths, resolve_runtime_with_config,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

mod contract;
mod migration;
use contract::{
    TemplatesContractArgs, TemplatesScaffoldArgs, run_templates_contract, run_templates_scaffold,
};

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
    #[command(about = "Export an exact named template/module dependency closure")]
    Closure(TemplatesClosureArgs),
    #[command(about = "Validate and compare declarative template contracts")]
    Contract(TemplatesContractArgs),
    #[command(about = "Preview or apply a contract-bound template scaffold")]
    Scaffold(TemplatesScaffoldArgs),
    #[command(about = "Plan exact local template migrations without changing source files")]
    MigrationPlan(migration::TemplatesMigrationPlanArgs),
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
    #[arg(
        long,
        value_enum,
        default_value_t = BriefView::Brief,
        value_name = "VIEW",
        help = "JSON view: brief|full"
    )]
    view: BriefView,
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

#[derive(Debug, Args)]
pub(crate) struct TemplatesClosureArgs {
    #[arg(required = true, num_args = 1.., value_name = "TEMPLATE")]
    templates: Vec<String>,
    #[arg(
        long,
        default_value_t = 128,
        value_name = "N",
        help = "Fail if the transitive template/module closure exceeds this node count"
    )]
    max_nodes: usize,
    #[arg(
        long,
        value_name = "PATH",
        help = "Write the full JSON closure atomically inside the project root"
    )]
    output: Option<PathBuf>,
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
        TemplatesSubcommand::Closure(args) => run_templates_closure(runtime, args),
        TemplatesSubcommand::Contract(args) => run_templates_contract(runtime, args),
        TemplatesSubcommand::Scaffold(args) => run_templates_scaffold(runtime, args),
        TemplatesSubcommand::MigrationPlan(args) => migration::run(runtime, args),
    }
}

#[derive(Debug, Serialize)]
struct TemplateClosureReceipt<'a> {
    schema: &'static str,
    output_path: &'a str,
    content_sha256: &'a str,
    roots: &'a [String],
    template_count: usize,
    module_count: usize,
    file_count: usize,
    edge_count: usize,
    runtime_dependency_count: usize,
    missing_count: usize,
    unresolved_count: usize,
}

fn run_templates_closure(runtime: &RuntimeOptions, args: TemplatesClosureArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let catalog = load_or_sync_catalog(&paths)?;
    let capabilities = load_wiki_capabilities_with_config(&paths, &config)?.ok_or_else(|| {
        anyhow::anyhow!(
            "wiki capability manifest is missing; run `wikitool wiki capabilities sync` before exporting a template closure"
        )
    })?;
    if capabilities.magic_words.is_empty() {
        bail!(
            "wiki capability manifest lacks MediaWiki magic-word data; refresh it with `wikitool wiki capabilities sync` before exporting a template closure"
        );
    }
    let closure = build_template_dependency_closure_with_capabilities(
        &paths,
        &catalog,
        &args.templates,
        Some(&capabilities),
        args.max_nodes,
    )?;
    let mut encoded = serde_json::to_string_pretty(&closure)?;
    encoded.push('\n');

    if let Some(output) = args.output {
        let output = if output.is_absolute() {
            output
        } else {
            paths.project_root.join(output)
        };
        wikitool_core::filesystem::validate_scoped_path(&paths, &output)?;
        let content_sha256 = wikitool_core::support::compute_sha256(&encoded);
        wikitool_core::support::atomic_write(&output, encoded)?;
        let output_display = normalize_path(output.canonicalize().unwrap_or(output));
        let receipt = TemplateClosureReceipt {
            schema: "template_dependency_closure_write_v2",
            output_path: &output_display,
            content_sha256: &content_sha256,
            roots: &closure.roots,
            template_count: closure.templates.len(),
            module_count: closure.modules.len(),
            file_count: closure.files.len(),
            edge_count: closure.edges.len(),
            runtime_dependency_count: closure.runtime_dependencies.len(),
            missing_count: closure.missing.len(),
            unresolved_count: closure.unresolved.len(),
        };
        if args.format.is_json() {
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        } else {
            print_template_closure_receipt(&receipt);
        }
        return Ok(());
    }

    if args.format.is_json() {
        print!("{encoded}");
    } else {
        println!("templates closure");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("roots: {}", closure.roots.join(", "));
        println!("templates.count: {}", closure.templates.len());
        println!("modules.count: {}", closure.modules.len());
        println!("files.count: {}", closure.files.len());
        println!("edges.count: {}", closure.edges.len());
        println!(
            "runtime_dependencies.count: {}",
            closure.runtime_dependencies.len()
        );
        println!("missing.count: {}", closure.missing.len());
        println!("unresolved.count: {}", closure.unresolved.len());
        for item in &closure.missing {
            println!(
                "missing: kind={} title={} referenced_by={} reason={}",
                item.kind, item.title, item.referenced_by, item.reason
            );
        }
        for item in &closure.runtime_dependencies {
            println!(
                "runtime: kind={} title={} referenced_by={} relation={} provider={}",
                item.kind, item.title, item.referenced_by, item.relation, item.provider
            );
        }
        for item in &closure.unresolved {
            println!(
                "unresolved: kind={} referenced_by={} relation={} reason={}",
                item.kind, item.referenced_by, item.relation, item.reason
            );
        }
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    }
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn print_template_closure_receipt(receipt: &TemplateClosureReceipt<'_>) {
    println!("templates closure");
    println!("output_path: {}", receipt.output_path);
    println!("content_sha256: {}", receipt.content_sha256);
    println!("roots: {}", receipt.roots.join(", "));
    println!("templates.count: {}", receipt.template_count);
    println!("modules.count: {}", receipt.module_count);
    println!("files.count: {}", receipt.file_count);
    println!("edges.count: {}", receipt.edge_count);
    println!(
        "runtime_dependencies.count: {}",
        receipt.runtime_dependency_count
    );
    println!("missing.count: {}", receipt.missing_count);
    println!("unresolved.count: {}", receipt.unresolved_count);
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
    let adapter = load_site_adapter(&paths)?;
    let catalog = sync_template_catalog_with_adapter(&paths, &adapter)?;

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
        if args.view.is_full() {
            println!("{}", serde_json::to_string_pretty(&entry)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&build_template_brief(&entry))?
            );
        }
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
    let adapter = load_site_adapter(paths)?;
    if let Some(catalog) = load_template_catalog(paths, &adapter.adapter_id)? {
        return Ok(catalog);
    }
    sync_template_catalog_with_adapter(paths, &adapter)
}

fn print_catalog_summary(catalog: &TemplateCatalog) {
    let summary = catalog.summary();
    println!("site_adapter_id: {}", summary.site_adapter_id);
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
        "adapter_templates: {}",
        if summary.adapter_template_titles.is_empty() {
            "<none>".to_string()
        } else {
            summary.adapter_template_titles.join(", ")
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
        if let Some(value) = parameter.example.as_deref() {
            println!("parameter.example: {value}");
        }
        if let Some(value) = parameter.default_value.as_deref() {
            println!("parameter.default_value: {value}");
        }
        if !parameter.suggested_values.is_empty() {
            println!(
                "parameter.suggested_values: {}",
                parameter.suggested_values.join(", ")
            );
        }
        if let Some(value) = parameter.auto_value.as_deref() {
            println!("parameter.auto_value: {value}");
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

#[derive(Debug, Serialize)]
struct TemplateBrief<'a> {
    schema_version: &'static str,
    command: &'static str,
    view: &'static str,
    status: &'static str,
    template_title: &'a str,
    category: &'a str,
    summary_text: Option<&'a str>,
    contract: TemplateContractCard<'a>,
    usage: TemplateUsageCard<'a>,
    examples: Vec<TemplateExampleCard<'a>>,
    warnings: Vec<String>,
    next_commands: Vec<BriefCommand>,
    full_view_command: BriefCommand,
}

#[derive(Debug, Serialize)]
struct TemplateContractCard<'a> {
    has_templatedata: bool,
    declared_parameter_count: usize,
    required_parameters: Vec<TemplateParameterCard<'a>>,
    suggested_parameters: Vec<TemplateParameterCard<'a>>,
    deprecated_parameters: Vec<TemplateParameterCard<'a>>,
    observed_only_parameters: Vec<TemplateParameterCard<'a>>,
    module_titles: Vec<&'a str>,
    documentation_titles: Vec<&'a str>,
    implementation_titles: Vec<&'a str>,
    recommendation_tags: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
struct TemplateParameterCard<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<&'a str>,
    sources: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    param_type: Option<&'a str>,
    usage_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    example_values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    example: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_value: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    suggested_values: Vec<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_value: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct TemplateUsageCard<'a> {
    usage_count: usize,
    distinct_page_count: usize,
    redirect_aliases: &'a [String],
    usage_aliases: &'a [String],
    example_pages: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TemplateExampleCard<'a> {
    source_kind: &'a str,
    source_title: Option<&'a str>,
    parameter_keys: Vec<&'a str>,
    invocation_preview: String,
    token_estimate: usize,
}

fn build_template_brief(entry: &TemplateCatalogEntry) -> TemplateBrief<'_> {
    let mut warnings = Vec::new();
    if entry.templatedata.is_none() {
        warnings.push("TemplateData is not available; parameter contract comes from local source and observed usage".to_string());
    }
    if entry.usage_count == 0 {
        warnings.push("no indexed usages were found for this template".to_string());
    }
    if entry.parameters.is_empty() {
        warnings.push("no parameters are declared or observed for this template".to_string());
    }

    TemplateBrief {
        schema_version: "wikitool_brief_v1",
        command: "templates show",
        view: "brief",
        status: "found",
        template_title: &entry.template_title,
        category: &entry.category,
        summary_text: entry.summary_text.as_deref(),
        contract: TemplateContractCard {
            has_templatedata: entry.templatedata.is_some(),
            declared_parameter_count: entry.declared_parameter_keys.len(),
            required_parameters: entry
                .parameters
                .iter()
                .filter(|parameter| parameter.required)
                .take(8)
                .map(parameter_card)
                .collect(),
            suggested_parameters: entry
                .parameters
                .iter()
                .filter(|parameter| parameter.suggested && !parameter.required)
                .take(8)
                .map(parameter_card)
                .collect(),
            deprecated_parameters: entry
                .parameters
                .iter()
                .filter(|parameter| parameter.deprecated)
                .take(8)
                .map(parameter_card)
                .collect(),
            observed_only_parameters: entry
                .parameters
                .iter()
                .filter(|parameter| {
                    parameter.sources.iter().any(|source| source == "usage")
                        && !parameter
                            .sources
                            .iter()
                            .any(|source| source == "templatedata" || source == "source")
                })
                .take(8)
                .map(parameter_card)
                .collect(),
            module_titles: entry
                .module_titles
                .iter()
                .map(String::as_str)
                .take(6)
                .collect(),
            documentation_titles: entry
                .documentation_titles
                .iter()
                .map(String::as_str)
                .take(4)
                .collect(),
            implementation_titles: entry
                .implementation_titles
                .iter()
                .map(String::as_str)
                .take(4)
                .collect(),
            recommendation_tags: entry
                .recommendation_tags
                .iter()
                .map(String::as_str)
                .take(6)
                .collect(),
        },
        usage: TemplateUsageCard {
            usage_count: entry.usage_count,
            distinct_page_count: entry.distinct_page_count,
            redirect_aliases: &entry.redirect_aliases,
            usage_aliases: &entry.usage_aliases,
            example_pages: capped_strings(&entry.example_pages, 5),
        },
        examples: entry
            .examples
            .iter()
            .take(2)
            .map(|example| TemplateExampleCard {
                source_kind: &example.source_kind,
                source_title: example.source_title.as_deref(),
                parameter_keys: example
                    .parameter_keys
                    .iter()
                    .map(String::as_str)
                    .take(10)
                    .collect(),
                invocation_preview: text_preview(&example.invocation_text, 260),
                token_estimate: example.token_estimate,
            })
            .collect(),
        warnings,
        next_commands: vec![
            brief_command_owned(vec![
                "wikitool".to_string(),
                "templates".to_string(),
                "examples".to_string(),
                entry.template_title.clone(),
                "--limit".to_string(),
                "4".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ]),
            brief_command_owned(vec![
                "wikitool".to_string(),
                "catalog".to_string(),
                "inspect".to_string(),
                "templates".to_string(),
                entry.template_title.clone(),
                "--format".to_string(),
                "json".to_string(),
            ]),
        ],
        full_view_command: brief_command_owned(vec![
            "wikitool".to_string(),
            "templates".to_string(),
            "show".to_string(),
            entry.template_title.clone(),
            "--format".to_string(),
            "json".to_string(),
            "--view".to_string(),
            "full".to_string(),
        ]),
    }
}

fn parameter_card(
    parameter: &wikitool_core::site::TemplateCatalogParameter,
) -> TemplateParameterCard<'_> {
    TemplateParameterCard {
        name: &parameter.name,
        aliases: parameter
            .aliases
            .iter()
            .map(String::as_str)
            .take(4)
            .collect(),
        sources: parameter
            .sources
            .iter()
            .map(String::as_str)
            .take(4)
            .collect(),
        param_type: parameter.param_type.as_deref(),
        usage_count: parameter.usage_count,
        example_values: capped_strings(&parameter.example_values, 3),
        example: parameter.example.as_deref(),
        default_value: parameter.default_value.as_deref(),
        suggested_values: parameter
            .suggested_values
            .iter()
            .map(String::as_str)
            .collect(),
        auto_value: parameter.auto_value.as_deref(),
    }
}
