use anyhow::{Result, bail};
use wikitool_core::knowledge::authoring::{
    AuthoringKnowledgePack, AuthoringKnowledgePackOptions, build_authoring_knowledge_pack,
};
use wikitool_core::knowledge::status::knowledge_status;

use crate::cli_support::{format_flag, normalize_option, normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::shared::{
    derive_topic_from_stub_path, format_list, format_readiness, load_knowledge_stub_content,
};
use super::*;
pub(super) fn run_knowledge_pack(runtime: &RuntimeOptions, args: KnowledgePackArgs) -> Result<()> {
    if args.related_limit == 0 {
        bail!("knowledge pack requires --related-limit >= 1");
    }
    if args.chunk_limit == 0 {
        bail!("knowledge pack requires --chunk-limit >= 1");
    }
    if args.token_budget == 0 {
        bail!("knowledge pack requires --token-budget >= 1");
    }
    if args.max_pages == 0 {
        bail!("knowledge pack requires --max-pages >= 1");
    }
    if args.link_limit == 0 {
        bail!("knowledge pack requires --link-limit >= 1");
    }
    if args.category_limit == 0 {
        bail!("knowledge pack requires --category-limit >= 1");
    }
    if args.template_limit == 0 {
        bail!("knowledge pack requires --template-limit >= 1");
    }
    if args.diversify && args.no_diversify {
        bail!("cannot use --diversify and --no-diversify together");
    }

    let use_diversify = !args.no_diversify;
    let paths = resolve_runtime_paths(runtime)?;
    let topic = normalize_option(args.topic.as_deref())
        .or_else(|| derive_topic_from_stub_path(args.stub_path.as_deref()));
    let stub_content = load_knowledge_stub_content(&paths, args.stub_path.as_deref())?;
    let pack = build_authoring_knowledge_pack(
        &paths,
        topic.as_deref(),
        stub_content.as_deref(),
        &AuthoringKnowledgePackOptions {
            related_page_limit: args.related_limit,
            chunk_limit: args.chunk_limit,
            token_budget: args.token_budget,
            max_pages: args.max_pages,
            link_limit: args.link_limit,
            category_limit: args.category_limit,
            template_limit: args.template_limit,
            docs_profile: args.docs_profile.clone(),
            diversify: use_diversify,
            payload_mode: args.payload.into(),
            contract_profile: args.contract_profile.into(),
            contract_query: normalize_option(args.contract_query.as_deref()),
        },
    )?;
    let status = knowledge_status(&paths, &args.docs_profile)?;
    let output = KnowledgePackOutput {
        docs_profile_requested: status.docs_profile_requested.clone(),
        readiness: status.readiness.clone(),
        degradations: status.degradations.clone(),
        knowledge_generation: status.knowledge_generation.clone(),
        result: match pack {
            AuthoringKnowledgePack::IndexMissing => KnowledgePackPayload::IndexMissing,
            AuthoringKnowledgePack::QueryMissing => KnowledgePackPayload::QueryMissing,
            AuthoringKnowledgePack::Found(report) => KnowledgePackPayload::Found(report),
        },
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("knowledge pack");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "topic: {}",
        topic.as_deref().unwrap_or("<derived-from-stub>")
    );
    println!("docs_profile_requested: {}", output.docs_profile_requested);
    println!("knowledge_generation: {}", output.knowledge_generation);
    println!("readiness: {}", format_readiness(&output.readiness));
    println!("degradations: {}", format_list(&output.degradations));
    match output.result {
        KnowledgePackPayload::IndexMissing => {
            bail!(
                "knowledge pack requires a built knowledge index; run `wikitool knowledge build`"
            );
        }
        KnowledgePackPayload::QueryMissing => {
            bail!(
                "knowledge pack requires a topic or a stub with at least one resolvable wikilink"
            );
        }
        KnowledgePackPayload::Found(report) => {
            println!("pack.query: {}", report.query);
            println!("pack.query_terms: {}", format_list(&report.query_terms));
            println!("pack.payload_mode: {}", report.payload_mode.as_str());
            println!(
                "pack.topic_state.title_exists_locally: {}",
                format_flag(report.topic_assessment.title_exists_locally)
            );
            println!(
                "pack.topic_state.should_create_new_article: {}",
                format_flag(report.topic_assessment.should_create_new_article)
            );
            if let Some(exact_page) = &report.topic_assessment.exact_page {
                println!(
                    "pack.topic_state.exact_page: {} (namespace={} redirect={})",
                    exact_page.title,
                    exact_page.namespace,
                    format_flag(exact_page.is_redirect)
                );
            } else {
                println!("pack.topic_state.exact_page: <none>");
            }
            println!(
                "pack.topic_state.local_title_hits.count: {}",
                report.topic_assessment.local_title_hit_count
            );
            for hit in report.topic_assessment.local_title_hits.iter().take(8) {
                println!(
                    "pack.topic_state.local_title_hit: {} (namespace={} redirect={})",
                    hit.title,
                    hit.namespace,
                    format_flag(hit.is_redirect)
                );
            }
            println!(
                "pack.topic_state.backlinks.count: {}",
                report.topic_assessment.backlink_count
            );
            for backlink in report.topic_assessment.backlinks.iter().take(8) {
                println!("pack.topic_state.backlink: {backlink}");
            }
            println!("pack.related_pages.count: {}", report.related_pages.len());
            for page in report.related_pages.iter().take(8) {
                println!(
                    "pack.related_page: {} (namespace={} source={} retrieval_weight={})",
                    page.title, page.namespace, page.source, page.retrieval_weight
                );
            }
            println!(
                "pack.suggested_links.count: {}",
                report.suggested_links.len()
            );
            println!(
                "pack.suggested_categories.count: {}",
                report.suggested_categories.len()
            );
            println!(
                "pack.suggested_templates.count: {}",
                report.suggested_templates.len()
            );
            println!(
                "pack.suggested_references.count: {}",
                report.suggested_references.len()
            );
            println!(
                "pack.suggested_media.count: {}",
                report.suggested_media.len()
            );
            println!(
                "pack.template_references.count: {}",
                report.template_references.len()
            );
            println!(
                "pack.module_patterns.count: {}",
                report.module_patterns.len()
            );
            println!(
                "pack.docs_context.count: {}",
                report
                    .docs_context
                    .as_ref()
                    .map(|context| {
                        context.pages.len()
                            + context.sections.len()
                            + context.symbols.len()
                            + context.examples.len()
                    })
                    .unwrap_or(0)
            );
            println!(
                "pack.context.subject.related_pages: {}",
                report.context_summary.subject_context.related_page_count
            );
            println!(
                "pack.context.subject.chunks: {}",
                report.context_summary.subject_context.retrieved_chunk_count
            );
            println!(
                "pack.context.contracts.templates: {}",
                report
                    .context_summary
                    .wiki_contract_context
                    .template_contracts
                    .len()
            );
            println!(
                "pack.context.contracts.modules: {}",
                report
                    .context_summary
                    .wiki_contract_context
                    .module_contracts
                    .len()
            );
            println!(
                "pack.context.contracts.edges: {}",
                report
                    .context_summary
                    .wiki_contract_context
                    .contract_edges
                    .len()
            );
            println!(
                "pack.context.contracts.traversal.profile: {}",
                report
                    .context_summary
                    .wiki_contract_context
                    .traversal_plan
                    .profile
                    .as_str()
            );
            println!(
                "pack.context.contracts.traversal.selected: {}",
                report
                    .context_summary
                    .wiki_contract_context
                    .traversal_plan
                    .selected_contracts
                    .len()
            );
            println!(
                "pack.context.contracts.traversal.missing_query_terms: {}",
                format_list(
                    &report
                        .context_summary
                        .wiki_contract_context
                        .traversal_plan
                        .missing_query_terms
                )
            );
            println!(
                "pack.context.contracts.traversal.omitted: {}",
                report
                    .context_summary
                    .wiki_contract_context
                    .traversal_plan
                    .omitted_contracts
                    .len()
            );
            println!(
                "pack.context.contracts.omitted_detail: {}",
                format_list(&report.context_summary.wiki_contract_context.omitted_detail)
            );
            println!("pack.retrieval_mode: {}", report.retrieval_mode);
            println!("pack.chunks.count: {}", report.chunks.len());
            println!(
                "pack.token_estimate_total: {}",
                report.pack_token_estimate_total
            );
            for chunk in report.chunks.iter().take(8) {
                println!(
                    "pack.chunk: source={} section={} tokens={} text={}",
                    chunk.source_title,
                    chunk.section_heading.as_deref().unwrap_or("<lead>"),
                    chunk.token_estimate,
                    chunk.chunk_text
                );
            }
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
