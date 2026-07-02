use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use wikitool_core::filesystem::validate_scoped_path;
use wikitool_core::knowledge_interview::{
    InterviewAuditReport, InterviewInitOptions, InterviewInitReport,
    InterviewOpenItemAppendOptions, InterviewOpenItemAppendReport, InterviewOpenItemListReport,
    InterviewOpenItemUpdateOptions, InterviewOpenItemUpdateReport, InterviewScoutContext,
    InterviewValidationReport, InterviewValidationStatus, append_interview_open_item,
    audit_interview_briefs, create_interview_brief, list_interview_open_items,
    update_interview_open_item, validate_interview_brief,
};

use crate::RuntimeOptions;
use crate::briefs::BriefView;
use crate::cli_support::{OutputFormat, normalize_path, resolve_runtime_paths};

#[derive(Debug, Args)]
pub(crate) struct KnowledgeInterviewArgs {
    #[command(subcommand)]
    command: KnowledgeInterviewSubcommand,
}

#[derive(Debug, Subcommand)]
enum KnowledgeInterviewSubcommand {
    #[command(about = "Create a timestamped knowledge interview brief and sidecars")]
    Init(KnowledgeInterviewInitArgs),
    #[command(about = "Validate a knowledge interview brief and sidecars")]
    Validate(KnowledgeInterviewValidateArgs),
    #[command(about = "Show a knowledge interview brief summary")]
    Show(KnowledgeInterviewShowArgs),
    #[command(about = "Audit all knowledge interview briefs in the local ledger")]
    Audit(KnowledgeInterviewAuditArgs),
    #[command(about = "Append or list structured interview open items")]
    OpenItem(KnowledgeInterviewOpenItemArgs),
}

#[derive(Debug, Args)]
struct KnowledgeInterviewInitArgs {
    #[arg(
        value_name = "TITLE",
        help = "Article title or topic for the interview"
    )]
    title: String,
    #[arg(
        long,
        value_enum,
        default_value_t = KnowledgeInterviewIntentArg::New,
        value_name = "INTENT",
        help = "Interview intent: new|expand|audit|refresh"
    )]
    intent: KnowledgeInterviewIntentArg,
    #[arg(long, value_name = "AGENT", help = "Agent label for brief metadata")]
    agent: Option<String>,
    #[arg(
        long = "no-scout",
        help = "Skip the local-evidence scout (blank brief, generic question agenda)"
    )]
    no_scout: bool,
    #[arg(
        long,
        value_name = "TITLE",
        help = "Existing article title this interview concerns"
    )]
    source_article: Option<String>,
    #[arg(
        long,
        value_name = "PATH",
        help = "Related draft path to record in brief metadata"
    )]
    related_draft: Option<String>,
    #[arg(
        long,
        value_name = "YYYYMMDDTHHMMSSZ",
        help = "UTC ledger timestamp; defaults to current time"
    )]
    timestamp: Option<String>,
    #[arg(long, help = "Overwrite files if the timestamped brief already exists")]
    force: bool,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct KnowledgeInterviewValidateArgs {
    #[arg(value_name = "PATH", help = "Path to .brief.md interview brief")]
    path: PathBuf,
    #[arg(
        long,
        default_value_t = 45,
        value_name = "DAYS",
        help = "Age in days after which a brief is considered stale"
    )]
    stale_days: u64,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct KnowledgeInterviewShowArgs {
    #[arg(value_name = "PATH", help = "Path to .brief.md interview brief")]
    path: PathBuf,
    #[arg(
        long,
        default_value_t = 45,
        value_name = "DAYS",
        help = "Age in days after which a brief is considered stale"
    )]
    stale_days: u64,
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
}

#[derive(Debug, Args)]
struct KnowledgeInterviewAuditArgs {
    #[arg(
        long,
        default_value_t = 45,
        value_name = "DAYS",
        help = "Age in days after which a brief is considered stale"
    )]
    stale_days: u64,
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
}

#[derive(Debug, Args)]
struct KnowledgeInterviewOpenItemArgs {
    #[command(subcommand)]
    command: KnowledgeInterviewOpenItemSubcommand,
}

#[derive(Debug, Subcommand)]
enum KnowledgeInterviewOpenItemSubcommand {
    #[command(about = "Append a structured open item to an interview brief sidecar")]
    Add(KnowledgeInterviewOpenItemAddArgs),
    #[command(about = "List structured open items for an interview brief")]
    List(KnowledgeInterviewOpenItemListArgs),
    #[command(about = "Update an existing open item's status, note, or text")]
    Update(KnowledgeInterviewOpenItemUpdateArgs),
}

#[derive(Debug, Args)]
struct KnowledgeInterviewOpenItemAddArgs {
    #[arg(value_name = "PATH", help = "Path to .brief.md interview brief")]
    path: PathBuf,
    #[arg(long, value_enum, value_name = "KIND", help = "Open item kind")]
    kind: KnowledgeInterviewOpenItemKindArg,
    #[arg(
        long,
        value_enum,
        default_value_t = KnowledgeInterviewOpenItemStatusArg::Open,
        value_name = "STATUS",
        help = "Open item status: open|resolved|rejected|deferred"
    )]
    status: KnowledgeInterviewOpenItemStatusArg,
    #[arg(long, value_name = "TEXT", help = "Open item text")]
    text: String,
    #[arg(long, value_name = "ID", help = "Explicit open item id")]
    item_id: Option<String>,
    #[arg(
        long = "source-lead",
        value_name = "VALUE",
        help = "Source lead associated with this open item; repeatable"
    )]
    source_leads: Vec<String>,
    #[arg(long, value_name = "TEXT", help = "Optional note")]
    notes: Option<String>,
    #[arg(
        long,
        value_name = "YYYYMMDDTHHMMSSZ",
        help = "UTC item timestamp; defaults to current time"
    )]
    timestamp: Option<String>,
    #[arg(long, help = "Do not update brief last_updated/freshness metadata")]
    no_touch_brief: bool,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Args)]
struct KnowledgeInterviewOpenItemListArgs {
    #[arg(value_name = "PATH", help = "Path to .brief.md interview brief")]
    path: PathBuf,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum KnowledgeInterviewIntentArg {
    New,
    Expand,
    Audit,
    Refresh,
}

impl KnowledgeInterviewIntentArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Expand => "expand",
            Self::Audit => "audit",
            Self::Refresh => "refresh",
        }
    }

    fn into_article_start_intent(self) -> wikitool_core::authoring::model::ArticleStartIntent {
        use wikitool_core::authoring::model::ArticleStartIntent;
        match self {
            Self::New => ArticleStartIntent::New,
            Self::Expand => ArticleStartIntent::Expand,
            Self::Audit => ArticleStartIntent::Audit,
            Self::Refresh => ArticleStartIntent::Refresh,
        }
    }
}

// Stored records and JSON output use snake_case kinds (e.g. `rejected_source`).
// clap renders variants in kebab-case; accept the snake_case form as an alias so an
// agent can feed a value read from `open-item list --format json` straight back in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum KnowledgeInterviewOpenItemKindArg {
    #[value(alias = "rejected_source")]
    RejectedSource,
    #[value(alias = "inaccessible_source")]
    InaccessibleSource,
    #[value(alias = "disproven_link")]
    DisprovenLink,
    #[value(alias = "source_wiki_only_template")]
    SourceWikiOnlyTemplate,
    #[value(alias = "rejected_category")]
    RejectedCategory,
    #[value(alias = "scope_unresolved")]
    ScopeUnresolved,
    #[value(alias = "stale_interview")]
    StaleInterview,
    #[value(alias = "privacy_exclusion")]
    PrivacyExclusion,
    #[value(alias = "missing_source")]
    MissingSource,
    #[value(alias = "user_followup_needed")]
    UserFollowupNeeded,
    #[value(alias = "do_not_assert")]
    DoNotAssert,
    Other,
}

impl KnowledgeInterviewOpenItemKindArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::RejectedSource => "rejected_source",
            Self::InaccessibleSource => "inaccessible_source",
            Self::DisprovenLink => "disproven_link",
            Self::SourceWikiOnlyTemplate => "source_wiki_only_template",
            Self::RejectedCategory => "rejected_category",
            Self::ScopeUnresolved => "scope_unresolved",
            Self::StaleInterview => "stale_interview",
            Self::PrivacyExclusion => "privacy_exclusion",
            Self::MissingSource => "missing_source",
            Self::UserFollowupNeeded => "user_followup_needed",
            Self::DoNotAssert => "do_not_assert",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum KnowledgeInterviewOpenItemStatusArg {
    Open,
    Resolved,
    Rejected,
    Deferred,
}

impl KnowledgeInterviewOpenItemStatusArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
            Self::Rejected => "rejected",
            Self::Deferred => "deferred",
        }
    }
}

#[derive(Debug, Args)]
struct KnowledgeInterviewOpenItemUpdateArgs {
    #[arg(value_name = "PATH", help = "Path to .brief.md interview brief")]
    path: PathBuf,
    #[arg(long, value_name = "ID", help = "Open item id to update")]
    item_id: String,
    #[arg(
        long,
        value_enum,
        value_name = "STATUS",
        help = "New status: open|resolved|rejected|deferred"
    )]
    status: Option<KnowledgeInterviewOpenItemStatusArg>,
    #[arg(long, value_name = "TEXT", help = "Replace the open item text")]
    text: Option<String>,
    #[arg(long, value_name = "TEXT", help = "Replace the optional note")]
    notes: Option<String>,
    #[arg(
        long,
        value_name = "YYYYMMDDTHHMMSSZ",
        help = "UTC timestamp; defaults to current time"
    )]
    timestamp: Option<String>,
    #[arg(long, help = "Do not update brief last_updated/freshness metadata")]
    no_touch_brief: bool,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    format: OutputFormat,
}

pub(crate) fn run_knowledge_interview(
    runtime: &RuntimeOptions,
    args: KnowledgeInterviewArgs,
) -> Result<()> {
    match args.command {
        KnowledgeInterviewSubcommand::Init(args) => run_init(runtime, args),
        KnowledgeInterviewSubcommand::Validate(args) => run_validate(runtime, args),
        KnowledgeInterviewSubcommand::Show(args) => run_show(runtime, args),
        KnowledgeInterviewSubcommand::Audit(args) => run_audit(runtime, args),
        KnowledgeInterviewSubcommand::OpenItem(args) => run_open_item(runtime, args),
    }
}

fn run_init(runtime: &RuntimeOptions, args: KnowledgeInterviewInitArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let scout = if args.no_scout {
        None
    } else {
        build_interview_scout(&paths, &args.title, args.intent)?
    };
    let report = create_interview_brief(
        &paths,
        &InterviewInitOptions {
            title: args.title,
            intent: args.intent.as_str().to_string(),
            agent: args.agent,
            source_article: args.source_article,
            related_draft: args.related_draft,
            timestamp: args.timestamp,
            force: args.force,
            scout,
        },
    )?;

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_init_report(&report);
    }
    Ok(())
}

fn run_validate(runtime: &RuntimeOptions, args: KnowledgeInterviewValidateArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let path = resolve_scoped_input_path(&paths, &args.path)?;
    let report = validate_interview_brief(&path, args.stale_days)?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_validation_report("knowledge interview validate", &report);
    }
    Ok(())
}

fn run_show(runtime: &RuntimeOptions, args: KnowledgeInterviewShowArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let path = resolve_scoped_input_path(&paths, &args.path)?;
    let report = validate_interview_brief(&path, args.stale_days)?;
    if args.format.is_json() {
        if args.view.is_full() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&report.summary)?);
        }
    } else {
        print_validation_report("knowledge interview show", &report);
    }
    Ok(())
}

fn run_audit(runtime: &RuntimeOptions, args: KnowledgeInterviewAuditArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let report = audit_interview_briefs(&paths, args.stale_days)?;
    if args.format.is_json() {
        if args.view.is_full() {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&audit_brief(&report))?);
        }
    } else {
        print_audit_report(&report);
    }
    Ok(())
}

fn run_open_item(runtime: &RuntimeOptions, args: KnowledgeInterviewOpenItemArgs) -> Result<()> {
    match args.command {
        KnowledgeInterviewOpenItemSubcommand::Add(args) => run_open_item_add(runtime, args),
        KnowledgeInterviewOpenItemSubcommand::List(args) => run_open_item_list(runtime, args),
        KnowledgeInterviewOpenItemSubcommand::Update(args) => run_open_item_update(runtime, args),
    }
}

fn run_open_item_add(
    runtime: &RuntimeOptions,
    args: KnowledgeInterviewOpenItemAddArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let path = resolve_scoped_input_path(&paths, &args.path)?;
    let report = append_interview_open_item(
        &path,
        &InterviewOpenItemAppendOptions {
            kind: args.kind.as_str().to_string(),
            status: args.status.as_str().to_string(),
            text: args.text,
            item_id: args.item_id,
            source_leads: args.source_leads,
            notes: args.notes,
            timestamp: args.timestamp,
            touch_brief: !args.no_touch_brief,
        },
    )?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_open_item_append_report(&report);
    }
    Ok(())
}

fn run_open_item_list(
    runtime: &RuntimeOptions,
    args: KnowledgeInterviewOpenItemListArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let path = resolve_scoped_input_path(&paths, &args.path)?;
    let report = list_interview_open_items(&path)?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_open_item_list_report(&report);
    }
    Ok(())
}

fn run_open_item_update(
    runtime: &RuntimeOptions,
    args: KnowledgeInterviewOpenItemUpdateArgs,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let path = resolve_scoped_input_path(&paths, &args.path)?;
    let report = update_interview_open_item(
        &path,
        &InterviewOpenItemUpdateOptions {
            item_id: args.item_id,
            status: args.status.map(|status| status.as_str().to_string()),
            notes: args.notes,
            text: args.text,
            timestamp: args.timestamp,
            touch_brief: !args.no_touch_brief,
        },
    )?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_open_item_update_report(&report);
    }
    Ok(())
}

fn print_open_item_update_report(report: &InterviewOpenItemUpdateReport) {
    println!("knowledge interview open-item update");
    println!("brief_path: {}", normalize_path(&report.brief_path));
    println!(
        "open_items_path: {}",
        normalize_path(&report.open_items_path)
    );
    println!(
        "item_id: {}",
        report.item.item_id.as_deref().unwrap_or("<missing>")
    );
    println!(
        "status: {}",
        report.item.status.as_deref().unwrap_or("<missing>")
    );
    println!("touched_brief: {}", yes_no(report.touched_brief));
}

fn resolve_scoped_input_path(
    paths: &wikitool_core::runtime::ResolvedPaths,
    path: &Path,
) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        paths.project_root.join(path)
    };
    validate_scoped_path(paths, &absolute)?;
    Ok(absolute)
}

/// Run the local authoring scout and reduce it to the interview's evidence
/// snapshot. The interview must stay usable on a cold runtime, so a missing
/// index degrades to `None` (blank brief + generic agenda) rather than failing.
fn build_interview_scout(
    paths: &wikitool_core::runtime::ResolvedPaths,
    title: &str,
    intent: KnowledgeInterviewIntentArg,
) -> Result<Option<InterviewScoutContext>> {
    use wikitool_core::authoring::article_start::build_article_start;
    use wikitool_core::knowledge::authoring::{
        AuthoringKnowledgePack, AuthoringKnowledgePackOptions, build_authoring_knowledge_pack,
    };
    use wikitool_core::profile::load_or_build_remilia_profile_overlay;

    let pack = build_authoring_knowledge_pack(
        paths,
        Some(title),
        None,
        &AuthoringKnowledgePackOptions::default(),
    )?;
    let AuthoringKnowledgePack::Found(report) = pack else {
        return Ok(None);
    };
    let overlay = load_or_build_remilia_profile_overlay(paths)?;
    let article_start = build_article_start(&report, &overlay, intent.into_article_start_intent());

    let local_state = serde_json::to_value(&article_start.local_state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    let integration = &article_start.local_integration;
    Ok(Some(InterviewScoutContext {
        local_state,
        comparable_pages: integration.comparable_pages.clone(),
        closest_comparable_title: integration
            .closest_comparable_outline
            .as_ref()
            .map(|outline| outline.title.clone()),
        closest_comparable_outline: integration
            .closest_comparable_outline
            .as_ref()
            .map(|outline| outline.ordered_headings.clone())
            .unwrap_or_default(),
        infobox_candidates: integration
            .available_infoboxes
            .iter()
            .map(|entry| entry.template_title.clone())
            .take(4)
            .collect(),
        categories_seen: integration
            .categories_seen
            .iter()
            .map(|entry| entry.category_title.clone())
            .take(6)
            .collect(),
        citation_template_families: {
            let mut families = article_start
                .subject_research
                .citation_template_families
                .clone();
            families.dedup();
            families.sort();
            families.dedup();
            families
        },
        missing_query_terms: article_start.evidence_profile.missing_query_terms.clone(),
    }))
}

fn print_init_report(report: &InterviewInitReport) {
    println!("knowledge interview init");
    println!("title: {}", report.title);
    println!("title_key: {}", report.title_key);
    println!("intent: {}", report.intent);
    println!("timestamp: {}", report.timestamp);
    println!("brief_path: {}", normalize_path(&report.brief_path));
    println!(
        "open_items_path: {}",
        normalize_path(&report.open_items_path)
    );
    println!("wrote_brief: {}", yes_no(report.wrote_brief));
    println!("wrote_open_items: {}", yes_no(report.wrote_open_items));
    println!("scout_included: {}", yes_no(report.scout_included));
    for area in &report.question_agenda {
        println!("question_area: {}", area.area);
        println!("  suggested: {}", area.suggested_question);
        println!("  why: {}", area.why);
    }
    for step in &report.next_steps {
        println!("next_step: {step}");
    }
}

fn print_validation_report(label: &str, report: &InterviewValidationReport) {
    println!("{label}");
    println!("path: {}", normalize_path(&report.path));
    println!("status: {}", validation_status(report.status.clone()));
    if let Some(title) = &report.summary.title {
        println!("title: {title}");
    }
    if let Some(intent) = &report.summary.intent {
        println!("intent: {intent}");
    }
    println!("computed_freshness: {}", report.summary.computed_freshness);
    println!(
        "sections_present: {}",
        report.summary.sections_present.len()
    );
    println!(
        "sections_missing: {}",
        report.summary.sections_missing.join(", ")
    );
    println!("open_items: {}", report.summary.open_item_count);
    for error in &report.errors {
        println!("error: {error}");
    }
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
}

fn print_audit_report(report: &InterviewAuditReport) {
    println!("knowledge interview audit");
    println!(
        "interviews_root: {}",
        normalize_path(&report.interviews_root)
    );
    println!("total_briefs: {}", report.total_briefs);
    println!("valid: {}", report.valid);
    println!("warning: {}", report.warning);
    println!("invalid: {}", report.invalid);
    println!("stale: {}", report.stale);
    println!("open_items: {}", report.open_items);
    println!("negative_evidence: {}", report.negative_evidence);
    for brief in &report.briefs {
        println!(
            "brief: status={} title={} path={}",
            validation_status(brief.status.clone()),
            brief.summary.title.as_deref().unwrap_or("<missing>"),
            normalize_path(&brief.path)
        );
    }
}

fn print_open_item_append_report(report: &InterviewOpenItemAppendReport) {
    println!("knowledge interview open-item add");
    println!("brief_path: {}", normalize_path(&report.brief_path));
    println!(
        "open_items_path: {}",
        normalize_path(&report.open_items_path)
    );
    println!(
        "item_id: {}",
        report.item.item_id.as_deref().unwrap_or("<missing>")
    );
    println!(
        "kind: {}",
        report.item.kind.as_deref().unwrap_or("<missing>")
    );
    println!(
        "status: {}",
        report.item.status.as_deref().unwrap_or("<missing>")
    );
    println!("touched_brief: {}", yes_no(report.touched_brief));
}

fn print_open_item_list_report(report: &InterviewOpenItemListReport) {
    println!("knowledge interview open-item list");
    println!("brief_path: {}", normalize_path(&report.brief_path));
    println!(
        "open_items_path: {}",
        normalize_path(&report.open_items_path)
    );
    println!("status: {}", validation_status(report.status.clone()));
    println!("open_items: {}", report.counts.total);
    println!("negative_evidence: {}", report.counts.negative_evidence);
    for item in &report.items {
        println!(
            "item: id={} kind={} status={} text={}",
            item.item_id.as_deref().unwrap_or("<missing>"),
            item.kind.as_deref().unwrap_or("<missing>"),
            item.status.as_deref().unwrap_or("<missing>"),
            item.text.as_deref().unwrap_or("<missing>")
        );
    }
    for error in &report.errors {
        println!("error: {error}");
    }
    for warning in &report.warnings {
        println!("warning: {warning}");
    }
}

#[derive(serde::Serialize)]
struct AuditBrief<'a> {
    schema_version: &'static str,
    total_briefs: usize,
    valid: usize,
    warning: usize,
    invalid: usize,
    stale: usize,
    open_items: usize,
    negative_evidence: usize,
    briefs: Vec<AuditBriefEntry<'a>>,
}

#[derive(serde::Serialize)]
struct AuditBriefEntry<'a> {
    path: &'a Path,
    status: &'a InterviewValidationStatus,
    title: Option<&'a str>,
    intent: Option<&'a str>,
    computed_freshness: &'a str,
    open_items: usize,
    negative_evidence: usize,
    errors: &'a [String],
    warnings: &'a [String],
}

fn audit_brief(report: &InterviewAuditReport) -> AuditBrief<'_> {
    AuditBrief {
        schema_version: report.schema_version,
        total_briefs: report.total_briefs,
        valid: report.valid,
        warning: report.warning,
        invalid: report.invalid,
        stale: report.stale,
        open_items: report.open_items,
        negative_evidence: report.negative_evidence,
        briefs: report
            .briefs
            .iter()
            .map(|brief| AuditBriefEntry {
                path: &brief.path,
                status: &brief.status,
                title: brief.summary.title.as_deref(),
                intent: brief.summary.intent.as_deref(),
                computed_freshness: &brief.summary.computed_freshness,
                open_items: brief.summary.open_item_counts.total,
                negative_evidence: brief.summary.open_item_counts.negative_evidence,
                errors: &brief.errors,
                warnings: &brief.warnings,
            })
            .collect(),
    }
}

fn validation_status(status: InterviewValidationStatus) -> &'static str {
    match status {
        InterviewValidationStatus::Valid => "valid",
        InterviewValidationStatus::Warning => "warning",
        InterviewValidationStatus::Invalid => "invalid",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
