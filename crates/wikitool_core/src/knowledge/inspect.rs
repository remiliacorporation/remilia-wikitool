use super::prelude::*;

use serde::Deserialize;

use crate::mw::{MediaWikiClient, WikiReadApi};

pub use super::model::{
    BrokenLinkIssue, DoubleRedirectIssue, LiveBrokenLinkVerification,
    LiveDoubleRedirectVerification, LivePageVerificationStatus, LiveRedirectVerificationStatus,
    LiveValidationReport, ValidationReport,
};

#[derive(Debug, Deserialize, Default)]
struct LiveQueryResponse {
    #[serde(default)]
    query: LiveQueryPayload,
}

#[derive(Debug, Deserialize, Default)]
struct LiveQueryPayload {
    #[serde(default)]
    pages: Vec<LiveQueryPage>,
    #[serde(default)]
    redirects: Vec<LiveQueryRedirect>,
}

#[derive(Debug, Deserialize)]
struct LiveQueryPage {
    pageid: Option<i64>,
    title: String,
    missing: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct LiveQueryRedirect {
    from: String,
    to: String,
}

pub fn run_validation_checks(paths: &ResolvedPaths) -> Result<Option<ValidationReport>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };

    Ok(Some(ValidationReport {
        broken_links: query_broken_links_for_connection(&connection)?,
        double_redirects: query_double_redirects_for_connection(&connection)?,
        uncategorized_pages: query_uncategorized_pages_for_connection(&connection)?,
        orphan_pages: query_orphans_for_connection(&connection)?,
    }))
}

pub fn verify_validation_report_live(
    report: &ValidationReport,
    config: &crate::config::WikiConfig,
) -> Result<LiveValidationReport> {
    let mut titles = BTreeSet::new();
    for issue in &report.broken_links {
        titles.insert(issue.target_title.clone());
    }
    for issue in &report.double_redirects {
        titles.insert(issue.title.clone());
    }

    let mut client = MediaWikiClient::from_config(config)?;
    let live_titles = fetch_live_title_statuses(&mut client, titles.into_iter().collect())?;

    let broken_links = report
        .broken_links
        .iter()
        .map(|issue| {
            let live_title = lookup_live_title(&live_titles, &issue.target_title);
            LiveBrokenLinkVerification {
                source_title: issue.source_title.clone(),
                target_title: issue.target_title.clone(),
                live_status: live_title
                    .map(LiveTitleStatus::page_status)
                    .unwrap_or(LivePageVerificationStatus::Missing),
                resolved_title: live_title.and_then(LiveTitleStatus::resolved_title),
                page_id: live_title.and_then(LiveTitleStatus::page_id),
            }
        })
        .collect();

    let double_redirects = report
        .double_redirects
        .iter()
        .map(|issue| {
            let live_title = lookup_live_title(&live_titles, &issue.title);
            let live_status = match live_title {
                Some(status) if status.is_missing() => {
                    LiveRedirectVerificationStatus::SourceMissing
                }
                Some(status) if status.redirected_to.is_some() => {
                    let resolved = status
                        .page
                        .title
                        .as_deref()
                        .or(status.redirected_to.as_deref());
                    if resolved
                        .map(|title| same_live_title(title, &issue.final_target))
                        .unwrap_or(false)
                    {
                        LiveRedirectVerificationStatus::ResolvesToExpectedFinal
                    } else {
                        LiveRedirectVerificationStatus::ResolvesToDifferentTarget
                    }
                }
                Some(_) => LiveRedirectVerificationStatus::SourceExistsWithoutRedirectResolution,
                None => LiveRedirectVerificationStatus::SourceMissing,
            };
            LiveDoubleRedirectVerification {
                title: issue.title.clone(),
                first_target: issue.first_target.clone(),
                final_target: issue.final_target.clone(),
                live_status,
                resolved_title: live_title.and_then(LiveTitleStatus::resolved_title),
                page_id: live_title.and_then(LiveTitleStatus::page_id),
            }
        })
        .collect();

    Ok(LiveValidationReport {
        request_count: client.request_count(),
        broken_links,
        double_redirects,
    })
}

pub fn query_backlinks(paths: &ResolvedPaths, title: &str) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    let normalized = normalize_query_title(title);
    if normalized.is_empty() {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(query_backlinks_for_connection(
        &connection,
        &normalized,
    )?))
}

pub fn query_orphans(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };
    Ok(Some(query_orphans_for_connection(&connection)?))
}

pub fn query_empty_categories(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    let connection = match open_indexed_connection(paths)? {
        Some(connection) => connection,
        None => return Ok(None),
    };

    let mut statement = connection
        .prepare(
            "SELECT p.title
             FROM indexed_pages p
             WHERE p.namespace = 'Category'
               AND NOT EXISTS (
                   SELECT 1
                   FROM indexed_links l
                   WHERE l.target_title = p.title
                     AND l.is_category_membership = 1
               )
             ORDER BY p.title ASC",
        )
        .context("failed to prepare empty category query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .context("failed to run empty category query")?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.context("failed to decode empty category row")?);
    }
    Ok(Some(out))
}

#[derive(Debug, Clone)]
struct LiveTitleStatus {
    requested_title: String,
    redirected_to: Option<String>,
    page: LivePageStatus,
}

#[derive(Debug, Clone)]
struct LivePageStatus {
    title: Option<String>,
    page_id: Option<i64>,
    missing: bool,
}

impl LiveTitleStatus {
    fn is_missing(&self) -> bool {
        self.page.missing
    }

    fn page_status(&self) -> LivePageVerificationStatus {
        if self.page.missing {
            LivePageVerificationStatus::Missing
        } else if self.redirected_to.is_some() {
            LivePageVerificationStatus::RedirectResolved
        } else {
            LivePageVerificationStatus::Exists
        }
    }

    fn resolved_title(&self) -> Option<String> {
        self.page
            .title
            .clone()
            .or_else(|| self.redirected_to.clone())
            .filter(|title| !same_live_title(title, &self.requested_title))
    }

    fn page_id(&self) -> Option<i64> {
        self.page.page_id
    }
}

fn fetch_live_title_statuses(
    client: &mut MediaWikiClient,
    titles: Vec<String>,
) -> Result<BTreeMap<String, LiveTitleStatus>> {
    let mut out = BTreeMap::new();
    if titles.is_empty() {
        return Ok(out);
    }

    for batch in titles.chunks(50) {
        let response = client.request_json_get(&[
            ("action", "query".to_string()),
            ("titles", batch.join("|")),
            ("prop", "info".to_string()),
            ("redirects", "1".to_string()),
        ])?;
        let parsed: LiveQueryResponse = serde_json::from_value(response)
            .context("failed to decode live validation API response")?;
        let mut pages_by_title = BTreeMap::<String, LivePageStatus>::new();
        for page in parsed.query.pages {
            pages_by_title.insert(
                live_title_key(&page.title),
                LivePageStatus {
                    title: Some(page.title),
                    page_id: page.pageid,
                    missing: page.missing.unwrap_or(false),
                },
            );
        }

        let redirect_map = parsed
            .query
            .redirects
            .into_iter()
            .map(|redirect| (live_title_key(&redirect.from), redirect.to))
            .collect::<BTreeMap<_, _>>();

        for requested_title in batch {
            let redirected_to = redirect_map.get(&live_title_key(requested_title)).cloned();
            let lookup_title = redirected_to.as_deref().unwrap_or(requested_title);
            let page = pages_by_title
                .get(&live_title_key(lookup_title))
                .or_else(|| pages_by_title.get(&live_title_key(requested_title)))
                .cloned()
                .unwrap_or(LivePageStatus {
                    title: None,
                    page_id: None,
                    missing: true,
                });
            out.insert(
                live_title_key(requested_title),
                LiveTitleStatus {
                    requested_title: requested_title.clone(),
                    redirected_to,
                    page,
                },
            );
        }
    }

    Ok(out)
}

fn lookup_live_title<'a>(
    live_titles: &'a BTreeMap<String, LiveTitleStatus>,
    title: &str,
) -> Option<&'a LiveTitleStatus> {
    live_titles.get(&live_title_key(title))
}

fn live_title_key(value: &str) -> String {
    normalize_query_title(&value.replace('_', " ")).to_ascii_lowercase()
}

fn same_live_title(left: &str, right: &str) -> bool {
    live_title_key(left) == live_title_key(right)
}
