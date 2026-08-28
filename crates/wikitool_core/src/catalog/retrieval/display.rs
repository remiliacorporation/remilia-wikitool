use super::*;

fn chunk_word_like_count(value: &str) -> usize {
    value
        .split_whitespace()
        .map(|token| {
            token
                .chars()
                .filter(|ch| ch.is_alphanumeric())
                .collect::<String>()
        })
        .filter(|token| token.chars().count() >= 2)
        .count()
}

fn chunk_markup_char_count(value: &str) -> usize {
    value
        .chars()
        .filter(|ch| matches!(ch, '[' | ']' | '{' | '}' | '<' | '>' | '|' | '='))
        .count()
}

fn consume_balanced_template_prefix(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'{' || bytes[1] != b'{' {
        return None;
    }

    let mut depth = 0usize;
    let mut cursor = 0usize;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            depth = depth.saturating_add(1);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'}' && bytes[cursor + 1] == b'}' {
            if depth == 0 {
                return None;
            }
            depth -= 1;
            cursor += 2;
            if depth == 0 {
                return Some(cursor);
            }
            continue;
        }
        cursor += 1;
    }
    None
}

fn consume_balanced_link_prefix(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'[' || bytes[1] != b'[' {
        return None;
    }

    let mut cursor = 2usize;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b']' && bytes[cursor + 1] == b']' {
            return Some(cursor + 2);
        }
        cursor += 1;
    }
    None
}

fn strip_leading_authoring_metadata(chunk_text: &str) -> &str {
    let mut remainder = chunk_text.trim_start();
    loop {
        if let Some(stripped) = remainder.strip_prefix("__") {
            remainder = stripped.trim_start();
            continue;
        }

        if let Some(end) = consume_balanced_template_prefix(remainder) {
            remainder = remainder[end..].trim_start();
            continue;
        }

        let lowered = remainder.to_ascii_lowercase();
        let starts_with_meta_link = lowered.starts_with("[[file:")
            || lowered.starts_with("[[image:")
            || lowered.starts_with("[[category:");
        if starts_with_meta_link && let Some(end) = consume_balanced_link_prefix(remainder) {
            remainder = remainder[end..].trim_start();
            continue;
        }

        return remainder;
    }
}

pub(super) fn sanitize_main_namespace_prose(value: &str) -> Option<String> {
    let normalized = normalize_spaces(value);
    if normalized.is_empty() {
        return None;
    }

    let stripped = normalize_spaces(strip_leading_authoring_metadata(&normalized));
    if stripped == normalized {
        return Some(normalized);
    }
    if stripped.is_empty() || chunk_word_like_count(&stripped) < 5 {
        return None;
    }
    Some(stripped)
}

pub(super) fn best_context_preview(
    section_rows: &[IndexedSectionRecord],
    prefer_main_namespace_prose: bool,
) -> Option<String> {
    for section in section_rows {
        let summary = normalize_spaces(&section.summary_text);
        if summary.is_empty() {
            continue;
        }
        if prefer_main_namespace_prose {
            if let Some(sanitized) = sanitize_main_namespace_prose(&summary) {
                return Some(sanitized);
            }
            continue;
        }
        return Some(summary);
    }
    None
}

pub(super) fn sanitize_context_chunks_for_display(
    chunks: Vec<LocalContextChunk>,
    prefer_main_namespace_prose: bool,
) -> Vec<LocalContextChunk> {
    if !prefer_main_namespace_prose {
        return chunks;
    }

    let mut sanitized = Vec::with_capacity(chunks.len());
    for mut chunk in chunks {
        let Some(chunk_text) = sanitize_main_namespace_prose(&chunk.chunk_text) else {
            continue;
        };
        chunk.token_estimate = estimate_tokens(&chunk_text);
        chunk.chunk_text = chunk_text;
        sanitized.push(chunk);
    }

    if sanitized.is_empty() {
        Vec::new()
    } else {
        sanitized
    }
}

pub(crate) fn chunk_looks_like_noise(chunk_text: &str) -> bool {
    let normalized = normalize_spaces(chunk_text);
    if normalized.is_empty() {
        return true;
    }

    let lowered = normalized.to_ascii_lowercase();
    if lowered.starts_with("#redirect") {
        return true;
    }

    let char_count = normalized.chars().count();
    let alphanumeric_count = normalized.chars().filter(|ch| ch.is_alphanumeric()).count();
    let word_like_count = chunk_word_like_count(&normalized);
    let markup_char_count = chunk_markup_char_count(&normalized);

    if alphanumeric_count < 10 {
        return true;
    }
    if word_like_count < 3 && alphanumeric_count < 24 {
        return true;
    }
    if normalized.ends_with("</ref>") && word_like_count < 5 {
        return true;
    }
    if normalized.contains('|') && normalized.ends_with("}}</ref>") {
        let prefix = normalized.split('|').next().unwrap_or_default();
        if chunk_word_like_count(prefix) < 5 {
            return true;
        }
    }
    markup_char_count.saturating_mul(3) >= char_count.saturating_mul(2) && word_like_count < 6
}

pub(crate) fn section_heading_is_low_signal(section_heading: Option<&str>) -> bool {
    let heading = section_heading.unwrap_or_default().to_ascii_lowercase();
    if heading.is_empty() {
        return false;
    }
    [
        "references",
        "notes",
        "external links",
        "further reading",
        "bibliography",
        "gallery",
        "see also",
    ]
    .iter()
    .any(|low_signal| heading.contains(low_signal))
}

pub(super) fn chunk_allowed_for_audience(
    chunk: &RetrievedChunk,
    audience: RetrievalAudience,
) -> bool {
    if chunk_looks_like_noise(&chunk.chunk_text) {
        return false;
    }

    if chunk.source_namespace != Namespace::Main.as_str() {
        return false;
    }
    if is_translation_variant(&chunk.source_title) {
        return false;
    }
    if section_heading_is_low_signal(chunk.section_heading.as_deref()) {
        return false;
    }

    if audience != RetrievalAudience::Authoring {
        return true;
    }

    let normalized = normalize_spaces(&chunk.chunk_text);
    let lowered = normalized.to_ascii_lowercase();
    if lowered.contains("{{reflist") {
        return false;
    }
    if lowered.starts_with("[[category:") {
        return false;
    }
    if lowered.matches("[[category:").count() >= 2 && chunk_word_like_count(&normalized) < 12 {
        return false;
    }
    if lowered.starts_with("{{see also|") {
        return false;
    }

    let prose_tail = normalize_spaces(strip_leading_authoring_metadata(&normalized));
    if prose_tail.is_empty() || chunk_word_like_count(&prose_tail) < 5 {
        return false;
    }

    true
}

pub(super) fn sanitize_chunk_for_audience(
    mut chunk: RetrievedChunk,
    audience: RetrievalAudience,
) -> Option<RetrievedChunk> {
    if chunk.source_namespace == Namespace::Main.as_str() {
        let chunk_text = sanitize_main_namespace_prose(&chunk.chunk_text)?;
        chunk.token_estimate = estimate_tokens(&chunk_text);
        chunk.chunk_text = chunk_text;
    } else {
        let normalized = normalize_spaces(&chunk.chunk_text);
        if normalized.is_empty() {
            return None;
        }
        chunk.token_estimate = estimate_tokens(&normalized);
        chunk.chunk_text = normalized;
    }

    if !chunk_allowed_for_audience(&chunk, audience) {
        return None;
    }

    Some(chunk)
}
