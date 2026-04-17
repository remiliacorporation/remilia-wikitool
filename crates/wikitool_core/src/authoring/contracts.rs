use std::collections::{BTreeMap, BTreeSet};

use crate::content_store::parsing;
use crate::docs::{DocsContextExample, DocsContextSection, DocsSymbolHit};
use crate::knowledge::model::{
    AuthoringContextSummary, AuthoringContractEdge, AuthoringContractTraversalPlan,
    AuthoringKnowledgePackResult, AuthoringPageCandidate, AuthoringPayloadMode,
    AuthoringSourceCount, AuthoringSubjectContext, AuthoringTopicAssessment,
    AuthoringWikiContractContext, ModuleContractSummary, ModuleUsageSummary,
    TemplateContractSummary, TemplateReference, TemplateUsageSummary,
};
use crate::knowledge::retrieval::RetrievedChunk;

pub(crate) struct AuthoringContextSummaryInputs<'a> {
    pub(crate) topic_assessment: &'a AuthoringTopicAssessment,
    pub(crate) related_pages: &'a [AuthoringPageCandidate],
    pub(crate) chunks: &'a [RetrievedChunk],
    pub(crate) stub_missing_links: &'a [String],
    pub(crate) suggested_templates: &'a [TemplateUsageSummary],
    pub(crate) template_baseline: &'a [TemplateUsageSummary],
    pub(crate) template_references: &'a [TemplateReference],
    pub(crate) module_patterns: &'a [ModuleUsageSummary],
    pub(crate) docs_context: Option<&'a crate::knowledge::model::AuthoringDocsContext>,
    pub(crate) contract_plan: AuthoringContractTraversalPlan,
}

pub(crate) fn build_context_summary(
    inputs: AuthoringContextSummaryInputs<'_>,
) -> AuthoringContextSummary {
    let subject_context = build_subject_context(
        inputs.topic_assessment,
        inputs.related_pages,
        inputs.chunks,
        inputs.stub_missing_links,
    );
    let wiki_contract_context = build_wiki_contract_context(
        inputs.suggested_templates,
        inputs.template_baseline,
        inputs.template_references,
        inputs.module_patterns,
        inputs.docs_context,
        inputs.contract_plan,
    );

    AuthoringContextSummary {
        subject_context,
        wiki_contract_context,
    }
}

pub(crate) fn apply_payload_mode(report: &mut AuthoringKnowledgePackResult) {
    if report.payload_mode == AuthoringPayloadMode::Full {
        refresh_contract_token_estimate(&mut report.context_summary.wiki_contract_context);
        return;
    }

    for template in &mut report.suggested_templates {
        compact_template_usage_summary(template);
    }
    for template in &mut report.template_baseline {
        compact_template_usage_summary(template);
    }
    for reference in &mut report.template_references {
        compact_template_reference(reference);
    }
    for module in &mut report.module_patterns {
        compact_module_summary(module);
    }
    if let Some(docs_context) = report.docs_context.as_mut() {
        compact_docs_context(docs_context);
    }

    let omitted = &mut report.context_summary.wiki_contract_context.omitted_detail;
    push_unique(
        omitted,
        "template implementation chunks omitted; use `--payload full` or `knowledge inspect templates TEMPLATE --format json` for full source context",
    );
    push_unique(
        omitted,
        "docs section/example bodies omitted; use `docs context` or `--payload full` for expanded technical context",
    );
    refresh_contract_token_estimate(&mut report.context_summary.wiki_contract_context);
}

fn build_subject_context(
    topic_assessment: &AuthoringTopicAssessment,
    related_pages: &[AuthoringPageCandidate],
    chunks: &[RetrievedChunk],
    stub_missing_links: &[String],
) -> AuthoringSubjectContext {
    let mut source_counts = BTreeMap::<String, usize>::new();
    for page in related_pages {
        *source_counts.entry(page.source.clone()).or_insert(0) += 1;
    }

    AuthoringSubjectContext {
        exact_page_available: topic_assessment.exact_page.is_some(),
        local_title_hit_count: topic_assessment.local_title_hit_count,
        backlink_count: topic_assessment.backlink_count,
        related_page_count: related_pages.len(),
        related_page_source_counts: source_counts
            .into_iter()
            .map(|(source, count)| AuthoringSourceCount { source, count })
            .collect(),
        retrieved_chunk_count: chunks.len(),
        retrieved_chunk_token_estimate: chunks.iter().map(|chunk| chunk.token_estimate).sum(),
        missing_stub_link_count: stub_missing_links.len(),
    }
}

fn build_wiki_contract_context(
    suggested_templates: &[TemplateUsageSummary],
    template_baseline: &[TemplateUsageSummary],
    template_references: &[TemplateReference],
    module_patterns: &[ModuleUsageSummary],
    docs_context: Option<&crate::knowledge::model::AuthoringDocsContext>,
    contract_plan: AuthoringContractTraversalPlan,
) -> AuthoringWikiContractContext {
    let reference_by_title = template_references
        .iter()
        .map(|reference| {
            (
                reference.template.template_title.to_ascii_lowercase(),
                reference,
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut seen_templates = BTreeSet::new();
    let mut template_contracts = Vec::new();
    for template in suggested_templates.iter().chain(template_baseline.iter()) {
        let key = template.template_title.to_ascii_lowercase();
        if !seen_templates.insert(key.clone()) {
            continue;
        }
        template_contracts.push(template_contract_summary(
            template,
            reference_by_title.get(&key).copied(),
        ));
    }
    for reference in template_references {
        let key = reference.template.template_title.to_ascii_lowercase();
        if seen_templates.insert(key) {
            template_contracts.push(template_contract_summary(
                &reference.template,
                Some(reference),
            ));
        }
    }

    let referenced_by_module = templates_by_module(template_references);
    let module_contracts = module_patterns
        .iter()
        .map(|module| {
            module_contract_summary(
                module,
                referenced_by_module
                    .get(&module.module_title.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    let contract_edges = template_module_edges(template_references);
    let docs_queries = docs_context
        .map(|context| context.queries.clone())
        .unwrap_or_default();

    let mut context = AuthoringWikiContractContext {
        template_contracts,
        module_contracts,
        docs_queries,
        contract_edges,
        traversal_plan: contract_plan,
        omitted_detail: Vec::new(),
        token_estimate_total: 0,
    };
    refresh_contract_token_estimate(&mut context);
    context
}

fn template_contract_summary(
    template: &TemplateUsageSummary,
    reference: Option<&TemplateReference>,
) -> TemplateContractSummary {
    let module_titles = reference
        .map(|reference| {
            reference
                .implementation_pages
                .iter()
                .filter(|page| page.role == "module")
                .map(|page| page.page_title.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let summary_text = reference.and_then(|reference| {
        reference
            .implementation_pages
            .iter()
            .map(|page| page.summary_text.trim())
            .find(|value| !value.is_empty())
            .map(|value| truncate_chars(value, 420))
    });

    TemplateContractSummary {
        template_title: template.template_title.clone(),
        summary_text,
        usage_count: template.usage_count,
        distinct_page_count: template.distinct_page_count,
        parameter_keys: template
            .parameter_stats
            .iter()
            .map(|parameter| parameter.key.clone())
            .collect(),
        implementation_titles: template.implementation_titles.clone(),
        module_titles,
        example_pages: template.example_pages.clone(),
        example_invocations: template
            .example_invocations
            .iter()
            .take(2)
            .cloned()
            .collect(),
    }
}

fn module_contract_summary(
    module: &ModuleUsageSummary,
    referenced_by_templates: Vec<String>,
) -> ModuleContractSummary {
    ModuleContractSummary {
        module_title: module.module_title.clone(),
        usage_count: module.usage_count,
        distinct_page_count: module.distinct_page_count,
        functions: module.function_stats.clone(),
        example_pages: module.example_pages.clone(),
        referenced_by_templates,
        example_invocations: module.example_invocations.iter().take(2).cloned().collect(),
    }
}

fn templates_by_module(template_references: &[TemplateReference]) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::<String, BTreeSet<String>>::new();
    for reference in template_references {
        for page in reference
            .implementation_pages
            .iter()
            .filter(|page| page.role == "module")
        {
            out.entry(page.page_title.to_ascii_lowercase())
                .or_default()
                .insert(reference.template.template_title.clone());
        }
    }
    out.into_iter()
        .map(|(module, templates)| (module, templates.into_iter().collect()))
        .collect()
}

fn template_module_edges(template_references: &[TemplateReference]) -> Vec<AuthoringContractEdge> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for reference in template_references {
        for page in reference
            .implementation_pages
            .iter()
            .filter(|page| page.role == "module")
        {
            let key = format!(
                "{}\0{}",
                reference.template.template_title.to_ascii_lowercase(),
                page.page_title.to_ascii_lowercase()
            );
            if !seen.insert(key) {
                continue;
            }
            out.push(AuthoringContractEdge {
                from_title: reference.template.template_title.clone(),
                from_kind: "template".to_string(),
                to_title: page.page_title.clone(),
                to_kind: "module".to_string(),
                relation: "implemented_by".to_string(),
            });
        }
    }
    out
}

fn compact_template_reference(reference: &mut TemplateReference) {
    compact_template_usage_summary(&mut reference.template);
    for page in &mut reference.implementation_pages {
        page.summary_text = truncate_chars(&page.summary_text, 420);
        page.section_summaries.truncate(6);
        for section in &mut page.section_summaries {
            section.summary_text = truncate_chars(&section.summary_text, 240);
            section.token_estimate = parsing::estimate_tokens(&section.summary_text);
        }
        page.context_chunks.clear();
    }
    reference.implementation_sections.truncate(8);
    for section in &mut reference.implementation_sections {
        section.summary_text = truncate_chars(&section.summary_text, 240);
        section.token_estimate = parsing::estimate_tokens(&section.summary_text);
    }
    reference.implementation_chunks.clear();
}

fn compact_template_usage_summary(template: &mut TemplateUsageSummary) {
    template.implementation_preview = None;
    for parameter in &mut template.parameter_stats {
        parameter.example_values.truncate(2);
    }
    template.example_invocations.truncate(2);
}

fn compact_module_summary(module: &mut ModuleUsageSummary) {
    module.example_invocations.truncate(2);
}

fn compact_docs_context(context: &mut crate::knowledge::model::AuthoringDocsContext) {
    context.pages.truncate(4);
    context.sections.truncate(4);
    for section in &mut context.sections {
        compact_docs_section(section);
    }
    context.symbols.truncate(4);
    for symbol in &mut context.symbols {
        compact_docs_symbol(symbol);
    }
    context.examples.truncate(2);
    for example in &mut context.examples {
        compact_docs_example(example);
    }
    context.token_estimate_total = context
        .sections
        .iter()
        .map(|section| section.token_estimate)
        .sum::<usize>()
        + context
            .examples
            .iter()
            .map(|example| example.token_estimate)
            .sum::<usize>()
        + context
            .symbols
            .iter()
            .map(|symbol| parsing::estimate_tokens(&symbol.summary_text))
            .sum::<usize>();
}

fn compact_docs_section(section: &mut DocsContextSection) {
    section.summary_text = truncate_chars(&section.summary_text, 300);
    section.section_text.clear();
    section.token_estimate = parsing::estimate_tokens(&section.summary_text);
}

fn compact_docs_symbol(symbol: &mut DocsSymbolHit) {
    symbol.summary_text = truncate_chars(&symbol.summary_text, 240);
    symbol.detail_text.clear();
}

fn compact_docs_example(example: &mut DocsContextExample) {
    example.summary_text = truncate_chars(&example.summary_text, 240);
    example.example_text.clear();
    example.token_estimate = parsing::estimate_tokens(&example.summary_text);
}

fn refresh_contract_token_estimate(context: &mut AuthoringWikiContractContext) {
    context.token_estimate_total = estimate_wiki_contract_context_tokens(context);
}

fn estimate_wiki_contract_context_tokens(context: &AuthoringWikiContractContext) -> usize {
    let template_tokens = context
        .template_contracts
        .iter()
        .map(|template| {
            parsing::estimate_tokens(&template.template_title)
                + template
                    .summary_text
                    .as_deref()
                    .map(parsing::estimate_tokens)
                    .unwrap_or(0)
                + template
                    .parameter_keys
                    .iter()
                    .map(|key| parsing::estimate_tokens(key))
                    .sum::<usize>()
                + template
                    .implementation_titles
                    .iter()
                    .map(|title| parsing::estimate_tokens(title))
                    .sum::<usize>()
                + template
                    .module_titles
                    .iter()
                    .map(|title| parsing::estimate_tokens(title))
                    .sum::<usize>()
        })
        .sum::<usize>();
    let module_tokens = context
        .module_contracts
        .iter()
        .map(|module| {
            parsing::estimate_tokens(&module.module_title)
                + module
                    .functions
                    .iter()
                    .map(|function| {
                        parsing::estimate_tokens(&function.function_name)
                            + function
                                .example_parameter_keys
                                .iter()
                                .map(|key| parsing::estimate_tokens(key))
                                .sum::<usize>()
                    })
                    .sum::<usize>()
                + module
                    .referenced_by_templates
                    .iter()
                    .map(|title| parsing::estimate_tokens(title))
                    .sum::<usize>()
        })
        .sum::<usize>();
    let docs_tokens = context
        .docs_queries
        .iter()
        .map(|query| parsing::estimate_tokens(query))
        .sum::<usize>();
    let edge_tokens = context
        .contract_edges
        .iter()
        .map(|edge| {
            parsing::estimate_tokens(&edge.from_title)
                + parsing::estimate_tokens(&edge.to_title)
                + parsing::estimate_tokens(&edge.relation)
        })
        .sum::<usize>();
    let traversal_tokens = context.traversal_plan.token_estimate_total;

    template_tokens
        .saturating_add(module_tokens)
        .saturating_add(docs_tokens)
        .saturating_add(edge_tokens)
        .saturating_add(traversal_tokens)
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut out = value.chars().take(limit).collect::<String>();
    out.push_str("...");
    out
}
