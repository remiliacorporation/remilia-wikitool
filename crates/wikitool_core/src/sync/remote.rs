use super::*;

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
    search_external_wiki_report_with_config(
        query,
        namespaces,
        limit,
        what,
        &crate::config::WikiConfig::default(),
    )
}

pub fn search_external_wiki_with_config(
    query: &str,
    namespaces: &[i32],
    limit: usize,
    config: &crate::config::WikiConfig,
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
    config: &crate::config::WikiConfig,
) -> Result<ExternalSearchReport> {
    let mut client = MediaWikiClient::from_config(config)?;
    search_pages_report(
        &mut client,
        query,
        &MediaWikiSearchOptions {
            namespaces: namespaces.to_vec(),
            limit,
            what,
        },
    )
}

pub fn delete_remote_page(title: &str, reason: &str) -> Result<RemoteDeleteReport> {
    delete_remote_page_with_config(title, reason, &crate::config::WikiConfig::default())
}

pub fn delete_remote_page_with_config(
    title: &str,
    reason: &str,
    config: &crate::config::WikiConfig,
) -> Result<RemoteDeleteReport> {
    let username = match env::var("WIKI_BOT_USER") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            return Ok(RemoteDeleteReport {
                status: RemoteDeleteStatus::SkippedMissingCredentials,
                title: title.to_string(),
                detail: Some("WIKI_BOT_USER is not set".to_string()),
                request_count: 0,
            });
        }
    };
    let password = match env::var("WIKI_BOT_PASS") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            return Ok(RemoteDeleteReport {
                status: RemoteDeleteStatus::SkippedMissingCredentials,
                title: title.to_string(),
                detail: Some("WIKI_BOT_PASS is not set".to_string()),
                request_count: 0,
            });
        }
    };

    let mut client = MediaWikiClient::from_config(config)?;
    client
        .login(username.trim(), password.trim())
        .context("remote delete login failed")?;

    match client.delete_page(title, reason) {
        Ok(()) => Ok(RemoteDeleteReport {
            status: RemoteDeleteStatus::Deleted,
            title: title.to_string(),
            detail: None,
            request_count: client.request_count(),
        }),
        Err(error) => {
            let message = error.to_string();
            if message.contains("missingtitle") {
                Ok(RemoteDeleteReport {
                    status: RemoteDeleteStatus::AlreadyMissing,
                    title: title.to_string(),
                    detail: Some(message),
                    request_count: client.request_count(),
                })
            } else {
                Err(error).context(format!("remote delete failed for {title}"))
            }
        }
    }
}

pub fn discover_custom_namespaces(
    config: &crate::config::WikiConfig,
) -> Result<Vec<crate::config::CustomNamespace>> {
    if config.api_url_owned().is_none() {
        bail!("wiki API URL is not configured (set [wiki].api_url or WIKI_API_URL)");
    }
    let mut client = MediaWikiClient::from_config(config)?;
    client.discover_custom_namespaces()
}
