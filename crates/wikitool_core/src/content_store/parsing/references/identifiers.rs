use super::super::*;

pub(super) fn collect_reference_identifier_keys(
    template: Option<&ReferenceTemplateDetails>,
    has_url: bool,
    has_archive: bool,
) -> Vec<String> {
    let mut out = BTreeSet::new();
    if let Some(template) = template {
        for key in [
            "doi", "isbn", "issn", "oclc", "pmid", "pmcid", "arxiv", "jstor", "id",
        ] {
            if template
                .named_params
                .get(key)
                .is_some_and(|value| !normalize_spaces(value).is_empty())
            {
                out.insert(key.to_string());
            }
        }
    }
    if has_url {
        out.insert("url".to_string());
    }
    if has_archive {
        out.insert("archive-url".to_string());
    }
    out.into_iter().collect()
}

pub(super) fn collect_reference_identifier_entries(
    template: Option<&ReferenceTemplateDetails>,
) -> Vec<String> {
    let Some(template) = template else {
        return Vec::new();
    };

    let mut out = BTreeSet::new();
    for key in [
        "doi", "isbn", "issn", "oclc", "pmid", "pmcid", "arxiv", "jstor", "id",
    ] {
        let Some(value) = template.named_params.get(key) else {
            continue;
        };
        let normalized_value = normalize_reference_identifier_value(key, value);
        if normalized_value.is_empty() {
            continue;
        }
        out.insert(format!("{key}:{normalized_value}"));
    }
    out.into_iter().collect()
}

pub(crate) fn normalize_reference_identifier_value(key: &str, value: &str) -> String {
    let flattened = flatten_markup_excerpt(value);
    if flattened.is_empty() {
        return String::new();
    }
    let lowered = flattened.to_ascii_lowercase();
    match key {
        "doi" => {
            let trimmed = lowered
                .trim_start_matches("https://doi.org/")
                .trim_start_matches("http://doi.org/")
                .trim_start_matches("doi:")
                .trim();
            normalize_reference_identifier_token(trimmed, true)
        }
        "isbn" | "issn" | "oclc" | "pmid" | "pmcid" | "jstor" => {
            normalize_reference_identifier_token(&lowered, false)
        }
        "arxiv" => {
            normalize_reference_identifier_token(lowered.trim_start_matches("arxiv:").trim(), true)
        }
        _ => normalize_spaces(&flattened),
    }
}

pub(crate) fn normalize_reference_identifier_token(value: &str, preserve_slash: bool) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            continue;
        }
        if preserve_slash && matches!(ch, '.' | '/' | '_' | '-') {
            out.push(ch);
        }
    }
    out
}

pub(crate) fn parse_identifier_entries(entries: &[String]) -> Vec<ParsedIdentifierEntry> {
    let mut out = Vec::new();
    for entry in entries {
        let Some((key, value)) = entry.split_once(':') else {
            continue;
        };
        let key = normalize_template_parameter_key(key);
        let value = normalize_spaces(value);
        if key.is_empty() || value.is_empty() {
            continue;
        }
        let normalized_value = normalize_reference_identifier_value(&key, &value);
        if normalized_value.is_empty() {
            continue;
        }
        out.push(ParsedIdentifierEntry {
            key,
            value,
            normalized_value,
        });
    }
    out
}

pub(crate) fn build_reference_authority_key(
    authority_kind: &str,
    source_authority: &str,
) -> String {
    let normalized_authority = normalize_spaces(source_authority);
    if normalized_authority.is_empty() {
        return authority_kind.to_string();
    }
    format!(
        "{}:{}",
        authority_kind,
        normalized_authority.to_ascii_lowercase()
    )
}

pub(crate) fn build_reference_authority_retrieval_text(
    reference: &IndexedReferenceRecord,
) -> String {
    let mut values = vec![
        reference.source_authority.clone(),
        reference.reference_title.clone(),
        reference.source_container.clone(),
        reference.source_author.clone(),
        reference.source_domain.clone(),
        reference.source_family.clone(),
        reference.source_type.clone(),
        reference.source_origin.clone(),
        reference.summary_text.clone(),
    ];
    values.extend(reference.identifier_entries.iter().cloned());
    values.extend(reference.template_titles.iter().cloned());
    values.extend(reference.link_titles.iter().cloned());
    collect_normalized_string_list(values).join("\n")
}
