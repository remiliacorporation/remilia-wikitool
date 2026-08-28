use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

use crate::config::load_config;
use crate::runtime::ResolvedPaths;
use crate::support::{normalize_path, normalize_pathbuf, table_exists};

pub use wikitool_sync::{Namespace, ScanOptions, ScanStats, ScannedFile};

pub fn sync_project_paths(paths: &ResolvedPaths) -> Result<wikitool_sync::SyncProjectPaths> {
    let config = load_config(&paths.config_path)?;
    let custom_namespaces = config
        .wiki
        .custom_namespaces
        .into_iter()
        .map(|namespace| wikitool_sync::SyncNamespace {
            folder: namespace.folder().to_string(),
            id: namespace.id,
            name: namespace.name,
        })
        .collect();
    Ok(wikitool_sync::SyncProjectPaths {
        project_root: paths.project_root.clone(),
        wiki_content_dir: paths.wiki_content_dir.clone(),
        templates_dir: paths.templates_dir.clone(),
        sync_store_path: paths.sync_store_path(),
        custom_namespaces,
        template_category_mappings: load_template_category_mappings(&paths.db_path)?,
    })
}

#[derive(Debug, Clone)]
pub struct NamespaceMapper {
    inner: wikitool_sync::NamespaceMapper,
    sync_paths: wikitool_sync::SyncProjectPaths,
}

impl NamespaceMapper {
    pub fn load(paths: &ResolvedPaths) -> Result<Self> {
        let sync_paths = sync_project_paths(paths)?;
        let inner = wikitool_sync::NamespaceMapper::load(&sync_paths)?;
        Ok(Self { inner, sync_paths })
    }

    pub fn title_to_relative_path(
        &self,
        _paths: &ResolvedPaths,
        title: &str,
        is_redirect: bool,
    ) -> String {
        self.inner
            .title_to_relative_path(&self.sync_paths, title, is_redirect)
    }

    pub fn relative_path_to_title(&self, _paths: &ResolvedPaths, relative_path: &str) -> String {
        self.inner
            .relative_path_to_title(&self.sync_paths, relative_path)
    }

    pub fn custom_folders(&self) -> Vec<String> {
        self.inner.custom_folders()
    }
}

pub fn scan_files(paths: &ResolvedPaths, options: &ScanOptions) -> Result<Vec<ScannedFile>> {
    wikitool_sync::scan_files(&sync_project_paths(paths)?, options)
}

pub fn scan_stats(paths: &ResolvedPaths, options: &ScanOptions) -> Result<ScanStats> {
    wikitool_sync::scan_stats(&sync_project_paths(paths)?, options)
}

pub fn title_to_relative_path(
    paths: &ResolvedPaths,
    title: &str,
    is_redirect: bool,
) -> Result<String> {
    wikitool_sync::title_to_relative_path(&sync_project_paths(paths)?, title, is_redirect)
}

pub fn relative_path_to_title(paths: &ResolvedPaths, relative_path: &str) -> Result<String> {
    wikitool_sync::relative_path_to_title(&sync_project_paths(paths)?, relative_path)
}

pub use wikitool_sync::{
    case_safe_title_relative_path, content_path_to_title, namespace_from_title,
    normalize_separators, template_path_to_title,
};

pub fn validate_scoped_path(paths: &ResolvedPaths, candidate: &Path) -> Result<()> {
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        paths.project_root.join(candidate)
    };
    let normalized = resolve_existing_ancestor(&absolute)?;
    let allowed = [
        resolve_existing_ancestor(&paths.wiki_content_dir)?,
        resolve_existing_ancestor(&paths.templates_dir)?,
        resolve_existing_ancestor(&paths.state_dir)?,
    ];
    if allowed.iter().any(|prefix| normalized.starts_with(prefix)) {
        return Ok(());
    }
    bail!(
        "path escapes scoped runtime directories: {}; allowed roots are {}, {}, and {}",
        normalize_path(&normalized),
        normalize_path(&allowed[0]),
        normalize_path(&allowed[1]),
        normalize_path(&allowed[2])
    )
}

fn resolve_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let normalized = normalize_pathbuf(path);
    if normalized.exists() {
        return fs::canonicalize(&normalized)
            .map(|resolved| normalize_pathbuf(&resolved))
            .with_context(|| format!("failed to canonicalize {}", normalize_path(&normalized)));
    }
    let mut cursor = normalized.as_path();
    let mut suffix = Vec::<OsString>::new();
    loop {
        if cursor.exists() {
            let mut resolved = fs::canonicalize(cursor)
                .map(|path| normalize_pathbuf(&path))
                .with_context(|| format!("failed to canonicalize {}", normalize_path(cursor)))?;
            for segment in suffix.iter().rev() {
                resolved.push(segment);
            }
            return Ok(normalize_pathbuf(&resolved));
        }
        let Some(name) = cursor.file_name() else {
            return Ok(normalized);
        };
        suffix.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            return Ok(normalized);
        };
        cursor = parent;
    }
}

fn load_template_category_mappings(db_path: &Path) -> Result<Vec<(String, String)>> {
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let connection = Connection::open(db_path).with_context(|| {
        format!(
            "failed to open catalog layout hints at {}",
            db_path.display()
        )
    })?;
    if !table_exists(&connection, "template_category_mappings")? {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "SELECT prefix, category FROM template_category_mappings \
         ORDER BY length(prefix) DESC, prefix ASC",
    )?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect::<rusqlite::Result<Vec<(String, String)>>>()
        .context("failed to load resolved template layout hints")
}
