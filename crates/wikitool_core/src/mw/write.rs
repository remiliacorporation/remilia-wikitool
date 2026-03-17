use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::auth::{LoginResponse, TokenQueryResponse};
use super::client::{MediaWikiClient, PageTimestampInfo, RemotePage, WikiReadApi, WikiWriteApi};

#[derive(Debug, Deserialize, Default)]
struct QueryResponse {
    #[serde(default)]
    query: QueryPayload,
}

#[derive(Debug, Deserialize, Default)]
struct QueryPayload {
    #[serde(default)]
    pages: Vec<PageQueryItem>,
}

#[derive(Debug, Deserialize)]
struct PageQueryItem {
    title: String,
    missing: Option<bool>,
    #[serde(default)]
    revisions: Vec<RevisionQueryItem>,
}

#[derive(Debug, Deserialize)]
struct RevisionQueryItem {
    revid: i64,
    timestamp: String,
}

#[derive(Debug, Deserialize, Default)]
struct EditResponse {
    edit: Option<EditPayload>,
}

#[derive(Debug, Deserialize, Default)]
struct EditPayload {
    result: Option<String>,
}

impl WikiWriteApi for MediaWikiClient {
    fn login(&mut self, username: &str, password: &str) -> Result<()> {
        let token_response = self.request_json_get(&[
            ("action", "query".to_string()),
            ("meta", "tokens".to_string()),
            ("type", "login".to_string()),
        ])?;
        let token_payload: TokenQueryResponse = serde_json::from_value(token_response)
            .context("failed to decode login token response")?;
        let login_token = token_payload
            .query
            .tokens
            .as_ref()
            .and_then(|tokens| tokens.logintoken.as_ref())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("failed to get MediaWiki login token"))?;

        let login_response = self.request_json_post(
            &[
                ("action", "login".to_string()),
                ("lgname", username.to_string()),
                ("lgpassword", password.to_string()),
                ("lgtoken", login_token),
            ],
            true,
        )?;
        let login_payload: LoginResponse =
            serde_json::from_value(login_response).context("failed to decode login response")?;
        match login_payload.login.result.as_deref() {
            Some("Success") => {
                self.csrf_token = None;
                Ok(())
            }
            other => bail!(
                "MediaWiki login failed: {}",
                login_payload
                    .login
                    .reason
                    .or_else(|| other.map(ToString::to_string))
                    .unwrap_or_else(|| "unknown error".to_string())
            ),
        }
    }

    fn get_page_timestamps(&mut self, titles: &[String]) -> Result<Vec<PageTimestampInfo>> {
        let mut output = Vec::new();
        for batch in titles.chunks(50) {
            let response = self.request_json_get(&[
                ("action", "query".to_string()),
                ("titles", batch.join("|")),
                ("prop", "revisions".to_string()),
                ("rvprop", "timestamp|ids".to_string()),
            ])?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode page timestamp response")?;
            for page in parsed.query.pages {
                if page.missing.unwrap_or(false) {
                    continue;
                }
                let revision = match page.revisions.first() {
                    Some(revision) => revision,
                    None => continue,
                };
                output.push(PageTimestampInfo {
                    title: page.title,
                    timestamp: revision.timestamp.clone(),
                    revision_id: revision.revid,
                });
            }
        }
        Ok(output)
    }

    fn edit_page(&mut self, title: &str, content: &str, summary: &str) -> Result<RemotePage> {
        let token = self.ensure_csrf_token()?;
        let response = self.request_json_post(
            &[
                ("action", "edit".to_string()),
                ("title", title.to_string()),
                ("text", content.to_string()),
                ("summary", summary.to_string()),
                ("bot", "1".to_string()),
                ("token", token),
            ],
            true,
        )?;
        let edit_payload: EditResponse =
            serde_json::from_value(response).context("failed to decode edit response")?;
        let edit = edit_payload
            .edit
            .ok_or_else(|| anyhow::anyhow!("missing edit payload in API response"))?;
        if edit.result.as_deref() != Some("Success") {
            bail!(
                "MediaWiki edit failed for {}: {}",
                title,
                edit.result.unwrap_or_else(|| "unknown".to_string())
            );
        }

        let page = self.get_page_contents(&[title.to_string()])?;
        page.into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("edited page not returned by API: {title}"))
    }

    fn delete_page(&mut self, title: &str, reason: &str) -> Result<()> {
        let token = self.ensure_csrf_token()?;
        let response = self.request_json_post(
            &[
                ("action", "delete".to_string()),
                ("title", title.to_string()),
                ("reason", reason.to_string()),
                ("token", token),
            ],
            true,
        );

        match response {
            Ok(_) => Ok(()),
            Err(error) => {
                let message = error.to_string();
                if message.contains("missingtitle") {
                    return Ok(());
                }
                Err(error)
            }
        }
    }
}
