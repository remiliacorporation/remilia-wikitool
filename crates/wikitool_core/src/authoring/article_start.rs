use crate::knowledge::model::AuthoringKnowledgePackResult;

use super::classify::classify_article_type;
use super::model::{
    ArticleStartResult, AuthoringConstraint, CategoryRecommendation, EvidenceRef,
    LinkRecommendation, LocalExistenceState, LocalIntegrationLane, OpenQuestion, RecommendedAction,
    SectionSkeleton, SubjectResearchLane, TemplateRecommendation,
};

pub fn build_article_start(pack: &AuthoringKnowledgePackResult) -> ArticleStartResult {
    let article_type = classify_article_type(pack);
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

    let infobox = pack
        .suggested_templates
        .iter()
        .find(|template| {
            template
                .template_title
                .to_ascii_lowercase()
                .contains("infobox")
        })
        .map(|template| TemplateRecommendation {
            template_title: template.template_title.clone(),
            rationale: "Most similar local pages use this infobox family.".to_string(),
            confidence: 80,
            evidence: evidence.iter().take(3).cloned().collect(),
        });

    let citation_families = pack
        .suggested_references
        .iter()
        .take(3)
        .map(|reference| TemplateRecommendation {
            template_title: reference
                .common_templates
                .first()
                .cloned()
                .unwrap_or_else(|| reference.citation_family.clone()),
            rationale: format!(
                "Observed citation pattern: {} / {}",
                reference.citation_family, reference.source_type
            ),
            confidence: 70,
            evidence: evidence.iter().take(2).cloned().collect(),
        })
        .collect();

    let template_recommendations = pack
        .suggested_templates
        .iter()
        .take(5)
        .map(|template| TemplateRecommendation {
            template_title: template.template_title.clone(),
            rationale: format!(
                "Seen across {} comparable pages.",
                template.distinct_page_count
            ),
            confidence: 65,
            evidence: evidence.iter().take(2).cloned().collect(),
        })
        .collect();

    let category_candidates = pack
        .suggested_categories
        .iter()
        .take(6)
        .map(|category| CategoryRecommendation {
            category_title: category.title.clone(),
            rationale: format!("Supported by {} related pages.", category.support_count),
            confidence: u8::try_from((category.support_count * 20).min(100)).unwrap_or(100),
            evidence_titles: category.evidence_titles.clone(),
        })
        .collect();

    let link_candidates = pack
        .suggested_links
        .iter()
        .take(8)
        .map(|link| LinkRecommendation {
            page_title: link.title.clone(),
            rationale: format!("Backed by {} related sources.", link.support_count),
            confidence: u8::try_from((link.support_count * 15).min(100)).unwrap_or(100),
        })
        .collect();

    let section_skeleton = default_section_skeleton(&article_type);
    let local_integration = LocalIntegrationLane {
        comparable_pages: pack
            .related_pages
            .iter()
            .take(8)
            .map(|page| page.title.clone())
            .collect(),
        infobox,
        citation_families,
        template_recommendations,
        category_candidates,
        link_candidates,
        section_skeleton,
        docs_queries: pack
            .docs_context
            .as_ref()
            .map(|docs| docs.queries.clone())
            .unwrap_or_default(),
    };

    let constraints = vec![
        AuthoringConstraint {
            level: "must".to_string(),
            rule_id: "files-first".to_string(),
            message:
                "Use local wiki content and template conventions as the primary source of fit."
                    .to_string(),
        },
        AuthoringConstraint {
            level: "should".to_string(),
            rule_id: "preserve-raw-evidence".to_string(),
            message: "Keep raw evidence paths visible when interpreting authoring guidance."
                .to_string(),
        },
    ];

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
            reason: "No citation families were surfaced from comparable pages.".to_string(),
            blocking: true,
            evidence: evidence.iter().take(1).cloned().collect(),
        });
    }

    let next_actions = vec![
        RecommendedAction {
            label: "Review comparables".to_string(),
            why: "The related-page set shows the closest local structure and terminology."
                .to_string(),
        },
        RecommendedAction {
            label: "Draft skeleton".to_string(),
            why: "Use the suggested section layout before writing prose.".to_string(),
        },
    ];

    ArticleStartResult {
        schema_version: "article_start_v1".to_string(),
        topic: pack.topic.clone(),
        article_type,
        local_state,
        subject_research,
        local_integration,
        constraints,
        open_questions,
        next_actions,
        raw_pack_ref: Some("knowledge.pack".to_string()),
    }
}

fn default_section_skeleton(article_type: &super::model::ArticleType) -> Vec<SectionSkeleton> {
    let mut sections = vec![SectionSkeleton {
        heading: "Overview".to_string(),
        rationale: "Lead with a concise summary anchored in local terminology.".to_string(),
        required: true,
    }];
    match article_type {
        super::model::ArticleType::Person => {
            sections.push(SectionSkeleton {
                heading: "Biography".to_string(),
                rationale: "Comparable person pages normally introduce background and role."
                    .to_string(),
                required: true,
            });
        }
        super::model::ArticleType::Organization => {
            sections.push(SectionSkeleton {
                heading: "History".to_string(),
                rationale: "Organization pages usually establish formation and activity."
                    .to_string(),
                required: true,
            });
        }
        _ => {
            sections.push(SectionSkeleton {
                heading: "Background".to_string(),
                rationale: "Concept and general-topic pages benefit from historical framing."
                    .to_string(),
                required: false,
            });
        }
    }
    sections.push(SectionSkeleton {
        heading: "References".to_string(),
        rationale: "Reference handling is a hard requirement for publication-quality pages."
            .to_string(),
        required: true,
    });
    sections
}
