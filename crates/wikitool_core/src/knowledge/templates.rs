pub use super::model::{
    ActiveTemplateCatalog, ActiveTemplateCatalogLookup, ModuleFunctionUsage,
    ModuleInvocationExample, ModuleUsageSummary, TemplateImplementationPage,
    TemplateInvocationExample, TemplateParameterUsage, TemplateReference, TemplateReferenceLookup,
    TemplateUsageSummary,
};
use super::prelude::*;
use crate::knowledge::model::StubTemplateHint;
use crate::knowledge::retrieval::LocalSectionSummary;
use crate::knowledge::retrieval::{
    load_context_chunks_for_bundle, load_section_records_for_bundle,
};

pub fn query_active_template_catalog(
    paths: &ResolvedPaths,
    limit: usize,
) -> Result<ActiveTemplateCatalogLookup> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(ActiveTemplateCatalogLookup::IndexMissing),
    };
    let active_template_count = if table_exists(&connection, "indexed_template_invocations")? {
        count_query(
            &connection,
            "SELECT COUNT(DISTINCT template_title) FROM indexed_template_invocations",
        )
        .context("failed to count active templates")?
    } else {
        0
    };
    let templates = summarize_template_usage_for_sources(&connection, None, limit.max(1))?;
    Ok(ActiveTemplateCatalogLookup::Found(ActiveTemplateCatalog {
        active_template_count,
        templates,
    }))
}

pub fn query_template_reference(
    paths: &ResolvedPaths,
    template_title: &str,
) -> Result<TemplateReferenceLookup> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(TemplateReferenceLookup::IndexMissing),
    };

    let normalized_template = normalize_template_lookup_title(template_title);
    if normalized_template.is_empty() {
        return Ok(TemplateReferenceLookup::TemplateMissing {
            template_title: template_title.trim().to_string(),
        });
    }

    let Some(reference) =
        load_template_reference_for_connection(&connection, &normalized_template)?
    else {
        return Ok(TemplateReferenceLookup::TemplateMissing {
            template_title: normalized_template,
        });
    };

    Ok(TemplateReferenceLookup::Found(Box::new(reference)))
}

pub(crate) fn load_template_reference_for_connection(
    connection: &Connection,
    template_title: &str,
) -> Result<Option<TemplateReference>> {
    let Some(template) = load_template_usage_summary_for_connection(connection, template_title)?
    else {
        return Ok(None);
    };

    let implementation_records =
        load_template_implementation_pages_for_connection(connection, template_title)?;
    let mut implementation_pages = Vec::new();
    let mut implementation_sections = Vec::new();
    let mut implementation_chunks = Vec::new();
    for page in implementation_records
        .into_iter()
        .take(TEMPLATE_IMPLEMENTATION_PAGE_LIMIT)
    {
        let section_summaries = load_section_records_for_bundle(connection, &page.relative_path)?
            .unwrap_or_default()
            .into_iter()
            .map(|section| LocalSectionSummary {
                section_heading: section.section_heading,
                section_level: section.section_level,
                summary_text: section.summary_text,
                token_estimate: section.token_estimate,
            })
            .collect::<Vec<_>>();
        let context_chunks = load_context_chunks_for_bundle(connection, &page.relative_path, None)?
            .unwrap_or_default();
        implementation_sections.extend(section_summaries.iter().cloned());
        implementation_chunks.extend(context_chunks.iter().cloned());
        implementation_pages.push(TemplateImplementationPage {
            page_title: page.title.clone(),
            namespace: page.namespace.clone(),
            role: page.role,
            summary_text: load_page_summary_for_connection(connection, &page.relative_path)?,
            section_summaries,
            context_chunks,
        });
    }

    Ok(Some(TemplateReference {
        template,
        implementation_pages,
        implementation_sections,
        implementation_chunks,
    }))
}

pub(crate) fn collect_authoring_template_reference_titles(
    stub_detected_templates: &[StubTemplateHint],
    suggested_templates: &[TemplateUsageSummary],
    template_baseline: &[TemplateUsageSummary],
    limit: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    for title in stub_detected_templates
        .iter()
        .map(|template| template.template_title.clone())
        .chain(
            suggested_templates
                .iter()
                .map(|template| template.template_title.clone()),
        )
        .chain(
            template_baseline
                .iter()
                .map(|template| template.template_title.clone()),
        )
    {
        let normalized = normalize_template_lookup_title(&title);
        if normalized.is_empty() || !seen.insert(normalized.to_ascii_lowercase()) {
            continue;
        }
        out.push(normalized);
        if out.len() >= limit {
            break;
        }
    }

    out
}

pub(crate) fn load_authoring_template_references(
    connection: &Connection,
    template_titles: &[String],
    limit: usize,
) -> Result<Vec<TemplateReference>> {
    let mut out = Vec::new();
    for template_title in template_titles.iter().take(limit) {
        if let Some(reference) = load_template_reference_for_connection(connection, template_title)?
        {
            out.push(reference);
        }
    }
    Ok(out)
}

pub(crate) fn build_authoring_module_patterns(
    connection: &Connection,
    source_titles: &[String],
    template_references: &[TemplateReference],
    limit: usize,
) -> Result<Vec<ModuleUsageSummary>> {
    let mut out = summarize_module_usage_for_sources(connection, source_titles, limit)?;
    let mut seen = out
        .iter()
        .map(|summary| summary.module_title.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();

    let referenced_modules = template_references
        .iter()
        .flat_map(|reference| reference.implementation_pages.iter())
        .filter(|page| page.role == "module")
        .map(|page| page.page_title.clone())
        .collect::<Vec<_>>();

    for module_title in referenced_modules {
        if out.len() >= limit {
            break;
        }
        let normalized = normalize_module_lookup_title(&module_title);
        if normalized.is_empty() || !seen.insert(normalized.to_ascii_lowercase()) {
            continue;
        }
        if let Some(summary) = load_module_usage_summary_for_connection(connection, &normalized)? {
            out.push(summary);
        }
    }

    out.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| right.distinct_page_count.cmp(&left.distinct_page_count))
            .then_with(|| left.module_title.cmp(&right.module_title))
    });
    out.truncate(limit);
    Ok(out)
}

#[derive(Default)]
struct TemplateUsageAccumulator {
    usage_count: usize,
    source_pages: BTreeSet<String>,
    parameter_key_counts: BTreeMap<String, usize>,
}

#[derive(Default)]
struct ModuleUsageAccumulator {
    usage_count: usize,
    source_pages: BTreeSet<String>,
    function_counts: BTreeMap<String, usize>,
    function_parameter_examples: BTreeMap<String, BTreeSet<String>>,
    examples: Vec<ModuleInvocationExample>,
}

#[derive(Debug, Clone)]
struct IndexedModuleInvocationRow {
    source_title: String,
    source_relative_path: String,
    module_title: String,
    function_name: String,
    parameter_keys: Vec<String>,
    invocation_wikitext: String,
    token_estimate: usize,
}

pub(crate) fn summarize_template_usage_for_sources(
    connection: &Connection,
    source_titles: Option<&[String]>,
    limit: usize,
) -> Result<Vec<TemplateUsageSummary>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if !table_exists(connection, "indexed_template_invocations")? {
        return Ok(Vec::new());
    }
    if let Some(source_titles) = source_titles
        && source_titles.is_empty()
    {
        return Ok(Vec::new());
    }

    let rows = load_template_invocation_rows_for_sources(connection, source_titles)?;
    let mut template_map = BTreeMap::<String, TemplateUsageAccumulator>::new();
    for (template_title, source_title, parameter_keys_serialized) in rows {
        let entry = template_map.entry(template_title).or_default();
        entry.usage_count = entry.usage_count.saturating_add(1);
        entry.source_pages.insert(source_title);
        for key in parse_parameter_key_list(&parameter_keys_serialized) {
            let count = entry.parameter_key_counts.entry(key).or_insert(0);
            *count = count.saturating_add(1);
        }
    }

    let mut ranked = template_map.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_title, left_acc), (right_title, right_acc)| {
        right_acc
            .usage_count
            .cmp(&left_acc.usage_count)
            .then_with(|| {
                right_acc
                    .source_pages
                    .len()
                    .cmp(&left_acc.source_pages.len())
            })
            .then_with(|| left_title.cmp(right_title))
    });
    ranked.truncate(limit);

    ranked
        .into_iter()
        .map(|(template_title, accumulator)| {
            materialize_template_usage_summary(connection, template_title, accumulator)
        })
        .collect()
}

pub(crate) fn load_template_invocation_rows_for_sources(
    connection: &Connection,
    source_titles: Option<&[String]>,
) -> Result<Vec<(String, String, String)>> {
    let (sql, values) = if let Some(source_titles) = source_titles {
        let placeholders = std::iter::repeat_n("?", source_titles.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT template_title, source_title, parameter_keys
             FROM indexed_template_invocations
             WHERE source_title IN ({placeholders})"
        );
        let values = source_titles
            .iter()
            .cloned()
            .map(rusqlite::types::Value::from)
            .collect::<Vec<_>>();
        (sql, values)
    } else {
        (
            "SELECT template_title, source_title, parameter_keys FROM indexed_template_invocations"
                .to_string(),
            Vec::new(),
        )
    };

    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare template invocation summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .context("failed to run template invocation summary query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode template invocation summary row")?);
    }
    Ok(out)
}

pub(crate) fn summarize_module_usage_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<ModuleUsageSummary>> {
    if limit == 0
        || source_titles.is_empty()
        || !table_exists(connection, "indexed_module_invocations")?
    {
        return Ok(Vec::new());
    }

    let rows = load_module_invocation_rows_for_sources(connection, source_titles)?;
    let mut grouped = BTreeMap::<String, ModuleUsageAccumulator>::new();
    for row in rows {
        let entry = grouped.entry(row.module_title.clone()).or_default();
        entry.usage_count = entry.usage_count.saturating_add(1);
        entry.source_pages.insert(row.source_title.clone());
        let function_count = entry
            .function_counts
            .entry(row.function_name.clone())
            .or_insert(0);
        *function_count = function_count.saturating_add(1);
        let function_examples = entry
            .function_parameter_examples
            .entry(row.function_name.clone())
            .or_default();
        for key in &row.parameter_keys {
            function_examples.insert(key.clone());
        }
        entry.examples.push(ModuleInvocationExample {
            source_title: row.source_title,
            source_relative_path: row.source_relative_path,
            function_name: row.function_name,
            parameter_keys: row.parameter_keys,
            invocation_text: row.invocation_wikitext,
            token_estimate: row.token_estimate,
        });
    }

    let mut ranked = grouped.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_title, left_acc), (right_title, right_acc)| {
        right_acc
            .usage_count
            .cmp(&left_acc.usage_count)
            .then_with(|| {
                right_acc
                    .source_pages
                    .len()
                    .cmp(&left_acc.source_pages.len())
            })
            .then_with(|| left_title.cmp(right_title))
    });
    ranked.truncate(limit);

    Ok(ranked
        .into_iter()
        .map(|(module_title, accumulator)| {
            materialize_module_usage_summary(module_title, accumulator)
        })
        .collect())
}

pub(crate) fn load_module_usage_summary_for_connection(
    connection: &Connection,
    module_title: &str,
) -> Result<Option<ModuleUsageSummary>> {
    if !table_exists(connection, "indexed_module_invocations")? {
        return Ok(None);
    }

    let rows = load_module_invocation_rows_for_module(connection, module_title)?;
    if rows.is_empty() {
        return Ok(None);
    }

    let mut accumulator = ModuleUsageAccumulator::default();
    for row in rows {
        accumulator.usage_count = accumulator.usage_count.saturating_add(1);
        accumulator.source_pages.insert(row.source_title.clone());
        let function_count = accumulator
            .function_counts
            .entry(row.function_name.clone())
            .or_insert(0);
        *function_count = function_count.saturating_add(1);
        let function_examples = accumulator
            .function_parameter_examples
            .entry(row.function_name.clone())
            .or_default();
        for key in &row.parameter_keys {
            function_examples.insert(key.clone());
        }
        accumulator.examples.push(ModuleInvocationExample {
            source_title: row.source_title,
            source_relative_path: row.source_relative_path,
            function_name: row.function_name,
            parameter_keys: row.parameter_keys,
            invocation_text: row.invocation_wikitext,
            token_estimate: row.token_estimate,
        });
    }

    Ok(Some(materialize_module_usage_summary(
        normalize_module_lookup_title(module_title),
        accumulator,
    )))
}

fn load_module_invocation_rows_for_sources(
    connection: &Connection,
    source_titles: &[String],
) -> Result<Vec<IndexedModuleInvocationRow>> {
    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT source_title, source_relative_path, module_title, function_name, parameter_keys,
                invocation_wikitext, token_estimate
         FROM indexed_module_invocations
         WHERE source_title IN ({placeholders})"
    );
    let values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare module invocation summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            let token_estimate_i64: i64 = row.get(6)?;
            Ok(IndexedModuleInvocationRow {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                module_title: row.get(2)?,
                function_name: row.get(3)?,
                parameter_keys: parse_parameter_key_list(&row.get::<_, String>(4)?),
                invocation_wikitext: row.get(5)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run module invocation summary query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode module invocation summary row")?);
    }
    Ok(out)
}

fn load_module_invocation_rows_for_module(
    connection: &Connection,
    module_title: &str,
) -> Result<Vec<IndexedModuleInvocationRow>> {
    let normalized = normalize_module_lookup_title(module_title);
    if normalized.is_empty() {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            "SELECT source_title, source_relative_path, module_title, function_name, parameter_keys,
                    invocation_wikitext, token_estimate
             FROM indexed_module_invocations
             WHERE lower(module_title) = lower(?1)",
        )
        .context("failed to prepare module invocation lookup query")?;
    let rows = statement
        .query_map([normalized], |row| {
            let token_estimate_i64: i64 = row.get(6)?;
            Ok(IndexedModuleInvocationRow {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                module_title: row.get(2)?,
                function_name: row.get(3)?,
                parameter_keys: parse_parameter_key_list(&row.get::<_, String>(4)?),
                invocation_wikitext: row.get(5)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run module invocation lookup query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode module invocation lookup row")?);
    }
    Ok(out)
}

fn materialize_module_usage_summary(
    module_title: String,
    mut accumulator: ModuleUsageAccumulator,
) -> ModuleUsageSummary {
    let mut function_stats = accumulator
        .function_counts
        .into_iter()
        .map(|(function_name, usage_count)| ModuleFunctionUsage {
            example_parameter_keys: accumulator
                .function_parameter_examples
                .remove(&function_name)
                .unwrap_or_default()
                .into_iter()
                .take(AUTHORING_TEMPLATE_KEY_LIMIT)
                .collect(),
            function_name,
            usage_count,
        })
        .collect::<Vec<_>>();
    function_stats.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| left.function_name.cmp(&right.function_name))
    });
    function_stats.truncate(AUTHORING_MODULE_FUNCTION_LIMIT);

    accumulator.examples.sort_by(|left, right| {
        left.token_estimate
            .cmp(&right.token_estimate)
            .then_with(|| left.source_title.cmp(&right.source_title))
            .then_with(|| left.function_name.cmp(&right.function_name))
    });
    accumulator
        .examples
        .truncate(MODULE_REFERENCE_EXAMPLE_LIMIT);

    ModuleUsageSummary {
        module_title,
        usage_count: accumulator.usage_count,
        distinct_page_count: accumulator.source_pages.len(),
        function_stats,
        example_pages: accumulator
            .source_pages
            .iter()
            .take(MODULE_REFERENCE_EXAMPLE_LIMIT)
            .cloned()
            .collect(),
        example_invocations: accumulator.examples,
    }
}

pub(crate) fn load_template_usage_summary_for_connection(
    connection: &Connection,
    template_title: &str,
) -> Result<Option<TemplateUsageSummary>> {
    if !table_exists(connection, "indexed_template_invocations")? {
        return Ok(None);
    }

    let rows = load_template_invocation_rows_for_template(connection, template_title)?;
    if rows.is_empty() {
        return Ok(None);
    }

    let mut accumulator = TemplateUsageAccumulator::default();
    for (source_title, parameter_keys_serialized) in rows {
        accumulator.usage_count = accumulator.usage_count.saturating_add(1);
        accumulator.source_pages.insert(source_title);
        for key in parse_parameter_key_list(&parameter_keys_serialized) {
            let count = accumulator.parameter_key_counts.entry(key).or_insert(0);
            *count = count.saturating_add(1);
        }
    }

    materialize_template_usage_summary(connection, template_title.to_string(), accumulator)
        .map(Some)
}

pub(crate) fn load_template_invocation_rows_for_template(
    connection: &Connection,
    template_title: &str,
) -> Result<Vec<(String, String)>> {
    let mut statement = connection
        .prepare(
            "SELECT source_title, parameter_keys
             FROM indexed_template_invocations
             WHERE lower(template_title) = lower(?1)",
        )
        .context("failed to prepare template invocation lookup query")?;
    let rows = statement
        .query_map([template_title], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .context("failed to run template invocation lookup query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode template invocation lookup row")?);
    }
    Ok(out)
}

fn materialize_template_usage_summary(
    connection: &Connection,
    template_title: String,
    accumulator: TemplateUsageAccumulator,
) -> Result<TemplateUsageSummary> {
    let example_invocations = load_template_examples_for_connection(
        connection,
        &template_title,
        TEMPLATE_REFERENCE_EXAMPLE_LIMIT,
    )?;
    let parameter_examples = collect_template_parameter_value_examples(
        &example_invocations,
        TEMPLATE_PARAMETER_VALUE_LIMIT,
    );
    let mut parameter_stats = accumulator
        .parameter_key_counts
        .into_iter()
        .map(|(key, usage_count)| TemplateParameterUsage {
            example_values: parameter_examples.get(&key).cloned().unwrap_or_default(),
            key,
            usage_count,
        })
        .collect::<Vec<_>>();
    parameter_stats.sort_by(|left, right| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| left.key.cmp(&right.key))
    });
    parameter_stats.truncate(AUTHORING_TEMPLATE_KEY_LIMIT);

    Ok(TemplateUsageSummary {
        aliases: load_template_aliases(connection, &template_title)?,
        usage_count: accumulator.usage_count,
        distinct_page_count: accumulator.source_pages.len(),
        parameter_stats,
        example_pages: accumulator
            .source_pages
            .iter()
            .take(TEMPLATE_REFERENCE_EXAMPLE_LIMIT)
            .cloned()
            .collect(),
        implementation_titles: load_template_implementation_titles(connection, &template_title)?,
        implementation_preview: load_template_implementation_preview(connection, &template_title)?,
        example_invocations,
        template_title,
    })
}

pub(crate) fn normalize_template_lookup_title(value: &str) -> String {
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return String::new();
    }
    canonical_template_title(&normalized).unwrap_or_else(|| normalize_query_title(&normalized))
}

pub(crate) fn normalize_module_lookup_title(value: &str) -> String {
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return String::new();
    }
    if normalized
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Module:"))
    {
        return normalize_query_title(&normalized);
    }
    format!("Module:{normalized}")
}

pub(crate) fn load_page_summary_for_connection(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<String> {
    if table_exists(connection, "indexed_page_sections")? {
        let mut statement = connection
            .prepare(
                "SELECT summary_text
                 FROM indexed_page_sections
                 WHERE source_relative_path = ?1
                 ORDER BY section_index ASC
                 LIMIT 1",
            )
            .context("failed to prepare page summary query")?;
        let summary = statement
            .query_row([source_relative_path], |row| row.get::<_, String>(0))
            .optional()
            .context("failed to run page summary query")?;
        if let Some(summary) = summary {
            let normalized = normalize_spaces(&summary);
            if !normalized.is_empty() {
                return Ok(normalized);
            }
        }
    }

    if table_exists(connection, "indexed_page_chunks")? {
        let mut statement = connection
            .prepare(
                "SELECT chunk_text
                 FROM indexed_page_chunks
                 WHERE source_relative_path = ?1
                 ORDER BY chunk_index ASC
                 LIMIT 1",
            )
            .context("failed to prepare page chunk summary query")?;
        let chunk = statement
            .query_row([source_relative_path], |row| row.get::<_, String>(0))
            .optional()
            .context("failed to run page chunk summary query")?;
        if let Some(chunk) = chunk {
            let normalized = summarize_words(&chunk, AUTHORING_PAGE_SUMMARY_WORD_LIMIT);
            if !normalized.is_empty() {
                return Ok(normalized);
            }
        }
    }

    Ok(String::new())
}

pub(crate) fn load_template_aliases(
    connection: &Connection,
    template_title: &str,
) -> Result<Vec<String>> {
    if !table_exists(connection, "indexed_page_aliases")? {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            "SELECT alias_title
             FROM indexed_page_aliases
             WHERE lower(canonical_title) = lower(?1)
               AND canonical_namespace = 'Template'
             ORDER BY alias_title ASC",
        )
        .context("failed to prepare template alias query")?;
    let rows = statement
        .query_map([template_title], |row| row.get::<_, String>(0))
        .context("failed to run template alias query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode template alias row")?);
    }
    Ok(out)
}

pub(crate) fn load_template_implementation_preview(
    connection: &Connection,
    template_title: &str,
) -> Result<Option<String>> {
    let Some(page) = load_page_record(connection, template_title)? else {
        return Ok(None);
    };
    let summary = load_page_summary_for_connection(connection, &page.relative_path)?;
    if summary.is_empty() {
        Ok(None)
    } else {
        Ok(Some(summary))
    }
}

#[derive(Debug, Clone)]
struct TemplateImplementationRecord {
    title: String,
    namespace: String,
    relative_path: String,
    role: String,
}

pub(crate) fn load_template_implementation_titles(
    connection: &Connection,
    template_title: &str,
) -> Result<Vec<String>> {
    Ok(
        load_template_implementation_pages_for_connection(connection, template_title)?
            .into_iter()
            .map(|page| page.title)
            .collect(),
    )
}

fn load_template_implementation_pages_for_connection(
    connection: &Connection,
    template_title: &str,
) -> Result<Vec<TemplateImplementationRecord>> {
    if table_exists(connection, "indexed_template_implementation_pages")? {
        let mut statement = connection
            .prepare(
                "SELECT implementation_page_title, implementation_namespace, source_relative_path, role
                 FROM indexed_template_implementation_pages
                 WHERE lower(template_title) = lower(?1)
                 ORDER BY
                   CASE role
                     WHEN 'template' THEN 0
                     WHEN 'documentation' THEN 1
                     WHEN 'module' THEN 2
                     ELSE 3
                   END,
                   implementation_page_title ASC",
            )
            .context("failed to prepare template implementation page query")?;
        let rows = statement
            .query_map([template_title], |row| {
                Ok(TemplateImplementationRecord {
                    title: row.get(0)?,
                    namespace: row.get(1)?,
                    relative_path: row.get(2)?,
                    role: row.get(3)?,
                })
            })
            .context("failed to run template implementation page query")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("failed to decode template implementation page row")?);
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    let Some(page) = load_page_record(connection, template_title)? else {
        return Ok(Vec::new());
    };
    Ok(vec![TemplateImplementationRecord {
        title: page.title,
        namespace: page.namespace,
        relative_path: page.relative_path,
        role: "template".to_string(),
    }])
}

pub(crate) fn load_template_examples_for_connection(
    connection: &Connection,
    template_title: &str,
    limit: usize,
) -> Result<Vec<TemplateInvocationExample>> {
    if limit == 0 || !table_exists(connection, "indexed_template_examples")? {
        return Ok(Vec::new());
    }

    let limit_i64 = i64::try_from(limit).context("template example limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT source_title, source_relative_path, parameter_keys, example_wikitext, token_estimate
             FROM indexed_template_examples
             WHERE lower(template_title) = lower(?1)
             ORDER BY token_estimate ASC, source_title ASC, invocation_index ASC
             LIMIT ?2",
        )
        .context("failed to prepare template example query")?;
    let rows = statement
        .query_map(params![template_title, limit_i64], |row| {
            let token_estimate_i64: i64 = row.get(4)?;
            Ok(TemplateInvocationExample {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                parameter_keys: parse_parameter_key_list(&row.get::<_, String>(2)?),
                invocation_text: row.get(3)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run template example query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode template example row")?);
    }
    Ok(out)
}

pub(crate) fn collect_template_parameter_value_examples(
    examples: &[TemplateInvocationExample],
    per_key_limit: usize,
) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::<String, Vec<String>>::new();
    let mut seen = BTreeMap::<String, BTreeSet<String>>::new();
    for example in examples {
        for (key, value) in parse_template_parameter_examples(&example.invocation_text) {
            let set = seen.entry(key.clone()).or_default();
            if !set.insert(value.clone()) {
                continue;
            }
            let entry = out.entry(key).or_default();
            if entry.len() < per_key_limit {
                entry.push(value);
            }
        }
    }
    out
}

pub(crate) fn parse_template_parameter_examples(invocation_text: &str) -> Vec<(String, String)> {
    let Some(inner) = invocation_text
        .strip_prefix("{{")
        .and_then(|value| value.strip_suffix("}}"))
    else {
        return Vec::new();
    };
    let segments = split_template_segments(inner);
    let mut out = Vec::new();
    let mut positional_index = 1usize;
    for segment in segments.into_iter().skip(1) {
        let value = segment.trim();
        if value.is_empty() {
            continue;
        }
        let (key, raw_value) = if let Some((key, value)) = split_once_top_level_equals(value) {
            let key = normalize_template_parameter_key(&key);
            if key.is_empty() {
                continue;
            }
            (key, value)
        } else {
            let key = format!("${positional_index}");
            positional_index += 1;
            (key, value.to_string())
        };
        let sample_value = summarize_words(&flatten_markup_excerpt(&raw_value), 8);
        if sample_value.is_empty() {
            continue;
        }
        out.push((key, sample_value));
    }
    out
}
