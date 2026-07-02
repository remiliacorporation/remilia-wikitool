use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::runtime::ResolvedPaths;

pub const INTERVIEW_SCHEMA_VERSION: u32 = 1;
pub const INTERVIEW_DOC_KIND: &str = "knowledge_interview_brief";
pub const OPEN_ITEM_SCHEMA_VERSION: &str = "knowledge_interview_open_item_v1";

const REQUIRED_BRIEF_SECTIONS: &[&str] = &[
    "Article Object",
    "Scope",
    "Initial Materials",
    "User-Framed Summary",
    "Interview Transcript and Context",
    "Chronology",
    "Entities and Relationships",
    "Editorial Framing",
    "Research Plan",
    "Interviewer Critic Notes",
    "Draft Plan",
];

const ALLOWED_INTENTS: &[&str] = &["new", "expand", "audit", "refresh"];
const ALLOWED_FRESHNESS: &[&str] = &["fresh", "stale", "unknown"];
const ALLOWED_OPEN_ITEM_KINDS: &[&str] = &[
    "rejected_source",
    "inaccessible_source",
    "disproven_link",
    "source_wiki_only_template",
    "rejected_category",
    "scope_unresolved",
    "stale_interview",
    "privacy_exclusion",
    "missing_source",
    "user_followup_needed",
    "do_not_assert",
    "other",
];
const NEGATIVE_EVIDENCE_KINDS: &[&str] = &[
    "rejected_source",
    "inaccessible_source",
    "disproven_link",
    "source_wiki_only_template",
    "rejected_category",
    "privacy_exclusion",
    "do_not_assert",
];
const ALLOWED_OPEN_ITEM_STATUSES: &[&str] = &["open", "resolved", "rejected", "deferred"];
#[derive(Debug, Clone)]
pub struct InterviewInitOptions {
    pub title: String,
    pub intent: String,
    pub agent: Option<String>,
    pub source_article: Option<String>,
    pub related_draft: Option<String>,
    pub timestamp: Option<String>,
    pub force: bool,
    /// Local-evidence snapshot from the authoring scout; when present it is
    /// written into the brief and drives the question agenda.
    pub scout: Option<InterviewScoutContext>,
}

/// What the local index already knows about the subject, captured at init so
/// the interviewer and the human share ground truth from the first question.
#[derive(Debug, Clone, Default, Serialize)]
pub struct InterviewScoutContext {
    /// Serialized local existence state, e.g. `likely_missing` or `exact_page_exists`.
    pub local_state: String,
    pub comparable_pages: Vec<String>,
    pub closest_comparable_title: Option<String>,
    pub closest_comparable_outline: Vec<String>,
    pub infobox_candidates: Vec<String>,
    pub categories_seen: Vec<String>,
    pub citation_template_families: Vec<String>,
    pub missing_query_terms: Vec<String>,
}

/// An evidence-grounded area the interviewer should consider raising, with a
/// suggested phrasing and the evidence that motivates it. These are prompts to
/// adapt in conversation, not a script: the interview stays open-ended, and the
/// agenda exists so questions start from what the wiki actually knows.
#[derive(Debug, Clone, Serialize)]
pub struct InterviewQuestionArea {
    pub area: String,
    pub suggested_question: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterviewInitReport {
    pub schema_version: &'static str,
    pub title: String,
    pub title_key: String,
    pub intent: String,
    pub timestamp: String,
    pub brief_path: PathBuf,
    pub open_items_path: PathBuf,
    pub wrote_brief: bool,
    pub wrote_open_items: bool,
    pub next_steps: Vec<String>,
    pub scout_included: bool,
    pub question_agenda: Vec<InterviewQuestionArea>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterviewValidationReport {
    pub schema_version: &'static str,
    pub path: PathBuf,
    pub status: InterviewValidationStatus,
    pub summary: InterviewBriefSummary,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterviewValidationStatus {
    Valid,
    Warning,
    Invalid,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterviewBriefSummary {
    pub doc_id: Option<String>,
    pub title: Option<String>,
    pub title_key: Option<String>,
    pub intent: Option<String>,
    pub created_at: Option<String>,
    pub last_updated: Option<String>,
    pub freshness_state: Option<String>,
    pub computed_freshness: String,
    pub agent: Option<String>,
    pub open_items_sidecar: Option<String>,
    pub sections_present: Vec<String>,
    pub sections_missing: Vec<String>,
    /// Sections whose body is still template scaffolding; advisory, not gating.
    pub sections_unfilled: Vec<String>,
    pub open_item_count: usize,
    pub open_item_counts: InterviewOpenItemCounts,
    pub draft_plan: BriefDraftPlan,
    /// Structured handoff signals parsed from the brief body beyond the Draft
    /// Plan, so downstream commands receive the interview's framing and leads
    /// instead of validating them and throwing them away.
    pub handoff: BriefHandoffSignals,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct InterviewOpenItemCounts {
    pub total: usize,
    pub open: usize,
    pub resolved: usize,
    pub rejected: usize,
    pub deferred: usize,
    pub unknown_status: usize,
    pub unknown_kind: usize,
    pub negative_evidence: usize,
    pub by_kind: BTreeMap<String, usize>,
    pub by_status: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterviewAuditReport {
    pub schema_version: &'static str,
    pub interviews_root: PathBuf,
    pub total_briefs: usize,
    pub valid: usize,
    pub warning: usize,
    pub invalid: usize,
    pub stale: usize,
    pub open_items: usize,
    pub negative_evidence: usize,
    pub briefs: Vec<InterviewValidationReport>,
}

#[derive(Debug, Clone)]
pub struct InterviewOpenItemAppendOptions {
    pub kind: String,
    pub status: String,
    pub text: String,
    pub item_id: Option<String>,
    pub source_leads: Vec<String>,
    pub notes: Option<String>,
    pub timestamp: Option<String>,
    pub touch_brief: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterviewOpenItemAppendReport {
    pub schema_version: &'static str,
    pub brief_path: PathBuf,
    pub open_items_path: PathBuf,
    pub item: InterviewOpenItemRecord,
    pub touched_brief: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterviewOpenItemListReport {
    pub schema_version: &'static str,
    pub brief_path: PathBuf,
    pub open_items_path: PathBuf,
    pub status: InterviewValidationStatus,
    pub counts: InterviewOpenItemCounts,
    pub items: Vec<InterviewOpenItemRecord>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterviewOpenItemRecord {
    #[serde(default)]
    pub schema_version: Option<String>,
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub source_leads: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedBrief {
    metadata: BriefFrontmatter,
    sections_present: Vec<String>,
    sections_missing: Vec<String>,
    sections_unfilled: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct BriefFrontmatter {
    schema_version: Option<u32>,
    doc_kind: Option<String>,
    doc_id: Option<String>,
    title: Option<String>,
    title_key: Option<String>,
    intent: Option<String>,
    created_at: Option<String>,
    last_updated: Option<String>,
    freshness_state: Option<String>,
    agent: Option<String>,
    open_items_sidecar: Option<String>,
}

struct OpenItemsValidationResult {
    path: PathBuf,
    items: Vec<InterviewOpenItemRecord>,
    counts: InterviewOpenItemCounts,
}

pub fn interviews_root(paths: &ResolvedPaths) -> PathBuf {
    paths.state_dir.join("interviews")
}

pub fn title_key_for_interview(title: &str) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in title.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
            last_was_separator = false;
        } else if !last_was_separator && !output.is_empty() {
            output.push('_');
            last_was_separator = true;
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    if output.is_empty() {
        "Untitled".to_string()
    } else {
        output
    }
}

pub fn create_interview_brief(
    paths: &ResolvedPaths,
    options: &InterviewInitOptions,
) -> Result<InterviewInitReport> {
    validate_intent(&options.intent)?;
    let title = normalize_title(&options.title)?;
    let title_key = title_key_for_interview(&title);
    let timestamp = match options.timestamp.as_deref() {
        Some(timestamp) => validate_compact_timestamp(timestamp)?.to_string(),
        None => current_compact_timestamp()?,
    };
    let created_at = compact_to_rfc3339(&timestamp)?;
    let doc_id = format!("KIB-{}-{}", title_key.to_ascii_uppercase(), timestamp);
    let dir = interviews_root(paths).join(&title_key);
    let brief_path = dir.join(format!("{timestamp}.brief.md"));
    let open_items_name = format!("{timestamp}.open_items.jsonl");
    let open_items_path = dir.join(&open_items_name);

    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let brief = render_brief_template(&BriefTemplateInput {
        doc_id: &doc_id,
        title: &title,
        title_key: &title_key,
        intent: &options.intent,
        created_at: &created_at,
        agent: options.agent.as_deref().unwrap_or("other"),
        source_article: options.source_article.as_deref(),
        related_draft: options.related_draft.as_deref(),
        open_items_name: &open_items_name,
    });

    let brief = match &options.scout {
        Some(scout) => insert_scout_section(&brief, scout),
        None => brief,
    };
    let question_agenda = build_question_agenda(&options.intent, options.scout.as_ref());

    let wrote_brief = write_if_allowed(&brief_path, &brief, options.force)?;
    let wrote_open_items = write_if_allowed(&open_items_path, "", options.force)?;

    Ok(InterviewInitReport {
        schema_version: "knowledge_interview_init_v1",
        title,
        title_key,
        intent: options.intent.clone(),
        timestamp,
        brief_path,
        open_items_path,
        wrote_brief,
        wrote_open_items,
        scout_included: options.scout.is_some(),
        question_agenda,
        next_steps: vec![
            "Open the interview from the question agenda, adapting it to the conversation."
                .to_string(),
            "Fill the brief from the user interview; keep user assertions as leads.".to_string(),
            "Run `wikitool knowledge interview validate PATH --format json` before drafting."
                .to_string(),
        ],
    })
}

/// Render the tool-written Scout Context section and place it directly after the
/// brief title, so both parties see the same local-evidence baseline. The section
/// is informational; validation never requires it and never checks its fill state.
fn insert_scout_section(brief: &str, scout: &InterviewScoutContext) -> String {
    let mut section = String::from("\n## Scout Context\n\n");
    section.push_str(
        "Tool-written local-evidence snapshot from `knowledge article-start`; do not edit.\n\n",
    );
    section.push_str(&format!("Local state: {}\n", scout.local_state));
    if !scout.comparable_pages.is_empty() {
        section.push_str(&format!(
            "Comparable pages: {}\n",
            scout.comparable_pages.join("; ")
        ));
    }
    if let Some(title) = &scout.closest_comparable_title
        && !scout.closest_comparable_outline.is_empty()
    {
        section.push_str(&format!(
            "Closest comparable outline ({}): {}\n",
            title,
            scout.closest_comparable_outline.join(" > ")
        ));
    }
    if !scout.infobox_candidates.is_empty() {
        section.push_str(&format!(
            "Infobox candidates: {}\n",
            scout.infobox_candidates.join("; ")
        ));
    }
    if !scout.categories_seen.is_empty() {
        section.push_str(&format!(
            "Categories seen on comparables: {}\n",
            scout.categories_seen.join("; ")
        ));
    }
    if !scout.citation_template_families.is_empty() {
        section.push_str(&format!(
            "Citation patterns seen locally: {}\n",
            scout.citation_template_families.join("; ")
        ));
    }
    if !scout.missing_query_terms.is_empty() {
        section.push_str(&format!(
            "Query terms with no local evidence: {}\n",
            scout.missing_query_terms.join("; ")
        ));
    }
    match brief.find("\n## Article Object") {
        Some(index) => {
            let mut out = String::with_capacity(brief.len() + section.len());
            out.push_str(&brief[..index]);
            out.push_str(&section);
            out.push_str(&brief[index..]);
            out
        }
        None => format!("{brief}{section}"),
    }
}

/// Derive evidence-grounded question areas for the interviewer. The agenda is
/// adaptive by design: the opener is always the freeform monologue invitation,
/// followed only by areas the local evidence actually raises, and it closes on
/// boundaries (what the article must not assert). Without a scout the agenda
/// still gives the interviewer a fluid opening rather than an empty form.
fn build_question_agenda(
    intent: &str,
    scout: Option<&InterviewScoutContext>,
) -> Vec<InterviewQuestionArea> {
    let mut agenda = Vec::new();
    agenda.push(InterviewQuestionArea {
        area: "subject in the person's own words".to_string(),
        suggested_question: "Before I narrow this into article sections, tell me what this subject is in your own words: why it matters, where it came from, what people misunderstand, what sources or artifacts I should look at, and what you would be disappointed to see omitted.".to_string(),
        why: "The freeform monologue sets intent, scope, and angle before any structure is imposed.".to_string(),
    });

    if let Some(scout) = scout {
        match scout.local_state.as_str() {
            "exact_page_exists" | "redirect_exists" => {
                if intent == "new" {
                    agenda.push(InterviewQuestionArea {
                        area: "intent check: this page already exists".to_string(),
                        suggested_question: "The wiki already has a page (or redirect) at this title. Should this be an expansion or refresh of the existing page rather than a new article - or a genuinely different subject that needs its own title?".to_string(),
                        why: format!("Local state is {} but the interview intent is `new`; resolving the mismatch first prevents drafting a duplicate.", scout.local_state),
                    });
                } else {
                    agenda.push(InterviewQuestionArea {
                        area: "what the current page gets wrong or misses".to_string(),
                        suggested_question: "The wiki already has this page. What is wrong, missing, or under-emphasized in it as it stands?".to_string(),
                        why: format!("Local state is {}; the interview should target the delta, not restate coverage.", scout.local_state),
                    });
                }
            }
            "likely_missing" | "linked_but_missing" => {
                agenda.push(InterviewQuestionArea {
                    area: "primary knowledge the wiki lacks".to_string(),
                    suggested_question: "The wiki has no coverage of this subject yet, so your knowledge is the starting record. What do you know firsthand, and what should I verify independently?".to_string(),
                    why: "Local index has no page for this subject; everything starts from the human's knowledge and leads.".to_string(),
                });
            }
            _ => {}
        }
        if let Some(title) = &scout.closest_comparable_title {
            let outline = if scout.closest_comparable_outline.is_empty() {
                String::new()
            } else {
                format!(" ({})", scout.closest_comparable_outline.join(" > "))
            };
            agenda.push(InterviewQuestionArea {
                area: "article shape".to_string(),
                suggested_question: format!(
                    "The closest existing page is {title}{outline}. Should this article follow a similar shape, or is the subject a different kind of thing?"
                ),
                why: "Comparable-page structure is the strongest local signal for the section plan; confirming or rejecting it early shapes every later question.".to_string(),
            });
        }
        for term in scout.missing_query_terms.iter().take(3) {
            agenda.push(InterviewQuestionArea {
                area: format!("unknown term: {term}"),
                suggested_question: format!(
                    "The wiki has no local evidence for \"{term}\". Is it central to this subject, and what is it?"
                ),
                why: "The topic query hit nothing locally for this term; it is either central and undocumented or incidental.".to_string(),
            });
        }
        if scout.citation_template_families.is_empty() {
            agenda.push(InterviewQuestionArea {
                area: "source leads".to_string(),
                suggested_question: "No citation patterns exist locally for this subject. What primary sources, posts, artifacts, or records should anchor it?".to_string(),
                why: "Comparable pages surfaced no citation templates, so the source strategy must come from the interview.".to_string(),
            });
        }
        if scout.infobox_candidates.len() > 1 {
            agenda.push(InterviewQuestionArea {
                area: "subject kind".to_string(),
                suggested_question: format!(
                    "Local evidence suggests more than one page kind ({}). Which fits, or is it none of these?",
                    scout.infobox_candidates.join("; ")
                ),
                why: "The infobox choice pins the page kind and steers required parameters.".to_string(),
            });
        }
    }

    agenda.push(InterviewQuestionArea {
        area: "boundaries".to_string(),
        suggested_question: "Is there anything the article must not say, overstate, or attribute — privacy limits, contested claims, or readings that need a source before they can appear?".to_string(),
        why: "Do-not-assert and privacy boundaries belong in open items before drafting, not discovered after.".to_string(),
    });
    agenda
}

pub fn validate_interview_brief(path: &Path, stale_days: u64) -> Result<InterviewValidationReport> {
    let absolute = path.to_path_buf();
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read interview brief {}", path.display()))?;
    let parsed = parse_brief(&content)?;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    validate_frontmatter(&parsed.metadata, &mut errors, &mut warnings);

    for missing in &parsed.sections_missing {
        warnings.push(format!("missing recommended section `{missing}`"));
    }
    for unfilled in &parsed.sections_unfilled {
        if CORE_SUBSTANCE_SECTIONS
            .iter()
            .any(|core| core.eq_ignore_ascii_case(unfilled))
        {
            warnings.push(format!(
                "section `{unfilled}` is still at its template state; fill it or record why it stays empty"
            ));
        }
    }

    let open_items_report =
        validate_open_items_sidecar(path, &parsed.metadata, &mut errors, &mut warnings)?;
    let computed_freshness = compute_freshness(parsed.metadata.last_updated.as_deref(), stale_days);
    if computed_freshness == "stale" {
        warnings.push(format!(
            "brief last_updated is older than the stale threshold ({stale_days} days)"
        ));
    }

    let summary = InterviewBriefSummary {
        doc_id: parsed.metadata.doc_id.clone(),
        title: parsed.metadata.title.clone(),
        title_key: parsed.metadata.title_key.clone(),
        intent: parsed.metadata.intent.clone(),
        created_at: parsed.metadata.created_at.clone(),
        last_updated: parsed.metadata.last_updated.clone(),
        freshness_state: parsed.metadata.freshness_state.clone(),
        computed_freshness,
        agent: parsed.metadata.agent.clone(),
        open_items_sidecar: parsed.metadata.open_items_sidecar.clone(),
        sections_present: parsed.sections_present,
        sections_missing: parsed.sections_missing,
        sections_unfilled: parsed.sections_unfilled,
        open_item_count: open_items_report.counts.total,
        open_item_counts: open_items_report.counts,
        draft_plan: parse_brief_draft_plan(&content),
        handoff: parse_brief_handoff_signals(&content),
    };

    let status = if !errors.is_empty() {
        InterviewValidationStatus::Invalid
    } else if !warnings.is_empty() {
        InterviewValidationStatus::Warning
    } else {
        InterviewValidationStatus::Valid
    };

    Ok(InterviewValidationReport {
        schema_version: "knowledge_interview_validation_v1",
        path: absolute,
        status,
        summary,
        errors,
        warnings,
    })
}

pub fn audit_interview_briefs(
    paths: &ResolvedPaths,
    stale_days: u64,
) -> Result<InterviewAuditReport> {
    let root = interviews_root(paths);
    let mut reports = Vec::new();
    if root.exists() {
        for entry in WalkDir::new(&root).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let is_brief = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.ends_with(".brief.md"))
                .unwrap_or(false);
            if is_brief {
                // Audit is a ledger receipt over many briefs written across disparate
                // sessions and agents; one unparseable file must not blind the whole
                // sweep. Represent read/parse failures as an invalid entry and continue.
                let report = match validate_interview_brief(path, stale_days) {
                    Ok(report) => report,
                    Err(error) => unreadable_brief_report(path, &error),
                };
                reports.push(report);
            }
        }
    }
    reports.sort_by(|left, right| left.path.cmp(&right.path));

    let valid = reports
        .iter()
        .filter(|report| report.status == InterviewValidationStatus::Valid)
        .count();
    let warning = reports
        .iter()
        .filter(|report| report.status == InterviewValidationStatus::Warning)
        .count();
    let invalid = reports
        .iter()
        .filter(|report| report.status == InterviewValidationStatus::Invalid)
        .count();
    let stale = reports
        .iter()
        .filter(|report| report.summary.computed_freshness == "stale")
        .count();
    let open_items = reports
        .iter()
        .map(|report| report.summary.open_item_counts.total)
        .sum();
    let negative_evidence = reports
        .iter()
        .map(|report| report.summary.open_item_counts.negative_evidence)
        .sum();

    Ok(InterviewAuditReport {
        schema_version: "knowledge_interview_audit_v1",
        interviews_root: root,
        total_briefs: reports.len(),
        valid,
        warning,
        invalid,
        stale,
        open_items,
        negative_evidence,
        briefs: reports,
    })
}

fn unreadable_brief_report(path: &Path, error: &anyhow::Error) -> InterviewValidationReport {
    InterviewValidationReport {
        schema_version: "knowledge_interview_validation_v1",
        path: path.to_path_buf(),
        status: InterviewValidationStatus::Invalid,
        summary: InterviewBriefSummary {
            doc_id: None,
            title: None,
            title_key: None,
            intent: None,
            created_at: None,
            last_updated: None,
            freshness_state: None,
            computed_freshness: "unknown".to_string(),
            agent: None,
            open_items_sidecar: None,
            sections_present: Vec::new(),
            sections_missing: Vec::new(),
            sections_unfilled: Vec::new(),
            open_item_count: 0,
            open_item_counts: InterviewOpenItemCounts::default(),
            draft_plan: BriefDraftPlan::default(),
            handoff: BriefHandoffSignals::default(),
        },
        errors: vec![format!("brief could not be parsed: {error}")],
        warnings: Vec::new(),
    }
}

pub fn append_interview_open_item(
    brief_path: &Path,
    options: &InterviewOpenItemAppendOptions,
) -> Result<InterviewOpenItemAppendReport> {
    validate_open_item_kind(&options.kind)?;
    validate_open_item_status(&options.status)?;
    let text = normalize_required_text("open item text", &options.text)?;
    let content = fs::read_to_string(brief_path)
        .with_context(|| format!("failed to read interview brief {}", brief_path.display()))?;
    let parsed = parse_brief(&content)?;
    let sidecar_path = open_items_sidecar_path(brief_path, &parsed.metadata)?;
    if !sidecar_path.is_file() {
        bail!("open items sidecar missing: {}", sidecar_path.display());
    }
    let existing = read_open_items_sidecar(&sidecar_path)?;
    if !existing.counts_unknown_free() {
        bail!(
            "open items sidecar has invalid structured records; run `wikitool knowledge interview open-item list {}`",
            brief_path.display()
        );
    }
    let timestamp = match options.timestamp.as_deref() {
        Some(timestamp) => validate_compact_timestamp(timestamp)?.to_string(),
        None => current_compact_timestamp()?,
    };
    let now = compact_to_rfc3339(&timestamp)?;
    let item_id = match options.item_id.as_deref() {
        Some(item_id) => normalize_required_text("open item id", item_id)?,
        None => next_open_item_id(&timestamp, &existing.items),
    };
    if existing
        .items
        .iter()
        .filter_map(|item| item.item_id.as_deref())
        .any(|existing_id| existing_id == item_id)
    {
        bail!("duplicate open item_id `{item_id}`");
    }
    let item = InterviewOpenItemRecord {
        schema_version: Some(OPEN_ITEM_SCHEMA_VERSION.to_string()),
        item_id: Some(item_id),
        kind: Some(options.kind.clone()),
        status: Some(options.status.clone()),
        text: Some(text),
        created_at: Some(now.clone()),
        updated_at: Some(now.clone()),
        source_leads: normalize_list(&options.source_leads),
        notes: options
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|notes| !notes.is_empty())
            .map(ToOwned::to_owned),
    };
    let line = serde_json::to_string(&item)?;
    append_jsonl_line(&sidecar_path, &line)?;
    let touched_brief = if options.touch_brief {
        touch_brief_freshness(brief_path, &content, &now)?
    } else {
        false
    };
    Ok(InterviewOpenItemAppendReport {
        schema_version: "knowledge_interview_open_item_append_v1",
        brief_path: brief_path.to_path_buf(),
        open_items_path: sidecar_path,
        item,
        touched_brief,
    })
}

pub fn list_interview_open_items(brief_path: &Path) -> Result<InterviewOpenItemListReport> {
    let content = fs::read_to_string(brief_path)
        .with_context(|| format!("failed to read interview brief {}", brief_path.display()))?;
    let parsed = parse_brief(&content)?;
    let sidecar_path = open_items_sidecar_path(brief_path, &parsed.metadata)?;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let result = collect_open_items(&sidecar_path, &mut errors, &mut warnings)?;
    let status = if !errors.is_empty() {
        InterviewValidationStatus::Invalid
    } else if !warnings.is_empty() {
        InterviewValidationStatus::Warning
    } else {
        InterviewValidationStatus::Valid
    };
    Ok(InterviewOpenItemListReport {
        schema_version: "knowledge_interview_open_item_list_v1",
        brief_path: brief_path.to_path_buf(),
        open_items_path: result.path,
        status,
        counts: result.counts,
        items: result.items,
        errors,
        warnings,
    })
}

#[derive(Debug, Clone)]
pub struct InterviewOpenItemUpdateOptions {
    pub item_id: String,
    pub status: Option<String>,
    pub notes: Option<String>,
    pub text: Option<String>,
    pub timestamp: Option<String>,
    pub touch_brief: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InterviewOpenItemUpdateReport {
    pub schema_version: &'static str,
    pub brief_path: PathBuf,
    pub open_items_path: PathBuf,
    pub item: InterviewOpenItemRecord,
    pub touched_brief: bool,
}

/// Transition an existing open item's status (and optionally its note or text)
/// in place, rewriting the JSONL sidecar. This is the resolve/defer lane so a
/// later session does not have to hand-edit the ledger.
pub fn update_interview_open_item(
    brief_path: &Path,
    options: &InterviewOpenItemUpdateOptions,
) -> Result<InterviewOpenItemUpdateReport> {
    let item_id = normalize_required_text("open item id", &options.item_id)?;
    if let Some(status) = options.status.as_deref() {
        validate_open_item_status(status)?;
    }
    if options.status.is_none() && options.notes.is_none() && options.text.is_none() {
        bail!("open-item update requires at least one of --status, --notes, or --text");
    }
    let content = fs::read_to_string(brief_path)
        .with_context(|| format!("failed to read interview brief {}", brief_path.display()))?;
    let parsed = parse_brief(&content)?;
    let sidecar_path = open_items_sidecar_path(brief_path, &parsed.metadata)?;
    if !sidecar_path.is_file() {
        bail!("open items sidecar missing: {}", sidecar_path.display());
    }
    let existing = read_open_items_sidecar(&sidecar_path)?;
    let timestamp = match options.timestamp.as_deref() {
        Some(timestamp) => validate_compact_timestamp(timestamp)?.to_string(),
        None => current_compact_timestamp()?,
    };
    let now = compact_to_rfc3339(&timestamp)?;
    let mut items = existing.items;
    let mut updated: Option<InterviewOpenItemRecord> = None;
    for item in &mut items {
        if item.item_id.as_deref() == Some(item_id.as_str()) {
            if let Some(status) = options.status.as_deref() {
                item.status = Some(status.to_string());
            }
            if let Some(notes) = normalize_optional(options.notes.as_deref()) {
                item.notes = Some(notes);
            }
            if let Some(text) = normalize_optional(options.text.as_deref()) {
                item.text = Some(text);
            }
            item.updated_at = Some(now.clone());
            updated = Some(item.clone());
            break;
        }
    }
    let Some(item) = updated else {
        bail!(
            "open item `{item_id}` not found in {}",
            sidecar_path.display()
        );
    };
    let mut serialized = String::new();
    for item in &items {
        serialized.push_str(&serde_json::to_string(item)?);
        serialized.push('\n');
    }
    fs::write(&sidecar_path, serialized).with_context(|| {
        format!(
            "failed to write open items sidecar {}",
            sidecar_path.display()
        )
    })?;
    let touched_brief = if options.touch_brief {
        touch_brief_freshness(brief_path, &content, &now)?
    } else {
        false
    };
    Ok(InterviewOpenItemUpdateReport {
        schema_version: "knowledge_interview_open_item_update_v1",
        brief_path: brief_path.to_path_buf(),
        open_items_path: sidecar_path,
        item,
        touched_brief,
    })
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
}

fn normalize_title(value: &str) -> Result<String> {
    let title = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        bail!("interview title cannot be empty");
    }
    Ok(title)
}

fn normalize_required_text(label: &str, value: &str) -> Result<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        bail!("{label} cannot be empty");
    }
    Ok(normalized)
}

fn normalize_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
        .collect()
}

fn validate_intent(value: &str) -> Result<()> {
    if ALLOWED_INTENTS.contains(&value) {
        Ok(())
    } else {
        bail!(
            "unsupported interview intent `{value}`; expected one of: {}",
            ALLOWED_INTENTS.join(", ")
        );
    }
}

struct BriefTemplateInput<'a> {
    doc_id: &'a str,
    title: &'a str,
    title_key: &'a str,
    intent: &'a str,
    created_at: &'a str,
    agent: &'a str,
    source_article: Option<&'a str>,
    related_draft: Option<&'a str>,
    open_items_name: &'a str,
}

fn render_brief_template(input: &BriefTemplateInput<'_>) -> String {
    let doc_id = yaml_string(input.doc_id);
    let title = yaml_string(input.title);
    let title_key = yaml_string(input.title_key);
    let intent = yaml_string(input.intent);
    let created_at = yaml_string(input.created_at);
    let agent = yaml_string(input.agent);
    let source_article = yaml_optional_string(input.source_article);
    let related_draft = yaml_optional_string(input.related_draft);
    let open_items_name = yaml_string(input.open_items_name);
    format!(
        "---\nschema_version: {INTERVIEW_SCHEMA_VERSION}\ndoc_kind: \"{INTERVIEW_DOC_KIND}\"\ndoc_id: {doc_id}\ntitle: {title}\ntitle_key: {title_key}\nintent: {intent}\ncreated_at: {created_at}\nlast_updated: {created_at}\nfreshness_state: \"fresh\"\nagent: {agent}\nsource_article: {source_article}\nrelated_draft: {related_draft}\nopen_items_sidecar: {open_items_name}\n---\n\n# Knowledge Interview Brief: {}\n\n## Article Object\n\nTBD.\n\n## Scope\n\nIncluded:\n\nExcluded:\n\nPossible redirects:\n\nPossible merge/split targets:\n\n## Initial Materials\n\nSupplied documents, links, transcripts, screenshots, source excerpts, or notes:\n\nHow the materials should steer interview questions or research:\n\n## User-Framed Summary\n\nTBD.\n\n## Interview Transcript and Context\n\nFreeform knowledge from the user's initial monologue:\n\nFollow-up rounds and answers:\n\nNuance that may not yet be publishable:\n\n## Chronology\n\nDates or order details that disambiguate versions, source records, or handoff state:\n\nApproximate periods only when they matter:\n\nOpen date/order conflicts:\n\n## Entities and Relationships\n\nPeople:\n\nProjects:\n\nGroups:\n\nTerms:\n\nRelated wiki pages:\n\n## Editorial Framing\n\nRecommended angle:\n\nTone risks:\n\nLikely misconceptions:\n\nTerminology notes:\n\n## Research Plan\n\nPrimary-source leads:\n\nSearch queries:\n\nArchive targets:\n\nExisting wiki pages to inspect:\n\nBlocking evidence gaps:\n\n## Interviewer Critic Notes\n\nWhat would make the article thin, duplicative, unsourced, wrongly framed, or missing the user's actual knowledge:\n\nFollow-up questions triggered by this critique:\n\nDeferred gaps and why they are acceptable:\n\n## Draft Plan\n\nLikely sections:\n\nInfobox/template candidates:\n\nCategories to verify:\n\nStatements that require citations:\n\nOpen questions before drafting:\n",
        input.title
    )
}

fn yaml_optional_string(value: Option<&str>) -> String {
    value.map(yaml_string).unwrap_or_else(|| "null".to_string())
}

fn yaml_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn write_if_allowed(path: &Path, content: &str, force: bool) -> Result<bool> {
    if path.exists() && !force {
        bail!(
            "refusing to overwrite existing file {}; pass --force to replace it",
            path.display()
        );
    }
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

fn parse_brief(content: &str) -> Result<ParsedBrief> {
    let mut lines = content.lines();
    let Some(first) = lines.next() else {
        bail!("interview brief is empty");
    };
    if first.trim() != "---" {
        bail!("interview brief must start with YAML frontmatter");
    }

    let mut frontmatter = String::new();
    let mut body = String::new();
    let mut in_frontmatter = true;
    for line in lines {
        if in_frontmatter {
            if line.trim() == "---" {
                in_frontmatter = false;
            } else {
                frontmatter.push_str(line);
                frontmatter.push('\n');
            }
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if in_frontmatter {
        bail!("interview brief frontmatter is not closed");
    }

    let metadata: BriefFrontmatter =
        serde_yaml::from_str(&frontmatter).context("failed to parse interview frontmatter")?;
    let sections_present = parse_second_level_headings(&body);
    let present_set = sections_present
        .iter()
        .map(|heading| heading.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let sections_missing = REQUIRED_BRIEF_SECTIONS
        .iter()
        .filter(|section| !present_set.contains(&section.to_ascii_lowercase()))
        .map(|section| (*section).to_string())
        .collect();
    let sections_unfilled = collect_unfilled_sections(&body);
    Ok(ParsedBrief {
        metadata,
        sections_present,
        sections_missing,
        sections_unfilled,
    })
}

/// A section is unfilled when every non-empty line is still template
/// scaffolding: a prompt line ending in `:` or the `TBD.` placeholder. Filled
/// answers are prose or `Label: value` lines, which do not end with `:`.
/// The tool-written Scout Context section is never counted.
fn collect_unfilled_sections(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut has_content = true;
    for raw in body.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("## ") && !trimmed.starts_with("### ") {
            if !section.is_empty() && !has_content {
                out.push(section.clone());
            }
            section = trimmed.trim_start_matches('#').trim().to_string();
            has_content = section.eq_ignore_ascii_case("Scout Context");
            continue;
        }
        if section.is_empty() || has_content || trimmed.is_empty() {
            continue;
        }
        if trimmed != "TBD." && !trimmed.ends_with(':') {
            has_content = true;
        }
    }
    if !section.is_empty() && !has_content {
        out.push(section);
    }
    out
}

fn parse_second_level_headings(body: &str) -> Vec<String> {
    let mut headings = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("## ") {
            continue;
        }
        if trimmed.starts_with("###") {
            continue;
        }
        let heading = trimmed.trim_start_matches('#').trim();
        if !heading.is_empty() {
            headings.push(heading.to_string());
        }
    }
    headings
}

/// Draft-plan signals extracted from a knowledge interview brief body, used by
/// `article-start` to fold human planning into its section skeleton and warnings,
/// and surfaced by `interview show` as the machine-readable handoff plan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BriefDraftPlan {
    pub likely_sections: Vec<String>,
    pub open_questions: Vec<String>,
    pub critic_notes_present: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DraftCapture {
    None,
    Likely,
    Open,
}

const DRAFT_PLAN_LABELS: &[&str] = &[
    "Likely sections:",
    "Infobox/template candidates:",
    "Categories to verify:",
    "Statements that require citations:",
    "Open questions before drafting:",
];

/// Parse the `Draft Plan` and `Interviewer Critic Notes` sections of an interview
/// brief body. Deterministic line scan with no regex, per the wikitool parsing rule.
/// Returns the planned section names, pre-draft open questions, and whether the
/// interviewer/critic loop left any notes.
pub fn parse_brief_draft_plan(body: &str) -> BriefDraftPlan {
    let mut plan = BriefDraftPlan::default();
    let mut section = String::new();
    let mut capture = DraftCapture::None;
    let mut likely_lines: Vec<String> = Vec::new();
    let mut open_lines: Vec<String> = Vec::new();

    for raw in body.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("## ") && !trimmed.starts_with("### ") {
            section = trimmed.trim_start_matches('#').trim().to_string();
            capture = DraftCapture::None;
            continue;
        }
        if section.eq_ignore_ascii_case("Interviewer Critic Notes") {
            if !trimmed.is_empty() && !trimmed.ends_with(':') {
                plan.critic_notes_present = true;
            }
            continue;
        }
        if !section.eq_ignore_ascii_case("Draft Plan") {
            capture = DraftCapture::None;
            continue;
        }
        if let Some(label) = DRAFT_PLAN_LABELS
            .iter()
            .find(|label| trimmed.starts_with(**label))
        {
            let rest = trimmed[label.len()..].trim();
            capture = match *label {
                "Likely sections:" => DraftCapture::Likely,
                "Open questions before drafting:" => DraftCapture::Open,
                _ => DraftCapture::None,
            };
            if !rest.is_empty() {
                match capture {
                    DraftCapture::Likely => likely_lines.push(rest.to_string()),
                    DraftCapture::Open => open_lines.push(rest.to_string()),
                    DraftCapture::None => {}
                }
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        match capture {
            DraftCapture::Likely => likely_lines.push(trimmed.to_string()),
            DraftCapture::Open => open_lines.push(trimmed.to_string()),
            DraftCapture::None => {}
        }
    }

    plan.likely_sections = split_labeled_items(&likely_lines);
    plan.open_questions = split_labeled_items(&open_lines);
    plan
}

/// Sections whose emptiness after an interview is worth an advisory warning.
/// Chronology, Entities, Scope, and Initial Materials stay warning-free: the
/// playbook explicitly says not to force them when they do not improve the
/// article.
const CORE_SUBSTANCE_SECTIONS: &[&str] = &[
    "Article Object",
    "User-Framed Summary",
    "Interview Transcript and Context",
    "Editorial Framing",
    "Research Plan",
    "Draft Plan",
];

/// Handoff signals parsed from brief sections beyond the Draft Plan. Everything
/// here is human knowledge the drafting agent should see without re-reading the
/// whole brief: framing, risks, blocking gaps, and related pages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BriefHandoffSignals {
    pub recommended_angle: Vec<String>,
    pub tone_risks: Vec<String>,
    pub likely_misconceptions: Vec<String>,
    pub terminology_notes: Vec<String>,
    pub blocking_evidence_gaps: Vec<String>,
    pub related_wiki_pages: Vec<String>,
}

/// Parse labeled lists from the Editorial Framing, Research Plan, and Entities
/// sections. Same deterministic line scan as the draft-plan parser: a label line
/// starts capture, list items follow until the next label or section.
pub fn parse_brief_handoff_signals(body: &str) -> BriefHandoffSignals {
    let captures: &[(&str, &str)] = &[
        ("Editorial Framing", "Recommended angle:"),
        ("Editorial Framing", "Tone risks:"),
        ("Editorial Framing", "Likely misconceptions:"),
        ("Editorial Framing", "Terminology notes:"),
        ("Research Plan", "Blocking evidence gaps:"),
        ("Entities and Relationships", "Related wiki pages:"),
    ];
    let mut collected: Vec<Vec<String>> = vec![Vec::new(); captures.len()];
    let mut section = String::new();
    let mut active: Option<usize> = None;

    for raw in body.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("## ") && !trimmed.starts_with("### ") {
            section = trimmed.trim_start_matches('#').trim().to_string();
            active = None;
            continue;
        }
        if let Some(index) = captures.iter().position(|(capture_section, label)| {
            section.eq_ignore_ascii_case(capture_section) && trimmed.starts_with(label)
        }) {
            active = Some(index);
            let rest = trimmed[captures[index].1.len()..].trim();
            if !rest.is_empty() {
                collected[index].push(rest.to_string());
            }
            continue;
        }
        if trimmed.ends_with(':') {
            // Another label within the section ends the current capture.
            active = None;
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if let Some(index) = active {
            collected[index].push(trimmed.to_string());
        }
    }

    let mut lists = collected
        .into_iter()
        .map(|lines| split_labeled_items(&lines));
    BriefHandoffSignals {
        recommended_angle: lists.next().unwrap_or_default(),
        tone_risks: lists.next().unwrap_or_default(),
        likely_misconceptions: lists.next().unwrap_or_default(),
        terminology_notes: lists.next().unwrap_or_default(),
        blocking_evidence_gaps: lists.next().unwrap_or_default(),
        related_wiki_pages: lists.next().unwrap_or_default(),
    }
}

fn strip_list_bullet(line: &str) -> &str {
    line.trim()
        .trim_start_matches(['-', '*', '+', '\u{2022}'])
        .trim()
}

/// Split accumulated label lines into deduplicated items. Semicolons separate
/// inline items; commas remain part of the item because section names and
/// questions often contain them.
fn split_labeled_items(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let stripped = strip_list_bullet(line);
        for part in stripped.split(';') {
            let item = part.trim().trim_end_matches('.').trim();
            if item.is_empty() {
                continue;
            }
            if !out
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(item))
            {
                out.push(item.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod draft_plan_tests {
    use super::*;

    #[test]
    fn parses_inline_semicolon_sections_and_questions() {
        let body = "## Draft Plan\n\nLikely sections: lead; Design, aesthetic, and presentation; Card presentation; Roster and seasonal variants\n\nInfobox/template candidates: none\n\nOpen questions before drafting: confirm plural title, with the user; whether to include an infobox\n\n## Interviewer Critic Notes\n\nWhat would make the article thin: the lineage claims are uncited\n";
        let plan = parse_brief_draft_plan(body);
        assert_eq!(
            plan.likely_sections,
            vec![
                "lead".to_string(),
                "Design, aesthetic, and presentation".to_string(),
                "Card presentation".to_string(),
                "Roster and seasonal variants".to_string(),
            ]
        );
        assert_eq!(
            plan.open_questions,
            vec![
                "confirm plural title, with the user".to_string(),
                "whether to include an infobox".to_string(),
            ]
        );
        assert!(plan.critic_notes_present);
    }

    #[test]
    fn parses_bulleted_sections_and_detects_empty_critic_notes() {
        let body = "## Draft Plan\n\nLikely sections:\n- Design\n- Reception\n\nOpen questions before drafting:\n\n## Interviewer Critic Notes\n\nWhat would make the article thin, duplicative, unsourced, wrongly framed, or missing knowledge:\n\nFollow-up questions triggered by this critique:\n";
        let plan = parse_brief_draft_plan(body);
        assert_eq!(
            plan.likely_sections,
            vec!["Design".to_string(), "Reception".to_string()]
        );
        assert!(plan.open_questions.is_empty());
        assert!(!plan.critic_notes_present);
    }
}

fn validate_frontmatter(
    metadata: &BriefFrontmatter,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    if metadata.schema_version != Some(INTERVIEW_SCHEMA_VERSION) {
        errors.push(format!("schema_version must be {INTERVIEW_SCHEMA_VERSION}"));
    }
    if metadata.doc_kind.as_deref() != Some(INTERVIEW_DOC_KIND) {
        errors.push(format!("doc_kind must be `{INTERVIEW_DOC_KIND}`"));
    }
    require_nonempty("doc_id", metadata.doc_id.as_deref(), errors);
    require_nonempty("title", metadata.title.as_deref(), errors);
    require_nonempty("title_key", metadata.title_key.as_deref(), errors);
    require_nonempty("created_at", metadata.created_at.as_deref(), errors);
    require_nonempty("last_updated", metadata.last_updated.as_deref(), errors);
    require_nonempty(
        "open_items_sidecar",
        metadata.open_items_sidecar.as_deref(),
        errors,
    );
    validate_rfc3339_field("created_at", metadata.created_at.as_deref(), errors);
    validate_rfc3339_field("last_updated", metadata.last_updated.as_deref(), errors);

    match metadata.intent.as_deref() {
        Some(intent) if ALLOWED_INTENTS.contains(&intent) => {}
        Some(intent) => errors.push(format!("unsupported intent `{intent}`")),
        None => errors.push("missing required frontmatter field `intent`".to_string()),
    }
    match metadata.freshness_state.as_deref() {
        Some(freshness) if ALLOWED_FRESHNESS.contains(&freshness) => {}
        Some(freshness) => warnings.push(format!("unsupported freshness_state `{freshness}`")),
        None => warnings.push("missing freshness_state".to_string()),
    }
}

fn require_nonempty(name: &str, value: Option<&str>, errors: &mut Vec<String>) {
    if value.map(str::trim).unwrap_or("").is_empty() {
        errors.push(format!("missing required frontmatter field `{name}`"));
    }
}

fn validate_rfc3339_field(name: &str, value: Option<&str>, errors: &mut Vec<String>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if let Err(error) = rfc3339_to_unix(value) {
        errors.push(format!(
            "frontmatter field `{name}` must be a UTC RFC3339 timestamp: {error}"
        ));
    }
}

fn validate_open_items_sidecar(
    brief_path: &Path,
    metadata: &BriefFrontmatter,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<OpenItemsValidationResult> {
    let path = match open_items_sidecar_path(brief_path, metadata) {
        Ok(path) => path,
        Err(error) => {
            errors.push(format!("open items sidecar path is invalid: {error}"));
            return Ok(OpenItemsValidationResult {
                path: brief_path.to_path_buf(),
                items: Vec::new(),
                counts: InterviewOpenItemCounts::default(),
            });
        }
    };
    collect_open_items(&path, errors, warnings)
}

fn open_items_sidecar_path(brief_path: &Path, metadata: &BriefFrontmatter) -> Result<PathBuf> {
    let Some(sidecar) = metadata.open_items_sidecar.as_deref() else {
        bail!("missing open_items_sidecar");
    };
    validate_sidecar_filename(sidecar)?;
    let path = brief_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(sidecar);
    reject_symlink_sidecar(&path)?;
    Ok(path)
}

fn validate_sidecar_filename(sidecar: &str) -> Result<()> {
    let trimmed = sidecar.trim();
    if trimmed.is_empty() {
        bail!("open_items_sidecar cannot be empty");
    }
    if trimmed != sidecar {
        bail!("open_items_sidecar must not contain leading or trailing whitespace");
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        bail!("open_items_sidecar must be a sibling filename, not a path");
    }
    let mut components = Path::new(trimmed).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => bail!("open_items_sidecar must be a sibling filename"),
    }
    if !trimmed.ends_with(".open_items.jsonl") {
        bail!("open_items_sidecar must end with `.open_items.jsonl`");
    }
    Ok(())
}

fn reject_symlink_sidecar(path: &Path) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        bail!(
            "open items sidecar must be a regular sibling file, not a symlink: {}",
            path.display()
        );
    }
    Ok(())
}

fn collect_open_items(
    path: &Path,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<OpenItemsValidationResult> {
    if !path.is_file() {
        errors.push(format!("open items sidecar missing: {}", path.display()));
        return Ok(OpenItemsValidationResult {
            path: path.to_path_buf(),
            items: Vec::new(),
            counts: InterviewOpenItemCounts::default(),
        });
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read open items sidecar {}", path.display()))?;
    let mut items = Vec::new();
    let mut counts = InterviewOpenItemCounts::default();
    let mut item_ids = BTreeSet::new();
    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        counts.total += 1;
        let item = match serde_json::from_str::<InterviewOpenItemRecord>(line) {
            Ok(item) => item,
            Err(error) => {
                errors.push(format!(
                    "open item line {} is not valid structured JSON: {error}",
                    index + 1
                ));
                continue;
            }
        };
        validate_open_item_record(
            index + 1,
            &item,
            &mut counts,
            &mut item_ids,
            errors,
            warnings,
        );
        items.push(item);
    }
    Ok(OpenItemsValidationResult {
        path: path.to_path_buf(),
        items,
        counts,
    })
}

fn read_open_items_sidecar(path: &Path) -> Result<OpenItemsValidationResult> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let result = collect_open_items(path, &mut errors, &mut warnings)?;
    if !errors.is_empty() {
        bail!("{}", errors.join("; "));
    }
    if !warnings.is_empty() {
        bail!("{}", warnings.join("; "));
    }
    Ok(result)
}

fn validate_open_item_record(
    line_number: usize,
    item: &InterviewOpenItemRecord,
    counts: &mut InterviewOpenItemCounts,
    item_ids: &mut BTreeSet<String>,
    errors: &mut Vec<String>,
    _warnings: &mut Vec<String>,
) {
    if item.schema_version.as_deref() != Some(OPEN_ITEM_SCHEMA_VERSION) {
        errors.push(format!(
            "open item line {line_number} schema_version must be `{OPEN_ITEM_SCHEMA_VERSION}`"
        ));
    }
    let item_id = match item.item_id.as_deref().map(str::trim) {
        Some(item_id) if !item_id.is_empty() => item_id,
        _ => {
            errors.push(format!("open item line {line_number} is missing item_id"));
            ""
        }
    };
    if !item_id.is_empty() && !item_ids.insert(item_id.to_string()) {
        errors.push(format!("duplicate open item_id `{item_id}`"));
    }
    match item.kind.as_deref().map(str::trim) {
        Some(kind) if ALLOWED_OPEN_ITEM_KINDS.contains(&kind) => {
            *counts.by_kind.entry(kind.to_string()).or_insert(0) += 1;
            if NEGATIVE_EVIDENCE_KINDS.contains(&kind) {
                counts.negative_evidence += 1;
            }
        }
        Some(kind) if !kind.is_empty() => {
            counts.unknown_kind += 1;
            errors.push(format!("unsupported open item kind `{kind}`"));
        }
        _ => {
            counts.unknown_kind += 1;
            errors.push(format!("open item `{item_id}` is missing kind"));
        }
    }
    match item.status.as_deref().map(str::trim) {
        Some("open") => counts.open += 1,
        Some("resolved") => counts.resolved += 1,
        Some("rejected") => counts.rejected += 1,
        Some("deferred") => counts.deferred += 1,
        Some(status) if !status.is_empty() => {
            counts.unknown_status += 1;
            errors.push(format!("unsupported open item status `{status}`"));
        }
        _ => {
            counts.unknown_status += 1;
            errors.push(format!("open item `{item_id}` is missing status"));
        }
    }
    if let Some(status) = item
        .status
        .as_deref()
        .map(str::trim)
        .filter(|status| !status.is_empty() && ALLOWED_OPEN_ITEM_STATUSES.contains(status))
    {
        *counts.by_status.entry(status.to_string()).or_insert(0) += 1;
    }
    if item.text.as_deref().map(str::trim).unwrap_or("").is_empty() {
        errors.push(format!("open item `{item_id}` is missing text"));
    }
    if item
        .created_at
        .as_deref()
        .map(|value| rfc3339_to_unix(value).is_ok())
        != Some(true)
    {
        errors.push(format!("open item `{item_id}` has invalid created_at"));
    }
    if item
        .updated_at
        .as_deref()
        .map(|value| rfc3339_to_unix(value).is_ok())
        != Some(true)
    {
        errors.push(format!("open item `{item_id}` has invalid updated_at"));
    }
}

impl OpenItemsValidationResult {
    fn counts_unknown_free(&self) -> bool {
        self.counts.unknown_kind == 0 && self.counts.unknown_status == 0
    }
}

fn validate_open_item_kind(value: &str) -> Result<()> {
    if ALLOWED_OPEN_ITEM_KINDS.contains(&value) {
        Ok(())
    } else {
        bail!(
            "unsupported open item kind `{value}`; expected one of: {}",
            ALLOWED_OPEN_ITEM_KINDS.join(", ")
        );
    }
}

fn validate_open_item_status(value: &str) -> Result<()> {
    if ALLOWED_OPEN_ITEM_STATUSES.contains(&value) {
        Ok(())
    } else {
        bail!(
            "unsupported open item status `{value}`; expected one of: {}",
            ALLOWED_OPEN_ITEM_STATUSES.join(", ")
        );
    }
}

fn next_open_item_id(timestamp: &str, items: &[InterviewOpenItemRecord]) -> String {
    let base = format!("OI-{timestamp}");
    let existing = items
        .iter()
        .filter_map(|item| item.item_id.as_deref())
        .collect::<BTreeSet<_>>();
    if !existing.contains(base.as_str()) {
        return base;
    }
    for suffix in 2.. {
        let candidate = format!("{base}-{suffix}");
        if !existing.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("unbounded suffix search should return")
}

fn append_jsonl_line(path: &Path, line: &str) -> Result<()> {
    let mut existing = fs::read_to_string(path).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(line);
    existing.push('\n');
    fs::write(path, existing).with_context(|| format!("failed to append {}", path.display()))
}

fn touch_brief_freshness(brief_path: &Path, content: &str, updated_at: &str) -> Result<bool> {
    let mut lines = Vec::new();
    let mut in_frontmatter = false;
    let mut frontmatter_closed = false;
    let mut saw_last_updated = false;
    let mut saw_freshness_state = false;
    for (index, line) in content.lines().enumerate() {
        if index == 0 && line.trim() == "---" {
            in_frontmatter = true;
            lines.push(line.to_string());
            continue;
        }
        if in_frontmatter && line.trim() == "---" {
            if !saw_last_updated {
                lines.push(format!("last_updated: {updated_at}"));
            }
            if !saw_freshness_state {
                lines.push("freshness_state: fresh".to_string());
            }
            lines.push(line.to_string());
            frontmatter_closed = true;
            in_frontmatter = false;
            continue;
        }
        if in_frontmatter {
            let trimmed = line.trim_start();
            if trimmed.starts_with("last_updated:") {
                lines.push(format!("last_updated: {updated_at}"));
                saw_last_updated = true;
                continue;
            }
            if trimmed.starts_with("freshness_state:") {
                lines.push("freshness_state: fresh".to_string());
                saw_freshness_state = true;
                continue;
            }
        }
        lines.push(line.to_string());
    }
    if !frontmatter_closed {
        bail!("interview brief frontmatter is not closed");
    }
    let mut updated = lines.join("\n");
    if content.ends_with('\n') {
        updated.push('\n');
    }
    if updated == content {
        return Ok(false);
    }
    fs::write(brief_path, updated)
        .with_context(|| format!("failed to update {}", brief_path.display()))?;
    Ok(true)
}

fn compute_freshness(last_updated: Option<&str>, stale_days: u64) -> String {
    let Some(last_updated) = last_updated else {
        return "unknown".to_string();
    };
    let Ok(updated) = rfc3339_to_unix(last_updated) else {
        return "unknown".to_string();
    };
    let Ok(now) = current_unix_seconds() else {
        return "unknown".to_string();
    };
    let stale_seconds = stale_days.saturating_mul(24 * 60 * 60);
    if now.saturating_sub(updated) > stale_seconds {
        "stale".to_string()
    } else {
        "fresh".to_string()
    }
}

fn current_unix_seconds() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before UNIX_EPOCH")?
        .as_secs())
}

fn current_compact_timestamp() -> Result<String> {
    let seconds = current_unix_seconds()?;
    let (year, month, day, hour, minute, second) = unix_to_utc_components(seconds);
    Ok(format!(
        "{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z"
    ))
}

fn validate_compact_timestamp(value: &str) -> Result<&str> {
    // Preserve the underlying cause (e.g. "invalid day" for 20260631T...) instead of
    // misreporting a calendar-invalid date as a format error.
    compact_to_rfc3339(value).with_context(|| {
        format!("invalid ledger timestamp `{value}` (expected calendar-valid YYYYMMDDTHHMMSSZ)")
    })?;
    Ok(value)
}

fn compact_to_rfc3339(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    if bytes.len() != 16 || bytes[8] != b'T' || bytes[15] != b'Z' {
        bail!("invalid compact timestamp");
    }
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 8 | 15) {
            continue;
        }
        if !byte.is_ascii_digit() {
            bail!("invalid compact timestamp");
        }
    }
    let year = parse_u32(&value[0..4])?;
    let month = parse_u32(&value[4..6])?;
    let day = parse_u32(&value[6..8])?;
    let hour = parse_u32(&value[9..11])?;
    let minute = parse_u32(&value[11..13])?;
    let second = parse_u32(&value[13..15])?;
    validate_datetime(year, month, day, hour, minute, second)?;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

fn rfc3339_to_unix(value: &str) -> Result<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        bail!("unsupported timestamp format");
    }
    let year = parse_i32(&value[0..4])?;
    let month = parse_u32(&value[5..7])?;
    let day = parse_u32(&value[8..10])?;
    let hour = parse_u32(&value[11..13])?;
    let minute = parse_u32(&value[14..16])?;
    let second = parse_u32(&value[17..19])?;
    validate_datetime(year as u32, month, day, hour, minute, second)?;
    let days = days_from_civil(year, month, day);
    if days < 0 {
        bail!("timestamp predates UNIX epoch");
    }
    Ok(days as u64 * 86_400 + hour as u64 * 3_600 + minute as u64 * 60 + second as u64)
}

fn parse_u32(value: &str) -> Result<u32> {
    value
        .parse::<u32>()
        .with_context(|| format!("invalid integer `{value}`"))
}

fn parse_i32(value: &str) -> Result<i32> {
    value
        .parse::<i32>()
        .with_context(|| format!("invalid integer `{value}`"))
}

fn validate_datetime(
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Result<()> {
    if !(1..=12).contains(&month) {
        bail!("invalid month");
    }
    let max_day = days_in_month(year, month);
    if day == 0 || day > max_day {
        bail!("invalid day");
    }
    if hour > 23 || minute > 59 || second > 59 {
        bail!("invalid time");
    }
    Ok(())
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn unix_to_utc_components(seconds: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = (day_seconds / 3_600) as u32;
    let minute = ((day_seconds % 3_600) / 60) as u32;
    let second = (day_seconds % 60) as u32;
    (year, month, day, hour, minute, second)
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let yoe = adjusted_year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era as i64 * 146_097 + doe as i64 - 719_468
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::runtime::{ResolvedPaths, ValueSource};

    use super::*;

    fn paths(root: &std::path::Path) -> ResolvedPaths {
        ResolvedPaths {
            project_root: root.to_path_buf(),
            wiki_content_dir: root.join("wiki_content"),
            templates_dir: root.join("templates"),
            state_dir: root.join(".wikitool"),
            data_dir: root.join(".wikitool/data"),
            db_path: root.join(".wikitool/data/wikitool.db"),
            config_path: root.join(".wikitool/config.toml"),
            parser_config_path: root.join(".wikitool/parser-config.json"),
            root_source: ValueSource::Default,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn title_key_is_filesystem_safe_and_readable() {
        assert_eq!(title_key_for_interview("Radbro Webring"), "Radbro_Webring");
        assert_eq!(
            title_key_for_interview("Category:Radbro/Webring?"),
            "Category_Radbro_Webring"
        );
    }

    #[test]
    fn init_creates_brief_and_sidecars() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let report = create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: Some("codex".to_string()),
                source_article: None,
                related_draft: None,
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");

        assert_eq!(report.title_key, "Radbro_Webring");
        assert!(report.brief_path.is_file());
        assert!(report.open_items_path.is_file());

        let validation = validate_interview_brief(&report.brief_path, 45).expect("validate");
        assert_ne!(validation.status, InterviewValidationStatus::Invalid);
        assert_eq!(validation.summary.title.as_deref(), Some("Radbro Webring"));
    }

    #[test]
    fn init_quotes_yaml_sensitive_frontmatter_values() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let report = create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Miya: Notes".to_string(),
                intent: "new".to_string(),
                agent: Some("codex: local".to_string()),
                source_article: Some("Source: Article".to_string()),
                related_draft: Some(".wikitool/drafts/Miya Notes.wiki".to_string()),
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");

        let validation = validate_interview_brief(&report.brief_path, 45).expect("validate");
        assert_ne!(validation.status, InterviewValidationStatus::Invalid);
        assert_eq!(validation.summary.title.as_deref(), Some("Miya: Notes"));
        assert_eq!(validation.summary.agent.as_deref(), Some("codex: local"));
    }

    #[test]
    fn validate_rejects_invalid_frontmatter_timestamps() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let report = create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: None,
                source_article: None,
                related_draft: None,
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");
        let brief = fs::read_to_string(&report.brief_path)
            .expect("read brief")
            .replace(
                "last_updated: \"2026-06-01T17:24:30Z\"",
                "last_updated: \"not-a-timestamp\"",
            );
        fs::write(&report.brief_path, brief).expect("write brief");

        let validation = validate_interview_brief(&report.brief_path, 45).expect("validate");
        assert_eq!(validation.status, InterviewValidationStatus::Invalid);
        assert!(validation.errors.iter().any(|error| {
            error.contains("frontmatter field `last_updated` must be a UTC RFC3339 timestamp")
        }));
    }

    #[test]
    fn validate_rejects_sidecar_paths_that_escape_the_brief_directory() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let report = create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: None,
                source_article: None,
                related_draft: None,
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");
        let brief = fs::read_to_string(&report.brief_path)
            .expect("read brief")
            .replace(
                "open_items_sidecar: \"20260601T172430Z.open_items.jsonl\"",
                "open_items_sidecar: \"../outside.open_items.jsonl\"",
            );
        fs::write(&report.brief_path, brief).expect("write brief");

        let validation = validate_interview_brief(&report.brief_path, 45).expect("validate");
        assert_eq!(validation.status, InterviewValidationStatus::Invalid);
        assert!(validation.errors.iter().any(|error| {
            error.contains("open items sidecar path is invalid")
                && error.contains("sibling filename")
        }));

        let append_error = append_interview_open_item(
            &report.brief_path,
            &InterviewOpenItemAppendOptions {
                kind: "missing_source".to_string(),
                status: "open".to_string(),
                text: "Need source for launch sequence.".to_string(),
                item_id: Some("OI-001".to_string()),
                source_leads: Vec::new(),
                notes: None,
                timestamp: Some("20260601T180000Z".to_string()),
                touch_brief: false,
            },
        )
        .expect_err("escaped sidecar must not be writable");
        assert!(append_error.to_string().contains("sibling filename"));
    }

    #[test]
    fn audit_summarizes_ledger() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: None,
                source_article: None,
                related_draft: None,
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");

        let audit = audit_interview_briefs(&paths, 45).expect("audit");
        assert_eq!(audit.total_briefs, 1);
        assert_eq!(audit.invalid, 0);
    }

    #[test]
    fn open_item_add_list_and_validate_counts_negative_evidence() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let report = create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: None,
                source_article: None,
                related_draft: None,
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");

        let append = append_interview_open_item(
            &report.brief_path,
            &InterviewOpenItemAppendOptions {
                kind: "rejected_source".to_string(),
                status: "open".to_string(),
                text: "A forum mirror did not contain the claimed Webring launch date.".to_string(),
                item_id: Some("OI-001".to_string()),
                source_leads: vec!["https://example.org/archive".to_string()],
                notes: Some("Keep as negative evidence.".to_string()),
                timestamp: Some("20260601T180000Z".to_string()),
                touch_brief: true,
            },
        )
        .expect("append");

        assert_eq!(append.item.item_id.as_deref(), Some("OI-001"));
        assert!(append.touched_brief);

        let list = list_interview_open_items(&report.brief_path).expect("list");
        assert_eq!(list.status, InterviewValidationStatus::Valid);
        assert_eq!(list.counts.total, 1);
        assert_eq!(list.counts.open, 1);
        assert_eq!(list.counts.negative_evidence, 1);

        let validation = validate_interview_brief(&report.brief_path, 45).expect("validate");
        assert_eq!(validation.summary.open_item_count, 1);
        assert_eq!(validation.summary.open_item_counts.negative_evidence, 1);

        let brief = fs::read_to_string(&report.brief_path).expect("read brief");
        assert!(brief.contains("last_updated: 2026-06-01T18:00:00Z"));
    }

    #[test]
    fn interview_lifecycle_without_claims() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let report = create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: None,
                source_article: None,
                related_draft: None,
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");

        // A do_not_assert open item folds the orphaned "don't state this until
        // sourced" memory into the single ledger and counts as negative evidence.
        let dna = append_interview_open_item(
            &report.brief_path,
            &InterviewOpenItemAppendOptions {
                kind: "do_not_assert".to_string(),
                status: "open".to_string(),
                text: "Do not state the founding year until a primary source is found.".to_string(),
                item_id: Some("OI-001".to_string()),
                source_leads: Vec::new(),
                notes: None,
                timestamp: Some("20260601T180000Z".to_string()),
                touch_brief: true,
            },
        )
        .expect("append do_not_assert");
        assert_eq!(dna.item.kind.as_deref(), Some("do_not_assert"));

        append_interview_open_item(
            &report.brief_path,
            &InterviewOpenItemAppendOptions {
                kind: "missing_source".to_string(),
                status: "open".to_string(),
                text: "Need a collections page citation.".to_string(),
                item_id: Some("OI-002".to_string()),
                source_leads: vec!["https://example.org/archive".to_string()],
                notes: None,
                timestamp: Some("20260601T180500Z".to_string()),
                touch_brief: true,
            },
        )
        .expect("append missing_source");

        let updated = update_interview_open_item(
            &report.brief_path,
            &InterviewOpenItemUpdateOptions {
                item_id: "OI-002".to_string(),
                status: Some("resolved".to_string()),
                notes: Some("Collections page shipped.".to_string()),
                text: None,
                timestamp: Some("20260601T181000Z".to_string()),
                touch_brief: true,
            },
        )
        .expect("resolve missing_source");
        assert_eq!(updated.item.status.as_deref(), Some("resolved"));

        let validation = validate_interview_brief(&report.brief_path, 45).expect("validate");
        assert_ne!(validation.status, InterviewValidationStatus::Invalid);
        assert_eq!(validation.summary.open_item_count, 2);
        assert_eq!(
            validation
                .summary
                .open_item_counts
                .by_kind
                .get("do_not_assert")
                .copied(),
            Some(1)
        );
        assert_eq!(
            validation
                .summary
                .open_item_counts
                .by_kind
                .get("missing_source")
                .copied(),
            Some(1)
        );
        // Only the do_not_assert item is a negative-evidence kind here; the
        // resolved missing_source item does not contribute.
        assert_eq!(validation.summary.open_item_counts.negative_evidence, 1);
        assert_eq!(validation.summary.open_item_counts.open, 1);
        assert_eq!(validation.summary.open_item_counts.resolved, 1);

        let audit = audit_interview_briefs(&paths, 45).expect("audit");
        assert_eq!(audit.total_briefs, 1);
        assert_eq!(audit.invalid, 0);
    }

    #[test]
    fn open_item_add_rejects_duplicate_item_ids() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let report = create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: None,
                source_article: None,
                related_draft: None,
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");
        let options = InterviewOpenItemAppendOptions {
            kind: "missing_source".to_string(),
            status: "open".to_string(),
            text: "Need source for launch sequence.".to_string(),
            item_id: Some("OI-001".to_string()),
            source_leads: Vec::new(),
            notes: None,
            timestamp: Some("20260601T180000Z".to_string()),
            touch_brief: false,
        };

        append_interview_open_item(&report.brief_path, &options).expect("first append");
        let error =
            append_interview_open_item(&report.brief_path, &options).expect_err("duplicate id");
        assert!(error.to_string().contains("duplicate open item_id"));
    }

    #[test]
    fn open_item_validation_rejects_invalid_structured_records() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let report = create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: None,
                source_article: None,
                related_draft: None,
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");
        fs::write(
            &report.open_items_path,
            r#"{"schema_version":"knowledge_interview_open_item_v1","item_id":"OI-001","kind":"made_up","status":"unknown","text":"bad","created_at":"2026-06-01T18:00:00Z","updated_at":"2026-06-01T18:00:00Z"}"#,
        )
        .expect("write open item");

        let validation = validate_interview_brief(&report.brief_path, 45).expect("validate");
        assert_eq!(validation.status, InterviewValidationStatus::Invalid);
        assert!(
            validation
                .errors
                .iter()
                .any(|error| error.contains("unsupported open item kind"))
        );
        assert!(
            validation
                .errors
                .iter()
                .any(|error| error.contains("unsupported open item status"))
        );
    }

    #[test]
    fn audit_is_resilient_to_unparseable_brief() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: None,
                source_article: None,
                related_draft: None,
                timestamp: Some("20260601T172430Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect("init");
        // A second brief that is not parseable must not abort the whole audit.
        let broken_dir = interviews_root(&paths).join("Broken");
        fs::create_dir_all(&broken_dir).expect("mkdir");
        fs::write(
            broken_dir.join("20260601T180000Z.brief.md"),
            "no frontmatter here\njust prose\n",
        )
        .expect("write broken brief");

        let audit = audit_interview_briefs(&paths, 45)
            .expect("audit must survive a single unparseable brief");
        assert_eq!(audit.total_briefs, 2);
        // A freshly-inited brief is all template scaffolding, which now draws
        // advisory substance warnings instead of counting as clean.
        assert_eq!(audit.valid, 0);
        assert_eq!(audit.warning, 1);
        assert_eq!(audit.invalid, 1);
        assert!(audit.briefs.iter().any(|brief| {
            brief.status == InterviewValidationStatus::Invalid
                && brief
                    .errors
                    .iter()
                    .any(|error| error.contains("could not be parsed"))
        }));
    }

    #[test]
    fn rejects_calendar_invalid_timestamp_with_cause() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let error = create_interview_brief(
            &paths,
            &InterviewInitOptions {
                title: "Radbro Webring".to_string(),
                intent: "new".to_string(),
                agent: None,
                source_article: None,
                related_draft: None,
                timestamp: Some("20260631T100000Z".to_string()),
                force: false,
                scout: None,
            },
        )
        .expect_err("June 31 is not a valid calendar date");
        let rendered = format!("{error:#}");
        assert!(rendered.contains("invalid ledger timestamp"));
        assert!(rendered.contains("invalid day"));
    }
}

#[cfg(test)]
mod scout_and_handoff_tests {
    use super::*;

    fn scout_fixture() -> InterviewScoutContext {
        InterviewScoutContext {
            local_state: "likely_missing".to_string(),
            comparable_pages: vec!["Post-Authorship".to_string()],
            closest_comparable_title: Some("Post-Authorship".to_string()),
            closest_comparable_outline: vec![
                "Core principles".to_string(),
                "Historical context".to_string(),
            ],
            infobox_candidates: vec![
                "Template:Infobox concept".to_string(),
                "Template:Infobox NFT collection".to_string(),
            ],
            categories_seen: vec!["Category:Concepts".to_string()],
            citation_template_families: Vec::new(),
            missing_query_terms: vec!["XCOPY".to_string()],
        }
    }

    #[test]
    fn scout_section_lands_after_title_and_before_article_object() {
        let brief =
            "---\nx: 1\n---\n\n# Knowledge Interview Brief: T\n\n## Article Object\n\nTBD.\n";
        let out = insert_scout_section(brief, &scout_fixture());
        let scout_index = out.find("## Scout Context").expect("scout section");
        let object_index = out.find("## Article Object").expect("object section");
        assert!(scout_index < object_index);
        assert!(out.contains("Local state: likely_missing"));
        assert!(out.contains(
            "Closest comparable outline (Post-Authorship): Core principles > Historical context"
        ));
        assert!(out.contains("Query terms with no local evidence: XCOPY"));
    }

    #[test]
    fn question_agenda_adapts_to_evidence() {
        let scout = scout_fixture();
        let agenda = build_question_agenda("new", Some(&scout));
        let areas: Vec<&str> = agenda.iter().map(|area| area.area.as_str()).collect();
        assert_eq!(areas.first(), Some(&"subject in the person's own words"));
        assert!(areas.contains(&"primary knowledge the wiki lacks"));
        assert!(areas.contains(&"article shape"));
        assert!(areas.contains(&"unknown term: XCOPY"));
        assert!(areas.contains(&"source leads"));
        assert!(areas.contains(&"subject kind"));
        assert_eq!(areas.last(), Some(&"boundaries"));

        // Without a scout the agenda still opens fluidly and closes on boundaries.
        let bare = build_question_agenda("new", None);
        assert_eq!(bare.len(), 2);

        // An existing page with expand intent asks for the delta, not a monologue only.
        let mut existing = scout_fixture();
        existing.local_state = "exact_page_exists".to_string();
        let expand = build_question_agenda("expand", Some(&existing));
        assert!(
            expand
                .iter()
                .any(|area| area.area == "what the current page gets wrong or misses")
        );

        // The same existing page under the default `new` intent must surface the
        // intent mismatch instead of silently proceeding toward a duplicate.
        let as_new = build_question_agenda("new", Some(&existing));
        assert!(
            as_new
                .iter()
                .any(|area| area.area == "intent check: this page already exists")
        );
        assert!(
            !as_new
                .iter()
                .any(|area| area.area == "what the current page gets wrong or misses")
        );
    }

    #[test]
    fn unfilled_sections_track_template_state() {
        let body = "## Scout Context\n\nLocal state: likely_missing\n\n## Article Object\n\nTBD.\n\n## Scope\n\nIncluded:\n\nExcluded:\n\n## User-Framed Summary\n\nA real answer from the interview.\n";
        let unfilled = collect_unfilled_sections(body);
        assert_eq!(
            unfilled,
            vec!["Article Object".to_string(), "Scope".to_string()]
        );
    }

    #[test]
    fn handoff_signals_parse_labeled_lists() {
        let body = "## Entities and Relationships\n\nPeople:\n\nRelated wiki pages:\nPost-Authorship\nXCOPY\n\n## Editorial Framing\n\nRecommended angle: collection as artifact of the 2021 wave\nTone risks:\n- significance inflation\n\n## Research Plan\n\nBlocking evidence gaps:\nmint date unverified\n";
        let signals = parse_brief_handoff_signals(body);
        assert_eq!(
            signals.recommended_angle,
            vec!["collection as artifact of the 2021 wave".to_string()]
        );
        assert_eq!(
            signals.tone_risks,
            vec!["significance inflation".to_string()]
        );
        assert_eq!(
            signals.blocking_evidence_gaps,
            vec!["mint date unverified".to_string()]
        );
        assert_eq!(
            signals.related_wiki_pages,
            vec!["Post-Authorship".to_string(), "XCOPY".to_string()]
        );
        assert!(signals.likely_misconceptions.is_empty());
    }
}
