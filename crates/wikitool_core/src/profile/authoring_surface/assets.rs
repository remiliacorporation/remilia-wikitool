use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::content_store::parsing::normalize_spaces;
use crate::filesystem::{ScanOptions, scan_files};
use crate::runtime::ResolvedPaths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringAssetSurface {
    pub title: String,
    pub relative_path: String,
    pub namespace: String,
    pub kind: String,
    pub content_model_hint: String,
    pub is_redirect: bool,
    pub redirect_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalAssetRecord {
    pub(super) title: String,
    pub(super) relative_path: String,
    pub(super) namespace: String,
    pub(super) kind: String,
    pub(super) content_model_hint: String,
    pub(super) is_redirect: bool,
    pub(super) redirect_target: Option<String>,
}

pub fn scan_local_asset_titles(paths: &ResolvedPaths) -> Result<BTreeSet<String>> {
    Ok(scan_local_assets(paths)?
        .into_values()
        .map(|asset| normalize_asset_title(&asset.title).to_ascii_lowercase())
        .collect())
}

pub fn normalize_asset_title(value: &str) -> String {
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return String::new();
    }
    let Some((prefix, rest)) = normalized.split_once(':') else {
        return format!("Template:{normalized}");
    };
    let body = normalize_spaces(rest);
    if body.is_empty() {
        return String::new();
    }
    if prefix.eq_ignore_ascii_case("Template") {
        return format!("Template:{body}");
    }
    if prefix.eq_ignore_ascii_case("Module") {
        return format!("Module:{body}");
    }
    if prefix.eq_ignore_ascii_case("MediaWiki") {
        return format!("MediaWiki:{body}");
    }
    normalized
}

pub(super) fn build_asset_surfaces(
    local_assets: Option<&BTreeMap<String, LocalAssetRecord>>,
    limit: usize,
) -> Vec<AuthoringAssetSurface> {
    let mut assets = local_assets
        .map(|assets| assets.values().collect::<Vec<_>>())
        .unwrap_or_default();
    assets.sort_by(|left, right| {
        asset_kind_rank(&left.kind)
            .cmp(&asset_kind_rank(&right.kind))
            .then_with(|| left.title.cmp(&right.title))
    });
    assets
        .into_iter()
        .take(limit)
        .map(|asset| AuthoringAssetSurface {
            title: asset.title.clone(),
            relative_path: asset.relative_path.clone(),
            namespace: asset.namespace.clone(),
            kind: asset.kind.clone(),
            content_model_hint: asset.content_model_hint.clone(),
            is_redirect: asset.is_redirect,
            redirect_target: asset.redirect_target.clone(),
        })
        .collect()
}

pub(super) fn scan_local_assets(
    paths: &ResolvedPaths,
) -> Result<BTreeMap<String, LocalAssetRecord>> {
    let files = scan_files(
        paths,
        &ScanOptions {
            include_content: false,
            include_templates: true,
            custom_content_folders: Vec::new(),
        },
    )?;
    let mut assets = BTreeMap::new();
    for file in files {
        let Some(kind) = asset_kind(&file.title, &file.namespace) else {
            continue;
        };
        let normalized = normalize_asset_title(&file.title);
        if normalized.is_empty() {
            continue;
        }
        let content_model_hint = content_model_hint_for_title(&normalized);
        assets.insert(
            normalized,
            LocalAssetRecord {
                title: file.title,
                relative_path: file.relative_path,
                namespace: file.namespace,
                kind,
                content_model_hint,
                is_redirect: file.is_redirect,
                redirect_target: file.redirect_target,
            },
        );
    }
    Ok(assets)
}

fn asset_kind(title: &str, namespace: &str) -> Option<String> {
    let lower = title.to_ascii_lowercase();
    match namespace {
        "MediaWiki" => {
            if lower.ends_with(".css") {
                Some("mediawiki_stylesheet".to_string())
            } else if lower.ends_with(".js") {
                Some("mediawiki_script".to_string())
            } else {
                Some("mediawiki_message".to_string())
            }
        }
        "Template" => {
            if lower.ends_with(".css") {
                Some("template_stylesheet".to_string())
            } else if lower.ends_with(".js") {
                Some("template_script".to_string())
            } else {
                None
            }
        }
        "Module" => {
            if lower.ends_with(".css") {
                Some("module_stylesheet".to_string())
            } else if lower.ends_with(".js") {
                Some("module_script".to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn content_model_hint_for_title(title: &str) -> String {
    let lower = title.to_ascii_lowercase();
    if lower.ends_with(".css") {
        "css".to_string()
    } else if lower.ends_with(".js") {
        "javascript".to_string()
    } else {
        "wikitext".to_string()
    }
}

fn asset_kind_rank(kind: &str) -> usize {
    match kind {
        "mediawiki_stylesheet" => 0,
        "mediawiki_script" => 1,
        "mediawiki_message" => 2,
        "template_stylesheet" => 3,
        "module_stylesheet" => 4,
        "template_script" => 5,
        "module_script" => 6,
        _ => 7,
    }
}
