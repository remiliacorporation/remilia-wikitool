use super::super::*;

pub(super) fn parse_reference_templates(reference_body: &str) -> Vec<ReferenceTemplateDetails> {
    extract_template_invocations(reference_body)
        .into_iter()
        .filter_map(|invocation| {
            let inner = invocation
                .raw_wikitext
                .strip_prefix("{{")
                .and_then(|value| value.strip_suffix("}}"))?;
            let segments = split_template_segments(inner);
            let mut named_params = BTreeMap::new();
            let mut positional_params = Vec::new();
            for segment in segments.into_iter().skip(1) {
                if let Some((key, value)) = split_once_top_level_equals(&segment) {
                    named_params.insert(
                        normalize_template_parameter_key(&key),
                        value.trim().to_string(),
                    );
                } else {
                    positional_params.push(segment.trim().to_string());
                }
            }
            Some(ReferenceTemplateDetails {
                template_title: invocation.template_title,
                named_params,
                positional_params,
            })
        })
        .collect()
}

pub(super) fn choose_primary_reference_template(
    templates: &[ReferenceTemplateDetails],
) -> Option<&ReferenceTemplateDetails> {
    templates.iter().min_by(|left, right| {
        reference_template_priority(&left.template_title)
            .cmp(&reference_template_priority(&right.template_title))
            .then_with(|| left.template_title.cmp(&right.template_title))
    })
}

pub(super) fn reference_template_priority(template_title: &str) -> u8 {
    let lowered = template_title.to_ascii_lowercase();
    if lowered.contains("cite ") || lowered.contains("citation") {
        return 0;
    }
    if lowered.contains("sfn") || lowered.contains("harv") {
        return 1;
    }
    if lowered.contains("ref") || lowered.contains("note") {
        return 2;
    }
    3
}

pub(super) fn first_reference_text_param(
    template: Option<&ReferenceTemplateDetails>,
    keys: &[&str],
) -> String {
    let Some(template) = template else {
        return String::new();
    };
    for key in keys {
        if let Some(value) = template.named_params.get(*key) {
            let normalized = flatten_markup_excerpt(value);
            if !normalized.is_empty() {
                return normalized;
            }
        }
    }
    String::new()
}

pub(super) fn first_reference_raw_param(
    template: Option<&ReferenceTemplateDetails>,
    keys: &[&str],
) -> Option<String> {
    let template = template?;
    for key in keys {
        if let Some(value) = template.named_params.get(*key) {
            let normalized = normalize_spaces(value);
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }
    None
}

pub(super) fn reference_author_text(template: Option<&ReferenceTemplateDetails>) -> String {
    let Some(template) = template else {
        return String::new();
    };
    for key in ["author", "authors", "last", "last1", "editor"] {
        if let Some(value) = template.named_params.get(key) {
            let normalized = flatten_markup_excerpt(value);
            if !normalized.is_empty() {
                if key == "last" || key == "last1" {
                    let first = template
                        .named_params
                        .get("first")
                        .or_else(|| template.named_params.get("first1"))
                        .map(|value| flatten_markup_excerpt(value))
                        .unwrap_or_default();
                    if !first.is_empty() {
                        return format!("{normalized}, {first}");
                    }
                }
                return normalized;
            }
        }
    }
    String::new()
}
