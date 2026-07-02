use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::content_store::parsing::normalize_spaces;
use crate::filesystem::{ScanOptions, scan_files};
use crate::knowledge::templates::normalize_module_lookup_title;
use crate::runtime::ResolvedPaths;

use super::super::template_catalog::TemplateCatalog;
use super::super::wiki_capabilities::WikiCapabilityManifest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthoringModuleSurface {
    pub module_title: String,
    pub relative_path: Option<String>,
    pub is_redirect: bool,
    pub redirect_target: Option<String>,
    pub sources: Vec<String>,
    pub used_by_templates: Vec<String>,
    #[serde(default)]
    pub exported_functions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalModuleRecord {
    pub(super) module_title: String,
    pub(super) relative_path: String,
    pub(super) is_redirect: bool,
    pub(super) redirect_target: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ModuleSurfaceAccumulator {
    module_title: String,
    relative_path: Option<String>,
    is_redirect: bool,
    redirect_target: Option<String>,
    sources: BTreeSet<String>,
    used_by_templates: BTreeSet<String>,
}

pub fn normalize_module_title(value: &str) -> String {
    let normalized = normalize_spaces(&value.replace('_', " "));
    if normalized.is_empty() {
        return String::new();
    }
    normalize_module_lookup_title(&normalized)
}

pub fn scan_local_module_titles(paths: &ResolvedPaths) -> Result<BTreeSet<String>> {
    Ok(scan_local_modules(paths)?
        .into_values()
        .map(|module| module.module_title)
        .collect())
}

pub fn supports_invoke_function(capabilities: &WikiCapabilityManifest) -> bool {
    capabilities.has_scribunto
        || capabilities
            .parser_function_hooks
            .iter()
            .any(|hook| hook.eq_ignore_ascii_case("invoke"))
}

pub(super) fn build_module_surfaces(
    catalog: Option<&TemplateCatalog>,
    local_modules: Option<&BTreeMap<String, LocalModuleRecord>>,
    local_module_functions: Option<&BTreeMap<String, BTreeSet<String>>>,
    limit: usize,
) -> Vec<AuthoringModuleSurface> {
    let mut modules = BTreeMap::<String, ModuleSurfaceAccumulator>::new();
    if let Some(local_modules) = local_modules {
        for module in local_modules.values() {
            let key = normalize_module_title(&module.module_title);
            if key.is_empty() || is_module_asset_title(&key) {
                continue;
            }
            let entry = modules
                .entry(key.clone())
                .or_insert_with(|| ModuleSurfaceAccumulator {
                    module_title: key,
                    ..ModuleSurfaceAccumulator::default()
                });
            entry.module_title = module.module_title.clone();
            entry.relative_path = Some(module.relative_path.clone());
            entry.is_redirect = module.is_redirect;
            entry.redirect_target = module.redirect_target.clone();
            entry.sources.insert("local_module_file".to_string());
        }
    }
    if let Some(catalog) = catalog {
        for entry in &catalog.entries {
            for module_title in &entry.module_titles {
                let key = normalize_module_title(module_title);
                if key.is_empty() || is_module_asset_title(&key) {
                    continue;
                }
                let module =
                    modules
                        .entry(key.clone())
                        .or_insert_with(|| ModuleSurfaceAccumulator {
                            module_title: key,
                            ..ModuleSurfaceAccumulator::default()
                        });
                module
                    .sources
                    .insert("template_catalog_reference".to_string());
                module
                    .used_by_templates
                    .insert(entry.template_title.clone());
            }
        }
    }
    let mut out = modules
        .into_iter()
        .map(|(key, module)| AuthoringModuleSurface {
            module_title: module.module_title,
            relative_path: module.relative_path,
            is_redirect: module.is_redirect,
            redirect_target: module.redirect_target,
            sources: module.sources.into_iter().collect(),
            used_by_templates: module.used_by_templates.into_iter().collect(),
            exported_functions: local_module_functions
                .and_then(|functions| functions.get(&key))
                .map(|functions| functions.iter().cloned().collect())
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| {
        right
            .used_by_templates
            .len()
            .cmp(&left.used_by_templates.len())
            .then_with(|| left.module_title.cmp(&right.module_title))
    });
    out.truncate(limit);
    out
}

pub(super) fn count_distinct_modules(
    catalog: Option<&TemplateCatalog>,
    local_modules: Option<&BTreeMap<String, LocalModuleRecord>>,
) -> usize {
    let mut modules = BTreeSet::new();
    if let Some(local_modules) = local_modules {
        for title in local_modules.keys() {
            if !is_module_asset_title(title) {
                modules.insert(title.clone());
            }
        }
    }
    if let Some(catalog) = catalog {
        for entry in &catalog.entries {
            for module_title in &entry.module_titles {
                let normalized = normalize_module_title(module_title);
                if !normalized.is_empty() && !is_module_asset_title(&normalized) {
                    modules.insert(normalized);
                }
            }
        }
    }
    modules.len()
}

pub(super) fn scan_local_modules(
    paths: &ResolvedPaths,
) -> Result<BTreeMap<String, LocalModuleRecord>> {
    let files = scan_files(
        paths,
        &ScanOptions {
            include_content: false,
            include_templates: true,
            custom_content_folders: Vec::new(),
        },
    )?;
    let mut modules = BTreeMap::new();
    for file in files {
        if file.namespace != "Module" {
            continue;
        }
        let normalized = normalize_module_title(&file.title);
        if normalized.is_empty() || is_module_asset_title(&normalized) {
            continue;
        }
        modules.insert(
            normalized,
            LocalModuleRecord {
                module_title: file.title,
                relative_path: file.relative_path,
                is_redirect: file.is_redirect,
                redirect_target: file.redirect_target,
            },
        );
    }
    Ok(modules)
}

fn is_module_asset_title(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    lower.ends_with(".css") || lower.ends_with(".js")
}

/// Scan local Lua module sources and return, per normalized module title, the set of
/// exported `p.<name>` function names. Modules without exported functions are omitted.
pub fn scan_local_module_functions(
    paths: &ResolvedPaths,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let files = scan_files(
        paths,
        &ScanOptions {
            include_content: false,
            include_templates: true,
            custom_content_folders: Vec::new(),
        },
    )?;
    let mut out = BTreeMap::new();
    for file in files {
        if file.namespace != "Module" {
            continue;
        }
        let module_title = normalize_module_title(&file.title);
        if module_title.is_empty() || is_module_asset_title(&module_title) {
            continue;
        }
        let absolute_path = paths.project_root.join(&file.relative_path);
        let content = fs::read_to_string(&absolute_path)
            .with_context(|| format!("failed to read {}", absolute_path.display()))?;
        let functions = extract_lua_exported_functions(&content);
        if !functions.is_empty() {
            out.insert(module_title, functions);
        }
    }
    Ok(out)
}

fn extract_lua_exported_functions(content: &str) -> BTreeSet<String> {
    let content = strip_lua_comments(content);
    let bytes = content.as_bytes();
    let mut functions = BTreeSet::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if starts_with_bytes(bytes, cursor, b"function") && boundary_after(bytes, cursor + 8) {
            let mut next = cursor + 8;
            skip_ascii_whitespace(bytes, &mut next);
            if starts_with_bytes(bytes, next, b"p.") {
                next += 2;
                if let Some((name, end)) = read_lua_identifier(&content, next) {
                    functions.insert(name);
                    cursor = end;
                    continue;
                }
            }
        }

        if starts_with_bytes(bytes, cursor, b"p.") {
            let name_start = cursor + 2;
            if let Some((name, mut next)) = read_lua_identifier(&content, name_start) {
                skip_ascii_whitespace(bytes, &mut next);
                if bytes.get(next).copied() == Some(b'=') {
                    next += 1;
                    skip_ascii_whitespace(bytes, &mut next);
                    if starts_with_bytes(bytes, next, b"function")
                        && boundary_after(bytes, next + 8)
                    {
                        functions.insert(name);
                        cursor = next + 8;
                        continue;
                    }
                }
            }
        }
        cursor += 1;
    }
    functions
}

fn strip_lua_comments(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if starts_with_bytes(bytes, cursor, b"--[[") {
            cursor += 4;
            while cursor + 1 < bytes.len() && !starts_with_bytes(bytes, cursor, b"]]") {
                if bytes[cursor] == b'\n' {
                    out.push('\n');
                }
                cursor += 1;
            }
            cursor = cursor.saturating_add(2).min(bytes.len());
            continue;
        }
        if starts_with_bytes(bytes, cursor, b"--") {
            cursor += 2;
            while cursor < bytes.len() && bytes[cursor] != b'\n' {
                cursor += 1;
            }
            continue;
        }
        out.push(bytes[cursor] as char);
        cursor += 1;
    }
    out
}

fn read_lua_identifier(content: &str, start: usize) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    if !bytes
        .get(start)
        .copied()
        .is_some_and(is_lua_identifier_start)
    {
        return None;
    }
    let mut end = start + 1;
    while end < bytes.len() && is_lua_identifier_continue(bytes[end]) {
        end += 1;
    }
    Some((content[start..end].to_string(), end))
}

fn skip_ascii_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes
        .get(*cursor)
        .copied()
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        *cursor += 1;
    }
}

fn boundary_after(bytes: &[u8], cursor: usize) -> bool {
    !bytes
        .get(cursor)
        .copied()
        .is_some_and(is_lua_identifier_continue)
}

fn starts_with_bytes(bytes: &[u8], cursor: usize, needle: &[u8]) -> bool {
    cursor
        .checked_add(needle.len())
        .is_some_and(|end| bytes.get(cursor..end) == Some(needle))
}

fn is_lua_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_lua_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::extract_lua_exported_functions;

    #[test]
    fn lua_exported_functions_cover_both_declaration_forms() {
        let content = r#"
local p = {}
-- p.commented = function() end
function p.render(frame)
    return frame
end
p.bar_chart = function(frame)
    return frame
end
local function helper()
end
return p
"#;
        let functions = extract_lua_exported_functions(content);
        assert_eq!(
            functions.into_iter().collect::<Vec<_>>(),
            vec!["bar_chart".to_string(), "render".to_string()]
        );
    }
}
