use super::*;

pub(super) fn persist_docs_corpus(
    paths: &ResolvedPaths,
    descriptor: &CorpusDescriptor,
    pages: &[FetchedDocsPage],
) -> Result<PersistStats> {
    let parsed_pages = pages
        .iter()
        .map(|page| {
            let parsed = parse_docs_page(DocsPageParseInput {
                page_title: page.page_title.clone(),
                local_path: page.local_path.clone(),
                content: page.content.clone(),
                source_revision_id: None,
                source_parent_revision_id: None,
                source_timestamp: None,
            });
            let mut alias_titles = page.alias_titles.clone();
            alias_titles.extend(parsed.alias_titles.clone());
            dedupe_titles_in_order(&mut alias_titles);
            (page, alias_titles, parsed)
        })
        .collect::<Vec<_>>();

    let mut stats = PersistStats::default();
    let mut connection = open_docs_connection(paths)?;
    let transaction = connection
        .transaction()
        .context("failed to start docs corpus transaction")?;

    transaction.execute(
        "DELETE FROM docs_corpora WHERE corpus_id = ?1",
        params![descriptor.corpus_id],
    )?;
    transaction.execute(
        "INSERT INTO docs_corpora (
            corpus_id, corpus_kind, label, source_wiki, source_version, source_profile,
            technical_type, refresh_kind, refresh_spec, pages_count, sections_count,
            symbols_count, examples_count, fetched_at_unix, expires_at_unix
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, 0, 0, ?10, ?11)",
        params![
            descriptor.corpus_id,
            descriptor.corpus_kind,
            descriptor.label,
            descriptor.source_wiki,
            descriptor.source_version,
            descriptor.source_profile,
            descriptor.technical_type,
            descriptor.refresh_kind,
            descriptor.refresh_spec,
            i64::try_from(descriptor.fetched_at_unix)?,
            i64::try_from(descriptor.expires_at_unix)?,
        ],
    )?;

    let mut page_statement = transaction.prepare(
        "INSERT INTO docs_pages (
            corpus_id, page_title, normalized_title_key, page_namespace, doc_type, title_aliases,
            local_path, raw_content, normalized_content, content_hash, summary_text,
            semantic_text, fetched_at_unix, token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    let mut section_statement = transaction.prepare(
        "INSERT INTO docs_sections (
            corpus_id, page_title, section_index, section_level, section_heading, summary_text,
            section_text, semantic_text, token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    let mut symbol_statement = transaction.prepare(
        "INSERT INTO docs_symbols (
            corpus_id, page_title, symbol_index, symbol_kind, symbol_name, normalized_symbol_key,
            aliases, section_heading, signature_text, summary_text, detail_text, retrieval_text,
            token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    let mut example_statement = transaction.prepare(
        "INSERT INTO docs_examples (
            corpus_id, page_title, example_index, example_kind, section_heading, language_hint,
            summary_text, example_text, retrieval_text, token_estimate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    let mut link_statement = transaction.prepare(
        "INSERT INTO docs_links (
            corpus_id, page_title, link_index, target_title, relation_kind, display_text
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    for (page, alias_titles, parsed) in &parsed_pages {
        page_statement.execute(params![
            descriptor.corpus_id,
            page.page_title,
            normalize_retrieval_key(&page.page_title),
            parsed.page_namespace,
            parsed.page_kind,
            serialize_string_list(alias_titles),
            page.local_path,
            page.content,
            parsed.normalized_content,
            compute_hash(&page.content),
            parsed.summary_text,
            parsed.semantic_text,
            i64::try_from(descriptor.fetched_at_unix)?,
            i64::try_from(parsed.token_estimate)?,
        ])?;
        stats.pages += 1;

        insert_sections(
            &mut section_statement,
            &descriptor.corpus_id,
            &page.page_title,
            &parsed.sections,
            &mut stats,
        )?;
        insert_symbols(
            &mut symbol_statement,
            &descriptor.corpus_id,
            &page.page_title,
            &parsed.symbols,
            &mut stats,
        )?;
        insert_examples(
            &mut example_statement,
            &descriptor.corpus_id,
            &page.page_title,
            &parsed.examples,
            &mut stats,
        )?;
        insert_links(
            &mut link_statement,
            &descriptor.corpus_id,
            &page.page_title,
            &parsed.links,
        )?;
    }

    transaction.execute(
        "UPDATE docs_corpora
         SET pages_count = ?2, sections_count = ?3, symbols_count = ?4, examples_count = ?5
         WHERE corpus_id = ?1",
        params![
            descriptor.corpus_id,
            i64::try_from(stats.pages)?,
            i64::try_from(stats.sections)?,
            i64::try_from(stats.symbols)?,
            i64::try_from(stats.examples)?,
        ],
    )?;

    drop(link_statement);
    drop(example_statement);
    drop(symbol_statement);
    drop(section_statement);
    drop(page_statement);
    transaction
        .commit()
        .context("failed to commit docs corpus transaction")?;
    Ok(stats)
}

fn insert_sections(
    statement: &mut rusqlite::Statement<'_>,
    corpus_id: &str,
    page_title: &str,
    sections: &[ParsedDocsSection],
    stats: &mut PersistStats,
) -> Result<()> {
    for section in sections {
        statement.execute(params![
            corpus_id,
            page_title,
            i64::try_from(section.section_index)?,
            i64::from(section.section_level),
            section.section_heading,
            section.summary_text,
            section.section_text,
            section.semantic_text,
            i64::try_from(section.token_estimate)?,
        ])?;
        stats.sections += 1;
    }
    Ok(())
}

fn insert_symbols(
    statement: &mut rusqlite::Statement<'_>,
    corpus_id: &str,
    page_title: &str,
    symbols: &[ParsedDocsSymbol],
    stats: &mut PersistStats,
) -> Result<()> {
    for (index, symbol) in symbols.iter().enumerate() {
        statement.execute(params![
            corpus_id,
            page_title,
            i64::try_from(index)?,
            symbol.symbol_kind,
            symbol.symbol_name,
            symbol.normalized_symbol_key,
            serialize_string_list(&symbol.aliases),
            symbol.section_heading,
            symbol.signature_text,
            symbol.summary_text,
            symbol.detail_text,
            symbol.retrieval_text,
            i64::try_from(symbol.token_estimate)?,
        ])?;
        stats.symbols += 1;
    }
    Ok(())
}

fn insert_examples(
    statement: &mut rusqlite::Statement<'_>,
    corpus_id: &str,
    page_title: &str,
    examples: &[ParsedDocsExample],
    stats: &mut PersistStats,
) -> Result<()> {
    for (index, example) in examples.iter().enumerate() {
        statement.execute(params![
            corpus_id,
            page_title,
            i64::try_from(index)?,
            example.example_kind,
            example.section_heading,
            example.language_hint,
            example.summary_text,
            example.example_text,
            example.retrieval_text,
            i64::try_from(example.token_estimate)?,
        ])?;
        stats.examples += 1;
    }
    Ok(())
}

fn insert_links(
    statement: &mut rusqlite::Statement<'_>,
    corpus_id: &str,
    page_title: &str,
    links: &[ParsedDocsLink],
) -> Result<()> {
    for (index, link) in links.iter().enumerate() {
        statement.execute(params![
            corpus_id,
            page_title,
            i64::try_from(index)?,
            link.target_title,
            link.relation_kind,
            link.display_text,
        ])?;
    }
    Ok(())
}

pub(super) fn accumulate_stats(target: &mut PersistStats, incoming: &PersistStats) {
    target.pages += incoming.pages;
    target.sections += incoming.sections;
    target.symbols += incoming.symbols;
    target.examples += incoming.examples;
}
