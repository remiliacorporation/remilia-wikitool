use super::*;
use super::query_common::{SearchContext, SearchScope, fts_position_bonus, make_snippet};
use super::query_context::load_context_examples;

pub fn search_docs(
    paths: &ResolvedPaths,
    query: &str,
    options: &DocsSearchOptions,
) -> Result<Vec<DocsSearchHit>> {
    let connection = open_docs_connection(paths)?;
    let context = SearchContext::new(query, options.limit)?;
    if context.query_lower.is_empty() {
        return Ok(Vec::new());
    }

    let scope = SearchScope::parse(options.tier.as_deref())?;
    let mut hits = Vec::new();
    if scope.include_pages {
        hits.extend(search_page_hits(
            &connection,
            &context,
            options.profile.as_deref(),
            scope.corpus_kind_filter.as_deref(),
        )?);
    }
    if scope.include_sections {
        hits.extend(search_section_hits(
            &connection,
            &context,
            options.profile.as_deref(),
            scope.corpus_kind_filter.as_deref(),
        )?);
    }
    if scope.include_symbols {
        hits.extend(
            search_symbol_hits(
                &connection,
                &context,
                options.profile.as_deref(),
                scope.corpus_kind_filter.as_deref(),
                None,
            )?
            .into_iter()
            .map(symbol_hit_to_search_hit),
        );
    }
    if scope.include_examples {
        hits.extend(search_example_hits(
            &connection,
            &context,
            options.profile.as_deref(),
            scope.corpus_kind_filter.as_deref(),
        )?);
    }

    hits.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.page_title.cmp(&right.page_title))
    });
    hits.truncate(context.limit);
    Ok(hits)
}

fn symbol_hit_to_search_hit(hit: DocsSymbolHit) -> DocsSearchHit {
    let snippet = if hit.detail_text.is_empty() {
        hit.summary_text.clone()
    } else {
        format!("{} {}", hit.summary_text, hit.detail_text)
    };
    DocsSearchHit {
        tier: "symbol".to_string(),
        title: hit.symbol_name.clone(),
        page_title: hit.page_title,
        corpus_id: hit.corpus_id,
        corpus_kind: hit.corpus_kind,
        source_profile: hit.source_profile,
        section_heading: hit.section_heading,
        retrieval_weight: hit.retrieval_weight,
        snippet,
        signals: hit.signals,
    }
}

pub fn lookup_docs_symbols(
    paths: &ResolvedPaths,
    query: &str,
    options: &DocsSymbolLookupOptions,
) -> Result<Vec<DocsSymbolHit>> {
    let connection = open_docs_connection(paths)?;
    let context = SearchContext::new(query, options.limit)?;
    if context.query_lower.is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = search_symbol_hits(
        &connection,
        &context,
        options.profile.as_deref(),
        None,
        options.kind.as_deref(),
    )?;
    hits.sort_by(|left, right| {
        right
            .retrieval_weight
            .cmp(&left.retrieval_weight)
            .then_with(|| left.symbol_name.cmp(&right.symbol_name))
            .then_with(|| left.page_title.cmp(&right.page_title))
    });
    hits.truncate(context.limit);
    Ok(hits)
}

pub(super) fn search_page_hits(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let hits = search_page_hits_fts(connection, context, profile, corpus_kind_filter)?;
    if hits.is_empty() {
        return search_page_hits_like(connection, context, profile, corpus_kind_filter);
    }
    Ok(hits)
}

fn search_page_hits_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(2))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                p.page_title, p.title_aliases, p.summary_text, p.normalized_content, p.semantic_text
         FROM docs_pages_fts
         JOIN docs_pages p ON p.rowid = docs_pages_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = p.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND docs_pages_fts MATCH ?3
         ORDER BY bm25(docs_pages_fts, 8.0, 6.0, 2.0, 1.5, 1.0) ASC, p.page_title ASC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, match_query, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            aliases,
            summary_text,
            content,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 70usize;
        weight += fts_position_bonus(out.len(), 64);
        signals.push("fts-match".to_string());
        if normalize_retrieval_key(&page_title) == context.query_key {
            weight += 120;
            signals.push("exact-page-title".to_string());
        }
        if page_title.to_ascii_lowercase() == context.query_lower {
            weight += 90;
            signals.push("page-title-match".to_string());
        }
        if aliases.to_ascii_lowercase().contains(&context.query_lower) {
            weight += 50;
            signals.push("page-alias-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("page-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("page-semantic-match".to_string());
        }
        let snippet = if summary_text.is_empty() {
            make_snippet(&content, &context.query_lower)
        } else {
            make_snippet(&summary_text, &context.query_lower)
        };
        out.push(DocsSearchHit {
            tier: "page".to_string(),
            title: page_title.clone(),
            page_title,
            corpus_id,
            corpus_kind,
            source_profile,
            section_heading: None,
            retrieval_weight: weight,
            snippet,
            signals,
        });
    }
    Ok(out)
}

fn search_page_hits_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(2))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                p.page_title, p.title_aliases, p.summary_text, p.normalized_content, p.semantic_text
         FROM docs_pages p
         JOIN docs_corpora c ON c.corpus_id = p.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND (
                lower(p.page_title) LIKE ?3
             OR lower(p.title_aliases) LIKE ?3
             OR lower(p.summary_text) LIKE ?3
             OR lower(p.normalized_content) LIKE ?3
             OR lower(p.semantic_text) LIKE ?3
           )
         ORDER BY p.page_title ASC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, context.like_pattern, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            aliases,
            summary_text,
            content,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 70usize;
        if normalize_retrieval_key(&page_title) == context.query_key {
            weight += 120;
            signals.push("exact-page-title".to_string());
        }
        if page_title.to_ascii_lowercase() == context.query_lower {
            weight += 90;
            signals.push("page-title-match".to_string());
        }
        if aliases.to_ascii_lowercase().contains(&context.query_lower) {
            weight += 50;
            signals.push("page-alias-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("page-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("page-semantic-match".to_string());
        }
        let snippet = if summary_text.is_empty() {
            make_snippet(&content, &context.query_lower)
        } else {
            make_snippet(&summary_text, &context.query_lower)
        };
        out.push(DocsSearchHit {
            tier: "page".to_string(),
            title: page_title.clone(),
            page_title,
            corpus_id,
            corpus_kind,
            source_profile,
            section_heading: None,
            retrieval_weight: weight,
            snippet,
            signals,
        });
    }
    Ok(out)
}

fn search_section_hits(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let hits = search_section_hits_fts(connection, context, profile, corpus_kind_filter)?;
    if hits.is_empty() {
        return search_section_hits_like(connection, context, profile, corpus_kind_filter);
    }
    Ok(hits)
}

fn search_section_hits_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(2))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                s.page_title, s.section_heading, s.summary_text, s.section_text, s.semantic_text
         FROM docs_sections_fts
         JOIN docs_sections s ON s.rowid = docs_sections_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND docs_sections_fts MATCH ?3
         ORDER BY bm25(docs_sections_fts, 7.0, 7.0, 2.0, 1.0, 1.0) ASC, s.page_title ASC, s.section_index ASC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, match_query, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            section_heading,
            summary_text,
            section_text,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 90usize;
        weight += fts_position_bonus(out.len(), 58);
        signals.push("fts-match".to_string());
        if let Some(heading) = &section_heading {
            if normalize_retrieval_key(heading) == context.query_key {
                weight += 110;
                signals.push("exact-section-heading".to_string());
            }
            if heading.to_ascii_lowercase().contains(&context.query_lower) {
                weight += 60;
                signals.push("section-heading-match".to_string());
            }
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("section-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 20;
            signals.push("section-semantic-match".to_string());
        }
        out.push(DocsSearchHit {
            tier: "section".to_string(),
            title: section_heading
                .clone()
                .unwrap_or_else(|| page_title.clone()),
            page_title,
            corpus_id,
            corpus_kind,
            source_profile,
            section_heading: section_heading.clone(),
            retrieval_weight: weight,
            snippet: make_snippet(&section_text, &context.query_lower),
            signals,
        });
    }
    Ok(out)
}

fn search_section_hits_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(2))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                s.page_title, s.section_heading, s.summary_text, s.section_text, s.semantic_text
         FROM docs_sections s
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND (
                lower(COALESCE(s.section_heading, '')) LIKE ?3
             OR lower(s.summary_text) LIKE ?3
             OR lower(s.section_text) LIKE ?3
             OR lower(s.semantic_text) LIKE ?3
           )
         ORDER BY s.page_title ASC, s.section_index ASC
         LIMIT ?4",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, context.like_pattern, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            section_heading,
            summary_text,
            section_text,
            semantic_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 90usize;
        if let Some(heading) = &section_heading {
            if normalize_retrieval_key(heading) == context.query_key {
                weight += 110;
                signals.push("exact-section-heading".to_string());
            }
            if heading.to_ascii_lowercase().contains(&context.query_lower) {
                weight += 60;
                signals.push("section-heading-match".to_string());
            }
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 35;
            signals.push("section-summary-match".to_string());
        }
        if semantic_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 20;
            signals.push("section-semantic-match".to_string());
        }
        out.push(DocsSearchHit {
            tier: "section".to_string(),
            title: section_heading
                .clone()
                .unwrap_or_else(|| page_title.clone()),
            page_title,
            corpus_id,
            corpus_kind,
            source_profile,
            section_heading: section_heading.clone(),
            retrieval_weight: weight,
            snippet: make_snippet(&section_text, &context.query_lower),
            signals,
        });
    }
    Ok(out)
}

pub(super) fn search_symbol_hits(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
    symbol_kind_filter: Option<&str>,
) -> Result<Vec<DocsSymbolHit>> {
    let hits = search_symbol_hits_fts(
        connection,
        context,
        profile,
        corpus_kind_filter,
        symbol_kind_filter,
    )?;
    if hits.is_empty() {
        return search_symbol_hits_like(
            connection,
            context,
            profile,
            corpus_kind_filter,
            symbol_kind_filter,
        );
    }
    Ok(hits)
}

fn search_symbol_hits_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
    symbol_kind_filter: Option<&str>,
) -> Result<Vec<DocsSymbolHit>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let symbol_kind = symbol_kind_filter.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                s.page_title, s.symbol_kind, s.symbol_name, s.aliases, s.section_heading,
                s.signature_text, s.summary_text, s.detail_text, s.retrieval_text,
                s.normalized_symbol_key
         FROM docs_symbols_fts
         JOIN docs_symbols s ON s.rowid = docs_symbols_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND (?3 = '' OR lower(s.symbol_kind) = lower(?3))
           AND docs_symbols_fts MATCH ?4
         ORDER BY bm25(docs_symbols_fts, 7.0, 6.0, 3.0, 8.0, 5.0, 2.0, 1.0, 1.0, 1.0) ASC,
                  s.page_title ASC,
                  s.symbol_index ASC
         LIMIT ?5",
    )?;
    let rows = statement.query_map(
        params![profile, corpus_kind, symbol_kind, match_query, limit_i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            symbol_kind,
            symbol_name,
            aliases_blob,
            section_heading,
            signature_text,
            summary_text,
            detail_text,
            retrieval_text,
            normalized_symbol_key,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 120usize;
        weight += fts_position_bonus(out.len(), 72);
        signals.push("fts-match".to_string());
        if normalized_symbol_key == context.query_key {
            weight += 140;
            signals.push("exact-symbol-key".to_string());
        }
        if symbol_name.to_ascii_lowercase() == context.query_lower {
            weight += 100;
            signals.push("symbol-name-match".to_string());
        }
        if aliases_blob
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 65;
            signals.push("symbol-alias-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 40;
            signals.push("symbol-summary-match".to_string());
        }
        if retrieval_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("symbol-retrieval-match".to_string());
        }
        out.push(DocsSymbolHit {
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            symbol_kind,
            symbol_name,
            aliases: deserialize_string_list(&aliases_blob),
            section_heading,
            signature_text,
            summary_text,
            detail_text,
            retrieval_weight: weight,
            signals,
        });
    }
    Ok(out)
}

fn search_symbol_hits_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    corpus_kind_filter: Option<&str>,
    symbol_kind_filter: Option<&str>,
) -> Result<Vec<DocsSymbolHit>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let corpus_kind = normalize_corpus_kind_filter(corpus_kind_filter);
    let symbol_kind = symbol_kind_filter.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                s.page_title, s.symbol_kind, s.symbol_name, s.aliases, s.section_heading,
                s.signature_text, s.summary_text, s.detail_text, s.retrieval_text,
                s.normalized_symbol_key
         FROM docs_symbols s
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (?2 = '' OR lower(c.corpus_kind) = lower(?2))
           AND (?3 = '' OR lower(s.symbol_kind) = lower(?3))
           AND (
                s.normalized_symbol_key = ?4
             OR lower(s.symbol_name) LIKE ?5
             OR lower(s.aliases) LIKE ?5
             OR lower(s.signature_text) LIKE ?5
             OR lower(s.summary_text) LIKE ?5
             OR lower(s.detail_text) LIKE ?5
             OR lower(s.retrieval_text) LIKE ?5
           )
         ORDER BY s.page_title ASC, s.symbol_index ASC
         LIMIT ?6",
    )?;
    let rows = statement.query_map(
        params![
            profile,
            corpus_kind,
            symbol_kind,
            context.query_key,
            context.like_pattern,
            limit_i64
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
            ))
        },
    )?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            symbol_kind,
            symbol_name,
            aliases_blob,
            section_heading,
            signature_text,
            summary_text,
            detail_text,
            retrieval_text,
            normalized_symbol_key,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 120usize;
        if normalized_symbol_key == context.query_key {
            weight += 140;
            signals.push("exact-symbol-key".to_string());
        }
        if symbol_name.to_ascii_lowercase() == context.query_lower {
            weight += 100;
            signals.push("symbol-name-match".to_string());
        }
        if aliases_blob
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 65;
            signals.push("symbol-alias-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 40;
            signals.push("symbol-summary-match".to_string());
        }
        if retrieval_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("symbol-retrieval-match".to_string());
        }
        out.push(DocsSymbolHit {
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            symbol_kind,
            symbol_name,
            aliases: deserialize_string_list(&aliases_blob),
            section_heading,
            signature_text,
            summary_text,
            detail_text,
            retrieval_weight: weight,
            signals,
        });
    }
    Ok(out)
}

fn search_example_hits(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
    _corpus_kind_filter: Option<&str>,
) -> Result<Vec<DocsSearchHit>> {
    let examples = load_context_examples(connection, context, profile)?;
    Ok(examples
        .into_iter()
        .map(|example| DocsSearchHit {
            tier: "example".to_string(),
            title: example.summary_text.clone(),
            page_title: example.page_title,
            corpus_id: example.corpus_id,
            corpus_kind: example.corpus_kind,
            source_profile: example.source_profile,
            section_heading: example.section_heading,
            retrieval_weight: example.retrieval_weight,
            snippet: make_snippet(&example.example_text, &context.query_lower),
            signals: example.signals,
        })
        .take(context.limit.saturating_mul(2))
        .collect())
}
