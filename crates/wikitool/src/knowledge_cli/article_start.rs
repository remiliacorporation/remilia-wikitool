use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::authoring::article_start::build_article_start;
use wikitool_core::authoring::model::{
    ArticleStartIntent, ArticleStartResult, ContextSurfaceSource, EvidenceCoverageItem,
    LocalExistenceState, OpenQuestion, RequiredTemplate, SectionSkeleton, TemplateSurfaceEntry,
};
use wikitool_core::filesystem::validate_scoped_path;
use wikitool_core::knowledge::authoring::{
    AuthoringKnowledgePack, AuthoringKnowledgePackOptions, AuthoringPayloadMode,
    build_authoring_knowledge_pack,
};
use wikitool_core::knowledge::status::{KnowledgeReadinessLevel, knowledge_status};
use wikitool_core::knowledge_interview::{
    InterviewBriefSummary, InterviewValidationReport, InterviewValidationStatus,
    parse_brief_draft_plan, validate_interview_brief,
};
use wikitool_core::profile::load_or_build_remilia_profile_overlay;

use crate::briefs::{
    BriefCommand, brief_command, brief_command_owned, capped_strings, text_preview,
};
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
    let (mut interview_brief, brief_abs) = match args.brief_path.as_deref() {
        Some(path) => {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                paths.project_root.join(path)
            };
            validate_scoped_path(&paths, &absolute)?;
            let report = validate_interview_brief(&absolute, args.brief_stale_days)?;
            (Some(report), Some(absolute))
        }
        None => (None, None),
    };
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
            payload_mode: AuthoringPayloadMode::Compact,
            contract_profile: args.contract_profile.into(),
            contract_query: normalize_option(args.contract_query.as_deref()),
        },
    )?;
    let status = knowledge_status(&paths, &args.docs_profile)?;
    let mut output = match pack {
        AuthoringKnowledgePack::IndexMissing => KnowledgeArticleStartOutput {
            docs_profile_requested: status.docs_profile_requested.clone(),
            readiness: status.readiness.clone(),
            degradations: status.degradations.clone(),
            knowledge_generation: status.knowledge_generation.clone(),
            interview_brief: interview_brief.clone(),
            result: KnowledgeArticleStartPayload::IndexMissing,
        },
        AuthoringKnowledgePack::QueryMissing => KnowledgeArticleStartOutput {
            docs_profile_requested: status.docs_profile_requested.clone(),
            readiness: status.readiness.clone(),
            degradations: status.degradations.clone(),
            knowledge_generation: status.knowledge_generation.clone(),
            interview_brief: interview_brief.clone(),
            result: KnowledgeArticleStartPayload::QueryMissing,
        },
        AuthoringKnowledgePack::Found(report) => {
            let overlay = load_or_build_remilia_profile_overlay(&paths)?;
            let mut article_start = build_article_start(&report, &overlay, args.intent.into());
            if let (Some(brief_report), Some(brief_path)) =
                (interview_brief.as_mut(), brief_abs.as_deref())
            {
                fold_interview_brief_into_article_start(
                    &mut article_start,
                    brief_report,
                    brief_path,
                );
            }
            KnowledgeArticleStartOutput {
                docs_profile_requested: status.docs_profile_requested.clone(),
                readiness: status.readiness.clone(),
                degradations: status.degradations.clone(),
                knowledge_generation: status.knowledge_generation.clone(),
                interview_brief,
                result: KnowledgeArticleStartPayload::Found {
                    article_start: Box::new(article_start),
                },
            }
        }
    };
    output.readiness = article_start_output_readiness(&output);

    if args.format.is_json() {
        if args.view.is_full() {
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&build_article_start_brief(&output))?
            );
        }
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

const BRIEF_SECTION_RATIONALE: &str =
    "Planned in the interview brief Draft Plan; not observed on comparable pages.";
const BRIEF_OPEN_QUESTION_REASON: &str =
    "Recorded as an open question before drafting in the interview brief.";

/// Fold the interview brief's Draft Plan into the article-start result: add planned
/// sections the comparables missed, surface pre-draft open questions as non-blocking
/// open questions, and flag a skipped interviewer/critic loop.
fn fold_interview_brief_into_article_start(
    article_start: &mut ArticleStartResult,
    brief_report: &mut InterviewValidationReport,
    brief_path: &std::path::Path,
) {
    let Ok(body) = std::fs::read_to_string(brief_path) else {
        return;
    };
    let plan = parse_brief_draft_plan(&body);

    merge_brief_planned_sections(
        &mut article_start.local_integration.section_skeleton,
        &plan.likely_sections,
    );

    let existing: std::collections::BTreeSet<String> = article_start
        .open_questions
        .iter()
        .map(|question| question.question.to_ascii_lowercase())
        .collect();
    for question in &plan.open_questions {
        if existing.contains(&question.to_ascii_lowercase()) {
            continue;
        }
        article_start.open_questions.push(OpenQuestion {
            question: question.clone(),
            reason: BRIEF_OPEN_QUESTION_REASON.to_string(),
            blocking: false,
            evidence: Vec::new(),
        });
    }

    if !plan.critic_notes_present {
        brief_report.warnings.push(
            "interview brief has no Interviewer Critic Notes; run the interviewer/critic loop before drafting"
                .to_string(),
        );
    }
}

fn merge_brief_planned_sections(section_skeleton: &mut Vec<SectionSkeleton>, planned: &[String]) {
    let mut planned_keys = std::collections::BTreeSet::new();
    let mut planned_sections = Vec::new();

    for name in planned {
        let key = normalized_heading_key(name);
        if key.is_empty() || is_structural_heading_key(&key) || !planned_keys.insert(key.clone()) {
            continue;
        }

        if let Some(index) = section_skeleton
            .iter()
            .position(|section| normalized_heading_key(&section.heading) == key)
        {
            planned_sections.push(section_skeleton.remove(index));
        } else {
            planned_sections.push(SectionSkeleton {
                heading: name.trim().to_string(),
                rationale: BRIEF_SECTION_RATIONALE.to_string(),
                required: false,
                content_backed: false,
                supporting_pages: Vec::new(),
            });
        }
    }

    if planned_sections.is_empty() {
        return;
    }

    let mut lead_sections = Vec::new();
    let mut remaining_body_sections = Vec::new();
    let mut appendix_sections = Vec::new();

    for section in std::mem::take(section_skeleton) {
        let key = normalized_heading_key(&section.heading);
        if key == "overview" {
            lead_sections.push(section);
        } else if is_terminal_appendix_key(&key) {
            appendix_sections.push(section);
        } else {
            remaining_body_sections.push(section);
        }
    }

    section_skeleton.extend(lead_sections);
    section_skeleton.extend(planned_sections);
    section_skeleton.extend(remaining_body_sections);
    section_skeleton.extend(appendix_sections);
}

fn normalized_heading_key(heading: &str) -> String {
    heading.trim().to_ascii_lowercase()
}

fn is_structural_heading_key(key: &str) -> bool {
    matches!(
        key,
        "lead"
            | "overview"
            | "introduction"
            | "summary"
            | "references"
            | "see also"
            | "external links"
            | "further reading"
            | "notes"
            | "citations"
    )
}

fn is_terminal_appendix_key(key: &str) -> bool {
    matches!(
        key,
        "see also" | "references" | "external links" | "further reading" | "notes" | "citations"
    )
}

#[derive(Debug, Serialize)]
struct ArticleStartBrief<'a> {
    schema_version: &'static str,
    command: &'static str,
    view: &'static str,
    status: &'static str,
    docs_profile_requested: &'a str,
    readiness: KnowledgeReadinessLevel,
    knowledge_generation: &'a str,
    topic: Option<&'a str>,
    intent: Option<&'a ArticleStartIntent>,
    local_state: Option<&'a LocalExistenceState>,
    interview_brief: Option<InterviewBriefCard<'a>>,
    evidence: Option<ArticleStartEvidenceCard<'a>>,
    local_integration: Option<ArticleStartIntegrationCard<'a>>,
    blocking: Vec<String>,
    warnings: Vec<String>,
    next_commands: Vec<BriefCommand>,
    drilldowns: Vec<BriefCommand>,
    full_view_command: Option<BriefCommand>,
}

#[derive(Debug, Serialize)]
struct ArticleStartEvidenceCard<'a> {
    query: &'a str,
    direct_subject_evidence_count: usize,
    broad_context_count: usize,
    comparable_page_count: usize,
    backlink_count: usize,
    missing_query_terms: &'a [String],
    live_leads_status: &'a str,
    top_direct_evidence: Vec<EvidenceCoverageCard<'a>>,
    top_context_evidence: Vec<EvidenceCoverageCard<'a>>,
}

#[derive(Debug, Serialize)]
struct InterviewBriefCard<'a> {
    status: &'a InterviewValidationStatus,
    path: &'a std::path::Path,
    summary: &'a InterviewBriefSummary,
    errors: &'a [String],
    warnings: &'a [String],
}

#[derive(Debug, Serialize)]
struct EvidenceCoverageCard<'a> {
    source_kind: &'a str,
    source_title: &'a str,
    locator: Option<&'a str>,
    evidence_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ArticleStartIntegrationCard<'a> {
    counts: ArticleStartIntegrationCounts,
    comparable_pages: Vec<String>,
    required_templates: Vec<RequiredTemplateCard<'a>>,
    available_infoboxes: Vec<TemplateSurfaceCard<'a>>,
    citation_templates_seen: Vec<&'a str>,
    template_surface: Vec<&'a str>,
    categories_seen: Vec<String>,
    links_seen: Vec<String>,
    section_skeleton: Vec<SectionSkeletonCard<'a>>,
    docs_queries: Vec<String>,
    contract_query: &'a str,
    contract_matched_query_terms: &'a [String],
    contract_missing_query_terms: &'a [String],
}

#[derive(Debug, Serialize)]
struct ArticleStartIntegrationCounts {
    comparable_pages: usize,
    required_templates: usize,
    available_infoboxes: usize,
    citation_templates_seen: usize,
    template_surface: usize,
    categories_seen: usize,
    links_seen: usize,
    section_skeleton: usize,
    docs_queries: usize,
}

#[derive(Debug, Serialize)]
struct RequiredTemplateCard<'a> {
    template_title: &'a str,
    reason: &'a str,
}

#[derive(Debug, Serialize)]
struct TemplateSurfaceCard<'a> {
    template_title: &'a str,
    source: &'a ContextSurfaceSource,
    mapped_subject_type: Option<&'a str>,
    supporting_pages: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SectionSkeletonCard<'a> {
    heading: &'a str,
    required: bool,
    content_backed: bool,
    rationale: &'a str,
    supporting_pages: Vec<String>,
}

fn build_article_start_brief<'a>(output: &'a KnowledgeArticleStartOutput) -> ArticleStartBrief<'a> {
    match &output.result {
        KnowledgeArticleStartPayload::IndexMissing => ArticleStartBrief {
            schema_version: "wikitool_brief_v1",
            command: "knowledge article-start",
            view: "brief",
            status: "index_missing",
            docs_profile_requested: &output.docs_profile_requested,
            readiness: output.readiness.clone(),
            knowledge_generation: &output.knowledge_generation,
            topic: None,
            intent: None,
            local_state: None,
            interview_brief: output.interview_brief.as_ref().map(interview_brief_card),
            evidence: None,
            local_integration: None,
            blocking: vec![
                "knowledge index is missing; run `wikitool knowledge build`".to_string(),
            ],
            warnings: output.degradations.clone(),
            next_commands: vec![brief_command(&[
                "wikitool",
                "knowledge",
                "build",
                "--format",
                "json",
            ])],
            drilldowns: Vec::new(),
            full_view_command: None,
        },
        KnowledgeArticleStartPayload::QueryMissing => ArticleStartBrief {
            schema_version: "wikitool_brief_v1",
            command: "knowledge article-start",
            view: "brief",
            status: "query_missing",
            docs_profile_requested: &output.docs_profile_requested,
            readiness: output.readiness.clone(),
            knowledge_generation: &output.knowledge_generation,
            topic: None,
            intent: None,
            local_state: None,
            interview_brief: output.interview_brief.as_ref().map(interview_brief_card),
            evidence: None,
            local_integration: None,
            blocking: vec!["topic or stub-derived query is required for article-start".to_string()],
            warnings: output.degradations.clone(),
            next_commands: Vec::new(),
            drilldowns: Vec::new(),
            full_view_command: None,
        },
        KnowledgeArticleStartPayload::Found { article_start } => {
            let mut warnings = output.degradations.clone();
            warnings.extend(
                article_start
                    .evidence_profile
                    .missing_evidence_warnings
                    .iter()
                    .take(6)
                    .cloned(),
            );
            warnings.extend(
                article_start
                    .local_integration
                    .contract_warnings
                    .iter()
                    .take(6)
                    .cloned(),
            );

            let blocking = article_start_blocking(article_start, output.interview_brief.as_ref());
            if let Some(brief) = &output.interview_brief {
                if brief.summary.claim_counts.pending_corroboration > 0 {
                    warnings.push(format!(
                        "interview brief has {} pending corroboration claim(s)",
                        brief.summary.claim_counts.pending_corroboration
                    ));
                }
                if brief.summary.do_not_assert_count > 0 {
                    warnings.push(format!(
                        "interview brief marks {} do-not-assert item(s); do not state these as fact without a source",
                        brief.summary.do_not_assert_count
                    ));
                }
                if brief.summary.open_item_counts.open > 0 {
                    warnings.push(format!(
                        "interview brief has {} open research item(s)",
                        brief.summary.open_item_counts.open
                    ));
                }
                if brief.summary.open_item_counts.negative_evidence > 0 {
                    warnings.push(format!(
                        "interview brief records {} negative-evidence item(s)",
                        brief.summary.open_item_counts.negative_evidence
                    ));
                }
                if brief.summary.computed_freshness == "stale" {
                    warnings.push("interview brief is stale".to_string());
                }
                warnings.extend(brief.warnings.iter().take(6).cloned());

                let brief_section_count = article_start
                    .local_integration
                    .section_skeleton
                    .iter()
                    .filter(|section| section.rationale == BRIEF_SECTION_RATIONALE)
                    .count();
                if brief_section_count > 0 {
                    warnings.push(format!(
                        "interview brief contributed {brief_section_count} planned section(s) beyond comparables; confirm they fit the subject"
                    ));
                }
                for question in article_start
                    .open_questions
                    .iter()
                    .filter(|question| {
                        !question.blocking && question.reason == BRIEF_OPEN_QUESTION_REASON
                    })
                    .take(6)
                {
                    warnings.push(format!(
                        "open question before drafting: {}",
                        question.question
                    ));
                }
            }

            let mut next_commands = Vec::new();
            next_commands.push(brief_command_owned(vec![
                "wikitool".to_string(),
                "knowledge".to_string(),
                "inspect".to_string(),
                "chunks".to_string(),
                "--across-pages".to_string(),
                "--query".to_string(),
                article_start.topic.clone(),
                "--limit".to_string(),
                "6".to_string(),
                "--token-budget".to_string(),
                "600".to_string(),
                "--format".to_string(),
                "json".to_string(),
                "--view".to_string(),
                "brief".to_string(),
            ]));
            if let Some(section) = article_start
                .local_integration
                .section_skeleton
                .iter()
                .find(|section| !section.content_backed)
            {
                next_commands.push(brief_command_owned(vec![
                    "wikitool".to_string(),
                    "knowledge".to_string(),
                    "inspect".to_string(),
                    "chunks".to_string(),
                    "--across-pages".to_string(),
                    "--query".to_string(),
                    format!("{} {}", article_start.topic, section.heading),
                    "--limit".to_string(),
                    "4".to_string(),
                    "--token-budget".to_string(),
                    "400".to_string(),
                    "--format".to_string(),
                    "json".to_string(),
                    "--view".to_string(),
                    "brief".to_string(),
                ]));
            }
            if let Some(template) = article_start
                .local_integration
                .required_templates
                .first()
                .map(|entry| entry.template_title.as_str())
                .or_else(|| {
                    article_start
                        .local_integration
                        .available_infoboxes
                        .first()
                        .map(|entry| entry.template_title.as_str())
                })
            {
                next_commands.push(brief_command_owned(vec![
                    "wikitool".to_string(),
                    "templates".to_string(),
                    "show".to_string(),
                    template.to_string(),
                    "--format".to_string(),
                    "json".to_string(),
                    "--view".to_string(),
                    "brief".to_string(),
                ]));
            }

            let drilldowns = vec![
                brief_command_owned(vec![
                    "wikitool".to_string(),
                    "research".to_string(),
                    "wiki-search".to_string(),
                    article_start.topic.clone(),
                    "--format".to_string(),
                    "json".to_string(),
                ]),
                brief_command_owned(vec![
                    "wikitool".to_string(),
                    "knowledge".to_string(),
                    "article-start".to_string(),
                    article_start.topic.clone(),
                    "--format".to_string(),
                    "json".to_string(),
                    "--view".to_string(),
                    "full".to_string(),
                ]),
            ];
            ArticleStartBrief {
                schema_version: "wikitool_brief_v1",
                command: "knowledge article-start",
                view: "brief",
                status: "found",
                docs_profile_requested: &output.docs_profile_requested,
                readiness: output.readiness.clone(),
                knowledge_generation: &output.knowledge_generation,
                topic: Some(&article_start.topic),
                intent: Some(&article_start.intent),
                local_state: Some(&article_start.local_state),
                interview_brief: output.interview_brief.as_ref().map(interview_brief_card),
                evidence: Some(ArticleStartEvidenceCard {
                    query: &article_start.evidence_profile.query,
                    direct_subject_evidence_count: article_start
                        .evidence_profile
                        .direct_subject_evidence
                        .len(),
                    broad_context_count: article_start.evidence_profile.broad_context.len(),
                    comparable_page_count: article_start.evidence_profile.comparable_pages.len(),
                    backlink_count: article_start.evidence_profile.backlink_count,
                    missing_query_terms: &article_start.evidence_profile.missing_query_terms,
                    live_leads_status: &article_start.evidence_profile.live_leads_status,
                    top_direct_evidence: article_start
                        .evidence_profile
                        .direct_subject_evidence
                        .iter()
                        .take(3)
                        .map(evidence_card)
                        .collect(),
                    top_context_evidence: article_start
                        .evidence_profile
                        .broad_context
                        .iter()
                        .take(3)
                        .map(evidence_card)
                        .collect(),
                }),
                local_integration: Some(ArticleStartIntegrationCard {
                    counts: ArticleStartIntegrationCounts {
                        comparable_pages: article_start.local_integration.comparable_pages.len(),
                        required_templates: article_start
                            .local_integration
                            .required_templates
                            .len(),
                        available_infoboxes: article_start
                            .local_integration
                            .available_infoboxes
                            .len(),
                        citation_templates_seen: article_start
                            .local_integration
                            .citation_templates_seen
                            .len(),
                        template_surface: article_start.local_integration.template_surface.len(),
                        categories_seen: article_start.local_integration.categories_seen.len(),
                        links_seen: article_start.local_integration.links_seen.len(),
                        section_skeleton: article_start.local_integration.section_skeleton.len(),
                        docs_queries: article_start.local_integration.docs_queries.len(),
                    },
                    comparable_pages: capped_strings(
                        &article_start.local_integration.comparable_pages,
                        5,
                    ),
                    required_templates: article_start
                        .local_integration
                        .required_templates
                        .iter()
                        .take(4)
                        .map(required_template_card)
                        .collect(),
                    available_infoboxes: article_start
                        .local_integration
                        .available_infoboxes
                        .iter()
                        .take(4)
                        .map(template_surface_card)
                        .collect(),
                    citation_templates_seen: article_start
                        .local_integration
                        .citation_templates_seen
                        .iter()
                        .map(|entry| entry.template_title.as_str())
                        .take(6)
                        .collect(),
                    template_surface: article_start
                        .local_integration
                        .template_surface
                        .iter()
                        .map(|entry| entry.template_title.as_str())
                        .take(8)
                        .collect(),
                    categories_seen: article_start
                        .local_integration
                        .categories_seen
                        .iter()
                        .take(6)
                        .map(|entry| entry.category_title.clone())
                        .collect(),
                    links_seen: article_start
                        .local_integration
                        .links_seen
                        .iter()
                        .take(8)
                        .map(|entry| entry.page_title.clone())
                        .collect(),
                    section_skeleton: article_start
                        .local_integration
                        .section_skeleton
                        .iter()
                        .take(if output.interview_brief.is_some() {
                            12
                        } else {
                            6
                        })
                        .map(section_card)
                        .collect(),
                    docs_queries: capped_strings(&article_start.local_integration.docs_queries, 4),
                    contract_query: &article_start.local_integration.contract_query,
                    contract_matched_query_terms: &article_start
                        .local_integration
                        .contract_matched_query_terms,
                    contract_missing_query_terms: &article_start
                        .local_integration
                        .contract_missing_query_terms,
                }),
                blocking,
                warnings,
                next_commands,
                drilldowns,
                full_view_command: Some(brief_command_owned(
                    vec![
                        "wikitool".to_string(),
                        "knowledge".to_string(),
                        "article-start".to_string(),
                        article_start.topic.clone(),
                        "--format".to_string(),
                        "json".to_string(),
                        "--view".to_string(),
                        "full".to_string(),
                    ]
                    .into_iter()
                    .filter(|value| !value.is_empty())
                    .collect(),
                )),
            }
        }
    }
}

fn article_start_brief_readiness(
    base: &KnowledgeReadinessLevel,
    blocking: &[String],
    interview_brief: Option<&InterviewValidationReport>,
) -> KnowledgeReadinessLevel {
    if matches!(base, KnowledgeReadinessLevel::NotReady) || !blocking.is_empty() {
        return KnowledgeReadinessLevel::NotReady;
    }

    let Some(brief) = interview_brief else {
        return base.clone();
    };
    if brief.status == InterviewValidationStatus::Invalid
        || brief.summary.computed_freshness == "stale"
    {
        return KnowledgeReadinessLevel::NotReady;
    }
    // Open interview items are normal for high-context subjects and are already
    // surfaced as warnings. Only unresolved factual claims or negative evidence
    // should cap an otherwise authoring-ready brief.
    if brief.summary.claim_counts.pending_corroboration > 0
        || brief.summary.open_item_counts.negative_evidence > 0
    {
        return KnowledgeReadinessLevel::ContentReady;
    }

    base.clone()
}

fn article_start_output_readiness(output: &KnowledgeArticleStartOutput) -> KnowledgeReadinessLevel {
    match &output.result {
        KnowledgeArticleStartPayload::IndexMissing | KnowledgeArticleStartPayload::QueryMissing => {
            KnowledgeReadinessLevel::NotReady
        }
        KnowledgeArticleStartPayload::Found { article_start } => {
            let blocking = article_start_blocking(article_start, output.interview_brief.as_ref());
            article_start_brief_readiness(
                &output.readiness,
                &blocking,
                output.interview_brief.as_ref(),
            )
        }
    }
}

fn article_start_blocking(
    article_start: &ArticleStartResult,
    interview_brief: Option<&InterviewValidationReport>,
) -> Vec<String> {
    // Only genuine blockers belong here. A contract query term miss is advisory
    // (already surfaced via contract_warnings) and is expected for niche
    // subjects, so it must not force readiness to not_ready.
    let mut blocking = article_start
        .open_questions
        .iter()
        .filter(|question| question.blocking)
        .map(|question| question.question.clone())
        .collect::<Vec<_>>();
    if let Some(brief) = interview_brief
        && brief.status == InterviewValidationStatus::Invalid
    {
        blocking.push(format!(
            "interview brief is invalid: {}",
            brief.errors.join("; ")
        ));
    }
    blocking
}

fn evidence_card(evidence: &EvidenceCoverageItem) -> EvidenceCoverageCard<'_> {
    EvidenceCoverageCard {
        source_kind: &evidence.source_kind,
        source_title: &evidence.source_title,
        locator: evidence.locator.as_deref(),
        evidence_id: evidence.evidence_id.as_deref(),
    }
}

fn interview_brief_card(report: &InterviewValidationReport) -> InterviewBriefCard<'_> {
    InterviewBriefCard {
        status: &report.status,
        path: &report.path,
        summary: &report.summary,
        errors: &report.errors,
        warnings: &report.warnings,
    }
}

fn required_template_card(template: &RequiredTemplate) -> RequiredTemplateCard<'_> {
    RequiredTemplateCard {
        template_title: &template.template_title,
        reason: &template.reason,
    }
}

fn template_surface_card(template: &TemplateSurfaceEntry) -> TemplateSurfaceCard<'_> {
    TemplateSurfaceCard {
        template_title: &template.template_title,
        source: &template.source,
        mapped_subject_type: template.mapped_subject_type.as_deref(),
        supporting_pages: capped_strings(&template.supporting_pages, 3),
    }
}

fn section_card(section: &SectionSkeleton) -> SectionSkeletonCard<'_> {
    SectionSkeletonCard {
        heading: &section.heading,
        required: section.required,
        content_backed: section.content_backed,
        rationale: &section.rationale,
        supporting_pages: capped_strings(&section.supporting_pages, 3)
            .into_iter()
            .map(|value| text_preview(&value, 120))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use wikitool_core::knowledge_interview::{InterviewClaimCounts, InterviewOpenItemCounts};

    fn section(heading: &str, rationale: &str, required: bool) -> SectionSkeleton {
        SectionSkeleton {
            heading: heading.to_string(),
            rationale: rationale.to_string(),
            required,
            content_backed: false,
            supporting_pages: Vec::new(),
        }
    }

    fn valid_interview_report() -> InterviewValidationReport {
        InterviewValidationReport {
            schema_version: "knowledge_interview_validation_v1",
            path: PathBuf::from("brief.md"),
            status: InterviewValidationStatus::Valid,
            summary: InterviewBriefSummary {
                doc_id: None,
                title: None,
                title_key: None,
                intent: None,
                created_at: None,
                last_updated: None,
                freshness_state: Some("fresh".to_string()),
                computed_freshness: "fresh".to_string(),
                agent: None,
                claims_sidecar: None,
                open_items_sidecar: None,
                sections_present: Vec::new(),
                sections_missing: Vec::new(),
                claim_counts: InterviewClaimCounts::default(),
                source_lead_count: 0,
                do_not_assert_count: 0,
                open_item_count: 0,
                open_item_counts: InterviewOpenItemCounts {
                    by_kind: BTreeMap::new(),
                    by_status: BTreeMap::new(),
                    ..InterviewOpenItemCounts::default()
                },
            },
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn brief_planned_sections_define_body_order_before_appendices() {
        let mut skeleton = vec![
            section("Overview", "lead", true),
            section("Background", "Seen on comparables.", false),
            section("References", "Required appendix.", true),
            section("Reception", "Seen on comparables.", false),
        ];

        merge_brief_planned_sections(
            &mut skeleton,
            &[
                "Design, aesthetic, and presentation".to_string(),
                "Reception".to_string(),
                "References".to_string(),
            ],
        );

        let headings = skeleton
            .iter()
            .map(|section| section.heading.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            headings,
            vec![
                "Overview",
                "Design, aesthetic, and presentation",
                "Reception",
                "Background",
                "References"
            ]
        );
        assert_eq!(
            skeleton[1].rationale,
            "Planned in the interview brief Draft Plan; not observed on comparable pages."
        );
        assert_eq!(skeleton[2].rationale, "Seen on comparables.");
    }

    #[test]
    fn brief_planned_sections_ignore_duplicates_and_structural_labels() {
        let mut skeleton = vec![
            section("Overview", "lead", true),
            section("References", "Required appendix.", true),
        ];

        merge_brief_planned_sections(
            &mut skeleton,
            &[
                "Lead".to_string(),
                "Design".to_string(),
                "design".to_string(),
                "See also".to_string(),
            ],
        );

        let headings = skeleton
            .iter()
            .map(|section| section.heading.as_str())
            .collect::<Vec<_>>();
        assert_eq!(headings, vec!["Overview", "Design", "References"]);
    }

    #[test]
    fn brief_readiness_is_not_ready_when_article_start_has_blockers() {
        let readiness = article_start_brief_readiness(
            &KnowledgeReadinessLevel::ContentReady,
            &["interview brief is invalid: missing required frontmatter".to_string()],
            None,
        );

        assert_eq!(readiness, KnowledgeReadinessLevel::NotReady);
    }

    #[test]
    fn brief_readiness_keeps_open_items_advisory() {
        let mut brief = valid_interview_report();
        brief.summary.open_item_counts.open = 1;

        let readiness = article_start_brief_readiness(
            &KnowledgeReadinessLevel::AuthoringReady,
            &[],
            Some(&brief),
        );

        assert_eq!(readiness, KnowledgeReadinessLevel::AuthoringReady);
    }

    #[test]
    fn brief_readiness_downgrades_pending_claims() {
        let mut brief = valid_interview_report();
        brief.summary.claim_counts.pending_corroboration = 1;

        let readiness = article_start_brief_readiness(
            &KnowledgeReadinessLevel::AuthoringReady,
            &[],
            Some(&brief),
        );

        assert_eq!(readiness, KnowledgeReadinessLevel::ContentReady);
    }
}
