use std::collections::{BTreeMap, BTreeSet};

use crate::knowledge::model::AuthoringKnowledgePackResult;
use crate::profile::SiteProfile;
use crate::support::{compute_hash, compute_sha256};

use super::model::{
    ArticleEvidenceProfile, ArticleStartIntent, ArticleStartResult, CategorySurfaceEntry,
    ContextSurfaceSource, EvidenceCoverageItem, EvidenceRef, LinkSurfaceEntry, LocalExistenceState,
    LocalIntegrationLane, QueryTermCoverage, RequiredTemplate, SectionCandidate,
    SubjectResearchLane, SubjectTypeHint, TemplateSurfaceEntry,
};

#[derive(Debug, Default)]
struct ComparableScope {
    exact_title: Option<String>,
    exact_infobox_titles: BTreeSet<String>,
    /// `Some` means the exact page established an infobox type, so only pages
    /// sharing that type are peers. An empty set intentionally means that no
    /// usable comparable was found.
    peer_titles: Option<BTreeSet<String>>,
}

impl ComparableScope {
    fn allows_peer(&self, title: &str) -> bool {
        if self
            .exact_title
            .as_deref()
            .is_some_and(|exact| title.eq_ignore_ascii_case(exact))
        {
            return false;
        }
        self.peer_titles
            .as_ref()
            .map(|peers| peers.contains(&title.to_ascii_lowercase()))
            .unwrap_or(true)
    }

    fn allows_supporting_page(&self, title: &str) -> bool {
        self.exact_title
            .as_deref()
            .is_some_and(|exact| title.eq_ignore_ascii_case(exact))
            || self.allows_peer(title)
    }
}

fn build_comparable_scope(pack: &AuthoringKnowledgePackResult) -> ComparableScope {
    let Some(exact_title) = pack
        .topic_assessment
        .exact_page
        .as_ref()
        .map(|page| page.title.clone())
    else {
        return ComparableScope::default();
    };

    let exact_infoboxes = pack
        .suggested_templates
        .iter()
        .filter(|template| {
            template_is_infobox(&template.template_title)
                && template
                    .example_pages
                    .iter()
                    .any(|page| page.eq_ignore_ascii_case(&exact_title))
        })
        .collect::<Vec<_>>();
    let exact_infobox_titles = exact_infoboxes
        .iter()
        .map(|template| normalize_template_title(&template.template_title))
        .collect::<BTreeSet<_>>();
    let peer_titles = if exact_infoboxes.is_empty() {
        None
    } else {
        Some(
            exact_infoboxes
                .iter()
                .flat_map(|template| template.example_pages.iter())
                .filter(|page| !page.eq_ignore_ascii_case(&exact_title))
                .map(|page| page.to_ascii_lowercase())
                .collect(),
        )
    };

    ComparableScope {
        exact_title: Some(exact_title),
        exact_infobox_titles,
        peer_titles,
    }
}

pub fn build_article_start(
    pack: &AuthoringKnowledgePackResult,
    profile: &SiteProfile,
    intent: ArticleStartIntent,
) -> ArticleStartResult {
    let comparable_scope = build_comparable_scope(pack);
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
        .map(|chunk| {
            let evidence_identity = format!(
                "{}\0{}\0{}",
                chunk.source_relative_path,
                chunk.section_heading.as_deref().unwrap_or(""),
                chunk.chunk_text
            );
            let evidence_hash = compute_hash(&evidence_identity);
            EvidenceRef {
                id: format!("local-chunk-{}", &evidence_hash[..16]),
                source_kind: "local_chunk".to_string(),
                source_title: chunk.source_title.clone(),
                source_relative_path: chunk.source_relative_path.clone(),
                locator: chunk.section_heading.clone(),
                chunk_sha256: compute_sha256(&chunk.chunk_text),
                token_estimate: u32::try_from(chunk.token_estimate.min(u32::MAX as usize))
                    .unwrap_or(u32::MAX),
                text_preview: Some(excerpt_text(&chunk.chunk_text, EVIDENCE_PREVIEW_CHARS)),
            }
        })
        .collect::<Vec<_>>();
    let subject_research = SubjectResearchLane {
        top_local_excerpt: pack.chunks.first().map(|chunk| chunk.chunk_text.clone()),
        comparable_page_excerpts: pack
            .chunks
            .iter()
            .filter(|chunk| comparable_scope.allows_peer(&chunk.source_title))
            .take(5)
            .map(|chunk| chunk.chunk_text.clone())
            .collect(),
        citation_template_families: pack
            .suggested_references
            .iter()
            .take(5)
            .map(|reference| format!("{} / {}", reference.citation_family, reference.source_type))
            .collect(),
        ambiguity_notes: pack.stub_missing_links.clone(),
        evidence: evidence.clone(),
    };
    let evidence_profile = build_evidence_profile(pack, &evidence, &comparable_scope);

    let comparable_pages = pack
        .related_pages
        .iter()
        .filter(|page| comparable_scope.allows_peer(&page.title))
        .take(8)
        .map(|page| page.title.clone())
        .collect::<Vec<_>>();
    let contract_parameter_keys = build_contract_parameter_key_map(pack);
    let required_templates = build_required_templates(profile, &contract_parameter_keys);
    let subject_type_hints = build_subject_type_hints(pack, profile, &comparable_scope);
    let available_infoboxes =
        build_available_infoboxes(pack, profile, &contract_parameter_keys, &comparable_scope);
    let citation_templates_seen =
        build_citation_templates(pack, profile, &contract_parameter_keys, &comparable_scope);
    let template_surface = build_template_surface(pack, profile, &contract_parameter_keys);
    let observed_categories = build_category_surface(pack, &comparable_scope);
    let observed_links = build_link_surface(pack, &comparable_scope);
    let section_candidates = build_section_candidates(pack, profile, &comparable_scope);
    let contract_plan = &pack.context_summary.wiki_contract_context.traversal_plan;

    let closest_comparable_outline =
        build_closest_comparable_outline(pack, profile, &comparable_scope);
    let local_integration = LocalIntegrationLane {
        comparable_pages,
        closest_comparable_outline,
        required_templates,
        subject_type_hints,
        available_infoboxes,
        citation_templates_seen,
        template_surface,
        observed_categories,
        observed_links,
        section_candidates,
        docs_queries: build_article_start_docs_queries(pack, &comparable_scope),
        contract_query: contract_plan.query.clone(),
        contract_matched_query_terms: contract_plan.matched_query_terms.clone(),
        contract_missing_query_terms: contract_plan.missing_query_terms.clone(),
        contract_warnings: contract_plan.warnings.clone(),
    };

    ArticleStartResult {
        schema_version: "article_start_v4".to_string(),
        topic: pack.topic.clone(),
        intent,
        local_state,
        evidence_profile,
        subject_research,
        local_integration,
    }
}

fn build_evidence_profile(
    pack: &AuthoringKnowledgePackResult,
    evidence_refs: &[EvidenceRef],
    comparable_scope: &ComparableScope,
) -> ArticleEvidenceProfile {
    let query_terms = normalized_query_terms(&pack.query_terms, &pack.query);
    let exact_local_title = pack
        .topic_assessment
        .exact_page
        .as_ref()
        .map(|page| page.title.clone());

    let mut subject_context = Vec::new();
    let mut broad_context = Vec::new();
    let mut comparable_pages = Vec::new();
    let mut query_term_coverage = query_terms
        .iter()
        .map(|term| QueryTermCoverage {
            term: term.clone(),
            local_chunk_matches: 0,
            comparable_page_matches: 0,
        })
        .collect::<Vec<_>>();

    if let Some(title) = &exact_local_title {
        subject_context.push(EvidenceCoverageItem {
            source_kind: "exact_local_title".to_string(),
            source_title: title.clone(),
            locator: None,
            matched_query_terms: query_terms.clone(),
            missing_query_terms: Vec::new(),
            evidence_id: None,
        });
    }

    for (index, chunk) in pack.chunks.iter().enumerate() {
        let mut text = String::new();
        text.push_str(&chunk.source_title);
        text.push('\n');
        if let Some(heading) = chunk.section_heading.as_deref() {
            text.push_str(heading);
            text.push('\n');
        }
        text.push_str(&chunk.chunk_text);
        let matched = matched_query_terms(&text, &query_terms);
        if matched.is_empty() {
            continue;
        }
        for term in &matched {
            if let Some(coverage) = query_term_coverage
                .iter_mut()
                .find(|coverage| coverage.term == *term)
            {
                coverage.local_chunk_matches += 1;
            }
        }
        let missing = missing_query_terms(&query_terms, &matched);
        let item = EvidenceCoverageItem {
            source_kind: "local_chunk".to_string(),
            source_title: chunk.source_title.clone(),
            locator: chunk.section_heading.clone(),
            matched_query_terms: matched,
            missing_query_terms: missing,
            evidence_id: evidence_refs.get(index).map(|evidence| evidence.id.clone()),
        };
        if item.missing_query_terms.is_empty() {
            subject_context.push(item);
        } else {
            broad_context.push(item);
        }
    }

    for page in &pack.related_pages {
        if !comparable_scope.allows_peer(&page.title) {
            continue;
        }
        let text = format!("{}\n{}", page.title, page.summary);
        let matched = matched_query_terms(&text, &query_terms);
        for term in &matched {
            if let Some(coverage) = query_term_coverage
                .iter_mut()
                .find(|coverage| coverage.term == *term)
            {
                coverage.comparable_page_matches += 1;
            }
        }
        let missing = missing_query_terms(&query_terms, &matched);
        comparable_pages.push(EvidenceCoverageItem {
            source_kind: page.source.clone(),
            source_title: page.title.clone(),
            locator: None,
            matched_query_terms: matched,
            missing_query_terms: missing,
            evidence_id: None,
        });
    }

    let missing_query_terms = query_term_coverage
        .iter()
        .filter(|coverage| {
            coverage.local_chunk_matches == 0 && coverage.comparable_page_matches == 0
        })
        .map(|coverage| coverage.term.clone())
        .collect::<Vec<_>>();
    let mut missing_evidence_warnings = Vec::new();
    if exact_local_title.is_none() {
        missing_evidence_warnings
            .push("No exact local page resolved for the requested topic.".to_string());
    }
    if !query_terms.is_empty()
        && !subject_context
            .iter()
            .any(|item| item.source_kind != "exact_local_title")
    {
        missing_evidence_warnings
            .push("No returned local content chunk matched every query term.".to_string());
    }
    if !missing_query_terms.is_empty() {
        missing_evidence_warnings.push(format!(
            "These query terms were not observed in returned local evidence: {}.",
            missing_query_terms.join(", ")
        ));
    }
    if exact_local_title.is_none()
        || !missing_query_terms.is_empty()
        || !subject_context
            .iter()
            .any(|item| item.source_kind != "exact_local_title")
    {
        missing_evidence_warnings.push(
            "Live research is not run by article-start; use independent web search plus `wikitool research fetch`; use `wikitool research wiki-search` only for the configured target wiki API.".to_string(),
        );
    }

    ArticleEvidenceProfile {
        query: pack.query.clone(),
        query_terms,
        exact_local_title,
        local_title_hit_count: pack.topic_assessment.local_title_hit_count,
        backlink_count: pack.topic_assessment.backlink_count,
        subject_context,
        broad_context,
        comparable_pages,
        live_leads_status: "not_checked_by_article_start".to_string(),
        live_leads: Vec::new(),
        missing_query_terms,
        query_term_coverage,
        missing_evidence_warnings,
    }
}

fn normalized_query_terms(raw_terms: &[String], fallback_query: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for value in raw_terms {
        for token in tokenize_for_coverage(value) {
            out.insert(token);
        }
    }
    for token in tokenize_for_coverage(fallback_query) {
        out.insert(token);
    }
    out.into_iter().collect()
}

fn tokenize_for_coverage(value: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut current = String::new();
    for ch in value.chars() {
        if ch.is_alphanumeric() {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            out.insert(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.insert(current);
    }
    out
}

fn matched_query_terms(text: &str, query_terms: &[String]) -> Vec<String> {
    if query_terms.is_empty() {
        return Vec::new();
    }
    let tokens = tokenize_for_coverage(text);
    query_terms
        .iter()
        .filter(|term| tokens.contains(*term))
        .cloned()
        .collect()
}

fn missing_query_terms(query_terms: &[String], matched_terms: &[String]) -> Vec<String> {
    query_terms
        .iter()
        .filter(|term| !matched_terms.contains(*term))
        .cloned()
        .collect()
}

fn build_required_templates(
    profile: &SiteProfile,
    contract_parameter_keys: &BTreeMap<String, Vec<String>>,
) -> Vec<RequiredTemplate> {
    let mut out = Vec::new();
    if profile.authoring.require_article_quality_banner
        && let Some(template_title) = profile.authoring.article_quality_template.as_deref()
    {
        out.push(RequiredTemplate {
            template_title: template_title.to_string(),
            reason: "Required by the configured site adapter.".to_string(),
            parameter_keys: lookup_parameter_keys(contract_parameter_keys, template_title),
        });
    }
    if let Some(template_title) = profile.authoring.references_template.as_deref() {
        out.push(RequiredTemplate {
            template_title: template_title.to_string(),
            reason: "Required to render the References appendix on this wiki.".to_string(),
            parameter_keys: lookup_parameter_keys(contract_parameter_keys, template_title),
        });
    }
    out
}

/// Contract-indexed parameter keys by lowercase template title, inlined into
/// the template surfaces so agents get the parameter contract without a second
/// command round-trip.
fn build_contract_parameter_key_map(
    pack: &AuthoringKnowledgePackResult,
) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for contract in &pack
        .context_summary
        .wiki_contract_context
        .traversal_plan
        .selected_contracts
    {
        if contract.contract_kind != "template" || contract.parameter_keys.is_empty() {
            continue;
        }
        out.entry(normalize_template_title(&contract.title).to_ascii_lowercase())
            .or_insert_with(|| contract.parameter_keys.clone());
    }
    out
}

fn lookup_parameter_keys(
    contract_parameter_keys: &BTreeMap<String, Vec<String>>,
    template_title: &str,
) -> Vec<String> {
    contract_parameter_keys
        .get(&normalize_template_title(template_title).to_ascii_lowercase())
        .cloned()
        .unwrap_or_default()
}

const EVIDENCE_PREVIEW_CHARS: usize = 200;

fn excerpt_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max_chars).collect();
    format!("{}…", cut.trim_end())
}

fn build_article_start_docs_queries(
    pack: &AuthoringKnowledgePackResult,
    comparable_scope: &ComparableScope,
) -> Vec<String> {
    let existing = pack
        .docs_context
        .as_ref()
        .map(|docs| docs.queries.as_slice())
        .unwrap_or_default();
    if comparable_scope.exact_infobox_titles.is_empty() {
        return existing.to_vec();
    }

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for title in &comparable_scope.exact_infobox_titles {
        let query = title
            .split_once(':')
            .map(|(_, tail)| tail)
            .unwrap_or(title)
            .to_string();
        if seen.insert(query.to_ascii_lowercase()) {
            out.push(query);
        }
    }
    for query in existing {
        if query.to_ascii_lowercase().contains("infobox") {
            continue;
        }
        if seen.insert(query.to_ascii_lowercase()) {
            out.push(query.clone());
        }
    }
    out.truncate(4);
    out
}

/// The closest comparable page's level-2 headings in document order.
fn build_closest_comparable_outline(
    pack: &AuthoringKnowledgePackResult,
    _profile: &SiteProfile,
    comparable_scope: &ComparableScope,
) -> Option<crate::authoring::model::ComparableOutline> {
    for closest in pack
        .related_pages
        .iter()
        .filter(|page| comparable_scope.allows_peer(&page.title))
    {
        let ordered_headings = pack
            .comparable_page_headings
            .iter()
            .filter(|heading| heading.source_title == closest.title)
            .map(|heading| normalize_heading(&heading.section_heading))
            .filter(|heading| !heading.is_empty() && !heading_is_low_signal(heading))
            .collect::<Vec<_>>();
        if !ordered_headings.is_empty() {
            return Some(crate::authoring::model::ComparableOutline {
                title: closest.title.clone(),
                ordered_headings,
            });
        }
    }
    None
}

fn build_subject_type_hints(
    pack: &AuthoringKnowledgePackResult,
    profile: &SiteProfile,
    comparable_scope: &ComparableScope,
) -> Vec<SubjectTypeHint> {
    let mut hints = BTreeMap::<String, (BTreeSet<String>, BTreeSet<String>)>::new();
    for template in &pack.suggested_templates {
        let template_title = normalize_template_title(&template.template_title);
        if !template_is_infobox(&template_title) {
            continue;
        }
        if !comparable_scope.exact_infobox_titles.is_empty()
            && !comparable_scope
                .exact_infobox_titles
                .iter()
                .any(|exact| exact.eq_ignore_ascii_case(&template_title))
        {
            continue;
        }
        for preference in &profile.templates.infobox_preferences {
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
    profile: &SiteProfile,
    contract_parameter_keys: &BTreeMap<String, Vec<String>>,
    comparable_scope: &ComparableScope,
) -> Vec<TemplateSurfaceEntry> {
    let profile_mappings = profile_infobox_subject_type_map(profile);
    let mut out = collect_template_entries(
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
    );
    for contract in &pack
        .context_summary
        .wiki_contract_context
        .traversal_plan
        .selected_contracts
    {
        if contract.contract_kind != "template"
            || !(contract.category == "infobox" || template_is_infobox(&contract.title))
        {
            continue;
        }
        let normalized = normalize_template_title(&contract.title);
        if out
            .iter()
            .any(|entry| entry.template_title.eq_ignore_ascii_case(&normalized))
        {
            continue;
        }
        out.push(TemplateSurfaceEntry {
            template_title: normalized.clone(),
            source: ContextSurfaceSource::ContractTraversal,
            mapped_subject_type: profile_mappings
                .get(&normalized.to_ascii_lowercase())
                .cloned(),
            supporting_pages: dedup_sorted(contract.example_titles.clone()),
            parameter_keys: contract.parameter_keys.clone(),
        });
    }
    for entry in &mut out {
        if entry.parameter_keys.is_empty() {
            entry.parameter_keys =
                lookup_parameter_keys(contract_parameter_keys, &entry.template_title);
        }
    }
    if !comparable_scope.exact_infobox_titles.is_empty() {
        out.retain(|entry| {
            comparable_scope
                .exact_infobox_titles
                .iter()
                .any(|exact| exact.eq_ignore_ascii_case(&entry.template_title))
        });
    }
    let exact_title = pack
        .topic_assessment
        .exact_page
        .as_ref()
        .map(|page| page.title.as_str());
    out.sort_by(|left, right| {
        let left_exact = exact_title.is_some_and(|title| {
            left.supporting_pages
                .iter()
                .any(|page| page.eq_ignore_ascii_case(title))
        });
        let right_exact = exact_title.is_some_and(|title| {
            right
                .supporting_pages
                .iter()
                .any(|page| page.eq_ignore_ascii_case(title))
        });
        right_exact
            .cmp(&left_exact)
            .then_with(|| left.template_title.cmp(&right.template_title))
    });
    out
}

fn build_citation_templates(
    pack: &AuthoringKnowledgePackResult,
    profile: &SiteProfile,
    contract_parameter_keys: &BTreeMap<String, Vec<String>>,
    comparable_scope: &ComparableScope,
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
                source: ContextSurfaceSource::Comparables,
                mapped_subject_type: None,
                supporting_pages: Vec::new(),
                parameter_keys: lookup_parameter_keys(contract_parameter_keys, &template_title),
                template_title: template_title.clone(),
            });
        let supporting_pages = reference
            .example_pages
            .iter()
            .filter(|page| comparable_scope.allows_supporting_page(page))
            .cloned()
            .collect::<Vec<_>>();
        extend_sorted_unique(&mut entry.supporting_pages, &supporting_pages);
    }

    for rule in &profile.citations.preferred_templates {
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
                parameter_keys: lookup_parameter_keys(
                    contract_parameter_keys,
                    &rule.template_title,
                ),
            },
        );
    }

    comparable_entries
        .into_values()
        .filter(|entry| {
            !entry.supporting_pages.is_empty()
                || matches!(
                    entry.source,
                    ContextSurfaceSource::Profile | ContextSurfaceSource::Both
                )
        })
        .collect()
}

fn build_template_surface(
    pack: &AuthoringKnowledgePackResult,
    profile: &SiteProfile,
    contract_parameter_keys: &BTreeMap<String, Vec<String>>,
) -> Vec<TemplateSurfaceEntry> {
    let profile_templates = profile
        .profile_template_titles()
        .into_iter()
        .map(|title| title.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut out = pack
        .suggested_templates
        .iter()
        .filter(|template| !template_is_infobox(&template.template_title))
        .map(|template| {
            let template_title = normalize_template_title(&template.template_title);
            TemplateSurfaceEntry {
                source: if profile_templates.contains(&template.template_title.to_ascii_lowercase())
                {
                    ContextSurfaceSource::Both
                } else {
                    ContextSurfaceSource::Comparables
                },
                mapped_subject_type: None,
                supporting_pages: dedup_sorted(template.example_pages.clone()),
                parameter_keys: lookup_parameter_keys(contract_parameter_keys, &template_title),
                template_title,
            }
        })
        .collect::<Vec<_>>();
    for contract in &pack
        .context_summary
        .wiki_contract_context
        .traversal_plan
        .selected_contracts
    {
        if contract.contract_kind != "template" || template_is_infobox(&contract.title) {
            continue;
        }
        out.push(TemplateSurfaceEntry {
            template_title: normalize_template_title(&contract.title),
            source: if profile_templates.contains(&contract.title.to_ascii_lowercase()) {
                ContextSurfaceSource::Both
            } else {
                ContextSurfaceSource::ContractTraversal
            },
            mapped_subject_type: None,
            supporting_pages: dedup_sorted(contract.example_titles.clone()),
            parameter_keys: contract.parameter_keys.clone(),
        });
    }
    out.sort_by(|left, right| left.template_title.cmp(&right.template_title));
    out.dedup_by(|left, right| {
        left.template_title
            .eq_ignore_ascii_case(&right.template_title)
    });
    out
}

fn build_category_surface(
    pack: &AuthoringKnowledgePackResult,
    comparable_scope: &ComparableScope,
) -> Vec<CategorySurfaceEntry> {
    let exact_title = comparable_scope.exact_title.as_deref();
    let mut out = pack
        .suggested_categories
        .iter()
        .filter_map(|category| {
            let supporting_pages = dedup_sorted(
                category
                    .evidence_titles
                    .iter()
                    .filter(|page| comparable_scope.allows_supporting_page(page))
                    .cloned()
                    .collect(),
            );
            if supporting_pages.is_empty() {
                return None;
            }
            let exact_supported = exact_title.is_some_and(|title| {
                supporting_pages
                    .iter()
                    .any(|page| page.eq_ignore_ascii_case(title))
            });
            let source = if exact_supported && supporting_pages.len() > 1 {
                ContextSurfaceSource::ExactPageAndComparables
            } else if exact_supported {
                ContextSurfaceSource::ExactPage
            } else {
                ContextSurfaceSource::Comparables
            };
            Some(CategorySurfaceEntry {
                category_title: category.title.clone(),
                source,
                supporting_pages,
            })
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| left.category_title.cmp(&right.category_title));
    out
}

fn build_link_surface(
    pack: &AuthoringKnowledgePackResult,
    comparable_scope: &ComparableScope,
) -> Vec<LinkSurfaceEntry> {
    let exact_title = comparable_scope.exact_title.as_deref();
    let mut out = pack
        .suggested_links
        .iter()
        .filter_map(|link| {
            let supporting_pages = dedup_sorted(
                link.evidence_titles
                    .iter()
                    .filter(|page| comparable_scope.allows_supporting_page(page))
                    .cloned()
                    .collect(),
            );
            if supporting_pages.is_empty() {
                return None;
            }
            let exact_supported = exact_title.is_some_and(|title| {
                supporting_pages
                    .iter()
                    .any(|page| page.eq_ignore_ascii_case(title))
            });
            let source = if exact_supported && supporting_pages.len() > 1 {
                ContextSurfaceSource::ExactPageAndComparables
            } else if exact_supported {
                ContextSurfaceSource::ExactPage
            } else {
                ContextSurfaceSource::Comparables
            };
            Some(LinkSurfaceEntry {
                page_title: link.title.clone(),
                source,
                supporting_pages,
            })
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| left.page_title.cmp(&right.page_title));
    out
}

fn build_section_candidates(
    pack: &AuthoringKnowledgePackResult,
    profile: &SiteProfile,
    comparable_scope: &ComparableScope,
) -> Vec<SectionCandidate> {
    let mut sections = Vec::new();

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
        if !comparable_scope.allows_peer(&cph.source_title) {
            continue;
        }
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
        if !comparable_scope.allows_peer(&chunk.source_title) {
            continue;
        }
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

    // Preserve document order: rank each heading by its earliest position on the
    // closest comparable page that carries it, so the skeleton reads like a real
    // article outline instead of an alphabetized set.
    let mut page_rank = BTreeMap::<String, usize>::new();
    for (rank, page) in pack.related_pages.iter().enumerate() {
        if !comparable_scope.allows_peer(&page.title) {
            continue;
        }
        page_rank.entry(page.title.clone()).or_insert(rank);
    }
    let mut heading_order = BTreeMap::<String, (usize, usize)>::new();
    let mut per_page_position = BTreeMap::<String, usize>::new();
    for cph in &pack.comparable_page_headings {
        if !comparable_scope.allows_peer(&cph.source_title) {
            continue;
        }
        let normalized = normalize_heading(&cph.section_heading);
        if normalized.is_empty() || heading_is_low_signal(&normalized) {
            continue;
        }
        let position_entry = per_page_position
            .entry(cph.source_title.clone())
            .or_insert(0);
        let position = *position_entry;
        *position_entry += 1;
        let rank = page_rank
            .get(&cph.source_title)
            .copied()
            .unwrap_or(usize::MAX);
        let key = normalized.to_ascii_lowercase();
        let candidate = (rank, position);
        heading_order
            .entry(key)
            .and_modify(|existing| {
                if candidate < *existing {
                    *existing = candidate;
                }
            })
            .or_insert(candidate);
    }

    let comparable_count = pack
        .related_pages
        .iter()
        .filter(|page| comparable_scope.allows_peer(&page.title))
        .count();
    let min_support = if comparable_count > 1 { 2 } else { 1 };
    let mut headings = heading_support
        .into_values()
        .filter(|(_, supporting_pages)| supporting_pages.len() >= min_support)
        .map(|(heading, supporting_pages)| {
            let key = heading.to_ascii_lowercase();
            let content_backed = chunk_heading_pages.contains_key(&key);
            let page_list: Vec<String> = supporting_pages.iter().cloned().collect();
            SectionCandidate {
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
    headings.sort_by(|left, right| {
        let left_order = heading_order
            .get(&left.heading.to_ascii_lowercase())
            .copied()
            .unwrap_or((usize::MAX, usize::MAX));
        let right_order = heading_order
            .get(&right.heading.to_ascii_lowercase())
            .copied()
            .unwrap_or((usize::MAX, usize::MAX));
        left_order
            .cmp(&right_order)
            .then_with(|| left.heading.cmp(&right.heading))
    });
    sections.extend(headings);
    if profile
        .authoring
        .required_appendix_sections
        .iter()
        .any(|section| section.eq_ignore_ascii_case("References"))
    {
        sections.push(SectionCandidate {
            heading: "References".to_string(),
            rationale: "Required by the configured site adapter.".to_string(),
            required: true,
            content_backed: false,
            supporting_pages: Vec::new(),
        });
    }
    sections
}

fn profile_infobox_subject_type_map(profile: &SiteProfile) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for preference in &profile.templates.infobox_preferences {
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
            parameter_keys: Vec::new(),
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
