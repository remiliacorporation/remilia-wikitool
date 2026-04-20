use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::ResolvedPaths;
use crate::support::{format_iso8601_utc, normalize_path, now_iso8601_utc, unix_timestamp};

use super::model::ExternalFetchSession;

const RESEARCH_SESSION_SCHEMA_VERSION: &str = "research_session_v1";

#[derive(Debug, Clone, Default)]
pub struct ResearchSessionImportOptions {
    pub user_agent: Option<String>,
    pub ttl_hint_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchSessionCookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub expires_at_unix: Option<u64>,
    pub secure: bool,
    pub http_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchSession {
    pub schema_version: String,
    pub domain: String,
    pub source_url: String,
    pub user_agent: Option<String>,
    pub obtained_at: String,
    pub obtained_at_unix: u64,
    pub ttl_hint_seconds: Option<u64>,
    pub expires_at_unix: Option<u64>,
    pub cookies: Vec<ResearchSessionCookie>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchSessionSummary {
    pub domain: String,
    pub source_url: String,
    pub cookie_count: usize,
    pub cookie_names: Vec<String>,
    pub user_agent_pinned: bool,
    pub obtained_at: String,
    pub expires_at: Option<String>,
    pub expired: bool,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchSessionImportResult {
    pub session: ResearchSession,
    pub path: String,
}

#[derive(Debug, Clone)]
struct ParsedCookieInput {
    cookies: Vec<ResearchSessionCookie>,
    user_agent: Option<String>,
    source_url: Option<String>,
    notes: Vec<String>,
}

pub fn import_research_session(
    paths: &ResolvedPaths,
    source_url: &str,
    raw_cookie_input: &str,
    options: &ResearchSessionImportOptions,
) -> Result<ResearchSessionImportResult> {
    let parsed_url = Url::parse(source_url)
        .with_context(|| format!("failed to parse session source URL: {source_url}"))?;
    let domain = normalize_session_domain(
        parsed_url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("session source URL has no host: {source_url}"))?,
    )?;
    let mut parsed = parse_cookie_input(raw_cookie_input, Some(source_url))?;
    parsed
        .cookies
        .retain(|cookie| !cookie.name.trim().is_empty());
    if parsed.cookies.is_empty() {
        bail!("research session import requires at least one cookie");
    }

    for cookie in &mut parsed.cookies {
        if let Some(cookie_domain) = cookie.domain.as_deref() {
            cookie.domain = Some(normalize_session_domain(cookie_domain)?);
        }
        if cookie.path.as_deref().unwrap_or("").trim().is_empty() {
            cookie.path = Some("/".to_string());
        }
    }

    let now = unix_timestamp().unwrap_or(0);
    let ttl_hint_seconds = options.ttl_hint_seconds;
    let ttl_expires_at = ttl_hint_seconds.map(|ttl| now.saturating_add(ttl));
    let cookie_expires_at = parsed
        .cookies
        .iter()
        .filter_map(|cookie| cookie.expires_at_unix)
        .min();
    let expires_at_unix = match (ttl_expires_at, cookie_expires_at) {
        (Some(ttl), Some(cookie)) => Some(ttl.min(cookie)),
        (Some(ttl), None) => Some(ttl),
        (None, Some(cookie)) => Some(cookie),
        (None, None) => None,
    };
    let session = ResearchSession {
        schema_version: RESEARCH_SESSION_SCHEMA_VERSION.to_string(),
        domain: domain.clone(),
        source_url: parsed
            .source_url
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| source_url.to_string()),
        user_agent: options.user_agent.clone().or(parsed.user_agent),
        obtained_at: now_iso8601_utc(),
        obtained_at_unix: now,
        ttl_hint_seconds,
        expires_at_unix,
        cookies: parsed.cookies,
        notes: parsed.notes,
    };

    let path = session_path(paths, &domain)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }
    write_session_file(&path, &session)?;

    Ok(ResearchSessionImportResult {
        session,
        path: normalize_path(path),
    })
}

fn write_session_file(path: &Path, session: &ResearchSession) -> Result<()> {
    let payload = serde_json::to_string_pretty(session)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("failed to write {}", normalize_path(path)))?;
        file.write_all(payload.as_bytes())
            .with_context(|| format!("failed to write {}", normalize_path(path)))?;
    }
    #[cfg(not(unix))]
    {
        fs::write(path, payload.as_bytes())
            .with_context(|| format!("failed to write {}", normalize_path(path)))?;
    }
    Ok(())
}

pub fn list_research_sessions(paths: &ResolvedPaths) -> Result<Vec<ResearchSessionSummary>> {
    let mut summaries = Vec::new();
    for (session, path) in read_all_sessions(paths)? {
        summaries.push(summarize_session(&session, &path));
    }
    summaries.sort_by(|left, right| left.domain.cmp(&right.domain));
    Ok(summaries)
}

pub fn show_research_session(
    paths: &ResolvedPaths,
    domain_or_url: &str,
) -> Result<Option<ResearchSessionSummary>> {
    let domain = normalize_domain_or_url(domain_or_url)?;
    let path = session_path(paths, &domain)?;
    let Some(session) = read_session(&path)? else {
        return Ok(None);
    };
    Ok(Some(summarize_session(&session, &path)))
}

pub fn clear_research_session(paths: &ResolvedPaths, domain_or_url: &str) -> Result<bool> {
    let domain = normalize_domain_or_url(domain_or_url)?;
    let path = session_path(paths, &domain)?;
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(&path).with_context(|| format!("failed to remove {}", normalize_path(path)))?;
    Ok(true)
}

pub fn prune_research_sessions(paths: &ResolvedPaths) -> Result<Vec<ResearchSessionSummary>> {
    let mut removed = Vec::new();
    for (session, path) in read_all_sessions(paths)? {
        let summary = summarize_session(&session, &path);
        if summary.expired {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", normalize_path(path)))?;
            removed.push(summary);
        }
    }
    Ok(removed)
}

pub fn load_research_session_for_url(
    paths: &ResolvedPaths,
    url: &str,
) -> Result<Option<ExternalFetchSession>> {
    let parsed_url =
        Url::parse(url).with_context(|| format!("failed to parse session target URL: {url}"))?;
    let Some(host) = parsed_url.host_str() else {
        return Ok(None);
    };
    let host = normalize_session_domain(host)?;
    let path = parsed_url.path();
    let now = unix_timestamp().unwrap_or(0);
    let mut candidates = Vec::new();
    for (session, _) in read_all_sessions(paths)? {
        if session_is_expired(&session, now) || !domain_matches(&host, &session.domain) {
            continue;
        }
        let cookie_header = cookie_header_for_request(&session, &host, path, now);
        if cookie_header.is_empty() {
            continue;
        }
        candidates.push(ExternalFetchSession {
            domain: session.domain.clone(),
            cookie_header,
            user_agent: session.user_agent.clone(),
        });
    }
    candidates.sort_by(|left, right| right.domain.len().cmp(&left.domain.len()));
    Ok(candidates.into_iter().next())
}

fn parse_cookie_input(raw: &str, fallback_source_url: Option<&str>) -> Result<ParsedCookieInput> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("cookie input is empty");
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return parse_json_cookie_input(&value, fallback_source_url);
    }
    if looks_like_netscape_cookie_file(trimmed) {
        return Ok(ParsedCookieInput {
            cookies: parse_netscape_cookie_lines(trimmed)?,
            user_agent: None,
            source_url: fallback_source_url.map(ToString::to_string),
            notes: vec!["imported_from_netscape_cookie_file".to_string()],
        });
    }
    Ok(ParsedCookieInput {
        cookies: parse_cookie_header(trimmed, None)?,
        user_agent: None,
        source_url: fallback_source_url.map(ToString::to_string),
        notes: vec!["imported_from_cookie_header".to_string()],
    })
}

fn parse_json_cookie_input(
    value: &Value,
    fallback_source_url: Option<&str>,
) -> Result<ParsedCookieInput> {
    let mut notes = vec!["imported_from_json".to_string()];
    let mut user_agent = None;
    let mut source_url = fallback_source_url.map(ToString::to_string);
    let mut cookies = Vec::new();

    match value {
        Value::Object(map) => {
            user_agent = string_field(value, &["user_agent", "ua"]);
            source_url = string_field(value, &["url", "source_url"]).or(source_url);
            if let Some(cookie_value) = map.get("cookies") {
                cookies.extend(parse_json_cookies_value(cookie_value)?);
            } else if map.contains_key("name") && map.contains_key("value") {
                cookies.push(parse_json_cookie_object(value)?);
            } else {
                let object_cookies = map
                    .iter()
                    .filter_map(|(name, value)| value.as_str().map(|v| (name, v)))
                    .map(|(name, value)| ResearchSessionCookie {
                        name: name.clone(),
                        value: value.to_string(),
                        domain: None,
                        path: Some("/".to_string()),
                        expires_at_unix: None,
                        secure: false,
                        http_only: false,
                    })
                    .collect::<Vec<_>>();
                if !object_cookies.is_empty() {
                    notes.push("interpreted_json_object_as_cookie_map".to_string());
                    cookies.extend(object_cookies);
                }
            }
        }
        Value::Array(_) | Value::String(_) => {
            cookies.extend(parse_json_cookies_value(value)?);
        }
        _ => {}
    }

    Ok(ParsedCookieInput {
        cookies,
        user_agent,
        source_url,
        notes,
    })
}

fn parse_json_cookies_value(value: &Value) -> Result<Vec<ResearchSessionCookie>> {
    match value {
        Value::String(raw) => parse_cookie_header(raw, None),
        Value::Array(items) => items.iter().map(parse_json_cookie_object).collect(),
        Value::Object(_) => {
            if value.get("name").is_some() && value.get("value").is_some() {
                Ok(vec![parse_json_cookie_object(value)?])
            } else {
                let mut cookies = Vec::new();
                let Some(map) = value.as_object() else {
                    return Ok(cookies);
                };
                for (name, value) in map {
                    if let Some(value) = value.as_str() {
                        cookies.push(ResearchSessionCookie {
                            name: name.clone(),
                            value: value.to_string(),
                            domain: None,
                            path: Some("/".to_string()),
                            expires_at_unix: None,
                            secure: false,
                            http_only: false,
                        });
                    }
                }
                Ok(cookies)
            }
        }
        _ => Ok(Vec::new()),
    }
}

fn parse_json_cookie_object(value: &Value) -> Result<ResearchSessionCookie> {
    let name = string_field(value, &["name"])
        .ok_or_else(|| anyhow::anyhow!("JSON cookie object is missing `name`"))?;
    let cookie_value = string_field(value, &["value"])
        .ok_or_else(|| anyhow::anyhow!("JSON cookie object is missing `value`"))?;
    Ok(ResearchSessionCookie {
        name,
        value: cookie_value,
        domain: string_field(value, &["domain"]),
        path: string_field(value, &["path"]).or_else(|| Some("/".to_string())),
        expires_at_unix: u64_field(
            value,
            &[
                "expires_at_unix",
                "expires",
                "expirationDate",
                "expiration_date",
            ],
        ),
        secure: bool_field(value, &["secure"]),
        http_only: bool_field(value, &["http_only", "httpOnly"]),
    })
}

fn parse_cookie_header(raw: &str, domain: Option<String>) -> Result<Vec<ResearchSessionCookie>> {
    let mut cookies = Vec::new();
    for part in raw.split(';') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let trimmed = trimmed
            .strip_prefix("Cookie:")
            .or_else(|| trimmed.strip_prefix("cookie:"))
            .unwrap_or(trimmed)
            .trim();
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty()
            || matches!(
                name.to_ascii_lowercase().as_str(),
                "path" | "domain" | "expires" | "max-age" | "samesite" | "secure" | "httponly"
            )
        {
            continue;
        }
        cookies.push(ResearchSessionCookie {
            name: name.to_string(),
            value: value.trim().to_string(),
            domain: domain.clone(),
            path: Some("/".to_string()),
            expires_at_unix: None,
            secure: false,
            http_only: false,
        });
    }
    if cookies.is_empty() {
        bail!("cookie header did not contain any name=value cookies");
    }
    Ok(cookies)
}

fn looks_like_netscape_cookie_file(raw: &str) -> bool {
    raw.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && (!trimmed.starts_with('#') || trimmed.starts_with("#HttpOnly_"))
            && trimmed.split('\t').count() >= 7
    })
}

fn parse_netscape_cookie_lines(raw: &str) -> Result<Vec<ResearchSessionCookie>> {
    let mut cookies = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || (trimmed.starts_with('#') && !trimmed.starts_with("#HttpOnly_")) {
            continue;
        }
        let parts = trimmed.split('\t').collect::<Vec<_>>();
        if parts.len() < 7 {
            continue;
        }
        let expires_at_unix = parts[4].parse::<u64>().ok().filter(|value| *value > 0);
        cookies.push(ResearchSessionCookie {
            name: parts[5].to_string(),
            value: parts[6].to_string(),
            domain: Some(parts[0].to_string()),
            path: Some(parts[2].to_string()),
            expires_at_unix,
            secure: parts[3].eq_ignore_ascii_case("TRUE"),
            http_only: parts[0].starts_with("#HttpOnly_"),
        });
    }
    if cookies.is_empty() {
        bail!("Netscape cookie input did not contain any cookies");
    }
    Ok(cookies)
}

fn cookie_header_for_request(
    session: &ResearchSession,
    host: &str,
    request_path: &str,
    now: u64,
) -> String {
    session
        .cookies
        .iter()
        .filter(|cookie| {
            cookie
                .expires_at_unix
                .is_none_or(|expires_at| expires_at > now)
        })
        .filter(|cookie| {
            let domain = cookie.domain.as_deref().unwrap_or(&session.domain);
            domain_matches(host, domain)
        })
        .filter(|cookie| {
            let path = cookie.path.as_deref().unwrap_or("/");
            request_path.starts_with(path)
        })
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join("; ")
}

fn summarize_session(session: &ResearchSession, path: &Path) -> ResearchSessionSummary {
    let now = unix_timestamp().unwrap_or(0);
    let mut cookie_names = session
        .cookies
        .iter()
        .map(|cookie| cookie.name.clone())
        .collect::<Vec<_>>();
    cookie_names.sort();
    cookie_names.dedup();
    ResearchSessionSummary {
        domain: session.domain.clone(),
        source_url: session.source_url.clone(),
        cookie_count: session.cookies.len(),
        cookie_names,
        user_agent_pinned: session.user_agent.is_some(),
        obtained_at: session.obtained_at.clone(),
        expires_at: session.expires_at_unix.map(format_iso8601_utc),
        expired: session_is_expired(session, now),
        path: normalize_path(path),
    }
}

fn session_is_expired(session: &ResearchSession, now: u64) -> bool {
    session
        .expires_at_unix
        .is_some_and(|expires_at| expires_at <= now)
}

fn read_all_sessions(paths: &ResolvedPaths) -> Result<Vec<(ResearchSession, PathBuf)>> {
    let directory = research_session_dir(paths);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&directory)
        .with_context(|| format!("failed to read {}", normalize_path(&directory)))?
    {
        let entry = entry
            .with_context(|| format!("failed to read entry in {}", normalize_path(&directory)))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if let Some(session) = read_session(&path)? {
            sessions.push((session, path));
        }
    }
    Ok(sessions)
}

fn read_session(path: &Path) -> Result<Option<ResearchSession>> {
    if !path.exists() {
        return Ok(None);
    }
    let payload = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", normalize_path(path)))?;
    let session = serde_json::from_str::<ResearchSession>(&payload)
        .with_context(|| format!("failed to parse {}", normalize_path(path)))?;
    Ok(Some(session))
}

fn session_path(paths: &ResolvedPaths, domain: &str) -> Result<PathBuf> {
    let domain = normalize_session_domain(domain)?;
    Ok(research_session_dir(paths).join(format!("{domain}.json")))
}

fn research_session_dir(paths: &ResolvedPaths) -> PathBuf {
    paths.state_dir.join("research").join("sessions")
}

fn normalize_domain_or_url(value: &str) -> Result<String> {
    if let Ok(url) = Url::parse(value) {
        let Some(host) = url.host_str() else {
            bail!("URL has no host: {value}");
        };
        return normalize_session_domain(host);
    }
    normalize_session_domain(value)
}

fn normalize_session_domain(value: &str) -> Result<String> {
    let normalized = value
        .trim()
        .trim_start_matches("#HttpOnly_")
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.contains('/')
        || normalized.contains('\\')
        || normalized.contains(':')
        || normalized == "."
        || normalized == ".."
    {
        bail!("invalid session domain: {value}");
    }
    Ok(normalized)
}

fn domain_matches(host: &str, domain: &str) -> bool {
    let Ok(domain) = normalize_session_domain(domain) else {
        return false;
    };
    host.eq_ignore_ascii_case(&domain)
        || host
            .to_ascii_lowercase()
            .ends_with(&format!(".{}", domain.to_ascii_lowercase()))
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn bool_field(value: &Value, keys: &[&str]) -> bool {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_bool))
        .unwrap_or(false)
}

fn u64_field(value: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        let Some(value) = value.get(*key) else {
            continue;
        };
        if let Some(number) = value.as_u64() {
            return Some(number);
        }
        if let Some(number) = value.as_f64()
            && number.is_finite()
            && number >= 0.0
        {
            return Some(number as u64);
        }
        if let Some(text) = value.as_str()
            && let Ok(number) = text.parse::<u64>()
        {
            return Some(number);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::runtime::{ResolvedPaths, ValueSource};

    use super::*;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "wikitool-research-session-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp test dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn test_paths(project_root: &Path) -> ResolvedPaths {
        let state_dir = project_root.join(".wikitool");
        let data_dir = state_dir.join("data");
        fs::create_dir_all(&data_dir).expect("data dir");
        ResolvedPaths {
            project_root: project_root.to_path_buf(),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: state_dir.clone(),
            data_dir: data_dir.clone(),
            db_path: data_dir.join("wikitool.db"),
            config_path: state_dir.join("config.toml"),
            parser_config_path: state_dir.join("parser-config.json"),
            root_source: ValueSource::Default,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn imports_cookie_header_and_loads_matching_session() {
        let temp = TestDir::new("header");
        let paths = test_paths(&temp.path);

        import_research_session(
            &paths,
            "https://example.com/protected",
            "cf_clearance=abc; other=def",
            &ResearchSessionImportOptions {
                user_agent: Some("TestAgent/1.0".to_string()),
                ttl_hint_seconds: Some(1800),
            },
        )
        .expect("import session");

        let session = load_research_session_for_url(&paths, "https://example.com/page")
            .expect("load session")
            .expect("session");

        assert_eq!(session.domain, "example.com");
        assert_eq!(session.user_agent.as_deref(), Some("TestAgent/1.0"));
        assert!(session.cookie_header.contains("cf_clearance=abc"));
        assert!(session.cookie_header.contains("other=def"));
    }

    #[test]
    fn imports_bookmarklet_json_cookie_map() {
        let parsed = parse_cookie_input(
            r#"{"url":"https://example.org/a","ua":"Agent","cookies":{"cf_clearance":"abc"}}"#,
            None,
        )
        .expect("parse json");

        assert_eq!(parsed.source_url.as_deref(), Some("https://example.org/a"));
        assert_eq!(parsed.user_agent.as_deref(), Some("Agent"));
        assert_eq!(parsed.cookies[0].name, "cf_clearance");
        assert_eq!(parsed.cookies[0].value, "abc");
    }

    #[test]
    fn imports_netscape_cookie_file() {
        let parsed = parse_cookie_input(
            ".example.net\tTRUE\t/\tTRUE\t1893456000\tanubis-auth\tjwt",
            Some("https://example.net"),
        )
        .expect("parse netscape");

        assert_eq!(parsed.cookies[0].domain.as_deref(), Some(".example.net"));
        assert_eq!(parsed.cookies[0].name, "anubis-auth");
        assert_eq!(parsed.cookies[0].expires_at_unix, Some(1_893_456_000));
    }

    #[test]
    fn imports_netscape_http_only_cookie_lines() {
        let parsed = parse_cookie_input(
            "#HttpOnly_.example.net\tTRUE\t/\tTRUE\t1893456000\tcf_clearance\tsecret",
            Some("https://example.net"),
        )
        .expect("parse netscape httponly");

        assert_eq!(
            parsed.cookies[0].domain.as_deref(),
            Some("#HttpOnly_.example.net")
        );
        assert_eq!(parsed.cookies[0].name, "cf_clearance");
        assert!(parsed.cookies[0].http_only);
    }

    #[test]
    fn expired_sessions_are_not_loaded_and_can_be_pruned() {
        let temp = TestDir::new("expired");
        let paths = test_paths(&temp.path);
        import_research_session(
            &paths,
            "https://example.com/protected",
            "cf_clearance=abc",
            &ResearchSessionImportOptions {
                user_agent: None,
                ttl_hint_seconds: Some(1),
            },
        )
        .expect("import session");
        let path = session_path(&paths, "example.com").expect("path");
        let mut session = read_session(&path).expect("read").expect("session");
        session.expires_at_unix = Some(1);
        fs::write(&path, serde_json::to_string_pretty(&session).expect("json")).expect("write");

        assert!(
            load_research_session_for_url(&paths, "https://example.com/page")
                .expect("load")
                .is_none()
        );
        let pruned = prune_research_sessions(&paths).expect("prune");

        assert_eq!(pruned.len(), 1);
        assert!(!path.exists());
    }
}
