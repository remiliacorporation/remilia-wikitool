use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use rusqlite::{Connection, params, params_from_iter};

use crate::content_store::parsing::{
    estimate_tokens, fts_table_exists, normalize_spaces, parse_string_list,
};
use crate::knowledge::model::{
    AuthoringContractEdge, AuthoringContractExpansionHint, AuthoringContractNode,
    AuthoringContractOmission, AuthoringContractProfile, AuthoringContractSelectionReason,
    AuthoringContractTraversalPlan, AuthoringPageCandidate, ModuleUsageSummary, StubTemplateHint,
    TemplateReference, TemplateUsageSummary,
};
use crate::profile::{
    TemplateCatalog, TemplateCatalogEntry, build_template_catalog_with_overlay,
    load_latest_template_catalog, load_or_build_remilia_profile_overlay,
};
use crate::runtime::ResolvedPaths;
use crate::support::table_exists;

const CONTRACT_PLAN_SCHEMA_VERSION: &str = "authoring_contract_traversal_v1";
const CONTRACT_INDEX_SCAN_LIMIT: usize = 4096;

#[derive(Debug, Clone)]
pub struct AuthoringContractPlanOptions {
    pub query: String,
    pub query_terms: Vec<String>,
    pub stub_detected_templates: Vec<StubTemplateHint>,
    pub related_pages: Vec<AuthoringPageCandidate>,
    pub suggested_templates: Vec<TemplateUsageSummary>,
    pub template_baseline: Vec<TemplateUsageSummary>,
    pub template_references: Vec<TemplateReference>,
    pub module_patterns: Vec<ModuleUsageSummary>,
    pub limit: usize,
    pub token_budget: usize,
    pub profile: AuthoringContractProfile,
}

impl Default for AuthoringContractPlanOptions {
    fn default() -> Self {
        Self {
            query: String::new(),
            query_terms: Vec::new(),
            stub_detected_templates: Vec::new(),
            related_pages: Vec::new(),
            suggested_templates: Vec::new(),
            template_baseline: Vec::new(),
            template_references: Vec::new(),
            module_patterns: Vec::new(),
            limit: 16,
            token_budget: 900,
            profile: AuthoringContractProfile::Author,
        }
    }
}

#[derive(Debug, Clone)]
struct ContractIndexRecord {
    key: String,
    kind: String,
    title: String,
    category: String,
    summary_text: String,
    usage_count: usize,
    distinct_page_count: usize,
    parameter_keys: Vec<String>,
    function_names: Vec<String>,
    module_titles: Vec<String>,
    example_titles: Vec<String>,
    semantic_text: String,
}

#[derive(Debug, Clone)]
struct Candidate {
    record: ContractIndexRecord,
    score: usize,
    reasons: Vec<AuthoringContractSelectionReason>,
    matched_query_terms: BTreeSet<String>,
}

pub fn query_authoring_contract_plan(
    paths: &ResolvedPaths,
    options: AuthoringContractPlanOptions,
) -> Result<AuthoringContractTraversalPlan> {
    let connection = crate::content_store::parsing::open_indexed_connection(paths)?;
    let overlay = load_or_build_remilia_profile_overlay(paths)?;
    if let Some(connection) = connection.as_ref() {
        let fallback_catalog = match load_latest_template_catalog(paths)? {
            Some(catalog) => Some(catalog),
            None => Some(build_template_catalog_with_overlay(paths, &overlay)?),
        };
        build_authoring_contract_plan_for_connection(
            connection,
            &overlay.profile_id,
            &options,
            fallback_catalog.as_ref(),
        )
    } else {
        let catalog = match load_latest_template_catalog(paths)? {
            Some(catalog) => catalog,
            None => build_template_catalog_with_overlay(paths, &overlay)?,
        };
        build_authoring_contract_plan_from_catalog(&overlay.profile_id, &catalog, &options)
    }
}

pub(crate) fn build_authoring_contract_plan_for_connection(
    connection: &Connection,
    profile_id: &str,
    options: &AuthoringContractPlanOptions,
    fallback_catalog: Option<&TemplateCatalog>,
) -> Result<AuthoringContractTraversalPlan> {
    let mut records = load_indexed_contract_records(connection, profile_id, options)?;
    if records.is_empty()
        && let Some(catalog) = fallback_catalog
    {
        records = contract_records_from_catalog(profile_id, catalog);
    }

    let mut candidates = score_contract_candidates(records, options);
    if candidates.is_empty() {
        candidates = fallback_contract_candidates(options);
    }

    let selected_keys = candidates
        .iter()
        .map(|candidate| candidate.record.key.clone())
        .collect::<BTreeSet<_>>();
    let edges = load_contract_edges(connection, profile_id, &selected_keys)
        .unwrap_or_else(|_| fallback_edges_from_template_references(&options.template_references));

    Ok(materialize_plan(options, candidates, edges))
}

fn build_authoring_contract_plan_from_catalog(
    profile_id: &str,
    catalog: &TemplateCatalog,
    options: &AuthoringContractPlanOptions,
) -> Result<AuthoringContractTraversalPlan> {
    let records = contract_records_from_catalog(profile_id, catalog);
    let mut candidates = score_contract_candidates(records, options);
    if candidates.is_empty() {
        candidates = fallback_contract_candidates(options);
    }
    Ok(materialize_plan(
        options,
        candidates,
        fallback_edges_from_template_references(&options.template_references),
    ))
}

fn load_indexed_contract_records(
    connection: &Connection,
    profile_id: &str,
    options: &AuthoringContractPlanOptions,
) -> Result<Vec<ContractIndexRecord>> {
    if !table_exists(connection, "indexed_authoring_contracts")? {
        return Ok(Vec::new());
    }

    let mut records = BTreeMap::<String, ContractIndexRecord>::new();
    if fts_table_exists(connection, "indexed_authoring_contracts_fts") {
        let terms = normalized_terms(&options.query_terms, &options.query);
        for token in fts_tokens(&terms) {
            for record in query_contract_records_by_fts(connection, profile_id, &token)? {
                records.entry(record.key.clone()).or_insert(record);
                if records.len() >= CONTRACT_INDEX_SCAN_LIMIT {
                    break;
                }
            }
        }
    }

    if records.len() < options.limit.saturating_mul(4).max(32) {
        for record in query_contract_records_by_usage(
            connection,
            profile_id,
            CONTRACT_INDEX_SCAN_LIMIT.saturating_sub(records.len()),
        )? {
            records.entry(record.key.clone()).or_insert(record);
            if records.len() >= CONTRACT_INDEX_SCAN_LIMIT {
                break;
            }
        }
    }

    Ok(records.into_values().collect())
}

fn query_contract_records_by_fts(
    connection: &Connection,
    profile_id: &str,
    token: &str,
) -> Result<Vec<ContractIndexRecord>> {
    if token.is_empty() {
        return Ok(Vec::new());
    }
    let fts_query = format!("{token}*");
    let mut statement = connection
        .prepare(
            "SELECT c.contract_key, c.contract_kind, c.title, c.category, c.summary_text,
                    c.usage_count, c.distinct_page_count, c.parameter_keys, c.function_names,
                    c.module_titles, c.example_titles, c.semantic_text
             FROM indexed_authoring_contracts_fts fts
             JOIN indexed_authoring_contracts c ON c.rowid = fts.rowid
             WHERE indexed_authoring_contracts_fts MATCH ?1
               AND c.profile = ?2
             ORDER BY bm25(indexed_authoring_contracts_fts) ASC, c.usage_count DESC, c.title ASC
             LIMIT 256",
        )
        .context("failed to prepare authoring contract FTS query")?;
    let rows = statement
        .query_map(params![fts_query, profile_id], decode_contract_record_row)
        .context("failed to run authoring contract FTS query")?;
    collect_contract_rows(rows)
}

fn query_contract_records_by_usage(
    connection: &Connection,
    profile_id: &str,
    limit: usize,
) -> Result<Vec<ContractIndexRecord>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare(
            "SELECT contract_key, contract_kind, title, category, summary_text,
                    usage_count, distinct_page_count, parameter_keys, function_names,
                    module_titles, example_titles, semantic_text
             FROM indexed_authoring_contracts
             WHERE profile = ?1
             ORDER BY
               CASE contract_kind WHEN 'template' THEN 0 WHEN 'module' THEN 1 ELSE 2 END,
               usage_count DESC,
               distinct_page_count DESC,
               title ASC
             LIMIT ?2",
        )
        .context("failed to prepare authoring contract usage query")?;
    let rows = statement
        .query_map(
            params![profile_id, i64::try_from(limit).unwrap_or(i64::MAX)],
            decode_contract_record_row,
        )
        .context("failed to run authoring contract usage query")?;
    collect_contract_rows(rows)
}

fn collect_contract_rows<F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<ContractIndexRecord>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<ContractIndexRecord>,
{
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode authoring contract row")?);
    }
    Ok(out)
}

fn decode_contract_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContractIndexRecord> {
    let usage_count: i64 = row.get(5)?;
    let distinct_page_count: i64 = row.get(6)?;
    Ok(ContractIndexRecord {
        key: row.get(0)?,
        kind: row.get(1)?,
        title: row.get(2)?,
        category: row.get(3)?,
        summary_text: row.get(4)?,
        usage_count: usize::try_from(usage_count).unwrap_or(0),
        distinct_page_count: usize::try_from(distinct_page_count).unwrap_or(0),
        parameter_keys: parse_string_list(&row.get::<_, String>(7)?),
        function_names: parse_string_list(&row.get::<_, String>(8)?),
        module_titles: parse_string_list(&row.get::<_, String>(9)?),
        example_titles: parse_string_list(&row.get::<_, String>(10)?),
        semantic_text: row.get(11)?,
    })
}

fn load_contract_edges(
    connection: &Connection,
    profile_id: &str,
    selected_keys: &BTreeSet<String>,
) -> Result<Vec<AuthoringContractEdge>> {
    if selected_keys.is_empty() || !table_exists(connection, "indexed_authoring_contract_edges")? {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", selected_keys.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT from_title, from_kind, to_title, to_kind, relation
         FROM indexed_authoring_contract_edges
         WHERE profile = ?
           AND (from_contract_key IN ({placeholders}) OR to_contract_key IN ({placeholders}))
         ORDER BY from_title ASC, relation ASC, to_title ASC"
    );
    let mut values = Vec::new();
    values.push(rusqlite::types::Value::from(profile_id.to_string()));
    for key in selected_keys {
        values.push(rusqlite::types::Value::from(key.clone()));
    }
    for key in selected_keys {
        values.push(rusqlite::types::Value::from(key.clone()));
    }
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare authoring contract edge query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            Ok(AuthoringContractEdge {
                from_title: row.get(0)?,
                from_kind: row.get(1)?,
                to_title: row.get(2)?,
                to_kind: row.get(3)?,
                relation: row.get(4)?,
            })
        })
        .context("failed to run authoring contract edge query")?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode authoring contract edge row")?);
    }
    Ok(out)
}

fn score_contract_candidates(
    records: Vec<ContractIndexRecord>,
    options: &AuthoringContractPlanOptions,
) -> Vec<Candidate> {
    let stub_templates = options
        .stub_detected_templates
        .iter()
        .map(|template| normalize_contract_title(&template.template_title))
        .collect::<BTreeSet<_>>();
    let suggested_templates = options
        .suggested_templates
        .iter()
        .map(|template| normalize_contract_title(&template.template_title))
        .collect::<BTreeSet<_>>();
    let baseline_templates = options
        .template_baseline
        .iter()
        .map(|template| normalize_contract_title(&template.template_title))
        .collect::<BTreeSet<_>>();
    let module_patterns = options
        .module_patterns
        .iter()
        .map(|module| normalize_contract_title(&module.module_title))
        .collect::<BTreeSet<_>>();

    let terms = normalized_terms(&options.query_terms, &options.query);
    let mut out = Vec::new();
    for record in records {
        let normalized_title = normalize_contract_title(&record.title);
        let mut score = 0usize;
        let mut reasons = Vec::new();

        if record.kind == "template" && stub_templates.contains(&normalized_title) {
            add_reason(
                &mut score,
                &mut reasons,
                "stub_template",
                520,
                "draft already invokes this template",
                Vec::new(),
            );
        }
        if record.kind == "template" && suggested_templates.contains(&normalized_title) {
            add_reason(
                &mut score,
                &mut reasons,
                "comparable_template",
                260,
                "retrieved comparable pages use this template",
                example_titles(&record),
            );
        }
        if record.kind == "template" && baseline_templates.contains(&normalized_title) {
            add_reason(
                &mut score,
                &mut reasons,
                "baseline_template",
                80,
                "template appears in the wiki baseline authoring surface",
                Vec::new(),
            );
        }
        if record.kind == "module" && module_patterns.contains(&normalized_title) {
            add_reason(
                &mut score,
                &mut reasons,
                "module_pattern",
                220,
                "module is used by comparable pages or selected templates",
                example_titles(&record),
            );
        }

        let matched_query_terms = score_query_matches(&record, &terms, &mut score, &mut reasons);
        if score == 0 {
            continue;
        }
        if record.usage_count > 0 {
            let usage_weight = record.usage_count.min(60);
            add_reason(
                &mut score,
                &mut reasons,
                "usage_frequency",
                usage_weight,
                "local index has observed usage for this contract",
                example_titles(&record),
            );
        }

        out.push(Candidate {
            record,
            score,
            reasons,
            matched_query_terms,
        });
    }

    out.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.record.usage_count.cmp(&left.record.usage_count))
            .then_with(|| left.record.title.cmp(&right.record.title))
    });
    out
}

fn score_query_matches(
    record: &ContractIndexRecord,
    terms: &[String],
    score: &mut usize,
    reasons: &mut Vec<AuthoringContractSelectionReason>,
) -> BTreeSet<String> {
    let mut matched = BTreeSet::new();
    if terms.is_empty() {
        return matched;
    }
    let title = record.title.to_ascii_lowercase();
    let category = record.category.to_ascii_lowercase();
    let summary = record.summary_text.to_ascii_lowercase();
    let params = record.parameter_keys.join(" ").to_ascii_lowercase();
    let functions = record.function_names.join(" ").to_ascii_lowercase();
    let semantic = record.semantic_text.to_ascii_lowercase();

    for term in terms {
        if title.contains(term) {
            matched.insert(term.clone());
            add_reason(
                score,
                reasons,
                "title_match",
                150,
                &format!("contract title contains `{term}`"),
                Vec::new(),
            );
        } else if category.contains(term) {
            matched.insert(term.clone());
            add_reason(
                score,
                reasons,
                "category_match",
                90,
                &format!("contract category contains `{term}`"),
                Vec::new(),
            );
        } else if summary.contains(term) {
            matched.insert(term.clone());
            add_reason(
                score,
                reasons,
                "summary_match",
                70,
                &format!("contract summary contains `{term}`"),
                Vec::new(),
            );
        } else if params.contains(term) {
            matched.insert(term.clone());
            add_reason(
                score,
                reasons,
                "parameter_match",
                48,
                &format!("contract parameters contain `{term}`"),
                Vec::new(),
            );
        } else if functions.contains(term) {
            matched.insert(term.clone());
            add_reason(
                score,
                reasons,
                "function_match",
                48,
                &format!("module functions contain `{term}`"),
                Vec::new(),
            );
        } else if semantic.contains(term) {
            matched.insert(term.clone());
            add_reason(
                score,
                reasons,
                "semantic_match",
                24,
                &format!("contract semantic text contains `{term}`"),
                Vec::new(),
            );
        }
    }
    matched
}

fn fallback_contract_candidates(options: &AuthoringContractPlanOptions) -> Vec<Candidate> {
    let mut out = Vec::new();
    for template in options
        .suggested_templates
        .iter()
        .chain(options.template_baseline.iter())
    {
        let record = record_from_template_summary("fallback", template);
        let mut score = 120usize.saturating_add(template.usage_count.min(60));
        let mut reasons = Vec::new();
        add_reason(
            &mut score,
            &mut reasons,
            "pack_template",
            120,
            "template was already selected by the authoring pack",
            template.example_pages.clone(),
        );
        out.push(Candidate {
            record,
            score,
            reasons,
            matched_query_terms: BTreeSet::new(),
        });
    }
    for module in &options.module_patterns {
        let record = record_from_module_summary("fallback", module);
        let mut score = 110usize.saturating_add(module.usage_count.min(60));
        let mut reasons = Vec::new();
        add_reason(
            &mut score,
            &mut reasons,
            "pack_module",
            110,
            "module was already selected by the authoring pack",
            module.example_pages.clone(),
        );
        out.push(Candidate {
            record,
            score,
            reasons,
            matched_query_terms: BTreeSet::new(),
        });
    }
    out.sort_by(|left, right| right.score.cmp(&left.score));
    out
}

fn materialize_plan(
    options: &AuthoringContractPlanOptions,
    candidates: Vec<Candidate>,
    edges: Vec<AuthoringContractEdge>,
) -> AuthoringContractTraversalPlan {
    let limit = options.limit.max(1);
    let token_budget = options.token_budget.max(1);
    let mut selected = Vec::new();
    let mut omitted = Vec::new();
    let mut matched_query_terms = BTreeSet::new();
    let mut used_tokens = 0usize;
    for candidate in candidates {
        let candidate_matched_terms = candidate.matched_query_terms.clone();
        let node = contract_node(candidate, options.profile);
        let next_tokens = used_tokens.saturating_add(node.token_estimate);
        if selected.len() >= limit || (!selected.is_empty() && next_tokens > token_budget) {
            omitted.push(AuthoringContractOmission {
                contract_kind: node.contract_kind,
                title: node.title,
                score: node.score,
                reason: if selected.len() >= limit {
                    "contract limit reached".to_string()
                } else {
                    "token budget reached".to_string()
                },
            });
            continue;
        }
        matched_query_terms.extend(candidate_matched_terms);
        used_tokens = next_tokens;
        selected.push(node);
    }

    let mut warnings = Vec::new();
    let query_terms = normalized_terms(&options.query_terms, &options.query);
    let missing_query_terms = query_terms
        .iter()
        .filter(|term| !matched_query_terms.contains(*term))
        .cloned()
        .collect::<Vec<_>>();
    if selected.is_empty() {
        warnings.push(
            "No contract matched the query or draft seeds; try a subject-type query such as `infobox person`, `species infobox`, or a known template title."
                .to_string(),
        );
    }
    if !selected.is_empty() && !missing_query_terms.is_empty() {
        warnings.push(format!(
            "No selected contract matched these query terms: {}.",
            missing_query_terms.join(", ")
        ));
    }
    if !omitted.is_empty() {
        warnings.push(
            "Some matching contracts were omitted; increase --limit or --token-budget, or expand one contract explicitly."
                .to_string(),
        );
    }

    AuthoringContractTraversalPlan {
        schema_version: CONTRACT_PLAN_SCHEMA_VERSION.to_string(),
        query: options.query.clone(),
        profile: options.profile,
        matched_query_terms: matched_query_terms.into_iter().collect(),
        missing_query_terms,
        token_budget,
        token_estimate_total: used_tokens,
        selected_contracts: selected,
        omitted_contracts: omitted,
        contract_edges: edges,
        warnings,
    }
}

fn contract_node(candidate: Candidate, profile: AuthoringContractProfile) -> AuthoringContractNode {
    let expansion_hints = expansion_hints(&candidate.record);
    let mut parameter_keys = candidate.record.parameter_keys.clone();
    let mut function_names = candidate.record.function_names.clone();
    let mut module_titles = candidate.record.module_titles.clone();
    let mut example_titles = candidate.record.example_titles.clone();
    match profile {
        AuthoringContractProfile::Index => {
            parameter_keys.truncate(16);
            function_names.truncate(12);
            module_titles.truncate(8);
            example_titles.clear();
        }
        AuthoringContractProfile::Author => {
            parameter_keys.truncate(32);
            function_names.truncate(20);
            module_titles.truncate(12);
            example_titles.truncate(6);
        }
        AuthoringContractProfile::Implementation => {
            parameter_keys.truncate(64);
            function_names.truncate(40);
            module_titles.truncate(24);
            example_titles.truncate(10);
        }
    }

    let summary_text = normalize_spaces(&candidate.record.summary_text);
    let summary_text = if summary_text.is_empty() {
        None
    } else {
        Some(truncate_chars(&summary_text, 420))
    };
    let token_text = [
        candidate.record.title.as_str(),
        candidate.record.category.as_str(),
        summary_text.as_deref().unwrap_or_default(),
        &parameter_keys.join(" "),
        &function_names.join(" "),
        &module_titles.join(" "),
        &example_titles.join(" "),
    ]
    .join(" ");

    AuthoringContractNode {
        contract_kind: candidate.record.kind.clone(),
        title: candidate.record.title.clone(),
        category: candidate.record.category.clone(),
        score: candidate.score,
        token_estimate: estimate_tokens(&token_text).max(1),
        summary_text,
        usage_count: candidate.record.usage_count,
        distinct_page_count: candidate.record.distinct_page_count,
        parameter_keys,
        function_names,
        module_titles,
        example_titles,
        selection_reasons: candidate.reasons,
        expansion_hints,
    }
}

fn expansion_hints(record: &ContractIndexRecord) -> Vec<AuthoringContractExpansionHint> {
    match record.kind.as_str() {
        "template" => vec![
            AuthoringContractExpansionHint {
                command: format!("wikitool templates show \"{}\" --format json", record.title),
                reason: "show full parameter contract and examples from the template catalog"
                    .to_string(),
            },
            AuthoringContractExpansionHint {
                command: format!(
                    "wikitool knowledge inspect templates \"{}\" --format json",
                    record.title
                ),
                reason: "expand implementation pages, module edges, and indexed source chunks"
                    .to_string(),
            },
        ],
        "module" => vec![AuthoringContractExpansionHint {
            command: format!(
                "wikitool knowledge contracts search \"{}\" --profile implementation --format json",
                record.title
            ),
            reason: "show module functions, examples, and referencing templates".to_string(),
        }],
        _ => Vec::new(),
    }
}

fn contract_records_from_catalog(
    profile_id: &str,
    catalog: &TemplateCatalog,
) -> Vec<ContractIndexRecord> {
    let mut out = Vec::new();
    for entry in &catalog.entries {
        out.push(record_from_catalog_entry(profile_id, entry));
    }
    out
}

fn record_from_catalog_entry(
    profile_id: &str,
    entry: &TemplateCatalogEntry,
) -> ContractIndexRecord {
    let parameter_keys = entry
        .declared_parameter_keys
        .iter()
        .chain(entry.parameters.iter().map(|parameter| &parameter.name))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let semantic_text = normalize_spaces(
        &[
            entry.template_title.clone(),
            entry.category.clone(),
            entry.summary_text.clone().unwrap_or_default(),
            parameter_keys.join(" "),
            entry.module_titles.join(" "),
            entry.example_pages.join(" "),
            entry.recommendation_tags.join(" "),
        ]
        .join(" "),
    );
    ContractIndexRecord {
        key: authoring_contract_key(profile_id, "template", &entry.template_title),
        kind: "template".to_string(),
        title: entry.template_title.clone(),
        category: entry.category.clone(),
        summary_text: entry.summary_text.clone().unwrap_or_default(),
        usage_count: entry.usage_count,
        distinct_page_count: entry.distinct_page_count,
        parameter_keys,
        function_names: Vec::new(),
        module_titles: entry.module_titles.clone(),
        example_titles: entry.example_pages.clone(),
        semantic_text,
    }
}

fn record_from_template_summary(
    profile_id: &str,
    template: &TemplateUsageSummary,
) -> ContractIndexRecord {
    let parameter_keys = template
        .parameter_stats
        .iter()
        .map(|parameter| parameter.key.clone())
        .collect::<Vec<_>>();
    let semantic_text = normalize_spaces(
        &[
            template.template_title.clone(),
            parameter_keys.join(" "),
            template.implementation_titles.join(" "),
            template.example_pages.join(" "),
        ]
        .join(" "),
    );
    ContractIndexRecord {
        key: authoring_contract_key(profile_id, "template", &template.template_title),
        kind: "template".to_string(),
        title: template.template_title.clone(),
        category: contract_category_from_title(&template.template_title),
        summary_text: template.implementation_preview.clone().unwrap_or_default(),
        usage_count: template.usage_count,
        distinct_page_count: template.distinct_page_count,
        parameter_keys,
        function_names: Vec::new(),
        module_titles: template
            .implementation_titles
            .iter()
            .filter(|title| title.starts_with("Module:"))
            .cloned()
            .collect(),
        example_titles: template.example_pages.clone(),
        semantic_text,
    }
}

fn record_from_module_summary(
    profile_id: &str,
    module: &ModuleUsageSummary,
) -> ContractIndexRecord {
    let function_names = module
        .function_stats
        .iter()
        .map(|function| function.function_name.clone())
        .collect::<Vec<_>>();
    let semantic_text = normalize_spaces(
        &[
            module.module_title.clone(),
            function_names.join(" "),
            module.example_pages.join(" "),
        ]
        .join(" "),
    );
    ContractIndexRecord {
        key: authoring_contract_key(profile_id, "module", &module.module_title),
        kind: "module".to_string(),
        title: module.module_title.clone(),
        category: "module".to_string(),
        summary_text: String::new(),
        usage_count: module.usage_count,
        distinct_page_count: module.distinct_page_count,
        parameter_keys: Vec::new(),
        function_names,
        module_titles: Vec::new(),
        example_titles: module.example_pages.clone(),
        semantic_text,
    }
}

fn fallback_edges_from_template_references(
    template_references: &[TemplateReference],
) -> Vec<AuthoringContractEdge> {
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

fn add_reason(
    score: &mut usize,
    reasons: &mut Vec<AuthoringContractSelectionReason>,
    signal: &str,
    weight: usize,
    detail: &str,
    evidence_titles: Vec<String>,
) {
    *score = score.saturating_add(weight);
    reasons.push(AuthoringContractSelectionReason {
        signal: signal.to_string(),
        weight,
        detail: detail.to_string(),
        evidence_titles,
    });
}

fn example_titles(record: &ContractIndexRecord) -> Vec<String> {
    record.example_titles.iter().take(4).cloned().collect()
}

fn normalized_terms(query_terms: &[String], query: &str) -> Vec<String> {
    let mut terms = query_terms
        .iter()
        .flat_map(|term| term.split_whitespace())
        .chain(query.split_whitespace())
        .map(normalize_contract_term)
        .filter(|term| term.chars().count() >= 2)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    terms.truncate(12);
    terms
}

fn fts_tokens(query_terms: &[String]) -> Vec<String> {
    query_terms
        .iter()
        .flat_map(|term| term.split_whitespace())
        .map(normalize_contract_term)
        .filter(|term| term.chars().count() >= 2)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(8)
        .collect()
}

fn normalize_contract_term(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>()
        .to_ascii_lowercase()
}

fn normalize_contract_title(value: &str) -> String {
    normalize_spaces(&value.replace('_', " "))
}

fn authoring_contract_key(profile_id: &str, kind: &str, title: &str) -> String {
    format!(
        "{}:{}:{}",
        profile_id.trim().to_ascii_lowercase(),
        kind.trim().to_ascii_lowercase(),
        normalize_contract_title(title)
    )
}

fn contract_category_from_title(title: &str) -> String {
    let lower = title.to_ascii_lowercase();
    if lower.contains("infobox") {
        "infobox".to_string()
    } else if lower.contains("cite") {
        "cite".to_string()
    } else if lower.contains("nav") {
        "navigation".to_string()
    } else {
        "template".to_string()
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
