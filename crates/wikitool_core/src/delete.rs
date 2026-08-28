use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::filesystem::{ScanOptions, scan_files, validate_scoped_path};
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{compute_sha256, normalize_path, normalize_pathbuf, table_exists};

#[derive(Debug, Clone, Serialize)]
pub struct LocalDeleteEffectPlan {
    pub tracked_title: Option<String>,
    pub relative_path: Option<String>,
    pub policy: wikitool_sync::RemoteDeleteLocalEffectPolicy,
}

/// Resolve the exact local effect that the sync-owned delete plan must bind.
/// This function is read-only: backup creation and source staging happen only
/// inside the durable sync mutation state machine.
pub fn plan_local_delete_effect(
    paths: &ResolvedPaths,
    title: &str,
    no_backup: bool,
    backup_dir: Option<&Path>,
) -> Result<LocalDeleteEffectPlan> {
    let requested = wikitool_sync::mediawiki_title_identity(title);
    if requested.is_empty() {
        bail!("delete requires a non-empty title");
    }
    let mut matches = scan_files(paths, &ScanOptions::default())?
        .into_iter()
        .filter(|item| wikitool_sync::mediawiki_title_identity(&item.title) == requested)
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        let paths = matches
            .iter()
            .map(|item| item.relative_path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("multiple local sources resolve to MediaWiki title identity {requested:?}: {paths}");
    }
    let Some(file) = matches.pop() else {
        return Ok(LocalDeleteEffectPlan {
            tracked_title: None,
            relative_path: None,
            policy: wikitool_sync::RemoteDeleteLocalEffectPolicy {
                backup_enabled: false,
                backup_directory: None,
                backup_path: None,
                local_content_sha256: None,
            },
        });
    };

    let absolute_path = paths.project_root.join(&file.relative_path);
    validate_scoped_path(paths, &absolute_path)?;
    let content = fs::read_to_string(&absolute_path)
        .with_context(|| format!("failed to read {}", absolute_path.display()))?;
    let content_sha256 = compute_sha256(&content);
    let (backup_directory, backup_path) = if no_backup {
        (None, None)
    } else {
        let directory = resolve_backup_directory(paths, backup_dir)?;
        let safe_title = sanitize_title_for_filename(&file.title);
        let backup = directory.join(format!("{safe_title}_{content_sha256}.wiki"));
        (
            Some(normalize_path(&directory)),
            Some(normalize_path(&backup)),
        )
    };
    Ok(LocalDeleteEffectPlan {
        tracked_title: Some(file.title),
        relative_path: Some(file.relative_path),
        policy: wikitool_sync::RemoteDeleteLocalEffectPolicy {
            backup_enabled: !no_backup,
            backup_directory,
            backup_path,
            local_content_sha256: Some(content_sha256),
        },
    })
}

/// The catalog index is derived state, not publication authority. Remove its
/// stale row after a terminal delete receipt; a crash merely requires rebuild.
pub fn remove_deleted_page_from_index(
    paths: &ResolvedPaths,
    relative_path: Option<&str>,
) -> Result<usize> {
    let Some(relative_path) = relative_path else {
        return Ok(0);
    };
    if !paths.db_path.exists() {
        return Ok(0);
    }
    let connection = open_initialized_database_connection(&paths.db_path)?;
    if !table_exists(&connection, "indexed_pages")? {
        return Ok(0);
    }
    connection
        .execute(
            "DELETE FROM indexed_pages WHERE relative_path = ?1",
            [relative_path],
        )
        .with_context(|| format!("failed to delete indexed row for {relative_path}"))
}

fn resolve_backup_directory(paths: &ResolvedPaths, backup_dir: Option<&Path>) -> Result<PathBuf> {
    let sync_state_dir = paths
        .sync_store_path()
        .parent()
        .expect("sync store path has a parent")
        .to_path_buf();
    let directory = match backup_dir {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => paths.project_root.join(dir),
        None => sync_state_dir.join("backups").join("deleted"),
    };
    validate_scoped_path(paths, &directory)?;
    let normalized_state = normalize_pathbuf(&sync_state_dir);
    let normalized_dir = normalize_pathbuf(&directory);
    if !normalized_dir.starts_with(&normalized_state) {
        bail!(
            "delete backup directory must be under the durable sync state root .wikitool/sync/: {}",
            normalize_path(&normalized_dir)
        );
    }
    Ok(normalized_dir)
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::plan_local_delete_effect;
    use crate::runtime::{ResolvedPaths, ValueSource};

    fn paths(project_root: &Path) -> ResolvedPaths {
        ResolvedPaths {
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool/data"),
            db_path: project_root.join(".wikitool/data/wikitool.db"),
            config_path: project_root.join(".wikitool/config.toml"),
            parser_config_path: project_root.join(".wikitool/parser.toml"),
            project_root: project_root.to_path_buf(),
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn local_delete_plan_is_read_only_and_content_bound() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let source = paths.wiki_content_dir.join("Main/Alpha.wiki");
        fs::create_dir_all(source.parent().expect("parent")).expect("content dir");
        fs::write(&source, "alpha body").expect("source");

        let plan = plan_local_delete_effect(&paths, "Alpha", false, None).expect("plan");
        assert_eq!(plan.tracked_title.as_deref(), Some("Alpha"));
        assert!(plan.policy.backup_enabled);
        assert!(plan.policy.local_content_sha256.is_some());
        assert_eq!(
            Path::new(
                plan.policy
                    .backup_directory
                    .as_deref()
                    .expect("backup directory")
            ),
            paths.state_dir.join("sync/backups/deleted")
        );
        assert!(source.exists(), "planning must not mutate the source");
        assert!(
            plan.policy
                .backup_path
                .as_deref()
                .is_some_and(|path| !Path::new(path).exists()),
            "planning must not pre-create the backup"
        );
    }

    #[test]
    fn title_identity_preserves_case_after_the_initial_character() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let upper = paths.wiki_content_dir.join("Main/ALPHA.wiki");
        fs::create_dir_all(upper.parent().expect("parent")).expect("content dir");
        fs::write(&upper, "upper").expect("source");

        let plan = plan_local_delete_effect(&paths, "Alpha", true, None).expect("plan");
        assert!(plan.tracked_title.is_none());
        assert!(upper.exists());
    }

    #[test]
    fn local_delete_backup_is_confined_to_the_sync_state_root() {
        let temp = tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let source = paths.wiki_content_dir.join("Main/Alpha.wiki");
        fs::create_dir_all(source.parent().expect("parent")).expect("content dir");
        fs::write(&source, "alpha body").expect("source");

        let error = plan_local_delete_effect(
            &paths,
            "Alpha",
            false,
            Some(Path::new(".wikitool/backups/deleted")),
        )
        .expect_err("backup outside sync state must fail closed");
        assert!(error.to_string().contains(".wikitool/sync/"));

        let plan = plan_local_delete_effect(
            &paths,
            "Alpha",
            false,
            Some(Path::new(".wikitool/sync/custom-delete-backups")),
        )
        .expect("custom backup under sync state");
        assert_eq!(
            Path::new(
                plan.policy
                    .backup_directory
                    .as_deref()
                    .expect("backup directory")
            ),
            paths.state_dir.join("sync/custom-delete-backups")
        );
    }
}
