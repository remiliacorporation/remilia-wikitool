use super::*;

pub(super) struct SearchContext {
    pub(super) query_lower: String,
    pub(super) query_key: String,
    pub(super) like_pattern: String,
    pub(super) fts_query: Option<String>,
    pub(super) limit: usize,
}

impl SearchContext {
    pub(super) fn new(query: &str, limit: usize) -> Result<Self> {
        let normalized = normalize_title(query);
        let lowered = normalized.to_ascii_lowercase();
        let query_key = normalize_retrieval_key(&normalized);
        if limit == 0 {
            bail!("search limit must be greater than zero");
        }
        Ok(Self {
            query_lower: lowered.clone(),
            query_key: query_key.clone(),
            like_pattern: format!("%{lowered}%"),
            fts_query: build_docs_fts_query(&normalized, &query_key),
            limit,
        })
    }
}

pub(super) fn build_docs_fts_query(normalized_query: &str, query_key: &str) -> Option<String> {
    let terms = collect_docs_fts_terms(normalized_query, query_key);
    if terms.is_empty() {
        return None;
    }

    let phrase = format!("\"{}\"", terms.join(" "));
    if terms.len() == 1 {
        return Some(format!("{phrase} OR {}*", terms[0]));
    }

    let conjunction = terms
        .iter()
        .map(|term| format!("{term}*"))
        .collect::<Vec<_>>()
        .join(" AND ");
    Some(format!("{phrase} OR ({conjunction})"))
}

pub(super) fn collect_docs_fts_terms(normalized_query: &str, query_key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    let push_current = |out: &mut Vec<String>, current: &mut String| {
        if current.is_empty() {
            return;
        }
        if !out.iter().any(|value| value.as_str() == current.as_str()) {
            out.push(current.clone());
        }
        current.clear();
    };

    for ch in normalized_query.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else {
            push_current(&mut out, &mut current);
        }
    }
    push_current(&mut out, &mut current);

    if out.is_empty() {
        for part in query_key.split(' ') {
            let term = part.trim();
            if term.is_empty() || out.iter().any(|value| value == term) {
                continue;
            }
            out.push(term.to_string());
        }
    }

    out
}

pub(super) fn fts_position_bonus(index: usize, base: usize) -> usize {
    base.saturating_sub(index.saturating_mul(4)).max(8)
}

#[derive(Debug, Clone)]
pub(super) struct SearchScope {
    pub(super) include_pages: bool,
    pub(super) include_sections: bool,
    pub(super) include_symbols: bool,
    pub(super) include_examples: bool,
    pub(super) corpus_kind_filter: Option<String>,
}

impl SearchScope {
    pub(super) fn parse(value: Option<&str>) -> Result<Self> {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(Self {
                include_pages: true,
                include_sections: true,
                include_symbols: true,
                include_examples: true,
                corpus_kind_filter: None,
            });
        };
        let lowered = value.to_ascii_lowercase();
        match lowered.as_str() {
            "page" => Ok(Self {
                include_pages: true,
                include_sections: false,
                include_symbols: false,
                include_examples: false,
                corpus_kind_filter: None,
            }),
            "section" => Ok(Self {
                include_pages: false,
                include_sections: true,
                include_symbols: false,
                include_examples: false,
                corpus_kind_filter: None,
            }),
            "symbol" => Ok(Self {
                include_pages: false,
                include_sections: false,
                include_symbols: true,
                include_examples: false,
                corpus_kind_filter: None,
            }),
            "example" => Ok(Self {
                include_pages: false,
                include_sections: false,
                include_symbols: false,
                include_examples: true,
                corpus_kind_filter: None,
            }),
            "extension" | "technical" | "profile" => Ok(Self {
                include_pages: true,
                include_sections: true,
                include_symbols: true,
                include_examples: true,
                corpus_kind_filter: Some(lowered),
            }),
            _ => bail!(
                "unsupported docs tier `{value}`; expected page|section|symbol|example|extension|technical|profile"
            ),
        }
    }
}

pub(super) fn make_snippet(content: &str, lowered_query: &str) -> String {
    let normalized = normalize_title(content);
    if normalized.is_empty() {
        return "<empty>".to_string();
    }
    let lowered = normalized.to_ascii_lowercase();
    let Some(index) = lowered.find(lowered_query) else {
        return truncate_text(&normalized, 200);
    };

    let start = clamp_to_char_boundary(&normalized, index.saturating_sub(80));
    let end = clamp_to_char_boundary(
        &normalized,
        index
            .saturating_add(lowered_query.len())
            .saturating_add(120)
            .min(normalized.len()),
    );
    let mut snippet = normalized[start..end].trim().to_string();
    if start > 0 {
        snippet = format!("...{snippet}");
    }
    if end < normalized.len() {
        snippet.push_str("...");
    }
    snippet
}

fn truncate_text(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let end = clamp_to_char_boundary(value, max_len);
    format!("{}...", &value[..end])
}

fn clamp_to_char_boundary(value: &str, mut index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    while !value.is_char_boundary(index) {
        index = index.saturating_sub(1);
    }
    index
}
