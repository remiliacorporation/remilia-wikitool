use super::*;

pub(super) fn store_template_catalog(
    paths: &ResolvedPaths,
    catalog: &TemplateCatalog,
) -> Result<()> {
    let mut connection = open_initialized_database_connection(&paths.db_path)?;
    let metadata_json =
        serde_json::to_string_pretty(catalog).context("failed to serialize template catalog")?;
    let built_at_unix = unix_timestamp()?;
    let transaction = connection
        .transaction()
        .context("failed to start template catalog transaction")?;
    transaction
        .execute(
            "INSERT INTO knowledge_artifacts (
                artifact_key,
                artifact_kind,
                profile,
                schema_generation,
                built_at_unix,
                row_count,
                metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(artifact_key) DO UPDATE SET
                artifact_kind = excluded.artifact_kind,
                profile = excluded.profile,
                schema_generation = excluded.schema_generation,
                built_at_unix = excluded.built_at_unix,
                row_count = excluded.row_count,
                metadata_json = excluded.metadata_json",
            params![
                template_catalog_artifact_key(&catalog.profile_id),
                TEMPLATE_CATALOG_ARTIFACT_KIND,
                Some(catalog.profile_id.as_str()),
                KNOWLEDGE_GENERATION,
                i64::try_from(built_at_unix).context("artifact timestamp does not fit into i64")?,
                i64::try_from(catalog.entries.len())
                    .context("artifact row count does not fit into i64")?,
                metadata_json,
            ],
        )
        .with_context(|| {
            format!(
                "failed to store template catalog for {}",
                catalog.profile_id
            )
        })?;
    store_authoring_contract_index(&transaction, catalog)?;
    transaction
        .commit()
        .context("failed to commit template catalog transaction")?;
    rebuild_fts_index(&connection)?;

    Ok(())
}

fn store_authoring_contract_index(
    connection: &Connection,
    catalog: &TemplateCatalog,
) -> Result<()> {
    connection
        .execute(
            "DELETE FROM indexed_authoring_contract_edges WHERE profile = ?1",
            [catalog.profile_id.as_str()],
        )
        .with_context(|| {
            format!(
                "failed to clear authoring contract edges for {}",
                catalog.profile_id
            )
        })?;
    connection
        .execute(
            "DELETE FROM indexed_authoring_contracts WHERE profile = ?1",
            [catalog.profile_id.as_str()],
        )
        .with_context(|| {
            format!(
                "failed to clear authoring contract nodes for {}",
                catalog.profile_id
            )
        })?;

    let module_usage = load_module_contract_usage(connection)?;
    let mut modules_by_title = BTreeMap::<String, ModuleContractIndexRecord>::new();
    let mut module_to_templates = BTreeMap::<String, BTreeSet<String>>::new();
    for entry in &catalog.entries {
        for module_title in &entry.module_titles {
            let normalized = normalize_contract_title(module_title);
            if normalized.is_empty() {
                continue;
            }
            let record = modules_by_title
                .entry(normalized.clone())
                .or_insert_with(|| ModuleContractIndexRecord {
                    module_title: normalized.clone(),
                    ..ModuleContractIndexRecord::default()
                });
            if let Some(usage) = module_usage.get(&normalized.to_ascii_lowercase()) {
                record.usage_count = usage.usage_count;
                record.example_pages = usage.example_pages.clone();
                record.function_names = usage.function_names.clone();
            }
            module_to_templates
                .entry(normalized)
                .or_default()
                .insert(entry.template_title.clone());
        }
    }

    let mut node_statement = connection
        .prepare(
            "INSERT INTO indexed_authoring_contracts (
                profile,
                contract_key,
                contract_kind,
                title,
                category,
                summary_text,
                usage_count,
                distinct_page_count,
                parameter_keys,
                function_names,
                module_titles,
                example_titles,
                semantic_text,
                token_estimate,
                source
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )
        .context("failed to prepare authoring contract node insert")?;
    for entry in &catalog.entries {
        let parameter_keys = template_contract_parameter_keys(entry);
        let semantic_text = template_contract_semantic_text(entry, &parameter_keys);
        let token_estimate = estimate_tokens(&semantic_text);
        node_statement
            .execute(params![
                catalog.profile_id,
                authoring_contract_key(&catalog.profile_id, "template", &entry.template_title),
                "template",
                entry.template_title,
                entry.category,
                entry.summary_text.clone().unwrap_or_default(),
                i64::try_from(entry.usage_count)
                    .context("template usage count does not fit into i64")?,
                i64::try_from(entry.distinct_page_count)
                    .context("template page count does not fit into i64")?,
                serialize_string_list(&parameter_keys),
                serialize_string_list(&Vec::<String>::new()),
                serialize_string_list(&entry.module_titles),
                serialize_string_list(&entry.example_pages),
                semantic_text,
                i64::try_from(token_estimate)
                    .context("template contract token estimate does not fit into i64")?,
                "template_catalog",
            ])
            .with_context(|| {
                format!(
                    "failed to index authoring template contract {}",
                    entry.template_title
                )
            })?;
    }

    for (module_title, mut record) in modules_by_title {
        record.referenced_by_templates = module_to_templates
            .remove(&module_title)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let semantic_text = module_contract_semantic_text(&record);
        let token_estimate = estimate_tokens(&semantic_text);
        node_statement
            .execute(params![
                catalog.profile_id,
                authoring_contract_key(&catalog.profile_id, "module", &record.module_title),
                "module",
                record.module_title,
                "module",
                "",
                i64::try_from(record.usage_count)
                    .context("module usage count does not fit into i64")?,
                i64::try_from(record.example_pages.len())
                    .context("module page count does not fit into i64")?,
                serialize_string_list(&Vec::<String>::new()),
                serialize_string_list(&record.function_names),
                serialize_string_list(&Vec::<String>::new()),
                serialize_string_list(&record.example_pages),
                semantic_text,
                i64::try_from(token_estimate)
                    .context("module contract token estimate does not fit into i64")?,
                "template_catalog",
            ])
            .with_context(|| {
                format!(
                    "failed to index authoring module contract {}",
                    record.module_title
                )
            })?;
    }

    let mut edge_statement = connection
        .prepare(
            "INSERT INTO indexed_authoring_contract_edges (
                profile,
                from_contract_key,
                from_kind,
                from_title,
                to_contract_key,
                to_kind,
                to_title,
                relation,
                evidence
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .context("failed to prepare authoring contract edge insert")?;
    for entry in &catalog.entries {
        for module_title in &entry.module_titles {
            let normalized_module = normalize_contract_title(module_title);
            if normalized_module.is_empty() {
                continue;
            }
            edge_statement
                .execute(params![
                    catalog.profile_id,
                    authoring_contract_key(&catalog.profile_id, "template", &entry.template_title),
                    "template",
                    entry.template_title,
                    authoring_contract_key(&catalog.profile_id, "module", &normalized_module),
                    "module",
                    normalized_module,
                    "implemented_by",
                    "template catalog module reference",
                ])
                .with_context(|| {
                    format!(
                        "failed to index authoring contract edge for {}",
                        entry.template_title
                    )
                })?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Default)]
struct ModuleContractIndexRecord {
    module_title: String,
    usage_count: usize,
    function_names: Vec<String>,
    example_pages: Vec<String>,
    referenced_by_templates: Vec<String>,
}

fn load_module_contract_usage(
    connection: &Connection,
) -> Result<BTreeMap<String, ModuleContractIndexRecord>> {
    if !table_exists(connection, "indexed_module_invocations")? {
        return Ok(BTreeMap::new());
    }

    let mut statement = connection
        .prepare(
            "SELECT module_title, function_name, source_title
             FROM indexed_module_invocations
             ORDER BY module_title ASC, function_name ASC, source_title ASC",
        )
        .context("failed to prepare module contract usage query")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .context("failed to run module contract usage query")?;

    let mut out = BTreeMap::<String, ModuleContractIndexRecord>::new();
    let mut function_sets = BTreeMap::<String, BTreeSet<String>>::new();
    let mut page_sets = BTreeMap::<String, BTreeSet<String>>::new();
    for row in rows {
        let (module_title, function_name, source_title) =
            row.context("failed to decode module contract usage row")?;
        let normalized = normalize_contract_title(&module_title);
        if normalized.is_empty() {
            continue;
        }
        let key = normalized.to_ascii_lowercase();
        let record = out
            .entry(key.clone())
            .or_insert_with(|| ModuleContractIndexRecord {
                module_title: normalized,
                ..ModuleContractIndexRecord::default()
            });
        record.usage_count = record.usage_count.saturating_add(1);
        function_sets
            .entry(key.clone())
            .or_default()
            .insert(function_name);
        page_sets.entry(key).or_default().insert(source_title);
    }

    for (key, record) in &mut out {
        record.function_names = function_sets
            .remove(key)
            .unwrap_or_default()
            .into_iter()
            .collect();
        record.example_pages = page_sets
            .remove(key)
            .unwrap_or_default()
            .into_iter()
            .collect();
    }
    Ok(out)
}

fn template_contract_parameter_keys(entry: &TemplateCatalogEntry) -> Vec<String> {
    let mut keys = entry
        .declared_parameter_keys
        .iter()
        .chain(entry.parameters.iter().map(|parameter| &parameter.name))
        .map(|value| normalize_template_parameter_key(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    for parameter in &entry.parameters {
        for alias in &parameter.aliases {
            let alias = normalize_template_parameter_key(alias);
            if !alias.is_empty() {
                keys.insert(alias);
            }
        }
    }
    keys.into_iter().collect()
}

fn template_contract_semantic_text(
    entry: &TemplateCatalogEntry,
    parameter_keys: &[String],
) -> String {
    normalize_spaces(
        &[
            entry.template_title.clone(),
            entry.category.clone(),
            entry.summary_text.clone().unwrap_or_default(),
            entry.redirect_aliases.join(" "),
            entry.usage_aliases.join(" "),
            entry.documentation_titles.join(" "),
            entry.implementation_titles.join(" "),
            entry.module_titles.join(" "),
            entry.recommendation_tags.join(" "),
            parameter_keys.join(" "),
            entry.example_pages.join(" "),
        ]
        .join(" "),
    )
}

fn module_contract_semantic_text(record: &ModuleContractIndexRecord) -> String {
    normalize_spaces(
        &[
            record.module_title.clone(),
            record.function_names.join(" "),
            record.example_pages.join(" "),
            record.referenced_by_templates.join(" "),
        ]
        .join(" "),
    )
}

fn authoring_contract_key(profile_id: &str, kind: &str, title: &str) -> String {
    format!(
        "{}:{}:{}",
        profile_id.trim().to_ascii_lowercase(),
        kind.trim().to_ascii_lowercase(),
        normalize_contract_title(title)
    )
}

fn normalize_contract_title(value: &str) -> String {
    normalize_spaces(&value.replace('_', " "))
}

pub(super) fn template_catalog_artifact_key(profile_id: &str) -> String {
    format!(
        "template_catalog:{}",
        profile_id.trim().to_ascii_lowercase()
    )
}

pub(super) fn decode_current_template_catalog(value: &str) -> Result<Option<TemplateCatalog>> {
    let catalog: TemplateCatalog =
        serde_json::from_str(value).context("failed to decode template catalog")?;
    if catalog.schema_version == TEMPLATE_CATALOG_SCHEMA_VERSION {
        Ok(Some(catalog))
    } else {
        Ok(None)
    }
}
