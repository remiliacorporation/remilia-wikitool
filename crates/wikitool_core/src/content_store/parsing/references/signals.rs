use super::super::*;

#[derive(Clone, Copy)]
pub(super) struct ReferenceSignalInputs<'a> {
    pub(super) primary_template_title: Option<&'a str>,
    pub(super) source_type: &'a str,
    pub(super) source_origin: &'a str,
    pub(super) source_family: &'a str,
    pub(super) authority_kind: &'a str,
    pub(super) reference_title: &'a str,
    pub(super) source_container: &'a str,
    pub(super) source_author: &'a str,
    pub(super) source_domain: &'a str,
    pub(super) source_date: &'a str,
    pub(super) identifier_keys: &'a [String],
    pub(super) identifier_entries: &'a [String],
    pub(super) has_quote: bool,
    pub(super) has_links: bool,
    pub(super) has_archive: bool,
    pub(super) reference_name: Option<&'a str>,
    pub(super) reference_group: Option<&'a str>,
    pub(super) reference_body: &'a str,
}

pub(super) fn collect_reference_signals(input: ReferenceSignalInputs<'_>) -> Vec<String> {
    let mut flags = BTreeSet::new();
    if input.primary_template_title.is_some() {
        flags.insert("citation-template".to_string());
    }
    if !input.reference_title.is_empty() {
        flags.insert("has-title".to_string());
    }
    if !input.source_container.is_empty() {
        flags.insert("has-container".to_string());
    }
    if !input.source_author.is_empty() {
        flags.insert("has-author".to_string());
    }
    if !input.source_domain.is_empty() {
        flags.insert("has-domain".to_string());
    }
    if !input.source_date.is_empty() {
        flags.insert("has-date".to_string());
    }
    if !input.identifier_keys.is_empty() {
        flags.insert("has-identifier".to_string());
    }
    if !input.source_family.is_empty() {
        flags.insert(format!("source-family:{}", input.source_family));
    }
    if !input.authority_kind.is_empty() {
        flags.insert(format!("authority:{}", input.authority_kind));
    }
    if input.has_archive {
        flags.insert("has-archive".to_string());
    }
    if input.has_quote {
        flags.insert("has-quote".to_string());
    }
    if input.has_links {
        flags.insert("has-links".to_string());
    }
    if input.reference_name.is_some() {
        flags.insert("named-reference".to_string());
    }
    if input.reference_group.is_some() {
        flags.insert("grouped-reference".to_string());
    }
    if input.reference_body.trim().is_empty() {
        flags.insert("reused-reference".to_string());
    }
    if input.primary_template_title.is_none() && !input.source_domain.is_empty() {
        flags.insert("bare-url".to_string());
    }
    if input.source_origin == "first-party" {
        flags.insert("first-party".to_string());
    }
    for key in input.identifier_keys {
        flags.insert(format!("identifier:{key}"));
    }
    for entry in input.identifier_entries {
        if let Some((key, _)) = entry.split_once(':') {
            flags.insert(format!("identifier-entry:{key}"));
        }
    }
    if matches!(input.source_type, "social" | "video" | "wiki") {
        flags.insert(format!("source-type:{}", input.source_type));
    }
    flags.into_iter().collect()
}
