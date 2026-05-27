use anyhow::{Result, bail};
use wikitool_core::authoring::article_start::build_article_start;
use wikitool_core::knowledge::authoring::{
    AuthoringKnowledgePack, AuthoringKnowledgePackOptions, build_authoring_knowledge_pack,
};
use wikitool_core::knowledge::status::knowledge_status;
use wikitool_core::profile::load_or_build_remilia_profile_overlay;

use crate::cli_support::{normalize_option, normalize_path, resolve_runtime_paths};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::shared::{
    derive_topic_from_stub_path, format_list, format_readiness, load_knowledge_stub_content,
};
use super::*;
pub(super) fn run_knowledge_article_start(
    runtime: &RuntimeOptions,
    args: KnowledgeArticleStartArgs,
) -> Result<()> {
    if args.related_limit == 0 {
        bail!("knowledge article-start requires --related-limit >= 1");
    }
    if args.chunk_limit == 0 {
        bail!("knowledge article-start requires --chunk-limit >= 1");
    }
    if args.token_budget == 0 {
        bail!("knowledge article-start requires --token-budget >= 1");
    }
    if args.max_pages == 0 {
        bail!("knowledge article-start requires --max-pages >= 1");
    }
    if args.link_limit == 0 {
        bail!("knowledge article-start requires --link-limit >= 1");
    }
    if args.category_limit == 0 {
        bail!("knowledge article-start requires --category-limit >= 1");
    }
    if args.template_limit == 0 {
        bail!("knowledge article-start requires --template-limit >= 1");
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
    let output = match pack {
        AuthoringKnowledgePack::IndexMissing => KnowledgeArticleStartOutput {
            docs_profile_requested: status.docs_profile_requested.clone(),
            readiness: status.readiness.clone(),
            degradations: status.degradations.clone(),
            knowledge_generation: status.knowledge_generation.clone(),
            result: KnowledgeArticleStartPayload::IndexMissing,
        },
        AuthoringKnowledgePack::QueryMissing => KnowledgeArticleStartOutput {
            docs_profile_requested: status.docs_profile_requested.clone(),
            readiness: status.readiness.clone(),
            degradations: status.degradations.clone(),
            knowledge_generation: status.knowledge_generation.clone(),
            result: KnowledgeArticleStartPayload::QueryMissing,
        },
        AuthoringKnowledgePack::Found(report) => {
            let overlay = load_or_build_remilia_profile_overlay(&paths)?;
            let article_start = build_article_start(&report, &overlay, args.intent.into());
            KnowledgeArticleStartOutput {
                docs_profile_requested: status.docs_profile_requested.clone(),
                readiness: status.readiness.clone(),
                degradations: status.degradations.clone(),
                knowledge_generation: status.knowledge_generation.clone(),
                result: KnowledgeArticleStartPayload::Found {
                    article_start: Box::new(article_start),
                    raw_pack: if args.include_pack {
                        Some(report)
                    } else {
                        None
                    },
                },
            }
        }
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!("knowledge article-start");
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
        KnowledgeArticleStartPayload::IndexMissing => {
            bail!(
                "knowledge article-start requires a built knowledge index; run `wikitool knowledge build`"
            );
        }
        KnowledgeArticleStartPayload::QueryMissing => {
            bail!(
                "knowledge article-start requires a topic or a stub with at least one resolvable wikilink"
            );
        }
        KnowledgeArticleStartPayload::Found { article_start, .. } => {
            println!(
                "article_start.schema_version: {}",
                article_start.schema_version
            );
            println!(
                "article_start.intent: {}",
                serde_json::to_string(&article_start.intent)?
            );
            println!("article_start.topic: {}", article_start.topic);
            println!(
                "article_start.local_state: {}",
                serde_json::to_string(&article_start.local_state)?
            );
            println!(
                "article_start.evidence.direct_subject_evidence.count: {}",
                article_start.evidence_profile.direct_subject_evidence.len()
            );
            println!(
                "article_start.evidence.broad_context.count: {}",
                article_start.evidence_profile.broad_context.len()
            );
            println!(
                "article_start.evidence.missing_query_terms: {}",
                format_list(&article_start.evidence_profile.missing_query_terms)
            );
            for warning in article_start
                .evidence_profile
                .missing_evidence_warnings
                .iter()
                .take(4)
            {
                println!("article_start.evidence.warning: {warning}");
            }
            println!(
                "article_start.comparable_pages: {}",
                format_list(&article_start.local_integration.comparable_pages)
            );
            println!(
                "article_start.required_templates: {}",
                article_start
                    .local_integration
                    .required_templates
                    .iter()
                    .map(|entry| entry.template_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_start.subject_type_hints: {}",
                article_start
                    .local_integration
                    .subject_type_hints
                    .iter()
                    .map(|entry| entry.subject_type.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_start.available_infoboxes: {}",
                article_start
                    .local_integration
                    .available_infoboxes
                    .iter()
                    .map(|entry| entry.template_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_start.citation_templates_seen: {}",
                article_start
                    .local_integration
                    .citation_templates_seen
                    .iter()
                    .map(|entry| entry.template_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_start.template_surface: {}",
                article_start
                    .local_integration
                    .template_surface
                    .iter()
                    .map(|entry| entry.template_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_start.categories_seen: {}",
                article_start
                    .local_integration
                    .categories_seen
                    .iter()
                    .map(|entry| entry.category_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_start.links_seen: {}",
                article_start
                    .local_integration
                    .links_seen
                    .iter()
                    .map(|entry| entry.page_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_start.section_skeleton: {}",
                article_start
                    .local_integration
                    .section_skeleton
                    .iter()
                    .map(|entry| entry.heading.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_start.docs_queries: {}",
                format_list(&article_start.local_integration.docs_queries)
            );
            println!(
                "article_start.contract_query: {}",
                article_start.local_integration.contract_query
            );
            println!(
                "article_start.contract_missing_query_terms: {}",
                format_list(&article_start.local_integration.contract_missing_query_terms)
            );
            for warning in article_start
                .local_integration
                .contract_warnings
                .iter()
                .take(4)
            {
                println!("article_start.contract_warning: {warning}");
            }
            println!(
                "article_start.open_questions.count: {}",
                article_start.open_questions.len()
            );
            for question in article_start.open_questions.iter().take(6) {
                println!("article_start.open_question: {}", question.question);
            }
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
