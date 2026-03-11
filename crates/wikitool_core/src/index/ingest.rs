use super::*;
use super::{model::*, parsing::*};

pub fn rebuild_index(paths: &ResolvedPaths, options: &ScanOptions) -> Result<RebuildReport> {
    let files = scan_files(paths, options)?;
    let scan = summarize_files(&files);
    let mut connection = open_initialized_database_connection(&paths.db_path)?;
    let indexed_at_unix = unix_timestamp()?;

    let transaction = connection
        .transaction()
        .context("failed to start index rebuild transaction")?;
    transaction
        .execute("DELETE FROM indexed_pages", [])
        .context("failed to clear indexed_pages table")?;

    let mut page_statement = transaction
        .prepare(
            "INSERT INTO indexed_pages (
                relative_path,
                title,
                namespace,
                is_redirect,
                redirect_target,
                content_hash,
                bytes,
                indexed_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .context("failed to prepare indexed_pages insert")?;

    let mut link_statement = transaction
        .prepare(
            "INSERT OR IGNORE INTO indexed_links (
                source_relative_path,
                source_title,
                target_title,
                target_namespace,
                is_category_membership
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .context("failed to prepare indexed_links insert")?;

    let mut chunk_statement = transaction
        .prepare(
            "INSERT INTO indexed_page_chunks (
                source_relative_path,
                chunk_index,
                source_title,
                source_namespace,
                section_heading,
                chunk_text,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .context("failed to prepare indexed_page_chunks insert")?;

    let mut template_invocation_statement = transaction
        .prepare(
            "INSERT OR IGNORE INTO indexed_template_invocations (
                source_relative_path,
                source_title,
                template_title,
                parameter_keys
            ) VALUES (?1, ?2, ?3, ?4)",
        )
        .context("failed to prepare indexed_template_invocations insert")?;

    let mut alias_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_page_aliases (
                alias_title,
                canonical_title,
                canonical_namespace,
                source_relative_path
            ) VALUES (?1, ?2, ?3, ?4)",
        )
        .context("failed to prepare indexed_page_aliases insert")?;

    let mut section_statement = transaction
        .prepare(
            "INSERT INTO indexed_page_sections (
                source_relative_path,
                section_index,
                source_title,
                source_namespace,
                section_heading,
                section_level,
                summary_text,
                section_text,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .context("failed to prepare indexed_page_sections insert")?;

    let mut template_example_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_template_examples (
                template_title,
                source_relative_path,
                source_title,
                invocation_index,
                example_wikitext,
                parameter_keys,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .context("failed to prepare indexed_template_examples insert")?;

    let mut module_invocation_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_module_invocations (
                source_relative_path,
                invocation_index,
                source_title,
                source_namespace,
                module_title,
                function_name,
                parameter_keys,
                invocation_wikitext,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .context("failed to prepare indexed_module_invocations insert")?;

    let mut reference_statement = transaction
        .prepare(
            "INSERT INTO indexed_page_references (
                source_relative_path,
                reference_index,
                source_title,
                source_namespace,
                section_heading,
                reference_name,
                reference_group,
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
                summary_text,
                reference_wikitext,
                template_titles,
                link_titles,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)",
        )
        .context("failed to prepare indexed_page_references insert")?;

    let mut reference_authority_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_reference_authorities (
                source_relative_path,
                reference_index,
                source_title,
                source_namespace,
                section_heading,
                citation_profile,
                citation_family,
                source_type,
                source_origin,
                source_family,
                authority_kind,
                authority_key,
                authority_label,
                primary_template_title,
                source_domain,
                source_container,
                source_author,
                identifier_keys,
                summary_text,
                retrieval_text
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        )
        .context("failed to prepare indexed_reference_authorities insert")?;

    let mut reference_identifier_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_reference_identifiers (
                source_relative_path,
                reference_index,
                source_title,
                source_namespace,
                section_heading,
                citation_profile,
                citation_family,
                source_type,
                source_origin,
                source_family,
                authority_key,
                authority_label,
                identifier_key,
                identifier_value,
                normalized_value,
                summary_text
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )
        .context("failed to prepare indexed_reference_identifiers insert")?;

    let mut media_statement = transaction
        .prepare(
            "INSERT INTO indexed_page_media (
                source_relative_path,
                media_index,
                source_title,
                source_namespace,
                section_heading,
                file_title,
                media_kind,
                caption_text,
                options_text,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .context("failed to prepare indexed_page_media insert")?;

    let mut semantic_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_page_semantics (
                source_relative_path,
                source_title,
                source_namespace,
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
                semantic_text,
                token_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        )
        .context("failed to prepare indexed_page_semantics insert")?;

    let mut template_implementation_statement = transaction
        .prepare(
            "INSERT OR REPLACE INTO indexed_template_implementation_pages (
                template_title,
                implementation_page_title,
                implementation_namespace,
                source_relative_path,
                role
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .context("failed to prepare indexed_template_implementation_pages insert")?;

    let mut inserted_rows = 0usize;
    let mut inserted_links = 0usize;
    let mut template_implementation_seeds = BTreeMap::<String, TemplateImplementationSeed>::new();
    for file in &files {
        page_statement
            .execute(params![
                file.relative_path,
                file.title,
                file.namespace,
                if file.is_redirect { 1i64 } else { 0i64 },
                file.redirect_target,
                file.content_hash,
                i64::try_from(file.bytes).context("bytes value does not fit into i64")?,
                i64::try_from(indexed_at_unix).context("timestamp does not fit into i64")?,
            ])
            .with_context(|| format!("failed to insert {}", file.relative_path))?;
        inserted_rows += 1;

        let content = load_scanned_file_content(paths, file)?;
        let links = extract_wikilinks(&content);
        for link in &links {
            let affected = link_statement
                .execute(params![
                    file.relative_path,
                    file.title,
                    link.target_title,
                    link.target_namespace,
                    if link.is_category_membership {
                        1i64
                    } else {
                        0i64
                    }
                ])
                .with_context(|| format!("failed to insert links for {}", file.relative_path))?;
            inserted_links += affected;
        }

        if file.is_redirect
            && let Some(target) = file.redirect_target.as_deref()
            && let Some((canonical_title, canonical_namespace)) =
                normalize_title_and_namespace(target)
        {
            alias_statement
                .execute(params![
                    file.title,
                    canonical_title,
                    canonical_namespace,
                    file.relative_path,
                ])
                .with_context(|| format!("failed to insert alias for {}", file.relative_path))?;
        }

        let artifacts = extract_page_artifacts(&content);
        maybe_record_template_implementation_seed(
            &mut template_implementation_seeds,
            file,
            &artifacts,
        );
        let semantic_profile = build_page_semantic_profile(file, &links, &artifacts);
        for (section_index, section) in artifacts.section_records.iter().enumerate() {
            section_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(section_index).context("section index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    section.section_heading.as_deref(),
                    i64::from(section.section_level),
                    section.summary_text,
                    section.section_text,
                    i64::try_from(section.token_estimate)
                        .context("section token estimate does not fit into i64")?,
                ])
                .with_context(|| format!("failed to insert sections for {}", file.relative_path))?;
        }

        for (chunk_index, chunk) in artifacts.context_chunks.iter().enumerate() {
            chunk_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(chunk_index).context("chunk index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    chunk.section_heading.as_deref(),
                    chunk.chunk_text.as_str(),
                    i64::try_from(chunk.token_estimate)
                        .context("chunk token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!("failed to insert context chunks for {}", file.relative_path)
                })?;
        }

        for (reference_index, reference) in artifacts.references.iter().enumerate() {
            reference_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(reference_index)
                        .context("reference index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    reference.section_heading.as_deref(),
                    reference.reference_name.as_deref(),
                    reference.reference_group.as_deref(),
                    reference.citation_profile.as_str(),
                    reference.citation_family.as_str(),
                    reference
                        .primary_template_title
                        .as_deref()
                        .unwrap_or_default(),
                    reference.source_type.as_str(),
                    reference.source_origin.as_str(),
                    reference.source_family.as_str(),
                    reference.authority_kind.as_str(),
                    reference.source_authority.as_str(),
                    reference.reference_title.as_str(),
                    reference.source_container.as_str(),
                    reference.source_author.as_str(),
                    reference.source_domain.as_str(),
                    reference.source_date.as_str(),
                    reference.canonical_url.as_str(),
                    serialize_string_list(&reference.identifier_keys),
                    serialize_string_list(&reference.identifier_entries),
                    serialize_string_list(&reference.source_urls),
                    serialize_string_list(&reference.retrieval_signals),
                    reference.summary_text.as_str(),
                    reference.reference_wikitext.as_str(),
                    serialize_string_list(&reference.template_titles),
                    serialize_string_list(&reference.link_titles),
                    i64::try_from(reference.token_estimate)
                        .context("reference token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!("failed to insert reference rows for {}", file.relative_path)
                })?;

            let authority_key = build_reference_authority_key(
                &reference.authority_kind,
                &reference.source_authority,
            );
            let authority_label = normalize_spaces(&reference.source_authority);
            let authority_identifier_keys = serialize_string_list(&reference.identifier_keys);
            let authority_retrieval_text = build_reference_authority_retrieval_text(reference);
            reference_authority_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(reference_index)
                        .context("reference index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    reference.section_heading.as_deref(),
                    reference.citation_profile.as_str(),
                    reference.citation_family.as_str(),
                    reference.source_type.as_str(),
                    reference.source_origin.as_str(),
                    reference.source_family.as_str(),
                    reference.authority_kind.as_str(),
                    authority_key.as_str(),
                    authority_label.as_str(),
                    reference
                        .primary_template_title
                        .as_deref()
                        .unwrap_or_default(),
                    reference.source_domain.as_str(),
                    reference.source_container.as_str(),
                    reference.source_author.as_str(),
                    authority_identifier_keys.as_str(),
                    reference.summary_text.as_str(),
                    authority_retrieval_text.as_str(),
                ])
                .with_context(|| {
                    format!(
                        "failed to insert reference authority row for {}",
                        file.relative_path
                    )
                })?;

            for entry in parse_identifier_entries(&reference.identifier_entries) {
                reference_identifier_statement
                    .execute(params![
                        file.relative_path,
                        i64::try_from(reference_index)
                            .context("reference index does not fit into i64")?,
                        file.title,
                        file.namespace,
                        reference.section_heading.as_deref(),
                        reference.citation_profile.as_str(),
                        reference.citation_family.as_str(),
                        reference.source_type.as_str(),
                        reference.source_origin.as_str(),
                        reference.source_family.as_str(),
                        authority_key.as_str(),
                        authority_label.as_str(),
                        entry.key.as_str(),
                        entry.value.as_str(),
                        entry.normalized_value.as_str(),
                        reference.summary_text.as_str(),
                    ])
                    .with_context(|| {
                        format!(
                            "failed to insert reference identifier row for {}",
                            file.relative_path
                        )
                    })?;
            }
        }

        for (media_index, media) in artifacts.media.iter().enumerate() {
            media_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(media_index).context("media index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    media.section_heading.as_deref(),
                    media.file_title.as_str(),
                    media.media_kind.as_str(),
                    media.caption_text.as_str(),
                    serialize_string_list(&media.options),
                    i64::try_from(media.token_estimate)
                        .context("media token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!("failed to insert media rows for {}", file.relative_path)
                })?;
        }

        semantic_statement
            .execute(params![
                file.relative_path,
                semantic_profile.source_title.as_str(),
                semantic_profile.source_namespace.as_str(),
                semantic_profile.summary_text.as_str(),
                serialize_string_list(&semantic_profile.section_headings),
                serialize_string_list(&semantic_profile.category_titles),
                serialize_string_list(&semantic_profile.template_titles),
                serialize_string_list(&semantic_profile.template_parameter_keys),
                serialize_string_list(&semantic_profile.link_titles),
                serialize_string_list(&semantic_profile.reference_titles),
                serialize_string_list(&semantic_profile.reference_containers),
                serialize_string_list(&semantic_profile.reference_domains),
                serialize_string_list(&semantic_profile.reference_source_families),
                serialize_string_list(&semantic_profile.reference_authorities),
                serialize_string_list(&semantic_profile.reference_identifiers),
                serialize_string_list(&semantic_profile.media_titles),
                serialize_string_list(&semantic_profile.media_captions),
                serialize_string_list(&semantic_profile.template_implementation_titles),
                semantic_profile.semantic_text.as_str(),
                i64::try_from(semantic_profile.token_estimate)
                    .context("semantic profile token estimate does not fit into i64")?,
            ])
            .with_context(|| {
                format!(
                    "failed to insert semantic profile for {}",
                    file.relative_path
                )
            })?;

        let mut seen_signatures = BTreeSet::new();
        for (invocation_index, invocation) in artifacts.template_invocations.into_iter().enumerate()
        {
            let parameter_keys = canonical_parameter_key_list(&invocation.parameter_keys);
            let signature = format!("{}|{}", invocation.template_title, parameter_keys);
            if !seen_signatures.insert(signature) {
                template_example_statement
                    .execute(params![
                        invocation.template_title,
                        file.relative_path,
                        file.title,
                        i64::try_from(invocation_index)
                            .context("invocation index does not fit into i64")?,
                        invocation.raw_wikitext,
                        parameter_keys,
                        i64::try_from(invocation.token_estimate)
                            .context("invocation token estimate does not fit into i64")?,
                    ])
                    .with_context(|| {
                        format!(
                            "failed to insert template example for {}",
                            file.relative_path
                        )
                    })?;
                continue;
            }
            template_invocation_statement
                .execute(params![
                    file.relative_path,
                    file.title,
                    invocation.template_title,
                    parameter_keys,
                ])
                .with_context(|| {
                    format!(
                        "failed to insert template invocations for {}",
                        file.relative_path
                    )
                })?;
            template_example_statement
                .execute(params![
                    invocation.template_title,
                    file.relative_path,
                    file.title,
                    i64::try_from(invocation_index)
                        .context("invocation index does not fit into i64")?,
                    invocation.raw_wikitext,
                    parameter_keys,
                    i64::try_from(invocation.token_estimate)
                        .context("invocation token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!(
                        "failed to insert template example for {}",
                        file.relative_path
                    )
                })?;
        }
        for (invocation_index, invocation) in artifacts.module_invocations.into_iter().enumerate() {
            module_invocation_statement
                .execute(params![
                    file.relative_path,
                    i64::try_from(invocation_index)
                        .context("module invocation index does not fit into i64")?,
                    file.title,
                    file.namespace,
                    invocation.module_title,
                    invocation.function_name,
                    canonical_parameter_key_list(&invocation.parameter_keys),
                    invocation.raw_wikitext,
                    i64::try_from(invocation.token_estimate)
                        .context("module invocation token estimate does not fit into i64")?,
                ])
                .with_context(|| {
                    format!(
                        "failed to insert module invocations for {}",
                        file.relative_path
                    )
                })?;
        }
    }
    persist_template_implementation_pages(
        &mut template_implementation_statement,
        &files,
        &template_implementation_seeds,
    )?;
    drop(template_implementation_statement);
    drop(media_statement);
    drop(semantic_statement);
    drop(reference_identifier_statement);
    drop(reference_authority_statement);
    drop(reference_statement);
    drop(module_invocation_statement);
    drop(template_example_statement);
    drop(section_statement);
    drop(alias_statement);
    drop(template_invocation_statement);
    drop(chunk_statement);
    drop(link_statement);
    drop(page_statement);

    transaction
        .commit()
        .context("failed to commit index rebuild transaction")?;

    // Rebuild FTS5 index if the virtual table exists from schema bootstrap.
    rebuild_fts_index(&connection)?;
    record_content_index_artifact(
        &connection,
        inserted_rows,
        &json!({
            "inserted_rows": inserted_rows,
            "inserted_links": inserted_links,
            "scan_total_files": scan.total_files,
            "scan_content_files": scan.content_files,
            "scan_template_files": scan.template_files,
            "scan_redirects": scan.redirects,
            "namespaces": scan.by_namespace.clone(),
        })
        .to_string(),
    )?;

    Ok(RebuildReport {
        db_path: normalize_path(&paths.db_path),
        inserted_rows,
        inserted_links,
        scan,
    })
}

pub fn load_stored_index_stats(paths: &ResolvedPaths) -> Result<Option<StoredIndexStats>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }

    let connection = open_initialized_database_connection(&paths.db_path)?;
    if !has_populated_local_index(&connection)? {
        return Ok(None);
    }

    let indexed_rows = count_query(&connection, "SELECT COUNT(*) FROM indexed_pages")
        .context("failed to count indexed rows")?;
    let redirects = count_query(
        &connection,
        "SELECT COUNT(*) FROM indexed_pages WHERE is_redirect = 1",
    )
    .context("failed to count redirects")?;
    let by_namespace = namespace_counts(&connection)?;

    Ok(Some(StoredIndexStats {
        indexed_rows,
        redirects,
        by_namespace,
    }))
}

