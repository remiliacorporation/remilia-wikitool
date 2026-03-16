use super::parse_markup::{decamelize, dedupe_strings};
use super::parse_sections::RawSection;
use super::*;
use std::collections::BTreeSet;

pub(super) fn extract_content_symbols(
    page_title: &str,
    page_kind: &str,
    content: &str,
    sections: &[RawSection],
) -> Vec<ParsedDocsSymbol> {
    let mut symbols = Vec::new();
    symbols.extend(extract_config_symbols(page_title, page_kind, sections));
    symbols.extend(extract_parser_function_symbols(page_title, content));
    symbols.extend(extract_magic_word_symbols(page_title, page_kind, content));
    symbols.extend(extract_tag_symbols(page_title, page_kind, content));
    symbols.extend(extract_heading_symbols(page_title, page_kind, sections));
    dedupe_symbols(&mut symbols);
    symbols
}

pub(super) fn extract_title_symbols(page_title: &str, page_kind: &str) -> Vec<ParsedDocsSymbol> {
    let mut symbols = Vec::new();
    match page_kind {
        "hook_page" => {
            if let Some(symbol_name) = page_title.strip_prefix("Manual:Hooks/") {
                symbols.push(build_symbol(
                    page_title,
                    Some(symbol_name),
                    "hook",
                    "page_title",
                    page_title,
                    None,
                    "",
                ));
            }
        }
        "config_page" => {
            if let Some(symbol_name) = page_title.strip_prefix("Manual:") {
                symbols.push(build_symbol(
                    page_title,
                    Some(symbol_name),
                    "config",
                    "page_title",
                    page_title,
                    None,
                    "",
                ));
            }
        }
        "api_page" => {
            if let Some(symbol_name) = page_title.strip_prefix("API:")
                && !symbol_name.contains('/')
            {
                symbols.push(build_symbol(
                    page_title,
                    Some(symbol_name),
                    "api_page",
                    "page_title",
                    page_title,
                    None,
                    "",
                ));
            }
        }
        _ => {}
    }
    symbols
}

fn extract_heading_symbols(
    page_title: &str,
    page_kind: &str,
    sections: &[RawSection],
) -> Vec<ParsedDocsSymbol> {
    let mut symbols = Vec::new();
    for section in sections {
        if section.kind == "lead" {
            continue;
        }
        let heading = section.heading.trim();
        if heading.is_empty() {
            continue;
        }
        let should_capture = heading.starts_with('$')
            || heading.starts_with('#')
            || heading.starts_with('<')
            || heading.contains("::")
            || heading.contains('.')
            || (page_kind == "lua_reference" && heading.ends_with(')'));
        if !should_capture {
            continue;
        }
        let symbol_kind = if heading.starts_with('$') {
            "config"
        } else if heading.starts_with('#') {
            "parser_function"
        } else if heading.starts_with('<') {
            "tag"
        } else if page_kind == "lua_reference" {
            "lua_symbol"
        } else {
            "symbol"
        };
        symbols.push(build_symbol(
            page_title,
            Some(heading),
            symbol_kind,
            "heading",
            page_title,
            Some(section.heading.clone()),
            &section.text,
        ));
    }
    symbols
}

fn extract_config_symbols(
    page_title: &str,
    page_kind: &str,
    sections: &[RawSection],
) -> Vec<ParsedDocsSymbol> {
    let mut symbols = Vec::new();
    for section in sections {
        let section_heading = if section.kind == "lead" {
            None
        } else {
            Some(section.heading.clone())
        };
        for identifier in scan_config_identifiers(&section.text, page_kind) {
            symbols.push(build_symbol(
                page_title,
                Some(&identifier),
                "config",
                "inline_config",
                page_title,
                section_heading.clone(),
                &section.text,
            ));
        }
    }
    dedupe_symbols(&mut symbols);
    symbols
}

fn scan_config_identifiers(content: &str, page_kind: &str) -> Vec<String> {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < chars.len() {
        if chars[cursor] != '$' {
            cursor += 1;
            continue;
        }

        let start = cursor;
        cursor += 1;
        if cursor >= chars.len() || !(chars[cursor].is_ascii_alphabetic() || chars[cursor] == '_') {
            continue;
        }

        while cursor < chars.len()
            && (chars[cursor].is_ascii_alphanumeric() || chars[cursor] == '_')
        {
            cursor += 1;
        }

        let candidate = chars[start..cursor].iter().collect::<String>();
        if is_docs_config_identifier(&candidate, page_kind) {
            out.push(candidate);
        }
    }

    dedupe_strings(&mut out);
    out
}

fn is_docs_config_identifier(value: &str, page_kind: &str) -> bool {
    if value.starts_with("$wg") || value.starts_with("$eg") {
        return true;
    }
    page_kind == "config_page"
}

fn extract_parser_function_symbols(page_title: &str, content: &str) -> Vec<ParsedDocsSymbol> {
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    let mut out = Vec::new();
    while cursor + 3 < bytes.len() {
        if bytes[cursor..].starts_with(b"{{#") {
            let mut end = cursor + 3;
            while end < bytes.len() {
                let ch = bytes[end] as char;
                if matches!(ch, ':' | '|' | '}' | '\n' | '\r') {
                    break;
                }
                end += 1;
            }
            if end > cursor + 3 {
                let name = format!("#{}", content[cursor + 3..end].trim());
                out.push(build_symbol(
                    page_title,
                    Some(&name),
                    "parser_function",
                    "wikitext",
                    page_title,
                    None,
                    "",
                ));
            }
            cursor = end;
            continue;
        }
        cursor += 1;
    }
    out
}

fn extract_magic_word_symbols(
    page_title: &str,
    page_kind: &str,
    content: &str,
) -> Vec<ParsedDocsSymbol> {
    if page_kind != "magic_word_reference" && !page_title.contains("Magic words") {
        return Vec::new();
    }

    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    let mut out = Vec::new();
    while cursor + 2 < bytes.len() {
        if bytes[cursor..].starts_with(b"{{") {
            let mut end = cursor + 2;
            while end < bytes.len() {
                let ch = bytes[end] as char;
                if matches!(ch, '|' | '}' | ':' | '\n' | '\r' | ' ') {
                    break;
                }
                end += 1;
            }
            if end > cursor + 2 {
                let candidate = content[cursor + 2..end].trim();
                if looks_like_magic_word(candidate) {
                    out.push(build_symbol(
                        page_title,
                        Some(candidate),
                        "magic_word",
                        "wikitext",
                        page_title,
                        None,
                        "",
                    ));
                }
            }
            cursor = end;
            continue;
        }
        cursor += 1;
    }
    out
}

fn extract_tag_symbols(page_title: &str, page_kind: &str, content: &str) -> Vec<ParsedDocsSymbol> {
    let tag_focused = page_kind == "tag_reference" || page_title.contains("Tags");
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    let mut names = BTreeSet::new();
    while cursor < bytes.len() {
        if bytes[cursor] == b'<' {
            let mut start = cursor + 1;
            if start < bytes.len() && bytes[start] == b'/' {
                start += 1;
            }
            if start >= bytes.len() {
                break;
            }
            let mut end = start;
            while end < bytes.len() {
                let ch = bytes[end] as char;
                if !(ch.is_ascii_alphanumeric() || ch == '-' || ch == ':') {
                    break;
                }
                end += 1;
            }
            if end > start {
                let tag_name = content[start..end].to_ascii_lowercase();
                if !is_ignored_tag_name(&tag_name)
                    && (tag_focused || looks_like_extension_tag(&tag_name))
                {
                    names.insert(tag_name);
                }
            }
            cursor = end;
            continue;
        }
        cursor += 1;
    }

    names
        .into_iter()
        .map(|name| {
            let wrapped = format!("<{name}>");
            build_symbol(
                page_title,
                Some(&wrapped),
                "tag",
                "html_tag",
                page_title,
                None,
                "",
            )
        })
        .collect()
}

fn build_symbol(
    page_title: &str,
    symbol_name: Option<&str>,
    symbol_kind: &str,
    origin: &str,
    canonical_source: &str,
    section_heading: Option<String>,
    context_text: &str,
) -> ParsedDocsSymbol {
    let symbol_name = symbol_name.unwrap_or(canonical_source).trim();
    let canonical_name = normalize_symbol_name(symbol_name);
    let aliases = build_symbol_aliases(&canonical_name, symbol_kind);
    let summary_text = if context_text.trim().is_empty() {
        format!("{canonical_name} documented on {page_title}")
    } else {
        make_summary_text(context_text, 220)
    };
    let signature_text = canonical_name.clone();
    let normalized_symbol_key = normalize_retrieval_key(&canonical_name);
    let detail_text = collapse_whitespace(&format!(
        "{} {} {} {} {}",
        page_title,
        section_heading.as_deref().unwrap_or("Lead"),
        origin,
        canonical_name,
        aliases.join(" ")
    ));
    let retrieval_text = collapse_whitespace(&format!(
        "{} {} {} {} {}",
        page_title,
        symbol_kind,
        canonical_name,
        aliases.join(" "),
        summary_text
    ));
    ParsedDocsSymbol {
        symbol_name: canonical_name.clone(),
        canonical_name,
        symbol_kind: symbol_kind.to_string(),
        page_title: page_title.to_string(),
        section_heading,
        signature_text,
        summary_text,
        aliases,
        origin: origin.to_string(),
        normalized_symbol_key,
        detail_text,
        retrieval_text: retrieval_text.clone(),
        token_estimate: estimate_token_count(&retrieval_text),
    }
}

fn build_symbol_aliases(symbol_name: &str, symbol_kind: &str) -> Vec<String> {
    let mut aliases = vec![symbol_name.to_string()];
    let stripped = symbol_name
        .trim_matches('<')
        .trim_matches('>')
        .trim_start_matches('$')
        .trim_start_matches('#')
        .to_string();
    if !stripped.is_empty() && stripped != symbol_name {
        aliases.push(stripped.clone());
    }
    let decamelized = decamelize(&stripped);
    if !decamelized.is_empty() {
        aliases.push(decamelized.clone());
    }
    if symbol_kind == "tag" {
        aliases.push(format!("tag {stripped}"));
    } else if symbol_kind == "parser_function" {
        aliases.push(format!("parser function {stripped}"));
    } else if symbol_kind == "config" {
        aliases.push(format!("config {stripped}"));
    } else if symbol_kind == "hook" && !decamelized.is_empty() {
        aliases.push(format!("hook {decamelized}"));
    }
    dedupe_strings(&mut aliases);
    aliases
}

fn normalize_symbol_name(value: &str) -> String {
    let normalized = collapse_whitespace(value);
    if normalized.starts_with('<') && !normalized.ends_with('>') {
        return format!("{normalized}>");
    }
    normalized
}

fn looks_like_magic_word(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return false;
    }
    let mut has_letter = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_uppercase() {
            has_letter = true;
            continue;
        }
        if ch.is_ascii_digit() || ch == '_' || ch == '-' {
            continue;
        }
        return false;
    }
    has_letter
}

fn looks_like_extension_tag(value: &str) -> bool {
    !matches!(
        value,
        "a" | "abbr"
            | "b"
            | "blockquote"
            | "body"
            | "br"
            | "caption"
            | "code"
            | "div"
            | "em"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "i"
            | "li"
            | "math"
            | "ol"
            | "p"
            | "pre"
            | "small"
            | "source"
            | "span"
            | "strong"
            | "syntaxhighlight"
            | "table"
            | "td"
            | "th"
            | "tr"
            | "tt"
            | "u"
            | "ul"
    )
}

fn is_ignored_tag_name(value: &str) -> bool {
    matches!(
        value,
        "code" | "includeonly" | "noinclude" | "onlyinclude" | "pre" | "source" | "syntaxhighlight"
    )
}

pub(super) fn dedupe_symbols(values: &mut Vec<ParsedDocsSymbol>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| {
        let key = format!(
            "{}|{}|{}|{}",
            value.symbol_kind,
            value.symbol_name.to_ascii_lowercase(),
            value.page_title.to_ascii_lowercase(),
            value
                .section_heading
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
        );
        if seen.contains(&key) {
            return false;
        }
        seen.insert(key);
        true
    });
}
