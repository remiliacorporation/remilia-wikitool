use anyhow::{Context, Result};
use serde::Deserialize;

use super::client::MediaWikiClient;

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TokenQueryResponse {
    #[serde(default)]
    pub(crate) query: TokenQueryPayload,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TokenQueryPayload {
    pub(crate) tokens: Option<TokenPayload>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TokenPayload {
    pub(crate) logintoken: Option<String>,
    pub(crate) csrftoken: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct LoginResponse {
    #[serde(default)]
    pub(crate) login: LoginPayload,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct LoginPayload {
    pub(crate) result: Option<String>,
    pub(crate) reason: Option<String>,
}

impl MediaWikiClient {
    pub(crate) fn ensure_csrf_token(&mut self) -> Result<String> {
        if let Some(token) = &self.csrf_token {
            return Ok(token.clone());
        }
        let response = self.request_json_get(&[
            ("action", "query".to_string()),
            ("meta", "tokens".to_string()),
        ])?;
        let parsed: TokenQueryResponse =
            serde_json::from_value(response).context("failed to decode csrf token response")?;
        let token = parsed
            .query
            .tokens
            .as_ref()
            .and_then(|tokens| tokens.csrftoken.as_ref())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("failed to get MediaWiki csrf token"))?;
        self.csrf_token = Some(token.clone());
        Ok(token)
    }
}
