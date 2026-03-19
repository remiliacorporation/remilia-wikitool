use super::prelude::*;

pub use super::model::{
    LocalMediaUsage, LocalReferenceUsage, MediaUsageExample, MediaUsageSummary,
    ReferenceAuditFilters, ReferenceAuditSummaryReport, ReferenceDuplicateGroup,
    ReferenceDuplicateKind, ReferenceDuplicatesReport, ReferenceListItem, ReferenceListReport,
    ReferenceUsageExample, ReferenceUsageSummary,
};

const REFERENCE_AUDIT_TOP_LIMIT: usize = 12;

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
    reference_index: usize,
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

    let rows = load_reference_rows(connection, Some(source_titles))?;
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

pub fn inspect_reference_summary(
    paths: &ResolvedPaths,
    source_titles: &[String],
    filters: &ReferenceAuditFilters,
) -> Result<Option<ReferenceAuditSummaryReport>> {
    let Some(connection) = open_reference_index_connection(paths)? else {
        return Ok(None);
    };
    let rows = filter_reference_rows(
        load_reference_rows(&connection, selection_titles(source_titles))?,
        filters,
    );
    let mut page_titles = BTreeSet::new();
    let mut domain_counts = BTreeMap::<String, usize>::new();
    let mut template_counts = BTreeMap::<String, usize>::new();
    let mut authority_counts = BTreeMap::<String, usize>::new();
    let mut identifier_key_counts = BTreeMap::<String, usize>::new();
    let mut identifier_entry_counts = BTreeMap::<String, usize>::new();

    for row in &rows {
        page_titles.insert(row.source_title.clone());
        if !row.source_domain.is_empty() {
            *domain_counts.entry(row.source_domain.clone()).or_insert(0) += 1;
        }
        if let Some(template_title) = row.primary_template_title.as_ref()
            && !template_title.is_empty()
        {
            *template_counts.entry(template_title.clone()).or_insert(0) += 1;
        }
        if !row.source_authority.is_empty() {
            *authority_counts
                .entry(row.source_authority.clone())
                .or_insert(0) += 1;
        }
        for identifier_key in &row.identifier_keys {
            *identifier_key_counts
                .entry(identifier_key.clone())
                .or_insert(0) += 1;
        }
        for identifier_entry in &row.identifier_entries {
            *identifier_entry_counts
                .entry(identifier_entry.clone())
                .or_insert(0) += 1;
        }
    }

    Ok(Some(ReferenceAuditSummaryReport {
        reference_count: rows.len(),
        distinct_page_count: page_titles.len(),
        distinct_domain_count: domain_counts.len(),
        distinct_template_count: template_counts.len(),
        distinct_authority_count: authority_counts.len(),
        distinct_identifier_key_count: identifier_key_counts.len(),
        distinct_identifier_entry_count: identifier_entry_counts.len(),
        top_domains: top_counted_keys(&domain_counts, REFERENCE_AUDIT_TOP_LIMIT),
        top_templates: top_counted_keys(&template_counts, REFERENCE_AUDIT_TOP_LIMIT),
        top_authorities: top_counted_keys(&authority_counts, REFERENCE_AUDIT_TOP_LIMIT),
        top_identifier_keys: top_counted_keys(&identifier_key_counts, REFERENCE_AUDIT_TOP_LIMIT),
        top_identifier_entries: top_counted_keys(
            &identifier_entry_counts,
            REFERENCE_AUDIT_TOP_LIMIT,
        ),
    }))
}

pub fn inspect_reference_list(
    paths: &ResolvedPaths,
    source_titles: &[String],
    filters: &ReferenceAuditFilters,
) -> Result<Option<ReferenceListReport>> {
    let Some(connection) = open_reference_index_connection(paths)? else {
        return Ok(None);
    };
    let rows = filter_reference_rows(
        load_reference_rows(&connection, selection_titles(source_titles))?,
        filters,
    );
    let items = rows
        .iter()
        .map(reference_list_item_from_row)
        .collect::<Vec<_>>();
    Ok(Some(ReferenceListReport {
        reference_count: items.len(),
        items,
    }))
}

pub fn inspect_reference_duplicates(
    paths: &ResolvedPaths,
    source_titles: &[String],
    filters: &ReferenceAuditFilters,
) -> Result<Option<ReferenceDuplicatesReport>> {
    let Some(connection) = open_reference_index_connection(paths)? else {
        return Ok(None);
    };
    let rows = filter_reference_rows(
        load_reference_rows(&connection, selection_titles(source_titles))?,
        filters,
    );
    let mut groups = Vec::new();
    groups.extend(build_duplicate_groups(
        &rows,
        ReferenceDuplicateKind::CanonicalUrl,
        |row| {
            if row.canonical_url.is_empty() {
                Vec::new()
            } else {
                vec![row.canonical_url.clone()]
            }
        },
    ));
    groups.extend(build_duplicate_groups(
        &rows,
        ReferenceDuplicateKind::NormalizedIdentifier,
        |row| {
            row.identifier_entries
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        },
    ));
    groups.extend(build_duplicate_groups(
        &rows,
        ReferenceDuplicateKind::ExactReferenceWikitext,
        |row| {
            let normalized = normalize_reference_wikitext(&row.reference_wikitext);
            if normalized.is_empty() {
                Vec::new()
            } else {
                vec![normalized]
            }
        },
    ));
    groups.sort_by(|left, right| {
        right
            .reference_count
            .cmp(&left.reference_count)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.match_key.cmp(&right.match_key))
    });

    Ok(Some(ReferenceDuplicatesReport {
        duplicate_group_count: groups.len(),
        duplicated_reference_count: groups.iter().map(|group| group.reference_count).sum(),
        groups,
    }))
}

fn load_reference_rows(
    connection: &Connection,
    source_titles: Option<&[String]>,
) -> Result<Vec<IndexedReferenceUsageRow>> {
    let (sql, values) = if let Some(source_titles) = source_titles {
        let placeholders = std::iter::repeat_n("?", source_titles.len())
            .collect::<Vec<_>>()
            .join(", ");
        (
            format!(
                "SELECT source_title, source_relative_path, reference_index, section_heading, reference_name, reference_group,
                        citation_profile, citation_family, primary_template_title, source_type, source_origin,
                        source_family, authority_kind, source_authority, reference_title, source_container,
                        source_author, source_domain, source_date, canonical_url, identifier_keys,
                        identifier_entries, source_urls, retrieval_signals, summary_text, reference_wikitext,
                        template_titles, link_titles, token_estimate
                 FROM indexed_page_references
                 WHERE source_title IN ({placeholders})
                 ORDER BY source_title ASC, reference_index ASC"
            ),
            source_titles
                .iter()
                .cloned()
                .map(rusqlite::types::Value::from)
                .collect::<Vec<_>>(),
        )
    } else {
        (
            "SELECT source_title, source_relative_path, reference_index, section_heading, reference_name, reference_group,
                    citation_profile, citation_family, primary_template_title, source_type, source_origin,
                    source_family, authority_kind, source_authority, reference_title, source_container,
                    source_author, source_domain, source_date, canonical_url, identifier_keys,
                    identifier_entries, source_urls, retrieval_signals, summary_text, reference_wikitext,
                    template_titles, link_titles, token_estimate
             FROM indexed_page_references
             ORDER BY source_title ASC, reference_index ASC"
                .to_string(),
            Vec::new(),
        )
    };
    let mut statement = connection
        .prepare(&sql)
        .context("failed to prepare reference summary query")?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            let reference_index_i64: i64 = row.get(2)?;
            let token_estimate_i64: i64 = row.get(28)?;
            Ok(IndexedReferenceUsageRow {
                source_title: row.get(0)?,
                source_relative_path: row.get(1)?,
                reference_index: usize::try_from(reference_index_i64).unwrap_or(0),
                section_heading: row.get(3)?,
                reference_name: row.get(4)?,
                reference_group: row.get(5)?,
                citation_profile: row.get(6)?,
                citation_family: row.get(7)?,
                primary_template_title: normalize_non_empty_string(row.get::<_, String>(8)?),
                source_type: row.get(9)?,
                source_origin: row.get(10)?,
                source_family: row.get(11)?,
                authority_kind: row.get(12)?,
                source_authority: row.get(13)?,
                reference_title: row.get(14)?,
                source_container: row.get(15)?,
                source_author: row.get(16)?,
                source_domain: row.get(17)?,
                source_date: row.get(18)?,
                canonical_url: row.get(19)?,
                identifier_keys: parse_string_list(&row.get::<_, String>(20)?),
                identifier_entries: parse_string_list(&row.get::<_, String>(21)?),
                source_urls: parse_string_list(&row.get::<_, String>(22)?),
                retrieval_signals: parse_string_list(&row.get::<_, String>(23)?),
                summary_text: row.get(24)?,
                reference_wikitext: row.get(25)?,
                template_titles: parse_string_list(&row.get::<_, String>(26)?),
                link_titles: parse_string_list(&row.get::<_, String>(27)?),
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

fn open_reference_index_connection(paths: &ResolvedPaths) -> Result<Option<Connection>> {
    if !paths.db_path.exists() {
        return Ok(None);
    }
    let connection = open_initialized_database_connection(&paths.db_path)?;
    if !table_exists(&connection, "indexed_page_references")? {
        return Ok(None);
    }
    Ok(Some(connection))
}

fn selection_titles(source_titles: &[String]) -> Option<&[String]> {
    if source_titles.is_empty() {
        None
    } else {
        Some(source_titles)
    }
}

fn filter_reference_rows(
    rows: Vec<IndexedReferenceUsageRow>,
    filters: &ReferenceAuditFilters,
) -> Vec<IndexedReferenceUsageRow> {
    let normalized_domain = normalize_optional_filter(filters.domain.as_deref());
    let normalized_template = normalize_optional_filter(filters.template.as_deref());
    let normalized_authority = normalize_optional_filter(filters.authority.as_deref());
    let normalized_identifier_key = normalize_optional_filter(filters.identifier_key.as_deref());
    let normalized_identifier = normalize_optional_filter(filters.identifier.as_deref());

    rows.into_iter()
        .filter(|row| {
            if let Some(domain) = normalized_domain.as_deref()
                && row.source_domain.to_ascii_lowercase() != domain
            {
                return false;
            }
            if let Some(template) = normalized_template.as_deref()
                && !row_matches_template_filter(row, template)
            {
                return false;
            }
            if let Some(authority) = normalized_authority.as_deref()
                && row.source_authority.to_ascii_lowercase() != authority
            {
                return false;
            }
            row_matches_identifier_filters(
                row,
                normalized_identifier_key.as_deref(),
                normalized_identifier.as_deref(),
            )
        })
        .collect()
}

fn row_matches_template_filter(row: &IndexedReferenceUsageRow, template_filter: &str) -> bool {
    row.primary_template_title
        .as_deref()
        .is_some_and(|template_title| template_matches_filter(template_title, template_filter))
        || row
            .template_titles
            .iter()
            .any(|template_title| template_matches_filter(template_title, template_filter))
}

fn template_matches_filter(template_title: &str, template_filter: &str) -> bool {
    let normalized = template_title.to_ascii_lowercase();
    normalized == template_filter
        || normalized
            .strip_prefix("template:")
            .is_some_and(|value| value == template_filter)
}

fn row_matches_identifier_filters(
    row: &IndexedReferenceUsageRow,
    identifier_key_filter: Option<&str>,
    identifier_filter: Option<&str>,
) -> bool {
    if identifier_key_filter.is_none() && identifier_filter.is_none() {
        return true;
    }

    let mut matched_key = identifier_key_filter.is_none();
    let mut matched_identifier = identifier_filter.is_none();
    for entry in &row.identifier_entries {
        let normalized_entry = entry.to_ascii_lowercase();
        let (entry_key, entry_value) = split_identifier_entry(&normalized_entry);
        if let Some(identifier_key_filter) = identifier_key_filter {
            if entry_key == identifier_key_filter {
                matched_key = true;
            } else {
                continue;
            }
        }
        if let Some(identifier_filter) = identifier_filter
            && (normalized_entry == identifier_filter || entry_value == identifier_filter)
        {
            matched_identifier = true;
        }
    }

    if let Some(identifier_key_filter) = identifier_key_filter
        && !matched_key
    {
        matched_key = row
            .identifier_keys
            .iter()
            .any(|key| key.to_ascii_lowercase() == identifier_key_filter);
    }

    matched_key && matched_identifier
}

fn split_identifier_entry(entry: &str) -> (&str, &str) {
    if let Some((key, value)) = entry.split_once(':') {
        (key.trim(), value.trim())
    } else {
        ("", entry.trim())
    }
}

fn normalize_optional_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn reference_list_item_from_row(row: &IndexedReferenceUsageRow) -> ReferenceListItem {
    ReferenceListItem {
        source_title: row.source_title.clone(),
        source_relative_path: row.source_relative_path.clone(),
        section_heading: row.section_heading.clone(),
        reference_index: row.reference_index,
        reference_name: row.reference_name.clone(),
        reference_group: row.reference_group.clone(),
        citation_profile: row.citation_profile.clone(),
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
        reference_wikitext: row.reference_wikitext.clone(),
        template_titles: row.template_titles.clone(),
        link_titles: row.link_titles.clone(),
        token_estimate: row.token_estimate,
    }
}

fn build_duplicate_groups<F>(
    rows: &[IndexedReferenceUsageRow],
    kind: ReferenceDuplicateKind,
    key_fn: F,
) -> Vec<ReferenceDuplicateGroup>
where
    F: Fn(&IndexedReferenceUsageRow) -> Vec<String>,
{
    let mut grouped = BTreeMap::<String, Vec<&IndexedReferenceUsageRow>>::new();
    for row in rows {
        for key in key_fn(row) {
            grouped.entry(key).or_default().push(row);
        }
    }

    let mut out = Vec::new();
    for (match_key, grouped_rows) in grouped {
        if grouped_rows.len() < 2 {
            continue;
        }
        let mut source_titles = grouped_rows
            .iter()
            .map(|row| row.source_title.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        source_titles.sort();
        let items = grouped_rows
            .iter()
            .map(|row| reference_list_item_from_row(row))
            .collect::<Vec<_>>();
        out.push(ReferenceDuplicateGroup {
            kind: kind.clone(),
            match_key,
            reference_count: items.len(),
            distinct_page_count: source_titles.len(),
            source_titles,
            items,
        });
    }
    out
}

fn normalize_reference_wikitext(value: &str) -> String {
    normalize_spaces(value)
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
