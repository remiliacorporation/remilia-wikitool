use std::collections::HashSet;

use reqwest::Url;

use super::model::ParsedWikiUrl;

pub fn parse_wiki_url(url: &str) -> Option<ParsedWikiUrl> {
    let parsed = Url::parse(url).ok()?;
    let domain = parsed.host_str()?.to_string();
    let scheme = parsed.scheme().to_string();
    let path = parsed.path();

    let mut title = None::<String>;
    let mut base_url = format!("{scheme}://{domain}/wiki/");
    let mut api_candidates = api_candidates_for_domain(&scheme, &domain);

    if let Some(rest) = path.strip_prefix("/wiki/") {
        if !rest.trim().is_empty() {
            title = Some(decode_title(rest));
        }
    } else if path.ends_with("/w/index.php") || path.ends_with("/index.php") {
        for (key, value) in parsed.query_pairs() {
            if key.eq_ignore_ascii_case("title") {
                let value = value.trim().to_string();
                if !value.is_empty() {
                    title = Some(decode_title(&value));
                }
                break;
            }
        }
        if path.ends_with("/index.php") {
            api_candidates = vec![format!("{scheme}://{domain}/api.php")];
        }
    } else {
        let segments = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if parsed.query().is_none() && segments.len() == 1 {
            title = Some(decode_title(segments[0]));
            base_url = format!("{scheme}://{domain}/");
            api_candidates = vec![
                format!("{scheme}://{domain}/api.php"),
                format!("{scheme}://{domain}/w/api.php"),
            ];
        }
    }

    let title = title?;
    Some(ParsedWikiUrl {
        domain,
        title,
        api_candidates: dedupe(api_candidates),
        base_url,
    })
}

pub(crate) fn decode_title(raw: &str) -> String {
    raw.replace('_', " ").trim().to_string()
}

pub(crate) fn encode_title(title: &str) -> String {
    title.trim().replace(' ', "_")
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        if seen.insert(value.clone()) {
            output.push(value);
        }
    }
    output
}

fn api_candidates_for_domain(scheme: &str, domain: &str) -> Vec<String> {
    if domain.ends_with("fandom.com") {
        return vec![
            format!("{scheme}://{domain}/api.php"),
            format!("{scheme}://{domain}/w/api.php"),
        ];
    }
    vec![
        format!("{scheme}://{domain}/w/api.php"),
        format!("{scheme}://{domain}/api.php"),
    ]
}
