use super::prelude::*;

pub use super::model::{
    LocalMediaUsage, LocalReferenceUsage, MediaUsageExample, MediaUsageSummary,
    ReferenceUsageExample, ReferenceUsageSummary,
};

#[derive(Default)]
struct ReferenceUsageAccumulator {
    usage_count: usize,
    source_pages: BTreeSet<String>,
    template_counts: BTreeMap<String, usize>,
    link_counts: BTreeMap<String, usize>,
    domain_counts: BTreeMap<String, usize>,
    authority_counts: BTreeMap<String, usize>,
    identifier_counts: BTreeMap<String, usize>,
    identifier_entry_counts: BTreeMap<String, usize>,
    retrieval_signal_counts: BTreeMap<String, usize>,
    citation_family: String,
    source_type: String,
    source_origin: String,
    source_family: String,
    examples: Vec<ReferenceUsageExample>,
}

#[derive(Debug, Clone)]
struct IndexedReferenceUsageRow {
    source_title: String,
    source_relative_path: String,
    section_heading: Option<String>,
    reference_name: Option<String>,
    reference_group: Option<String>,
    citation_profile: String,
    citation_family: String,
    primary_template_title: Option<String>,
    source_type: String,
    source_origin: String,
    source_family: String,
    authority_kind: String,
    source_authority: String,
    reference_title: String,
    source_container: String,
    source_author: String,
    source_domain: String,
    source_date: String,
    canonical_url: String,
    identifier_keys: Vec<String>,
    identifier_entries: Vec<String>,
    source_urls: Vec<String>,
    retrieval_signals: Vec<String>,
    summary_text: String,
    reference_wikitext: String,
    template_titles: Vec<String>,
    link_titles: Vec<String>,
    token_estimate: usize,
}

pub(crate) fn summarize_reference_usage_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<ReferenceUsageSummary>> {
    if limit == 0
        || source_titles.is_empty()
        || !table_exists(connection, "indexed_page_references")?
    {
        return Ok(Vec::new());
    }

    let rows = load_reference_rows_for_sources(connection, source_titles)?;
    let mut grouped = BTreeMap::<String, ReferenceUsageAccumulator>::new();
    for row in rows {
        let example = ReferenceUsageExample {
            source_title: row.source_title.clone(),
            source_relative_path: row.source_relative_path.clone(),
            section_heading: row.section_heading.clone(),
            reference_name: row.reference_name.clone(),
            reference_group: row.reference_group.clone(),
            citation_family: row.citation_family.clone(),
            primary_template_title: row.primary_template_title.clone(),
            source_type: row.source_type.clone(),
            source_origin: row.source_origin.clone(),
            source_family: row.source_family.clone(),
            authority_kind: row.authority_kind.clone(),
            source_authority: row.source_authority.clone(),
            reference_title: row.reference_title.clone(),
            source_container: row.source_container.clone(),
            source_author: row.source_author.clone(),
            source_domain: row.source_domain.clone(),
            source_date: row.source_date.clone(),
            canonical_url: row.canonical_url.clone(),
            identifier_keys: row.identifier_keys.clone(),
            identifier_entries: row.identifier_entries.clone(),
            source_urls: row.source_urls.clone(),
            retrieval_signals: row.retrieval_signals.clone(),
            summary_text: row.summary_text.clone(),
            template_titles: row.template_titles.clone(),
            link_titles: row.link_titles.clone(),
            reference_wikitext: row.reference_wikitext.clone(),
            token_estimate: row.token_estimate,
        };
        let entry = grouped.entry(row.citation_profile.clone()).or_default();
        if entry.citation_family.is_empty() {
            entry.citation_family = row.citation_family.clone();
        }
        if entry.source_type.is_empty() {
            entry.source_type = row.source_type.clone();
        }
        if entry.source_origin.is_empty() {
            entry.source_origin = row.source_origin.clone();
        }
        if entry.source_family.is_empty() {
            entry.source_family = row.source_family.clone();
        }
        entry.usage_count = entry.usage_count.saturating_add(1);
        entry.source_pages.insert(row.source_title);
        for template_title in row.template_titles {
            let count = entry.template_counts.entry(template_title).or_insert(0);
            *count = count.saturating_add(1);
        }
        for link_title in row.link_titles {
            let count = entry.link_counts.entry(link_title).or_insert(0);
            *count = count.saturating_add(1);
        }
        if !row.source_domain.is_empty() {
            let count = entry.domain_counts.entry(row.source_domain).or_insert(0);
            *count = count.saturating_add(1);
        }
        if !row.source_authority.is_empty() {
            let count = entry
                .authority_counts
                .entry(row.source_authority)
                .or_insert(0);
            *count = count.saturating_add(1);
        }
        for identifier in row.identifier_keys {
            let count = entry.identifier_counts.entry(identifier).or_insert(0);
            *count = count.saturating_add(1);
        }
        for identifier_entry in row.identifier_entries {
            let count = entry
                .identifier_entry_counts
                .entry(identifier_entry)
                .or_insert(0);
            *count = count.saturating_add(1);
        }
        for retrieval_signal in row.retrieval_signals {
            let count = entry
                .retrieval_signal_counts
                .entry(retrieval_signal)
                .or_insert(0);
            *count = count.saturating_add(1);
        }
        entry.examples.push(example);
    }

    let mut ranked = grouped.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_profile, left), (right_profile, right)| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| right.source_pages.len().cmp(&left.source_pages.len()))
            .then_with(|| {
                reference_example_rank_key(right.examples.first())
                    .cmp(&reference_example_rank_key(left.examples.first()))
            })
            .then_with(|| left_profile.cmp(right_profile))
    });
    ranked.truncate(limit);

    ranked
        .into_iter()
        .map(|(citation_profile, mut accumulator)| {
            accumulator.examples.sort_by(|left, right| {
                reference_example_rank_key(Some(right))
                    .cmp(&reference_example_rank_key(Some(left)))
                    .then_with(|| left.token_estimate.cmp(&right.token_estimate))
                    .then_with(|| left.source_title.cmp(&right.source_title))
            });
            accumulator
                .examples
                .truncate(AUTHORING_REFERENCE_EXAMPLE_LIMIT);
            Ok(ReferenceUsageSummary {
                citation_profile,
                citation_family: accumulator.citation_family,
                source_type: accumulator.source_type,
                source_origin: accumulator.source_origin,
                source_family: accumulator.source_family,
                usage_count: accumulator.usage_count,
                distinct_page_count: accumulator.source_pages.len(),
                example_pages: accumulator
                    .source_pages
                    .iter()
                    .take(AUTHORING_REFERENCE_EXAMPLE_LIMIT)
                    .cloned()
                    .collect(),
                common_templates: top_counted_keys(
                    &accumulator.template_counts,
                    AUTHORING_TEMPLATE_KEY_LIMIT,
                ),
                common_links: top_counted_keys(
                    &accumulator.link_counts,
                    AUTHORING_TEMPLATE_KEY_LIMIT,
                ),
                common_domains: top_counted_keys(
                    &accumulator.domain_counts,
                    AUTHORING_REFERENCE_DOMAIN_LIMIT,
                ),
                common_authorities: top_counted_keys(
                    &accumulator.authority_counts,
                    AUTHORING_REFERENCE_AUTHORITY_LIMIT,
                ),
                common_identifier_keys: top_counted_keys(
                    &accumulator.identifier_counts,
                    AUTHORING_REFERENCE_IDENTIFIER_LIMIT,
                ),
                common_identifier_entries: top_counted_keys(
                    &accumulator.identifier_entry_counts,
                    AUTHORING_REFERENCE_IDENTIFIER_LIMIT,
                ),
                common_retrieval_signals: top_counted_keys(
                    &accumulator.retrieval_signal_counts,
                    AUTHORING_REFERENCE_FLAG_LIMIT,
                ),
                example_references: accumulator.examples,
            })
        })
        .collect()
}

fn load_reference_rows_for_sources(
    connection: &Connection,
    source_titles: &[String],
) -> Result<Vec<IndexedReferenceUsageRow>> {
    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT source_title, source_relative_path, section_heading, reference_name, reference_group,
                citation_profile, citation_family, primary_template_title, source_type, source_origin,
                source_family, authority_kind, source_authority, reference_title, source_container,
                source_author, source_domain, source_date, canonical_url, identifier_keys,
                identifier_entries, source_urls, retrieval_signals, summary_text, reference_wikitext,
                template_titles, link_titles, token_estimate
         FROM indexed_page_references
         WHERE source_title IN ({placeholders})"
    );
    let values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare reference summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            let token_estimate_i64: i64 = row.get(27)?;
            Ok(IndexedReferenceUsageRow {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                section_heading: row.get(2)?,
                reference_name: row.get(3)?,
                reference_group: row.get(4)?,
                citation_profile: row.get(5)?,
                citation_family: row.get(6)?,
                primary_template_title: normalize_non_empty_string(row.get::<_, String>(7)?),
                source_type: row.get(8)?,
                source_origin: row.get(9)?,
                source_family: row.get(10)?,
                authority_kind: row.get(11)?,
                source_authority: row.get(12)?,
                reference_title: row.get(13)?,
                source_container: row.get(14)?,
                source_author: row.get(15)?,
                source_domain: row.get(16)?,
                source_date: row.get(17)?,
                canonical_url: row.get(18)?,
                identifier_keys: parse_string_list(&row.get::<_, String>(19)?),
                identifier_entries: parse_string_list(&row.get::<_, String>(20)?),
                source_urls: parse_string_list(&row.get::<_, String>(21)?),
                retrieval_signals: parse_string_list(&row.get::<_, String>(22)?),
                summary_text: row.get(23)?,
                reference_wikitext: row.get(24)?,
                template_titles: parse_string_list(&row.get::<_, String>(25)?),
                link_titles: parse_string_list(&row.get::<_, String>(26)?),
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run reference summary query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode reference summary row")?);
    }
    Ok(out)
}

#[derive(Default)]
struct MediaUsageAccumulator {
    usage_count: usize,
    source_pages: BTreeSet<String>,
    examples: Vec<MediaUsageExample>,
}

#[derive(Debug, Clone)]
struct IndexedMediaUsageRow {
    source_title: String,
    source_relative_path: String,
    section_heading: Option<String>,
    file_title: String,
    media_kind: String,
    caption_text: String,
    options: Vec<String>,
    token_estimate: usize,
}

pub(crate) fn summarize_media_usage_for_sources(
    connection: &Connection,
    source_titles: &[String],
    limit: usize,
) -> Result<Vec<MediaUsageSummary>> {
    if limit == 0 || source_titles.is_empty() || !table_exists(connection, "indexed_page_media")? {
        return Ok(Vec::new());
    }

    let rows = load_media_rows_for_sources(connection, source_titles)?;
    let mut grouped = BTreeMap::<String, MediaUsageAccumulator>::new();
    let mut file_titles = BTreeMap::<String, String>::new();
    let mut media_kinds = BTreeMap::<String, String>::new();
    for row in rows {
        let key = format!("{}\u{1f}{}", row.file_title, row.media_kind);
        file_titles.insert(key.clone(), row.file_title.clone());
        media_kinds.insert(key.clone(), row.media_kind.clone());
        let entry = grouped.entry(key).or_default();
        entry.usage_count = entry.usage_count.saturating_add(1);
        entry.source_pages.insert(row.source_title.clone());
        entry.examples.push(MediaUsageExample {
            source_title: row.source_title,
            source_relative_path: row.source_relative_path,
            section_heading: row.section_heading,
            caption_text: row.caption_text,
            options: row.options,
            token_estimate: row.token_estimate,
        });
    }

    let mut ranked = grouped.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_key, left), (right_key, right)| {
        right
            .usage_count
            .cmp(&left.usage_count)
            .then_with(|| right.source_pages.len().cmp(&left.source_pages.len()))
            .then_with(|| left_key.cmp(right_key))
    });
    ranked.truncate(limit);

    ranked
        .into_iter()
        .map(|(key, mut accumulator)| {
            accumulator.examples.sort_by(|left, right| {
                let left_has_caption = !left.caption_text.is_empty();
                let right_has_caption = !right.caption_text.is_empty();
                right_has_caption
                    .cmp(&left_has_caption)
                    .then_with(|| left.token_estimate.cmp(&right.token_estimate))
                    .then_with(|| left.source_title.cmp(&right.source_title))
            });
            accumulator.examples.truncate(AUTHORING_MEDIA_EXAMPLE_LIMIT);
            Ok(MediaUsageSummary {
                file_title: file_titles.get(&key).cloned().unwrap_or_default(),
                media_kind: media_kinds
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| "inline".to_string()),
                usage_count: accumulator.usage_count,
                distinct_page_count: accumulator.source_pages.len(),
                example_pages: accumulator
                    .source_pages
                    .iter()
                    .take(AUTHORING_MEDIA_EXAMPLE_LIMIT)
                    .cloned()
                    .collect(),
                example_usages: accumulator.examples,
            })
        })
        .collect()
}

fn load_media_rows_for_sources(
    connection: &Connection,
    source_titles: &[String],
) -> Result<Vec<IndexedMediaUsageRow>> {
    let placeholders = std::iter::repeat_n("?", source_titles.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT source_title, source_relative_path, section_heading, file_title, media_kind,
                caption_text, options_text, token_estimate
         FROM indexed_page_media
         WHERE source_title IN ({placeholders})"
    );
    let values = source_titles
        .iter()
        .cloned()
        .map(rusqlite::types::Value::from)
        .collect::<Vec<_>>();
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare media summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            let token_estimate_i64: i64 = row.get(7)?;
            Ok(IndexedMediaUsageRow {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                section_heading: row.get(2)?,
                file_title: row.get(3)?,
                media_kind: row.get(4)?,
                caption_text: row.get(5)?,
                options: parse_string_list(&row.get::<_, String>(6)?),
                token_estimate: usize::try_from(token_estimate_i64).unwrap_or(0),
            })
        })
        .context("failed to run media summary query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode media summary row")?);
    }
    Ok(out)
}

pub(crate) fn top_counted_keys(counts: &BTreeMap<String, usize>, limit: usize) -> Vec<String> {
    let mut ranked = counts
        .iter()
        .map(|(key, count)| (key.clone(), *count))
        .collect::<Vec<_>>();
    ranked.sort_by(|(left_key, left_count), (right_key, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_key.cmp(right_key))
    });
    ranked.into_iter().take(limit).map(|(key, _)| key).collect()
}

pub(crate) fn reference_example_rank_key(
    example: Option<&ReferenceUsageExample>,
) -> (
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
) {
    let Some(example) = example else {
        return (0, 0, 0, 0, 0, 0, 0, 0, 0);
    };
    (
        usize::from(example.primary_template_title.is_some()),
        usize::from(!example.reference_title.is_empty()),
        usize::from(!example.source_author.is_empty()),
        usize::from(!example.source_domain.is_empty()),
        usize::from(!example.source_container.is_empty() || !example.source_authority.is_empty()),
        example
            .identifier_entries
            .len()
            .max(example.identifier_keys.len()),
        example.retrieval_signals.len(),
        example.template_titles.len(),
        example.link_titles.len(),
    )
}
