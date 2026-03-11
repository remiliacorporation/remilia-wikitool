use anyhow::Result;

use crate::filesystem::ScanOptions;
use crate::runtime::ResolvedPaths;

pub use crate::index::{RebuildReport, StoredIndexStats};

pub fn rebuild_index(paths: &ResolvedPaths, options: &ScanOptions) -> Result<RebuildReport> {
    crate::index::rebuild_index(paths, options)
}

pub fn load_stored_index_stats(paths: &ResolvedPaths) -> Result<Option<StoredIndexStats>> {
    crate::index::load_stored_index_stats(paths)
}
