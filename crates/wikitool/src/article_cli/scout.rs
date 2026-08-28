use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, ValueEnum};
use serde::Serialize;
use wikitool_core::authoring::article_scout::build_article_scout;
use wikitool_core::authoring::model::{
    ArticleScoutIntent, ArticleScoutResult, ContextCoverageItem, ContextSurfaceSource,
    LocalExistenceState, RequiredTemplate, SectionCandidate, TemplateSurfaceEntry,
};
use wikitool_core::catalog::authoring::{
    AuthoringContextOptions, AuthoringContextOutcome, AuthoringContractProfile,
    AuthoringPayloadMode, build_authoring_context,
};
use wikitool_core::catalog::status::{CatalogReadiness, catalog_status};
use wikitool_core::filesystem::validate_scoped_path;
use wikitool_core::site::load_site_adapter;
use wikitool_core::wiki_interview::{
    InterviewBriefSummary, InterviewValidationReport, InterviewValidationStatus,
    validate_interview_brief,
};

use crate::briefs::{
    BriefCommand, BriefView, brief_command, brief_command_owned, capped_strings, text_preview,
};
use crate::catalog_cli::shared::{
    derive_topic_from_stub_path, format_list, format_readiness, load_stub_content,
};
use crate::cli_support::{
    OutputFormat, normalize_option, normalize_path, resolve_runtime_with_docs_profile,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args, Clone)]
pub(crate) struct ArticleScoutArgs {
    #[arg(
        value_name = "TOPIC",
        help = "Primary article topic/title for retrieval"
    )]
    topic: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional stub wikitext file used for link/template hint extraction"
    )]
    stub_path: Option<PathBuf>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional wiki interview ledger to validate and include in the scout packet"
    )]
    brief_path: Option<PathBuf>,
    #[arg(
        long,
        default_value_t = 45,
        value_name = "DAYS",
        help = "Age in days after which an interview ledger is considered stale"
    )]
    brief_stale_days: u64,
    #[arg(
        long,
        default_value_t = 18,
        value_name = "N",
        help = "Maximum related pages"
    )]
    related_limit: usize,
    #[arg(
        long,
        default_value_t = 10,
        value_name = "N",
        help = "Maximum retrieved context chunks"
    )]
    chunk_limit: usize,
    #[arg(
        long,
        default_value_t = 1200,
        value_name = "TOKENS",
        help = "Token budget across retrieved chunks"
    )]
    token_budget: usize,
    #[arg(
        long,
        default_value_t = 8,
        value_name = "N",
        help = "Maximum distinct source pages in chunk retrieval"
    )]
    max_pages: usize,
    #[arg(
        long,
        default_value_t = 18,
        value_name = "N",
        help = "Maximum internal-link observations"
    )]
    link_limit: usize,
    #[arg(
        long,
        default_value_t = 8,
        value_name = "N",
        help = "Maximum category observations"
    )]
    category_limit: usize,
    #[arg(
        long,
        default_value_t = 16,
        value_name = "N",
        help = "Maximum template summaries"
    )]
    template_limit: usize,
    #[arg(
        long,
        value_name = "PROFILE",
        help = "Docs profile for bridged retrieval (default: configured site adapter)"
    )]
    docs_profile: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = ArticleScoutContractProfileArg::Author,
        value_name = "PROFILE",
        help = "Contract traversal profile: index|author|implementation"
    )]
    contract_profile: ArticleScoutContractProfileArg,
    #[arg(
        long,
        value_name = "QUERY",
        help = "Optional contract traversal query separate from TOPIC"
    )]
    contract_query: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
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
    #[arg(
        long,
        value_enum,
        default_value_t = ArticleScoutIntentArg::New,
        value_name = "INTENT",
        help = "Authoring intent: new|expand|audit|refresh"
    )]
    intent: ArticleScoutIntentArg,
    #[arg(long, help = "Enable lexical chunk de-duplication and diversification")]
    diversify: bool,
    #[arg(
        long,
        help = "Disable lexical chunk de-duplication and diversification"
    )]
    no_diversify: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ArticleScoutIntentArg {
    New,
    Expand,
    Audit,
    Refresh,
}

impl From<ArticleScoutIntentArg> for ArticleScoutIntent {
    fn from(value: ArticleScoutIntentArg) -> Self {
        match value {
            ArticleScoutIntentArg::New => Self::New,
            ArticleScoutIntentArg::Expand => Self::Expand,
            ArticleScoutIntentArg::Audit => Self::Audit,
            ArticleScoutIntentArg::Refresh => Self::Refresh,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ArticleScoutContractProfileArg {
    Index,
    Author,
    Implementation,
}

impl From<ArticleScoutContractProfileArg> for AuthoringContractProfile {
    fn from(value: ArticleScoutContractProfileArg) -> Self {
        match value {
            ArticleScoutContractProfileArg::Index => Self::Index,
            ArticleScoutContractProfileArg::Author => Self::Author,
            ArticleScoutContractProfileArg::Implementation => Self::Implementation,
        }
    }
}

#[derive(Debug, Serialize)]
struct ArticleScoutOutput {
    docs_profile_requested: String,
    readiness: CatalogReadiness,
    degradations: Vec<String>,
    catalog_generation: String,
    interview_brief: Option<InterviewValidationReport>,
    result: ArticleScoutPayload,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ArticleScoutPayload {
    IndexMissing,
    QueryMissing,
    Found {
        #[serde(rename = "scout")]
        article_scout: Box<ArticleScoutResult>,
    },
}

pub(crate) fn run_article_scout(runtime: &RuntimeOptions, args: ArticleScoutArgs) -> Result<()> {
    if args.related_limit == 0 {
        bail!("article scout requires --related-limit >= 1");
    }
    if args.chunk_limit == 0 {
        bail!("article scout requires --chunk-limit >= 1");
    }
    if args.token_budget == 0 {
        bail!("article scout requires --token-budget >= 1");
    }
    if args.max_pages == 0 {
        bail!("article scout requires --max-pages >= 1");
    }
    if args.link_limit == 0 {
        bail!("article scout requires --link-limit >= 1");
    }
    if args.category_limit == 0 {
        bail!("article scout requires --category-limit >= 1");
    }
    if args.template_limit == 0 {
        bail!("article scout requires --template-limit >= 1");
    }
    if args.diversify && args.no_diversify {
        bail!("cannot use --diversify and --no-diversify together");
    }

    let use_diversify = !args.no_diversify;
    let (paths, _config, docs_profile) =
        resolve_runtime_with_docs_profile(runtime, args.docs_profile.as_deref())?;
    let interview_brief = match args.brief_path.as_deref() {
        Some(path) => {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                paths.project_root.join(path)
            };
            validate_scoped_path(&paths, &absolute)?;
            let report = validate_interview_brief(&absolute, args.brief_stale_days)?;
            Some(report)
        }
        None => None,
    };
    let topic = normalize_option(args.topic.as_deref())
        .or_else(|| derive_topic_from_stub_path(args.stub_path.as_deref()));
    let stub_content = load_stub_content(&paths, args.stub_path.as_deref())?;
    let pack = build_authoring_context(
        &paths,
        topic.as_deref(),
        stub_content.as_deref(),
        &AuthoringContextOptions {
            related_page_limit: args.related_limit,
            chunk_limit: args.chunk_limit,
            token_budget: args.token_budget,
            max_pages: args.max_pages,
            link_limit: args.link_limit,
            category_limit: args.category_limit,
            template_limit: args.template_limit,
            docs_profile: docs_profile.clone(),
            diversify: use_diversify,
            payload_mode: AuthoringPayloadMode::Compact,
            contract_profile: args.contract_profile.into(),
            contract_query: normalize_option(args.contract_query.as_deref()),
        },
    )?;
    let status = catalog_status(&paths, &docs_profile)?;
    let mut output = match pack {
        AuthoringContextOutcome::IndexMissing => ArticleScoutOutput {
            docs_profile_requested: status.docs_profile_requested.clone(),
            readiness: status.readiness.clone(),
            degradations: status.degradations.clone(),
            catalog_generation: status.catalog_generation.clone(),
            interview_brief: interview_brief.clone(),
            result: ArticleScoutPayload::IndexMissing,
        },
        AuthoringContextOutcome::QueryMissing => ArticleScoutOutput {
            docs_profile_requested: status.docs_profile_requested.clone(),
            readiness: status.readiness.clone(),
            degradations: status.degradations.clone(),
            catalog_generation: status.catalog_generation.clone(),
            interview_brief: interview_brief.clone(),
            result: ArticleScoutPayload::QueryMissing,
        },
        AuthoringContextOutcome::Found(report) => {
            let adapter = load_site_adapter(&paths)?;
            let article_scout = build_article_scout(&report, &adapter, args.intent.into());
            ArticleScoutOutput {
                docs_profile_requested: status.docs_profile_requested.clone(),
                readiness: status.readiness.clone(),
                degradations: status.degradations.clone(),
                catalog_generation: status.catalog_generation.clone(),
                interview_brief,
                result: ArticleScoutPayload::Found {
                    article_scout: Box::new(article_scout),
                },
            }
        }
    };
    output.readiness = article_scout_output_readiness(&output);

    if args.format.is_json() {
        if args.view.is_full() {
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&build_article_scout_brief(&output))?
            );
        }
        return Ok(());
    }

    println!("article scout");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!(
        "topic: {}",
        topic.as_deref().unwrap_or("<derived-from-stub>")
    );
    println!("docs_profile_requested: {}", output.docs_profile_requested);
    println!("catalog_generation: {}", output.catalog_generation);
    println!("readiness: {}", format_readiness(&output.readiness));
    println!("degradations: {}", format_list(&output.degradations));
    match output.result {
        ArticleScoutPayload::IndexMissing => {
            bail!("article scout requires a built content catalog; run `wikitool catalog build`");
        }
        ArticleScoutPayload::QueryMissing => {
            bail!("article scout requires a topic or a stub with at least one resolvable wikilink");
        }
        ArticleScoutPayload::Found { article_scout, .. } => {
            println!(
                "article_scout.schema_version: {}",
                article_scout.schema_version
            );
            println!(
                "article_scout.intent: {}",
                serde_json::to_string(&article_scout.intent)?
            );
            println!("article_scout.topic: {}", article_scout.topic);
            println!(
                "article_scout.local_state: {}",
                serde_json::to_string(&article_scout.local_state)?
            );
            println!(
                "article_scout.context.subject_context.count: {}",
                article_scout.context_profile.subject_context.len()
            );
            println!(
                "article_scout.context.broad_context.count: {}",
                article_scout.context_profile.broad_context.len()
            );
            println!(
                "article_scout.context.missing_query_terms: {}",
                format_list(&article_scout.context_profile.missing_query_terms)
            );
            for warning in article_scout.context_profile.context_gaps.iter().take(4) {
                println!("article_scout.context.gap: {warning}");
            }
            println!(
                "article_scout.comparable_pages: {}",
                format_list(&article_scout.local_integration.comparable_pages)
            );
            println!(
                "article_scout.required_templates: {}",
                article_scout
                    .local_integration
                    .required_templates
                    .iter()
                    .map(|entry| entry.template_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_scout.subject_type_hints: {}",
                article_scout
                    .local_integration
                    .subject_type_hints
                    .iter()
                    .map(|entry| entry.subject_type.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_scout.available_infoboxes: {}",
                article_scout
                    .local_integration
                    .available_infoboxes
                    .iter()
                    .map(|entry| entry.template_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_scout.citation_templates_seen: {}",
                article_scout
                    .local_integration
                    .citation_templates_seen
                    .iter()
                    .map(|entry| entry.template_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_scout.template_surface: {}",
                article_scout
                    .local_integration
                    .template_surface
                    .iter()
                    .map(|entry| entry.template_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_scout.observed_categories: {}",
                article_scout
                    .local_integration
                    .observed_categories
                    .iter()
                    .map(|entry| entry.category_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_scout.observed_links: {}",
                article_scout
                    .local_integration
                    .observed_links
                    .iter()
                    .map(|entry| entry.page_title.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_scout.section_candidates: {}",
                article_scout
                    .local_integration
                    .section_candidates
                    .iter()
                    .map(|entry| entry.heading.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!(
                "article_scout.docs_queries: {}",
                format_list(&article_scout.local_integration.docs_queries)
            );
            println!(
                "article_scout.contract_query: {}",
                article_scout.local_integration.contract_query
            );
            println!(
                "article_scout.contract_missing_query_terms: {}",
                format_list(&article_scout.local_integration.contract_missing_query_terms)
            );
            for warning in article_scout
                .local_integration
                .contract_warnings
                .iter()
                .take(4)
            {
                println!("article_scout.contract_warning: {warning}");
            }
        }
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct ArticleScoutBrief<'a> {
    schema_version: &'static str,
    command: &'static str,
    view: &'static str,
    status: &'static str,
    docs_profile_requested: &'a str,
    readiness: CatalogReadiness,
    catalog_generation: &'a str,
    topic: Option<&'a str>,
    intent: Option<&'a ArticleScoutIntent>,
    local_state: Option<&'a LocalExistenceState>,
    interview_brief: Option<InterviewBriefCard<'a>>,
    context: Option<ArticleScoutContextCard<'a>>,
    local_integration: Option<ArticleScoutIntegrationCard<'a>>,
    blocking: Vec<String>,
    warnings: Vec<String>,
    next_commands: Vec<BriefCommand>,
    drilldowns: Vec<BriefCommand>,
    full_view_command: Option<BriefCommand>,
}

#[derive(Debug, Serialize)]
struct ArticleScoutContextCard<'a> {
    query: &'a str,
    subject_context_count: usize,
    broad_context_count: usize,
    comparable_page_count: usize,
    backlink_count: usize,
    missing_query_terms: &'a [String],
    live_leads_status: &'a str,
    top_subject_context: Vec<ContextCoverageCard<'a>>,
    top_broad_context: Vec<ContextCoverageCard<'a>>,
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
struct ContextCoverageCard<'a> {
    source_kind: &'a str,
    source_title: &'a str,
    locator: Option<&'a str>,
    context_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_preview: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ArticleScoutIntegrationCard<'a> {
    comparable_pages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    closest_comparable_outline: Option<&'a wikitool_core::authoring::model::ComparableOutline>,
    required_templates: Vec<RequiredTemplateCard<'a>>,
    available_infoboxes: Vec<TemplateSurfaceCard<'a>>,
    citation_templates_seen: Vec<&'a str>,
    template_surface: Vec<&'a str>,
    observed_categories: Vec<ObservedCategoryCard<'a>>,
    observed_links: Vec<ObservedLinkCard<'a>>,
    section_candidates: Vec<SectionCandidateCard<'a>>,
    docs_queries: Vec<String>,
    contract_query: &'a str,
    contract_matched_query_terms: &'a [String],
    contract_missing_query_terms: &'a [String],
}

#[derive(Debug, Serialize)]
struct RequiredTemplateCard<'a> {
    template_title: &'a str,
    reason: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    parameter_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TemplateSurfaceCard<'a> {
    template_title: &'a str,
    source: &'a ContextSurfaceSource,
    mapped_subject_type: Option<&'a str>,
    supporting_pages: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    parameter_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ObservedCategoryCard<'a> {
    category_title: &'a str,
    source: &'a ContextSurfaceSource,
    supporting_pages: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ObservedLinkCard<'a> {
    page_title: &'a str,
    source: &'a ContextSurfaceSource,
    supporting_pages: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SectionCandidateCard<'a> {
    heading: &'a str,
    required: bool,
    content_backed: bool,
    rationale: &'a str,
    supporting_pages: Vec<String>,
}

fn build_article_scout_brief<'a>(output: &'a ArticleScoutOutput) -> ArticleScoutBrief<'a> {
    match &output.result {
        ArticleScoutPayload::IndexMissing => ArticleScoutBrief {
            schema_version: "wikitool_brief_v1",
            command: "article scout",
            view: "brief",
            status: "index_missing",
            docs_profile_requested: &output.docs_profile_requested,
            readiness: output.readiness.clone(),
            catalog_generation: &output.catalog_generation,
            topic: None,
            intent: None,
            local_state: None,
            interview_brief: output.interview_brief.as_ref().map(interview_brief_card),
            context: None,
            local_integration: None,
            blocking: vec!["content catalog is missing; run `wikitool catalog build`".to_string()],
            warnings: output.degradations.clone(),
            next_commands: vec![brief_command(&[
                "wikitool", "catalog", "build", "--format", "json",
            ])],
            drilldowns: Vec::new(),
            full_view_command: None,
        },
        ArticleScoutPayload::QueryMissing => ArticleScoutBrief {
            schema_version: "wikitool_brief_v1",
            command: "article scout",
            view: "brief",
            status: "query_missing",
            docs_profile_requested: &output.docs_profile_requested,
            readiness: output.readiness.clone(),
            catalog_generation: &output.catalog_generation,
            topic: None,
            intent: None,
            local_state: None,
            interview_brief: output.interview_brief.as_ref().map(interview_brief_card),
            context: None,
            local_integration: None,
            blocking: vec!["topic or stub-derived query is required for article scout".to_string()],
            warnings: output.degradations.clone(),
            next_commands: Vec::new(),
            drilldowns: Vec::new(),
            full_view_command: None,
        },
        ArticleScoutPayload::Found { article_scout } => {
            let mut warnings = output.degradations.clone();
            warnings.extend(
                article_scout
                    .context_profile
                    .context_gaps
                    .iter()
                    .take(6)
                    .cloned(),
            );
            warnings.extend(
                article_scout
                    .local_integration
                    .contract_warnings
                    .iter()
                    .take(6)
                    .cloned(),
            );

            let blocking = article_scout_blocking(article_scout, output.interview_brief.as_ref());
            if let Some(brief) = &output.interview_brief {
                let dna = brief
                    .summary
                    .open_item_counts
                    .by_kind
                    .get("do_not_assert")
                    .copied()
                    .unwrap_or(0);
                if dna > 0 {
                    warnings.push(format!(
                        "interview brief marks {dna} do-not-assert item(s); do not state these as fact without a source"
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
            }

            let mut next_commands = Vec::new();
            next_commands.push(brief_command_owned(vec![
                "wikitool".to_string(),
                "catalog".to_string(),
                "inspect".to_string(),
                "chunks".to_string(),
                "--across-pages".to_string(),
                "--query".to_string(),
                article_scout.topic.clone(),
                "--limit".to_string(),
                "6".to_string(),
                "--token-budget".to_string(),
                "600".to_string(),
                "--format".to_string(),
                "json".to_string(),
                "--view".to_string(),
                "brief".to_string(),
            ]));
            if let Some(section) = article_scout
                .local_integration
                .section_candidates
                .iter()
                .find(|section| !section.required && !section.content_backed)
            {
                next_commands.push(brief_command_owned(vec![
                    "wikitool".to_string(),
                    "catalog".to_string(),
                    "inspect".to_string(),
                    "chunks".to_string(),
                    "--across-pages".to_string(),
                    "--query".to_string(),
                    format!("{} {}", article_scout.topic, section.heading),
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
            if let Some(template) = article_scout
                .local_integration
                .required_templates
                .first()
                .map(|entry| entry.template_title.as_str())
                .or_else(|| {
                    article_scout
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
                    article_scout.topic.clone(),
                    "--format".to_string(),
                    "json".to_string(),
                ]),
                brief_command_owned(vec![
                    "wikitool".to_string(),
                    "article".to_string(),
                    "scout".to_string(),
                    article_scout.topic.clone(),
                    "--format".to_string(),
                    "json".to_string(),
                    "--view".to_string(),
                    "full".to_string(),
                ]),
            ];
            ArticleScoutBrief {
                schema_version: "wikitool_brief_v1",
                command: "article scout",
                view: "brief",
                status: "found",
                docs_profile_requested: &output.docs_profile_requested,
                readiness: output.readiness.clone(),
                catalog_generation: &output.catalog_generation,
                topic: Some(&article_scout.topic),
                intent: Some(&article_scout.intent),
                local_state: Some(&article_scout.local_state),
                interview_brief: output.interview_brief.as_ref().map(interview_brief_card),
                context: Some(ArticleScoutContextCard {
                    query: &article_scout.context_profile.query,
                    subject_context_count: article_scout.context_profile.subject_context.len(),
                    broad_context_count: article_scout.context_profile.broad_context.len(),
                    comparable_page_count: article_scout.context_profile.comparable_pages.len(),
                    backlink_count: article_scout.context_profile.backlink_count,
                    missing_query_terms: &article_scout.context_profile.missing_query_terms,
                    live_leads_status: &article_scout.context_profile.live_leads_status,
                    top_subject_context: article_scout
                        .context_profile
                        .subject_context
                        .iter()
                        .take(3)
                        .map(|item| {
                            context_card(item, &article_scout.research_context.context_refs)
                        })
                        .collect(),
                    top_broad_context: article_scout
                        .context_profile
                        .broad_context
                        .iter()
                        .take(3)
                        .map(|item| {
                            context_card(item, &article_scout.research_context.context_refs)
                        })
                        .collect(),
                }),
                local_integration: Some(ArticleScoutIntegrationCard {
                    comparable_pages: capped_strings(
                        &article_scout.local_integration.comparable_pages,
                        5,
                    ),
                    closest_comparable_outline: article_scout
                        .local_integration
                        .closest_comparable_outline
                        .as_ref(),
                    required_templates: article_scout
                        .local_integration
                        .required_templates
                        .iter()
                        .take(4)
                        .map(required_template_card)
                        .collect(),
                    available_infoboxes: article_scout
                        .local_integration
                        .available_infoboxes
                        .iter()
                        .take(4)
                        .map(template_surface_card)
                        .collect(),
                    citation_templates_seen: article_scout
                        .local_integration
                        .citation_templates_seen
                        .iter()
                        .map(|entry| entry.template_title.as_str())
                        .take(6)
                        .collect(),
                    template_surface: article_scout
                        .local_integration
                        .template_surface
                        .iter()
                        .map(|entry| entry.template_title.as_str())
                        .take(8)
                        .collect(),
                    observed_categories: article_scout
                        .local_integration
                        .observed_categories
                        .iter()
                        .take(6)
                        .map(|entry| ObservedCategoryCard {
                            category_title: &entry.category_title,
                            source: &entry.source,
                            supporting_pages: capped_strings(&entry.supporting_pages, 3),
                        })
                        .collect(),
                    observed_links: article_scout
                        .local_integration
                        .observed_links
                        .iter()
                        .take(8)
                        .map(|entry| ObservedLinkCard {
                            page_title: &entry.page_title,
                            source: &entry.source,
                            supporting_pages: capped_strings(&entry.supporting_pages, 3),
                        })
                        .collect(),
                    section_candidates: article_scout
                        .local_integration
                        .section_candidates
                        .iter()
                        .take(if output.interview_brief.is_some() {
                            12
                        } else {
                            6
                        })
                        .map(section_card)
                        .collect(),
                    docs_queries: capped_strings(&article_scout.local_integration.docs_queries, 4),
                    contract_query: &article_scout.local_integration.contract_query,
                    contract_matched_query_terms: &article_scout
                        .local_integration
                        .contract_matched_query_terms,
                    contract_missing_query_terms: &article_scout
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
                        "article".to_string(),
                        "scout".to_string(),
                        article_scout.topic.clone(),
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

fn article_scout_brief_readiness(
    base: &CatalogReadiness,
    blocking: &[String],
    interview_brief: Option<&InterviewValidationReport>,
) -> CatalogReadiness {
    if matches!(base, CatalogReadiness::NotReady) || !blocking.is_empty() {
        return CatalogReadiness::NotReady;
    }

    let Some(brief) = interview_brief else {
        return base.clone();
    };
    if brief.status == InterviewValidationStatus::Invalid
        || brief.summary.computed_freshness == "stale"
    {
        return CatalogReadiness::NotReady;
    }
    // Open interview items are normal for high-context subjects and are already
    // surfaced as warnings. Only negative evidence should cap an otherwise
    // authoring-ready brief.
    if brief.summary.open_item_counts.negative_evidence > 0 {
        return CatalogReadiness::ContentReady;
    }

    base.clone()
}

fn article_scout_output_readiness(output: &ArticleScoutOutput) -> CatalogReadiness {
    match &output.result {
        ArticleScoutPayload::IndexMissing | ArticleScoutPayload::QueryMissing => {
            CatalogReadiness::NotReady
        }
        ArticleScoutPayload::Found { article_scout } => {
            let blocking = article_scout_blocking(article_scout, output.interview_brief.as_ref());
            article_scout_brief_readiness(
                &output.readiness,
                &blocking,
                output.interview_brief.as_ref(),
            )
        }
    }
}

fn article_scout_blocking(
    _article_scout: &ArticleScoutResult,
    interview_brief: Option<&InterviewValidationReport>,
) -> Vec<String> {
    // Only genuine blockers belong here. A contract query term miss is advisory
    // (already surfaced via contract_warnings) and is expected for niche
    // subjects, so it must not force readiness to not_ready.
    let mut blocking = Vec::new();
    if let Some(brief) = interview_brief {
        blocking.extend(
            brief
                .summary
                .ledger
                .blocking_evidence_gaps
                .iter()
                .map(|gap| format!("interview brief records a blocking evidence gap: {gap}")),
        );
        if brief.status == InterviewValidationStatus::Invalid {
            blocking.push(format!(
                "interview brief is invalid: {}",
                brief.errors.join("; ")
            ));
        }
    }
    blocking
}

fn context_card<'a>(
    context: &'a ContextCoverageItem,
    context_refs: &'a [wikitool_core::authoring::model::ContextRef],
) -> ContextCoverageCard<'a> {
    let text_preview = context.context_id.as_deref().and_then(|id| {
        context_refs
            .iter()
            .find(|reference| reference.id == id)
            .and_then(|reference| reference.text_preview.as_deref())
    });
    ContextCoverageCard {
        source_kind: &context.source_kind,
        source_title: &context.source_title,
        locator: context.locator.as_deref(),
        context_id: context.context_id.as_deref(),
        text_preview,
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
        parameter_keys: capped_strings(&template.parameter_keys, 12),
    }
}

fn template_surface_card(template: &TemplateSurfaceEntry) -> TemplateSurfaceCard<'_> {
    TemplateSurfaceCard {
        template_title: &template.template_title,
        source: &template.source,
        mapped_subject_type: template.mapped_subject_type.as_deref(),
        supporting_pages: capped_strings(&template.supporting_pages, 3),
        parameter_keys: capped_strings(&template.parameter_keys, 12),
    }
}

fn section_card(section: &SectionCandidate) -> SectionCandidateCard<'_> {
    SectionCandidateCard {
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
    use wikitool_core::wiki_interview::{BriefLedgerSignals, InterviewOpenItemCounts};

    fn valid_interview_report() -> InterviewValidationReport {
        InterviewValidationReport {
            schema_version: "wiki_interview_validation_v1",
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
                open_items_sidecar: None,
                sections_present: Vec::new(),
                sections_missing: Vec::new(),
                sections_unfilled: Vec::new(),
                open_item_count: 0,
                open_item_counts: InterviewOpenItemCounts {
                    by_kind: BTreeMap::new(),
                    by_status: BTreeMap::new(),
                    ..InterviewOpenItemCounts::default()
                },
                ledger: BriefLedgerSignals::default(),
            },
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn brief_readiness_is_not_ready_when_article_scout_has_blockers() {
        let readiness = article_scout_brief_readiness(
            &CatalogReadiness::ContentReady,
            &["interview brief is invalid: missing required frontmatter".to_string()],
            None,
        );

        assert_eq!(readiness, CatalogReadiness::NotReady);
    }

    #[test]
    fn brief_readiness_keeps_open_items_advisory() {
        let mut brief = valid_interview_report();
        brief.summary.open_item_counts.open = 1;

        let readiness =
            article_scout_brief_readiness(&CatalogReadiness::RetrievalReady, &[], Some(&brief));

        assert_eq!(readiness, CatalogReadiness::RetrievalReady);
    }

    #[test]
    fn brief_readiness_downgrades_negative_evidence() {
        let mut brief = valid_interview_report();
        brief.summary.open_item_counts.negative_evidence = 1;

        let readiness =
            article_scout_brief_readiness(&CatalogReadiness::RetrievalReady, &[], Some(&brief));

        assert_eq!(readiness, CatalogReadiness::ContentReady);
    }
}
