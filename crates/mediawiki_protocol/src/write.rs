use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::auth::{LoginResponse, TokenQueryResponse};
use super::client::{
    DeleteLogEntry, DeleteOutcome, DeleteReceipt, EditConstraint, EditReceipt, MediaWikiApiError,
    MediaWikiClient, PageTimestampInfo, WikiReadApi, WikiWriteApi,
};

#[derive(Debug, Deserialize, Default)]
struct QueryResponse {
    #[serde(default)]
    query: QueryPayload,
    #[serde(default, rename = "continue")]
    continuation: Option<DeleteLogContinuation>,
}

#[derive(Debug, Deserialize)]
struct DeleteLogContinuation {
    lecontinue: String,
}

#[derive(Debug, Deserialize, Default)]
struct QueryPayload {
    #[serde(default)]
    pages: Vec<PageQueryItem>,
    #[serde(default)]
    logevents: Vec<DeleteLogEventItem>,
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

#[derive(Debug, Deserialize)]
struct DeleteLogEventItem {
    logid: i64,
    title: String,
    timestamp: String,
    #[serde(rename = "type")]
    event_type: String,
    action: String,
    comment: Option<String>,
    commenthidden: Option<bool>,
    user: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct EditResponse {
    edit: Option<EditPayload>,
}

#[derive(Debug, Deserialize, Default)]
struct EditPayload {
    result: Option<String>,
    title: Option<String>,
    pageid: Option<i64>,
    oldrevid: Option<i64>,
    newrevid: Option<i64>,
    newtimestamp: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct DeleteResponse {
    delete: Option<DeletePayload>,
}

#[derive(Debug, Deserialize)]
struct DeletePayload {
    title: String,
    logid: i64,
}

#[derive(Debug, Clone)]
pub struct PurgeOptions {
    pub forcelinkupdate: bool,
    pub forcerecursivelinkupdate: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PurgeReport {
    pub titles: Vec<String>,
    pub forcelinkupdate: bool,
    pub forcerecursivelinkupdate: bool,
    pub request_count: usize,
    pub pages: Vec<PurgePageReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PurgePageReport {
    pub title: String,
    pub namespace: Option<i32>,
    pub purged: bool,
    pub linkupdate: bool,
    pub missing: bool,
    pub status: String,
}

#[derive(Debug, Deserialize, Default)]
struct PurgeResponse {
    #[serde(default)]
    purge: Vec<PurgeItem>,
    #[serde(default)]
    normalized: Vec<NormalizedTitle>,
}

#[derive(Debug, Deserialize)]
struct NormalizedTitle {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize, Default)]
struct PurgeItem {
    ns: Option<i32>,
    title: String,
    purged: Option<bool>,
    linkupdate: Option<bool>,
    missing: Option<bool>,
    invalid: Option<bool>,
}

impl WikiWriteApi for MediaWikiClient {
    fn login(&mut self, username: &str, password: &str) -> Result<()> {
        let token_response = self.query_json(&[
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
            let response = self.query_json(&[
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

    fn edit_page(
        &mut self,
        title: &str,
        content: &str,
        summary: &str,
        constraint: EditConstraint,
    ) -> Result<EditReceipt> {
        let token = self.ensure_csrf_token()?;
        let params = build_edit_parameters(
            title,
            content,
            summary,
            &token,
            constraint,
            self.config.mark_edits_as_bot,
        );
        let response = self.request_json_post(&params, true)?;
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

        Ok(EditReceipt {
            title: edit
                .title
                .ok_or_else(|| anyhow::anyhow!("edit response omitted title for {title}"))?,
            page_id: edit
                .pageid
                .ok_or_else(|| anyhow::anyhow!("edit response omitted pageid for {title}"))?,
            old_revision_id: edit
                .oldrevid
                .ok_or_else(|| anyhow::anyhow!("edit response omitted oldrevid for {title}"))?,
            new_revision_id: edit
                .newrevid
                .ok_or_else(|| anyhow::anyhow!("edit response omitted newrevid for {title}"))?,
            new_timestamp: edit
                .newtimestamp
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("edit response omitted newtimestamp for {title}"))?,
        })
    }

    fn delete_page(&mut self, title: &str, reason: &str) -> Result<DeleteOutcome> {
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
            Ok(response) => decode_delete_response(response, title),
            Err(error) => {
                if error
                    .downcast_ref::<MediaWikiApiError>()
                    .is_some_and(|error| error.code() == "missingtitle")
                {
                    return Ok(DeleteOutcome::AlreadyMissing);
                }
                Err(error)
            }
        }
    }

    fn get_delete_log_entries(&mut self, title: &str) -> Result<Vec<DeleteLogEntry>> {
        if title.trim().is_empty() {
            bail!("delete-log lookup requires a non-empty title");
        }
        let mut entries = Vec::new();
        let mut continuation = None::<String>;
        loop {
            let mut params = vec![
                ("action", "query".to_string()),
                ("list", "logevents".to_string()),
                ("leaction", "delete/delete".to_string()),
                ("letitle", title.to_string()),
                (
                    "leprop",
                    "ids|title|timestamp|comment|type|user".to_string(),
                ),
                ("lelimit", "max".to_string()),
            ];
            if let Some(token) = &continuation {
                params.push(("lecontinue", token.clone()));
            }
            let (page, next) = decode_delete_log_response(self.query_json(&params)?)?;
            entries.extend(page);
            continuation = next;
            if continuation.is_none() {
                return Ok(entries);
            }
        }
    }
}

impl MediaWikiClient {
    pub fn purge_pages(
        &mut self,
        titles: &[String],
        options: &PurgeOptions,
    ) -> Result<PurgeReport> {
        let titles = normalize_unique_titles(titles)?;
        let token = self.ensure_csrf_token()?;
        let mut pages = Vec::with_capacity(titles.len());

        for batch in titles.chunks(50) {
            let mut params = vec![
                ("action", "purge".to_string()),
                ("titles", batch.join("|")),
                ("token", token.clone()),
            ];
            if options.forcelinkupdate {
                params.push(("forcelinkupdate", "1".to_string()));
            }
            if options.forcerecursivelinkupdate {
                params.push(("forcerecursivelinkupdate", "1".to_string()));
            }

            let response = self.request_json_post(&params, true)?;
            pages.extend(decode_purge_response(batch, response)?);
        }

        Ok(PurgeReport {
            titles,
            forcelinkupdate: options.forcelinkupdate,
            forcerecursivelinkupdate: options.forcerecursivelinkupdate,
            request_count: self.request_count(),
            pages,
        })
    }
}

fn normalize_unique_titles(titles: &[String]) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(titles.len());
    let mut identities = HashSet::with_capacity(titles.len());
    for title in titles {
        let title = normalize_title(title);
        if title.is_empty() {
            bail!("purge titles must be non-empty");
        }
        let identity = canonical_title_identity(&title);
        if !identities.insert(identity) {
            bail!("purge titles must be unique; duplicate title `{title}`");
        }
        normalized.push(title);
    }
    if normalized.is_empty() {
        bail!("purge requires at least one title");
    }
    Ok(normalized)
}

fn decode_purge_response(requested: &[String], response: Value) -> Result<Vec<PurgePageReport>> {
    let parsed: PurgeResponse =
        serde_json::from_value(response).context("failed to decode purge response")?;
    if parsed.purge.len() != requested.len() {
        bail!(
            "purge response returned {} page result(s) for {} requested title(s)",
            parsed.purge.len(),
            requested.len()
        );
    }

    let mut expected = requested
        .iter()
        .map(|title| canonical_title_identity(title))
        .collect::<HashSet<_>>();
    for mapping in parsed.normalized {
        let from = canonical_title_identity(&mapping.from);
        if !expected.remove(&from) {
            bail!(
                "purge response normalized unknown or duplicate title {:?}",
                mapping.from
            );
        }
        let to = canonical_title_identity(&mapping.to);
        if !expected.insert(to) {
            bail!(
                "purge response normalized multiple titles to {:?}",
                mapping.to
            );
        }
    }

    let mut pages = Vec::with_capacity(requested.len());
    for item in parsed.purge {
        let identity = canonical_title_identity(&item.title);
        if !expected.remove(&identity) {
            bail!(
                "purge response returned unknown or duplicate title {:?}",
                item.title
            );
        }
        if item.invalid.unwrap_or(false) {
            bail!("purge response marked title {:?} invalid", item.title);
        }

        let purged = item.purged.unwrap_or(false);
        let linkupdate = item.linkupdate.unwrap_or(false);
        let missing = item.missing.unwrap_or(false);
        let status = if missing {
            "missing"
        } else if purged || linkupdate {
            "purged"
        } else {
            bail!(
                "purge response did not prove an outcome for {:?}",
                item.title
            );
        };
        pages.push(PurgePageReport {
            title: item.title,
            namespace: item.ns,
            purged,
            linkupdate,
            missing,
            status: status.to_string(),
        });
    }
    if !expected.is_empty() {
        bail!(
            "purge response omitted {} requested title(s)",
            expected.len()
        );
    }
    Ok(pages)
}

fn decode_delete_response(response: Value, requested_title: &str) -> Result<DeleteOutcome> {
    let parsed: DeleteResponse =
        serde_json::from_value(response).context("failed to decode delete response")?;
    let payload = parsed
        .delete
        .ok_or_else(|| anyhow::anyhow!("missing delete payload in API response"))?;
    if payload.title.trim().is_empty() {
        bail!("delete response omitted the deleted page title");
    }
    if payload.logid <= 0 {
        bail!("delete response returned invalid logid {}", payload.logid);
    }
    let requested = normalize_title(requested_title);
    let returned = normalize_title(&payload.title);
    if returned != requested {
        bail!(
            "delete response title {:?} does not match requested title {:?}",
            payload.title,
            requested_title
        );
    }
    Ok(DeleteOutcome::Deleted(DeleteReceipt {
        title: payload.title,
        log_id: payload.logid,
    }))
}

fn decode_delete_log_response(response: Value) -> Result<(Vec<DeleteLogEntry>, Option<String>)> {
    let parsed: QueryResponse =
        serde_json::from_value(response).context("failed to decode delete-log response")?;
    let continuation = parsed.continuation.map(|item| item.lecontinue);
    let entries = parsed
        .query
        .logevents
        .into_iter()
        .map(|item| {
            if item.logid <= 0 {
                bail!("delete-log response returned invalid logid {}", item.logid);
            }
            if item.title.trim().is_empty() || item.timestamp.trim().is_empty() {
                bail!("delete-log response omitted title or timestamp");
            }
            if item.event_type != "delete" || item.action != "delete" {
                bail!(
                    "delete-log response returned unexpected event {}/{}",
                    item.event_type,
                    item.action
                );
            }
            Ok(DeleteLogEntry {
                log_id: item.logid,
                title: item.title,
                timestamp: item.timestamp,
                comment: item.comment.filter(|value| !value.trim().is_empty()),
                comment_hidden: item.commenthidden.unwrap_or(false),
                user: item.user.filter(|value| !value.trim().is_empty()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((entries, continuation))
}

fn normalize_title(title: &str) -> String {
    title.replace('_', " ").trim().to_string()
}

fn canonical_title_identity(title: &str) -> String {
    normalize_title(title)
}

fn build_edit_parameters(
    title: &str,
    content: &str,
    summary: &str,
    token: &str,
    constraint: EditConstraint,
    mark_as_bot: bool,
) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("action", "edit".to_string()),
        ("title", title.to_string()),
        ("text", content.to_string()),
        ("summary", summary.to_string()),
        ("token", token.to_string()),
    ];
    if mark_as_bot {
        params.push(("bot", "1".to_string()));
    }
    match constraint {
        EditConstraint::CreateOnly => params.push(("createonly", "1".to_string())),
        EditConstraint::ExistingRevision { revision_id } => {
            params.push(("baserevid", revision_id.to_string()));
        }
    }
    params
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn new_page_edit_is_create_only_and_not_bot_marked_by_default() {
        let params = build_edit_parameters(
            "New page",
            "body",
            "summary",
            "token",
            EditConstraint::CreateOnly,
            false,
        );
        assert!(
            params
                .iter()
                .any(|(key, value)| *key == "createonly" && value == "1")
        );
        assert!(!params.iter().any(|(key, _)| *key == "baserevid"));
        assert!(!params.iter().any(|(key, _)| *key == "bot"));
    }

    #[test]
    fn existing_page_edit_binds_revision_and_bot_marker_is_explicit() {
        let params = build_edit_parameters(
            "Existing page",
            "body",
            "summary",
            "token",
            EditConstraint::ExistingRevision { revision_id: 4242 },
            true,
        );
        assert!(
            params
                .iter()
                .any(|(key, value)| *key == "baserevid" && value == "4242")
        );
        assert!(!params.iter().any(|(key, _)| *key == "createonly"));
        assert!(
            params
                .iter()
                .any(|(key, value)| *key == "bot" && value == "1")
        );
    }

    #[test]
    fn decodes_official_delete_success_payload() {
        let outcome = decode_delete_response(
            json!({"delete": {"title": "Sample Page", "logid": 42}}),
            "Sample_Page",
        )
        .expect("delete response");
        assert_eq!(
            outcome,
            DeleteOutcome::Deleted(DeleteReceipt {
                title: "Sample Page".to_string(),
                log_id: 42,
            })
        );
    }

    #[test]
    fn decodes_delete_log_lineage_for_reconciliation() {
        let (entries, continuation) = decode_delete_log_response(json!({
            "query": {
                "logevents": [{
                    "logid": 43,
                    "title": "Sample Page",
                    "timestamp": "2026-08-28T12:00:00Z",
                    "type": "delete",
                    "action": "delete",
                    "comment": "[wikitool-delete:7] cleanup",
                    "user": "WikiBot"
                }]
            }
        }))
        .expect("delete log response");
        assert!(continuation.is_none());
        assert_eq!(
            entries,
            vec![DeleteLogEntry {
                log_id: 43,
                title: "Sample Page".to_string(),
                timestamp: "2026-08-28T12:00:00Z".to_string(),
                comment: Some("[wikitool-delete:7] cleanup".to_string()),
                comment_hidden: false,
                user: Some("WikiBot".to_string()),
            }]
        );
    }

    #[test]
    fn rejects_non_delete_log_lineage() {
        let error = decode_delete_log_response(json!({
            "query": {
                "logevents": [{
                    "logid": 43,
                    "title": "Sample Page",
                    "timestamp": "2026-08-28T12:00:00Z",
                    "type": "delete",
                    "action": "restore"
                }]
            }
        }))
        .expect_err("non-delete log entry must fail closed");
        assert!(error.to_string().contains("unexpected event"));
    }

    #[test]
    fn rejects_error_free_json_without_verified_delete_payload() {
        let error = decode_delete_response(json!({"batchcomplete": true}), "Sample Page")
            .expect_err("malformed success must fail");
        assert!(error.to_string().contains("missing delete payload"));
    }

    #[test]
    fn rejects_delete_payload_without_positive_log_identity() {
        let error = decode_delete_response(
            json!({"delete": {"title": "Sample Page", "logid": 0}}),
            "Sample Page",
        )
        .expect_err("invalid log identity must fail");
        assert!(error.to_string().contains("invalid logid"));
    }

    #[test]
    fn delete_receipt_rejects_case_drift_after_the_initial_character() {
        let error =
            decode_delete_response(json!({"delete": {"title": "ALPHA", "logid": 42}}), "Alpha")
                .expect_err("distinct MediaWiki title identity must not be accepted");
        assert!(error.to_string().contains("does not match requested title"));
    }

    #[test]
    fn purge_requires_one_proven_result_per_requested_title() {
        let pages = decode_purge_response(
            &["Main_Page".to_string(), "NonexistentArticle".to_string()],
            json!({
                "purge": [
                    {"ns": 0, "title": "NonexistentArticle", "missing": true},
                    {"ns": 0, "title": "Main Page", "purged": true}
                ],
                "normalized": [{"from": "Main_Page", "to": "Main Page"}]
            }),
        )
        .expect("strict purge response");
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].status, "missing");
        assert_eq!(pages[1].status, "purged");
    }

    #[test]
    fn purge_rejects_incomplete_unknown_and_mismatched_results() {
        assert!(
            decode_purge_response(
                &["A".to_string(), "B".to_string()],
                json!({"purge": [{"title": "A", "purged": true}]}),
            )
            .is_err()
        );
        assert!(
            decode_purge_response(&["A".to_string()], json!({"purge": [{"title": "A"}]}),).is_err()
        );
        assert!(
            decode_purge_response(
                &["A".to_string()],
                json!({"purge": [{"title": "B", "purged": true}]}),
            )
            .is_err()
        );
    }

    #[test]
    fn purge_identity_preserves_case_after_the_initial_character() {
        let pages = decode_purge_response(
            &["Alpha".to_string(), "ALPHA".to_string()],
            json!({
                "purge": [
                    {"title": "ALPHA", "purged": true},
                    {"title": "Alpha", "purged": true}
                ]
            }),
        )
        .expect("case-distinct titles should bind independently");
        assert_eq!(pages.len(), 2);
    }
}
