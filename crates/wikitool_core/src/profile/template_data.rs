use std::collections::BTreeSet;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::knowledge::templates::normalize_template_lookup_title;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateDataParameter {
    pub name: String,
    pub aliases: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param_type: Option<String>,
    pub required: bool,
    pub suggested: bool,
    pub deprecated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateDataRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    pub parameters: Vec<TemplateDataParameter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalTemplateExample {
    pub source_title: String,
    pub source_relative_path: String,
    pub invocation_text: String,
    pub parameter_keys: Vec<String>,
}

pub(crate) fn extract_template_data(content: &str) -> Result<Option<TemplateDataRecord>> {
    let Some(body) = extract_first_tag_body(content, "templatedata") else {
        return Ok(None);
    };
    let value: Value =
        serde_json::from_str(body).context("failed to decode <templatedata> JSON payload")?;
    let Some(object) = value.as_object() else {
        return Ok(None);
    };

    let description = string_field(object.get("description"));
    let format = string_field(object.get("format"));
    let mut parameters = Vec::new();
    if let Some(params) = object.get("params").and_then(Value::as_object) {
        for (name, param) in params {
            let aliases = param
                .as_object()
                .and_then(|value| value.get("aliases"))
                .map(value_string_list)
                .unwrap_or_default();
            let label = param
                .as_object()
                .and_then(|value| string_field(value.get("label")));
            let description = param
                .as_object()
                .and_then(|value| string_field(value.get("description")));
            let param_type = param
                .as_object()
                .and_then(|value| string_field(value.get("type")));
            let required = param
                .as_object()
                .and_then(|value| value.get("required"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let suggested = param
                .as_object()
                .and_then(|value| value.get("suggested"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let deprecated = param
                .as_object()
                .and_then(|value| value.get("deprecated"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let example = param
                .as_object()
                .and_then(|value| string_field(value.get("example")));
            let default_value = param
                .as_object()
                .and_then(|value| string_field(value.get("default")));

            parameters.push(TemplateDataParameter {
                name: collapse_whitespace(name),
                aliases,
                label,
                description,
                param_type,
                required,
                suggested,
                deprecated,
                example,
                default_value,
            });
        }
    }

    Ok(Some(TemplateDataRecord {
        description,
        format,
        parameters,
    }))
}

pub(crate) fn extract_source_parameters(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut index = 0usize;
    while index + 2 < bytes.len() {
        if bytes[index..].starts_with(b"{{{") {
            let mut end = index + 3;
            while end < bytes.len() {
                let byte = bytes[end];
                if byte == b'|' || byte == b'}' || byte == b'\n' || byte == b'\r' {
                    break;
                }
                end += 1;
            }
            let name = collapse_whitespace(&content[index + 3..end]);
            if !name.is_empty() {
                let key = name.to_ascii_lowercase();
                if seen.insert(key) {
                    out.push(name);
                }
            }
            index += 3;
            continue;
        }
        index += 1;
    }
    out
}

pub(crate) fn extract_module_references(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    for module in collect_invoke_modules(content) {
        let normalized = normalize_module_title(&module);
        if seen.insert(normalized.to_ascii_lowercase()) {
            out.push(normalized);
        }
    }
    for module in collect_inline_module_titles(content) {
        let normalized = normalize_module_title(&module);
        if seen.insert(normalized.to_ascii_lowercase()) {
            out.push(normalized);
        }
    }

    out
}

pub(crate) fn extract_template_examples(
    content: &str,
    template_title: &str,
    source_title: &str,
    source_relative_path: &str,
    limit: usize,
) -> Vec<LocalTemplateExample> {
    if limit == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for block in extract_tag_bodies(content, "syntaxhighlight", limit.saturating_mul(2)) {
        if let Some(invocation) = find_template_invocation(block, template_title) {
            out.push(LocalTemplateExample {
                source_title: source_title.to_string(),
                source_relative_path: source_relative_path.to_string(),
                parameter_keys: extract_invocation_parameter_keys(&invocation),
                invocation_text: collapse_whitespace(&invocation),
            });
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

pub(crate) fn extract_summary_text(content: &str) -> Option<String> {
    let scope = extract_first_tag_body(content, "noinclude").unwrap_or(content);
    for line in scope.lines() {
        let trimmed = collapse_whitespace(line);
        if trimmed.is_empty()
            || trimmed.starts_with("{{")
            || trimmed.starts_with("__")
            || trimmed.starts_with("[[")
            || trimmed.starts_with('<')
            || trimmed.starts_with("==")
            || trimmed.starts_with("{|")
            || trimmed.starts_with("|-")
            || trimmed.starts_with('|')
            || trimmed.starts_with('!')
            || trimmed.starts_with('*')
            || trimmed.starts_with('#')
        {
            continue;
        }
        return Some(trimmed);
    }
    None
}

fn string_field(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => {
            let trimmed = collapse_whitespace(value);
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        _ => None,
    }
}

fn value_string_list(value: &Value) -> Vec<String> {
    match value {
        Value::Array(values) => values
            .iter()
            .filter_map(|item| string_field(Some(item)))
            .collect(),
        Value::String(value) => {
            let trimmed = collapse_whitespace(value);
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed]
            }
        }
        _ => Vec::new(),
    }
}

fn collect_invoke_modules(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = content.to_ascii_lowercase();
    let needle = "#invoke:";
    let mut start = 0usize;
    while let Some(found) = lower[start..].find(needle) {
        let name_start = start + found + needle.len();
        let mut end = name_start;
        while end < content.len() {
            let ch = content.as_bytes()[end];
            if matches!(ch, b'|' | b'}' | b'\n' | b'\r' | b' ') {
                break;
            }
            end += 1;
        }
        let module = collapse_whitespace(&content[name_start..end]);
        if !module.is_empty() {
            out.push(module);
        }
        start = name_start;
    }
    out
}

fn collect_inline_module_titles(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = content.to_ascii_lowercase();
    let needle = "module:";
    let mut start = 0usize;
    while let Some(found) = lower[start..].find(needle) {
        let title_start = start + found;
        let mut end = title_start + needle.len();
        while end < content.len() {
            let ch = content.as_bytes()[end];
            if matches!(
                ch,
                b'"' | b'\'' | b'|' | b'}' | b'>' | b']' | b'\n' | b'\r' | b' '
            ) {
                break;
            }
            end += 1;
        }
        let module = collapse_whitespace(&content[title_start..end]);
        if !module.is_empty() {
            out.push(module);
        }
        start = title_start + needle.len();
    }
    out
}

fn normalize_module_title(value: &str) -> String {
    let trimmed = collapse_whitespace(&value.replace('_', " "));
    if trimmed
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Module:"))
    {
        trimmed
    } else {
        format!("Module:{trimmed}")
    }
}

fn find_template_invocation(block: &str, template_title: &str) -> Option<String> {
    let normalized_target = normalize_template_lookup_title(template_title);
    let short_target = normalized_target
        .strip_prefix("Template:")
        .unwrap_or(&normalized_target);
    let bytes = block.as_bytes();
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        if bytes[index..].starts_with(b"{{") && !bytes[index..].starts_with(b"{{{") {
            let name_start = index + 2;
            let mut name_end = name_start;
            while name_end < bytes.len() {
                let byte = bytes[name_end];
                if matches!(byte, b'|' | b'}' | b'\n' | b'\r') {
                    break;
                }
                name_end += 1;
            }
            let candidate = collapse_whitespace(&block[name_start..name_end].replace('_', " "));
            let normalized_candidate = normalize_template_lookup_title(&candidate);
            let short_candidate = normalized_candidate
                .strip_prefix("Template:")
                .unwrap_or(&normalized_candidate);
            if normalized_candidate == normalized_target || short_candidate == short_target {
                return extract_balanced_template(block, index);
            }
            index = name_start;
            continue;
        }
        index += 1;
    }
    None
}

fn extract_balanced_template(content: &str, start: usize) -> Option<String> {
    let bytes = content.as_bytes();
    if start + 1 >= bytes.len() || !bytes[start..].starts_with(b"{{") {
        return None;
    }
    let mut depth = 0usize;
    let mut index = start;
    while index + 1 < bytes.len() {
        if bytes[index..].starts_with(b"{{") {
            depth += 1;
            index += 2;
            continue;
        }
        if bytes[index..].starts_with(b"}}") {
            depth = depth.saturating_sub(1);
            index += 2;
            if depth == 0 {
                return Some(content[start..index].to_string());
            }
            continue;
        }
        index += 1;
    }
    None
}

fn extract_invocation_parameter_keys(invocation: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for line in invocation.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('|') {
            continue;
        }
        let rest = &trimmed[1..];
        let Some((key, _)) = rest.split_once('=') else {
            continue;
        };
        let key = collapse_whitespace(key);
        if key.is_empty() {
            continue;
        }
        let normalized = key.to_ascii_lowercase();
        if seen.insert(normalized) {
            out.push(key);
        }
    }
    out
}

fn extract_first_tag_body<'a>(content: &'a str, tag_name: &str) -> Option<&'a str> {
    extract_tag_bodies(content, tag_name, 1).into_iter().next()
}

fn extract_tag_bodies<'a>(content: &'a str, tag_name: &str, limit: usize) -> Vec<&'a str> {
    if limit == 0 {
        return Vec::new();
    }

    let lower = content.to_ascii_lowercase();
    let open_prefix = format!("<{}", tag_name.to_ascii_lowercase());
    let close = format!("</{}>", tag_name.to_ascii_lowercase());
    let mut out = Vec::new();
    let mut start = 0usize;
    while out.len() < limit {
        let Some(found) = lower[start..].find(&open_prefix) else {
            break;
        };
        let open_start = start + found;
        let Some(open_end_rel) = lower[open_start..].find('>') else {
            break;
        };
        let body_start = open_start + open_end_rel + 1;
        let Some(close_rel) = lower[body_start..].find(&close) else {
            break;
        };
        let body_end = body_start + close_rel;
        out.push(content[body_start..body_end].trim());
        start = body_end + close.len();
    }
    out
}

fn collapse_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_was_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        extract_module_references, extract_source_parameters, extract_summary_text,
        extract_template_data, extract_template_examples,
    };

    #[test]
    fn templatedata_and_examples_are_extracted() {
        let content = r#"
<includeonly>{{{name|}}}</includeonly><noinclude>
General-purpose infobox.
<syntaxhighlight lang="wikitext">
{{Infobox person
| name = Example
| occupation = Writer
}}
</syntaxhighlight>
<templatedata>
{
  "description": "Infobox for biographies.",
  "format": "block",
  "params": {
    "name": {"label": "Name", "required": true},
    "occupation": {"label": "Occupation", "suggested": true, "aliases": ["job"]}
  }
}
</templatedata>
</noinclude>
"#;

        let data = extract_template_data(content)
            .expect("templatedata parse")
            .expect("templatedata exists");
        assert_eq!(
            data.description.as_deref(),
            Some("Infobox for biographies.")
        );
        assert_eq!(data.parameters.len(), 2);
        assert_eq!(data.parameters[1].aliases, vec!["job".to_string()]);

        let examples = extract_template_examples(
            content,
            "Template:Infobox person",
            "Template:Infobox person",
            "templates/infobox/Template_Infobox_person.wiki",
            4,
        );
        assert_eq!(examples.len(), 1);
        assert!(examples[0].parameter_keys.contains(&"name".to_string()));
    }

    #[test]
    fn source_params_modules_and_summary_are_extracted() {
        let content = r#"
<templatestyles src="Module:Infobox/styles.css" />
{{#invoke:Infobox|render|name={{{name|}}}|occupation={{{occupation|}}}}}
<noinclude>
General-purpose infobox for biographies.
</noinclude>
"#;

        assert_eq!(
            extract_source_parameters(content),
            vec!["name".to_string(), "occupation".to_string()]
        );
        assert_eq!(
            extract_module_references(content),
            vec![
                "Module:Infobox".to_string(),
                "Module:Infobox/styles.css".to_string()
            ]
        );
        assert_eq!(
            extract_summary_text(content).as_deref(),
            Some("General-purpose infobox for biographies.")
        );
    }
}
