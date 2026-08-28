//! Exact-content editorial authorization and publication policy.
//!
//! This crate has no Wikitool configuration, site-adapter, catalog, or runtime
//! dependency. Callers resolve a publication authority and a narrow workspace
//! before invoking it.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

mod acceptance;
mod changeset;
mod preflight;
mod store;
mod support;

pub use acceptance::*;
pub use changeset::*;
pub use preflight::EncyclopedicPublicationPreflight;

#[derive(Debug, Clone)]
pub struct PublicationWorkspace {
    pub project_root: PathBuf,
    pub wiki_content_dir: PathBuf,
    pub state_dir: PathBuf,
    pub acceptance_store_path: PathBuf,
}

impl PublicationWorkspace {
    pub fn acceptance_store_path(&self) -> PathBuf {
        self.acceptance_store_path.clone()
    }

    pub fn validate_scoped_path(&self, candidate: &Path) -> Result<()> {
        let absolute = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.project_root.join(candidate)
        };
        let normalized = support::resolve_existing_ancestor(&absolute)?;
        let content = support::resolve_existing_ancestor(&self.wiki_content_dir)?;
        let state = support::resolve_existing_ancestor(&self.state_dir)?;
        if normalized.starts_with(content) || normalized.starts_with(state) {
            return Ok(());
        }
        bail!(
            "publication path escapes content and state roots: {}",
            normalized.display()
        )
    }
}

#[cfg(test)]
mod tests;
