use super::*;

pub(crate) fn extract_page_artifacts(content: &str) -> ParsedPageArtifacts {
    let sections = parse_content_sections(content);
    ParsedPageArtifacts {
        section_records: extract_section_records_from_sections(&sections),
        context_chunks: chunk_article_context_from_sections(&sections),
        template_invocations: extract_template_invocations(content),
        module_invocations: extract_module_invocations(content),
        references: extract_reference_records_from_sections(&sections),
        media: extract_media_records_from_sections(&sections),
    }
}

pub(crate) fn build_page_semantic_profile(
    file: &ScannedFile,
    links: &[ParsedLink],
    artifacts: &ParsedPageArtifacts,
) -> IndexedSemanticProfileRecord {
    let summary_text = artifacts
        .section_records
        .iter()
        .find(|section| section.section_heading.is_none())
        .map(|section| section.summary_text.clone())
        .filter(|summary| !summary.is_empty())
        .or_else(|| {
            artifacts
                .context_chunks
                .first()
                .map(|chunk| summarize_words(&chunk.chunk_text, AUTHORING_PAGE_SUMMARY_WORD_LIMIT))
                .filter(|summary| !summary.is_empty())
        })
        .unwrap_or_default();
    let section_headings = collect_normalized_string_list(
        artifacts
            .section_records
            .iter()
            .filter_map(|section| section.section_heading.clone()),
    );
    let category_titles = collect_normalized_string_list(
        links
            .iter()
            .filter(|link| link.is_category_membership)
            .map(|link| link.target_title.clone()),
    );
    let link_titles = collect_normalized_string_list(
        links
            .iter()
            .filter(|link| !link.is_category_membership)
            .map(|link| link.target_title.clone()),
    );
    let template_titles = collect_normalized_string_list(
        artifacts
            .template_invocations
            .iter()
            .map(|invocation| invocation.template_title.clone()),
    );
    let template_parameter_keys = collect_normalized_string_list(
        artifacts
            .template_invocations
            .iter()
            .flat_map(|invocation| invocation.parameter_keys.iter().cloned()),
    );
    let reference_titles = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.reference_title.clone()),
    );
    let reference_containers = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.source_container.clone()),
    );
    let reference_domains = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.source_domain.clone()),
    );
    let reference_source_families = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.source_family.clone()),
    );
    let reference_authorities = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .map(|reference| reference.source_authority.clone()),
    );
    let reference_identifiers = collect_normalized_string_list(
        artifacts
            .references
            .iter()
            .flat_map(|reference| reference.identifier_entries.iter().cloned()),
    );
    let media_titles = collect_normalized_string_list(
        artifacts.media.iter().map(|media| media.file_title.clone()),
    );
    let media_captions = collect_normalized_string_list(
        artifacts
            .media
            .iter()
            .map(|media| media.caption_text.clone()),
    );
    let template_implementation_titles = collect_template_implementation_terms(file, artifacts);
    let module_terms = collect_normalized_string_list(
        artifacts.module_invocations.iter().flat_map(|invocation| {
            [
                invocation.module_title.clone(),
                invocation.function_name.clone(),
            ]
        }),
    );
    let mut profile = IndexedSemanticProfileRecord {
        source_title: file.title.clone(),
        source_namespace: file.namespace.clone(),
        summary_text,
        section_headings,
        category_titles,
        template_titles,
        template_parameter_keys,
        link_titles,
        reference_titles,
        reference_containers,
        reference_domains,
        reference_source_families,
        reference_authorities,
        reference_identifiers,
        media_titles,
        media_captions,
        template_implementation_titles,
        semantic_text: String::new(),
        token_estimate: 0,
    };
    profile.semantic_text = build_page_semantic_text(file, &profile, &module_terms);
    profile.token_estimate = estimate_tokens(&profile.semantic_text);
    profile
}

pub(crate) fn build_page_semantic_text(
    file: &ScannedFile,
    profile: &IndexedSemanticProfileRecord,
    module_terms: &[String],
) -> String {
    let mut terms = Vec::new();
    let mut seen = BTreeSet::new();

    push_semantic_term(&mut terms, &mut seen, &file.title);
    push_semantic_term(&mut terms, &mut seen, &profile.summary_text);
    for values in [
        &profile.section_headings,
        &profile.category_titles,
        &profile.template_titles,
        &profile.template_parameter_keys,
        &profile.link_titles,
        &profile.reference_titles,
        &profile.reference_containers,
        &profile.reference_domains,
        &profile.reference_source_families,
        &profile.reference_authorities,
        &profile.reference_identifiers,
        &profile.media_titles,
        &profile.media_captions,
        &profile.template_implementation_titles,
        module_terms,
    ] {
        for value in values {
            push_semantic_term(&mut terms, &mut seen, value);
        }
    }

    if terms.is_empty() {
        push_semantic_term(&mut terms, &mut seen, &file.title);
    }
    terms.join("\n")
}

pub(crate) fn collect_normalized_string_list<I>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    values
        .into_iter()
        .map(|value| normalize_spaces(&value.replace('_', " ")))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn collect_template_implementation_terms(
    file: &ScannedFile,
    artifacts: &ParsedPageArtifacts,
) -> Vec<String> {
    if file.namespace != Namespace::Template.as_str() {
        return Vec::new();
    }

    let mut terms = Vec::new();
    terms.extend(artifacts.module_invocations.iter().flat_map(|invocation| {
        [
            invocation.module_title.clone(),
            invocation.function_name.clone(),
        ]
    }));
    terms.extend(
        artifacts
            .template_invocations
            .iter()
            .map(|invocation| invocation.template_title.clone())
            .filter(|title| !title.eq_ignore_ascii_case(&file.title)),
    );
    if file.title.ends_with("/doc") {
        if let Some(base_title) = file.title.strip_suffix("/doc") {
            terms.push(base_title.to_string());
        }
    } else {
        terms.push(format!("{}/doc", file.title));
    }
    collect_normalized_string_list(terms)
}

pub(crate) fn maybe_record_template_implementation_seed(
    seeds: &mut BTreeMap<String, TemplateImplementationSeed>,
    file: &ScannedFile,
    artifacts: &ParsedPageArtifacts,
) {
    if file.namespace != Namespace::Template.as_str() {
        return;
    }

    let mut seed = TemplateImplementationSeed {
        template_dependencies: artifacts
            .template_invocations
            .iter()
            .map(|invocation| invocation.template_title.clone())
            .filter(|title| !title.eq_ignore_ascii_case(&file.title))
            .collect(),
        module_dependencies: artifacts
            .module_invocations
            .iter()
            .map(|invocation| invocation.module_title.clone())
            .collect(),
    };
    seed.template_dependencies.sort();
    seed.template_dependencies.dedup();
    seed.module_dependencies.sort();
    seed.module_dependencies.dedup();
    seeds.insert(file.title.to_ascii_lowercase(), seed);
}

pub(crate) fn persist_template_implementation_pages(
    statement: &mut rusqlite::Statement<'_>,
    files: &[ScannedFile],
    seeds: &BTreeMap<String, TemplateImplementationSeed>,
) -> Result<()> {
    let page_map = files
        .iter()
        .map(|file| (file.title.to_ascii_lowercase(), file))
        .collect::<BTreeMap<_, _>>();

    let mut active_templates = BTreeSet::new();
    for file in files {
        if file.namespace == Namespace::Template.as_str() && !file.title.ends_with("/doc") {
            active_templates.insert(file.title.clone());
        }
    }
    for seed in seeds.values() {
        for dependency in &seed.template_dependencies {
            active_templates.insert(dependency.clone());
        }
    }

    for template_title in active_templates {
        let normalized_key = template_title.to_ascii_lowercase();
        if let Some(file) = page_map.get(&normalized_key) {
            statement
                .execute(params![
                    template_title.as_str(),
                    file.title.as_str(),
                    file.namespace.as_str(),
                    file.relative_path.as_str(),
                    "template",
                ])
                .with_context(|| {
                    format!(
                        "failed to insert template implementation page for {}",
                        template_title
                    )
                })?;
        }

        let doc_title = format!("{template_title}/doc");
        if let Some(file) = page_map.get(&doc_title.to_ascii_lowercase()) {
            statement
                .execute(params![
                    template_title.as_str(),
                    file.title.as_str(),
                    file.namespace.as_str(),
                    file.relative_path.as_str(),
                    "documentation",
                ])
                .with_context(|| {
                    format!(
                        "failed to insert template documentation page for {}",
                        template_title
                    )
                })?;
        }

        for seed_key in [normalized_key.clone(), doc_title.to_ascii_lowercase()] {
            let Some(seed) = seeds.get(&seed_key) else {
                continue;
            };

            for module_title in &seed.module_dependencies {
                if let Some(file) = page_map.get(&module_title.to_ascii_lowercase()) {
                    statement
                        .execute(params![
                            template_title.as_str(),
                            file.title.as_str(),
                            file.namespace.as_str(),
                            file.relative_path.as_str(),
                            "module",
                        ])
                        .with_context(|| {
                            format!(
                                "failed to insert template module implementation for {}",
                                template_title
                            )
                        })?;
                }
            }

            for dependency_title in &seed.template_dependencies {
                if let Some(file) = page_map.get(&dependency_title.to_ascii_lowercase()) {
                    statement
                        .execute(params![
                            template_title.as_str(),
                            file.title.as_str(),
                            file.namespace.as_str(),
                            file.relative_path.as_str(),
                            "dependency",
                        ])
                        .with_context(|| {
                            format!(
                                "failed to insert template dependency implementation for {}",
                                template_title
                            )
                        })?;
                }
            }
        }
    }

    Ok(())
}

pub(crate) fn push_semantic_term(out: &mut Vec<String>, seen: &mut BTreeSet<String>, value: &str) {
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return;
    }
    let key = normalized.to_ascii_lowercase();
    if seen.insert(key) {
        out.push(normalized.clone());
    }
    if let Some((_, body)) = normalized.split_once(':') {
        let body = normalize_spaces(body);
        if body.is_empty() {
            return;
        }
        let body_key = body.to_ascii_lowercase();
        if seen.insert(body_key) {
            out.push(body);
        }
    }
}
