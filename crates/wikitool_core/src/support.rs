use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

pub fn compute_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut output = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

pub fn parse_redirect(content: &str) -> (bool, Option<String>) {
    let trimmed = content.trim();
    if !trimmed.to_ascii_uppercase().starts_with("#REDIRECT") {
        return (false, None);
    }
    if let Some(start) = trimmed.find("[[")
        && let Some(end) = trimmed[start + 2..].find("]]")
    {
        let target = trimmed[start + 2..start + 2 + end].trim().to_string();
        if !target.is_empty() {
            return (true, Some(target));
        }
    }
    (true, None)
}

pub fn normalize_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

pub fn normalize_pathbuf(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            std::path::Component::RootDir => {
                output.push(Path::new(std::path::MAIN_SEPARATOR_STR));
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                output.pop();
            }
            std::path::Component::Normal(part) => output.push(part),
        }
    }
    output
}

pub fn ensure_db_parent(db_path: &Path) -> Result<()> {
    let parent = db_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("db path has no parent: {}", db_path.display()))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create database parent directory {}",
            parent.display()
        )
    })
}

pub fn table_exists(connection: &Connection, table_name: &str) -> Result<bool> {
    let exists: i64 = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to inspect sqlite_master for table {table_name}"))?;
    Ok(exists == 1)
}

pub fn unix_timestamp() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")
        .map(|duration| duration.as_secs())
}

/// Format a UNIX epoch seconds value as RFC 3339 / ISO-8601 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
/// Self-contained so the crate does not need a date/time dependency for the one format
/// we emit publicly. Uses Howard Hinnant's civil-from-days algorithm.
pub fn format_iso8601_utc(unix_seconds: u64) -> String {
    const SECONDS_PER_DAY: u64 = 86_400;
    let days = (unix_seconds / SECONDS_PER_DAY) as i64;
    let time_of_day = unix_seconds % SECONDS_PER_DAY;
    let hour = time_of_day / 3_600;
    let minute = (time_of_day % 3_600) / 60;
    let second = time_of_day % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year_base = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_base + if month <= 2 { 1 } else { 0 };

    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z",
        year = year,
        month = month,
        day = day,
        hour = hour,
        minute = minute,
        second = second,
    )
}

/// Current wall clock in ISO-8601 UTC, or a deterministic epoch string if the clock
/// is unreadable (preserves the previous never-panics contract of `now_timestamp_string`).
pub fn now_iso8601_utc() -> String {
    format_iso8601_utc(unix_timestamp().unwrap_or(0))
}

pub fn env_value(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

pub fn env_value_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

pub fn env_value_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        compute_hash, format_iso8601_utc, normalize_path, normalize_pathbuf, parse_redirect,
    };

    #[test]
    fn short_hash_is_stable() {
        assert_eq!(compute_hash("alpha"), "8ed3f6ad685b959e");
    }

    #[test]
    fn formats_epoch_zero_as_iso8601() {
        assert_eq!(format_iso8601_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn formats_known_checkpoint() {
        assert_eq!(format_iso8601_utc(1_776_211_200), "2026-04-15T00:00:00Z");
    }

    #[test]
    fn formats_leap_day_2024() {
        assert_eq!(format_iso8601_utc(1_709_210_096), "2024-02-29T12:34:56Z");
    }

    #[test]
    fn formats_end_of_year_non_leap() {
        assert_eq!(format_iso8601_utc(1_767_225_599), "2025-12-31T23:59:59Z");
    }

    #[test]
    fn redirect_parser_extracts_target() {
        assert_eq!(
            parse_redirect("#REDIRECT [[Alpha]]"),
            (true, Some("Alpha".to_string()))
        );
        assert_eq!(parse_redirect("plain text"), (false, None));
    }

    #[test]
    fn path_helpers_normalize_separators_and_parents() {
        assert_eq!(normalize_path("a\\b\\c"), "a/b/c");
        assert_eq!(
            normalize_pathbuf(Path::new("wiki_content/../templates")),
            PathBuf::from("templates")
        );
    }
}
