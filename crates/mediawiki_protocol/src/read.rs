use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::client::{MediaWikiClient, RemotePage, RevisionLineageEntry, WikiReadApi};

#[derive(Debug, Deserialize, Default)]
struct QueryResponse {
    #[serde(default)]
    query: QueryPayload,
    #[serde(default, rename = "continue")]
    continuation: Option<ContinuationPayload>,
}

#[derive(Debug, Deserialize, Default)]
struct QueryPayload {
    #[serde(default)]
    allpages: Vec<TitleQueryItem>,
    #[serde(default)]
    categorymembers: Vec<TitleQueryItem>,
    #[serde(default)]
    recentchanges: Vec<RecentChangeItem>,
    #[serde(default)]
    pages: Vec<PageQueryItem>,
}

#[derive(Debug, Deserialize, Default)]
struct ContinuationPayload {
    apcontinue: Option<String>,
    cmcontinue: Option<String>,
    rccontinue: Option<String>,
    rvcontinue: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TitleQueryItem {
    title: String,
}

#[derive(Debug, Deserialize)]
struct RecentChangeItem {
    title: String,
}

#[derive(Debug, Deserialize)]
struct PageQueryItem {
    pageid: Option<i64>,
    ns: i32,
    title: String,
    missing: Option<bool>,
    #[serde(default)]
    revisions: Vec<RevisionQueryItem>,
}

#[derive(Debug, Deserialize)]
struct RevisionQueryItem {
    revid: i64,
    timestamp: String,
    comment: Option<String>,
    commenthidden: Option<bool>,
    slots: Option<RevisionSlotContainer>,
}

#[derive(Debug, Deserialize)]
struct RevisionSlotContainer {
    main: Option<RevisionMainSlot>,
}

#[derive(Debug, Deserialize)]
struct RevisionMainSlot {
    content: String,
}

impl WikiReadApi for MediaWikiClient {
    fn target_api_url(&self) -> &str {
        &self.config.api_url
    }

    fn get_all_pages(&mut self, namespace: i32) -> Result<Vec<String>> {
        let mut titles = Vec::new();
        let mut continue_token = None::<String>;

        loop {
            let mut params = vec![
                ("action", "query".to_string()),
                ("list", "allpages".to_string()),
                ("apnamespace", namespace.to_string()),
                ("aplimit", "500".to_string()),
            ];
            if let Some(token) = &continue_token {
                params.push(("apcontinue", token.clone()));
            }

            let response = self.query_json(&params)?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode allpages API response")?;

            for item in parsed.query.allpages {
                titles.push(item.title);
            }

            continue_token = parsed.continuation.and_then(|cont| cont.apcontinue);
            if continue_token.is_none() {
                break;
            }
        }

        Ok(titles)
    }

    fn get_category_members(&mut self, category: &str) -> Result<Vec<String>> {
        let mut titles = Vec::new();
        let mut continue_token = None::<String>;
        let category_title = if category.starts_with("Category:") {
            category.to_string()
        } else {
            format!("Category:{category}")
        };

        loop {
            let mut params = vec![
                ("action", "query".to_string()),
                ("list", "categorymembers".to_string()),
                ("cmtitle", category_title.clone()),
                ("cmtype", "page".to_string()),
                ("cmlimit", "500".to_string()),
            ];
            if let Some(token) = &continue_token {
                params.push(("cmcontinue", token.clone()));
            }

            let response = self.query_json(&params)?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode categorymembers API response")?;
            for item in parsed.query.categorymembers {
                titles.push(item.title);
            }

            continue_token = parsed.continuation.and_then(|cont| cont.cmcontinue);
            if continue_token.is_none() {
                break;
            }
        }

        Ok(titles)
    }

    fn get_recent_changes(&mut self, since: &str, namespaces: &[i32]) -> Result<Vec<String>> {
        let mut titles = BTreeSet::new();
        let mut continue_token = None::<String>;
        let namespace_filter = namespaces
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("|");

        loop {
            let mut params = vec![
                ("action", "query".to_string()),
                ("list", "recentchanges".to_string()),
                ("rcstart", since.to_string()),
                ("rcdir", "newer".to_string()),
                ("rcnamespace", namespace_filter.clone()),
                ("rcprop", "title".to_string()),
                ("rclimit", "500".to_string()),
                ("rctype", "edit|new".to_string()),
            ];
            if let Some(token) = &continue_token {
                params.push(("rccontinue", token.clone()));
            }

            let response = self.query_json(&params)?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode recentchanges API response")?;
            for item in parsed.query.recentchanges {
                titles.insert(item.title);
            }
            continue_token = parsed.continuation.and_then(|cont| cont.rccontinue);
            if continue_token.is_none() {
                break;
            }
        }

        Ok(titles.into_iter().collect())
    }

    fn get_page_contents(&mut self, titles: &[String]) -> Result<Vec<RemotePage>> {
        let mut results = Vec::new();
        for batch in titles.chunks(50) {
            let params = vec![
                ("action", "query".to_string()),
                ("titles", batch.join("|")),
                ("prop", "revisions".to_string()),
                ("rvprop", "content|timestamp|ids".to_string()),
                ("rvslots", "main".to_string()),
            ];

            let response = self.query_json(&params)?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode page content API response")?;

            for page in parsed.query.pages {
                if page.missing.unwrap_or(false) {
                    continue;
                }
                let revision = match page.revisions.first() {
                    Some(revision) => revision,
                    None => continue,
                };
                let slot = match revision
                    .slots
                    .as_ref()
                    .and_then(|slots| slots.main.as_ref())
                {
                    Some(slot) => slot,
                    None => continue,
                };
                let page_id = match page.pageid {
                    Some(value) => value,
                    None => continue,
                };

                results.push(RemotePage {
                    title: page.title,
                    namespace: page.ns,
                    page_id,
                    revision_id: revision.revid,
                    timestamp: revision.timestamp.clone(),
                    content: slot.content.clone(),
                });
            }
        }
        Ok(results)
    }

    fn get_revision_by_id(&mut self, revision_id: i64) -> Result<Option<RemotePage>> {
        let response = self.query_json(&[
            ("action", "query".to_string()),
            ("revids", revision_id.to_string()),
            ("prop", "revisions".to_string()),
            ("rvprop", "content|timestamp|ids".to_string()),
            ("rvslots", "main".to_string()),
        ])?;
        let parsed: QueryResponse = serde_json::from_value(response)
            .context("failed to decode exact revision content response")?;

        for page in parsed.query.pages {
            if page.missing.unwrap_or(false) {
                continue;
            }
            let Some(revision) = page
                .revisions
                .iter()
                .find(|revision| revision.revid == revision_id)
            else {
                continue;
            };
            let slot = revision
                .slots
                .as_ref()
                .and_then(|slots| slots.main.as_ref())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "exact revision {revision_id} did not return readable main-slot content"
                    )
                })?;
            let page_id = page.pageid.ok_or_else(|| {
                anyhow::anyhow!("exact revision {revision_id} did not return a page ID")
            })?;
            if page.title.trim().is_empty() {
                bail!("exact revision {revision_id} returned an empty page title");
            }
            return Ok(Some(RemotePage {
                title: page.title,
                namespace: page.ns,
                page_id,
                revision_id: revision.revid,
                timestamp: revision.timestamp.clone(),
                content: slot.content.clone(),
            }));
        }

        Ok(None)
    }

    fn get_revision_lineage(&mut self, title: &str) -> Result<Vec<RevisionLineageEntry>> {
        if title.trim().is_empty() {
            bail!("revision-lineage lookup requires a non-empty title");
        }
        let mut lineage = Vec::new();
        let mut continue_token = None::<String>;
        loop {
            let mut params = vec![
                ("action", "query".to_string()),
                ("titles", title.to_string()),
                ("prop", "revisions".to_string()),
                ("rvslots", "main".to_string()),
                ("rvprop", "ids|timestamp|comment|content".to_string()),
                ("rvlimit", "max".to_string()),
            ];
            if let Some(token) = &continue_token {
                params.push(("rvcontinue", token.clone()));
            }
            let response = self.query_json(&params)?;
            let parsed: QueryResponse = serde_json::from_value(response)
                .context("failed to decode revision-lineage response")?;
            for page in parsed.query.pages {
                if page.missing.unwrap_or(false) {
                    continue;
                }
                let page_id = page.pageid.ok_or_else(|| {
                    anyhow::anyhow!(
                        "revision-lineage response omitted page ID for {}",
                        page.title
                    )
                })?;
                for revision in page.revisions {
                    let content = revision
                        .slots
                        .and_then(|slots| slots.main)
                        .map(|slot| slot.content);
                    lineage.push(RevisionLineageEntry {
                        title: page.title.clone(),
                        page_id,
                        revision_id: revision.revid,
                        timestamp: revision.timestamp,
                        comment: revision.comment.filter(|value| !value.trim().is_empty()),
                        comment_hidden: revision.commenthidden.unwrap_or(false),
                        content,
                    });
                }
            }
            continue_token = parsed.continuation.and_then(|item| item.rvcontinue);
            if continue_token.is_none() {
                return Ok(lineage);
            }
        }
    }

    fn request_count(&self) -> usize {
        self.request_count
    }
}
