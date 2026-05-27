use super::super::*;
use super::analysis::analyze_reference_body;

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
