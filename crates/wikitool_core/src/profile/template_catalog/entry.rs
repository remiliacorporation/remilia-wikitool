use super::*;

pub(super) fn build_catalog_entry(
    template: LocalTemplateRecord,
    usage: Option<&crate::knowledge::templates::TemplateUsageSummary>,
    implementation_pages: Option<&[crate::knowledge::templates::TemplateImplementationRecord]>,
    redirect_aliases: &[String],
    overlay: &ProfileOverlay,
) -> TemplateCatalogEntry {
    let usage_aliases = merge_titles(
        usage.map(|item| item.aliases.clone()).unwrap_or_default(),
        None,
    );
    let parameters = merge_parameters(
        template.templatedata.as_ref(),
        &template.declared_parameter_keys,
        usage,
    );
    let summary_text = template
        .summary_text
        .clone()
        .or_else(|| {
            template
                .templatedata
                .as_ref()
                .and_then(|item| item.description.clone())
        })
        .or_else(|| {
            usage.and_then(|item| {
                item.implementation_preview
                    .as_deref()
                    .and_then(extract_summary_text)
            })
        });
    let documentation_titles = merge_titles(
        template
            .documentation_pages
            .iter()
            .map(|page| page.title.clone())
            .collect(),
        implementation_pages.map(|item| {
            item.iter()
                .filter(|page| page.role == "documentation")
                .map(|page| page.title.clone())
                .collect()
        }),
    );
    let implementation_titles = merge_titles(
        usage
            .map(|item| item.implementation_titles.clone())
            .unwrap_or_default(),
        None,
    );
    let module_titles = merge_titles(
        template.module_titles.clone(),
        Some(
            implementation_pages
                .map(|item| {
                    item.iter()
                        .filter(|page| page.role == "module")
                        .map(|page| page.title.clone())
                        .collect()
                })
                .unwrap_or_else(|| {
                    implementation_titles
                        .iter()
                        .filter(|title| title.starts_with("Module:"))
                        .cloned()
                        .collect()
                }),
        ),
    );

    TemplateCatalogEntry {
        template_title: template.template_title.clone(),
        relative_path: template.relative_path,
        category: template.category,
        summary_text,
        templatedata: template.templatedata,
        redirect_aliases: merge_titles(redirect_aliases.to_vec(), None),
        usage_aliases,
        usage_count: usage.map(|item| item.usage_count).unwrap_or(0),
        distinct_page_count: usage.map(|item| item.distinct_page_count).unwrap_or(0),
        example_pages: usage
            .map(|item| item.example_pages.clone())
            .unwrap_or_default(),
        documentation_titles,
        implementation_titles,
        implementation_preview: usage.and_then(|item| item.implementation_preview.clone()),
        module_titles,
        declared_parameter_keys: template.declared_parameter_keys,
        parameters,
        examples: merge_examples(template.local_examples, usage),
        recommendation_tags: recommendation_tags(&template.template_title, overlay),
    }
}

fn merge_parameters(
    templatedata: Option<&TemplateDataRecord>,
    declared_parameter_keys: &[String],
    usage: Option<&crate::knowledge::templates::TemplateUsageSummary>,
) -> Vec<TemplateCatalogParameter> {
    let mut templatedata_map = BTreeMap::<String, &TemplateDataParameter>::new();
    let mut alias_to_canonical = BTreeMap::<String, String>::new();
    let mut observed_names = BTreeMap::<String, BTreeSet<String>>::new();
    let mut order = Vec::<String>::new();
    let mut seen = BTreeSet::new();
    if let Some(templatedata) = templatedata {
        for parameter in &templatedata.parameters {
            let canonical_name = canonical_parameter_key(&parameter.name);
            let key = parameter_match_key(&canonical_name);
            templatedata_map.insert(key.clone(), parameter);
            if seen.insert(key) {
                order.push(canonical_name.clone());
            }
            record_parameter_surface(&mut observed_names, &canonical_name, &parameter.name);
            for alias in &parameter.aliases {
                alias_to_canonical.insert(parameter_match_key(alias), canonical_name.clone());
                record_parameter_surface(&mut observed_names, &canonical_name, alias);
            }
        }
    }

    let declared_set = declared_parameter_keys
        .iter()
        .map(|item| parameter_match_key(item))
        .collect::<BTreeSet<_>>();
    for key in declared_parameter_keys {
        let canonical = canonical_parameter_key(key);
        let match_key = parameter_match_key(&canonical);
        if seen.insert(match_key) {
            order.push(canonical);
        }
        let canonical = canonical_parameter_key(key);
        record_parameter_surface(&mut observed_names, &canonical, key);
    }

    let mut usage_map = BTreeMap::<String, LocalUsageAccumulator>::new();
    if let Some(usage) = usage {
        let mut usage_keys = usage
            .parameter_stats
            .iter()
            .map(|parameter| canonical_parameter_key(&parameter.key))
            .collect::<Vec<_>>();
        usage_keys.sort();
        for key in usage_keys {
            let canonical = alias_to_canonical
                .get(&parameter_match_key(&key))
                .cloned()
                .unwrap_or_else(|| key.clone());
            let match_key = parameter_match_key(&canonical);
            if seen.insert(match_key) {
                order.push(canonical);
            }
        }

        for parameter in &usage.parameter_stats {
            let usage_key = canonical_parameter_key(&parameter.key);
            let canonical = alias_to_canonical
                .get(&parameter_match_key(&usage_key))
                .cloned()
                .unwrap_or(usage_key);
            let entry = usage_map
                .entry(parameter_match_key(&canonical))
                .or_default();
            entry.usage_count = entry.usage_count.saturating_add(parameter.usage_count);
            for value in &parameter.example_values {
                if !entry.example_values.iter().any(|item| item == value) {
                    entry.example_values.push(value.clone());
                }
            }
            record_parameter_surface(&mut observed_names, &canonical, &parameter.key);
        }
    }

    let mut out = Vec::new();
    for name in order {
        let match_key = parameter_match_key(&name);
        let templatedata_parameter = templatedata_map.get(&match_key).copied();
        let usage_parameter = usage_map.get(&match_key);
        let mut sources = Vec::new();
        if templatedata_parameter.is_some() {
            sources.push("templatedata".to_string());
        }
        if declared_set.contains(&match_key) {
            sources.push("source".to_string());
        }
        if usage_parameter.is_some() {
            sources.push("usage".to_string());
        }
        let observed_names_for_parameter = observed_names
            .remove(&match_key)
            .map(|items| items.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        let aliases = merged_parameter_aliases(
            &name,
            templatedata_parameter
                .map(|item| item.aliases.as_slice())
                .unwrap_or(&[]),
            &observed_names_for_parameter,
        );

        out.push(TemplateCatalogParameter {
            name: name.clone(),
            aliases,
            observed_names: observed_names_for_parameter,
            sources,
            label: templatedata_parameter.and_then(|item| item.label.clone()),
            description: templatedata_parameter.and_then(|item| item.description.clone()),
            param_type: templatedata_parameter.and_then(|item| item.param_type.clone()),
            required: templatedata_parameter
                .map(|item| item.required)
                .unwrap_or(false),
            suggested: templatedata_parameter
                .map(|item| item.suggested)
                .unwrap_or(false),
            deprecated: templatedata_parameter
                .map(|item| item.deprecated)
                .unwrap_or(false),
            usage_count: usage_parameter.map(|item| item.usage_count).unwrap_or(0),
            example_values: usage_parameter
                .map(|item| item.example_values.clone())
                .unwrap_or_default(),
            example: templatedata_parameter.and_then(|item| item.example.clone()),
            default_value: templatedata_parameter.and_then(|item| item.default_value.clone()),
            suggested_values: templatedata_parameter
                .map(|item| item.suggested_values.clone())
                .unwrap_or_default(),
            auto_value: templatedata_parameter.and_then(|item| item.auto_value.clone()),
        });
    }
    out
}

fn merge_examples(
    local_examples: Vec<LocalTemplateExample>,
    usage: Option<&crate::knowledge::templates::TemplateUsageSummary>,
) -> Vec<TemplateCatalogExample> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for example in local_examples {
        let key = example.invocation_text.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(TemplateCatalogExample {
                source_kind: "documentation".to_string(),
                source_title: Some(example.source_title),
                source_relative_path: Some(example.source_relative_path),
                parameter_keys: example.parameter_keys,
                invocation_text: example.invocation_text,
                token_estimate: 0,
            });
        }
    }
    if let Some(usage) = usage {
        for example in &usage.example_invocations {
            let key = example.invocation_text.to_ascii_lowercase();
            if seen.insert(key) {
                out.push(TemplateCatalogExample {
                    source_kind: "indexed_usage".to_string(),
                    source_title: Some(example.source_title.clone()),
                    source_relative_path: Some(example.source_relative_path.clone()),
                    parameter_keys: example
                        .parameter_keys
                        .iter()
                        .map(|key| canonical_parameter_key(key))
                        .collect(),
                    invocation_text: example.invocation_text.clone(),
                    token_estimate: example.token_estimate,
                });
            }
        }
    }
    out
}

fn recommendation_tags(template_title: &str, overlay: &ProfileOverlay) -> Vec<String> {
    let mut tags = Vec::new();
    if overlay.authoring.article_quality_template.as_deref() == Some(template_title) {
        tags.push("required_quality_banner".to_string());
    }
    if overlay.authoring.references_template.as_deref() == Some(template_title) {
        tags.push("required_references_template".to_string());
    }
    if overlay
        .citations
        .preferred_templates
        .iter()
        .any(|rule| rule.template_title == template_title)
    {
        tags.push("preferred_citation_template".to_string());
    }
    if overlay
        .remilia
        .infobox_preferences
        .iter()
        .any(|rule| rule.template_title == template_title)
    {
        tags.push("preferred_infobox_template".to_string());
    }
    tags
}

fn merge_titles(left: Vec<String>, right: Option<Vec<String>>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for title in left.into_iter().chain(right.unwrap_or_default()) {
        let normalized = title.to_ascii_lowercase();
        if !title.is_empty() && seen.insert(normalized) {
            out.push(title);
        }
    }
    out
}

fn canonical_parameter_key(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(rest) = trimmed.strip_prefix('$')
        && !rest.is_empty()
        && rest.chars().all(|ch| ch.is_ascii_digit())
    {
        return rest.to_string();
    }
    trimmed.to_string()
}

fn parameter_match_key(value: &str) -> String {
    normalize_template_parameter_key(&canonical_parameter_key(value))
}

fn record_parameter_surface(
    observed_names: &mut BTreeMap<String, BTreeSet<String>>,
    canonical_name: &str,
    observed_name: &str,
) {
    let observed_name = canonical_parameter_key(observed_name);
    if observed_name.is_empty() {
        return;
    }
    observed_names
        .entry(parameter_match_key(canonical_name))
        .or_default()
        .insert(observed_name);
}

fn merged_parameter_aliases(
    canonical_name: &str,
    templatedata_aliases: &[String],
    observed_names: &[String],
) -> Vec<String> {
    let canonical_match = parameter_match_key(canonical_name);
    let mut aliases = templatedata_aliases
        .iter()
        .chain(observed_names)
        .map(|alias| canonical_parameter_key(alias))
        .filter(|alias| !alias.is_empty() && alias != canonical_name)
        .collect::<BTreeSet<_>>();
    aliases.retain(|alias| parameter_match_key(alias) == canonical_match);
    aliases.into_iter().collect()
}
