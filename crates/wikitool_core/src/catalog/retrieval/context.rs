use super::*;

pub fn build_local_context(
    paths: &ResolvedPaths,
    title: &str,
) -> Result<Option<LocalContextBundle>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    let normalized = normalize_query_title(title);
    if normalized.is_empty() {
        return Ok(None);
    }
    if is_translation_variant(&normalized) {
        bail!(unsupported_translation_message(&normalized));
    }

    let page = match load_page_record(&connection, &normalized)? {
        Some(page) => page,
        None => return Ok(None),
    };
    let link_rows = load_outgoing_link_rows(&connection, &page.relative_path)?;
    let backlinks = query_backlinks_for_connection(&connection, &page.title)?;
    let mut content = None;

    let section_rows =
        if let Some(rows) = load_section_records_for_bundle(&connection, &page.relative_path)? {
            rows
        } else {
            let loaded = load_page_content(paths, &page.relative_path)?;
            let rows = extract_section_records(&loaded);
            content = Some(loaded);
            rows
        };
    let sections = section_rows
        .iter()
        .filter_map(|section| {
            let heading = section.section_heading.as_ref()?.clone();
            Some(LocalContextHeading {
                level: section.section_level,
                heading,
            })
        })
        .take(AUTHORING_SECTION_LIMIT)
        .collect::<Vec<_>>();
    let section_summaries = section_rows
        .iter()
        .take(AUTHORING_SECTION_LIMIT)
        .map(|section| LocalSectionSummary {
            section_heading: section.section_heading.clone(),
            section_level: section.section_level,
            summary_text: section.summary_text.clone(),
            token_estimate: section.token_estimate,
        })
        .collect::<Vec<_>>();
    let word_count = section_rows
        .iter()
        .map(|section| count_words(&section.section_text))
        .sum::<usize>();
    let prefer_main_namespace_prose = page.namespace == Namespace::Main.as_str();
    let raw_context_chunks =
        match load_context_chunks_for_bundle(&connection, &page.relative_path, content.as_deref())?
        {
            Some(chunks) => chunks,
            None => {
                let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
                fallback_context_chunks_from_content(loaded)
            }
        };
    let context_chunks =
        sanitize_context_chunks_for_display(raw_context_chunks, prefer_main_namespace_prose);
    let content_preview = if let Some(preview) = context_chunks
        .first()
        .map(|chunk| make_content_preview(&chunk.chunk_text, 280))
        .or_else(|| best_context_preview(&section_rows, prefer_main_namespace_prose))
    {
        preview
    } else {
        let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
        let preview = make_content_preview(loaded, 280);
        if prefer_main_namespace_prose {
            sanitize_main_namespace_prose(&preview).unwrap_or(preview)
        } else {
            preview
        }
    };
    let context_tokens_estimate = context_chunks
        .iter()
        .map(|chunk| chunk.token_estimate)
        .sum::<usize>();
    let template_invocations =
        match load_template_invocations_for_bundle(&connection, &page.relative_path)? {
            Some(invocations) => invocations,
            None => {
                let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
                summarize_template_invocations(
                    extract_template_invocations(loaded),
                    TEMPLATE_INVOCATION_LIMIT,
                )
            }
        };
    let references = match load_references_for_bundle(&connection, &page.relative_path)? {
        Some(references) => references,
        None => {
            let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
            extract_reference_records(loaded)
        }
    };
    let media = match load_media_for_bundle(&connection, &page.relative_path)? {
        Some(media) => media,
        None => {
            let loaded = content.get_or_insert(load_page_content(paths, &page.relative_path)?);
            extract_media_records(loaded)
        }
    };

    let mut outgoing_set = BTreeSet::new();
    let mut category_set = BTreeSet::new();
    let mut template_set = BTreeSet::new();
    let mut module_set = BTreeSet::new();
    for link in &link_rows {
        outgoing_set.insert(link.target_title.clone());
        if link.is_category_membership {
            category_set.insert(link.target_title.clone());
        }
        if link.target_namespace == Namespace::Template.as_str() {
            template_set.insert(link.target_title.clone());
        }
        if link.target_namespace == Namespace::Module.as_str() {
            module_set.insert(link.target_title.clone());
        }
    }
    for invocation in &template_invocations {
        template_set.insert(invocation.template_title.clone());
    }

    Ok(Some(LocalContextBundle {
        title: page.title,
        namespace: page.namespace,
        is_redirect: page.is_redirect,
        redirect_target: page.redirect_target,
        relative_path: page.relative_path,
        bytes: page.bytes,
        word_count,
        content_preview,
        sections,
        section_summaries,
        context_chunks,
        context_tokens_estimate,
        outgoing_links: outgoing_set.into_iter().collect(),
        backlinks,
        categories: category_set.into_iter().collect(),
        templates: template_set.into_iter().collect(),
        modules: module_set.into_iter().collect(),
        template_invocations,
        references,
        media,
    }))
}

pub(crate) fn load_context_chunks_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
    content: Option<&str>,
) -> Result<Option<Vec<LocalContextChunk>>> {
    if table_exists(connection, "indexed_page_chunks")? {
        let db_rows = load_indexed_context_chunks_for_connection(
            connection,
            source_relative_path,
            CONTEXT_CHUNK_LIMIT,
            CONTEXT_TOKEN_BUDGET,
        )?;
        if !db_rows.is_empty() {
            return Ok(Some(db_rows));
        }
    }
    Ok(content.map(fallback_context_chunks_from_content))
}

pub(crate) fn load_template_invocations_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<LocalTemplateInvocation>>> {
    if table_exists(connection, "indexed_template_invocations")? {
        let db_rows = load_indexed_template_invocations_for_connection(
            connection,
            source_relative_path,
            TEMPLATE_INVOCATION_LIMIT,
        )?;
        if !db_rows.is_empty() {
            return Ok(Some(db_rows));
        }
    }
    Ok(None)
}

pub(crate) fn load_references_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<LocalReferenceUsage>>> {
    if !table_exists(connection, "indexed_page_references")? {
        return Ok(None);
    }
    let rows = load_indexed_reference_rows_for_connection(
        connection,
        source_relative_path,
        CONTEXT_REFERENCE_LIMIT,
    )?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rows))
    }
}

pub(crate) fn load_media_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<LocalMediaUsage>>> {
    if !table_exists(connection, "indexed_page_media")? {
        return Ok(None);
    }
    let rows = load_indexed_media_rows_for_connection(
        connection,
        source_relative_path,
        CONTEXT_MEDIA_LIMIT,
    )?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rows))
    }
}

pub(crate) fn load_section_records_for_bundle(
    connection: &Connection,
    source_relative_path: &str,
) -> Result<Option<Vec<IndexedSectionRecord>>> {
    if !table_exists(connection, "indexed_page_sections")? {
        return Ok(None);
    }
    let rows = load_indexed_section_rows_for_connection(
        connection,
        source_relative_path,
        AUTHORING_SECTION_LIMIT,
    )?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(rows))
    }
}

pub(crate) fn fallback_context_chunks_from_content(content: &str) -> Vec<LocalContextChunk> {
    let fallback_rows = chunk_article_context(content);
    apply_context_chunk_budget(
        fallback_rows
            .into_iter()
            .map(|row| LocalContextChunk {
                section_heading: row.section_heading,
                token_estimate: row.token_estimate,
                chunk_text: row.chunk_text,
            })
            .collect(),
        CONTEXT_CHUNK_LIMIT,
        CONTEXT_TOKEN_BUDGET,
    )
}

pub(crate) fn load_indexed_section_rows_for_connection(
    connection: &Connection,
    source_relative_path: &str,
    limit: usize,
) -> Result<Vec<IndexedSectionRecord>> {
    let limit_i64 = i64::try_from(limit).context("section limit does not fit into i64")?;
    let mut statement = connection
        .prepare(
            "SELECT section_heading, section_level, summary_text, section_text, token_estimate
             FROM indexed_page_sections
             WHERE source_relative_path = ?1
             ORDER BY section_index ASC
             LIMIT ?2",
        )
        .context("failed to prepare indexed_page_sections query")?;
    let rows = statement
        .query_map(params![source_relative_path, limit_i64], |row| {
            let section_level_i64: i64 = row.get(1)?;
            let token_estimate_i64: i64 = row.get(4)?;
            Ok(IndexedSectionRecord {
                section_heading: row.get(0)?,
                section_level: u8::try_from(section_level_i64).unwrap_or(1),
                summary_text: row.get(2)?,
                section_text: row.get(3)?,
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run indexed_page_sections query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode indexed_page_sections row")?);
    }
    Ok(out)
}

pub(crate) fn load_page_content(
    paths: &ResolvedPaths,
    source_relative_path: &str,
) -> Result<String> {
    let absolute = absolute_path_from_relative(paths, source_relative_path);
    validate_scoped_path(paths, &absolute)?;
    fs::read_to_string(&absolute)
        .with_context(|| format!("failed to read indexed source file {}", absolute.display()))
}
