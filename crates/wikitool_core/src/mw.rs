use anyhow::Result;

use crate::config::{
    DEFAULT_USER_AGENT, ENV_WIKITOOL_USER_AGENT, ENV_WIKITOOL_WIKI_API_URL, WikiConfig,
    env_override_owned,
};
use crate::support::{env_value_u64, env_value_usize};

pub use mediawiki_protocol::*;

/// Resolve Wikitool configuration and environment overrides into the neutral
/// MediaWiki protocol client configuration.
pub fn client_config_from_wikitool_config(config: &WikiConfig) -> MediaWikiClientConfig {
    let api_url_default = config.wiki.api_url.as_deref().unwrap_or("");
    client_config_with_defaults(
        api_url_default,
        &config.user_agent(),
        config.wiki.mark_edits_as_bot,
    )
}

pub fn client_config_from_wikitool_env() -> MediaWikiClientConfig {
    client_config_with_defaults("", DEFAULT_USER_AGENT, false)
}

pub fn client_from_wikitool_config(config: &WikiConfig) -> Result<MediaWikiClient> {
    MediaWikiClient::new(client_config_from_wikitool_config(config))
}

pub fn client_from_wikitool_env() -> Result<MediaWikiClient> {
    MediaWikiClient::new(client_config_from_wikitool_env())
}

fn client_config_with_defaults(
    api_url_default: &str,
    user_agent_default: &str,
    mark_edits_as_bot: bool,
) -> MediaWikiClientConfig {
    MediaWikiClientConfig {
        api_url: env_override_owned(ENV_WIKITOOL_WIKI_API_URL)
            .unwrap_or_else(|| api_url_default.to_string()),
        user_agent: env_override_owned(ENV_WIKITOOL_USER_AGENT)
            .unwrap_or_else(|| user_agent_default.to_string()),
        timeout_ms: env_value_u64("WIKITOOL_HTTP_TIMEOUT_MS", 30_000),
        rate_limit_read_ms: env_value_u64("WIKITOOL_RATE_LIMIT_READ_MS", 300),
        rate_limit_write_ms: env_value_u64("WIKITOOL_RATE_LIMIT_WRITE_MS", 1_000),
        max_retries: env_value_usize("WIKITOOL_HTTP_RETRIES", 2),
        retry_delay_ms: env_value_u64("WIKITOOL_HTTP_RETRY_DELAY_MS", 500),
        mark_edits_as_bot,
    }
}
