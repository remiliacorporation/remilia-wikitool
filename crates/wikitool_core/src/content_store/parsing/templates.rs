use super::*;

pub(crate) fn summarize_template_invocations(
    invocations: Vec<ParsedTemplateInvocation>,
    limit: usize,
) -> Vec<LocalTemplateInvocation> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for invocation in invocations {
        let parameter_keys = canonical_parameter_key_list(&invocation.parameter_keys);
        let signature = format!("{}|{}", invocation.template_title, parameter_keys);
        if !seen.insert(signature) {
            continue;
        }
        out.push(LocalTemplateInvocation {
            template_title: invocation.template_title,
            parameter_keys: parse_parameter_key_list(&parameter_keys),
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

pub(crate) fn extract_template_invocations(content: &str) -> Vec<ParsedTemplateInvocation> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut stack = Vec::new();

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            stack.push(cursor + 2);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'}' && bytes[cursor + 1] == b'}' {
            if let Some(start) = stack.pop()
                && cursor >= start
            {
                let inner = &content[start..cursor];
                if let Some(invocation) = parse_template_invocation(inner) {
                    out.push(invocation);
                }
            }
            cursor += 2;
            continue;
        }
        cursor += 1;
    }

    out
}

pub(crate) fn extract_module_invocations(content: &str) -> Vec<ParsedModuleInvocation> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut cursor = 0usize;
    let mut stack = Vec::new();

    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
            stack.push(cursor + 2);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'}' && bytes[cursor + 1] == b'}' {
            if let Some(start) = stack.pop()
                && cursor >= start
            {
                let inner = &content[start..cursor];
                if let Some(invocation) = parse_module_invocation(inner) {
                    let signature = format!(
                        "{}|{}|{}",
                        invocation.module_title.to_ascii_lowercase(),
                        invocation.function_name.to_ascii_lowercase(),
                        canonical_parameter_key_list(&invocation.parameter_keys)
                    );
                    if seen.insert(signature) {
                        out.push(invocation);
                    }
                }
            }
            cursor += 2;
            continue;
        }
        cursor += 1;
    }

    out
}

pub(crate) fn parse_template_invocation(inner: &str) -> Option<ParsedTemplateInvocation> {
    let segments = split_template_segments(inner);
    let raw_name = segments.first()?.trim();
    let template_title = canonical_template_title(raw_name)?;

    let mut parameter_keys = Vec::new();
    let mut positional_index = 1usize;
    for segment in segments.iter().skip(1) {
        let value = segment.trim();
        if value.is_empty() {
            continue;
        }
        if let Some((key, _)) = split_once_top_level_equals(value) {
            let normalized = normalize_template_parameter_key(&key);
            if !normalized.is_empty() {
                parameter_keys.push(normalized);
                continue;
            }
        }
        parameter_keys.push(format!("${positional_index}"));
        positional_index += 1;
    }
    parameter_keys.sort();
    parameter_keys.dedup();

    Some(ParsedTemplateInvocation {
        template_title,
        parameter_keys,
        raw_wikitext: format!("{{{{{inner}}}}}"),
        token_estimate: estimate_tokens(inner),
    })
}

pub(crate) fn parse_module_invocation(inner: &str) -> Option<ParsedModuleInvocation> {
    let segments = split_template_segments(inner);
    let raw_name = segments.first()?.trim();
    let remainder = raw_name.strip_prefix("#invoke:")?;
    let module_name = normalize_spaces(remainder);
    if module_name.is_empty() {
        return None;
    }
    let function_name = normalize_spaces(segments.get(1).map(String::as_str).unwrap_or(""));
    if function_name.is_empty() {
        return None;
    }

    let mut parameter_keys = Vec::new();
    let mut positional_index = 1usize;
    for segment in segments.iter().skip(2) {
        let value = segment.trim();
        if value.is_empty() {
            continue;
        }
        if let Some((key, _)) = split_once_top_level_equals(value) {
            let normalized = normalize_template_parameter_key(&key);
            if !normalized.is_empty() {
                parameter_keys.push(normalized);
                continue;
            }
        }
        parameter_keys.push(format!("${positional_index}"));
        positional_index += 1;
    }
    parameter_keys.sort();
    parameter_keys.dedup();

    Some(ParsedModuleInvocation {
        module_title: format!("Module:{module_name}"),
        function_name,
        parameter_keys,
        raw_wikitext: format!("{{{{{inner}}}}}"),
        token_estimate: estimate_tokens(inner),
    })
}

pub(crate) fn split_template_segments(inner: &str) -> Vec<String> {
    let chars: Vec<char> = inner.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut cursor = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;

    while cursor < chars.len() {
        let current_char = chars[cursor];
        let next_char = chars.get(cursor + 1).copied();
        if current_char == '{' && next_char == Some('{') {
            template_depth += 1;
            current.push('{');
            current.push('{');
            cursor += 2;
            continue;
        }
        if current_char == '}' && next_char == Some('}') {
            template_depth = template_depth.saturating_sub(1);
            current.push('}');
            current.push('}');
            cursor += 2;
            continue;
        }
        if current_char == '[' && next_char == Some('[') {
            link_depth += 1;
            current.push('[');
            current.push('[');
            cursor += 2;
            continue;
        }
        if current_char == ']' && next_char == Some(']') {
            link_depth = link_depth.saturating_sub(1);
            current.push(']');
            current.push(']');
            cursor += 2;
            continue;
        }
        if current_char == '|' && template_depth == 0 && link_depth == 0 {
            out.push(current.trim().to_string());
            current.clear();
            cursor += 1;
            continue;
        }
        current.push(current_char);
        cursor += 1;
    }

    out.push(current.trim().to_string());
    out
}

pub(crate) fn split_once_top_level_equals(value: &str) -> Option<(String, String)> {
    let chars: Vec<char> = value.chars().collect();
    let mut cursor = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;
    while cursor < chars.len() {
        let current_char = chars[cursor];
        let next_char = chars.get(cursor + 1).copied();
        if current_char == '{' && next_char == Some('{') {
            template_depth += 1;
            cursor += 2;
            continue;
        }
        if current_char == '}' && next_char == Some('}') {
            template_depth = template_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if current_char == '[' && next_char == Some('[') {
            link_depth += 1;
            cursor += 2;
            continue;
        }
        if current_char == ']' && next_char == Some(']') {
            link_depth = link_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if current_char == '=' && template_depth == 0 && link_depth == 0 {
            let key = chars[..cursor].iter().collect::<String>();
            let value = chars[cursor + 1..].iter().collect::<String>();
            return Some((key, value));
        }
        cursor += 1;
    }
    None
}

pub(crate) fn canonical_template_title(raw: &str) -> Option<String> {
    let mut name = normalize_spaces(&raw.replace('_', " "));
    while let Some(stripped) = name.strip_prefix(':') {
        name = stripped.trim_start().to_string();
    }
    if name.is_empty() {
        return None;
    }
    if name.starts_with('#')
        || name.starts_with('!')
        || name.contains('{')
        || name.contains('}')
        || name.contains('[')
        || name.contains(']')
    {
        return None;
    }

    if let Some((prefix, rest)) = name.split_once(':') {
        if !prefix.eq_ignore_ascii_case("Template") {
            return None;
        }
        let body = normalize_spaces(rest);
        if body.is_empty() {
            return None;
        }
        return Some(format!("Template:{body}"));
    }
    Some(format!("Template:{name}"))
}

pub(crate) fn normalize_template_parameter_key(value: &str) -> String {
    normalize_spaces(&value.replace('_', " ")).to_ascii_lowercase()
}
