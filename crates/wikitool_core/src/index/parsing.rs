use super::*;
use super::model::*;

pub(crate) fn parse_parameter_key_list(value: &str) -> Vec<String> {
    if value.trim().is_empty() || value == NO_PARAMETER_KEYS_SENTINEL {
        return Vec::new();
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) fn serialize_string_list(values: &[String]) -> String {
    let normalized = values
        .iter()
        .map(|value| normalize_spaces(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        return NO_STRING_LIST_SENTINEL.to_string();
    }
    normalized.join("\n")
}

pub(crate) fn parse_string_list(value: &str) -> Vec<String> {
    if value.trim().is_empty() || value == NO_STRING_LIST_SENTINEL {
        return Vec::new();
    }
    value
        .lines()
        .map(normalize_spaces)
        .filter(|item| !item.is_empty())
        .collect()
}

pub(crate) fn normalize_non_empty_string(value: String) -> Option<String> {
    let normalized = normalize_spaces(&value);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(crate) fn canonical_parameter_key_list(keys: &[String]) -> String {
    if keys.is_empty() {
        return NO_PARAMETER_KEYS_SENTINEL.to_string();
    }
    let mut normalized = Vec::new();
    for key in keys {
        let key = normalize_template_parameter_key(key);
        if !key.is_empty() {
            normalized.push(key);
        }
    }
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        return NO_PARAMETER_KEYS_SENTINEL.to_string();
    }
    normalized.join(",")
}

pub(crate) fn apply_context_chunk_budget(
    chunks: Vec<LocalContextChunk>,
    max_chunks: usize,
    token_budget: usize,
) -> Vec<LocalContextChunk> {
    let mut out = Vec::new();
    let mut used_tokens = 0usize;
    for chunk in chunks {
        if out.len() >= max_chunks {
            break;
        }
        let next_tokens = used_tokens.saturating_add(chunk.token_estimate);
        if !out.is_empty() && next_tokens > token_budget {
            break;
        }
        used_tokens = next_tokens;
        out.push(chunk);
    }
    out
}

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

pub(crate) fn chunk_article_context(content: &str) -> Vec<ArticleContextChunkRow> {
    let sections = parse_content_sections(content);
    chunk_article_context_from_sections(&sections)
}

pub(crate) fn extract_section_records(content: &str) -> Vec<IndexedSectionRecord> {
    let sections = parse_content_sections(content);
    extract_section_records_from_sections(&sections)
}

pub(crate) fn extract_section_records_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<IndexedSectionRecord> {
    sections
        .iter()
        .map(|section| IndexedSectionRecord {
            section_heading: section.section_heading.clone(),
            section_level: section.section_level,
            summary_text: summarize_words(
                &normalize_multiline_spaces(&section.section_text),
                AUTHORING_PAGE_SUMMARY_WORD_LIMIT,
            ),
            token_estimate: estimate_tokens(&section.section_text),
            section_text: normalize_multiline_spaces(&section.section_text),
        })
        .collect()
}

pub(crate) fn chunk_article_context_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<ArticleContextChunkRow> {
    let mut out = Vec::new();
    for section in sections {
        for chunk_text in chunk_section_text(&section.section_text) {
            out.push(ArticleContextChunkRow {
                section_heading: section.section_heading.clone(),
                token_estimate: estimate_tokens(&chunk_text),
                chunk_text,
            });
        }
    }
    out
}

pub(crate) fn parse_content_sections(content: &str) -> Vec<ParsedContentSection> {
    let mut out = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_level = 1u8;
    let mut current_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((level, heading)) = parse_heading_line(trimmed) {
            flush_content_section(
                &mut out,
                current_heading.take(),
                current_level,
                &current_lines,
            );
            current_lines.clear();
            current_heading = Some(heading);
            current_level = level;
            continue;
        }
        current_lines.push(line);
    }
    flush_content_section(&mut out, current_heading, current_level, &current_lines);
    out
}

pub(crate) fn flush_content_section(
    out: &mut Vec<ParsedContentSection>,
    section_heading: Option<String>,
    section_level: u8,
    lines: &[&str],
) {
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() {
        return;
    }
    out.push(ParsedContentSection {
        section_heading,
        section_level,
        section_text: text,
    });
}

pub(crate) fn chunk_section_text(section_text: &str) -> Vec<String> {
    let paragraphs = section_text
        .split("\n\n")
        .map(normalize_multiline_spaces)
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>();
    if paragraphs.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current_parts = Vec::<String>::new();
    let mut current_words = 0usize;
    for paragraph in paragraphs {
        let paragraph_words = count_words(&paragraph);
        if paragraph_words > INDEX_CHUNK_WORD_TARGET {
            if !current_parts.is_empty() {
                out.push(current_parts.join(" "));
                current_parts.clear();
                current_words = 0;
            }
            out.extend(split_text_by_words(&paragraph, INDEX_CHUNK_WORD_TARGET));
            continue;
        }
        if !current_parts.is_empty()
            && current_words.saturating_add(paragraph_words) > INDEX_CHUNK_WORD_TARGET
        {
            out.push(current_parts.join(" "));
            current_parts.clear();
            current_words = 0;
        }
        current_words = current_words.saturating_add(paragraph_words);
        current_parts.push(paragraph);
    }
    if !current_parts.is_empty() {
        out.push(current_parts.join(" "));
    }
    out
}

pub(crate) fn extract_reference_records_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<IndexedReferenceRecord> {
    let mut out = Vec::new();
    for section in sections {
        out.extend(extract_reference_records_for_section(
            section.section_heading.clone(),
            &section.section_text,
        ));
    }
    out
}

pub(crate) fn extract_reference_records(content: &str) -> Vec<LocalReferenceUsage> {
    extract_reference_records_from_sections(&parse_content_sections(content))
        .into_iter()
        .map(|record| LocalReferenceUsage {
            section_heading: record.section_heading,
            reference_name: record.reference_name,
            reference_group: record.reference_group,
            citation_profile: record.citation_profile,
            citation_family: record.citation_family,
            primary_template_title: record.primary_template_title,
            source_type: record.source_type,
            source_origin: record.source_origin,
            source_family: record.source_family,
            authority_kind: record.authority_kind,
            source_authority: record.source_authority,
            reference_title: record.reference_title,
            source_container: record.source_container,
            source_author: record.source_author,
            source_domain: record.source_domain,
            source_date: record.source_date,
            canonical_url: record.canonical_url,
            identifier_keys: record.identifier_keys,
            identifier_entries: record.identifier_entries,
            source_urls: record.source_urls,
            retrieval_signals: record.retrieval_signals,
            summary_text: record.summary_text,
            template_titles: record.template_titles,
            link_titles: record.link_titles,
            token_estimate: record.token_estimate,
        })
        .take(CONTEXT_REFERENCE_LIMIT)
        .collect()
}

pub(crate) fn extract_reference_records_for_section(
    section_heading: Option<String>,
    content: &str,
) -> Vec<IndexedReferenceRecord> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if !starts_with_html_tag(bytes, cursor, "ref") {
            cursor += 1;
            continue;
        }
        let Some((tag_end, tag_body, self_closing)) = parse_open_tag(content, cursor, "ref") else {
            cursor += 1;
            continue;
        };
        let attributes = parse_html_attributes(&tag_body);
        let reference_name = attributes
            .get("name")
            .map(|value| normalize_spaces(value))
            .filter(|value| !value.is_empty());
        let reference_group = attributes
            .get("group")
            .map(|value| normalize_spaces(value))
            .filter(|value| !value.is_empty());

        let (reference_wikitext, reference_body, next_cursor) = if self_closing {
            (content[cursor..tag_end].to_string(), String::new(), tag_end)
        } else if let Some((close_start, close_end)) =
            find_closing_html_tag(content, tag_end, "ref")
        {
            (
                content[cursor..close_end].to_string(),
                content[tag_end..close_start].to_string(),
                close_end,
            )
        } else {
            (content[cursor..tag_end].to_string(), String::new(), tag_end)
        };

        let template_titles = extract_template_titles(&reference_body);
        let link_titles = extract_link_titles(&reference_body);
        let analysis = analyze_reference_body(
            &reference_body,
            &template_titles,
            &link_titles,
            reference_name.as_deref(),
            reference_group.as_deref(),
        );
        let mut summary_text = flatten_markup_excerpt(&reference_body);
        if summary_text.is_empty() {
            summary_text = analysis.summary_hint.clone();
        }
        if summary_text.is_empty() && !template_titles.is_empty() {
            summary_text = template_titles.join(", ");
        }
        if summary_text.is_empty()
            && let Some(name) = &reference_name
        {
            summary_text = format!("Named reference {name}");
        }
        if summary_text.is_empty() {
            summary_text = "<ref>".to_string();
        }

        let token_estimate = estimate_tokens(&reference_wikitext);
        out.push(IndexedReferenceRecord {
            section_heading: section_heading.clone(),
            reference_name,
            reference_group,
            citation_profile: analysis.citation_profile,
            citation_family: analysis.citation_family,
            primary_template_title: analysis.primary_template_title,
            source_type: analysis.source_type,
            source_origin: analysis.source_origin,
            source_family: analysis.source_family,
            authority_kind: analysis.authority_kind,
            source_authority: analysis.source_authority,
            reference_title: analysis.reference_title,
            source_container: analysis.source_container,
            source_author: analysis.source_author,
            source_domain: analysis.source_domain,
            source_date: analysis.source_date,
            canonical_url: analysis.canonical_url,
            identifier_keys: analysis.identifier_keys,
            identifier_entries: analysis.identifier_entries,
            source_urls: analysis.source_urls,
            retrieval_signals: analysis.retrieval_signals,
            summary_text: summarize_words(&summary_text, AUTHORING_PAGE_SUMMARY_WORD_LIMIT),
            reference_wikitext,
            template_titles,
            link_titles,
            token_estimate,
        });
        cursor = next_cursor.max(cursor.saturating_add(1));
    }

    out
}

pub(crate) fn extract_media_records_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<IndexedMediaRecord> {
    let mut out = Vec::new();
    for section in sections {
        out.extend(extract_media_records_for_section(
            section.section_heading.clone(),
            &section.section_text,
        ));
    }
    out
}

pub(crate) fn extract_media_records(content: &str) -> Vec<LocalMediaUsage> {
    extract_media_records_from_sections(&parse_content_sections(content))
        .into_iter()
        .map(|record| LocalMediaUsage {
            section_heading: record.section_heading,
            file_title: record.file_title,
            media_kind: record.media_kind,
            caption_text: record.caption_text,
            options: record.options,
            token_estimate: record.token_estimate,
        })
        .take(CONTEXT_MEDIA_LIMIT)
        .collect()
}

pub(crate) fn extract_media_records_for_section(
    section_heading: Option<String>,
    content: &str,
) -> Vec<IndexedMediaRecord> {
    let mut out = extract_inline_media_records(section_heading.clone(), content);
    out.extend(extract_gallery_media_records(section_heading, content));
    out
}

pub(crate) fn extract_inline_media_records(
    section_heading: Option<String>,
    content: &str,
) -> Vec<IndexedMediaRecord> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
            let start = cursor + 2;
            let mut end = start;
            while end + 1 < bytes.len() {
                if bytes[end] == b']' && bytes[end + 1] == b']' {
                    break;
                }
                end += 1;
            }
            if end + 1 >= bytes.len() {
                break;
            }

            let inner = &content[start..end];
            if let Some(record) = parse_inline_media_record(section_heading.clone(), inner) {
                out.push(record);
            }
            cursor = end + 2;
            continue;
        }
        cursor += 1;
    }

    out
}

pub(crate) fn extract_gallery_media_records(
    section_heading: Option<String>,
    content: &str,
) -> Vec<IndexedMediaRecord> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if !starts_with_html_tag(bytes, cursor, "gallery") {
            cursor += 1;
            continue;
        }
        let Some((tag_end, tag_body, self_closing)) = parse_open_tag(content, cursor, "gallery")
        else {
            cursor += 1;
            continue;
        };
        if self_closing {
            cursor = tag_end;
            continue;
        }
        let Some((close_start, close_end)) = find_closing_html_tag(content, tag_end, "gallery")
        else {
            cursor = tag_end;
            continue;
        };
        let gallery_options = parse_html_attributes(&tag_body)
            .into_iter()
            .map(|(key, value)| {
                if value.is_empty() {
                    key
                } else {
                    format!("{key}={value}")
                }
            })
            .collect::<Vec<_>>();
        let body = &content[tag_end..close_start];
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(record) =
                parse_gallery_media_line(section_heading.clone(), trimmed, &gallery_options)
            {
                out.push(record);
            }
        }
        cursor = close_end;
    }

    out
}

pub(crate) fn parse_inline_media_record(
    section_heading: Option<String>,
    inner: &str,
) -> Option<IndexedMediaRecord> {
    let trimmed = inner.trim();
    if trimmed.starts_with(':') {
        return None;
    }
    let segments = split_template_segments(trimmed);
    let target = segments.first()?.trim();
    let (file_title, namespace) =
        normalize_title_and_namespace(&normalize_spaces(&target.replace('_', " ")))?;
    if namespace != Namespace::File.as_str() {
        return None;
    }

    let options = segments
        .iter()
        .skip(1)
        .map(|segment| normalize_spaces(segment))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let caption_text = options
        .iter()
        .rev()
        .find(|segment| !is_media_option(segment))
        .map(|segment| flatten_markup_excerpt(segment))
        .unwrap_or_default();

    Some(IndexedMediaRecord {
        section_heading,
        file_title,
        media_kind: "inline".to_string(),
        caption_text: summarize_words(&caption_text, AUTHORING_PAGE_SUMMARY_WORD_LIMIT),
        options,
        token_estimate: estimate_tokens(trimmed),
    })
}

pub(crate) fn parse_gallery_media_line(
    section_heading: Option<String>,
    line: &str,
    gallery_options: &[String],
) -> Option<IndexedMediaRecord> {
    let segments = split_template_segments(line);
    let target = segments.first()?.trim();
    let (file_title, namespace) =
        normalize_title_and_namespace(&normalize_spaces(&target.replace('_', " ")))?;
    if namespace != Namespace::File.as_str() {
        return None;
    }

    let line_options = segments
        .iter()
        .skip(1)
        .map(|segment| normalize_spaces(segment))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let caption_text = line_options
        .iter()
        .rev()
        .find(|segment| !is_media_option(segment))
        .map(|segment| flatten_markup_excerpt(segment))
        .unwrap_or_default();
    let mut options = gallery_options.to_vec();
    options.extend(line_options);

    Some(IndexedMediaRecord {
        section_heading,
        file_title,
        media_kind: "gallery".to_string(),
        caption_text: summarize_words(&caption_text, AUTHORING_PAGE_SUMMARY_WORD_LIMIT),
        options,
        token_estimate: estimate_tokens(line),
    })
}

pub(crate) fn split_text_by_words(text: &str, word_target: usize) -> Vec<String> {
    let words = text.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < words.len() {
        let end = (cursor + word_target.max(1)).min(words.len());
        let chunk_text = words[cursor..end].join(" ");
        if !chunk_text.is_empty() {
            out.push(chunk_text);
        }
        cursor = end;
    }
    out
}

pub(crate) fn parse_heading_line(value: &str) -> Option<(u8, String)> {
    if value.len() < 4 || !value.starts_with('=') || !value.ends_with('=') {
        return None;
    }
    let leading = value.chars().take_while(|ch| *ch == '=').count();
    let trailing = value.chars().rev().take_while(|ch| *ch == '=').count();
    if leading != trailing || !(2..=6).contains(&leading) {
        return None;
    }
    if leading * 2 >= value.len() {
        return None;
    }
    let heading = value[leading..value.len() - trailing].trim();
    if heading.is_empty() {
        return None;
    }
    Some((u8::try_from(leading).unwrap_or(6), heading.to_string()))
}

pub(crate) fn summarize_words(value: &str, max_words: usize) -> String {
    normalize_spaces(value)
        .split_whitespace()
        .take(max_words.max(1))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn normalize_multiline_spaces(value: &str) -> String {
    value
        .lines()
        .map(normalize_spaces)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn estimate_tokens(value: &str) -> usize {
    value.chars().count().div_ceil(4)
}

pub(crate) fn summarize_template_invocations(
    invocations: Vec<ParsedTemplateInvocation>,
    limit: usize,
) -> Vec<LocalTemplateInvocation> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for invocation in invocations {
        let parameter_keys = canonical_parameter_key_list(&invocation.parameter_keys);
        let signature = format!("{}|{}", invocation.template_title, parameter_keys);
        if !seen.insert(signature) {
            continue;
        }
        out.push(LocalTemplateInvocation {
            template_title: invocation.template_title,
            parameter_keys: parse_parameter_key_list(&parameter_keys),
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

pub(crate) fn extract_template_invocations(content: &str) -> Vec<ParsedTemplateInvocation> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut stack = Vec::new();

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            stack.push(cursor + 2);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'}' && bytes[cursor + 1] == b'}' {
            if let Some(start) = stack.pop()
                && cursor >= start
            {
                let inner = &content[start..cursor];
                if let Some(invocation) = parse_template_invocation(inner) {
                    out.push(invocation);
                }
            }
            cursor += 2;
            continue;
        }
        cursor += 1;
    }

    out
}

pub(crate) fn extract_module_invocations(content: &str) -> Vec<ParsedModuleInvocation> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut cursor = 0usize;
    let mut stack = Vec::new();

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            stack.push(cursor + 2);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'}' && bytes[cursor + 1] == b'}' {
            if let Some(start) = stack.pop()
                && cursor >= start
            {
                let inner = &content[start..cursor];
                if let Some(invocation) = parse_module_invocation(inner) {
                    let signature = format!(
                        "{}|{}|{}",
                        invocation.module_title.to_ascii_lowercase(),
                        invocation.function_name.to_ascii_lowercase(),
                        canonical_parameter_key_list(&invocation.parameter_keys)
                    );
                    if seen.insert(signature) {
                        out.push(invocation);
                    }
                }
            }
            cursor += 2;
            continue;
        }
        cursor += 1;
    }

    out
}

pub(crate) fn parse_template_invocation(inner: &str) -> Option<ParsedTemplateInvocation> {
    let segments = split_template_segments(inner);
    let raw_name = segments.first()?.trim();
    let template_title = canonical_template_title(raw_name)?;

    let mut parameter_keys = Vec::new();
    let mut positional_index = 1usize;
    for segment in segments.iter().skip(1) {
        let value = segment.trim();
        if value.is_empty() {
            continue;
        }
        if let Some((key, _)) = split_once_top_level_equals(value) {
            let normalized = normalize_template_parameter_key(&key);
            if !normalized.is_empty() {
                parameter_keys.push(normalized);
                continue;
            }
        }
        parameter_keys.push(format!("${positional_index}"));
        positional_index += 1;
    }
    parameter_keys.sort();
    parameter_keys.dedup();

    Some(ParsedTemplateInvocation {
        template_title,
        parameter_keys,
        raw_wikitext: format!("{{{{{inner}}}}}"),
        token_estimate: estimate_tokens(inner),
    })
}

pub(crate) fn parse_module_invocation(inner: &str) -> Option<ParsedModuleInvocation> {
    let segments = split_template_segments(inner);
    let raw_name = segments.first()?.trim();
    let remainder = raw_name.strip_prefix("#invoke:")?;
    let module_name = normalize_spaces(remainder);
    if module_name.is_empty() {
        return None;
    }
    let function_name = normalize_spaces(segments.get(1).map(String::as_str).unwrap_or(""));
    if function_name.is_empty() {
        return None;
    }

    let mut parameter_keys = Vec::new();
    let mut positional_index = 1usize;
    for segment in segments.iter().skip(2) {
        let value = segment.trim();
        if value.is_empty() {
            continue;
        }
        if let Some((key, _)) = split_once_top_level_equals(value) {
            let normalized = normalize_template_parameter_key(&key);
            if !normalized.is_empty() {
                parameter_keys.push(normalized);
                continue;
            }
        }
        parameter_keys.push(format!("${positional_index}"));
        positional_index += 1;
    }
    parameter_keys.sort();
    parameter_keys.dedup();

    Some(ParsedModuleInvocation {
        module_title: format!("Module:{module_name}"),
        function_name,
        parameter_keys,
        raw_wikitext: format!("{{{{{inner}}}}}"),
        token_estimate: estimate_tokens(inner),
    })
}

pub(crate) fn split_template_segments(inner: &str) -> Vec<String> {
    let chars: Vec<char> = inner.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut cursor = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;

    while cursor < chars.len() {
        let current_char = chars[cursor];
        let next_char = chars.get(cursor + 1).copied();
        if current_char == '{' && next_char == Some('{') {
            template_depth += 1;
            current.push('{');
            current.push('{');
            cursor += 2;
            continue;
        }
        if current_char == '}' && next_char == Some('}') {
            template_depth = template_depth.saturating_sub(1);
            current.push('}');
            current.push('}');
            cursor += 2;
            continue;
        }
        if current_char == '[' && next_char == Some('[') {
            link_depth += 1;
            current.push('[');
            current.push('[');
            cursor += 2;
            continue;
        }
        if current_char == ']' && next_char == Some(']') {
            link_depth = link_depth.saturating_sub(1);
            current.push(']');
            current.push(']');
            cursor += 2;
            continue;
        }
        if current_char == '|' && template_depth == 0 && link_depth == 0 {
            out.push(current.trim().to_string());
            current.clear();
            cursor += 1;
            continue;
        }
        current.push(current_char);
        cursor += 1;
    }

    out.push(current.trim().to_string());
    out
}

pub(crate) fn split_once_top_level_equals(value: &str) -> Option<(String, String)> {
    let chars: Vec<char> = value.chars().collect();
    let mut cursor = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;
    while cursor < chars.len() {
        let current_char = chars[cursor];
        let next_char = chars.get(cursor + 1).copied();
        if current_char == '{' && next_char == Some('{') {
            template_depth += 1;
            cursor += 2;
            continue;
        }
        if current_char == '}' && next_char == Some('}') {
            template_depth = template_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if current_char == '[' && next_char == Some('[') {
            link_depth += 1;
            cursor += 2;
            continue;
        }
        if current_char == ']' && next_char == Some(']') {
            link_depth = link_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if current_char == '=' && template_depth == 0 && link_depth == 0 {
            let key = chars[..cursor].iter().collect::<String>();
            let value = chars[cursor + 1..].iter().collect::<String>();
            return Some((key, value));
        }
        cursor += 1;
    }
    None
}

pub(crate) fn starts_with_html_tag(bytes: &[u8], cursor: usize, tag_name: &str) -> bool {
    let tag_bytes = tag_name.as_bytes();
    if cursor + tag_bytes.len() + 1 >= bytes.len() || bytes[cursor] != b'<' {
        return false;
    }
    let start = cursor + 1;
    let end = start + tag_bytes.len();
    if end > bytes.len() || !bytes[start..end].eq_ignore_ascii_case(tag_bytes) {
        return false;
    }
    matches!(
        bytes.get(end).copied(),
        Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
    )
}

pub(crate) fn parse_open_tag(content: &str, start: usize, tag_name: &str) -> Option<(usize, String, bool)> {
    let bytes = content.as_bytes();
    if !starts_with_html_tag(bytes, start, tag_name) {
        return None;
    }

    let mut cursor = start + tag_name.len() + 1;
    let mut quote = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
            cursor += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            cursor += 1;
            continue;
        }
        if byte == b'>' {
            let raw_body = &content[start + tag_name.len() + 1..cursor];
            let trimmed = raw_body.trim();
            let self_closing = trimmed.ends_with('/');
            let body = if self_closing {
                trimmed.trim_end_matches('/').trim_end().to_string()
            } else {
                trimmed.to_string()
            };
            return Some((cursor + 1, body, self_closing));
        }
        cursor += 1;
    }
    None
}

pub(crate) fn find_closing_html_tag(content: &str, start: usize, tag_name: &str) -> Option<(usize, usize)> {
    let bytes = content.as_bytes();
    let needle = format!("</{tag_name}");
    let needle_bytes = needle.as_bytes();
    let mut cursor = start;

    while cursor + needle_bytes.len() < bytes.len() {
        if bytes[cursor] == b'<'
            && bytes[cursor..cursor + needle_bytes.len()].eq_ignore_ascii_case(needle_bytes)
        {
            let boundary = bytes.get(cursor + needle_bytes.len()).copied();
            if !matches!(
                boundary,
                Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
            ) {
                cursor += 1;
                continue;
            }
            let mut end = cursor + needle_bytes.len();
            while end < bytes.len() && bytes[end] != b'>' {
                end += 1;
            }
            if end < bytes.len() {
                return Some((cursor, end + 1));
            }
        }
        cursor += 1;
    }
    None
}

pub(crate) fn parse_html_attributes(value: &str) -> BTreeMap<String, String> {
    let chars = value.chars().collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut out = BTreeMap::new();

    while cursor < chars.len() {
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor >= chars.len() {
            break;
        }

        let key_start = cursor;
        while cursor < chars.len()
            && !chars[cursor].is_whitespace()
            && chars[cursor] != '='
            && chars[cursor] != '/'
        {
            cursor += 1;
        }
        let key = chars[key_start..cursor]
            .iter()
            .collect::<String>()
            .trim()
            .to_ascii_lowercase();
        if key.is_empty() {
            cursor += 1;
            continue;
        }

        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        let mut value_out = String::new();
        if cursor < chars.len() && chars[cursor] == '=' {
            cursor += 1;
            while cursor < chars.len() && chars[cursor].is_whitespace() {
                cursor += 1;
            }
            if cursor < chars.len() && (chars[cursor] == '"' || chars[cursor] == '\'') {
                let quote = chars[cursor];
                cursor += 1;
                let start = cursor;
                while cursor < chars.len() && chars[cursor] != quote {
                    cursor += 1;
                }
                value_out = chars[start..cursor].iter().collect::<String>();
                if cursor < chars.len() {
                    cursor += 1;
                }
            } else {
                let start = cursor;
                while cursor < chars.len() && !chars[cursor].is_whitespace() && chars[cursor] != '/'
                {
                    cursor += 1;
                }
                value_out = chars[start..cursor].iter().collect::<String>();
            }
        }

        out.insert(key, normalize_spaces(&value_out));
    }

    out
}

pub(crate) fn extract_template_titles(content: &str) -> Vec<String> {
    extract_template_invocations(content)
        .into_iter()
        .map(|invocation| invocation.template_title)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn extract_link_titles(content: &str) -> Vec<String> {
    extract_wikilinks(content)
        .into_iter()
        .map(|link| link.target_title)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn flatten_markup_excerpt(value: &str) -> String {
    let mut output = String::new();
    let bytes = value.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if cursor + 1 < bytes.len() && bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
            let start = cursor + 2;
            let mut end = start;
            while end + 1 < bytes.len() {
                if bytes[end] == b']' && bytes[end + 1] == b']' {
                    break;
                }
                end += 1;
            }
            if end + 1 >= bytes.len() {
                break;
            }
            if let Some(display) = display_text_for_wikilink(&value[start..end])
                && !display.is_empty()
            {
                if !output.ends_with(' ') && !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&display);
                output.push(' ');
            }
            cursor = end + 2;
            continue;
        }

        if bytes[cursor] == b'<' {
            let mut end = cursor + 1;
            while end < bytes.len() && bytes[end] != b'>' {
                end += 1;
            }
            cursor = end.saturating_add(1);
            continue;
        }

        if cursor + 1 < bytes.len() && bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            let mut depth = 1usize;
            let mut end = cursor + 2;
            while end + 1 < bytes.len() && depth > 0 {
                if bytes[end] == b'{' && bytes[end + 1] == b'{' {
                    depth += 1;
                    end += 2;
                    continue;
                }
                if bytes[end] == b'}' && bytes[end + 1] == b'}' {
                    depth = depth.saturating_sub(1);
                    end += 2;
                    continue;
                }
                end += 1;
            }
            cursor = end.min(bytes.len());
            continue;
        }

        if bytes[cursor] == b'[' && (cursor + 1 >= bytes.len() || bytes[cursor + 1] != b'[') {
            let mut end = cursor + 1;
            while end < bytes.len() && bytes[end] != b']' {
                end += 1;
            }
            let inner = if end < bytes.len() {
                &value[cursor + 1..end]
            } else {
                &value[cursor + 1..]
            };
            let label = inner
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            if !label.is_empty() {
                if !output.ends_with(' ') && !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&label);
                output.push(' ');
            }
            cursor = end.saturating_add(1);
            continue;
        }

        output.push(bytes[cursor] as char);
        cursor += 1;
    }

    normalize_spaces(&output)
}

pub(crate) fn display_text_for_wikilink(inner: &str) -> Option<String> {
    let segments = split_template_segments(inner);
    let target = segments.first()?.trim();
    if target.is_empty() {
        return None;
    }
    let display = segments.last().map(String::as_str).unwrap_or(target).trim();
    if let Some((_, tail)) = display.rsplit_once(':') {
        return Some(normalize_spaces(&tail.replace('_', " ")));
    }
    Some(normalize_spaces(&display.replace('_', " ")))
}

pub(crate) fn analyze_reference_body(
    reference_body: &str,
    template_titles: &[String],
    link_titles: &[String],
    reference_name: Option<&str>,
    reference_group: Option<&str>,
) -> ReferenceAnalysis {
    let templates = parse_reference_templates(reference_body);
    let primary_template = choose_primary_reference_template(&templates);
    let primary_template_title = primary_template
        .map(|template| template.template_title.clone())
        .or_else(|| template_titles.first().cloned());

    let mut reference_title = first_reference_text_param(
        primary_template,
        &["title", "chapter", "entry", "article", "script-title"],
    );
    if reference_title.is_empty()
        && let Some(template) = primary_template
        && let Some(value) = template.positional_params.first()
    {
        reference_title = flatten_markup_excerpt(value);
    }
    if reference_title.is_empty()
        && let Some(first_link) = link_titles.first()
    {
        reference_title = first_link.clone();
    }

    let mut source_container = first_reference_text_param(
        primary_template,
        &[
            "website",
            "work",
            "journal",
            "newspaper",
            "magazine",
            "periodical",
            "encyclopedia",
            "publisher",
            "publication",
        ],
    );
    let source_author = reference_author_text(primary_template);
    let source_date = first_reference_text_param(
        primary_template,
        &["date", "year", "publication-date", "access-date"],
    );
    let has_quote =
        !first_reference_text_param(primary_template, &["quote", "quotation"]).is_empty();
    let source_urls = collect_reference_source_urls(primary_template, reference_body);
    let canonical_url = source_urls.first().cloned().unwrap_or_default();
    let archive_url = first_reference_raw_param(primary_template, &["archive-url", "archiveurl"]);
    let source_domain = normalize_source_domain(&canonical_url)
        .or_else(|| archive_url.as_deref().and_then(normalize_source_domain))
        .unwrap_or_default();
    let source_type = classify_reference_source_type(
        primary_template,
        &source_domain,
        !source_urls.is_empty(),
        reference_body,
    );
    if source_container.is_empty()
        && !source_domain.is_empty()
        && matches!(
            source_type.as_str(),
            "web" | "news" | "social" | "video" | "wiki"
        )
    {
        source_container = source_domain.clone();
    }
    let source_origin = source_origin_for_reference(&source_domain, &source_type);
    let source_family = classify_reference_source_family(&source_type, &source_origin);
    let (authority_kind, source_authority) = choose_reference_authority(
        &source_domain,
        &source_container,
        &source_author,
        primary_template_title.as_deref(),
        reference_name,
        &source_type,
    );
    let citation_family = citation_family_for_reference(
        primary_template_title.as_deref(),
        &source_type,
        reference_group,
    );
    let identifier_keys = collect_reference_identifier_keys(
        primary_template,
        !source_urls.is_empty(),
        archive_url.is_some(),
    );
    let identifier_entries = collect_reference_identifier_entries(primary_template);
    let signal_inputs = ReferenceSignalInputs {
        primary_template_title: primary_template_title.as_deref(),
        source_type: &source_type,
        source_origin: &source_origin,
        source_family: &source_family,
        authority_kind: &authority_kind,
        reference_title: &reference_title,
        source_container: &source_container,
        source_author: &source_author,
        source_domain: &source_domain,
        source_date: &source_date,
        identifier_keys: &identifier_keys,
        identifier_entries: &identifier_entries,
        has_quote,
        has_links: !link_titles.is_empty(),
        has_archive: archive_url.is_some(),
        reference_name,
        reference_group,
        reference_body,
    };
    let retrieval_signals = collect_reference_signals(signal_inputs);
    let summary_hint = build_reference_summary_hint(
        &reference_title,
        &source_container,
        &source_author,
        &source_domain,
        &source_authority,
        primary_template_title.as_deref(),
        reference_name,
    );
    let citation_profile = build_reference_citation_profile(
        &source_type,
        &source_origin,
        &citation_family,
        &source_domain,
        &authority_kind,
        &source_authority,
    );

    ReferenceAnalysis {
        citation_profile,
        citation_family,
        primary_template_title,
        source_type,
        source_origin,
        source_family,
        authority_kind,
        source_authority,
        reference_title,
        source_container,
        source_author,
        source_domain,
        source_date,
        canonical_url,
        identifier_keys,
        identifier_entries,
        source_urls,
        retrieval_signals,
        summary_hint,
    }
}

pub(crate) fn parse_reference_templates(reference_body: &str) -> Vec<ReferenceTemplateDetails> {
    extract_template_invocations(reference_body)
        .into_iter()
        .filter_map(|invocation| {
            let inner = invocation
                .raw_wikitext
                .strip_prefix("{{")
                .and_then(|value| value.strip_suffix("}}"))?;
            let segments = split_template_segments(inner);
            let mut named_params = BTreeMap::new();
            let mut positional_params = Vec::new();
            for segment in segments.into_iter().skip(1) {
                if let Some((key, value)) = split_once_top_level_equals(&segment) {
                    named_params.insert(
                        normalize_template_parameter_key(&key),
                        value.trim().to_string(),
                    );
                } else {
                    positional_params.push(segment.trim().to_string());
                }
            }
            Some(ReferenceTemplateDetails {
                template_title: invocation.template_title,
                named_params,
                positional_params,
            })
        })
        .collect()
}

pub(crate) fn choose_primary_reference_template(
    templates: &[ReferenceTemplateDetails],
) -> Option<&ReferenceTemplateDetails> {
    templates.iter().min_by(|left, right| {
        reference_template_priority(&left.template_title)
            .cmp(&reference_template_priority(&right.template_title))
            .then_with(|| left.template_title.cmp(&right.template_title))
    })
}

pub(crate) fn reference_template_priority(template_title: &str) -> u8 {
    let lowered = template_title.to_ascii_lowercase();
    if lowered.contains("cite ") || lowered.contains("citation") {
        return 0;
    }
    if lowered.contains("sfn") || lowered.contains("harv") {
        return 1;
    }
    if lowered.contains("ref") || lowered.contains("note") {
        return 2;
    }
    3
}

pub(crate) fn first_reference_text_param(
    template: Option<&ReferenceTemplateDetails>,
    keys: &[&str],
) -> String {
    let Some(template) = template else {
        return String::new();
    };
    for key in keys {
        if let Some(value) = template.named_params.get(*key) {
            let normalized = flatten_markup_excerpt(value);
            if !normalized.is_empty() {
                return normalized;
            }
        }
    }
    String::new()
}

pub(crate) fn first_reference_raw_param(
    template: Option<&ReferenceTemplateDetails>,
    keys: &[&str],
) -> Option<String> {
    let template = template?;
    for key in keys {
        if let Some(value) = template.named_params.get(*key) {
            let normalized = normalize_spaces(value);
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }
    None
}

pub(crate) fn reference_author_text(template: Option<&ReferenceTemplateDetails>) -> String {
    let Some(template) = template else {
        return String::new();
    };
    for key in ["author", "authors", "last", "last1", "editor"] {
        if let Some(value) = template.named_params.get(key) {
            let normalized = flatten_markup_excerpt(value);
            if !normalized.is_empty() {
                if key == "last" || key == "last1" {
                    let first = template
                        .named_params
                        .get("first")
                        .or_else(|| template.named_params.get("first1"))
                        .map(|value| flatten_markup_excerpt(value))
                        .unwrap_or_default();
                    if !first.is_empty() {
                        return format!("{normalized}, {first}");
                    }
                }
                return normalized;
            }
        }
    }
    String::new()
}

pub(crate) fn collect_reference_identifier_keys(
    template: Option<&ReferenceTemplateDetails>,
    has_url: bool,
    has_archive: bool,
) -> Vec<String> {
    let mut out = BTreeSet::new();
    if let Some(template) = template {
        for key in [
            "doi", "isbn", "issn", "oclc", "pmid", "pmcid", "arxiv", "jstor", "id",
        ] {
            if template
                .named_params
                .get(key)
                .is_some_and(|value| !normalize_spaces(value).is_empty())
            {
                out.insert(key.to_string());
            }
        }
    }
    if has_url {
        out.insert("url".to_string());
    }
    if has_archive {
        out.insert("archive-url".to_string());
    }
    out.into_iter().collect()
}

pub(crate) fn collect_reference_identifier_entries(
    template: Option<&ReferenceTemplateDetails>,
) -> Vec<String> {
    let Some(template) = template else {
        return Vec::new();
    };

    let mut out = BTreeSet::new();
    for key in [
        "doi", "isbn", "issn", "oclc", "pmid", "pmcid", "arxiv", "jstor", "id",
    ] {
        let Some(value) = template.named_params.get(key) else {
            continue;
        };
        let normalized_value = normalize_reference_identifier_value(key, value);
        if normalized_value.is_empty() {
            continue;
        }
        out.insert(format!("{key}:{normalized_value}"));
    }
    out.into_iter().collect()
}

pub(crate) fn collect_reference_source_urls(
    template: Option<&ReferenceTemplateDetails>,
    reference_body: &str,
) -> Vec<String> {
    let mut out = BTreeSet::new();
    if let Some(template) = template {
        for key in [
            "url",
            "chapter-url",
            "article-url",
            "archive-url",
            "archiveurl",
        ] {
            if let Some(value) = template.named_params.get(key)
                && let Some(normalized) = normalize_reference_url(value)
            {
                out.insert(normalized);
            }
        }
    }
    if let Some(url) = extract_first_url(reference_body)
        && let Some(normalized) = normalize_reference_url(&url)
    {
        out.insert(normalized);
    }
    out.into_iter().collect()
}

pub(crate) fn normalize_reference_url(value: &str) -> Option<String> {
    let candidate = normalize_spaces(value);
    if candidate.is_empty() {
        return None;
    }
    if candidate.starts_with("//") {
        return Some(format!("https:{candidate}"));
    }
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        return Some(candidate);
    }
    None
}

pub(crate) fn choose_reference_authority(
    source_domain: &str,
    source_container: &str,
    source_author: &str,
    primary_template_title: Option<&str>,
    reference_name: Option<&str>,
    source_type: &str,
) -> (String, String) {
    if !source_domain.is_empty() {
        return ("domain".to_string(), source_domain.to_string());
    }
    if !source_container.is_empty() {
        return ("container".to_string(), source_container.to_string());
    }
    if !source_author.is_empty() {
        return ("author".to_string(), source_author.to_string());
    }
    if let Some(template_title) = primary_template_title {
        return ("template".to_string(), template_title.to_string());
    }
    if let Some(name) = reference_name {
        let normalized = normalize_spaces(name);
        if !normalized.is_empty() {
            return ("named-reference".to_string(), normalized);
        }
    }
    if !source_type.is_empty() {
        return ("source-type".to_string(), source_type.to_string());
    }
    ("unknown".to_string(), String::new())
}

pub(crate) fn classify_reference_source_family(source_type: &str, source_origin: &str) -> String {
    if source_type.is_empty() {
        return "unknown".to_string();
    }
    if source_origin == "first-party" {
        return format!("first-party-{source_type}");
    }
    source_type.to_string()
}

pub(crate) fn normalize_reference_identifier_value(key: &str, value: &str) -> String {
    let flattened = flatten_markup_excerpt(value);
    if flattened.is_empty() {
        return String::new();
    }
    let lowered = flattened.to_ascii_lowercase();
    match key {
        "doi" => {
            let trimmed = lowered
                .trim_start_matches("https://doi.org/")
                .trim_start_matches("http://doi.org/")
                .trim_start_matches("doi:")
                .trim();
            normalize_reference_identifier_token(trimmed, true)
        }
        "isbn" | "issn" | "oclc" | "pmid" | "pmcid" | "jstor" => {
            normalize_reference_identifier_token(&lowered, false)
        }
        "arxiv" => {
            normalize_reference_identifier_token(lowered.trim_start_matches("arxiv:").trim(), true)
        }
        _ => normalize_spaces(&flattened),
    }
}

pub(crate) fn normalize_reference_identifier_token(value: &str, preserve_slash: bool) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            continue;
        }
        if preserve_slash && matches!(ch, '.' | '/' | '_' | '-') {
            out.push(ch);
        }
    }
    out
}

pub(crate) fn parse_identifier_entries(entries: &[String]) -> Vec<ParsedIdentifierEntry> {
    let mut out = Vec::new();
    for entry in entries {
        let Some((key, value)) = entry.split_once(':') else {
            continue;
        };
        let key = normalize_template_parameter_key(key);
        let value = normalize_spaces(value);
        if key.is_empty() || value.is_empty() {
            continue;
        }
        let normalized_value = normalize_reference_identifier_value(&key, &value);
        if normalized_value.is_empty() {
            continue;
        }
        out.push(ParsedIdentifierEntry {
            key,
            value,
            normalized_value,
        });
    }
    out
}

pub(crate) fn build_reference_authority_key(authority_kind: &str, source_authority: &str) -> String {
    let normalized_authority = normalize_spaces(source_authority);
    if normalized_authority.is_empty() {
        return authority_kind.to_string();
    }
    format!(
        "{}:{}",
        authority_kind,
        normalized_authority.to_ascii_lowercase()
    )
}

pub(crate) fn build_reference_authority_retrieval_text(reference: &IndexedReferenceRecord) -> String {
    let mut values = vec![
        reference.source_authority.clone(),
        reference.reference_title.clone(),
        reference.source_container.clone(),
        reference.source_author.clone(),
        reference.source_domain.clone(),
        reference.source_family.clone(),
        reference.source_type.clone(),
        reference.source_origin.clone(),
        reference.summary_text.clone(),
    ];
    values.extend(reference.identifier_entries.iter().cloned());
    values.extend(reference.template_titles.iter().cloned());
    values.extend(reference.link_titles.iter().cloned());
    collect_normalized_string_list(values).join("\n")
}

#[derive(Clone, Copy)]
pub(super) struct ReferenceSignalInputs<'a> {
    primary_template_title: Option<&'a str>,
    source_type: &'a str,
    source_origin: &'a str,
    source_family: &'a str,
    authority_kind: &'a str,
    reference_title: &'a str,
    source_container: &'a str,
    source_author: &'a str,
    source_domain: &'a str,
    source_date: &'a str,
    identifier_keys: &'a [String],
    identifier_entries: &'a [String],
    has_quote: bool,
    has_links: bool,
    has_archive: bool,
    reference_name: Option<&'a str>,
    reference_group: Option<&'a str>,
    reference_body: &'a str,
}

pub(crate) fn collect_reference_signals(input: ReferenceSignalInputs<'_>) -> Vec<String> {
    let mut flags = BTreeSet::new();
    if input.primary_template_title.is_some() {
        flags.insert("citation-template".to_string());
    }
    if !input.reference_title.is_empty() {
        flags.insert("has-title".to_string());
    }
    if !input.source_container.is_empty() {
        flags.insert("has-container".to_string());
    }
    if !input.source_author.is_empty() {
        flags.insert("has-author".to_string());
    }
    if !input.source_domain.is_empty() {
        flags.insert("has-domain".to_string());
    }
    if !input.source_date.is_empty() {
        flags.insert("has-date".to_string());
    }
    if !input.identifier_keys.is_empty() {
        flags.insert("has-identifier".to_string());
    }
    if !input.source_family.is_empty() {
        flags.insert(format!("source-family:{}", input.source_family));
    }
    if !input.authority_kind.is_empty() {
        flags.insert(format!("authority:{}", input.authority_kind));
    }
    if input.has_archive {
        flags.insert("has-archive".to_string());
    }
    if input.has_quote {
        flags.insert("has-quote".to_string());
    }
    if input.has_links {
        flags.insert("has-links".to_string());
    }
    if input.reference_name.is_some() {
        flags.insert("named-reference".to_string());
    }
    if input.reference_group.is_some() {
        flags.insert("grouped-reference".to_string());
    }
    if input.reference_body.trim().is_empty() {
        flags.insert("reused-reference".to_string());
    }
    if input.primary_template_title.is_none() && !input.source_domain.is_empty() {
        flags.insert("bare-url".to_string());
    }
    if input.source_origin == "first-party" {
        flags.insert("first-party".to_string());
    }
    for key in input.identifier_keys {
        flags.insert(format!("identifier:{key}"));
    }
    for entry in input.identifier_entries {
        if let Some((key, _)) = entry.split_once(':') {
            flags.insert(format!("identifier-entry:{key}"));
        }
    }
    if matches!(input.source_type, "social" | "video" | "wiki") {
        flags.insert(format!("source-type:{}", input.source_type));
    }
    flags.into_iter().collect()
}

pub(crate) fn classify_reference_source_type(
    template: Option<&ReferenceTemplateDetails>,
    source_domain: &str,
    has_url: bool,
    reference_body: &str,
) -> String {
    if let Some(template) = template {
        let lowered = template.template_title.to_ascii_lowercase();
        if lowered.contains("cite journal") || lowered.contains("journal") {
            return "journal".to_string();
        }
        if lowered.contains("cite book") || lowered.contains("book") {
            return "book".to_string();
        }
        if lowered.contains("cite news") || lowered.contains("news") {
            return "news".to_string();
        }
        if lowered.contains("cite video") || lowered.contains("video") {
            return "video".to_string();
        }
        if lowered.contains("tweet") || lowered.contains("social") {
            return "social".to_string();
        }
        if lowered.contains("wiki") {
            return "wiki".to_string();
        }
        if lowered.contains("sfn") || lowered.contains("harv") {
            return "short-footnote".to_string();
        }
        if lowered.contains("cite web") || lowered.contains("web") {
            return "web".to_string();
        }
    }
    if is_video_domain(source_domain) {
        return "video".to_string();
    }
    if is_social_domain(source_domain) {
        return "social".to_string();
    }
    if is_wiki_domain(source_domain) {
        return "wiki".to_string();
    }
    if has_url {
        return "web".to_string();
    }
    if reference_body.trim().is_empty() {
        return "note".to_string();
    }
    "other".to_string()
}

pub(crate) fn citation_family_for_reference(
    primary_template_title: Option<&str>,
    source_type: &str,
    reference_group: Option<&str>,
) -> String {
    if let Some(template_title) = primary_template_title {
        return template_title.to_string();
    }
    if reference_group.is_some() || source_type == "note" {
        return "note".to_string();
    }
    if source_type == "web" {
        return "bare-url".to_string();
    }
    "<ref>".to_string()
}

pub(crate) fn source_origin_for_reference(source_domain: &str, source_type: &str) -> String {
    if source_domain.ends_with("remilia.org") {
        return "first-party".to_string();
    }
    if source_type == "wiki" {
        return "wiki".to_string();
    }
    if source_domain.is_empty() {
        return "unknown".to_string();
    }
    "external".to_string()
}

pub(crate) fn build_reference_summary_hint(
    reference_title: &str,
    source_container: &str,
    source_author: &str,
    source_domain: &str,
    source_authority: &str,
    primary_template_title: Option<&str>,
    reference_name: Option<&str>,
) -> String {
    if !reference_title.is_empty() && !source_container.is_empty() {
        return format!("{reference_title} ({source_container})");
    }
    if !reference_title.is_empty() {
        return reference_title.to_string();
    }
    if !source_container.is_empty() && !source_author.is_empty() {
        return format!("{source_container} ({source_author})");
    }
    if !source_container.is_empty() {
        return source_container.to_string();
    }
    if !source_author.is_empty() {
        return source_author.to_string();
    }
    if !source_domain.is_empty() {
        return source_domain.to_string();
    }
    if !source_authority.is_empty() {
        return source_authority.to_string();
    }
    if let Some(template_title) = primary_template_title {
        return template_title.to_string();
    }
    if let Some(name) = reference_name {
        return format!("Named reference {name}");
    }
    String::new()
}

pub(crate) fn build_reference_citation_profile(
    source_type: &str,
    source_origin: &str,
    citation_family: &str,
    source_domain: &str,
    authority_kind: &str,
    source_authority: &str,
) -> String {
    if !source_domain.is_empty()
        && matches!(source_type, "web" | "news" | "social" | "video" | "wiki")
    {
        if source_origin == "first-party" {
            return format!("first-party {source_type} / {source_domain}");
        }
        return format!("{source_type} / {source_domain}");
    }
    if !source_authority.is_empty() && matches!(authority_kind, "container" | "author") {
        if source_origin == "first-party" {
            return format!("first-party {source_type} / {source_authority}");
        }
        return format!("{source_type} / {source_authority}");
    }
    if citation_family != "<ref>" && !citation_family.is_empty() {
        return format!("{source_type} / {citation_family}");
    }
    source_type.to_string()
}

pub(crate) fn extract_first_url(value: &str) -> Option<String> {
    for (start, _) in value.char_indices() {
        let rest = &value[start..];
        let starts_http = rest.starts_with("http://");
        let starts_https = rest.starts_with("https://");
        let starts_protocol_relative = rest.starts_with("//");
        if !(starts_http || starts_https || starts_protocol_relative) {
            continue;
        }

        let mut end = value.len();
        for (offset, ch) in rest.char_indices() {
            if ch.is_whitespace() || matches!(ch, '|' | '}' | ']' | '<' | '"' | '\'') {
                end = start + offset;
                break;
            }
        }
        let candidate = normalize_spaces(&value[start..end]);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn normalize_source_domain(url: &str) -> Option<String> {
    let candidate = if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.to_string()
    };
    let parsed = Url::parse(&candidate).ok()?;
    let host = parsed
        .host_str()?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    if host.is_empty() { None } else { Some(host) }
}

pub(crate) fn is_social_domain(domain: &str) -> bool {
    matches!(
        domain,
        "twitter.com"
            | "x.com"
            | "farcaster.xyz"
            | "instagram.com"
            | "tiktok.com"
            | "mastodon.social"
    )
}

pub(crate) fn is_video_domain(domain: &str) -> bool {
    matches!(
        domain,
        "youtube.com" | "youtu.be" | "vimeo.com" | "twitch.tv"
    )
}

pub(crate) fn is_wiki_domain(domain: &str) -> bool {
    domain.ends_with(".wikipedia.org")
        || domain.ends_with(".wiktionary.org")
        || domain.ends_with(".wikimedia.org")
        || domain.ends_with(".miraheze.org")
        || domain.ends_with(".fandom.com")
        || domain.starts_with("wiki.")
}

pub(crate) fn is_media_option(value: &str) -> bool {
    let normalized = normalize_spaces(value).to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }
    if matches!(
        normalized.as_str(),
        "thumb"
            | "thumbnail"
            | "frame"
            | "framed"
            | "frameless"
            | "border"
            | "right"
            | "left"
            | "center"
            | "none"
            | "baseline"
            | "sub"
            | "super"
            | "top"
            | "text-top"
            | "middle"
            | "bottom"
    ) {
        return true;
    }
    if normalized.ends_with("px")
        || normalized.starts_with("upright")
        || normalized.starts_with("alt=")
        || normalized.starts_with("link=")
        || normalized.starts_with("page=")
        || normalized.starts_with("class=")
        || normalized.starts_with("lang=")
        || normalized.starts_with("start=")
        || normalized.starts_with("end=")
    {
        return true;
    }
    false
}

pub(crate) fn canonical_template_title(raw: &str) -> Option<String> {
    let mut name = normalize_spaces(&raw.replace('_', " "));
    while let Some(stripped) = name.strip_prefix(':') {
        name = stripped.trim_start().to_string();
    }
    if name.is_empty() {
        return None;
    }
    if name.starts_with('#')
        || name.starts_with('!')
        || name.contains('{')
        || name.contains('}')
        || name.contains('[')
        || name.contains(']')
    {
        return None;
    }

    if let Some((prefix, rest)) = name.split_once(':') {
        if !prefix.eq_ignore_ascii_case("Template") {
            return None;
        }
        let body = normalize_spaces(rest);
        if body.is_empty() {
            return None;
        }
        return Some(format!("Template:{body}"));
    }
    Some(format!("Template:{name}"))
}

pub(crate) fn normalize_template_parameter_key(value: &str) -> String {
    normalize_spaces(&value.replace('_', " ")).to_ascii_lowercase()
}

pub(crate) fn extract_wikilinks(content: &str) -> Vec<ParsedLink> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'[' && bytes[cursor + 1] == b'[' {
            let start = cursor + 2;
            let mut end = start;
            while end + 1 < bytes.len() {
                if bytes[end] == b']' && bytes[end + 1] == b']' {
                    break;
                }
                end += 1;
            }
            if end + 1 >= bytes.len() {
                break;
            }

            let inner = &content[start..end];
            if let Some(link) = parse_wikilink(inner) {
                out.push(link);
            }
            cursor = end + 2;
            continue;
        }
        cursor += 1;
    }

    out
}

pub(crate) fn parse_wikilink(inner: &str) -> Option<ParsedLink> {
    let target_part = inner.split('|').next().unwrap_or("").trim();
    if target_part.is_empty() {
        return None;
    }

    let mut target = target_part;
    let mut leading_colon = false;
    while let Some(stripped) = target.strip_prefix(':') {
        leading_colon = true;
        target = stripped.trim_start();
    }
    if target.is_empty() {
        return None;
    }

    if let Some((without_fragment, _)) = target.split_once('#') {
        target = without_fragment.trim_end();
    }
    if target.is_empty() {
        return None;
    }

    if target.starts_with("http://") || target.starts_with("https://") || target.starts_with("//") {
        return None;
    }

    let target = normalize_spaces(&target.replace('_', " "));
    if target.is_empty() {
        return None;
    }

    let (title, namespace) = normalize_title_and_namespace(&target)?;
    let is_category_membership = namespace == Namespace::Category.as_str() && !leading_colon;

    Some(ParsedLink {
        target_title: title,
        target_namespace: namespace.to_string(),
        is_category_membership,
    })
}

pub(crate) fn normalize_title_and_namespace(value: &str) -> Option<(String, &'static str)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((prefix, rest)) = trimmed.split_once(':')
        && let Some(namespace) = canonical_namespace(prefix)
    {
        let body = normalize_spaces(rest);
        if body.is_empty() {
            return None;
        }
        return Some((format!("{namespace}:{body}"), namespace));
    }

    Some((trimmed.to_string(), Namespace::Main.as_str()))
}

pub(crate) fn canonical_namespace(prefix: &str) -> Option<&'static str> {
    let trimmed = prefix.trim();
    if trimmed.eq_ignore_ascii_case("Category") {
        return Some(Namespace::Category.as_str());
    }
    if trimmed.eq_ignore_ascii_case("File") {
        return Some(Namespace::File.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Image") {
        return Some(Namespace::File.as_str());
    }
    if trimmed.eq_ignore_ascii_case("User") {
        return Some(Namespace::User.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Template") {
        return Some(Namespace::Template.as_str());
    }
    if trimmed.eq_ignore_ascii_case("Module") {
        return Some(Namespace::Module.as_str());
    }
    if trimmed.eq_ignore_ascii_case("MediaWiki") {
        return Some(Namespace::MediaWiki.as_str());
    }
    None
}

pub(crate) fn normalize_query_title(title: &str) -> String {
    let normalized = normalize_spaces(&title.replace('_', " "));
    if normalized.is_empty() {
        return normalized;
    }
    match normalize_title_and_namespace(&normalized) {
        Some((value, _)) => value,
        None => String::new(),
    }
}

pub(crate) fn normalize_spaces(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;

    for ch in value.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
                previous_was_space = true;
            }
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }

    output.trim().to_string()
}

pub(crate) fn load_page_record(connection: &Connection, title: &str) -> Result<Option<IndexedPageRecord>> {
    if let Some(record) = load_page_record_exact(connection, title)? {
        return Ok(Some(record));
    }
    let resolved = resolve_alias_title(connection, title, 6)?;
    if resolved.eq_ignore_ascii_case(title) {
        return Ok(None);
    }
    load_page_record_exact(connection, &resolved)
}

pub(crate) fn load_page_record_exact(
    connection: &Connection,
    title: &str,
) -> Result<Option<IndexedPageRecord>> {
    let mut statement = connection
        .prepare(
            "SELECT
                title,
                namespace,
                is_redirect,
                redirect_target,
                relative_path,
                bytes
             FROM indexed_pages
             WHERE lower(title) = lower(?1)
             LIMIT 1",
        )
        .context("failed to prepare page record lookup")?;

    let mut rows = statement
        .query([title])
        .context("failed to run page record lookup")?;
    let row = match rows.next().context("failed to read page record row")? {
        Some(row) => row,
        None => return Ok(None),
    };

    let bytes_i64: i64 = row.get(5).context("failed to decode page bytes")?;
    let bytes = u64::try_from(bytes_i64).context("page bytes are negative")?;
    Ok(Some(IndexedPageRecord {
        title: row.get(0).context("failed to decode page title")?,
        namespace: row.get(1).context("failed to decode page namespace")?,
        is_redirect: row
            .get::<_, i64>(2)
            .context("failed to decode redirect flag")?
            == 1,
        redirect_target: row.get(3).context("failed to decode redirect target")?,
        relative_path: row.get(4).context("failed to decode relative path")?,
        bytes,
    }))
}

pub(crate) fn resolve_alias_title(connection: &Connection, title: &str, max_hops: usize) -> Result<String> {
    let mut current = normalize_query_title(title);
    if current.is_empty() {
        return Ok(current);
    }
    if !table_exists(connection, "indexed_page_aliases")? {
        return Ok(current);
    }
    let mut seen = BTreeSet::new();
    for _ in 0..max_hops.max(1) {
        let normalized = normalize_query_title(&current);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            break;
        }
        let mut statement = connection
            .prepare(
                "SELECT canonical_title
                 FROM indexed_page_aliases
                 WHERE lower(alias_title) = lower(?1)
                 LIMIT 1",
            )
            .context("failed to prepare alias resolution query")?;
        let mut rows = statement
            .query([normalized.as_str()])
            .context("failed to run alias resolution query")?;
        let Some(row) = rows.next().context("failed to read alias resolution row")? else {
            return Ok(normalized);
        };
        let canonical: String = row
            .get(0)
            .context("failed to decode alias canonical title")?;
        if canonical.eq_ignore_ascii_case(&normalized) {
            return Ok(normalized);
        }
        current = canonical;
    }
    Ok(current)
}

pub(crate) fn load_outgoing_link_rows(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Vec<IndexedLinkRow>> {
    let mut statement = connection
        .prepare(
            "SELECT target_title, target_namespace, is_category_membership
             FROM indexed_links
             WHERE source_relative_path = ?1
             ORDER BY target_title ASC",
        )
        .context("failed to prepare outgoing links query")?;
    let rows = statement
        .query_map([source_relative_path], |row| {
            let target_title: String = row.get(0)?;
            let target_namespace: String = row.get(1)?;
            let is_category_membership: i64 = row.get(2)?;
            Ok(IndexedLinkRow {
                target_title,
                target_namespace,
                is_category_membership: is_category_membership == 1,
            })
        })
        .context("failed to run outgoing links query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode outgoing link row")?);
    }
    Ok(out)
}

pub(crate) fn query_backlinks_for_connection(connection: &Connection, title: &str) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT source_title
             FROM indexed_links
             WHERE target_title = ?1
             ORDER BY source_title ASC",
        )
        .context("failed to prepare backlinks query")?;
    let rows = statement
        .query_map([title], |row| row.get::<_, String>(0))
        .context("failed to run backlinks query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode backlinks row")?);
    }
    Ok(out)
}

pub(crate) fn query_orphans_for_connection(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT p.title
             FROM indexed_pages p
             WHERE p.namespace = 'Main'
               AND p.is_redirect = 0
               AND NOT EXISTS (
                   SELECT 1
                   FROM indexed_links l
                   JOIN indexed_pages src ON src.relative_path = l.source_relative_path
                   WHERE l.target_title = p.title
                     AND src.namespace = 'Main'
                     AND src.is_redirect = 0
                     AND src.title <> p.title
               )
             ORDER BY p.title ASC",
        )
        .context("failed to prepare orphan query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .context("failed to run orphan query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode orphan row")?);
    }
    Ok(out)
}

pub(crate) fn query_broken_links_for_connection(connection: &Connection) -> Result<Vec<BrokenLinkIssue>> {
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT l.source_title, l.target_title
             FROM indexed_links l
             LEFT JOIN indexed_pages p ON p.title = l.target_title
             WHERE l.target_namespace = 'Main'
               AND p.title IS NULL
             ORDER BY l.source_title ASC, l.target_title ASC",
        )
        .context("failed to prepare broken-links query")?;
    let rows = statement
        .query_map([], |row| {
            Ok(BrokenLinkIssue {
                source_title: row.get(0)?,
                target_title: row.get(1)?,
            })
        })
        .context("failed to run broken-links query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode broken-link row")?);
    }
    Ok(out)
}

pub(crate) fn query_double_redirects_for_connection(
    connection: &Connection,
) -> Result<Vec<DoubleRedirectIssue>> {
    let mut statement = connection
        .prepare(
            "SELECT
                p.title,
                p.redirect_target,
                p2.redirect_target
             FROM indexed_pages p
             JOIN indexed_pages p2 ON p.redirect_target = p2.title
             WHERE p.is_redirect = 1
               AND p2.is_redirect = 1
             ORDER BY p.title ASC",
        )
        .context("failed to prepare double-redirect query")?;
    let rows = statement
        .query_map([], |row| {
            let first_target: String = row.get(1)?;
            let final_target: String = row.get(2)?;
            Ok(DoubleRedirectIssue {
                title: row.get(0)?,
                first_target,
                final_target,
            })
        })
        .context("failed to run double-redirect query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode double-redirect row")?);
    }
    Ok(out)
}

pub(crate) fn query_uncategorized_pages_for_connection(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT p.title
             FROM indexed_pages p
             WHERE p.namespace = 'Main'
               AND p.is_redirect = 0
               AND NOT EXISTS (
                   SELECT 1
                   FROM indexed_links l
                   WHERE l.source_relative_path = p.relative_path
                     AND l.is_category_membership = 1
               )
             ORDER BY p.title ASC",
        )
        .context("failed to prepare uncategorized query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .context("failed to run uncategorized query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode uncategorized row")?);
    }
    Ok(out)
}

pub(crate) fn count_words(content: &str) -> usize {
    content
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .count()
}

pub(crate) fn make_content_preview(content: &str, max_chars: usize) -> String {
    let normalized = normalize_spaces(content);
    if normalized.len() <= max_chars {
        return normalized;
    }
    let output = normalized.chars().take(max_chars).collect::<String>();
    format!("{output}...")
}

pub(crate) fn summarize_files(files: &[ScannedFile]) -> ScanStats {
    let mut by_namespace = BTreeMap::new();
    let mut content_files = 0usize;
    let mut template_files = 0usize;
    let mut redirects = 0usize;

    for file in files {
        *by_namespace.entry(file.namespace.clone()).or_insert(0) += 1;
        match file.namespace.as_str() {
            value
                if value == Namespace::Template.as_str()
                    || value == Namespace::Module.as_str()
                    || value == Namespace::MediaWiki.as_str() =>
            {
                template_files += 1;
            }
            _ => {
                content_files += 1;
            }
        }
        if file.is_redirect {
            redirects += 1;
        }
    }

    ScanStats {
        total_files: files.len(),
        content_files,
        template_files,
        redirects,
        by_namespace,
    }
}

pub(crate) fn load_scanned_file_content(paths: &ResolvedPaths, file: &ScannedFile) -> Result<String> {
    let absolute = absolute_path_from_relative(paths, &file.relative_path);
    fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))
}

pub(crate) fn absolute_path_from_relative(paths: &ResolvedPaths, relative: &str) -> PathBuf {
    let mut out = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            out.push(segment);
        }
    }
    out
}

pub(crate) fn open_indexed_connection(paths: &ResolvedPaths) -> Result<Option<Connection>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }
    let connection = open_initialized_database_connection(&paths.db_path)?;
    if !has_populated_local_index(&connection)? {
        return Ok(None);
    }
    Ok(Some(connection))
}

pub(crate) fn has_populated_local_index(connection: &Connection) -> Result<bool> {
    if !table_exists(connection, "indexed_pages")? || !table_exists(connection, "indexed_links")? {
        return Ok(false);
    }
    Ok(count_query(connection, "SELECT COUNT(*) FROM indexed_pages")? > 0)
}

pub(crate) fn count_query(connection: &Connection, sql: &str) -> Result<usize> {
    let count: i64 = connection
        .query_row(sql, [], |row| row.get(0))
        .with_context(|| format!("failed query: {sql}"))?;
    usize::try_from(count).context("count does not fit into usize")
}

pub(crate) fn namespace_counts(connection: &Connection) -> Result<BTreeMap<String, usize>> {
    let mut statement = connection
        .prepare(
            "SELECT namespace, COUNT(*) AS count
             FROM indexed_pages
             GROUP BY namespace
             ORDER BY namespace ASC",
        )
        .context("failed to prepare namespace aggregation query")?;

    let rows = statement
        .query_map([], |row| {
            let namespace: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((namespace, count))
        })
        .context("failed to run namespace aggregation query")?;

    let mut out = BTreeMap::new();
    for row in rows {
        let (namespace, count) = row.context("failed to read namespace aggregation row")?;
        let count = usize::try_from(count).context("namespace count does not fit into usize")?;
        out.insert(namespace, count);
    }
    Ok(out)
}

pub(crate) fn fts_table_exists(connection: &Connection, table_name: &str) -> bool {
    table_exists(connection, table_name).unwrap_or(false)
}

pub(crate) fn rebuild_fts_index(connection: &Connection) -> Result<()> {
    if fts_table_exists(connection, "indexed_pages_fts") {
        connection
            .execute_batch("INSERT INTO indexed_pages_fts(indexed_pages_fts) VALUES('rebuild')")
            .context("failed to rebuild indexed_pages_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_chunks_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_chunks_fts(indexed_page_chunks_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_chunks_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_sections_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_sections_fts(indexed_page_sections_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_sections_fts")?;
    }
    if fts_table_exists(connection, "indexed_template_examples_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_template_examples_fts(indexed_template_examples_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_template_examples_fts")?;
    }
    if fts_table_exists(connection, "indexed_module_invocations_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_module_invocations_fts(indexed_module_invocations_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_module_invocations_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_references_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_references_fts(indexed_page_references_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_references_fts")?;
    }
    if fts_table_exists(connection, "indexed_reference_authorities_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_reference_authorities_fts(indexed_reference_authorities_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_reference_authorities_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_media_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_media_fts(indexed_page_media_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_media_fts")?;
    }
    if fts_table_exists(connection, "indexed_page_semantics_fts") {
        connection
            .execute_batch(
                "INSERT INTO indexed_page_semantics_fts(indexed_page_semantics_fts) VALUES('rebuild')",
            )
            .context("failed to rebuild indexed_page_semantics_fts")?;
    }
    Ok(())
}




