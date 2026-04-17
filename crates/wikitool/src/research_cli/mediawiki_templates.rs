use anyhow::{Result, bail};
use clap::{ArgAction, Args};
use wikitool_core::research::{
    MediaWikiTemplatePage, MediaWikiTemplateQueryOptions, MediaWikiTemplateReport,
    fetch_mediawiki_template_report,
};

use crate::RuntimeOptions;
use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_with_config};

#[derive(Debug, Args)]
pub(crate) struct ResearchMediaWikiTemplatesArgs {
    url: String,
    #[arg(
        long,
        default_value_t = 16,
        value_name = "N",
        help = "Maximum selected template pages and invocation samples to return"
    )]
    limit: usize,
    #[arg(
        long,
        default_value_t = 2400,
        value_name = "BYTES",
        help = "Maximum source bytes per selected template page preview"
    )]
    content_limit: usize,
    #[arg(
        long,
        default_value_t = 64,
        value_name = "N",
        help = "Maximum TemplateData parameters returned per selected template"
    )]
    parameter_limit: usize,
    #[arg(
        long = "template",
        value_name = "TITLE",
        action = ArgAction::Append,
        help = "Fetch an exact template page from the source wiki; may be repeated"
    )]
    template: Vec<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

pub(crate) fn run(runtime: &RuntimeOptions, args: ResearchMediaWikiTemplatesArgs) -> Result<()> {
    if args.limit == 0 {
        bail!("research mediawiki-templates requires --limit >= 1");
    }
    if args.content_limit == 0 {
        bail!("research mediawiki-templates requires --content-limit >= 1");
    }
    if args.parameter_limit == 0 {
        bail!("research mediawiki-templates requires --parameter-limit >= 1");
    }
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    let report = fetch_mediawiki_template_report(
        &args.url,
        &MediaWikiTemplateQueryOptions {
            limit: args.limit,
            content_limit: args.content_limit,
            parameter_limit: args.parameter_limit,
            template_titles: args.template,
        },
    )?;

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_mediawiki_template_report_text(&paths, &report);
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn print_mediawiki_template_report_text(
    paths: &wikitool_core::runtime::ResolvedPaths,
    report: &MediaWikiTemplateReport,
) {
    println!("research mediawiki-templates");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("contract_scope: {}", report.contract_scope);
    println!("target_compatibility: {}", report.target_compatibility);
    println!(
        "target_compatibility_note: {}",
        report.target_compatibility_note
    );
    println!("source_url: {}", report.source_url);
    println!("source_domain: {}", report.source_domain);
    println!("api_endpoint: {}", report.api_endpoint);
    println!("page_title: {}", report.page_title);
    println!("canonical_url: {}", report.canonical_url);
    println!("fetched_at: {}", report.fetched_at);
    if let Some(value) = report.page_revision_id {
        println!("page_revision_id: {value}");
    }
    if let Some(value) = report.page_revision_timestamp.as_deref() {
        println!("page_revision_timestamp: {value}");
    }
    println!("api_template_count: {}", report.api_template_count);
    println!(
        "page_template_count_returned: {}",
        report.page_template_count_returned
    );
    println!("invocation_count: {}", report.invocation_count);
    println!(
        "selected_template_count: {}",
        report.selected_template_count
    );
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
    for invocation in &report.template_invocations {
        println!(
            "template_invocation: title={} keys={} tokens={} text={}",
            invocation.template_title,
            if invocation.parameter_keys.is_empty() {
                "<none>".to_string()
            } else {
                invocation.parameter_keys.join(", ")
            },
            invocation.token_estimate,
            single_line(&invocation.raw_wikitext)
        );
    }
    for page in &report.template_pages {
        print_mediawiki_template_page(page);
    }
}

fn print_mediawiki_template_page(page: &MediaWikiTemplatePage) {
    println!(
        "template_page: title={} exists={} revision_id={} revision_timestamp={} hash={} truncated={}",
        page.title,
        if page.exists { "yes" } else { "no" },
        page.revision_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        page.revision_timestamp.as_deref().unwrap_or("<none>"),
        page.content_hash.as_deref().unwrap_or("<none>"),
        if page.content_truncated { "yes" } else { "no" }
    );
    if let Some(templatedata) = page.templatedata.as_ref() {
        println!(
            "template_page.templatedata: title={} params={} description={}",
            page.title,
            templatedata.parameter_count,
            templatedata.description.as_deref().unwrap_or("<none>")
        );
        for parameter in &templatedata.parameters {
            println!(
                "template_page.templatedata.param: template={} name={} type={} required={} suggested={} deprecated={} aliases={} label={} description={}",
                page.title,
                parameter.name,
                parameter.param_type.as_deref().unwrap_or("<none>"),
                if parameter.required { "yes" } else { "no" },
                if parameter.suggested { "yes" } else { "no" },
                if parameter.deprecated { "yes" } else { "no" },
                if parameter.aliases.is_empty() {
                    "<none>".to_string()
                } else {
                    parameter.aliases.join(", ")
                },
                parameter.label.as_deref().unwrap_or("<none>"),
                parameter.description.as_deref().unwrap_or("<none>")
            );
        }
    }
    if let Some(preview) = page.content_preview.as_deref() {
        println!("template_page.preview: {}", single_line(preview));
    }
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
