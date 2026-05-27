use anyhow::{Result, bail};
use wikitool_core::knowledge::authoring::{
    AuthoringContractPlanOptions, extract_authoring_stub_hints, query_authoring_contract_plan,
};

use crate::RuntimeOptions;
use crate::cli_support::{collapse_whitespace, normalize_option, resolve_runtime_paths};

use super::shared::{
    derive_topic_from_stub_path, load_knowledge_stub_content, print_contract_plan,
    query_terms_for_contract_query,
};
use super::*;
pub(super) fn run_knowledge_contracts(
    runtime: &RuntimeOptions,
    args: KnowledgeContractsArgs,
) -> Result<()> {
    match args.command {
        KnowledgeContractsSubcommand::Search(args) => run_knowledge_contracts_search(runtime, args),
        KnowledgeContractsSubcommand::Plan(args) => run_knowledge_contracts_plan(runtime, args),
    }
}

fn run_knowledge_contracts_search(
    runtime: &RuntimeOptions,
    args: KnowledgeContractsSearchArgs,
) -> Result<()> {
    if args.limit == 0 {
        bail!("knowledge contracts search requires --limit >= 1");
    }
    if args.token_budget == 0 {
        bail!("knowledge contracts search requires --token-budget >= 1");
    }
    let query = collapse_whitespace(&args.query);
    if query.is_empty() {
        bail!("knowledge contracts search requires a non-empty QUERY");
    }
    let paths = resolve_runtime_paths(runtime)?;
    let plan = query_authoring_contract_plan(
        &paths,
        AuthoringContractPlanOptions {
            query: query.clone(),
            query_terms: query_terms_for_contract_query(&query),
            limit: args.limit,
            token_budget: args.token_budget,
            profile: args.profile.into(),
            ..AuthoringContractPlanOptions::default()
        },
    )?;

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    print_contract_plan("knowledge contracts search", &paths, &plan);
    Ok(())
}

fn run_knowledge_contracts_plan(
    runtime: &RuntimeOptions,
    args: KnowledgeContractsPlanArgs,
) -> Result<()> {
    if args.limit == 0 {
        bail!("knowledge contracts plan requires --limit >= 1");
    }
    if args.token_budget == 0 {
        bail!("knowledge contracts plan requires --token-budget >= 1");
    }
    let paths = resolve_runtime_paths(runtime)?;
    let topic = normalize_option(args.topic.as_deref())
        .or_else(|| derive_topic_from_stub_path(args.stub_path.as_deref()));
    let stub_content = load_knowledge_stub_content(&paths, args.stub_path.as_deref())?;
    let (stub_links, stub_templates) = extract_authoring_stub_hints(stub_content.as_deref());
    let query = normalize_option(args.contract_query.as_deref())
        .or(topic)
        .or_else(|| stub_links.first().cloned())
        .unwrap_or_default();
    let query = collapse_whitespace(&query);
    if query.is_empty() && stub_templates.is_empty() {
        bail!("knowledge contracts plan requires TOPIC or --stub-path with template/link hints");
    }
    let plan = query_authoring_contract_plan(
        &paths,
        AuthoringContractPlanOptions {
            query: query.clone(),
            query_terms: query_terms_for_contract_query(&query),
            stub_detected_templates: stub_templates,
            limit: args.limit,
            token_budget: args.token_budget,
            profile: args.profile.into(),
            ..AuthoringContractPlanOptions::default()
        },
    )?;

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    print_contract_plan("knowledge contracts plan", &paths, &plan);
    Ok(())
}
