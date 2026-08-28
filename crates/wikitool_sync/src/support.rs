use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

/// Replace a file with fully written bytes from a temporary file in the same
/// directory. Persisting within one directory preserves the filesystem's atomic
/// rename boundary, including replacement of an existing target on Windows.
pub fn atomic_write(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("atomic write target has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary file in {}", parent.display()))?;
    temporary
        .write_all(content.as_ref())
        .with_context(|| format!("failed to stage bytes for {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("failed to flush staged bytes for {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to atomically replace {}", path.display()))?;
    Ok(())
}

pub fn compute_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut output = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

/// Full SHA-256 digest for identities and attestations where a compact cache
/// key is not sufficient.
pub fn compute_sha256(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

/// Normalize wiki page content to MediaWiki's canonical stored form for sync comparison.
/// MediaWiki rewrites CR and CRLF line endings to LF and strips trailing whitespace on
/// save, so a local file's trailing newline (the POSIX editor default) would otherwise
/// hash differently from the saved page and drift as "modified" forever. Used only for
/// sync-state hashing — never for content-addressed cache keys, which must hash exact
/// bytes.
pub fn normalize_wiki_content(content: &str) -> String {
    content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end()
        .to_string()
}

/// Hash content for sync-state comparison after normalizing it to MediaWiki's canonical
/// form, so trailing-newline and line-ending differences between a local file and the
/// saved page do not register as spurious modifications.
pub fn compute_wiki_sync_hash(content: &str) -> String {
    compute_hash(&normalize_wiki_content(content))
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        atomic_write, compute_hash, compute_sha256, compute_wiki_sync_hash, normalize_path,
        normalize_pathbuf, normalize_wiki_content, parse_redirect,
    };

    #[test]
    fn atomic_write_replaces_complete_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("state.json");
        std::fs::write(&target, "old").expect("seed");
        atomic_write(&target, b"new complete state").expect("atomic replace");
        assert_eq!(
            std::fs::read_to_string(target).expect("read target"),
            "new complete state"
        );
    }

    #[test]
    fn short_hash_is_stable() {
        assert_eq!(compute_hash("alpha"), "8ed3f6ad685b959e");
    }

    #[test]
    fn full_sha256_is_stable() {
        assert_eq!(
            compute_sha256("alpha"),
            "8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8"
        );
    }

    #[test]
    fn wiki_sync_hash_ignores_trailing_newline_and_line_endings() {
        // A local trailing newline (the POSIX editor default) must not drift against
        // MediaWiki's stripped canonical form.
        assert_eq!(
            compute_wiki_sync_hash("return p\n"),
            compute_wiki_sync_hash("return p")
        );
        // CR and CRLF normalize to LF.
        assert_eq!(
            compute_wiki_sync_hash("a\r\nb"),
            compute_wiki_sync_hash("a\nb")
        );
        assert_eq!(
            compute_wiki_sync_hash("a\rb"),
            compute_wiki_sync_hash("a\nb")
        );
        // The sync hash equals the raw hash of the canonical (normalized) form.
        assert_eq!(compute_wiki_sync_hash("x\n\n"), compute_hash("x"));
        // Genuine content differences still register.
        assert_ne!(compute_wiki_sync_hash("foo"), compute_wiki_sync_hash("bar"));
    }

    #[test]
    fn normalize_wiki_content_matches_mediawiki_canonical_form() {
        assert_eq!(normalize_wiki_content("line\n"), "line");
        assert_eq!(normalize_wiki_content("a\r\nb\r\n"), "a\nb");
        assert_eq!(
            normalize_wiki_content("trailing spaces  \n\n"),
            "trailing spaces"
        );
        // Internal newlines are preserved; only the trailing edge is trimmed.
        assert_eq!(normalize_wiki_content("a\n\nb\n"), "a\n\nb");
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
