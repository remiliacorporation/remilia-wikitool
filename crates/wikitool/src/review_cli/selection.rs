use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use wikitool_core::sync::SyncSelection;

use crate::cli_support::normalize_path;

pub(super) fn review_selection_from_args(
    titles: &[String],
    paths: &[PathBuf],
    titles_file: Option<&PathBuf>,
) -> Result<SyncSelection> {
    let mut loaded_titles = titles.to_vec();
    if let Some(titles_file) = titles_file {
        let content = fs::read_to_string(titles_file)
            .with_context(|| format!("failed to read {}", normalize_path(titles_file)))?;
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                loaded_titles.push(trimmed.to_string());
            }
        }
    }

    Ok(SyncSelection {
        titles: loaded_titles,
        paths: paths.iter().map(normalize_path).collect(),
    })
}
