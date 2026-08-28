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
    extract_template_bodies(content)
        .into_iter()
        .filter_map(parse_template_invocation)
        .collect()
}

pub(crate) fn extract_transclusion_heads(content: &str) -> Vec<String> {
    let mut heads = extract_template_bodies(content)
        .into_iter()
        .filter_map(|inner| {
            split_template_segments(inner)
                .into_iter()
                .next()
                .map(|head| normalize_spaces(head.trim()))
                .filter(|head| !head.is_empty())
        })
        .collect::<Vec<_>>();
    heads.sort();
    heads.dedup();
    heads
}

fn extract_template_bodies(content: &str) -> Vec<&str> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut stack = Vec::new();

    while cursor + 1 < bytes.len() {
        if let Some(end) = skip_template_literal_region(content, cursor) {
            cursor = end;
            continue;
        }
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
                out.push(inner);
            }
            cursor += 2;
            continue;
        }
        cursor += 1;
    }

    out
}

fn skip_template_literal_region(content: &str, cursor: usize) -> Option<usize> {
    let remaining = content.get(cursor..)?;
    if remaining.starts_with("<!--") {
        return remaining
            .get(4..)?
            .find("-->")
            .map(|offset| cursor + 4 + offset + 3)
            .or(Some(content.len()));
    }
    if !remaining.starts_with('<') {
        return None;
    }

    for tag in ["nowiki", "pre", "syntaxhighlight", "source", "templatedata"] {
        if !starts_with_open_tag_ascii_case_insensitive(content, cursor, tag) {
            continue;
        }
        let open_end = find_template_tag_end(content, cursor + 1).unwrap_or(content.len());
        if content[cursor..open_end]
            .trim_end_matches('>')
            .trim_end()
            .ends_with('/')
        {
            return Some(open_end);
        }
        let lower = content.to_ascii_lowercase();
        let close = format!("</{tag}>");
        return lower[open_end..]
            .find(&close)
            .map(|offset| open_end + offset + close.len())
            .or(Some(content.len()));
    }
    None
}

fn starts_with_open_tag_ascii_case_insensitive(content: &str, cursor: usize, tag: &str) -> bool {
    let Some(candidate) = content.get(cursor..) else {
        return false;
    };
    let prefix = format!("<{tag}");
    let Some(actual) = candidate.get(..prefix.len()) else {
        return false;
    };
    if !actual.eq_ignore_ascii_case(&prefix) {
        return false;
    }
    candidate
        .as_bytes()
        .get(prefix.len())
        .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))
}

fn find_template_tag_end(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut cursor = start;
    let mut quote = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active) = quote {
            if byte == active {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(cursor + 1);
        }
        cursor += 1;
    }
    None
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
    let mut out = Vec::new();
    let mut current = String::new();
    let mut cursor = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;

    while cursor < inner.len() {
        if let Some(end) = skip_template_literal_region(inner, cursor) {
            current.push_str(&inner[cursor..end]);
            cursor = end;
            continue;
        }
        let remaining = &inner.as_bytes()[cursor..];
        if remaining.starts_with(b"{{") {
            template_depth += 1;
            current.push_str("{{");
            cursor += 2;
            continue;
        }
        if remaining.starts_with(b"}}") {
            template_depth = template_depth.saturating_sub(1);
            current.push_str("}}");
            cursor += 2;
            continue;
        }
        if remaining.starts_with(b"[[") {
            link_depth += 1;
            current.push_str("[[");
            cursor += 2;
            continue;
        }
        if remaining.starts_with(b"]]") {
            link_depth = link_depth.saturating_sub(1);
            current.push_str("]]");
            cursor += 2;
            continue;
        }
        if remaining[0] == b'|' && template_depth == 0 && link_depth == 0 {
            out.push(current.trim().to_string());
            current.clear();
            cursor += 1;
            continue;
        }
        let current_char = inner[cursor..]
            .chars()
            .next()
            .expect("cursor is inside template invocation");
        current.push(current_char);
        cursor += current_char.len_utf8();
    }

    out.push(current.trim().to_string());
    out
}

pub(crate) fn split_once_top_level_equals(value: &str) -> Option<(String, String)> {
    let mut cursor = 0usize;
    let mut template_depth = 0usize;
    let mut link_depth = 0usize;
    while cursor < value.len() {
        if let Some(end) = skip_template_literal_region(value, cursor) {
            cursor = end;
            continue;
        }
        let remaining = &value.as_bytes()[cursor..];
        if remaining.starts_with(b"{{") {
            template_depth += 1;
            cursor += 2;
            continue;
        }
        if remaining.starts_with(b"}}") {
            template_depth = template_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if remaining.starts_with(b"[[") {
            link_depth += 1;
            cursor += 2;
            continue;
        }
        if remaining.starts_with(b"]]") {
            link_depth = link_depth.saturating_sub(1);
            cursor += 2;
            continue;
        }
        if remaining[0] == b'=' && template_depth == 0 && link_depth == 0 {
            return Some((value[..cursor].to_string(), value[cursor + 1..].to_string()));
        }
        let current_char = value[cursor..]
            .chars()
            .next()
            .expect("cursor is inside template parameter");
        cursor += current_char.len_utf8();
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
    value.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{extract_template_invocations, extract_transclusion_heads};

    #[test]
    fn invocation_extraction_skips_comments_and_literal_regions() {
        let content = r#"
{{Runtime|name=value|literal=<nowiki>{{Nested literal|...}}</nowiki>}}
<!-- {{Commented}} -->
<pre>{{Preformatted}}</pre>
<syntaxhighlight lang="wikitext">{{Highlighted}}</syntaxhighlight>
<templatedata>{"description":"{{Metadata literal}}"}</templatedata>
"#;
        let invocations = extract_template_invocations(content);
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].template_title, "Template:Runtime");
        assert_eq!(invocations[0].parameter_keys, vec!["literal", "name"]);
    }

    #[test]
    fn named_parameter_identity_preserves_case_underscores_and_internal_space() {
        let invocations = extract_template_invocations(
            "{{Example| source1_sha256 =one|Reason=two|reason=three|two words=four}}",
        );
        assert_eq!(
            invocations[0].parameter_keys,
            vec!["Reason", "reason", "source1_sha256", "two words"]
        );
    }

    #[test]
    fn transclusion_heads_preserve_runtime_primitive_syntax() {
        let heads = extract_transclusion_heads(
            "{{#if:{{NAMESPACE}}|{{lc: VALUE}}|{{Real template}}}}<nowiki>{{#ignored:x}}</nowiki>",
        );

        assert_eq!(
            heads,
            vec![
                "#if:{{NAMESPACE}}".to_string(),
                "NAMESPACE".to_string(),
                "Real template".to_string(),
                "lc: VALUE".to_string(),
            ]
        );
    }
}
