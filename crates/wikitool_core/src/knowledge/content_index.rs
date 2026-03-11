use anyhow::Result;

use crate::filesystem::ScanOptions;
use crate::runtime::ResolvedPaths;

pub use super::model::{RebuildReport, StoredIndexStats};

pub fn rebuild_index(paths: &ResolvedPaths, options: &ScanOptions) -> Result<RebuildReport> {
    crate::index::ingest::rebuild_index(paths, options)
}

pub fn load_stored_index_stats(paths: &ResolvedPaths) -> Result<Option<StoredIndexStats>> {
    crate::index::ingest::load_stored_index_stats(paths)
}
