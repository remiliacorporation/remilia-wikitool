use super::super::*;
use super::identifiers::{collect_reference_identifier_entries, collect_reference_identifier_keys};
use super::signals::{ReferenceSignalInputs, collect_reference_signals};
use super::source::{
    build_reference_citation_profile, build_reference_summary_hint, choose_reference_authority,
    citation_family_for_reference, classify_reference_source_family,
    classify_reference_source_type, collect_reference_source_urls, normalize_source_domain,
    source_origin_for_reference,
};
use super::templates::{
    choose_primary_reference_template, first_reference_raw_param, first_reference_text_param,
    parse_reference_templates, reference_author_text,
};

pub(super) fn analyze_reference_body(
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
