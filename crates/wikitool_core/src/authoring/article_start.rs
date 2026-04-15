use std::collections::{BTreeMap, BTreeSet};

use crate::knowledge::model::AuthoringKnowledgePackResult;
use crate::profile::ProfileOverlay;

use super::model::{
    ArticleStartIntent, ArticleStartResult, AuthoringConstraint, CategorySurfaceEntry,
    ContextSurfaceSource, EvidenceRef, LinkSurfaceEntry, LocalExistenceState, LocalIntegrationLane,
    OpenQuestion, RecommendedAction, RequiredTemplate, SectionSkeleton, SubjectResearchLane,
    SubjectTypeHint, TemplateSurfaceEntry,
};

pub fn build_article_start(
    pack: &AuthoringKnowledgePackResult,
    overlay: &ProfileOverlay,
    intent: ArticleStartIntent,
) -> ArticleStartResult {
    let local_state = if let Some(exact_page) = &pack.topic_assessment.exact_page {
        if exact_page.is_redirect {
            LocalExistenceState::RedirectExists
        } else {
            LocalExistenceState::ExactPageExists
        }
    } else if !pack.stub_missing_links.is_empty() {
        LocalExistenceState::LinkedButMissing
    } else if pack.topic_assessment.local_title_hit_count > 1 {
        LocalExistenceState::AmbiguousLocalCoverage
    } else {
        LocalExistenceState::LikelyMissing
    };

    let evidence = pack
        .chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| EvidenceRef {
            id: format!("local-chunk-{index}"),
            source_kind: "local_chunk".to_string(),
            source_title: chunk.source_title.clone(),
            locator: chunk.section_heading.clone(),
            score: u32::try_from(chunk.token_estimate.min(u32::MAX as usize)).unwrap_or(u32::MAX),
        })
        .collect::<Vec<_>>();

    let subject_research = SubjectResearchLane {
        summary: pack.chunks.first().map(|chunk| chunk.chunk_text.clone()),
        candidate_facts: pack
            .chunks
            .iter()
            .take(5)
            .map(|chunk| chunk.chunk_text.clone())
            .collect(),
        external_sources_shortlist: pack
            .suggested_references
            .iter()
            .take(5)
            .map(|reference| format!("{} / {}", reference.citation_family, reference.source_type))
            .collect(),
        ambiguity_notes: pack.stub_missing_links.clone(),
        evidence: evidence.clone(),
    };

    let comparable_pages = pack
        .related_pages
        .iter()
        .take(8)
        .map(|page| page.title.clone())
        .collect::<Vec<_>>();
    let required_templates = build_required_templates(overlay);
    let subject_type_hints = build_subject_type_hints(pack, overlay);
    let available_infoboxes = build_available_infoboxes(pack, overlay);
    let citation_templates_seen = build_citation_templates(pack, overlay);
    let template_surface = build_template_surface(pack, overlay);
    let categories_seen = build_category_surface(pack);
    let links_seen = build_link_surface(pack);
    let section_skeleton = build_section_skeleton(pack);

    let local_integration = LocalIntegrationLane {
        comparable_pages,
        required_templates,
        subject_type_hints,
        available_infoboxes,
        citation_templates_seen,
        template_surface,
        categories_seen,
        links_seen,
        section_skeleton,
        docs_queries: pack
            .docs_context
            .as_ref()
            .map(|docs| docs.queries.clone())
            .unwrap_or_default(),
    };

    let constraints = build_constraints(overlay);
    let mut open_questions = Vec::new();
    if !pack.stub_missing_links.is_empty() {
        open_questions.push(OpenQuestion {
            question: "Which missing linked pages represent real prerequisites for this article?"
                .to_string(),
            reason: "The stub references titles that do not exist locally.".to_string(),
            blocking: false,
            evidence: evidence.iter().take(2).cloned().collect(),
        });
    }
    if pack.suggested_references.is_empty() {
        open_questions.push(OpenQuestion {
            question: "Which reliable sources will substantiate the core claims?".to_string(),
            reason: "No citation templates or reference patterns were surfaced locally."
                .to_string(),
            blocking: true,
            evidence: evidence.iter().take(1).cloned().collect(),
        });
    }

    let next_actions = build_next_actions(intent, &local_state);

    ArticleStartResult {
        schema_version: "article_start".to_string(),
        topic: pack.topic.clone(),
        intent,
        local_state,
        subject_research,
        local_integration,
        constraints,
        open_questions,
        next_actions,
        raw_pack_ref: Some("knowledge.pack".to_string()),
    }
}

fn build_next_actions(
    intent: ArticleStartIntent,
    local_state: &LocalExistenceState,
) -> Vec<RecommendedAction> {
    match intent {
        ArticleStartIntent::New => {
            let mut actions = Vec::new();
            if matches!(
                local_state,
                LocalExistenceState::ExactPageExists | LocalExistenceState::RedirectExists
            ) {
                actions.push(RecommendedAction {
                    label: "Confirm new-page target".to_string(),
                    why: "The requested title already resolves locally; choose a missing title or switch to expand, audit, or refresh intent.".to_string(),
                });
            }
            actions.push(RecommendedAction {
                label: "Review comparables".to_string(),
                why: "Use local pages as the fit check for terminology, scope, and structure."
                    .to_string(),
            });
            actions.push(RecommendedAction {
                label: "Draft structure".to_string(),
                why: "Start from the section skeleton and required templates before prose."
                    .to_string(),
            });
            actions
        }
        ArticleStartIntent::Expand => vec![
            RecommendedAction {
                label: "Read the existing page".to_string(),
                why: "Expansion should preserve current scope and add only evidenced gaps."
                    .to_string(),
            },
            RecommendedAction {
                label: "Compare section coverage".to_string(),
                why: "Use comparable pages and the skeleton to identify missing local structure."
                    .to_string(),
            },
            RecommendedAction {
                label: "Draft additive edits".to_string(),
                why: "Keep the next pass scoped to new sections, citations, or integration links."
                    .to_string(),
            },
        ],
        ArticleStartIntent::Audit => vec![
            RecommendedAction {
                label: "Run title-scoped checks".to_string(),
                why: "Use article lint and validate --title before changing content.".to_string(),
            },
            RecommendedAction {
                label: "Inspect sources and templates".to_string(),
                why: "Verify citations, required appendices, categories, and template parameters against local evidence.".to_string(),
            },
            RecommendedAction {
                label: "Report actionable findings".to_string(),
                why: "Separate blocking defects from ordinary future-work links and orphan signals."
                    .to_string(),
            },
        ],
        ArticleStartIntent::Refresh => vec![
            RecommendedAction {
                label: "Check local and live state".to_string(),
                why: "Refresh work should start by confirming the current page and sync surface."
                    .to_string(),
            },
            RecommendedAction {
                label: "Refresh dated claims".to_string(),
                why: "Prioritize sources, citations, template usage, categories, and stale wording."
                    .to_string(),
            },
            RecommendedAction {
                label: "Run fix and lint".to_string(),
                why: "Close with safe mechanical fixes and article lint before push review."
                    .to_string(),
            },
        ],
    }
}

fn build_required_templates(overlay: &ProfileOverlay) -> Vec<RequiredTemplate> {
    let mut out = Vec::new();
    if overlay.authoring.require_article_quality_banner
        && let Some(template_title) = overlay.authoring.article_quality_template.as_deref()
    {
        out.push(RequiredTemplate {
            template_title: template_title.to_string(),
            reason: "Required by the current profile overlay for article starts.".to_string(),
        });
    }
    if let Some(template_title) = overlay.authoring.references_template.as_deref() {
        out.push(RequiredTemplate {
            template_title: template_title.to_string(),
            reason: "Required to render the References appendix on this wiki.".to_string(),
        });
    }
    out
}

fn build_subject_type_hints(
    pack: &AuthoringKnowledgePackResult,
    overlay: &ProfileOverlay,
) -> Vec<SubjectTypeHint> {
    let mut hints = BTreeMap::<String, (BTreeSet<String>, BTreeSet<String>)>::new();
    for template in &pack.suggested_templates {
        let template_title = normalize_template_title(&template.template_title);
        if !template_is_infobox(&template_title) {
            continue;
        }
        for preference in &overlay.remilia.infobox_preferences {
            if !preference
                .template_title
                .eq_ignore_ascii_case(&template_title)
            {
                continue;
            }
            let entry = hints
                .entry(preference.subject_type.clone())
                .or_insert_with(|| (BTreeSet::new(), BTreeSet::new()));
            entry.0.extend(template.example_pages.iter().cloned());
            entry.1.insert(template_title.clone());
        }
    }

    let mut out = hints
        .into_iter()
        .map(
            |(subject_type, (supporting_pages, supporting_templates))| SubjectTypeHint {
                subject_type,
                source: ContextSurfaceSource::Both,
                supporting_pages: supporting_pages.into_iter().collect(),
                supporting_templates: supporting_templates.into_iter().collect(),
            },
        )
        .collect::<Vec<_>>();
    out.sort_by(|left, right| left.subject_type.cmp(&right.subject_type));
    out
}

fn build_available_infoboxes(
    pack: &AuthoringKnowledgePackResult,
    overlay: &ProfileOverlay,
) -> Vec<TemplateSurfaceEntry> {
    let profile_mappings = overlay_infobox_subject_type_map(overlay);
    collect_template_entries(
        pack.suggested_templates
            .iter()
            .filter(|template| template_is_infobox(&template.template_title))
            .map(|template| {
                let normalized = normalize_template_title(&template.template_title);
                (
                    normalized.clone(),
                    template.example_pages.clone(),
                    profile_mappings
                        .get(&normalized.to_ascii_lowercase())
                        .cloned(),
                )
            }),
    )
}

fn build_citation_templates(
    pack: &AuthoringKnowledgePackResult,
    overlay: &ProfileOverlay,
) -> Vec<TemplateSurfaceEntry> {
    let mut comparable_entries = BTreeMap::<String, TemplateSurfaceEntry>::new();
    for reference in &pack.suggested_references {
        let template_title = normalize_template_title(
            reference
                .common_templates
                .first()
                .unwrap_or(&reference.citation_family),
        );
        if template_title.is_empty() {
            continue;
        }
        let key = template_title.to_ascii_lowercase();
        let entry = comparable_entries
            .entry(key)
            .or_insert_with(|| TemplateSurfaceEntry {
                template_title: template_title.clone(),
                source: ContextSurfaceSource::Comparables,
                mapped_subject_type: None,
                supporting_pages: Vec::new(),
            });
        extend_sorted_unique(&mut entry.supporting_pages, &reference.example_pages);
    }

    for rule in &overlay.citations.preferred_templates {
        let key = rule.template_title.to_ascii_lowercase();
        if let Some(entry) = comparable_entries.get_mut(&key) {
            entry.source = ContextSurfaceSource::Both;
            continue;
        }
        comparable_entries.insert(
            key,
            TemplateSurfaceEntry {
                template_title: rule.template_title.clone(),
                source: ContextSurfaceSource::Profile,
                mapped_subject_type: None,
                supporting_pages: Vec::new(),
            },
        );
    }

    comparable_entries.into_values().collect()
}

fn build_template_surface(
    pack: &AuthoringKnowledgePackResult,
    overlay: &ProfileOverlay,
) -> Vec<TemplateSurfaceEntry> {
    let profile_templates = overlay
        .profile_template_titles()
        .into_iter()
        .map(|title| title.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut out = pack
        .suggested_templates
        .iter()
        .filter(|template| !template_is_infobox(&template.template_title))
        .map(|template| TemplateSurfaceEntry {
            template_title: normalize_template_title(&template.template_title),
            source: if profile_templates.contains(&template.template_title.to_ascii_lowercase()) {
                ContextSurfaceSource::Both
            } else {
                ContextSurfaceSource::Comparables
            },
            mapped_subject_type: None,
            supporting_pages: dedup_sorted(template.example_pages.clone()),
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| left.template_title.cmp(&right.template_title));
    out.dedup_by(|left, right| {
        left.template_title
            .eq_ignore_ascii_case(&right.template_title)
    });
    out
}

fn build_category_surface(pack: &AuthoringKnowledgePackResult) -> Vec<CategorySurfaceEntry> {
    let mut out = pack
        .suggested_categories
        .iter()
        .map(|category| CategorySurfaceEntry {
            category_title: category.title.clone(),
            source: ContextSurfaceSource::Comparables,
            supporting_pages: dedup_sorted(category.evidence_titles.clone()),
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| left.category_title.cmp(&right.category_title));
    out
}

fn build_link_surface(pack: &AuthoringKnowledgePackResult) -> Vec<LinkSurfaceEntry> {
    let mut out = pack
        .suggested_links
        .iter()
        .map(|link| LinkSurfaceEntry {
            page_title: link.title.clone(),
            source: ContextSurfaceSource::Comparables,
            supporting_pages: dedup_sorted(link.evidence_titles.clone()),
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| left.page_title.cmp(&right.page_title));
    out
}

fn build_section_skeleton(pack: &AuthoringKnowledgePackResult) -> Vec<SectionSkeleton> {
    let mut sections = vec![SectionSkeleton {
        heading: "Overview".to_string(),
        rationale: "Use a concise lead anchored in local terminology.".to_string(),
        required: true,
        content_backed: true,
        supporting_pages: Vec::new(),
    }];

    // Collect chunk headings to determine content_backed status.
    let mut chunk_heading_pages = BTreeMap::<String, BTreeSet<String>>::new();
    for chunk in &pack.chunks {
        if let Some(heading) = chunk.section_heading.as_deref() {
            let normalized = normalize_heading(heading);
            if !normalized.is_empty() && !heading_is_low_signal(&normalized) {
                chunk_heading_pages
                    .entry(normalized.to_ascii_lowercase())
                    .or_default()
                    .insert(chunk.source_title.clone());
            }
        }
    }

    // Primary signal: section headings from all comparable pages (deterministic, complete).
    let mut heading_support = BTreeMap::<String, (String, BTreeSet<String>)>::new();
    for cph in &pack.comparable_page_headings {
        let normalized = normalize_heading(&cph.section_heading);
        if normalized.is_empty() || heading_is_low_signal(&normalized) {
            continue;
        }
        let entry = heading_support
            .entry(normalized.to_ascii_lowercase())
            .or_insert_with(|| (normalized.clone(), BTreeSet::new()));
        entry.1.insert(cph.source_title.clone());
    }

    // Secondary signal: headings seen only in retrieved chunks (may come from pages
    // outside the top comparable set, preserving backward-compatible discovery).
    for chunk in &pack.chunks {
        if let Some(heading) = chunk.section_heading.as_deref() {
            let normalized = normalize_heading(heading);
            if normalized.is_empty() || heading_is_low_signal(&normalized) {
                continue;
            }
            let entry = heading_support
                .entry(normalized.to_ascii_lowercase())
                .or_insert_with(|| (normalized.clone(), BTreeSet::new()));
            entry.1.insert(chunk.source_title.clone());
        }
    }

    let min_support = if pack.related_pages.len() > 1 { 2 } else { 1 };
    let mut headings = heading_support
        .into_values()
        .filter(|(_, supporting_pages)| supporting_pages.len() >= min_support)
        .map(|(heading, supporting_pages)| {
            let key = heading.to_ascii_lowercase();
            let content_backed = chunk_heading_pages.contains_key(&key);
            let page_list: Vec<String> = supporting_pages.iter().cloned().collect();
            SectionSkeleton {
                rationale: format!(
                    "Seen on {} comparable page{}.",
                    supporting_pages.len(),
                    if supporting_pages.len() == 1 { "" } else { "s" }
                ),
                heading,
                required: false,
                content_backed,
                supporting_pages: page_list,
            }
        })
        .collect::<Vec<_>>();
    headings.sort_by(|left, right| left.heading.cmp(&right.heading));
    sections.extend(headings);
    sections.push(SectionSkeleton {
        heading: "References".to_string(),
        rationale: "Reference handling is a hard requirement for publication-quality pages."
            .to_string(),
        required: true,
        content_backed: false,
        supporting_pages: Vec::new(),
    });
    sections
}

fn build_constraints(overlay: &ProfileOverlay) -> Vec<AuthoringConstraint> {
    let mut constraints = vec![AuthoringConstraint {
        level: "must".to_string(),
        rule_id: "files-first".to_string(),
        message: "Use local wiki content and conventions as the primary fit check.".to_string(),
    }];
    if overlay.authoring.require_short_description {
        constraints.push(AuthoringConstraint {
            level: "must".to_string(),
            rule_id: "short-description".to_string(),
            message: "Add a short description before the article body.".to_string(),
        });
    }
    if overlay.authoring.require_article_quality_banner
        && let Some(template_title) = overlay.authoring.article_quality_template.as_deref()
    {
        constraints.push(AuthoringConstraint {
            level: "must".to_string(),
            rule_id: "article-quality-banner".to_string(),
            message: format!("Include {template_title} near the start of the page."),
        });
    }
    if overlay
        .authoring
        .required_appendix_sections
        .iter()
        .any(|section| section.eq_ignore_ascii_case("References"))
        && let Some(template_title) = overlay.authoring.references_template.as_deref()
    {
        constraints.push(AuthoringConstraint {
            level: "must".to_string(),
            rule_id: "references-section".to_string(),
            message: format!("Keep a References section and render it with {template_title}."),
        });
    }
    constraints
}

fn overlay_infobox_subject_type_map(overlay: &ProfileOverlay) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for preference in &overlay.remilia.infobox_preferences {
        out.insert(
            preference.template_title.to_ascii_lowercase(),
            preference.subject_type.clone(),
        );
    }
    out
}

fn collect_template_entries<I>(entries: I) -> Vec<TemplateSurfaceEntry>
where
    I: IntoIterator<Item = (String, Vec<String>, Option<String>)>,
{
    let mut out = BTreeMap::<String, TemplateSurfaceEntry>::new();
    for (template_title, supporting_pages, mapped_subject_type) in entries {
        let normalized = normalize_template_title(&template_title);
        if normalized.is_empty() {
            continue;
        }
        let key = normalized.to_ascii_lowercase();
        let entry = out.entry(key).or_insert_with(|| TemplateSurfaceEntry {
            template_title: normalized.clone(),
            source: if mapped_subject_type.is_some() {
                ContextSurfaceSource::Both
            } else {
                ContextSurfaceSource::Comparables
            },
            mapped_subject_type: mapped_subject_type.clone(),
            supporting_pages: Vec::new(),
        });
        if entry.mapped_subject_type.is_none() {
            entry.mapped_subject_type = mapped_subject_type.clone();
        }
        if mapped_subject_type.is_some() {
            entry.source = ContextSurfaceSource::Both;
        }
        extend_sorted_unique(&mut entry.supporting_pages, &supporting_pages);
    }
    out.into_values().collect()
}

fn normalize_template_title(value: &str) -> String {
    value.trim().replace('_', " ")
}

fn template_is_infobox(template_title: &str) -> bool {
    template_title
        .trim()
        .to_ascii_lowercase()
        .contains("infobox")
}

fn normalize_heading(value: &str) -> String {
    let normalized = value.trim().replace('_', " ");
    if normalized.is_empty() {
        String::new()
    } else {
        normalized
    }
}

fn heading_is_low_signal(heading: &str) -> bool {
    let lowered = heading.to_ascii_lowercase();
    [
        "references",
        "notes",
        "external links",
        "further reading",
        "bibliography",
        "gallery",
        "see also",
        "overview",
    ]
    .iter()
    .any(|value| lowered.contains(value))
}

fn dedup_sorted(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().replace('_', " "))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn extend_sorted_unique(target: &mut Vec<String>, values: &[String]) {
    target.extend(values.iter().cloned());
    target.sort();
    target.dedup();
}
