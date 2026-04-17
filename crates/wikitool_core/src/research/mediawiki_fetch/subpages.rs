use anyhow::{Result, bail};
use serde_json::Value;

use super::siteinfo::mediawiki_query_namespace_id;
use crate::research::model::ParsedWikiUrl;
use crate::research::web_fetch::{ExternalClient, external_client};

pub fn list_subpages(
    parent_title: &str,
    parsed: &ParsedWikiUrl,
    limit: usize,
) -> Result<Vec<String>> {
    let mut client = external_client()?;
    let target = SubpageQueryTarget::from_parent_title(parent_title);
    let mut candidate_errors = Vec::new();
    for api_url in &parsed.api_candidates {
        let (namespace, prefix) = match target.namespace_prefix.as_deref() {
            Some(prefix) => match mediawiki_query_namespace_id(&mut client, api_url, prefix) {
                Ok(Some(namespace)) => (namespace, target.namespace_local_prefix.as_str()),
                Ok(None) => (0, target.main_namespace_prefix.as_str()),
                Err(error) => {
                    candidate_errors.push(format!("{api_url}: {error:#}"));
                    continue;
                }
            },
            None => (0, target.main_namespace_prefix.as_str()),
        };
        let response =
            mediawiki_query_allpages(&mut client, api_url, prefix, namespace, limit.max(1));
        match response {
            Ok(value) => return Ok(value),
            Err(error) => candidate_errors.push(format!("{api_url}: {error:#}")),
        }
    }
    if !candidate_errors.is_empty() {
        bail!(
            "all MediaWiki API candidates failed while listing subpages for `{parent_title}` on {}:\n  - {}",
            parsed.domain,
            candidate_errors.join("\n  - ")
        );
    }
    Ok(Vec::new())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubpageQueryTarget {
    namespace_prefix: Option<String>,
    namespace_local_prefix: String,
    main_namespace_prefix: String,
}

impl SubpageQueryTarget {
    fn from_parent_title(parent_title: &str) -> Self {
        let trimmed = parent_title.trim().trim_end_matches('/');
        if let Some((namespace, local_title)) = trimmed.split_once(':') {
            let namespace = namespace.trim();
            let local_title = local_title.trim();
            if !namespace.is_empty() && !local_title.is_empty() {
                return Self {
                    namespace_prefix: Some(namespace.to_string()),
                    namespace_local_prefix: format!("{}/", local_title.trim_end_matches('/')),
                    main_namespace_prefix: format!("{trimmed}/"),
                };
            }
        }
        Self {
            namespace_prefix: None,
            namespace_local_prefix: String::new(),
            main_namespace_prefix: format!("{trimmed}/"),
        }
    }
}

fn mediawiki_query_allpages(
    client: &mut ExternalClient,
    api_url: &str,
    prefix: &str,
    namespace: i32,
    limit: usize,
) -> Result<Vec<String>> {
    let target = limit.max(1);
    let mut titles = Vec::new();
    let mut continuation = None::<String>;

    while titles.len() < target {
        let mut params = vec![
            ("action", "query".to_string()),
            ("list", "allpages".to_string()),
            ("apprefix", prefix.to_string()),
            ("apnamespace", namespace.to_string()),
            (
                "aplimit",
                target.saturating_sub(titles.len()).min(500).to_string(),
            ),
        ];
        if let Some(token) = &continuation {
            params.push(("apcontinue", token.clone()));
        }

        let payload = client.request_json(api_url, &params)?;
        let (page_titles, next_continue) = parse_allpages_payload(&payload);
        titles.extend(page_titles);
        continuation = next_continue;
        if continuation.is_none() {
            break;
        }
    }

    titles.truncate(target);
    Ok(titles)
}

fn parse_allpages_payload(payload: &Value) -> (Vec<String>, Option<String>) {
    let titles = payload
        .get("query")
        .and_then(|value| value.get("allpages"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("title").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let continuation = payload
        .get("continue")
        .and_then(|value| value.get("apcontinue"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    (titles, continuation)
}

#[cfg(test)]
mod tests {
    use super::SubpageQueryTarget;

    #[test]
    fn subpage_query_target_splits_namespace_prefix_for_allpages() {
        let target = SubpageQueryTarget::from_parent_title("Manual:Hooks");

        assert_eq!(target.namespace_prefix.as_deref(), Some("Manual"));
        assert_eq!(target.namespace_local_prefix, "Hooks/");
        assert_eq!(target.main_namespace_prefix, "Manual:Hooks/");

        let main = SubpageQueryTarget::from_parent_title("Main Page");
        assert_eq!(main.namespace_prefix, None);
        assert_eq!(main.namespace_local_prefix, "");
        assert_eq!(main.main_namespace_prefix, "Main Page/");
    }
}
