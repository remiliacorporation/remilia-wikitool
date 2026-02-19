use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde::Serialize;

use crate::filesystem::{ScanOptions, scan_files, validate_scoped_path};
use crate::runtime::ResolvedPaths;

#[derive(Debug, Clone)]
pub struct DeleteOptions {
    pub reason: String,
    pub no_backup: bool,
    pub backup_dir: Option<PathBuf>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteReport {
    pub title: String,
    pub reason: String,
    pub relative_path: String,
    pub dry_run: bool,
    pub backup_path: Option<String>,
    pub deleted_local_file: bool,
    pub deleted_index_rows: usize,
}

pub fn delete_local_page(
    paths: &ResolvedPaths,
    title: &str,
    options: &DeleteOptions,
) -> Result<DeleteReport> {
    let normalized_title = normalize_title(title);
    if normalized_title.is_empty() {
        bail!("delete requires a non-empty title");
    }
    if options.reason.trim().is_empty() {
        bail!("delete requires a non-empty reason");
    }

    let file = scan_files(paths, &ScanOptions::default())?
        .into_iter()
        .find(|item| item.title.eq_ignore_ascii_case(&normalized_title))
        .ok_or_else(|| anyhow::anyhow!("page not found locally: {normalized_title}"))?;

    let absolute_path = absolute_path_from_relative(paths, &file.relative_path);
    validate_scoped_path(paths, &absolute_path)?;

    let backup_path = if options.no_backup {
        None
    } else {
        Some(plan_backup_path(
            paths,
            &normalized_title,
            options.backup_dir.as_deref(),
        )?)
    };

    if options.dry_run {
        return Ok(DeleteReport {
            title: file.title,
            reason: options.reason.trim().to_string(),
            relative_path: file.relative_path,
            dry_run: true,
            backup_path: backup_path.as_ref().map(|path| normalize_path(path)),
            deleted_local_file: false,
            deleted_index_rows: 0,
        });
    }

    if let Some(backup_path) = &backup_path {
        let content = fs::read_to_string(&absolute_path)
            .with_context(|| format!("failed to read {}", absolute_path.display()))?;
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create backup dir {}", parent.display()))?;
        }
        fs::write(backup_path, content)
            .with_context(|| format!("failed to write backup {}", backup_path.display()))?;
    }

    fs::remove_file(&absolute_path)
        .with_context(|| format!("failed to remove local file {}", absolute_path.display()))?;
    let deleted_index_rows = delete_from_index(paths, &file.relative_path)?;

    Ok(DeleteReport {
        title: file.title,
        reason: options.reason.trim().to_string(),
        relative_path: file.relative_path,
        dry_run: false,
        backup_path: backup_path.as_ref().map(|path| normalize_path(path)),
        deleted_local_file: true,
        deleted_index_rows,
    })
}

fn plan_backup_path(
    paths: &ResolvedPaths,
    title: &str,
    backup_dir: Option<&Path>,
) -> Result<PathBuf> {
    let directory = match backup_dir {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => paths.project_root.join(dir),
        None => paths.state_dir.join("backups").join("deleted"),
    };
    validate_scoped_path(paths, &directory)?;

    let normalized_state = normalize_pathbuf(&paths.state_dir);
    let normalized_dir = normalize_pathbuf(&directory);
    if !normalized_dir.starts_with(&normalized_state) {
        bail!(
            "backup directory must be under .wikitool/: {}",
            normalize_path(&normalized_dir)
        );
    }

    let timestamp = unix_timestamp()?;
    let safe_title = sanitize_title_for_filename(title);
    Ok(directory.join(format!("{safe_title}_{timestamp}.wiki")))
}

fn delete_from_index(paths: &ResolvedPaths, relative_path: &str) -> Result<usize> {
    if !paths.db_path.exists() {
        return Ok(0);
    }

    let connection = Connection::open(&paths.db_path)
        .with_context(|| format!("failed to open {}", paths.db_path.display()))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .context("failed to set sqlite busy timeout")?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .context("failed to enable foreign_keys pragma")?;

    if !table_exists(&connection, "indexed_pages")? {
        return Ok(0);
    }

    let deleted = connection
        .execute(
            "DELETE FROM indexed_pages WHERE relative_path = ?1",
            [relative_path],
        )
        .with_context(|| format!("failed to delete indexed row for {relative_path}"))?;
    Ok(deleted)
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool> {
    let exists: i64 = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to inspect sqlite_master for table {table_name}"))?;
    Ok(exists == 1)
}

fn absolute_path_from_relative(paths: &ResolvedPaths, relative: &str) -> PathBuf {
    let mut out = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            out.push(segment);
        }
    }
    out
}

fn sanitize_title_for_filename(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for ch in title.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || ch.is_whitespace()
        {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

fn normalize_title(value: &str) -> String {
    value.replace('_', " ").trim().to_string()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_pathbuf(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            std::path::Component::RootDir => output.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                output.pop();
            }
            std::path::Component::Normal(part) => output.push(part),
        }
    }
    output
}

fn unix_timestamp() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")
        .map(|duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use super::{DeleteOptions, delete_local_page};
    use crate::filesystem::ScanOptions;
    use crate::index::{load_stored_index_stats, rebuild_index};
    use crate::runtime::{ResolvedPaths, ValueSource};

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, content).expect("write");
    }

    fn paths(project_root: &Path) -> ResolvedPaths {
        ResolvedPaths {
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool").join("data"),
            db_path: project_root
                .join(".wikitool")
                .join("data")
                .join("wikitool.db"),
            config_path: project_root.join(".wikitool").join("config.toml"),
            parser_config_path: project_root.join(".wikitool").join("remilia-parser.json"),
            project_root: project_root.to_path_buf(),
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn delete_creates_backup_and_removes_file() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "alpha body",
        );
        fs::create_dir_all(paths.state_dir.join("backups").join("deleted"))
            .expect("create backups");

        let report = delete_local_page(
            &paths,
            "Alpha",
            &DeleteOptions {
                reason: "cleanup".to_string(),
                no_backup: false,
                backup_dir: None,
                dry_run: false,
            },
        )
        .expect("delete");

        assert!(report.deleted_local_file);
        let deleted_path = paths.wiki_content_dir.join("Main").join("Alpha.wiki");
        assert!(!deleted_path.exists());
        let backup_path = report.backup_path.expect("backup path");
        assert!(PathBuf::from(backup_path).exists());
    }

    #[test]
    fn delete_dry_run_keeps_original_file() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "alpha body",
        );
        fs::create_dir_all(paths.state_dir.join("backups").join("deleted"))
            .expect("create backups");

        let report = delete_local_page(
            &paths,
            "Alpha",
            &DeleteOptions {
                reason: "preview".to_string(),
                no_backup: false,
                backup_dir: None,
                dry_run: true,
            },
        )
        .expect("dry run");

        assert!(!report.deleted_local_file);
        assert!(
            paths
                .wiki_content_dir
                .join("Main")
                .join("Alpha.wiki")
                .exists()
        );
        if let Some(backup_path) = report.backup_path {
            assert!(!PathBuf::from(backup_path).exists());
        }
    }

    #[test]
    fn delete_updates_index_rows_when_index_exists() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).expect("create root");
        let paths = paths(&project_root);

        write_file(
            &paths.wiki_content_dir.join("Main").join("Alpha.wiki"),
            "[[Beta]]",
        );
        write_file(
            &paths.wiki_content_dir.join("Main").join("Beta.wiki"),
            "beta body",
        );
        rebuild_index(&paths, &ScanOptions::default()).expect("rebuild");

        let before = load_stored_index_stats(&paths)
            .expect("stats")
            .expect("stats exist");
        assert_eq!(before.indexed_rows, 2);

        let report = delete_local_page(
            &paths,
            "Alpha",
            &DeleteOptions {
                reason: "cleanup".to_string(),
                no_backup: true,
                backup_dir: None,
                dry_run: false,
            },
        )
        .expect("delete");
        assert_eq!(report.deleted_index_rows, 1);

        let after = load_stored_index_stats(&paths)
            .expect("stats")
            .expect("stats exist");
        assert_eq!(after.indexed_rows, 1);
    }
}
