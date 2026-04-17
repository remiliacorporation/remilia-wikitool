use anyhow::Result;
use serde_json::Value;

use crate::research::web_fetch::ExternalClient;

pub(super) fn mediawiki_query_namespace_id(
    client: &mut ExternalClient,
    api_url: &str,
    namespace_prefix: &str,
) -> Result<Option<i32>> {
    let payload = client.request_json(
        api_url,
        &[
            ("action", "query".to_string()),
            ("meta", "siteinfo".to_string()),
            ("siprop", "namespaces|namespacealiases".to_string()),
        ],
    )?;
    Ok(parse_namespace_id(&payload, namespace_prefix))
}

fn parse_namespace_id(payload: &Value, namespace_prefix: &str) -> Option<i32> {
    let target = normalize_namespace_label(namespace_prefix);
    if target.is_empty() {
        return Some(0);
    }

    if let Some(namespaces) = payload
        .get("query")
        .and_then(|value| value.get("namespaces"))
        .and_then(Value::as_object)
    {
        for (key, namespace) in namespaces {
            let Some(id) = namespace
                .get("id")
                .and_then(Value::as_i64)
                .or_else(|| key.parse::<i64>().ok())
            else {
                continue;
            };
            let matches_name = namespace
                .get("*")
                .and_then(Value::as_str)
                .is_some_and(|value| normalize_namespace_label(value) == target)
                || namespace
                    .get("canonical")
                    .and_then(Value::as_str)
                    .is_some_and(|value| normalize_namespace_label(value) == target);
            if matches_name {
                return i32::try_from(id).ok();
            }
        }
    }

    if let Some(aliases) = payload
        .get("query")
        .and_then(|value| value.get("namespacealiases"))
        .and_then(Value::as_array)
    {
        for alias in aliases {
            let alias_name = alias.get("*").and_then(Value::as_str);
            if alias_name.is_some_and(|value| normalize_namespace_label(value) == target) {
                return alias
                    .get("id")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok());
            }
        }
    }

    None
}

fn normalize_namespace_label(value: &str) -> String {
    value.replace('_', " ").trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_namespace_id;

    #[test]
    fn parse_namespace_id_matches_canonical_names_and_aliases() {
        let payload = json!({
            "query": {
                "namespaces": {
                    "0": { "id": 0, "*": "" },
                    "100": { "id": 100, "*": "Manual", "canonical": "Manual" }
                },
                "namespacealiases": [
                    { "id": 100, "*": "Man" }
                ]
            }
        });

        assert_eq!(parse_namespace_id(&payload, "Manual"), Some(100));
        assert_eq!(parse_namespace_id(&payload, "manual"), Some(100));
        assert_eq!(parse_namespace_id(&payload, "Man"), Some(100));
        assert_eq!(parse_namespace_id(&payload, "Unknown"), None);
    }
}
