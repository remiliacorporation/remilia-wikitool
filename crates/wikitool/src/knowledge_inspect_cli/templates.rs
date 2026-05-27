use anyhow::{Result, bail};
use wikitool_core::knowledge::templates::{
    ActiveTemplateCatalogLookup, TemplateParameterUsage, TemplateReferenceLookup,
    TemplateUsageSummary, query_active_template_catalog, query_template_reference,
};

use crate::cli_support::{normalize_path, normalize_title_query, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::*;
pub(super) fn run_inspect_templates(
    runtime: &RuntimeOptions,
    template: Option<&str>,
    limit: usize,
    all: bool,
    format: OutputFormat,
) -> Result<()> {
    if limit == 0 {
        bail!("knowledge inspect templates requires --limit >= 1");
    }
    if template.is_some() && all {
        bail!(
            "cannot use `knowledge inspect templates TEMPLATE --all`; omit TEMPLATE in catalog mode"
        );
    }
    let paths = resolve_runtime_paths(runtime)?;

    if let Some(template_title) = template {
        let lookup = query_template_reference(&paths, template_title)?;
        if format.is_json() {
            println!("{}", serde_json::to_string_pretty(&lookup)?);
            return Ok(());
        }

        println!("knowledge inspect templates");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("template: {}", normalize_title_query(template_title));
        match lookup {
            TemplateReferenceLookup::IndexMissing => {
                println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
            }
            TemplateReferenceLookup::TemplateMissing { template_title } => {
                println!("template.reference: <missing>");
                println!("template.title: {template_title}");
            }
            TemplateReferenceLookup::Found(reference) => {
                let reference = *reference;
                print_template_summary("template", &reference.template);
                println!(
                    "template.implementation_pages.count: {}",
                    reference.implementation_pages.len()
                );
                for page in &reference.implementation_pages {
                    println!(
                        "template.implementation_page: role={} page={} summary={}",
                        page.role,
                        page.page_title,
                        if page.summary_text.is_empty() {
                            "<none>"
                        } else {
                            &page.summary_text
                        }
                    );
                }
                println!(
                    "template.implementation_chunks.count: {}",
                    reference.implementation_chunks.len()
                );
            }
        }
    } else {
        let lookup = query_active_template_catalog(&paths, if all { limit.max(1) } else { limit })?;
        if format.is_json() {
            println!("{}", serde_json::to_string_pretty(&lookup)?);
            return Ok(());
        }

        println!("knowledge inspect templates");
        println!("project_root: {}", normalize_path(&paths.project_root));
        println!("limit: {limit}");
        println!("all: {}", if all { "yes" } else { "no" });
        match lookup {
            ActiveTemplateCatalogLookup::IndexMissing => {
                println!("knowledge.inspect.storage: <not built> (run `wikitool knowledge build`)");
            }
            ActiveTemplateCatalogLookup::Found(catalog) => {
                println!(
                    "templates.active_template_count: {}",
                    catalog.active_template_count
                );
                println!("templates.count: {}", catalog.templates.len());
                for template in &catalog.templates {
                    print_template_summary("template", template);
                }
            }
        }
    }

    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn print_template_summary(label: &str, template: &TemplateUsageSummary) {
    println!(
        "{label}: {} (usage={} pages={} aliases={} keys={} implementations={} preview={})",
        template.template_title,
        template.usage_count,
        template.distinct_page_count,
        if template.aliases.is_empty() {
            "<none>".to_string()
        } else {
            template.aliases.join(", ")
        },
        format_parameter_stats(&template.parameter_stats),
        if template.implementation_titles.is_empty() {
            "<none>".to_string()
        } else {
            template.implementation_titles.join(", ")
        },
        template
            .implementation_preview
            .as_deref()
            .unwrap_or("<none>")
    );
    if !template.example_pages.is_empty() {
        println!(
            "{label}.example_pages: {}",
            template.example_pages.join(", ")
        );
    }
    for example in &template.example_invocations {
        println!(
            "{label}.example: template={} source={} keys={} tokens={} text={}",
            template.template_title,
            example.source_title,
            if example.parameter_keys.is_empty() {
                "<none>".to_string()
            } else {
                example.parameter_keys.join(", ")
            },
            example.token_estimate,
            example.invocation_text
        );
    }
}

fn format_parameter_stats(stats: &[TemplateParameterUsage]) -> String {
    if stats.is_empty() {
        return "<none>".to_string();
    }
    stats
        .iter()
        .map(|stat| {
            if stat.example_values.is_empty() {
                format!("{}:{}", stat.key, stat.usage_count)
            } else {
                format!(
                    "{}:{}[{}]",
                    stat.key,
                    stat.usage_count,
                    stat.example_values.join(" | ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
