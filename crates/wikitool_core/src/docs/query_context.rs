use super::query_common::{SearchContext, fts_position_bonus};
use super::query_search::{search_page_hits, search_symbol_hits};
use super::*;

pub fn build_docs_context(
    paths: &ResolvedPaths,
    query: &str,
    options: &DocsContextOptions,
) -> Result<DocsContextReport> {
    let connection = open_docs_connection(paths)?;
    let limit = options.limit.max(1);
    let token_budget = options.token_budget.max(1);
    let context = SearchContext::new(query, limit.saturating_mul(3))?;
    if context.query_lower.is_empty() {
        return Ok(DocsContextReport {
            query: query.to_string(),
            profile: options.profile.clone(),
            pages: Vec::new(),
            sections: Vec::new(),
            symbols: Vec::new(),
            examples: Vec::new(),
            token_estimate: 0,
        });
    }

    let mut pages = search_page_hits(&connection, &context, options.profile.as_deref(), None)?;
    let mut sections = load_context_sections(&connection, &context, options.profile.as_deref())?;
    let mut symbols = search_symbol_hits(
        &connection,
        &context,
        options.profile.as_deref(),
        None,
        None,
    )?;
    let mut examples = load_context_examples(&connection, &context, options.profile.as_deref())?;

    pages.sort_by_key(|page| std::cmp::Reverse(page.retrieval_weight));
    sections.sort_by_key(|section| std::cmp::Reverse(section.retrieval_weight));
    symbols.sort_by_key(|symbol| std::cmp::Reverse(symbol.retrieval_weight));
    examples.sort_by_key(|example| std::cmp::Reverse(example.retrieval_weight));

    let mut used_tokens = 0usize;
    let mut selected_pages = Vec::new();
    let mut selected_sections = Vec::new();
    let mut selected_symbols = Vec::new();
    let mut selected_examples = Vec::new();

    for symbol in symbols.into_iter().take(limit) {
        let estimated =
            estimate_tokens(&format!("{} {}", symbol.summary_text, symbol.detail_text)).max(1);
        if !selected_symbols.is_empty() && used_tokens + estimated > token_budget {
            continue;
        }
        used_tokens += estimated;
        selected_symbols.push(symbol);
    }
    for section in sections.into_iter().take(limit) {
        if !selected_sections.is_empty() && used_tokens + section.token_estimate > token_budget {
            continue;
        }
        used_tokens += section.token_estimate;
        selected_sections.push(section);
    }
    for example in examples.into_iter().take(limit) {
        if !selected_examples.is_empty() && used_tokens + example.token_estimate > token_budget {
            continue;
        }
        used_tokens += example.token_estimate;
        selected_examples.push(example);
    }
    for page in pages.into_iter().take(limit) {
        let estimated = estimate_tokens(&page.snippet).max(1);
        if !selected_pages.is_empty() && used_tokens + estimated > token_budget {
            continue;
        }
        used_tokens += estimated;
        selected_pages.push(page);
    }

    Ok(DocsContextReport {
        query: normalize_title(query),
        profile: options.profile.clone(),
        pages: selected_pages,
        sections: selected_sections,
        symbols: selected_symbols,
        examples: selected_examples,
        token_estimate: used_tokens,
    })
}

fn load_context_sections(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextSection>> {
    let sections = load_context_sections_fts(connection, context, profile)?;
    if sections.is_empty() {
        return load_context_sections_like(connection, context, profile);
    }
    Ok(sections)
}

fn load_context_sections_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextSection>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, s.page_title, s.section_heading, s.summary_text, s.section_text,
                s.token_estimate, s.semantic_text
         FROM docs_sections_fts
         JOIN docs_sections s ON s.rowid = docs_sections_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND docs_sections_fts MATCH ?2
         ORDER BY bm25(docs_sections_fts, 7.0, 7.0, 2.0, 1.0, 1.0) ASC, s.page_title ASC, s.section_index ASC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(params![profile, match_query, limit_i64], |row| {
        let token_estimate: i64 = row.get(5)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            usize::try_from(token_estimate).unwrap_or(0),
            row.get::<_, String>(6)?,
        ))
    })?;
    for row in rows {
        let (
            corpus_id,
            page_title,
            section_heading,
            summary_text,
            section_text,
            token_estimate,
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
        out.push(DocsContextSection {
            corpus_id,
            page_title,
            section_heading,
            summary_text,
            section_text,
            retrieval_weight: weight,
            token_estimate,
            signals,
        });
    }
    Ok(out)
}

fn load_context_sections_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextSection>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, s.page_title, s.section_heading, s.summary_text, s.section_text,
                s.token_estimate, s.semantic_text
         FROM docs_sections s
         JOIN docs_corpora c ON c.corpus_id = s.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (
                lower(COALESCE(s.section_heading, '')) LIKE ?2
             OR lower(s.summary_text) LIKE ?2
             OR lower(s.section_text) LIKE ?2
             OR lower(s.semantic_text) LIKE ?2
           )
         ORDER BY s.page_title ASC, s.section_index ASC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(params![profile, context.like_pattern, limit_i64], |row| {
        let token_estimate: i64 = row.get(5)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            usize::try_from(token_estimate).unwrap_or(0),
            row.get::<_, String>(6)?,
        ))
    })?;
    for row in rows {
        let (
            corpus_id,
            page_title,
            section_heading,
            summary_text,
            section_text,
            token_estimate,
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
        out.push(DocsContextSection {
            corpus_id,
            page_title,
            section_heading,
            summary_text,
            section_text,
            retrieval_weight: weight,
            token_estimate,
            signals,
        });
    }
    Ok(out)
}

pub(super) fn load_context_examples(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextExample>> {
    let examples = load_context_examples_fts(connection, context, profile)?;
    if examples.is_empty() {
        return load_context_examples_like(connection, context, profile);
    }
    Ok(examples)
}

fn load_context_examples_fts(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextExample>> {
    let Some(match_query) = context.fts_query.as_deref() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                e.page_title, e.example_kind, e.section_heading, e.language_hint,
                e.summary_text, e.example_text, e.token_estimate, e.retrieval_text
         FROM docs_examples_fts
         JOIN docs_examples e ON e.rowid = docs_examples_fts.rowid
         JOIN docs_corpora c ON c.corpus_id = e.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND docs_examples_fts MATCH ?2
         ORDER BY bm25(docs_examples_fts, 5.0, 5.0, 2.0, 4.0, 2.0, 1.0, 1.0) ASC,
                  e.page_title ASC,
                  e.example_index ASC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(params![profile, match_query, limit_i64], |row| {
        let token_estimate: i64 = row.get(9)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            usize::try_from(token_estimate).unwrap_or(0),
            row.get::<_, String>(10)?,
        ))
    })?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            example_kind,
            section_heading,
            language_hint,
            summary_text,
            example_text,
            token_estimate,
            retrieval_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 80usize;
        weight += fts_position_bonus(out.len(), 54);
        signals.push("fts-match".to_string());
        if let Some(heading) = &section_heading
            && heading.to_ascii_lowercase().contains(&context.query_lower)
        {
            weight += 40;
            signals.push("example-heading-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 40;
            signals.push("example-summary-match".to_string());
        }
        if retrieval_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("example-retrieval-match".to_string());
        }
        out.push(DocsContextExample {
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            example_kind,
            section_heading,
            language_hint,
            summary_text,
            example_text,
            retrieval_weight: weight,
            token_estimate,
            signals,
        });
    }
    Ok(out)
}

fn load_context_examples_like(
    connection: &Connection,
    context: &SearchContext,
    profile: Option<&str>,
) -> Result<Vec<DocsContextExample>> {
    let mut out = Vec::new();
    let limit_i64 = i64::try_from(context.limit.saturating_mul(3))?;
    let profile = profile.unwrap_or_default().to_string();
    let mut statement = connection.prepare(
        "SELECT c.corpus_id, c.corpus_kind, c.source_profile,
                e.page_title, e.example_kind, e.section_heading, e.language_hint,
                e.summary_text, e.example_text, e.token_estimate, e.retrieval_text
         FROM docs_examples e
         JOIN docs_corpora c ON c.corpus_id = e.corpus_id
         WHERE (?1 = '' OR lower(c.source_profile) = lower(?1))
           AND (
                lower(COALESCE(e.section_heading, '')) LIKE ?2
             OR lower(e.language_hint) LIKE ?2
             OR lower(e.summary_text) LIKE ?2
             OR lower(e.example_text) LIKE ?2
             OR lower(e.retrieval_text) LIKE ?2
           )
         ORDER BY e.page_title ASC, e.example_index ASC
         LIMIT ?3",
    )?;
    let rows = statement.query_map(params![profile, context.like_pattern, limit_i64], |row| {
        let token_estimate: i64 = row.get(9)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            usize::try_from(token_estimate).unwrap_or(0),
            row.get::<_, String>(10)?,
        ))
    })?;
    for row in rows {
        let (
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            example_kind,
            section_heading,
            language_hint,
            summary_text,
            example_text,
            token_estimate,
            retrieval_text,
        ) = row?;
        let mut signals = Vec::new();
        let mut weight = 80usize;
        if let Some(heading) = &section_heading
            && heading.to_ascii_lowercase().contains(&context.query_lower)
        {
            weight += 40;
            signals.push("example-heading-match".to_string());
        }
        if summary_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 40;
            signals.push("example-summary-match".to_string());
        }
        if retrieval_text
            .to_ascii_lowercase()
            .contains(&context.query_lower)
        {
            weight += 25;
            signals.push("example-retrieval-match".to_string());
        }
        out.push(DocsContextExample {
            corpus_id,
            corpus_kind,
            source_profile,
            page_title,
            example_kind,
            section_heading,
            language_hint,
            summary_text,
            example_text,
            retrieval_weight: weight,
            token_estimate,
            signals,
        });
    }
    Ok(out)
}
