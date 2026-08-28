use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::ResolvedPaths;
use crate::support::{format_iso8601_utc, normalize_path, now_iso8601_utc, unix_timestamp};

use super::model::ExternalFetchSession;
use super::session_security;

const SOURCE_ACCESS_SESSION_SCHEMA_VERSION: &str = "source_session_v1";

#[derive(Debug, Clone, Default)]
pub struct SourceAccessSessionImportOptions {
    pub user_agent: Option<String>,
    pub ttl_hint_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceAccessSessionCookie {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub expires_at_unix: Option<u64>,
    pub secure: bool,
    pub http_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceAccessSession {
    pub schema_version: String,
    pub domain: String,
    pub source_url: String,
    pub user_agent: Option<String>,
    pub obtained_at: String,
    pub obtained_at_unix: u64,
    pub ttl_hint_seconds: Option<u64>,
    pub expires_at_unix: Option<u64>,
    pub cookies: Vec<SourceAccessSessionCookie>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceAccessSessionSummary {
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
pub struct SourceAccessSessionImportResult {
    pub session: SourceAccessSession,
    pub path: String,
}

#[derive(Debug, Clone)]
struct ParsedCookieInput {
    cookies: Vec<SourceAccessSessionCookie>,
    user_agent: Option<String>,
    source_url: Option<String>,
    notes: Vec<String>,
}

pub fn import_source_access_session(
    paths: &ResolvedPaths,
    source_url: &str,
    raw_cookie_input: &str,
    options: &SourceAccessSessionImportOptions,
) -> Result<SourceAccessSessionImportResult> {
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
        bail!("source session import requires at least one cookie");
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
    let session = SourceAccessSession {
        schema_version: SOURCE_ACCESS_SESSION_SCHEMA_VERSION.to_string(),
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
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("source-session path has no parent"))?;
    ensure_source_access_session_directory(parent, true)?;
    write_session_file(&path, &session)?;

    Ok(SourceAccessSessionImportResult {
        session,
        path: normalize_path(path),
    })
}

fn write_session_file(path: &Path, session: &SourceAccessSession) -> Result<()> {
    let payload = serde_json::to_string_pretty(session)?;
    session_security::write_private_session_file(path, payload.as_bytes()).with_context(|| {
        format!(
            "source-session persistence was rejected for {}",
            normalize_path(path)
        )
    })
}

pub fn list_source_access_sessions(
    paths: &ResolvedPaths,
) -> Result<Vec<SourceAccessSessionSummary>> {
    let mut summaries = Vec::new();
    for (session, path) in read_all_sessions(paths)? {
        summaries.push(summarize_session(&session, &path));
    }
    summaries.sort_by(|left, right| left.domain.cmp(&right.domain));
    Ok(summaries)
}

pub fn show_source_access_session(
    paths: &ResolvedPaths,
    domain_or_url: &str,
) -> Result<Option<SourceAccessSessionSummary>> {
    let domain = normalize_domain_or_url(domain_or_url)?;
    if !ensure_source_access_session_directory(&source_access_session_dir(paths), false)? {
        return Ok(None);
    }
    let path = session_path(paths, &domain)?;
    let Some(session) = read_session(&path)? else {
        return Ok(None);
    };
    Ok(Some(summarize_session(&session, &path)))
}

pub fn clear_source_access_session(paths: &ResolvedPaths, domain_or_url: &str) -> Result<bool> {
    let domain = normalize_domain_or_url(domain_or_url)?;
    if !ensure_source_access_session_directory(&source_access_session_dir(paths), false)? {
        return Ok(false);
    }
    let path = session_path(paths, &domain)?;
    session_security::remove_session_file(&path)
        .with_context(|| format!("failed to remove {}", normalize_path(path)))
}

pub fn prune_source_access_sessions(
    paths: &ResolvedPaths,
) -> Result<Vec<SourceAccessSessionSummary>> {
    let mut removed = Vec::new();
    for (session, path) in read_all_sessions(paths)? {
        let summary = summarize_session(&session, &path);
        if summary.expired {
            if !session_security::remove_session_file(&path)
                .with_context(|| format!("failed to remove {}", normalize_path(&path)))?
            {
                bail!(
                    "source-session file disappeared before pruning: {}",
                    normalize_path(&path)
                );
            }
            removed.push(summary);
        }
    }
    Ok(removed)
}

pub fn load_source_access_session_for_url(
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
        let cookie_header =
            cookie_header_for_request(&session, &host, path, parsed_url.scheme(), now);
        if cookie_header.is_empty() {
            continue;
        }
        candidates.push(ExternalFetchSession {
            domain: session.domain.clone(),
            cookie_header,
            user_agent: session.user_agent.clone(),
        });
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.domain.len()));
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
                    .map(|(name, value)| SourceAccessSessionCookie {
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

fn parse_json_cookies_value(value: &Value) -> Result<Vec<SourceAccessSessionCookie>> {
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
                        cookies.push(SourceAccessSessionCookie {
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

fn parse_json_cookie_object(value: &Value) -> Result<SourceAccessSessionCookie> {
    let name = string_field(value, &["name"])
        .ok_or_else(|| anyhow::anyhow!("JSON cookie object is missing `name`"))?;
    let cookie_value = string_field(value, &["value"])
        .ok_or_else(|| anyhow::anyhow!("JSON cookie object is missing `value`"))?;
    Ok(SourceAccessSessionCookie {
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

fn parse_cookie_header(
    raw: &str,
    domain: Option<String>,
) -> Result<Vec<SourceAccessSessionCookie>> {
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
        cookies.push(SourceAccessSessionCookie {
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

fn parse_netscape_cookie_lines(raw: &str) -> Result<Vec<SourceAccessSessionCookie>> {
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
        cookies.push(SourceAccessSessionCookie {
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
    session: &SourceAccessSession,
    host: &str,
    request_path: &str,
    scheme: &str,
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
        .filter(|cookie| !cookie.secure || scheme.eq_ignore_ascii_case("https"))
        .filter(|cookie| match cookie.domain.as_deref() {
            Some(domain) => domain_matches(host, domain),
            None => host.eq_ignore_ascii_case(&session.domain),
        })
        .filter(|cookie| {
            let path = cookie.path.as_deref().unwrap_or("/");
            cookie_path_matches(request_path, path)
        })
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join("; ")
}

fn cookie_path_matches(request_path: &str, cookie_path: &str) -> bool {
    if request_path == cookie_path {
        return true;
    }
    if !request_path.starts_with(cookie_path) {
        return false;
    }
    cookie_path.ends_with('/')
        || request_path
            .as_bytes()
            .get(cookie_path.len())
            .is_some_and(|value| *value == b'/')
}

fn summarize_session(session: &SourceAccessSession, path: &Path) -> SourceAccessSessionSummary {
    let now = unix_timestamp().unwrap_or(0);
    let mut cookie_names = session
        .cookies
        .iter()
        .map(|cookie| cookie.name.clone())
        .collect::<Vec<_>>();
    cookie_names.sort();
    cookie_names.dedup();
    SourceAccessSessionSummary {
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

fn session_is_expired(session: &SourceAccessSession, now: u64) -> bool {
    session
        .expires_at_unix
        .is_some_and(|expires_at| expires_at <= now)
}

fn read_all_sessions(paths: &ResolvedPaths) -> Result<Vec<(SourceAccessSession, PathBuf)>> {
    let directory = source_access_session_dir(paths);
    if !ensure_source_access_session_directory(&directory, false)? {
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

fn read_session(path: &Path) -> Result<Option<SourceAccessSession>> {
    read_session_with_security(path, session_security::secure_session_file)
}

fn read_session_with_security<F>(path: &Path, secure_file: F) -> Result<Option<SourceAccessSession>>
where
    F: FnOnce(&Path) -> Result<()>,
{
    if !path
        .try_exists()
        .with_context(|| format!("failed to inspect {}", normalize_path(path)))?
    {
        return Ok(None);
    }
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {}", normalize_path(path)))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "source-session entry is not a regular file: {}",
            normalize_path(path)
        );
    }
    secure_file(path).with_context(|| {
        format!(
            "refusing to read source-session data without verified private storage at {}",
            normalize_path(path)
        )
    })?;
    let payload = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", normalize_path(path)))?;
    let session = serde_json::from_str::<SourceAccessSession>(&payload)
        .with_context(|| format!("failed to parse {}", normalize_path(path)))?;
    Ok(Some(session))
}

fn ensure_source_access_session_directory(directory: &Path, create: bool) -> Result<bool> {
    if !directory
        .try_exists()
        .with_context(|| format!("failed to inspect {}", normalize_path(directory)))?
    {
        if !create {
            return Ok(false);
        }
        fs::create_dir_all(directory)
            .with_context(|| format!("failed to create {}", normalize_path(directory)))?;
    }
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("failed to inspect {}", normalize_path(directory)))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "source-session storage path is not a regular directory: {}",
            normalize_path(directory)
        );
    }
    session_security::secure_session_directory(directory).with_context(|| {
        format!(
            "refusing source-session persistence without verified private storage at {}",
            normalize_path(directory)
        )
    })?;
    Ok(true)
}

fn session_path(paths: &ResolvedPaths, domain: &str) -> Result<PathBuf> {
    let domain = normalize_session_domain(domain)?;
    Ok(source_access_session_dir(paths).join(format!("{domain}.json")))
}

fn source_access_session_dir(paths: &ResolvedPaths) -> PathBuf {
    paths.state_dir.join("source").join("sessions")
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
                "wikitool-source-session-{label}-{}-{unique}",
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

        import_source_access_session(
            &paths,
            "https://example.com/protected",
            "cf_clearance=abc; other=def",
            &SourceAccessSessionImportOptions {
                user_agent: Some("TestAgent/1.0".to_string()),
                ttl_hint_seconds: Some(1800),
            },
        )
        .expect("import session");

        let session = load_source_access_session_for_url(&paths, "https://example.com/page")
            .expect("load session")
            .expect("session");

        assert_eq!(session.domain, "example.com");
        assert_eq!(session.user_agent.as_deref(), Some("TestAgent/1.0"));
        assert!(session.cookie_header.contains("cf_clearance=abc"));
        assert!(session.cookie_header.contains("other=def"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_import_persists_verified_current_user_only_acls() {
        let temp = TestDir::new("windows-acl");
        let paths = test_paths(&temp.path);

        let imported = import_source_access_session(
            &paths,
            "https://example.com/protected",
            "cf_clearance=private-value",
            &SourceAccessSessionImportOptions::default(),
        )
        .expect("import session");
        let session_file = PathBuf::from(imported.path);
        let session_directory = session_file.parent().expect("session parent");

        session_security::verify_session_directory(session_directory)
            .expect("current-user-only directory ACL");
        session_security::verify_session_file(&session_file).expect("current-user-only file ACL");
    }

    #[cfg(windows)]
    #[test]
    fn windows_acl_supports_session_paths_beyond_max_path() {
        use std::os::windows::ffi::OsStrExt;

        let temp = TestDir::new("windows-long-acl");
        let mut project_root = temp.path.clone();
        for index in 0..12 {
            project_root.push(format!("long-session-path-segment-{index:02}"));
        }
        fs::create_dir_all(&project_root).expect("create long project path");
        let paths = test_paths(&project_root);
        let session_directory = source_access_session_dir(&paths);
        assert!(
            session_directory.as_os_str().encode_wide().count() > 260,
            "test path must exceed legacy MAX_PATH"
        );

        let imported = import_source_access_session(
            &paths,
            "https://example.com/protected",
            "cf_clearance=private-value",
            &SourceAccessSessionImportOptions::default(),
        )
        .expect("import session through long path");
        let session_file = PathBuf::from(imported.path);

        session_security::verify_session_directory(&session_directory)
            .expect("long directory current-user-only ACL");
        session_security::verify_session_file(&session_file)
            .expect("long file current-user-only ACL");
        assert!(
            clear_source_access_session(&paths, "example.com").expect("clear long-path session")
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_import_and_read_enforce_current_user_only_modes() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TestDir::new("unix-private-mode");
        let paths = test_paths(&temp.path);
        let imported = import_source_access_session(
            &paths,
            "https://example.com/protected",
            "cf_clearance=private-value",
            &SourceAccessSessionImportOptions::default(),
        )
        .expect("import session");
        let session_file = PathBuf::from(imported.path);
        let session_directory = session_file.parent().expect("session parent");

        session_security::verify_session_directory(session_directory).expect("directory mode 0700");
        session_security::verify_session_file(&session_file).expect("file mode 0600");

        fs::set_permissions(session_directory, fs::Permissions::from_mode(0o755))
            .expect("broaden directory fixture mode");
        fs::set_permissions(&session_file, fs::Permissions::from_mode(0o644))
            .expect("broaden file fixture mode");
        show_source_access_session(&paths, "example.com")
            .expect("repair before read")
            .expect("session");

        session_security::verify_session_directory(session_directory)
            .expect("repaired directory mode 0700");
        session_security::verify_session_file(&session_file).expect("repaired file mode 0600");
    }

    #[test]
    fn acl_verification_failure_refuses_existing_session_before_read() {
        let temp = TestDir::new("acl-read-failure");
        let path = temp.path.join("example.com.json");
        let secret = "stored-secret-that-must-not-reach-diagnostics";
        fs::write(
            &path,
            format!(r#"{{"cookies":[{{"name":"session","value":"{secret}"}}]}}"#),
        )
        .expect("write fixture");

        let error =
            read_session_with_security(&path, |_| bail!("simulated ACL verification failure"))
                .expect_err("ACL verification failure must prevent reading");

        assert!(!format!("{error:#}").contains(secret));
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
    fn cookie_scope_enforces_host_only_secure_and_path_boundaries() {
        let session = SourceAccessSession {
            schema_version: SOURCE_ACCESS_SESSION_SCHEMA_VERSION.to_string(),
            domain: "example.com".to_string(),
            source_url: "https://example.com/foo".to_string(),
            user_agent: None,
            obtained_at: "2026-01-01T00:00:00Z".to_string(),
            obtained_at_unix: 1,
            ttl_hint_seconds: None,
            expires_at_unix: None,
            cookies: vec![
                SourceAccessSessionCookie {
                    name: "host_only".to_string(),
                    value: "one".to_string(),
                    domain: None,
                    path: Some("/foo".to_string()),
                    expires_at_unix: None,
                    secure: false,
                    http_only: false,
                },
                SourceAccessSessionCookie {
                    name: "secure_domain".to_string(),
                    value: "two".to_string(),
                    domain: Some("example.com".to_string()),
                    path: Some("/".to_string()),
                    expires_at_unix: None,
                    secure: true,
                    http_only: false,
                },
            ],
            notes: Vec::new(),
        };

        let exact_https =
            cookie_header_for_request(&session, "example.com", "/foo/bar", "https", 2);
        assert!(exact_https.contains("host_only=one"));
        assert!(exact_https.contains("secure_domain=two"));

        let subdomain_http =
            cookie_header_for_request(&session, "sub.example.com", "/foo/bar", "http", 2);
        assert!(!subdomain_http.contains("host_only=one"));
        assert!(!subdomain_http.contains("secure_domain=two"));

        let false_path = cookie_header_for_request(&session, "example.com", "/foobar", "https", 2);
        assert!(!false_path.contains("host_only=one"));
    }

    #[test]
    fn expired_sessions_are_not_loaded_and_can_be_pruned() {
        let temp = TestDir::new("expired");
        let paths = test_paths(&temp.path);
        import_source_access_session(
            &paths,
            "https://example.com/protected",
            "cf_clearance=abc",
            &SourceAccessSessionImportOptions {
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
            load_source_access_session_for_url(&paths, "https://example.com/page")
                .expect("load")
                .is_none()
        );
        let pruned = prune_source_access_sessions(&paths).expect("prune");

        assert_eq!(pruned.len(), 1);
        assert!(!path.exists());
    }
}
