use std::collections::BTreeSet;

use anyhow::{Result, bail};
use rusqlite::Connection;

use crate::content_store::parsing::open_indexed_connection;
use crate::profile::{
    ProfileOverlay, TemplateCatalog, WikiCapabilityManifest, build_template_catalog_with_overlay,
    load_latest_wiki_capabilities, load_or_build_remilia_profile_overlay, scan_local_asset_titles,
    scan_local_module_titles,
};
use crate::runtime::ResolvedPaths;

use super::REMILIA_PROFILE_ID;

#[derive(Debug)]
pub(super) struct LoadedResources {
    pub(super) overlay: ProfileOverlay,
    pub(super) capabilities: Option<WikiCapabilityManifest>,
    pub(super) template_catalog: Option<TemplateCatalog>,
    pub(super) local_module_titles: BTreeSet<String>,
    pub(super) local_asset_titles: BTreeSet<String>,
    pub(super) index_connection: Option<Connection>,
}

pub(super) fn load_resources(paths: &ResolvedPaths, profile_id: &str) -> Result<LoadedResources> {
    let overlay = if profile_id.eq_ignore_ascii_case(REMILIA_PROFILE_ID) {
        load_or_build_remilia_profile_overlay(paths)?
    } else {
        bail!("unsupported article lint profile: {profile_id}");
    };

    let capabilities = if paths.db_path.exists() {
        load_latest_wiki_capabilities(paths)?
    } else {
        None
    };
    let template_catalog = {
        let built = build_template_catalog_with_overlay(paths, &overlay)?;
        if built.entries.is_empty() {
            None
        } else {
            Some(built)
        }
    };
    let local_module_titles = scan_local_module_titles(paths)?;
    let local_asset_titles = scan_local_asset_titles(paths)?;
    let index_connection = open_indexed_connection(paths)?;

    Ok(LoadedResources {
        overlay,
        capabilities,
        template_catalog,
        local_module_titles,
        local_asset_titles,
        index_connection,
    })
}
