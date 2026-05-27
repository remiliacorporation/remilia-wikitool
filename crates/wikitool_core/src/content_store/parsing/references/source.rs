use super::super::*;

pub(super) fn collect_reference_source_urls(
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

pub(super) fn normalize_reference_url(value: &str) -> Option<String> {
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

pub(super) fn choose_reference_authority(
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

pub(super) fn classify_reference_source_family(source_type: &str, source_origin: &str) -> String {
    if source_type.is_empty() {
        return "unknown".to_string();
    }
    if source_origin == "first-party" {
        return format!("first-party-{source_type}");
    }
    source_type.to_string()
}

pub(super) fn classify_reference_source_type(
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

pub(super) fn citation_family_for_reference(
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

pub(super) fn source_origin_for_reference(source_domain: &str, source_type: &str) -> String {
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

pub(super) fn build_reference_summary_hint(
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

pub(super) fn build_reference_citation_profile(
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

pub(super) fn normalize_source_domain(url: &str) -> Option<String> {
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

fn is_social_domain(domain: &str) -> bool {
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

fn is_video_domain(domain: &str) -> bool {
    matches!(
        domain,
        "youtube.com" | "youtu.be" | "vimeo.com" | "twitch.tv"
    )
}

fn is_wiki_domain(domain: &str) -> bool {
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
