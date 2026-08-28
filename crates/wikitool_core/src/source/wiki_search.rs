use anyhow::Result;

use crate::config::WikiConfig;
use crate::mw::client_from_wikitool_config;

pub use mediawiki_protocol::{
    ExternalSearchHit, ExternalSearchReport, MediaWikiSearchOptions, MediaWikiSearchWhat,
};

pub fn search_external_wiki(
    query: &str,
    namespaces: &[i32],
    limit: usize,
) -> Result<Vec<ExternalSearchHit>> {
    Ok(search_external_wiki_report(query, namespaces, limit, MediaWikiSearchWhat::Text)?.hits)
}

pub fn search_external_wiki_report(
    query: &str,
    namespaces: &[i32],
    limit: usize,
    what: MediaWikiSearchWhat,
) -> Result<ExternalSearchReport> {
    search_external_wiki_report_with_config(query, namespaces, limit, what, &WikiConfig::default())
}

pub fn search_external_wiki_with_config(
    query: &str,
    namespaces: &[i32],
    limit: usize,
    config: &WikiConfig,
) -> Result<Vec<ExternalSearchHit>> {
    Ok(search_external_wiki_report_with_config(
        query,
        namespaces,
        limit,
        MediaWikiSearchWhat::Text,
        config,
    )?
    .hits)
}

pub fn search_external_wiki_report_with_config(
    query: &str,
    namespaces: &[i32],
    limit: usize,
    what: MediaWikiSearchWhat,
    config: &WikiConfig,
) -> Result<ExternalSearchReport> {
    let mut client = client_from_wikitool_config(config)?;
    mediawiki_protocol::search_pages_report(
        &mut client,
        query,
        &MediaWikiSearchOptions {
            namespaces: namespaces.to_vec(),
            limit,
            what,
        },
    )
}
