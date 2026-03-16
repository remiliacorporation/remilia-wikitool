use super::*;

pub(super) fn import_extension_docs_with_api_internal<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportOptions,
    api: &mut A,
    source_profile: &str,
    source_version: &str,
) -> Result<DocsImportReport> {
    if options.extensions.is_empty() {
        bail!("no extensions specified for docs import");
    }

    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let requested_extensions = options.extensions.len();
    let mut imported_extensions = 0usize;
    let mut stats = PersistStats::default();
    let mut failures = Vec::new();

    for extension in normalize_extensions(&options.extensions) {
        let main_page = format!("Extension:{extension}");
        let mut pages_to_fetch = vec![main_page.clone()];
        if options.include_subpages {
            match api.get_subpages(
                &format!("Extension:{extension}/"),
                DOCS_NAMESPACE_EXTENSION,
                usize::MAX,
            ) {
                Ok(mut subpages) => pages_to_fetch.append(&mut subpages),
                Err(error) => {
                    failures.push(format!("{extension}: failed to list subpages: {error}"));
                    continue;
                }
            }
        }
        filter_translation_titles(&mut pages_to_fetch);
        dedupe_titles_in_order(&mut pages_to_fetch);

        let mut fetched_pages = Vec::new();
        let mut page_failed = false;
        for title in pages_to_fetch {
            if is_translation_variant(&title) {
                continue;
            }
            match api.get_page(&title) {
                Ok(Some(page)) => {
                    if is_translation_variant(&page.title) {
                        continue;
                    }
                    let mut alias_titles = Vec::new();
                    if !page.requested_title.eq_ignore_ascii_case(&page.title) {
                        alias_titles.push(page.requested_title);
                    }
                    fetched_pages.push(FetchedDocsPage {
                        page_title: page.title.clone(),
                        alias_titles,
                        local_path: extension_local_path(&extension, &page.title),
                        content: page.content,
                    });
                }
                Ok(None) => {
                    failures.push(format!("{extension}: page missing during refresh: {title}"));
                    page_failed = true;
                    break;
                }
                Err(error) => {
                    failures.push(format!("{extension}: failed to fetch {title}: {error}"));
                    page_failed = true;
                    break;
                }
            }
        }
        if page_failed || fetched_pages.is_empty() {
            continue;
        }

        let descriptor = CorpusDescriptor {
            corpus_id: extension_corpus_id(&extension, source_profile),
            corpus_kind: "extension".to_string(),
            label: format!("Extension:{extension}"),
            source_wiki: "mediawiki.org".to_string(),
            source_version: source_version.to_string(),
            source_profile: source_profile.to_string(),
            technical_type: String::new(),
            refresh_kind: "extension".to_string(),
            refresh_spec: serde_json::to_string(&ExtensionRefreshSpec {
                extension_name: extension.clone(),
                include_subpages: options.include_subpages,
                source_profile: source_profile.to_string(),
                source_version: source_version.to_string(),
            })?,
            fetched_at_unix: now_unix,
            expires_at_unix,
        };
        let persisted = persist_docs_corpus(paths, &descriptor, &fetched_pages)?;
        imported_extensions += 1;
        accumulate_stats(&mut stats, &persisted);
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsImportReport {
        requested_extensions,
        imported_extensions,
        imported_pages: stats.pages,
        imported_sections: stats.sections,
        imported_symbols: stats.symbols,
        imported_examples: stats.examples,
        failures,
        request_count: api.request_count(),
    })
}

pub(super) fn import_technical_docs_with_api_internal<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportTechnicalOptions,
    api: &mut A,
    source_profile: &str,
    source_version: &str,
) -> Result<DocsImportTechnicalReport> {
    if options.tasks.is_empty() {
        bail!("no technical docs tasks specified");
    }

    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let mut imported_corpora = 0usize;
    let mut imported_by_type = BTreeMap::new();
    let mut stats = PersistStats::default();
    let mut failures = Vec::new();

    for task in &options.tasks {
        let mut task_clone = task.clone();
        match collect_pages_for_technical_task(api, &mut task_clone, options.limit.max(1)) {
            Ok(fetched_pages) => {
                if fetched_pages.is_empty() {
                    failures.push(format!(
                        "{}: no pages fetched for task",
                        task.doc_type.as_str()
                    ));
                    continue;
                }
                let descriptor = CorpusDescriptor {
                    corpus_id: technical_corpus_id(
                        task.doc_type,
                        task.page_title.as_deref(),
                        source_profile,
                    ),
                    corpus_kind: "technical".to_string(),
                    label: task
                        .page_title
                        .clone()
                        .unwrap_or_else(|| task.doc_type.main_page().to_string()),
                    source_wiki: "mediawiki.org".to_string(),
                    source_version: source_version.to_string(),
                    source_profile: source_profile.to_string(),
                    technical_type: task.doc_type.as_str().to_string(),
                    refresh_kind: "technical".to_string(),
                    refresh_spec: serde_json::to_string(&TechnicalRefreshSpec {
                        doc_type: task.doc_type.as_str().to_string(),
                        page_title: task.page_title.clone(),
                        include_subpages: task.include_subpages,
                        limit: options.limit.max(1),
                        source_profile: source_profile.to_string(),
                        source_version: source_version.to_string(),
                    })?,
                    fetched_at_unix: now_unix,
                    expires_at_unix,
                };
                let persisted = persist_docs_corpus(paths, &descriptor, &fetched_pages)?;
                imported_corpora += 1;
                *imported_by_type
                    .entry(task.doc_type.as_str().to_string())
                    .or_insert(0) += persisted.pages;
                accumulate_stats(&mut stats, &persisted);
            }
            Err(error) => failures.push(format!("{}: {error}", task.doc_type.as_str())),
        }
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsImportTechnicalReport {
        requested_tasks: options.tasks.len(),
        imported_corpora,
        imported_pages: stats.pages,
        imported_sections: stats.sections,
        imported_symbols: stats.symbols,
        imported_examples: stats.examples,
        imported_by_type,
        failures,
        request_count: api.request_count(),
    })
}

pub(super) fn collect_pages_for_technical_task<A: DocsApi>(
    api: &mut A,
    task: &mut TechnicalImportTask,
    limit: usize,
) -> Result<Vec<FetchedDocsPage>> {
    let mut pages_to_fetch = Vec::new();
    if let Some(page_title) = task.page_title.as_deref() {
        let normalized = normalize_title(page_title);
        if !normalized.is_empty() {
            pages_to_fetch.push(normalized.clone());
            if task.include_subpages {
                let prefix = if normalized.ends_with('/') {
                    normalized.clone()
                } else {
                    format!("{normalized}/")
                };
                let mut subpages = api.get_subpages(
                    &prefix,
                    infer_doc_type_from_title(&normalized).namespace(),
                    limit.max(1),
                )?;
                pages_to_fetch.append(&mut subpages);
            }
        }
    } else {
        pages_to_fetch.push(task.doc_type.main_page().to_string());
        if task.include_subpages {
            let mut subpages = api.get_subpages(
                task.doc_type.subpage_prefix(),
                task.doc_type.namespace(),
                limit.max(1),
            )?;
            pages_to_fetch.append(&mut subpages);
        }
    }
    filter_translation_titles(&mut pages_to_fetch);
    dedupe_titles_in_order(&mut pages_to_fetch);

    let mut fetched_pages = Vec::new();
    for title in pages_to_fetch {
        if is_translation_variant(&title) {
            continue;
        }
        match api.get_page(&title)? {
            Some(page) => {
                if is_translation_variant(&page.title) {
                    continue;
                }
                let mut alias_titles = Vec::new();
                if !page.requested_title.eq_ignore_ascii_case(&page.title) {
                    alias_titles.push(page.requested_title);
                }
                fetched_pages.push(FetchedDocsPage {
                    page_title: page.title.clone(),
                    alias_titles,
                    local_path: technical_local_path(
                        infer_doc_type_from_title(&page.title),
                        &page.title,
                    ),
                    content: page.content,
                });
            }
            None => bail!("page missing during refresh: {title}"),
        }
    }
    Ok(fetched_pages)
}
