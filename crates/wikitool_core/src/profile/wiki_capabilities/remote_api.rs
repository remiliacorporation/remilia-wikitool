use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RemoteApiCandidate {
    pub(super) api_url: String,
    pub(super) wiki_url: String,
}

pub(super) fn remote_mediawiki_api_candidates(source_url: &str) -> Result<Vec<RemoteApiCandidate>> {
    let parsed =
        Url::parse(source_url).with_context(|| format!("invalid remote wiki URL: {source_url}"))?;
    let origin = parsed_origin(&parsed)?;
    let mut out = Vec::new();
    let path = parsed.path().trim_end_matches('/');

    if path.ends_with("/api.php") || path == "/api.php" {
        push_remote_api_candidate(
            &mut out,
            parsed.as_str().to_string(),
            remote_wiki_url(&parsed)?,
        );
        return Ok(out);
    }

    if let Some(prefix) = path.strip_suffix("/index.php") {
        let base_path = prefix.trim_end_matches('/');
        let api_url = format!("{origin}{base_path}/api.php");
        let wiki_url = remote_wiki_url(&Url::parse(&api_url)?)?;
        push_remote_api_candidate(&mut out, api_url, wiki_url);
    }

    if let Some((base_path, _)) = path.split_once("/wiki/") {
        let base_path = base_path.trim_end_matches('/');
        push_remote_api_candidate(
            &mut out,
            format!("{origin}{base_path}/w/api.php"),
            format!("{origin}{base_path}"),
        );
        push_remote_api_candidate(
            &mut out,
            format!("{origin}{base_path}/api.php"),
            format!("{origin}{base_path}"),
        );
    }

    if let Some((base_path, _)) = path.split_once("/w/") {
        let base_path = base_path.trim_end_matches('/');
        push_remote_api_candidate(
            &mut out,
            format!("{origin}{base_path}/w/api.php"),
            format!("{origin}{base_path}"),
        );
    }

    push_remote_api_candidate(&mut out, format!("{origin}/w/api.php"), origin.clone());
    push_remote_api_candidate(&mut out, format!("{origin}/api.php"), origin);

    if out.is_empty() {
        bail!("failed to derive MediaWiki API candidates from `{source_url}`");
    }
    Ok(out)
}

fn parsed_origin(url: &Url) -> Result<String> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("remote wiki URL has no host"))?;
    let port = url
        .port()
        .map(|value| format!(":{value}"))
        .unwrap_or_default();
    Ok(format!("{}://{}{}", url.scheme(), host, port))
}

fn remote_wiki_url(api_url: &Url) -> Result<String> {
    let origin = parsed_origin(api_url)?;
    let path = api_url.path();
    let wiki_path = path
        .strip_suffix("/api.php")
        .map(|value| value.trim_end_matches('/'))
        .unwrap_or("");
    if wiki_path == "/w" {
        return Ok(origin);
    }
    if wiki_path.is_empty() {
        Ok(origin)
    } else {
        Ok(format!("{origin}{wiki_path}"))
    }
}

fn push_remote_api_candidate(out: &mut Vec<RemoteApiCandidate>, api_url: String, wiki_url: String) {
    if out.iter().any(|candidate| candidate.api_url == api_url) {
        return;
    }
    out.push(RemoteApiCandidate { api_url, wiki_url });
}
