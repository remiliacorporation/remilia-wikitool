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
}
