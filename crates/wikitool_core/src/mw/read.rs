use std::collections::BTreeSet;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::client::{ExternalSearchHit, MediaWikiClient, RemotePage, WikiReadApi};

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

            let response = self.request_json_get(&params)?;
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

            let response = self.request_json_get(&params)?;
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

            let response = self.request_json_get(&params)?;
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

            let response = self.request_json_get(&params)?;
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

    fn search(
        &mut self,
        query: &str,
        namespaces: &[i32],
        limit: usize,
    ) -> Result<Vec<ExternalSearchHit>> {
        super::search::search_pages(self, query, namespaces, limit)
    }

    fn request_count(&self) -> usize {
        self.request_count
    }
}
