use super::*;

pub(crate) fn extract_reference_records_from_sections(
    sections: &[ParsedContentSection],
) -> Vec<IndexedReferenceRecord> {
    let mut out = Vec::new();
    for section in sections {
        out.extend(extract_reference_records_for_section(
            section.section_heading.clone(),
            &section.section_text,
        ));
    }
    out
}

pub(crate) fn extract_reference_records(content: &str) -> Vec<LocalReferenceUsage> {
    extract_reference_records_from_sections(&parse_content_sections(content))
        .into_iter()
        .map(|record| LocalReferenceUsage {
            section_heading: record.section_heading,
            reference_name: record.reference_name,
            reference_group: record.reference_group,
            citation_profile: record.citation_profile,
            citation_family: record.citation_family,
            primary_template_title: record.primary_template_title,
            source_type: record.source_type,
            source_origin: record.source_origin,
            source_family: record.source_family,
            authority_kind: record.authority_kind,
            source_authority: record.source_authority,
            reference_title: record.reference_title,
            source_container: record.source_container,
            source_author: record.source_author,
            source_domain: record.source_domain,
            source_date: record.source_date,
            canonical_url: record.canonical_url,
            identifier_keys: record.identifier_keys,
            identifier_entries: record.identifier_entries,
            source_urls: record.source_urls,
            retrieval_signals: record.retrieval_signals,
            summary_text: record.summary_text,
            template_titles: record.template_titles,
            link_titles: record.link_titles,
            token_estimate: record.token_estimate,
        })
        .take(CONTEXT_REFERENCE_LIMIT)
        .collect()
}

pub(crate) fn extract_reference_records_for_section(
    section_heading: Option<String>,
    content: &str,
) -> Vec<IndexedReferenceRecord> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if !starts_with_html_tag(bytes, cursor, "ref") {
            cursor += 1;
            continue;
        }
        let Some((tag_end, tag_body, self_closing)) = parse_open_tag(content, cursor, "ref") else {
            cursor += 1;
            continue;
        };
        let attributes = parse_html_attributes(&tag_body);
        let reference_name = attributes
            .get("name")
            .map(|value| normalize_spaces(value))
            .filter(|value| !value.is_empty());
        let reference_group = attributes
            .get("group")
            .map(|value| normalize_spaces(value))
            .filter(|value| !value.is_empty());

        let (reference_wikitext, reference_body, next_cursor) = if self_closing {
            (content[cursor..tag_end].to_string(), String::new(), tag_end)
        } else if let Some((close_start, close_end)) =
            find_closing_html_tag(content, tag_end, "ref")
        {
            (
                content[cursor..close_end].to_string(),
                content[tag_end..close_start].to_string(),
                close_end,
            )
        } else {
            (content[cursor..tag_end].to_string(), String::new(), tag_end)
        };

        let template_titles = extract_template_titles(&reference_body);
        let link_titles = extract_link_titles(&reference_body);
        let analysis = analyze_reference_body(
            &reference_body,
            &template_titles,
            &link_titles,
            reference_name.as_deref(),
            reference_group.as_deref(),
        );
        let mut summary_text = flatten_markup_excerpt(&reference_body);
        if summary_text.is_empty() {
            summary_text = analysis.summary_hint.clone();
        }
        if summary_text.is_empty() && !template_titles.is_empty() {
            summary_text = template_titles.join(", ");
        }
        if summary_text.is_empty()
            && let Some(name) = &reference_name
        {
            summary_text = format!("Named reference {name}");
        }
        if summary_text.is_empty() {
            summary_text = "<ref>".to_string();
        }

        let token_estimate = estimate_tokens(&reference_wikitext);
        out.push(IndexedReferenceRecord {
            section_heading: section_heading.clone(),
            reference_name,
            reference_group,
            citation_profile: analysis.citation_profile,
            citation_family: analysis.citation_family,
            primary_template_title: analysis.primary_template_title,
            source_type: analysis.source_type,
            source_origin: analysis.source_origin,
            source_family: analysis.source_family,
            authority_kind: analysis.authority_kind,
            source_authority: analysis.source_authority,
            reference_title: analysis.reference_title,
            source_container: analysis.source_container,
            source_author: analysis.source_author,
            source_domain: analysis.source_domain,
            source_date: analysis.source_date,
            canonical_url: analysis.canonical_url,
            identifier_keys: analysis.identifier_keys,
            identifier_entries: analysis.identifier_entries,
            source_urls: analysis.source_urls,
            retrieval_signals: analysis.retrieval_signals,
            summary_text: summarize_words(&summary_text, AUTHORING_PAGE_SUMMARY_WORD_LIMIT),
            reference_wikitext,
            template_titles,
            link_titles,
            token_estimate,
        });
        cursor = next_cursor.max(cursor.saturating_add(1));
    }

    out
}

pub(crate) fn analyze_reference_body(
    reference_body: &str,
    template_titles: &[String],
    link_titles: &[String],
    reference_name: Option<&str>,
    reference_group: Option<&str>,
) -> ReferenceAnalysis {
    let templates = parse_reference_templates(reference_body);
    let primary_template = choose_primary_reference_template(&templates);
    let primary_template_title = primary_template
        .map(|template| template.template_title.clone())
        .or_else(|| template_titles.first().cloned());

    let mut reference_title = first_reference_text_param(
        primary_template,
        &["title", "chapter", "entry", "article", "script-title"],
    );
    if reference_title.is_empty()
        && let Some(template) = primary_template
        && let Some(value) = template.positional_params.first()
    {
        reference_title = flatten_markup_excerpt(value);
    }
    if reference_title.is_empty()
        && let Some(first_link) = link_titles.first()
    {
        reference_title = first_link.clone();
    }

    let mut source_container = first_reference_text_param(
        primary_template,
        &[
            "website",
            "work",
            "journal",
            "newspaper",
            "magazine",
            "periodical",
            "encyclopedia",
            "publisher",
            "publication",
        ],
    );
    let source_author = reference_author_text(primary_template);
    let source_date = first_reference_text_param(
        primary_template,
        &["date", "year", "publication-date", "access-date"],
    );
    let has_quote =
        !first_reference_text_param(primary_template, &["quote", "quotation"]).is_empty();
    let source_urls = collect_reference_source_urls(primary_template, reference_body);
    let canonical_url = source_urls.first().cloned().unwrap_or_default();
    let archive_url = first_reference_raw_param(primary_template, &["archive-url", "archiveurl"]);
    let source_domain = normalize_source_domain(&canonical_url)
        .or_else(|| archive_url.as_deref().and_then(normalize_source_domain))
        .unwrap_or_default();
    let source_type = classify_reference_source_type(
        primary_template,
        &source_domain,
        !source_urls.is_empty(),
        reference_body,
    );
    if source_container.is_empty()
        && !source_domain.is_empty()
        && matches!(
            source_type.as_str(),
            "web" | "news" | "social" | "video" | "wiki"
        )
    {
        source_container = source_domain.clone();
    }
    let source_origin = source_origin_for_reference(&source_domain, &source_type);
    let source_family = classify_reference_source_family(&source_type, &source_origin);
    let (authority_kind, source_authority) = choose_reference_authority(
        &source_domain,
        &source_container,
        &source_author,
        primary_template_title.as_deref(),
        reference_name,
        &source_type,
    );
    let citation_family = citation_family_for_reference(
        primary_template_title.as_deref(),
        &source_type,
        reference_group,
    );
    let identifier_keys = collect_reference_identifier_keys(
        primary_template,
        !source_urls.is_empty(),
        archive_url.is_some(),
    );
    let identifier_entries = collect_reference_identifier_entries(primary_template);
    let signal_inputs = ReferenceSignalInputs {
        primary_template_title: primary_template_title.as_deref(),
        source_type: &source_type,
        source_origin: &source_origin,
        source_family: &source_family,
        authority_kind: &authority_kind,
        reference_title: &reference_title,
        source_container: &source_container,
        source_author: &source_author,
        source_domain: &source_domain,
        source_date: &source_date,
        identifier_keys: &identifier_keys,
        identifier_entries: &identifier_entries,
        has_quote,
        has_links: !link_titles.is_empty(),
        has_archive: archive_url.is_some(),
        reference_name,
        reference_group,
        reference_body,
    };
    let retrieval_signals = collect_reference_signals(signal_inputs);
    let summary_hint = build_reference_summary_hint(
        &reference_title,
        &source_container,
        &source_author,
        &source_domain,
        &source_authority,
        primary_template_title.as_deref(),
        reference_name,
    );
    let citation_profile = build_reference_citation_profile(
        &source_type,
        &source_origin,
        &citation_family,
        &source_domain,
        &authority_kind,
        &source_authority,
    );

    ReferenceAnalysis {
        citation_profile,
        citation_family,
        primary_template_title,
        source_type,
        source_origin,
        source_family,
        authority_kind,
        source_authority,
        reference_title,
        source_container,
        source_author,
        source_domain,
        source_date,
        canonical_url,
        identifier_keys,
        identifier_entries,
        source_urls,
        retrieval_signals,
        summary_hint,
    }
}

pub(crate) fn parse_reference_templates(reference_body: &str) -> Vec<ReferenceTemplateDetails> {
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

pub(crate) fn choose_primary_reference_template(
    templates: &[ReferenceTemplateDetails],
) -> Option<&ReferenceTemplateDetails> {
    templates.iter().min_by(|left, right| {
        reference_template_priority(&left.template_title)
            .cmp(&reference_template_priority(&right.template_title))
            .then_with(|| left.template_title.cmp(&right.template_title))
    })
}

pub(crate) fn reference_template_priority(template_title: &str) -> u8 {
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

pub(crate) fn first_reference_text_param(
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

pub(crate) fn first_reference_raw_param(
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

pub(crate) fn reference_author_text(template: Option<&ReferenceTemplateDetails>) -> String {
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

pub(crate) fn collect_reference_identifier_keys(
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

pub(crate) fn collect_reference_identifier_entries(
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

pub(crate) fn collect_reference_source_urls(
    template: Option<&ReferenceTemplateDetails>,
    reference_body: &str,
) -> Vec<String> {
    let mut out = BTreeSet::new();
    if let Some(template) = template {
        for key in [
            "url",
            "chapter-url",
            "article-url",
            "archive-url",
            "archiveurl",
        ] {
            if let Some(value) = template.named_params.get(key)
                && let Some(normalized) = normalize_reference_url(value)
            {
                out.insert(normalized);
            }
        }
    }
    if let Some(url) = extract_first_url(reference_body)
        && let Some(normalized) = normalize_reference_url(&url)
    {
        out.insert(normalized);
    }
    out.into_iter().collect()
}

pub(crate) fn normalize_reference_url(value: &str) -> Option<String> {
    let candidate = normalize_spaces(value);
    if candidate.is_empty() {
        return None;
    }
    if candidate.starts_with("//") {
        return Some(format!("https:{candidate}"));
    }
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        return Some(candidate);
    }
    None
}

pub(crate) fn choose_reference_authority(
    source_domain: &str,
    source_container: &str,
    source_author: &str,
    primary_template_title: Option<&str>,
    reference_name: Option<&str>,
    source_type: &str,
) -> (String, String) {
    if !source_domain.is_empty() {
        return ("domain".to_string(), source_domain.to_string());
    }
    if !source_container.is_empty() {
        return ("container".to_string(), source_container.to_string());
    }
    if !source_author.is_empty() {
        return ("author".to_string(), source_author.to_string());
    }
    if let Some(template_title) = primary_template_title {
        return ("template".to_string(), template_title.to_string());
    }
    if let Some(name) = reference_name {
        let normalized = normalize_spaces(name);
        if !normalized.is_empty() {
            return ("named-reference".to_string(), normalized);
        }
    }
    if !source_type.is_empty() {
        return ("source-type".to_string(), source_type.to_string());
    }
    ("unknown".to_string(), String::new())
}

pub(crate) fn classify_reference_source_family(source_type: &str, source_origin: &str) -> String {
    if source_type.is_empty() {
        return "unknown".to_string();
    }
    if source_origin == "first-party" {
        return format!("first-party-{source_type}");
    }
    source_type.to_string()
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

#[derive(Clone, Copy)]
pub(super) struct ReferenceSignalInputs<'a> {
    primary_template_title: Option<&'a str>,
    source_type: &'a str,
    source_origin: &'a str,
    source_family: &'a str,
    authority_kind: &'a str,
    reference_title: &'a str,
    source_container: &'a str,
    source_author: &'a str,
    source_domain: &'a str,
    source_date: &'a str,
    identifier_keys: &'a [String],
    identifier_entries: &'a [String],
    has_quote: bool,
    has_links: bool,
    has_archive: bool,
    reference_name: Option<&'a str>,
    reference_group: Option<&'a str>,
    reference_body: &'a str,
}

fn collect_reference_signals(input: ReferenceSignalInputs<'_>) -> Vec<String> {
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

pub(crate) fn classify_reference_source_type(
    template: Option<&ReferenceTemplateDetails>,
    source_domain: &str,
    has_url: bool,
    reference_body: &str,
) -> String {
    if let Some(template) = template {
        let lowered = template.template_title.to_ascii_lowercase();
        if lowered.contains("cite journal") || lowered.contains("journal") {
            return "journal".to_string();
        }
        if lowered.contains("cite book") || lowered.contains("book") {
            return "book".to_string();
        }
        if lowered.contains("cite news") || lowered.contains("news") {
            return "news".to_string();
        }
        if lowered.contains("cite video") || lowered.contains("video") {
            return "video".to_string();
        }
        if lowered.contains("tweet") || lowered.contains("social") {
            return "social".to_string();
        }
        if lowered.contains("wiki") {
            return "wiki".to_string();
        }
        if lowered.contains("sfn") || lowered.contains("harv") {
            return "short-footnote".to_string();
        }
        if lowered.contains("cite web") || lowered.contains("web") {
            return "web".to_string();
        }
    }
    if is_video_domain(source_domain) {
        return "video".to_string();
    }
    if is_social_domain(source_domain) {
        return "social".to_string();
    }
    if is_wiki_domain(source_domain) {
        return "wiki".to_string();
    }
    if has_url {
        return "web".to_string();
    }
    if reference_body.trim().is_empty() {
        return "note".to_string();
    }
    "other".to_string()
}

pub(crate) fn citation_family_for_reference(
    primary_template_title: Option<&str>,
    source_type: &str,
    reference_group: Option<&str>,
) -> String {
    if let Some(template_title) = primary_template_title {
        return template_title.to_string();
    }
    if reference_group.is_some() || source_type == "note" {
        return "note".to_string();
    }
    if source_type == "web" {
        return "bare-url".to_string();
    }
    "<ref>".to_string()
}

pub(crate) fn source_origin_for_reference(source_domain: &str, source_type: &str) -> String {
    if source_domain.ends_with("remilia.org") {
        return "first-party".to_string();
    }
    if source_type == "wiki" {
        return "wiki".to_string();
    }
    if source_domain.is_empty() {
        return "unknown".to_string();
    }
    "external".to_string()
}

pub(crate) fn build_reference_summary_hint(
    reference_title: &str,
    source_container: &str,
    source_author: &str,
    source_domain: &str,
    source_authority: &str,
    primary_template_title: Option<&str>,
    reference_name: Option<&str>,
) -> String {
    if !reference_title.is_empty() && !source_container.is_empty() {
        return format!("{reference_title} ({source_container})");
    }
    if !reference_title.is_empty() {
        return reference_title.to_string();
    }
    if !source_container.is_empty() && !source_author.is_empty() {
        return format!("{source_container} ({source_author})");
    }
    if !source_container.is_empty() {
        return source_container.to_string();
    }
    if !source_author.is_empty() {
        return source_author.to_string();
    }
    if !source_domain.is_empty() {
        return source_domain.to_string();
    }
    if !source_authority.is_empty() {
        return source_authority.to_string();
    }
    if let Some(template_title) = primary_template_title {
        return template_title.to_string();
    }
    if let Some(name) = reference_name {
        return format!("Named reference {name}");
    }
    String::new()
}

pub(crate) fn build_reference_citation_profile(
    source_type: &str,
    source_origin: &str,
    citation_family: &str,
    source_domain: &str,
    authority_kind: &str,
    source_authority: &str,
) -> String {
    if !source_domain.is_empty()
        && matches!(source_type, "web" | "news" | "social" | "video" | "wiki")
    {
        if source_origin == "first-party" {
            return format!("first-party {source_type} / {source_domain}");
        }
        return format!("{source_type} / {source_domain}");
    }
    if !source_authority.is_empty() && matches!(authority_kind, "container" | "author") {
        if source_origin == "first-party" {
            return format!("first-party {source_type} / {source_authority}");
        }
        return format!("{source_type} / {source_authority}");
    }
    if citation_family != "<ref>" && !citation_family.is_empty() {
        return format!("{source_type} / {citation_family}");
    }
    source_type.to_string()
}

pub(crate) fn extract_first_url(value: &str) -> Option<String> {
    for (start, _) in value.char_indices() {
        let rest = &value[start..];
        let starts_http = rest.starts_with("http://");
        let starts_https = rest.starts_with("https://");
        let starts_protocol_relative = rest.starts_with("//");
        if !(starts_http || starts_https || starts_protocol_relative) {
            continue;
        }

        let mut end = value.len();
        for (offset, ch) in rest.char_indices() {
            if ch.is_whitespace() || matches!(ch, '|' | '}' | ']' | '<' | '"' | '\'') {
                end = start + offset;
                break;
            }
        }
        let candidate = normalize_spaces(&value[start..end]);
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn normalize_source_domain(url: &str) -> Option<String> {
    let candidate = if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.to_string()
    };
    let parsed = Url::parse(&candidate).ok()?;
    let host = parsed
        .host_str()?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    if host.is_empty() { None } else { Some(host) }
}

pub(crate) fn is_social_domain(domain: &str) -> bool {
    matches!(
        domain,
        "twitter.com"
            | "x.com"
            | "farcaster.xyz"
            | "instagram.com"
            | "tiktok.com"
            | "mastodon.social"
    )
}

pub(crate) fn is_video_domain(domain: &str) -> bool {
    matches!(
        domain,
        "youtube.com" | "youtu.be" | "vimeo.com" | "twitch.tv"
    )
}

pub(crate) fn is_wiki_domain(domain: &str) -> bool {
    domain.ends_with(".wikipedia.org")
        || domain.ends_with(".wiktionary.org")
        || domain.ends_with(".wikimedia.org")
        || domain.ends_with(".miraheze.org")
        || domain.ends_with(".fandom.com")
        || domain.starts_with("wiki.")
}

pub(crate) fn is_media_option(value: &str) -> bool {
    let normalized = normalize_spaces(value).to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }
    if matches!(
        normalized.as_str(),
        "thumb"
            | "thumbnail"
            | "frame"
            | "framed"
            | "frameless"
            | "border"
            | "right"
            | "left"
            | "center"
            | "none"
            | "baseline"
            | "sub"
            | "super"
            | "top"
            | "text-top"
            | "middle"
            | "bottom"
    ) {
        return true;
    }
    if normalized.ends_with("px")
        || normalized.starts_with("upright")
        || normalized.starts_with("alt=")
        || normalized.starts_with("link=")
        || normalized.starts_with("page=")
        || normalized.starts_with("class=")
        || normalized.starts_with("lang=")
        || normalized.starts_with("start=")
        || normalized.starts_with("end=")
    {
        return true;
    }
    false
}
