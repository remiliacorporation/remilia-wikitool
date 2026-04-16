use super::*;

pub(crate) fn parse_parameter_key_list(value: &str) -> Vec<String> {
    if value.trim().is_empty() || value == NO_PARAMETER_KEYS_SENTINEL {
        return Vec::new();
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) fn serialize_string_list(values: &[String]) -> String {
    let normalized = values
        .iter()
        .map(|value| normalize_spaces(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        return NO_STRING_LIST_SENTINEL.to_string();
    }
    normalized.join("\n")
}

pub(crate) fn parse_string_list(value: &str) -> Vec<String> {
    if value.trim().is_empty() || value == NO_STRING_LIST_SENTINEL {
        return Vec::new();
    }
    value
        .lines()
        .map(normalize_spaces)
        .filter(|item| !item.is_empty())
        .collect()
}

pub(crate) fn normalize_non_empty_string(value: String) -> Option<String> {
    let normalized = normalize_spaces(&value);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(crate) fn canonical_parameter_key_list(keys: &[String]) -> String {
    if keys.is_empty() {
        return NO_PARAMETER_KEYS_SENTINEL.to_string();
    }
    let mut normalized = Vec::new();
    for key in keys {
        let key = normalize_template_parameter_key(key);
        if !key.is_empty() {
            normalized.push(key);
        }
    }
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        return NO_PARAMETER_KEYS_SENTINEL.to_string();
    }
    normalized.join(",")
}

pub(crate) fn apply_context_chunk_budget(
    chunks: Vec<LocalContextChunk>,
    max_chunks: usize,
    token_budget: usize,
) -> Vec<LocalContextChunk> {
    let mut out = Vec::new();
    let mut used_tokens = 0usize;
    for chunk in chunks {
        if out.len() >= max_chunks {
            break;
        }
        let next_tokens = used_tokens.saturating_add(chunk.token_estimate);
        if !out.is_empty() && next_tokens > token_budget {
            break;
        }
        used_tokens = next_tokens;
        out.push(chunk);
    }
    out
}

pub(crate) fn normalize_spaces(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;

    for ch in value.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
                previous_was_space = true;
            }
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }

    output.trim().to_string()
}
