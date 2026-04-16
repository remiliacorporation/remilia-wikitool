use super::*;

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
