use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use super::client::MediaWikiClient;
use super::namespace::{namespace_display_name, should_include_discovered_namespace};

#[derive(Debug, Deserialize, Default)]
struct SiteInfoResponse {
    #[serde(default)]
    query: SiteInfoQueryPayload,
}

#[derive(Debug, Deserialize, Default)]
struct SiteInfoQueryPayload {
    #[serde(default)]
    namespaces: BTreeMap<String, SiteInfoNamespace>,
}

#[derive(Debug, Deserialize, Default)]
pub struct SiteInfoNamespace {
    #[serde(default)]
    pub id: i32,
    #[serde(default, rename = "*")]
    pub star_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub canonical: Option<String>,
    #[serde(default)]
    pub content: Option<Value>,
}

impl MediaWikiClient {
    pub(crate) fn discover_custom_namespaces(
        &mut self,
    ) -> Result<Vec<crate::config::CustomNamespace>> {
        let payload = self.request_json_get(&[
            ("action", "query".to_string()),
            ("meta", "siteinfo".to_string()),
            ("siprop", "namespaces".to_string()),
        ])?;
        let parsed: SiteInfoResponse = serde_json::from_value(payload)
            .context("failed to decode namespace discovery response")?;

        let mut namespaces = Vec::new();
        for (key, mut namespace) in parsed.query.namespaces {
            if namespace.id == 0
                && let Ok(parsed_id) = key.parse::<i32>()
                && parsed_id != 0
            {
                namespace.id = parsed_id;
            }
            if !should_include_discovered_namespace(&namespace) {
                continue;
            }
            let Some(name) = namespace_display_name(&namespace) else {
                continue;
            };
            namespaces.push(crate::config::CustomNamespace {
                folder: Some(name.clone()),
                id: namespace.id,
                name,
            });
        }
        namespaces.sort_by_key(|namespace| namespace.id);
        namespaces.dedup_by_key(|namespace| namespace.id);
        Ok(namespaces)
    }
}
