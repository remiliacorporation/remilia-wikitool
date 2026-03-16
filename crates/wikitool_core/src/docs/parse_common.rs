use super::parse_sections::strip_summary_noise;

pub(crate) fn collapse_whitespace(value: &str) -> String {
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

pub(crate) fn normalize_title(value: &str) -> String {
    collapse_whitespace(&value.replace('_', " "))
}

pub(crate) fn estimate_token_count(value: &str) -> usize {
    let text = collapse_whitespace(value);
    if text.is_empty() {
        return 0;
    }
    text.len().div_ceil(4)
}

pub(crate) fn estimate_tokens(value: &str) -> usize {
    estimate_token_count(value)
}

pub(crate) fn normalize_retrieval_key(value: &str) -> String {
    let normalized = normalize_title(value);
    let mut out = String::with_capacity(normalized.len());
    let mut previous_was_space = false;
    for ch in normalized.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                out.push(' ');
                previous_was_space = true;
            }
            continue;
        }
        previous_was_space = false;
        out.push(ch.to_ascii_lowercase());
    }
    out.trim().to_string()
}

pub(crate) fn truncate_text(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let mut end = max_len.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
}

pub(crate) fn make_summary_text(value: &str, max_len: usize) -> String {
    let stripped = strip_summary_noise(value);
    if stripped.is_empty() {
        return String::new();
    }
    truncate_text(&stripped, max_len)
}

pub(crate) fn namespace_label(title: &str) -> String {
    if let Some(index) = title.find(':') {
        let namespace = title[..index].trim();
        if !namespace.is_empty() {
            return namespace.to_string();
        }
    }
    "Main".to_string()
}

pub(crate) fn classify_docs_page_kind(title: &str) -> String {
    if let Some(symbol) = title.strip_prefix("Manual:Hooks/")
        && !symbol.trim().is_empty()
    {
        return "hook_page".to_string();
    }
    if title.starts_with("Manual:$wg") {
        return "config_page".to_string();
    }
    if title == "Manual:Hooks" {
        return "hooks_index".to_string();
    }
    if title == "Manual:Configuration settings" || title == "Manual:$wg" {
        return "config_index".to_string();
    }
    if title.starts_with("API:") {
        if title.contains("/Sample code ") {
            return "api_example_page".to_string();
        }
        return "api_page".to_string();
    }
    if title == "Help:Extension:ParserFunctions" {
        return "parser_reference".to_string();
    }
    if title == "Help:Magic words" {
        return "magic_word_reference".to_string();
    }
    if title == "Help:Tags" {
        return "tag_reference".to_string();
    }
    if title == "Extension:Scribunto/Lua reference manual" {
        return "lua_reference".to_string();
    }
    if title.starts_with("Manual:") {
        return "manual_page".to_string();
    }
    if title.starts_with("Extension:") {
        return "extension_page".to_string();
    }
    if title.starts_with("Help:") {
        return "help_page".to_string();
    }
    "page".to_string()
}

pub(crate) fn is_translation_variant(title: &str) -> bool {
    let Some((_, suffix)) = title.rsplit_once('/') else {
        return false;
    };
    let suffix = suffix.trim();
    if suffix.is_empty() || suffix.contains(' ') {
        return false;
    }
    if suffix.eq_ignore_ascii_case("qqq") {
        return true;
    }

    let mut letter_count = 0usize;
    for ch in suffix.chars() {
        if ch.is_ascii_lowercase() {
            letter_count += 1;
            continue;
        }
        if ch == '-' || ch.is_ascii_digit() {
            continue;
        }
        return false;
    }
    letter_count >= 2 && suffix.len() <= 12
}
