use anyhow::Result;

use crate::config::{WikiConfig, load_config};
use crate::runtime::ResolvedPaths;
use crate::site::publication_policy_identity_with_config;
use crate::sync::SyncTargetIdentity;

pub use wikitool_publication::EncyclopedicPublicationPreflight;
pub use wikitool_sync::{
    PreparedPublication, PublicationCandidate, PublicationPreflight, PublicationProvenance,
};

pub fn publication_workspace(paths: &ResolvedPaths) -> wikitool_publication::PublicationWorkspace {
    wikitool_publication::PublicationWorkspace {
        project_root: paths.project_root.clone(),
        wiki_content_dir: paths.wiki_content_dir.clone(),
        state_dir: paths.state_dir.clone(),
        acceptance_store_path: paths.acceptance_store_path(),
    }
}

pub fn resolve_article_publication_authority(
    paths: &ResolvedPaths,
) -> Result<wikitool_publication::ArticlePublicationAuthority> {
    let config = load_config(&paths.config_path)?;
    resolve_article_publication_authority_with_config(paths, &config)
}

pub fn resolve_article_publication_authority_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<wikitool_publication::ArticlePublicationAuthority> {
    let target =
        SyncTargetIdentity::from_api_url(config.api_url_owned().as_deref().unwrap_or_default())?;
    let policy = publication_policy_identity_with_config(paths, config)?;
    Ok(wikitool_publication::ArticlePublicationAuthority {
        target_api_url: target.api_url().to_string(),
        site_adapter_id: policy.adapter_id,
        publication_policy_sha256: policy.policy_sha256,
    })
}

pub fn encyclopedic_preflight(paths: &ResolvedPaths) -> Result<EncyclopedicPublicationPreflight> {
    Ok(EncyclopedicPublicationPreflight::new(
        publication_workspace(paths),
        resolve_article_publication_authority(paths)?,
    ))
}
