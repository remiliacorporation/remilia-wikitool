use anyhow::{Result, bail};

use crate::config::{CustomNamespace, WikiConfig};
use crate::mw::client_from_wikitool_config;

pub fn discover_custom_namespaces(config: &WikiConfig) -> Result<Vec<CustomNamespace>> {
    if config.api_url_owned().is_none() {
        bail!("wiki API URL is not configured (set [wiki].api_url or WIKITOOL_WIKI_API_URL)");
    }
    let mut client = client_from_wikitool_config(config)?;
    Ok(client
        .discover_namespaces()?
        .into_iter()
        .map(|namespace| CustomNamespace {
            folder: Some(namespace.name.clone()),
            id: namespace.id,
            name: namespace.name,
        })
        .collect())
}
