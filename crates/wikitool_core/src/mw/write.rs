use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use reqwest::blocking::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

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
    pub invalid: bool,
    pub status: String,
}

#[derive(Debug, Deserialize, Default)]
struct PurgeResponse {
    #[serde(default)]
    purge: Vec<PurgeItem>,
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

#[derive(Debug, Clone)]
pub struct UploadOptions {
    pub path: PathBuf,
    pub filename: String,
    pub comment: String,
    pub text: Option<String>,
    pub ignore_warnings: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UploadReport {
    pub filename: String,
    pub source_path: String,
    pub bytes: u64,
    pub sha256: String,
    pub comment: String,
    pub ignore_warnings: bool,
    pub request_count: usize,
    pub result: String,
    pub uploaded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_info: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
struct UploadResponse {
    upload: Option<UploadPayload>,
}

#[derive(Debug, Deserialize, Default)]
struct UploadPayload {
    result: Option<String>,
    filename: Option<String>,
    warnings: Option<Value>,
    imageinfo: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct MovePageOptions {
    pub from: String,
    pub to: String,
    pub reason: String,
    pub no_redirect: bool,
    pub move_talk: bool,
    pub move_subpages: bool,
    pub ignore_warnings: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MoveReport {
    pub requested_from: String,
    pub requested_to: String,
    pub from: String,
    pub to: String,
    pub reason: String,
    pub redirect_created: bool,
    pub ignore_warnings: bool,
    pub talk_moved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub talk_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub talk_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Value>,
    pub request_count: usize,
}

#[derive(Debug, Deserialize, Default)]
struct MoveResponse {
    #[serde(rename = "move")]
    move_: Option<MovePayload>,
    warnings: Option<Value>,
}

// Mirrors the MediaWiki action=move response shape; not every field is consumed.
#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct MovePayload {
    from: Option<String>,
    to: Option<String>,
    reason: Option<String>,
    redirectcreated: Option<Value>,
    talkfrom: Option<String>,
    talkto: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct UserInfoResponse {
    #[serde(default)]
    query: UserInfoQuery,
}

#[derive(Debug, Deserialize, Default)]
struct UserInfoQuery {
    #[serde(default)]
    userinfo: UserInfoPayload,
}

#[derive(Debug, Deserialize, Default)]
struct UserInfoPayload {
    #[serde(default)]
    rights: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct UserGroupsResponse {
    #[serde(default)]
    query: UserGroupsQuery,
}

#[derive(Debug, Deserialize, Default)]
struct UserGroupsQuery {
    #[serde(default)]
    usergroups: Vec<UserGroupPayload>,
}

#[derive(Debug, Deserialize, Default)]
struct UserGroupPayload {
    #[serde(default)]
    rights: Vec<String>,
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

impl MediaWikiClient {
    pub fn purge_pages(
        &mut self,
        titles: &[String],
        options: &PurgeOptions,
    ) -> Result<PurgeReport> {
        let normalized_titles = titles
            .iter()
            .map(|title| title.replace('_', " ").trim().to_string())
            .filter(|title| !title.is_empty())
            .collect::<Vec<_>>();
        if normalized_titles.is_empty() {
            bail!("purge requires at least one non-empty title");
        }

        let token = self.ensure_csrf_token()?;
        let mut pages = Vec::new();
        for batch in normalized_titles.chunks(50) {
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
            let parsed: PurgeResponse =
                serde_json::from_value(response).context("failed to decode purge response")?;
            for item in parsed.purge {
                let purged = item.purged.unwrap_or(false);
                let linkupdate = item.linkupdate.unwrap_or(false);
                let missing = item.missing.unwrap_or(false);
                let invalid = item.invalid.unwrap_or(false);
                let status = if invalid {
                    "invalid"
                } else if missing {
                    "missing"
                } else if purged || linkupdate {
                    "purged"
                } else {
                    "unknown"
                };
                pages.push(PurgePageReport {
                    title: item.title,
                    namespace: item.ns,
                    purged,
                    linkupdate,
                    missing,
                    invalid,
                    status: status.to_string(),
                });
            }
        }

        Ok(PurgeReport {
            titles: normalized_titles,
            forcelinkupdate: options.forcelinkupdate,
            forcerecursivelinkupdate: options.forcerecursivelinkupdate,
            request_count: self.request_count(),
            pages,
        })
    }

    pub fn upload_file(&mut self, options: &UploadOptions) -> Result<UploadReport> {
        if options.filename.trim().is_empty() {
            bail!("upload filename must be non-empty");
        }
        let bytes = fs::read(&options.path)
            .with_context(|| format!("failed to read upload source {}", options.path.display()))?;
        let byte_count = u64::try_from(bytes.len()).context("upload source is too large")?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let token = self.ensure_csrf_token()?;

        let filename = options.filename.trim().to_string();
        let comment = options.comment.trim().to_string();
        let text = options.text.clone();
        let ignore_warnings = options.ignore_warnings;
        let file_bytes = bytes.clone();
        let response = self.request_json_multipart_post(
            || {
                let mut form = Form::new()
                    .text("format", "json")
                    .text("formatversion", "2")
                    .text("action", "upload")
                    .text("filename", filename.clone())
                    .text("comment", comment.clone())
                    .text("token", token.clone())
                    .part(
                        "file",
                        Part::bytes(file_bytes.clone()).file_name(filename.clone()),
                    );
                if let Some(text) = &text {
                    form = form.text("text", text.clone());
                }
                if ignore_warnings {
                    form = form.text("ignorewarnings", "1");
                }
                Ok(form)
            },
            true,
        )?;
        let parsed: UploadResponse =
            serde_json::from_value(response).context("failed to decode upload response")?;
        let upload = parsed
            .upload
            .ok_or_else(|| anyhow::anyhow!("missing upload payload in API response"))?;
        let result = upload.result.unwrap_or_else(|| "unknown".to_string());
        let uploaded = result == "Success";

        Ok(UploadReport {
            filename: upload.filename.unwrap_or(filename),
            source_path: options.path.display().to_string(),
            bytes: byte_count,
            sha256,
            comment,
            ignore_warnings,
            request_count: self.request_count(),
            result,
            uploaded,
            warnings: upload.warnings,
            image_info: upload.imageinfo,
        })
    }

    pub fn move_page(&mut self, options: &MovePageOptions) -> Result<MoveReport> {
        let from = options.from.replace('_', " ").trim().to_string();
        let to = options.to.replace('_', " ").trim().to_string();
        if from.is_empty() || to.is_empty() {
            bail!("move requires non-empty from and to titles");
        }
        let reason = options.reason.trim().to_string();

        let rights = self.current_user_rights()?;
        require_move_right(&rights, "move")?;
        if self.site_exposes_right("skip-move-moderation")? {
            require_move_right(&rights, "skip-move-moderation")?;
        }
        if options.no_redirect {
            require_move_right(&rights, "suppressredirect")?;
        }
        if options.move_subpages {
            require_move_right(&rights, "move-subpages")?;
        }

        let token = self.ensure_csrf_token()?;

        let mut params = vec![
            ("action", "move".to_string()),
            ("from", from.clone()),
            ("to", to.clone()),
            ("reason", reason.clone()),
            ("watchlist", "nochange".to_string()),
            ("token", token),
        ];
        if options.no_redirect {
            params.push(("noredirect", "1".to_string()));
        }
        if options.move_talk {
            params.push(("movetalk", "1".to_string()));
        }
        if options.move_subpages {
            params.push(("movesubpages", "1".to_string()));
        }
        if options.ignore_warnings {
            params.push(("ignorewarnings", "1".to_string()));
        }

        let response = self.request_json_post(&params, true)?;
        decode_move_response(
            from,
            to,
            reason,
            options.ignore_warnings,
            response,
            self.request_count(),
        )
    }

    fn current_user_rights(&mut self) -> Result<Vec<String>> {
        let response = self.request_json_get(&[
            ("action", "query".to_string()),
            ("meta", "userinfo".to_string()),
            ("uiprop", "rights".to_string()),
        ])?;
        decode_current_user_rights(response)
    }

    fn site_exposes_right(&mut self, right: &str) -> Result<bool> {
        let response = self.request_json_get(&[
            ("action", "query".to_string()),
            ("meta", "siteinfo".to_string()),
            ("siprop", "usergroups".to_string()),
        ])?;
        decode_site_exposes_right(response, right)
    }
}

fn decode_current_user_rights(response: Value) -> Result<Vec<String>> {
    let parsed: UserInfoResponse =
        serde_json::from_value(response).context("failed to decode user rights response")?;
    Ok(parsed.query.userinfo.rights)
}

fn decode_site_exposes_right(response: Value, right: &str) -> Result<bool> {
    let parsed: UserGroupsResponse =
        serde_json::from_value(response).context("failed to decode site user groups response")?;
    Ok(parsed
        .query
        .usergroups
        .iter()
        .any(|group| group.rights.iter().any(|candidate| candidate == right)))
}

fn require_move_right(rights: &[String], right: &str) -> Result<()> {
    if rights.iter().any(|candidate| candidate == right) {
        return Ok(());
    }
    bail!(
        "current MediaWiki user lacks `{right}` right; refusing action=move because it would not complete immediately"
    );
}

fn decode_move_response(
    requested_from: String,
    requested_to: String,
    reason: String,
    ignore_warnings: bool,
    response: Value,
    request_count: usize,
) -> Result<MoveReport> {
    let parsed: MoveResponse =
        serde_json::from_value(response).context("failed to decode move response")?;
    let payload = parsed
        .move_
        .ok_or_else(|| anyhow::anyhow!("missing move payload in API response"))?;
    let redirect_created = match payload.redirectcreated {
        Some(Value::Bool(value)) => value,
        Some(_) => true,
        None => false,
    };
    let talk_moved = payload.talkto.is_some();
    let from = payload.from.unwrap_or_else(|| requested_from.clone());
    let to = payload.to.unwrap_or_else(|| requested_to.clone());

    Ok(MoveReport {
        requested_from,
        requested_to,
        from,
        to,
        reason: payload.reason.unwrap_or(reason),
        redirect_created,
        ignore_warnings,
        talk_moved,
        talk_from: payload.talkfrom,
        talk_to: payload.talkto,
        warnings: parsed.warnings,
        request_count,
    })
}

#[cfg(test)]
mod move_tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn decodes_move_response_using_api_normalized_titles() {
        let report = decode_move_response(
            "user:codex move test".to_string(),
            "user:codex move target".to_string(),
            "requested reason".to_string(),
            false,
            json!({
                "move": {
                    "from": "User:Codex move test",
                    "to": "User:Codex move target",
                    "reason": "api reason",
                    "redirectcreated": ""
                }
            }),
            4,
        )
        .unwrap();

        assert_eq!(report.requested_from, "user:codex move test");
        assert_eq!(report.requested_to, "user:codex move target");
        assert_eq!(report.from, "User:Codex move test");
        assert_eq!(report.to, "User:Codex move target");
        assert_eq!(report.reason, "api reason");
        assert!(report.redirect_created);
        assert!(!report.talk_moved);
        assert_eq!(report.request_count, 4);
    }

    #[test]
    fn decodes_move_response_talk_and_warnings() {
        let report = decode_move_response(
            "User:Source".to_string(),
            "User:Target".to_string(),
            "reason".to_string(),
            true,
            json!({
                "warnings": {
                    "move": {
                        "*": "warning text"
                    }
                },
                "move": {
                    "from": "User:Source",
                    "to": "User:Target",
                    "talkfrom": "User talk:Source",
                    "talkto": "User talk:Target"
                }
            }),
            7,
        )
        .unwrap();

        assert!(report.ignore_warnings);
        assert!(report.talk_moved);
        assert_eq!(report.talk_from.as_deref(), Some("User talk:Source"));
        assert_eq!(report.talk_to.as_deref(), Some("User talk:Target"));
        assert!(report.warnings.is_some());
    }

    #[test]
    fn decodes_current_user_rights() {
        let rights = decode_current_user_rights(json!({
            "query": {
                "userinfo": {
                    "rights": ["read", "edit", "move"]
                }
            }
        }))
        .unwrap();

        assert_eq!(rights, vec!["read", "edit", "move"]);
        assert!(require_move_right(&rights, "move").is_ok());
        assert!(require_move_right(&rights, "suppressredirect").is_err());
    }

    #[test]
    fn decodes_site_exposed_rights() {
        let has_right = decode_site_exposes_right(
            json!({
                "query": {
                    "usergroups": [
                        { "name": "user", "rights": ["read", "edit"] },
                        { "name": "bot", "rights": ["skip-move-moderation"] }
                    ]
                }
            }),
            "skip-move-moderation",
        )
        .unwrap();

        assert!(has_right);

        let missing = decode_site_exposes_right(
            json!({
                "query": {
                    "usergroups": [
                        { "name": "user", "rights": ["read", "edit"] }
                    ]
                }
            }),
            "skip-move-moderation",
        )
        .unwrap();

        assert!(!missing);
    }
}
