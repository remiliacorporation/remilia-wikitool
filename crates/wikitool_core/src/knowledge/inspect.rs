use anyhow::Result;

use crate::runtime::ResolvedPaths;

pub use crate::index::ValidationReport;

pub fn run_validation_checks(paths: &ResolvedPaths) -> Result<Option<ValidationReport>> {
    crate::index::run_validation_checks(paths)
}

pub fn query_backlinks(paths: &ResolvedPaths, title: &str) -> Result<Option<Vec<String>>> {
    crate::index::query_backlinks(paths, title)
}

pub fn query_orphans(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    crate::index::query_orphans(paths)
}

pub fn query_empty_categories(paths: &ResolvedPaths) -> Result<Option<Vec<String>>> {
    crate::index::query_empty_categories(paths)
}
