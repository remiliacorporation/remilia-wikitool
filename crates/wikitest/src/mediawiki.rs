use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tiny_http::{Header, Response, Server};
use url::form_urlencoded;

use crate::artifact::sha256_bytes;
use crate::model::{
    AssertionReceipt, MediaWikiExpectation, MediaWikiPage, MediaWikiRequestExpectation,
};

pub const MEDIAWIKI_FIXTURE_SCHEMA: &str = "wikitest.mediawiki-fixture.v3";
const MEDIAWIKI_OBSERVATION_SCHEMA: &str = "wikitest.mediawiki-observation.v2";
const LOGIN_TOKEN: &str = "WIKITEST-LOGIN-TOKEN";
const SESSION_COOKIE_NAME: &str = "wikitest_session";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiFixture {
    pub schema: String,
    pub username: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub siteinfo_query: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambiguous_edit_failure: Option<AmbiguousEditFailure>,
    #[serde(default)]
    pub ambiguous_delete_failures: Vec<AmbiguousDeleteFailure>,
    #[serde(default)]
    pub missingtitle_delete_failures: Vec<String>,
    #[serde(default = "default_delete_log_page_size")]
    pub delete_log_page_size: usize,
    #[serde(default)]
    pub delete_logs: Vec<MediaWikiDeleteLog>,
    pub pages: Vec<MediaWikiPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbiguousEditFailure {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbiguousDeleteFailure {
    pub title: String,
    #[serde(default)]
    pub hide_log_comment: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiDeleteLog {
    pub log_id: i64,
    pub title: String,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default)]
    pub comment_hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiObservation {
    pub schema: String,
    pub requests: Vec<MediaWikiRequest>,
    pub pages: Vec<MediaWikiPage>,
    pub delete_logs: Vec<MediaWikiDeleteLog>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiRequest {
    pub method: String,
    pub params: BTreeMap<String, String>,
}

#[derive(Debug)]
struct FixtureState {
    username: String,
    password: String,
    pages: BTreeMap<String, MediaWikiPage>,
    requests: Vec<MediaWikiRequest>,
    siteinfo_query: Option<Value>,
    ambiguous_edit_failure: Option<AmbiguousEditFailure>,
    ambiguous_edit_failure_used: bool,
    ambiguous_delete_failures: BTreeMap<String, bool>,
    ambiguous_delete_failures_used: BTreeSet<String>,
    missingtitle_delete_failures: BTreeSet<String>,
    missingtitle_delete_failures_used: BTreeSet<String>,
    delete_log_page_size: usize,
    delete_logs: Vec<MediaWikiDeleteLog>,
    login_token_issued: bool,
    authenticated: bool,
    csrf_token_issued: bool,
    session_cookie: String,
}

enum FixtureResponse {
    Json(Value),
    Authenticated(Value),
    Disconnect,
}

pub struct MediaWikiService {
    endpoint: String,
    state: Arc<Mutex<FixtureState>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<Result<()>>>,
}

impl MediaWikiFixture {
    pub fn from_path(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read MediaWiki fixture {}", path.display()))?;
        let fixture: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid MediaWiki fixture {}", path.display()))?;
        fixture.validate()?;
        Ok(fixture)
    }

    fn validate(&self) -> Result<()> {
        if self.schema != MEDIAWIKI_FIXTURE_SCHEMA {
            bail!("unsupported MediaWiki fixture schema '{}'", self.schema);
        }
        if self.username.trim().is_empty() || self.password.is_empty() {
            bail!("MediaWiki fixture credentials must be nonblank");
        }
        if self.pages.is_empty() {
            bail!("MediaWiki fixture must contain at least one page");
        }
        if self
            .siteinfo_query
            .as_ref()
            .is_some_and(|query| !query.is_object())
        {
            bail!("MediaWiki fixture siteinfo_query must be a JSON object");
        }
        if self
            .ambiguous_edit_failure
            .as_ref()
            .is_some_and(|failure| failure.title.trim().is_empty())
        {
            bail!("MediaWiki ambiguous edit failure title must be nonblank");
        }
        if self.delete_log_page_size == 0 || self.delete_log_page_size > 100 {
            bail!("MediaWiki delete-log page size must be between 1 and 100");
        }
        let mut titles = BTreeSet::new();
        for page in &self.pages {
            validate_page(page)?;
            if !titles.insert(page.title.as_str()) {
                bail!("MediaWiki fixture repeats page '{}'", page.title);
            }
        }
        let mut ambiguous_delete_titles = BTreeSet::new();
        for failure in &self.ambiguous_delete_failures {
            if failure.title.trim().is_empty() {
                bail!("MediaWiki ambiguous delete failure title must be nonblank");
            }
            if !titles.contains(failure.title.as_str()) {
                bail!(
                    "MediaWiki ambiguous delete failure references missing fixture page '{}'",
                    failure.title
                );
            }
            if !ambiguous_delete_titles.insert(failure.title.as_str()) {
                bail!(
                    "MediaWiki fixture repeats ambiguous delete failure '{}'",
                    failure.title
                );
            }
        }
        let mut missingtitle_delete_titles = BTreeSet::new();
        for title in &self.missingtitle_delete_failures {
            if title.trim().is_empty() {
                bail!("MediaWiki missingtitle delete failure title must be nonblank");
            }
            if !titles.contains(title.as_str()) {
                bail!(
                    "MediaWiki missingtitle delete failure references missing fixture page '{title}'"
                );
            }
            if !missingtitle_delete_titles.insert(title.as_str()) {
                bail!("MediaWiki fixture repeats missingtitle delete failure '{title}'");
            }
        }
        let mut log_ids = BTreeSet::new();
        for log in &self.delete_logs {
            validate_delete_log(log)?;
            if !log_ids.insert(log.log_id) {
                bail!("MediaWiki fixture repeats delete log ID {}", log.log_id);
            }
        }
        Ok(())
    }
}

const fn default_delete_log_page_size() -> usize {
    50
}

impl MediaWikiService {
    pub fn start(fixture: MediaWikiFixture) -> Result<Self> {
        fixture.validate()?;
        let server = Server::http("127.0.0.1:0")
            .map_err(|error| anyhow!("failed to bind MediaWiki fixture server: {error}"))?;
        let address = server
            .server_addr()
            .to_ip()
            .context("MediaWiki fixture did not bind an IP socket")?;
        let endpoint = format!("http://{address}/api.php");
        let session_cookie = sha256_bytes(
            format!("{}:{}:{endpoint}", fixture.username, std::process::id()).as_bytes(),
        );
        let state = Arc::new(Mutex::new(FixtureState {
            username: fixture.username,
            password: fixture.password,
            pages: fixture
                .pages
                .into_iter()
                .map(|page| (page.title.clone(), page))
                .collect(),
            requests: Vec::new(),
            siteinfo_query: fixture.siteinfo_query,
            ambiguous_edit_failure: fixture.ambiguous_edit_failure,
            ambiguous_edit_failure_used: false,
            ambiguous_delete_failures: fixture
                .ambiguous_delete_failures
                .into_iter()
                .map(|failure| (failure.title, failure.hide_log_comment))
                .collect(),
            ambiguous_delete_failures_used: BTreeSet::new(),
            missingtitle_delete_failures: fixture
                .missingtitle_delete_failures
                .into_iter()
                .collect(),
            missingtitle_delete_failures_used: BTreeSet::new(),
            delete_log_page_size: fixture.delete_log_page_size,
            delete_logs: fixture.delete_logs,
            login_token_issued: false,
            authenticated: false,
            csrf_token_issued: false,
            session_cookie,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let service_state = Arc::clone(&state);
        let service_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || serve(server, service_state, service_stop));
        Ok(Self {
            endpoint,
            state,
            stop,
            thread: Some(thread),
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn update_page(&self, page: MediaWikiPage) -> Result<()> {
        validate_page(&page)?;
        self.state
            .lock()
            .map_err(|_| anyhow!("MediaWiki fixture state lock is poisoned"))?
            .pages
            .insert(page.title.clone(), page);
        Ok(())
    }

    pub fn observation(&self) -> Result<MediaWikiObservation> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow!("MediaWiki fixture state lock is poisoned"))?;
        Ok(MediaWikiObservation {
            schema: MEDIAWIKI_OBSERVATION_SCHEMA.to_owned(),
            requests: state.requests.clone(),
            pages: state.pages.values().cloned().collect(),
            delete_logs: state.delete_logs.clone(),
        })
    }
}

impl Drop for MediaWikiService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn evaluate_expectation(
    observation: &MediaWikiObservation,
    expectation: &MediaWikiExpectation,
) -> Vec<AssertionReceipt> {
    let mut assertions = Vec::new();
    for expected in &expectation.requests {
        assertions.push(evaluate_request(observation, expected));
    }
    for expected in &expectation.pages {
        let page = observation
            .pages
            .iter()
            .find(|page| page.title == expected.title);
        let revision_matches = expected
            .revision_id
            .is_none_or(|revision_id| page.is_some_and(|page| page.revision_id == revision_id));
        let content_matches = expected.content_sha256.as_ref().is_none_or(|digest| {
            page.is_some_and(|page| sha256_bytes(page.content.as_bytes()) == *digest)
        });
        assertions.push(AssertionReceipt {
            target: format!("mediawiki.page:{}", expected.title),
            assertion: "page_state".to_owned(),
            passed: page.is_some() && revision_matches && content_matches,
            detail: page.map_or_else(
                || "page is missing".to_owned(),
                |page| {
                    format!(
                        "revision_id={}, content_sha256={}",
                        page.revision_id,
                        sha256_bytes(page.content.as_bytes())
                    )
                },
            ),
            file_evidence: None,
        });
    }
    for title in &expectation.missing_pages {
        let page = observation.pages.iter().find(|page| page.title == *title);
        assertions.push(AssertionReceipt {
            target: format!("mediawiki.page:{title}"),
            assertion: "page_missing".to_owned(),
            passed: page.is_none(),
            detail: page.map_or_else(
                || "page is missing".to_owned(),
                |page| format!("page is present as revision {}", page.revision_id),
            ),
            file_evidence: None,
        });
    }
    for expected in &expectation.delete_logs {
        let matches = observation
            .delete_logs
            .iter()
            .filter(|log| {
                log.title == expected.title
                    && expected
                        .comment_hidden
                        .is_none_or(|hidden| log.comment_hidden == hidden)
                    && expected.comment_contains.as_ref().is_none_or(|needle| {
                        log.comment
                            .as_ref()
                            .is_some_and(|comment| comment.contains(needle))
                    })
            })
            .count() as u64;
        assertions.push(AssertionReceipt {
            target: format!("mediawiki.delete-log:{}", expected.title),
            assertion: "delete_log_count".to_owned(),
            passed: matches == expected.count,
            detail: format!(
                "expected {} delete log(s), observed {matches}; comment_hidden={:?}, comment_contains={:?}",
                expected.count, expected.comment_hidden, expected.comment_contains
            ),
            file_evidence: None,
        });
    }
    assertions
}

fn evaluate_request(
    observation: &MediaWikiObservation,
    expected: &MediaWikiRequestExpectation,
) -> AssertionReceipt {
    let matches = observation
        .requests
        .iter()
        .filter(|request| {
            request.method == expected.method
                && expected
                    .params
                    .iter()
                    .all(|(key, value)| request.params.get(key) == Some(value))
        })
        .count() as u64;
    AssertionReceipt {
        target: "mediawiki.requests".to_owned(),
        assertion: "request_count".to_owned(),
        passed: matches == expected.count,
        detail: format!(
            "expected {} matching {} request(s) with {:?}, observed {matches}",
            expected.count, expected.method, expected.params
        ),
        file_evidence: None,
    }
}

fn validate_page(page: &MediaWikiPage) -> Result<()> {
    if page.title.trim().is_empty() {
        bail!("MediaWiki fixture page title must be nonblank");
    }
    if page.page_id <= 0 || page.revision_id <= 0 {
        bail!("MediaWiki fixture page and revision IDs must be positive");
    }
    if page.timestamp.trim().is_empty() {
        bail!("MediaWiki fixture page timestamp must be nonblank");
    }
    Ok(())
}

fn validate_delete_log(log: &MediaWikiDeleteLog) -> Result<()> {
    if log.log_id <= 0 {
        bail!("MediaWiki fixture delete log ID must be positive");
    }
    if log.title.trim().is_empty() || log.timestamp.trim().is_empty() {
        bail!("MediaWiki fixture delete log title and timestamp must be nonblank");
    }
    if log.comment_hidden && log.comment.is_some() {
        bail!("MediaWiki fixture hidden delete log must not expose a comment");
    }
    if log
        .comment
        .as_ref()
        .is_some_and(|comment| comment.trim().is_empty())
    {
        bail!("MediaWiki fixture delete log comment must be nonblank when present");
    }
    if log.user.as_ref().is_some_and(|user| user.trim().is_empty()) {
        bail!("MediaWiki fixture delete log user must be nonblank when present");
    }
    Ok(())
}

fn serve(server: Server, state: Arc<Mutex<FixtureState>>, stop: Arc<AtomicBool>) -> Result<()> {
    while !stop.load(Ordering::SeqCst) {
        let Some(mut request) = server.recv_timeout(Duration::from_millis(50))? else {
            continue;
        };
        let method = request.method().to_string();
        let cookie = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Cookie"))
            .map(|header| header.value.as_str().to_owned());
        let mut params = parse_query(request.url());
        if method == "POST" {
            let mut body = String::new();
            request
                .as_reader()
                .read_to_string(&mut body)
                .context("failed to read MediaWiki fixture request body")?;
            params.extend(parse_form(&body));
        }
        let response = {
            let mut state = state
                .lock()
                .map_err(|_| anyhow!("MediaWiki fixture state lock is poisoned"))?;
            state.requests.push(MediaWikiRequest {
                method: method.clone(),
                params: params.clone(),
            });
            handle_request(&mut state, &params, cookie.as_deref())
        };
        let (payload, set_cookie) = match response {
            Ok(FixtureResponse::Json(value)) => (value, None),
            Ok(FixtureResponse::Authenticated(value)) => {
                let state = state
                    .lock()
                    .map_err(|_| anyhow!("MediaWiki fixture state lock is poisoned"))?;
                (
                    value,
                    Some(format!(
                        "{SESSION_COOKIE_NAME}={}; Path=/; HttpOnly; SameSite=Strict",
                        state.session_cookie
                    )),
                )
            }
            Ok(FixtureResponse::Disconnect) => {
                drop(request);
                continue;
            }
            Err(error) => (
                json!({"error": {"code": "wikitest-fixture-error", "info": format!("{error:#}")}}),
                None,
            ),
        };
        let content_type = Header::from_bytes("Content-Type", "application/json; charset=utf-8")
            .map_err(|_| anyhow!("failed to construct response content type"))?;
        let mut response = Response::from_string(serde_json::to_string(&payload)?)
            .with_status_code(200)
            .with_header(content_type);
        if let Some(cookie) = set_cookie {
            response.add_header(
                Header::from_bytes("Set-Cookie", cookie)
                    .map_err(|_| anyhow!("failed to construct session cookie"))?,
            );
        }
        request.respond(response)?;
    }
    Ok(())
}

fn parse_query(url: &str) -> BTreeMap<String, String> {
    url.split_once('?')
        .map(|(_, query)| parse_form(query))
        .unwrap_or_default()
}

fn parse_form(value: &str) -> BTreeMap<String, String> {
    form_urlencoded::parse(value.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

fn handle_request(
    state: &mut FixtureState,
    params: &BTreeMap<String, String>,
    cookie: Option<&str>,
) -> Result<FixtureResponse> {
    match params.get("action").map(String::as_str) {
        Some("query") => handle_query(state, params, cookie),
        Some("login") => handle_login(state, params),
        Some("edit") => handle_edit(state, params, cookie),
        Some("delete") => handle_delete(state, params, cookie),
        Some(other) => bail!("unsupported MediaWiki fixture action {other:?}"),
        None => bail!("MediaWiki fixture request omitted action"),
    }
}

fn handle_query(
    state: &mut FixtureState,
    params: &BTreeMap<String, String>,
    cookie: Option<&str>,
) -> Result<FixtureResponse> {
    if params.get("meta").is_some_and(|value| value == "siteinfo") {
        let query = state
            .siteinfo_query
            .as_ref()
            .context("MediaWiki fixture has no siteinfo_query response")?;
        return Ok(FixtureResponse::Json(
            json!({"batchcomplete": true, "query": query}),
        ));
    }
    if params.get("meta").is_some_and(|value| value == "tokens") {
        return if params.get("type").is_some_and(|value| value == "login") {
            state.login_token_issued = true;
            Ok(FixtureResponse::Json(
                json!({"query": {"tokens": {"logintoken": LOGIN_TOKEN}}}),
            ))
        } else {
            require_authenticated_session(state, cookie)?;
            state.csrf_token_issued = true;
            Ok(FixtureResponse::Json(json!({
                "query": {"tokens": {"csrftoken": csrf_token(state)}}
            })))
        };
    }
    if params.get("list").is_some_and(|value| value == "logevents") {
        return handle_delete_log_query(state, params);
    }
    if params.get("list").is_some_and(|value| value == "allpages") {
        let namespace = params
            .get("apnamespace")
            .context("allpages request omitted apnamespace")?
            .parse::<i32>()
            .context("invalid apnamespace")?;
        let pages = state
            .pages
            .values()
            .filter(|page| page.namespace == namespace)
            .map(|page| json!({"title": page.title}))
            .collect::<Vec<_>>();
        return Ok(FixtureResponse::Json(
            json!({"batchcomplete": true, "query": {"allpages": pages}}),
        ));
    }
    if params
        .get("list")
        .is_some_and(|value| value == "categorymembers")
    {
        return Ok(FixtureResponse::Json(
            json!({"batchcomplete": true, "query": {"categorymembers": []}}),
        ));
    }
    if params
        .get("list")
        .is_some_and(|value| value == "recentchanges")
    {
        return Ok(FixtureResponse::Json(
            json!({"batchcomplete": true, "query": {"recentchanges": []}}),
        ));
    }
    if let Some(revids) = params.get("revids") {
        let mut pages = Vec::new();
        for revision_id in revids.split('|') {
            let revision_id = revision_id
                .parse::<i64>()
                .context("invalid revids revision ID")?;
            if let Some(page) = state
                .pages
                .values()
                .find(|page| page.revision_id == revision_id)
            {
                pages.push(page_response(Some(page), &page.title, true));
            }
        }
        return Ok(FixtureResponse::Json(
            json!({"batchcomplete": true, "query": {"pages": pages}}),
        ));
    }
    if let Some(titles) = params.get("titles") {
        let include_content = params
            .get("rvprop")
            .is_some_and(|value| value.split('|').any(|part| part == "content"));
        let pages = titles
            .split('|')
            .map(|title| page_response(state.pages.get(title), title, include_content))
            .collect::<Vec<_>>();
        return Ok(FixtureResponse::Json(
            json!({"batchcomplete": true, "query": {"pages": pages}}),
        ));
    }
    bail!("unsupported MediaWiki fixture query")
}

fn handle_delete_log_query(
    state: &FixtureState,
    params: &BTreeMap<String, String>,
) -> Result<FixtureResponse> {
    if params.get("leaction").map(String::as_str) != Some("delete/delete") {
        bail!("delete-log query requires leaction=delete/delete");
    }
    if params.get("leprop").map(String::as_str) != Some("ids|title|timestamp|comment|type|user") {
        bail!("delete-log query used an unexpected leprop contract");
    }
    if params.get("lelimit").map(String::as_str) != Some("max") {
        bail!("delete-log query requires lelimit=max");
    }
    let title = params
        .get("letitle")
        .context("delete-log query omitted letitle")?;
    let offset = params
        .get("lecontinue")
        .map(|value| {
            value
                .parse::<usize>()
                .context("invalid delete-log continuation")
        })
        .transpose()?
        .unwrap_or(0);
    let mut matching = state
        .delete_logs
        .iter()
        .filter(|log| log.title == *title)
        .collect::<Vec<_>>();
    matching.sort_by_key(|entry| std::cmp::Reverse(entry.log_id));
    if offset > matching.len() {
        bail!("delete-log continuation exceeds the matching lineage");
    }
    let page = matching
        .iter()
        .skip(offset)
        .take(state.delete_log_page_size)
        .map(|log| delete_log_response_item(log))
        .collect::<Vec<_>>();
    let next = offset + page.len();
    let mut response = json!({"query": {"logevents": page}});
    if next < matching.len() {
        response["continue"] = json!({"lecontinue": next.to_string()});
    } else {
        response["batchcomplete"] = json!(true);
    }
    Ok(FixtureResponse::Json(response))
}

fn delete_log_response_item(log: &MediaWikiDeleteLog) -> Value {
    let mut item = json!({
        "logid": log.log_id,
        "title": log.title,
        "timestamp": log.timestamp,
        "type": "delete",
        "action": "delete"
    });
    if log.comment_hidden {
        item["commenthidden"] = json!(true);
    } else if let Some(comment) = &log.comment {
        item["comment"] = json!(comment);
    }
    if let Some(user) = &log.user {
        item["user"] = json!(user);
    }
    item
}

fn page_response(page: Option<&MediaWikiPage>, title: &str, include_content: bool) -> Value {
    let Some(page) = page else {
        return json!({"ns": namespace_for_title(title), "title": title, "missing": true});
    };
    let revision = if include_content {
        json!({
            "revid": page.revision_id,
            "timestamp": page.timestamp,
            "slots": {"main": {"content": page.content}}
        })
    } else {
        json!({"revid": page.revision_id, "timestamp": page.timestamp})
    };
    json!({
        "pageid": page.page_id,
        "ns": page.namespace,
        "title": page.title,
        "revisions": [revision]
    })
}

fn namespace_for_title(title: &str) -> i32 {
    if title.starts_with("Template:") {
        10
    } else {
        0
    }
}

fn handle_login(
    state: &mut FixtureState,
    params: &BTreeMap<String, String>,
) -> Result<FixtureResponse> {
    let success = params.get("lgname") == Some(&state.username)
        && params.get("lgpassword") == Some(&state.password)
        && params
            .get("lgtoken")
            .is_some_and(|value| value == LOGIN_TOKEN)
        && state.login_token_issued;
    state.authenticated = false;
    state.csrf_token_issued = false;
    if success {
        state.login_token_issued = false;
        state.authenticated = true;
        Ok(FixtureResponse::Authenticated(
            json!({"login": {"result": "Success"}}),
        ))
    } else {
        Ok(FixtureResponse::Json(
            json!({"login": {"result": "Failed", "reason": "fixture credentials rejected"}}),
        ))
    }
}

fn handle_edit(
    state: &mut FixtureState,
    params: &BTreeMap<String, String>,
    cookie: Option<&str>,
) -> Result<FixtureResponse> {
    require_authenticated_session(state, cookie)?;
    if !state.csrf_token_issued {
        bail!("edit request did not obtain a CSRF token for the authenticated session");
    }
    if !params
        .get("token")
        .is_some_and(|value| value == &csrf_token(state))
    {
        bail!("edit request used an invalid CSRF token");
    }
    let title = params.get("title").context("edit request omitted title")?;
    let content = params.get("text").context("edit request omitted text")?;
    let existing = state.pages.get(title).cloned();
    if params.get("createonly").is_some_and(|value| value == "1") && existing.is_some() {
        return Ok(FixtureResponse::Json(
            json!({"error": {"code": "articleexists", "info": "page already exists"}}),
        ));
    }
    if let Some(base) = params.get("baserevid") {
        let base = base.parse::<i64>().context("invalid baserevid")?;
        if existing.as_ref().map(|page| page.revision_id) != Some(base) {
            return Ok(FixtureResponse::Json(
                json!({"error": {"code": "editconflict", "info": "baserevid mismatch"}}),
            ));
        }
    }
    let old_revision = existing.as_ref().map(|page| page.revision_id).unwrap_or(0);
    let next_revision = state
        .pages
        .values()
        .map(|page| page.revision_id)
        .max()
        .unwrap_or(0)
        + 1;
    let page_id = existing
        .as_ref()
        .map(|page| page.page_id)
        .unwrap_or_else(|| {
            state
                .pages
                .values()
                .map(|page| page.page_id)
                .max()
                .unwrap_or(0)
                + 1
        });
    let page = MediaWikiPage {
        title: title.clone(),
        namespace: namespace_for_title(title),
        page_id,
        revision_id: next_revision,
        timestamp: "2026-08-27T20:00:00Z".to_owned(),
        content: content.clone(),
    };
    state.pages.insert(title.clone(), page.clone());
    if state
        .ambiguous_edit_failure
        .as_ref()
        .is_some_and(|failure| failure.title == *title)
        && !state.ambiguous_edit_failure_used
    {
        state.ambiguous_edit_failure_used = true;
        return Ok(FixtureResponse::Disconnect);
    }
    Ok(FixtureResponse::Json(json!({
        "edit": {
            "result": "Success",
            "pageid": page.page_id,
            "title": page.title,
            "oldrevid": old_revision,
            "newrevid": page.revision_id,
            "newtimestamp": page.timestamp
        }
    })))
}

fn handle_delete(
    state: &mut FixtureState,
    params: &BTreeMap<String, String>,
    cookie: Option<&str>,
) -> Result<FixtureResponse> {
    require_authenticated_session(state, cookie)?;
    if !state.csrf_token_issued {
        bail!("delete request did not obtain a CSRF token for the authenticated session");
    }
    if !params
        .get("token")
        .is_some_and(|value| value == &csrf_token(state))
    {
        bail!("delete request used an invalid CSRF token");
    }
    let title = params
        .get("title")
        .context("delete request omitted title")?
        .clone();
    let reason = params
        .get("reason")
        .context("delete request omitted reason")?
        .clone();
    if reason.trim().is_empty() {
        bail!("delete request used a blank reason");
    }

    if state.missingtitle_delete_failures.contains(&title)
        && state
            .missingtitle_delete_failures_used
            .insert(title.clone())
    {
        state.pages.remove(&title);
        return Ok(FixtureResponse::Json(
            json!({"error": {"code": "missingtitle", "info": "page disappeared before deletion"}}),
        ));
    }
    if state.pages.remove(&title).is_none() {
        return Ok(FixtureResponse::Json(
            json!({"error": {"code": "missingtitle", "info": "page does not exist"}}),
        ));
    }

    let ambiguous = state
        .ambiguous_delete_failures
        .get(&title)
        .copied()
        .filter(|_| !state.ambiguous_delete_failures_used.contains(&title));
    let hide_log_comment = ambiguous.unwrap_or(false);
    let log_id = state
        .delete_logs
        .iter()
        .map(|log| log.log_id)
        .max()
        .unwrap_or(0)
        + 1;
    state.delete_logs.push(MediaWikiDeleteLog {
        log_id,
        title: title.clone(),
        timestamp: "2026-08-27T22:00:00Z".to_owned(),
        comment: (!hide_log_comment).then_some(reason),
        comment_hidden: hide_log_comment,
        user: Some(state.username.clone()),
    });
    if ambiguous.is_some() {
        state.ambiguous_delete_failures_used.insert(title);
        return Ok(FixtureResponse::Disconnect);
    }
    Ok(FixtureResponse::Json(
        json!({"delete": {"title": title, "logid": log_id}}),
    ))
}

fn require_authenticated_session(state: &FixtureState, cookie: Option<&str>) -> Result<()> {
    let authenticated = state.authenticated
        && cookie.is_some_and(|header| {
            header.split(';').any(|pair| {
                pair.trim().split_once('=').is_some_and(|(name, value)| {
                    name == SESSION_COOKIE_NAME && value == state.session_cookie
                })
            })
        });
    if !authenticated {
        bail!("MediaWiki request requires an authenticated fixture session");
    }
    Ok(())
}

fn csrf_token(state: &FixtureState) -> String {
    format!("WIKITEST-CSRF-{}", state.session_cookie)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        MediaWikiDeleteLogExpectation, MediaWikiPageExpectation, MediaWikiRequestExpectation,
    };

    #[test]
    fn expectation_matches_wire_parameters_and_page_state() {
        let observation = MediaWikiObservation {
            schema: MEDIAWIKI_OBSERVATION_SCHEMA.to_owned(),
            requests: vec![MediaWikiRequest {
                method: "POST".to_owned(),
                params: BTreeMap::from([
                    ("action".to_owned(), "edit".to_owned()),
                    ("baserevid".to_owned(), "41".to_owned()),
                ]),
            }],
            pages: vec![MediaWikiPage {
                title: "Template:Alpha".to_owned(),
                namespace: 10,
                page_id: 1,
                revision_id: 42,
                timestamp: "2026-08-27T20:00:00Z".to_owned(),
                content: "updated".to_owned(),
            }],
            delete_logs: vec![MediaWikiDeleteLog {
                log_id: 9,
                title: "Template:Deleted".to_owned(),
                timestamp: "2026-08-27T20:01:00Z".to_owned(),
                comment: Some("Wikitest delete reason".to_owned()),
                comment_hidden: false,
                user: Some("bot".to_owned()),
            }],
        };
        let expectation = MediaWikiExpectation {
            requests: vec![MediaWikiRequestExpectation {
                method: "POST".to_owned(),
                params: BTreeMap::from([
                    ("action".to_owned(), "edit".to_owned()),
                    ("baserevid".to_owned(), "41".to_owned()),
                ]),
                count: 1,
            }],
            pages: vec![MediaWikiPageExpectation {
                title: "Template:Alpha".to_owned(),
                revision_id: Some(42),
                content_sha256: Some(sha256_bytes(b"updated")),
            }],
            missing_pages: vec!["Template:Missing".to_owned()],
            delete_logs: vec![MediaWikiDeleteLogExpectation {
                title: "Template:Deleted".to_owned(),
                comment_contains: Some("delete reason".to_owned()),
                comment_hidden: Some(false),
                count: 1,
            }],
        };
        assert!(
            evaluate_expectation(&observation, &expectation)
                .iter()
                .all(|assertion| assertion.passed)
        );
    }

    #[test]
    fn edits_require_login_cookie_and_session_bound_csrf_token() {
        let mut state = FixtureState {
            username: "bot".to_owned(),
            password: "secret".to_owned(),
            pages: BTreeMap::new(),
            requests: Vec::new(),
            siteinfo_query: None,
            ambiguous_edit_failure: None,
            ambiguous_edit_failure_used: false,
            ambiguous_delete_failures: BTreeMap::new(),
            ambiguous_delete_failures_used: BTreeSet::new(),
            missingtitle_delete_failures: BTreeSet::new(),
            missingtitle_delete_failures_used: BTreeSet::new(),
            delete_log_page_size: 1,
            delete_logs: Vec::new(),
            login_token_issued: false,
            authenticated: false,
            csrf_token_issued: false,
            session_cookie: "session-value".to_owned(),
        };
        let mut edit = BTreeMap::from([
            ("action".to_owned(), "edit".to_owned()),
            ("title".to_owned(), "Template:Alpha".to_owned()),
            ("text".to_owned(), "updated".to_owned()),
            ("createonly".to_owned(), "1".to_owned()),
        ]);
        assert!(handle_edit(&mut state, &edit, None).is_err());
        let cookie = format!("{SESSION_COOKIE_NAME}={}", state.session_cookie);
        let csrf = BTreeMap::from([
            ("action".to_owned(), "query".to_owned()),
            ("meta".to_owned(), "tokens".to_owned()),
        ]);
        assert!(handle_query(&mut state, &csrf, Some(&cookie)).is_err());

        let login_token = BTreeMap::from([
            ("action".to_owned(), "query".to_owned()),
            ("meta".to_owned(), "tokens".to_owned()),
            ("type".to_owned(), "login".to_owned()),
        ]);
        assert!(matches!(
            handle_query(&mut state, &login_token, None).expect("login token"),
            FixtureResponse::Json(_)
        ));
        let login = BTreeMap::from([
            ("action".to_owned(), "login".to_owned()),
            ("lgname".to_owned(), "bot".to_owned()),
            ("lgpassword".to_owned(), "secret".to_owned()),
            ("lgtoken".to_owned(), LOGIN_TOKEN.to_owned()),
        ]);
        assert!(matches!(
            handle_login(&mut state, &login).expect("login"),
            FixtureResponse::Authenticated(_)
        ));
        assert!(matches!(
            handle_query(&mut state, &csrf, Some(&cookie)).expect("csrf token"),
            FixtureResponse::Json(_)
        ));
        edit.insert("token".to_owned(), csrf_token(&state));
        assert!(matches!(
            handle_edit(&mut state, &edit, Some(&cookie)).expect("authenticated edit"),
            FixtureResponse::Json(_)
        ));
    }

    #[test]
    fn deletes_are_authenticated_one_shot_and_delete_logs_paginate() {
        let page = |title: &str, page_id: i64, revision_id: i64| MediaWikiPage {
            title: title.to_owned(),
            namespace: 10,
            page_id,
            revision_id,
            timestamp: "2026-08-27T20:00:00Z".to_owned(),
            content: "fixture".to_owned(),
        };
        let mut state = FixtureState {
            username: "bot".to_owned(),
            password: "secret".to_owned(),
            pages: BTreeMap::from([
                (
                    "Template:Ambiguous".to_owned(),
                    page("Template:Ambiguous", 1, 41),
                ),
                (
                    "Template:Missingtitle".to_owned(),
                    page("Template:Missingtitle", 2, 42),
                ),
            ]),
            requests: Vec::new(),
            siteinfo_query: None,
            ambiguous_edit_failure: None,
            ambiguous_edit_failure_used: false,
            ambiguous_delete_failures: BTreeMap::from([("Template:Ambiguous".to_owned(), false)]),
            ambiguous_delete_failures_used: BTreeSet::new(),
            missingtitle_delete_failures: BTreeSet::from(["Template:Missingtitle".to_owned()]),
            missingtitle_delete_failures_used: BTreeSet::new(),
            delete_log_page_size: 1,
            delete_logs: vec![MediaWikiDeleteLog {
                log_id: 7,
                title: "Template:Ambiguous".to_owned(),
                timestamp: "2026-08-27T19:00:00Z".to_owned(),
                comment: Some("older delete".to_owned()),
                comment_hidden: false,
                user: Some("bot".to_owned()),
            }],
            login_token_issued: false,
            authenticated: true,
            csrf_token_issued: true,
            session_cookie: "session-value".to_owned(),
        };
        let delete = |title: &str, token: String| {
            BTreeMap::from([
                ("action".to_owned(), "delete".to_owned()),
                ("title".to_owned(), title.to_owned()),
                ("reason".to_owned(), "test reason [marker]".to_owned()),
                ("token".to_owned(), token),
            ])
        };
        let ambiguous = delete("Template:Ambiguous", csrf_token(&state));
        assert!(handle_delete(&mut state, &ambiguous, None).is_err());
        let cookie = format!("{SESSION_COOKIE_NAME}={}", state.session_cookie);
        assert!(matches!(
            handle_delete(&mut state, &ambiguous, Some(&cookie)).expect("ambiguous delete"),
            FixtureResponse::Disconnect
        ));
        assert!(!state.pages.contains_key("Template:Ambiguous"));
        assert_eq!(state.delete_logs.len(), 2);
        assert_eq!(
            state.delete_logs[1].comment.as_deref(),
            Some("test reason [marker]")
        );

        let first_query = BTreeMap::from([
            ("action".to_owned(), "query".to_owned()),
            ("list".to_owned(), "logevents".to_owned()),
            ("leaction".to_owned(), "delete/delete".to_owned()),
            ("letitle".to_owned(), "Template:Ambiguous".to_owned()),
            (
                "leprop".to_owned(),
                "ids|title|timestamp|comment|type|user".to_owned(),
            ),
            ("lelimit".to_owned(), "max".to_owned()),
        ]);
        let FixtureResponse::Json(first) =
            handle_delete_log_query(&state, &first_query).expect("first log page")
        else {
            panic!("JSON log page");
        };
        assert_eq!(first.pointer("/query/logevents/0/logid"), Some(&json!(8)));
        assert_eq!(first.pointer("/continue/lecontinue"), Some(&json!("1")));
        let mut second_query = first_query;
        second_query.insert("lecontinue".to_owned(), "1".to_owned());
        let FixtureResponse::Json(second) =
            handle_delete_log_query(&state, &second_query).expect("second log page")
        else {
            panic!("JSON log page");
        };
        assert_eq!(second.pointer("/query/logevents/0/logid"), Some(&json!(7)));
        assert!(second.get("continue").is_none());

        let missingtitle = delete("Template:Missingtitle", csrf_token(&state));
        let FixtureResponse::Json(response) =
            handle_delete(&mut state, &missingtitle, Some(&cookie)).expect("missingtitle delete")
        else {
            panic!("JSON missingtitle response");
        };
        assert_eq!(
            response.pointer("/error/code"),
            Some(&json!("missingtitle"))
        );
        assert!(!state.pages.contains_key("Template:Missingtitle"));
        assert_eq!(state.delete_logs.len(), 2);
    }

    #[test]
    fn exact_revision_queries_return_current_main_slot_content() {
        let mut state = FixtureState {
            username: "bot".to_owned(),
            password: "secret".to_owned(),
            pages: BTreeMap::from([(
                "Template:Alpha".to_owned(),
                MediaWikiPage {
                    title: "Template:Alpha".to_owned(),
                    namespace: 10,
                    page_id: 1,
                    revision_id: 42,
                    timestamp: "2026-08-27T20:00:00Z".to_owned(),
                    content: "updated".to_owned(),
                },
            )]),
            requests: Vec::new(),
            siteinfo_query: None,
            ambiguous_edit_failure: None,
            ambiguous_edit_failure_used: false,
            ambiguous_delete_failures: BTreeMap::new(),
            ambiguous_delete_failures_used: BTreeSet::new(),
            missingtitle_delete_failures: BTreeSet::new(),
            missingtitle_delete_failures_used: BTreeSet::new(),
            delete_log_page_size: 1,
            delete_logs: Vec::new(),
            login_token_issued: false,
            authenticated: false,
            csrf_token_issued: false,
            session_cookie: "session-value".to_owned(),
        };
        let query = BTreeMap::from([
            ("action".to_owned(), "query".to_owned()),
            ("revids".to_owned(), "42".to_owned()),
            ("prop".to_owned(), "revisions".to_owned()),
            ("rvprop".to_owned(), "content|timestamp|ids".to_owned()),
            ("rvslots".to_owned(), "main".to_owned()),
        ]);
        let FixtureResponse::Json(response) =
            handle_query(&mut state, &query, None).expect("exact revision")
        else {
            panic!("JSON response");
        };
        assert_eq!(
            response.pointer("/query/pages/0/revisions/0/revid"),
            Some(&json!(42))
        );
        assert_eq!(
            response.pointer("/query/pages/0/revisions/0/slots/main/content"),
            Some(&json!("updated"))
        );
    }
}
