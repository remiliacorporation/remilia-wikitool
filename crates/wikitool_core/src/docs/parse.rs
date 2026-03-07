use std::collections::BTreeSet;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsPage {
    pub page_title: String,
    pub page_namespace: String,
    pub page_kind: String,
    pub local_path: String,
    pub source_revision_id: Option<i64>,
    pub source_parent_revision_id: Option<i64>,
    pub source_timestamp: Option<String>,
    pub summary_text: String,
    pub lead_text: String,
    pub headings_text: String,
    pub alias_titles: Vec<String>,
    pub link_titles: Vec<String>,
    pub template_titles: Vec<String>,
    pub symbol_names: Vec<String>,
    pub normalized_content: String,
    pub semantic_text: String,
    pub content: String,
    pub token_estimate: usize,
    pub sections: Vec<ParsedDocsSection>,
    pub symbols: Vec<ParsedDocsSymbol>,
    pub examples: Vec<ParsedDocsExample>,
    pub links: Vec<ParsedDocsLink>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsSection {
    pub section_index: usize,
    pub heading: String,
    pub section_heading: Option<String>,
    pub heading_path: String,
    pub section_level: u8,
    pub section_kind: String,
    pub summary_text: String,
    pub section_text: String,
    pub semantic_text: String,
    pub symbol_names: Vec<String>,
    pub link_titles: Vec<String>,
    pub token_estimate: usize,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsSymbol {
    pub symbol_name: String,
    pub canonical_name: String,
    pub symbol_kind: String,
    pub page_title: String,
    pub section_heading: Option<String>,
    pub signature_text: String,
    pub summary_text: String,
    pub aliases: Vec<String>,
    pub origin: String,
    pub normalized_symbol_key: String,
    pub detail_text: String,
    pub retrieval_text: String,
    pub token_estimate: usize,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsExample {
    pub example_index: usize,
    pub page_title: String,
    pub section_heading: Option<String>,
    pub example_kind: String,
    pub language: Option<String>,
    pub language_hint: String,
    pub summary_text: String,
    pub example_text: String,
    pub retrieval_text: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsLink {
    pub target_title: String,
    pub relation_kind: String,
    pub display_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DocsPageParseInput {
    pub page_title: String,
    pub local_path: String,
    pub content: String,
    pub source_revision_id: Option<i64>,
    pub source_parent_revision_id: Option<i64>,
    pub source_timestamp: Option<String>,
}

#[derive(Debug, Clone)]
struct RawSection {
    heading: String,
    heading_path: String,
    level: u8,
    kind: String,
    text: String,
}

pub(crate) fn parse_docs_page(input: DocsPageParseInput) -> ParsedDocsPage {
    let page_kind = classify_docs_page_kind(&input.page_title);
    let page_namespace = namespace_label(&input.page_title);
    let raw_sections = split_into_sections(&input.content);
    let link_titles = extract_link_titles(&input.content);
    let template_titles = extract_template_titles(&input.content);
    let mut symbols = extract_title_symbols(&input.page_title, &page_kind);
    symbols.extend(extract_content_symbols(
        &input.page_title,
        &page_kind,
        &input.content,
        &raw_sections,
    ));
    dedupe_symbols(&mut symbols);

    let mut sections = Vec::with_capacity(raw_sections.len());
    for (index, raw_section) in raw_sections.iter().enumerate() {
        let section_link_titles = extract_link_titles(&raw_section.text);
        let section_symbol_names = symbols
            .iter()
            .filter(|symbol| {
                symbol
                    .section_heading
                    .as_deref()
                    .is_some_and(|heading| heading == raw_section.heading)
            })
            .map(|symbol| symbol.symbol_name.clone())
            .collect::<Vec<_>>();
        let section_heading = if raw_section.kind == "lead" {
            None
        } else {
            Some(raw_section.heading.clone())
        };
        let summary_text = make_summary_text(&raw_section.text, 260);
        let semantic_text = build_section_semantic_text(
            &input.page_title,
            raw_section,
            &summary_text,
            &section_symbol_names,
            &section_link_titles,
        );
        sections.push(ParsedDocsSection {
            section_index: index,
            heading: raw_section.heading.clone(),
            section_heading,
            heading_path: raw_section.heading_path.clone(),
            section_level: raw_section.level,
            section_kind: raw_section.kind.clone(),
            summary_text,
            section_text: raw_section.text.clone(),
            semantic_text,
            symbol_names: section_symbol_names,
            link_titles: section_link_titles,
            token_estimate: estimate_token_count(&raw_section.text),
        });
    }

    let mut examples = Vec::new();
    for raw_section in &raw_sections {
        examples.extend(extract_examples_for_section(&input.page_title, raw_section));
    }
    for (index, example) in examples.iter_mut().enumerate() {
        example.example_index = index;
    }

    let mut alias_titles = build_page_aliases(&input.page_title);
    for symbol in &symbols {
        alias_titles.extend(symbol.aliases.clone());
    }
    dedupe_strings(&mut alias_titles);

    let symbol_names = symbols
        .iter()
        .map(|symbol| symbol.symbol_name.clone())
        .collect::<Vec<_>>();
    let lead_text = raw_sections
        .first()
        .map(|section| section.text.clone())
        .unwrap_or_default();
    let headings_text = raw_sections
        .iter()
        .skip(1)
        .map(|section| section.heading_path.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    let normalized_content = collapse_whitespace(&input.content);
    let summary_text = sections
        .iter()
        .find(|section| !section.summary_text.is_empty())
        .map(|section| section.summary_text.clone())
        .unwrap_or_else(|| make_summary_text(&input.content, 260));
    let semantic_text = build_semantic_text(
        &input.page_title,
        &page_kind,
        &summary_text,
        &headings_text,
        &alias_titles,
        &symbol_names,
        &link_titles,
        &sections,
        &examples,
    );
    let links = build_page_links(&link_titles, &template_titles);
    let token_estimate = estimate_token_count(&lead_text);

    ParsedDocsPage {
        page_title: input.page_title,
        page_namespace,
        page_kind,
        local_path: input.local_path,
        source_revision_id: input.source_revision_id,
        source_parent_revision_id: input.source_parent_revision_id,
        source_timestamp: input.source_timestamp,
        summary_text,
        lead_text,
        headings_text,
        alias_titles,
        link_titles,
        template_titles,
        symbol_names,
        normalized_content,
        semantic_text,
        content: input.content,
        token_estimate,
        sections,
        symbols,
        examples,
        links,
    }
}

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

fn split_into_sections(content: &str) -> Vec<RawSection> {
    let mut sections = Vec::new();
    let mut heading_stack: Vec<(u8, String)> = Vec::new();
    let mut current_heading = "Lead".to_string();
    let mut current_level = 1u8;
    let mut current_kind = "lead".to_string();
    let mut current_lines = Vec::new();

    let flush_section = |sections: &mut Vec<RawSection>,
                         heading: &str,
                         level: u8,
                         kind: &str,
                         lines: &mut Vec<String>,
                         heading_stack: &[(u8, String)]| {
        let text = lines.join("\n").trim().to_string();
        if text.is_empty() && kind != "lead" {
            lines.clear();
            return;
        }
        let heading_path = if kind == "lead" {
            "Lead".to_string()
        } else {
            heading_stack
                .iter()
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>()
                .join(" > ")
        };
        sections.push(RawSection {
            heading: heading.to_string(),
            heading_path,
            level,
            kind: kind.to_string(),
            text,
        });
        lines.clear();
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((level, heading)) = parse_heading_line(trimmed) {
            flush_section(
                &mut sections,
                &current_heading,
                current_level,
                &current_kind,
                &mut current_lines,
                &heading_stack,
            );
            while heading_stack
                .last()
                .is_some_and(|(existing_level, _)| *existing_level >= level)
            {
                heading_stack.pop();
            }
            heading_stack.push((level, heading.clone()));
            current_heading = heading;
            current_level = level;
            current_kind = "section".to_string();
        } else {
            current_lines.push(line.to_string());
        }
    }

    flush_section(
        &mut sections,
        &current_heading,
        current_level,
        &current_kind,
        &mut current_lines,
        &heading_stack,
    );

    if sections.is_empty() {
        sections.push(RawSection {
            heading: "Lead".to_string(),
            heading_path: "Lead".to_string(),
            level: 1,
            kind: "lead".to_string(),
            text: content.trim().to_string(),
        });
    }
    sections
}

fn parse_heading_line(value: &str) -> Option<(u8, String)> {
    if value.len() < 4 || !value.starts_with('=') || !value.ends_with('=') {
        return None;
    }

    let leading = value.chars().take_while(|ch| *ch == '=').count();
    let trailing = value.chars().rev().take_while(|ch| *ch == '=').count();
    if leading != trailing || !(2..=6).contains(&leading) {
        return None;
    }

    let inner = value[leading..value.len() - trailing].trim();
    if inner.is_empty() || inner.contains('=') {
        return None;
    }

    Some((leading as u8, normalize_title(inner)))
}

fn strip_summary_noise(value: &str) -> String {
    let without_blocks = strip_tagged_block(value, "syntaxhighlight");
    let without_blocks = strip_tagged_block(&without_blocks, "source");
    let without_blocks = strip_tagged_block(&without_blocks, "pre");
    let without_blocks = strip_tagged_block(&without_blocks, "code");
    let mut output = String::with_capacity(without_blocks.len());
    let bytes = without_blocks.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"[[")
            && let Some(end) = find_delimited(bytes, cursor + 2, b"]]")
        {
            let body = &without_blocks[cursor + 2..end];
            let display = body
                .split('|')
                .next_back()
                .unwrap_or(body)
                .split('#')
                .next()
                .unwrap_or(body);
            output.push_str(display.trim_start_matches(':'));
            cursor = end + 2;
            continue;
        }
        if bytes[cursor..].starts_with(b"{{")
            && let Some(end) = find_balanced_braces(bytes, cursor)
        {
            cursor = end;
            continue;
        }
        if bytes[cursor] == b'<'
            && let Some(end) = find_tag_end(bytes, cursor)
        {
            cursor = end + 1;
            continue;
        }
        output.push(bytes[cursor] as char);
        cursor += 1;
    }

    collapse_whitespace(&output)
}

fn build_page_aliases(page_title: &str) -> Vec<String> {
    let mut aliases = vec![page_title.to_string()];
    if let Some((_, tail)) = page_title.split_once(':') {
        aliases.push(normalize_title(tail));
        let decamelized = decamelize(tail);
        if !decamelized.is_empty() {
            aliases.push(decamelized);
        }
    }
    dedupe_strings(&mut aliases);
    aliases
}

#[allow(clippy::too_many_arguments)]
fn build_semantic_text(
    page_title: &str,
    page_kind: &str,
    summary_text: &str,
    headings_text: &str,
    alias_titles: &[String],
    symbol_names: &[String],
    link_titles: &[String],
    sections: &[ParsedDocsSection],
    examples: &[ParsedDocsExample],
) -> String {
    let mut terms = vec![
        page_title.to_string(),
        page_kind.to_string(),
        summary_text.to_string(),
        headings_text.to_string(),
    ];
    terms.extend(alias_titles.iter().cloned());
    terms.extend(symbol_names.iter().cloned());
    terms.extend(link_titles.iter().cloned());
    terms.extend(sections.iter().map(|section| section.summary_text.clone()));
    terms.extend(examples.iter().map(|example| example.summary_text.clone()));
    collapse_whitespace(&terms.join(" | "))
}

fn build_section_semantic_text(
    page_title: &str,
    section: &RawSection,
    summary_text: &str,
    symbol_names: &[String],
    link_titles: &[String],
) -> String {
    let mut terms = vec![
        page_title.to_string(),
        section.kind.clone(),
        section.heading.clone(),
        section.heading_path.clone(),
        summary_text.to_string(),
    ];
    terms.extend(symbol_names.iter().cloned());
    terms.extend(link_titles.iter().cloned());
    collapse_whitespace(&terms.join(" | "))
}

fn build_page_links(link_titles: &[String], template_titles: &[String]) -> Vec<ParsedDocsLink> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for title in link_titles {
        let key = format!("wikilink|{}", title.to_ascii_lowercase());
        if seen.insert(key) {
            out.push(ParsedDocsLink {
                target_title: title.clone(),
                relation_kind: "wikilink".to_string(),
                display_text: title.clone(),
            });
        }
    }
    for title in template_titles {
        let key = format!("template|{}", title.to_ascii_lowercase());
        if seen.insert(key) {
            out.push(ParsedDocsLink {
                target_title: title.clone(),
                relation_kind: "template".to_string(),
                display_text: title.clone(),
            });
        }
    }
    out
}

fn extract_examples_for_section(page_title: &str, section: &RawSection) -> Vec<ParsedDocsExample> {
    let mut examples = Vec::new();
    for tag_name in ["syntaxhighlight", "source", "pre", "code"] {
        for (language, body) in extract_tagged_examples(&section.text, tag_name) {
            let example_text = collapse_whitespace(&body);
            if example_text.len() < 4 {
                continue;
            }
            let heading = if section.kind == "lead" {
                None
            } else {
                Some(section.heading.clone())
            };
            let summary_text = if section.kind == "lead" {
                format!("Example from {page_title}")
            } else {
                format!("Example from {} > {}", page_title, section.heading_path)
            };
            let retrieval_text = collapse_whitespace(&format!(
                "{} {} {} {}",
                page_title,
                heading.as_deref().unwrap_or("Lead"),
                language.as_deref().unwrap_or(""),
                example_text
            ));
            examples.push(ParsedDocsExample {
                example_index: 0,
                page_title: page_title.to_string(),
                section_heading: heading,
                example_kind: tag_name.to_string(),
                language_hint: language.clone().unwrap_or_default(),
                language,
                summary_text,
                example_text: body.trim().to_string(),
                retrieval_text,
                token_estimate: estimate_token_count(&body),
            });
        }
    }
    examples
}

fn extract_content_symbols(
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

fn extract_title_symbols(page_title: &str, page_kind: &str) -> Vec<ParsedDocsSymbol> {
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

fn extract_link_titles(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    let mut titles = Vec::new();
    while cursor + 1 < bytes.len() {
        if bytes[cursor..].starts_with(b"[[")
            && let Some(end) = find_delimited(bytes, cursor + 2, b"]]")
        {
            let body = &content[cursor + 2..end];
            let title = body
                .split('|')
                .next()
                .unwrap_or(body)
                .split('#')
                .next()
                .unwrap_or(body)
                .trim()
                .trim_start_matches(':');
            let normalized = normalize_title(title);
            if !normalized.is_empty()
                && !normalized.starts_with("http://")
                && !normalized.starts_with("https://")
            {
                titles.push(normalized);
            }
            cursor = end + 2;
            continue;
        }
        cursor += 1;
    }
    dedupe_strings(&mut titles);
    titles
}

fn extract_template_titles(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut cursor = 0usize;
    let mut titles = Vec::new();
    while cursor + 1 < bytes.len() {
        if bytes[cursor..].starts_with(b"{{") {
            let start = cursor + 2;
            let mut end = start;
            while end < bytes.len() {
                let ch = bytes[end] as char;
                if matches!(ch, '|' | '}' | '\n' | '\r') {
                    break;
                }
                end += 1;
            }
            if end > start {
                let name = normalize_title(content[start..end].trim());
                if !name.is_empty() && !name.starts_with('#') {
                    titles.push(name);
                }
            }
            cursor = end;
            continue;
        }
        cursor += 1;
    }
    dedupe_strings(&mut titles);
    titles
}

fn extract_tagged_examples(content: &str, tag_name: &str) -> Vec<(Option<String>, String)> {
    let mut out = Vec::new();
    let lower = content.to_ascii_lowercase();
    let open_pattern = format!("<{tag_name}");
    let close_pattern = format!("</{tag_name}>");
    let bytes = content.as_bytes();
    let lower_bytes = lower.as_bytes();
    let open_bytes = open_pattern.as_bytes();
    let close_bytes = close_pattern.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if lower_bytes[cursor..].starts_with(open_bytes) {
            let Some(tag_end) = find_tag_end(bytes, cursor) else {
                break;
            };
            let attrs = &content[cursor + 1 + tag_name.len()..tag_end];
            let body_start = tag_end + 1;
            let Some(close_start) = find_case_insensitive(lower_bytes, body_start, close_bytes)
            else {
                break;
            };
            let body = content[body_start..close_start].to_string();
            let language = extract_attribute_value(attrs, "lang")
                .or_else(|| extract_attribute_value(attrs, "language"));
            out.push((language, body));
            cursor = close_start + close_pattern.len();
            continue;
        }
        cursor += 1;
    }

    out
}

fn extract_attribute_value(attrs: &str, key: &str) -> Option<String> {
    let chars = attrs.chars().collect::<Vec<_>>();
    let mut cursor = 0usize;
    while cursor < chars.len() {
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        let start = cursor;
        while cursor < chars.len()
            && (chars[cursor].is_ascii_alphanumeric()
                || chars[cursor] == '-'
                || chars[cursor] == '_')
        {
            cursor += 1;
        }
        if start == cursor {
            cursor += 1;
            continue;
        }
        let name = chars[start..cursor].iter().collect::<String>();
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor >= chars.len() || chars[cursor] != '=' {
            continue;
        }
        cursor += 1;
        while cursor < chars.len() && chars[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor >= chars.len() {
            break;
        }
        let quote = if chars[cursor] == '"' || chars[cursor] == '\'' {
            let quote = chars[cursor];
            cursor += 1;
            Some(quote)
        } else {
            None
        };
        let value_start = cursor;
        while cursor < chars.len() {
            let ch = chars[cursor];
            if let Some(quote_char) = quote {
                if ch == quote_char {
                    break;
                }
            } else if ch.is_whitespace() || ch == '>' || ch == '/' {
                break;
            }
            cursor += 1;
        }
        let value = chars[value_start..cursor].iter().collect::<String>();
        if quote.is_some() && cursor < chars.len() {
            cursor += 1;
        }
        if name.eq_ignore_ascii_case(key) {
            let normalized = collapse_whitespace(&value);
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }
    None
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

fn decamelize(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 8);
    let mut previous_lower_or_digit = false;
    for ch in value.chars() {
        if ch.is_ascii_uppercase() && previous_lower_or_digit {
            output.push(' ');
        } else if (ch == '_' || ch == '-' || ch == '/') && !output.ends_with(' ') {
            output.push(' ');
            previous_lower_or_digit = false;
            continue;
        }
        output.push(ch);
        previous_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
    }
    collapse_whitespace(&output)
}

fn strip_tagged_block(value: &str, tag_name: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let bytes = value.as_bytes();
    let lower_bytes = lower.as_bytes();
    let open_pattern = format!("<{tag_name}").into_bytes();
    let close_pattern = format!("</{tag_name}>").into_bytes();
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if lower_bytes[cursor..].starts_with(&open_pattern) {
            let Some(tag_end) = find_tag_end(bytes, cursor) else {
                break;
            };
            if let Some(close_start) =
                find_case_insensitive(lower_bytes, tag_end + 1, &close_pattern)
            {
                cursor = close_start + close_pattern.len();
                output.push(' ');
                continue;
            }
        }
        output.push(bytes[cursor] as char);
        cursor += 1;
    }

    collapse_whitespace(&output)
}

fn find_balanced_braces(bytes: &[u8], start: usize) -> Option<usize> {
    if !bytes.get(start..)?.starts_with(b"{{") {
        return None;
    }
    let mut depth = 0usize;
    let mut cursor = start;
    while cursor + 1 < bytes.len() {
        if bytes[cursor..].starts_with(b"{{") {
            depth += 1;
            cursor += 2;
            continue;
        }
        if bytes[cursor..].starts_with(b"}}") {
            depth = depth.saturating_sub(1);
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

fn find_delimited(bytes: &[u8], start: usize, pattern: &[u8]) -> Option<usize> {
    let mut cursor = start;
    while cursor + pattern.len() <= bytes.len() {
        if bytes[cursor..].starts_with(pattern) {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn find_tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut quote: Option<u8> = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(quote_char) = quote {
            if byte == quote_char {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn find_case_insensitive(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    let mut cursor = start;
    while cursor + needle.len() <= haystack.len() {
        if haystack[cursor..].starts_with(needle) {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| {
        let key = value.to_ascii_lowercase();
        if seen.contains(&key) {
            return false;
        }
        seen.insert(key);
        true
    });
}

fn dedupe_symbols(values: &mut Vec<ParsedDocsSymbol>) {
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

#[cfg(test)]
mod tests {
    use super::{
        DocsPageParseInput, classify_docs_page_kind, is_translation_variant, parse_docs_page,
    };

    #[test]
    fn parse_docs_page_extracts_symbols_sections_and_examples() {
        let parsed = parse_docs_page(DocsPageParseInput {
            page_title: "Manual:Hooks/PageContentSave".to_string(),
            local_path: "docs/mediawiki/mw-1.44/hooks/Manual_Hooks_PageContentSave.wiki"
                .to_string(),
            content: "Lead intro.\n== Parameters ==\n<syntaxhighlight lang=\"php\">$hookContainer->run( 'PageContentSave' );</syntaxhighlight>\n== Related ==\nSee [[API:Edit]] and {{#if:foo|bar}}.".to_string(),
            source_revision_id: Some(1),
            source_parent_revision_id: Some(0),
            source_timestamp: Some("2026-01-01T00:00:00Z".to_string()),
        });

        assert_eq!(parsed.page_kind, "hook_page");
        assert!(
            parsed
                .sections
                .iter()
                .any(|section| section.heading == "Parameters")
        );
        assert!(
            parsed
                .symbols
                .iter()
                .any(|symbol| symbol.symbol_name == "PageContentSave")
        );
        assert!(
            parsed
                .symbols
                .iter()
                .any(|symbol| symbol.symbol_name == "#if")
        );
        assert!(
            parsed
                .examples
                .iter()
                .any(|example| example.language.as_deref() == Some("php"))
        );
        assert!(parsed.link_titles.iter().any(|title| title == "API:Edit"));
    }

    #[test]
    fn page_kind_classification_covers_core_mediawiki_surfaces() {
        assert_eq!(
            classify_docs_page_kind("Manual:$wgParserEnableLegacyMediaDOM"),
            "config_page"
        );
        assert_eq!(classify_docs_page_kind("Help:Tags"), "tag_reference");
        assert_eq!(
            classify_docs_page_kind("Extension:Scribunto/Lua reference manual"),
            "lua_reference"
        );
    }

    #[test]
    fn translation_variant_detection_skips_language_subpages_only() {
        assert!(is_translation_variant("Manual:Hooks/PageSave/en"));
        assert!(is_translation_variant("API:Edit/pt-br"));
        assert!(!is_translation_variant("API:Edit/Sample code 1"));
        assert!(!is_translation_variant(
            "Extension:Scribunto/Lua reference manual"
        ));
    }

    #[test]
    fn parse_docs_page_extracts_inline_config_symbols_without_promoting_local_php_vars() {
        let parsed = parse_docs_page(DocsPageParseInput {
            page_title: "Extension:TestExtension".to_string(),
            local_path: "docs/extensions/TestExtension/Extension_TestExtension.wiki".to_string(),
            content: "Configuration: $wgTestExtensionEnable = true.\n== Hooks ==\nHook parameters include $parser and $text.".to_string(),
            source_revision_id: None,
            source_parent_revision_id: None,
            source_timestamp: None,
        });

        assert!(
            parsed
                .symbols
                .iter()
                .any(|symbol| symbol.symbol_name == "$wgTestExtensionEnable")
        );
        assert!(
            !parsed
                .symbols
                .iter()
                .any(|symbol| symbol.symbol_name == "$parser")
        );
    }
}
