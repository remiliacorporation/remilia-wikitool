use std::collections::HashSet;

use reqwest::Url;

use super::model::ParsedWikiUrl;

pub fn parse_wiki_url(url: &str) -> Option<ParsedWikiUrl> {
    let parsed = Url::parse(url).ok()?;
    let domain = parsed.host_str()?.to_string();
    let origin = parsed.origin().ascii_serialization();
    let path = parsed.path();

    let mut title = None::<String>;
    let mut base_url = None::<String>;
    let mut api_candidates = Vec::new();

    if let Some((prefix, rest)) = split_article_path(path, "/wiki/") {
        if !rest.trim().is_empty() {
            title = Some(decode_title(rest));
            base_url = Some(format!("{origin}{prefix}/wiki/"));
            api_candidates = api_candidates_for_base_prefix(&origin, prefix, false);
        }
    } else if let Some(prefix) = path_prefix_before(path, "/w/index.php") {
        for (key, value) in parsed.query_pairs() {
            if key.eq_ignore_ascii_case("title") {
                let value = value.trim().to_string();
                if !value.is_empty() {
                    title = Some(decode_title(&value));
                    base_url = Some(format!("{origin}{prefix}/w/index.php?title="));
                    api_candidates = api_candidates_for_base_prefix(&origin, prefix, true);
                }
                break;
            }
        }
    } else if let Some(prefix) = path_prefix_before(path, "/index.php") {
        for (key, value) in parsed.query_pairs() {
            if key.eq_ignore_ascii_case("title") {
                let value = value.trim().to_string();
                if !value.is_empty() {
                    title = Some(decode_title(&value));
                    base_url = Some(format!("{origin}{prefix}/index.php?title="));
                    api_candidates = api_candidates_for_base_prefix(&origin, prefix, false);
                }
                break;
            }
        }
    } else {
        let segments = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if parsed.query().is_none()
            && segments.len() == 1
            && host_likely_uses_root_article_paths(&domain)
        {
            title = Some(decode_title(segments[0]));
            base_url = Some(format!("{origin}/"));
            api_candidates = api_candidates_for_base_prefix(&origin, "", false);
        }
    }

    let title = title?;
    Some(ParsedWikiUrl {
        domain,
        title,
        base_url: base_url?,
        api_candidates: dedupe(api_candidates),
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

fn api_candidates_for_base_prefix(origin: &str, prefix: &str, prefer_w_path: bool) -> Vec<String> {
    let normalized_prefix = normalize_prefix(prefix);
    if prefer_w_path {
        let mut candidates = vec![format!("{origin}{normalized_prefix}/w/api.php")];
        candidates.push(format!("{origin}{normalized_prefix}/api.php"));
        return dedupe(candidates);
    }

    vec![
        format!("{origin}{normalized_prefix}/api.php"),
        format!("{origin}{normalized_prefix}/w/api.php"),
    ]
}

fn normalize_prefix(prefix: &str) -> String {
    if prefix.is_empty() {
        String::new()
    } else {
        prefix.trim_end_matches('/').to_string()
    }
}

fn split_article_path<'a>(path: &'a str, marker: &str) -> Option<(&'a str, &'a str)> {
    let marker_index = path.find(marker)?;
    let prefix = &path[..marker_index];
    let rest = &path[marker_index + marker.len()..];
    Some((prefix, rest))
}

fn path_prefix_before<'a>(path: &'a str, suffix: &str) -> Option<&'a str> {
    path.strip_suffix(suffix)
}

fn host_likely_uses_root_article_paths(domain: &str) -> bool {
    let lowered = domain.to_ascii_lowercase();
    lowered.contains("wiki")
        || lowered.ends_with("fandom.com")
        || lowered.ends_with("wikimedia.org")
        || lowered.ends_with("miraheze.org")
}

#[cfg(test)]
mod tests {
    use super::parse_wiki_url;

    #[test]
    fn parses_root_article_paths_for_wiki_like_hosts() {
        let parsed = parse_wiki_url("https://wiki.remilia.org/Hypercitationalism")
            .expect("wiki-like root article path");
        assert_eq!(parsed.domain, "wiki.remilia.org");
        assert_eq!(parsed.title, "Hypercitationalism");
        assert_eq!(parsed.base_url, "https://wiki.remilia.org/");
        assert_eq!(
            parsed.api_candidates,
            vec![
                "https://wiki.remilia.org/api.php",
                "https://wiki.remilia.org/w/api.php",
            ]
        );
    }

    #[test]
    fn parses_short_urls_under_base_paths() {
        let parsed = parse_wiki_url("https://example.org/mediawiki/wiki/Foo_Bar")
            .expect("base-path short URL");
        assert_eq!(parsed.title, "Foo Bar");
        assert_eq!(parsed.base_url, "https://example.org/mediawiki/wiki/");
        assert_eq!(
            parsed.api_candidates,
            vec![
                "https://example.org/mediawiki/api.php",
                "https://example.org/mediawiki/w/api.php",
            ]
        );
    }

    #[test]
    fn parses_index_php_urls_under_base_paths() {
        let parsed = parse_wiki_url("https://example.org/mediawiki/index.php?title=Foo_Bar")
            .expect("base-path index.php URL");
        assert_eq!(parsed.title, "Foo Bar");
        assert_eq!(
            parsed.base_url,
            "https://example.org/mediawiki/index.php?title="
        );
        assert_eq!(
            parsed.api_candidates,
            vec![
                "https://example.org/mediawiki/api.php",
                "https://example.org/mediawiki/w/api.php",
            ]
        );
    }

    #[test]
    fn parses_w_index_php_urls_under_base_paths() {
        let parsed = parse_wiki_url("https://example.org/mediawiki/w/index.php?title=Manual:Hooks")
            .expect("base-path w/index.php URL");
        assert_eq!(parsed.title, "Manual:Hooks");
        assert_eq!(
            parsed.base_url,
            "https://example.org/mediawiki/w/index.php?title="
        );
        assert_eq!(
            parsed.api_candidates,
            vec![
                "https://example.org/mediawiki/w/api.php",
                "https://example.org/mediawiki/api.php",
            ]
        );
    }

    #[test]
    fn parses_non_short_urls_with_extra_query_parameters() {
        let parsed = parse_wiki_url(
            "https://wiki.remilia.org/index.php?title=Special:UserLogout&returnto=Main+Page",
        )
        .expect("non-short query URL");
        assert_eq!(parsed.title, "Special:UserLogout");
        assert_eq!(parsed.base_url, "https://wiki.remilia.org/index.php?title=");
        assert_eq!(
            parsed.api_candidates,
            vec![
                "https://wiki.remilia.org/api.php",
                "https://wiki.remilia.org/w/api.php",
            ]
        );
    }

    #[test]
    fn does_not_treat_generic_root_paths_as_mediawiki_pages() {
        assert!(
            parse_wiki_url(
                "https://goldenlight.mirror.xyz/c3bZd7hLmn60CR-aDeVkzhiQfZcEKglbzZmP__e4JlI"
            )
            .is_none()
        );
    }
}
