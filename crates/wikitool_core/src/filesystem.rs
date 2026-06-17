use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use walkdir::WalkDir;

use crate::config::load_config;
use crate::runtime::ResolvedPaths;
use crate::support::{compute_wiki_sync_hash, normalize_pathbuf, parse_redirect};

const CASE_SAFE_TITLE_MARKER: &str = "__mwtitle_";

#[derive(Debug, Clone)]
struct CachedTemplateCategoryMappings {
    modified_at: Option<SystemTime>,
    mappings: Vec<(String, String)>,
}

static TEMPLATE_CATEGORY_CACHE: OnceLock<Mutex<HashMap<String, CachedTemplateCategoryMappings>>> =
    OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Main,
    Category,
    File,
    User,
    Template,
    Module,
    MediaWiki,
}

impl Namespace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::Category => "Category",
            Self::File => "File",
            Self::User => "User",
            Self::Template => "Template",
            Self::Module => "Module",
            Self::MediaWiki => "MediaWiki",
        }
    }
}

#[derive(Debug, Clone)]
struct CustomNamespaceRule {
    name: String,
    folder: String,
}

#[derive(Debug, Clone, Default)]
pub struct NamespaceMapper {
    custom_rules: Vec<CustomNamespaceRule>,
}

impl NamespaceMapper {
    pub fn load(paths: &ResolvedPaths) -> Result<Self> {
        Ok(Self {
            custom_rules: load_custom_namespace_rules(paths)?,
        })
    }

    pub fn title_to_relative_path(
        &self,
        paths: &ResolvedPaths,
        title: &str,
        is_redirect: bool,
    ) -> String {
        title_to_relative_path_with_rules(paths, title, is_redirect, &self.custom_rules)
    }

    pub fn relative_path_to_title(&self, paths: &ResolvedPaths, relative_path: &str) -> String {
        relative_path_to_title_with_rules(paths, relative_path, &self.custom_rules)
    }

    pub fn custom_folders(&self) -> Vec<String> {
        let mut folders = Vec::new();
        for rule in &self.custom_rules {
            if folders
                .iter()
                .any(|folder: &String| folder.eq_ignore_ascii_case(&rule.folder))
            {
                continue;
            }
            folders.push(rule.folder.clone());
        }
        folders
    }

    fn custom_rules(&self) -> &[CustomNamespaceRule] {
        &self.custom_rules
    }
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub include_content: bool,
    pub include_templates: bool,
    pub custom_content_folders: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            include_content: true,
            include_templates: true,
            custom_content_folders: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ScannedFile {
    pub relative_path: String,
    pub title: String,
    pub namespace: String,
    pub is_redirect: bool,
    pub redirect_target: Option<String>,
    pub content_hash: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanStats {
    pub total_files: usize,
    pub content_files: usize,
    pub template_files: usize,
    pub redirects: usize,
    pub by_namespace: BTreeMap<String, usize>,
}

pub fn scan_files(paths: &ResolvedPaths, options: &ScanOptions) -> Result<Vec<ScannedFile>> {
    let mapper = NamespaceMapper::load(paths)?;
    let custom_folders = if options.custom_content_folders.is_empty() {
        mapper.custom_folders()
    } else {
        options.custom_content_folders.clone()
    };

    let mut files = Vec::new();
    if options.include_content && paths.wiki_content_dir.exists() {
        scan_content_files(paths, &custom_folders, mapper.custom_rules(), &mut files)?;
    }
    if options.include_templates && paths.templates_dir.exists() {
        scan_template_files(paths, mapper.custom_rules(), &mut files)?;
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

pub fn scan_stats(paths: &ResolvedPaths, options: &ScanOptions) -> Result<ScanStats> {
    let files = scan_files(paths, options)?;
    let mut by_namespace: BTreeMap<String, usize> = BTreeMap::new();
    let mut content_files = 0usize;
    let mut template_files = 0usize;
    let mut redirects = 0usize;

    for file in &files {
        *by_namespace.entry(file.namespace.clone()).or_insert(0) += 1;
        if file.namespace == Namespace::Template.as_str()
            || file.namespace == Namespace::Module.as_str()
            || file.namespace == Namespace::MediaWiki.as_str()
        {
            template_files += 1;
        } else {
            content_files += 1;
        }
        if file.is_redirect {
            redirects += 1;
        }
    }

    Ok(ScanStats {
        total_files: files.len(),
        content_files,
        template_files,
        redirects,
        by_namespace,
    })
}

/// Check if a title belongs to a custom content namespace (e.g. "Goldenlight:Page").
/// Returns `(folder_name, bare_title)` if the prefix matches a known custom namespace,
/// or `None` if it doesn't.
fn custom_namespace_parts(
    title: &str,
    custom_rules: &[CustomNamespaceRule],
) -> Option<(String, String)> {
    let (prefix, rest) = title.split_once(':')?;
    let normalized_prefix = prefix.trim();
    if normalized_prefix.is_empty() || rest.trim().is_empty() {
        return None;
    }
    for rule in custom_rules {
        if rule.name.eq_ignore_ascii_case(normalized_prefix) {
            return Some((rule.folder.clone(), rest.to_string()));
        }
    }
    None
}

const STANDARD_CONTENT_FOLDERS: &[&str] = &["Main", "Category", "File", "User"];

/// Discover custom content namespace folders by listing directories under wiki_content/
/// that aren't standard namespace folders.
fn custom_content_folders(paths: &ResolvedPaths) -> Vec<String> {
    if !paths.wiki_content_dir.exists() {
        return Vec::new();
    }
    let mut folders = Vec::new();
    let Ok(entries) = fs::read_dir(&paths.wiki_content_dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !STANDARD_CONTENT_FOLDERS.contains(&name_str.as_ref()) {
            folders.push(name_str.to_string());
        }
    }
    folders
}

fn load_custom_namespace_rules(paths: &ResolvedPaths) -> Result<Vec<CustomNamespaceRule>> {
    let mut rules = Vec::new();
    if paths.config_path.exists() {
        let config = load_config(&paths.config_path)?;
        for namespace in config.wiki.custom_namespaces {
            let name = normalize_namespace_token(&namespace.name);
            let folder = normalize_namespace_token(namespace.folder());
            if name.is_empty()
                || folder.is_empty()
                || !is_valid_namespace_name(&name)
                || !is_valid_namespace_folder(&folder)
            {
                continue;
            }
            if rules.iter().any(|rule: &CustomNamespaceRule| {
                rule.name.eq_ignore_ascii_case(&name) || rule.folder.eq_ignore_ascii_case(&folder)
            }) {
                continue;
            }
            rules.push(CustomNamespaceRule { name, folder });
        }
    }

    for folder in custom_content_folders(paths) {
        if !is_valid_namespace_folder(&folder) {
            continue;
        }
        if rules
            .iter()
            .any(|rule| rule.folder.eq_ignore_ascii_case(&folder))
        {
            continue;
        }
        rules.push(CustomNamespaceRule {
            name: folder.clone(),
            folder,
        });
    }
    Ok(rules)
}

fn normalize_namespace_token(value: &str) -> String {
    value.trim().replace('_', " ")
}

fn is_valid_namespace_name(value: &str) -> bool {
    !value.contains(':')
        && !value.contains('/')
        && !value.contains('\\')
        && value != "."
        && value != ".."
}

fn is_valid_namespace_folder(value: &str) -> bool {
    !value.contains('/')
        && !value.contains('\\')
        && !value.contains(':')
        && value != "."
        && value != ".."
}

pub fn title_to_relative_path(
    paths: &ResolvedPaths,
    title: &str,
    is_redirect: bool,
) -> Result<String> {
    let mapper = NamespaceMapper::load(paths)?;
    Ok(mapper.title_to_relative_path(paths, title, is_redirect))
}

pub fn case_safe_title_relative_path(relative_path: &str, title: &str) -> String {
    let normalized = normalize_separators(relative_path);
    let (directory, filename) = normalized
        .rsplit_once('/')
        .map(|(directory, filename)| (Some(directory), filename))
        .unwrap_or((None, normalized.as_str()));
    let (stem, extension) = split_known_extension(filename);
    let encoded_title = hex_encode(title.as_bytes());
    let filename = format!("{stem}{CASE_SAFE_TITLE_MARKER}{encoded_title}{extension}");
    match directory {
        Some(directory) if !directory.is_empty() => format!("{directory}/{filename}"),
        _ => filename,
    }
}

fn title_to_relative_path_with_rules(
    paths: &ResolvedPaths,
    title: &str,
    is_redirect: bool,
    custom_rules: &[CustomNamespaceRule],
) -> String {
    // Check custom namespaces first (they aren't in the Namespace enum)
    if let Some((folder, bare)) = custom_namespace_parts(title, custom_rules) {
        let filename = bare
            .replace(' ', "_")
            .replace('/', "___")
            .replace(':', "--");
        let content_rel = rel_from_root(paths, &paths.wiki_content_dir);
        if is_redirect {
            return format!("{content_rel}/{folder}/_redirects/{filename}.wiki");
        }
        return format!("{content_rel}/{folder}/{filename}.wiki");
    }

    let namespace = namespace_from_title(title);
    let content_rel = rel_from_root(paths, &paths.wiki_content_dir);
    let templates_rel = rel_from_root(paths, &paths.templates_dir);
    let bare_title = title_without_namespace(title);
    let filename = title_to_filename(title);

    if is_redirect {
        if matches!(
            namespace,
            Namespace::Template | Namespace::Module | Namespace::MediaWiki
        ) {
            let category = template_category_with_db(title, Some(&paths.db_path));
            let encoded = match namespace {
                Namespace::Template => format!("Template_{}", bare_title.replace(' ', "_")),
                Namespace::Module => {
                    if bare_title.ends_with("/styles.css") {
                        let base = bare_title
                            .strip_suffix("/styles.css")
                            .unwrap_or(bare_title)
                            .replace(' ', "_");
                        format!("Module_{base}_styles")
                    } else {
                        format!("Module_{}", bare_title.replace(' ', "_"))
                    }
                }
                Namespace::MediaWiki => bare_title.replace(' ', "_"),
                _ => unreachable!(),
            };
            return format!("{templates_rel}/{category}/_redirects/{encoded}.wiki");
        }
        return format!(
            "{}/{}/_redirects/{}.wiki",
            content_rel,
            namespace_folder(namespace),
            filename
        );
    }

    if matches!(
        namespace,
        Namespace::Template | Namespace::Module | Namespace::MediaWiki
    ) {
        let category = template_category_with_db(title, Some(&paths.db_path));
        let extension = file_extension(namespace, title);
        let encoded = match namespace {
            Namespace::Template => format!("Template_{}", bare_title.replace(' ', "_")),
            Namespace::Module => {
                if bare_title.ends_with("/styles.css") {
                    let base = bare_title
                        .strip_suffix("/styles.css")
                        .unwrap_or(bare_title)
                        .replace(' ', "_");
                    format!("Module_{base}_styles")
                } else {
                    format!("Module_{}", bare_title.replace(' ', "_"))
                }
            }
            Namespace::MediaWiki => {
                if bare_title.ends_with(".css") || bare_title.ends_with(".js") {
                    return format!("{templates_rel}/{category}/{bare_title}");
                }
                bare_title.to_string()
            }
            _ => unreachable!(),
        };
        return format!("{templates_rel}/{category}/{encoded}{extension}");
    }

    format!(
        "{}/{}/{}{}",
        content_rel,
        namespace_folder(namespace),
        filename,
        file_extension(namespace, title)
    )
}

pub fn relative_path_to_title(paths: &ResolvedPaths, relative_path: &str) -> Result<String> {
    let mapper = NamespaceMapper::load(paths)?;
    Ok(mapper.relative_path_to_title(paths, relative_path))
}

fn relative_path_to_title_with_rules(
    paths: &ResolvedPaths,
    relative_path: &str,
    custom_rules: &[CustomNamespaceRule],
) -> String {
    let normalized = normalize_separators(relative_path);
    if let Some(title) = case_safe_title_from_path(&normalized) {
        return title;
    }
    let content_rel = rel_from_root(paths, &paths.wiki_content_dir);
    let templates_rel = rel_from_root(paths, &paths.templates_dir);

    if let Some(rest) = normalized.strip_prefix(&format!("{content_rel}/")) {
        return content_path_to_title_with_rules(rest, custom_rules);
    }
    if let Some(rest) = normalized.strip_prefix(&format!("{templates_rel}/")) {
        return template_path_to_title(rest);
    }

    decode_segment(strip_known_extensions(
        Path::new(&normalized)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(relative_path),
    ))
}

pub fn validate_scoped_path(paths: &ResolvedPaths, candidate: &Path) -> Result<()> {
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        paths.project_root.join(candidate)
    };
    let normalized = resolve_scoped_candidate_path(&absolute)?;
    let allowed = [
        resolve_scope_root(&paths.wiki_content_dir)?,
        resolve_scope_root(&paths.templates_dir)?,
        resolve_scope_root(&paths.state_dir)?,
    ];

    if allowed.iter().any(|prefix| normalized.starts_with(prefix)) {
        return Ok(());
    }

    bail!(
        "path escapes scoped runtime directories: {}\nallowed roots:\n  - {}\n  - {}\n  - {}\n\nDrafts authored off-wiki (for example when composing against `wikitool knowledge article-start`) belong under the state directory — `{}/drafts/` is the conventional scratch location.",
        display_path(&normalized),
        display_path(&allowed[0]),
        display_path(&allowed[1]),
        display_path(&allowed[2]),
        display_path(&allowed[2])
    )
}

fn resolve_scope_root(path: &Path) -> Result<PathBuf> {
    resolve_existing_ancestor(path)
}

fn resolve_scoped_candidate_path(path: &Path) -> Result<PathBuf> {
    resolve_existing_ancestor(path)
}

fn resolve_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let normalized = normalize_pathbuf(path);
    if normalized.exists() {
        return fs::canonicalize(&normalized)
            .map(|resolved| normalize_pathbuf(&resolved))
            .with_context(|| format!("failed to canonicalize {}", display_path(&normalized)));
    }

    let mut cursor = normalized.as_path();
    let mut suffix = Vec::<OsString>::new();
    loop {
        if cursor.exists() {
            let mut resolved = fs::canonicalize(cursor)
                .map(|value| normalize_pathbuf(&value))
                .with_context(|| format!("failed to canonicalize {}", display_path(cursor)))?;
            for segment in suffix.iter().rev() {
                resolved.push(segment);
            }
            return Ok(normalize_pathbuf(&resolved));
        }

        let Some(name) = cursor.file_name() else {
            return Ok(normalized);
        };
        suffix.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            return Ok(normalized);
        };
        cursor = parent;
    }
}

fn scan_content_files(
    paths: &ResolvedPaths,
    custom_folders: &[String],
    custom_rules: &[CustomNamespaceRule],
    out: &mut Vec<ScannedFile>,
) -> Result<()> {
    let content_rel = rel_from_root(paths, &paths.wiki_content_dir);
    let standard = ["Main", "Category", "File", "User"];
    for folder in standard
        .iter()
        .copied()
        .chain(custom_folders.iter().map(String::as_str))
    {
        let base = paths.wiki_content_dir.join(folder);
        if !base.exists() {
            continue;
        }
        for entry in WalkDir::new(&base).follow_links(false) {
            let entry = entry.with_context(|| format!("failed to walk {}", base.display()))?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("wiki") {
                continue;
            }
            validate_scoped_path(paths, path)?;
            let relative = relative_from_root(paths, path)?;
            if !normalize_separators(&relative).starts_with(&format!("{content_rel}/")) {
                continue;
            }
            out.push(read_scanned_file(paths, path, &relative, custom_rules)?);
        }
    }
    Ok(())
}

fn scan_template_files(
    paths: &ResolvedPaths,
    custom_rules: &[CustomNamespaceRule],
    out: &mut Vec<ScannedFile>,
) -> Result<()> {
    let templates_rel = rel_from_root(paths, &paths.templates_dir);
    if !paths.templates_dir.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(&paths.templates_dir).follow_links(false) {
        let entry =
            entry.with_context(|| format!("failed to walk {}", paths.templates_dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|item| item.to_str())
            .unwrap_or("");
        if !matches!(ext, "wiki" | "wikitext" | "lua" | "css" | "js") {
            continue;
        }
        validate_scoped_path(paths, path)?;
        let relative = relative_from_root(paths, path)?;
        let normalized = normalize_separators(&relative);
        if !normalized.starts_with(&format!("{templates_rel}/")) {
            continue;
        }
        if !is_syncable_template_path(&normalized, &templates_rel) {
            continue;
        }
        out.push(read_scanned_file(paths, path, &relative, custom_rules)?);
    }
    Ok(())
}

fn read_scanned_file(
    paths: &ResolvedPaths,
    path: &Path,
    relative: &str,
    custom_rules: &[CustomNamespaceRule],
) -> Result<ScannedFile> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    let (is_redirect, redirect_target) = parse_redirect(&content);
    let title = relative_path_to_title_with_rules(paths, relative, custom_rules);
    let namespace = namespace_string_from_title(&title, custom_rules);

    Ok(ScannedFile {
        relative_path: normalize_separators(relative),
        title,
        namespace,
        is_redirect,
        redirect_target,
        content_hash: compute_wiki_sync_hash(&content),
        bytes: metadata.len(),
    })
}

fn is_syncable_template_path(relative: &str, templates_rel: &str) -> bool {
    let rest = relative
        .strip_prefix(&format!("{templates_rel}/"))
        .unwrap_or(relative);
    let segments: Vec<&str> = rest
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.contains(&"_redirects") {
        return true;
    }
    if segments
        .iter()
        .any(|segment| segment.starts_with("Template_") || segment.starts_with("Module_"))
    {
        return true;
    }
    segments.first().copied() == Some("mediawiki")
}

pub fn content_path_to_title(content_rel_path: &str) -> String {
    content_path_to_title_with_rules(content_rel_path, &[])
}

fn content_path_to_title_with_rules(
    content_rel_path: &str,
    custom_rules: &[CustomNamespaceRule],
) -> String {
    let normalized = normalize_separators(content_rel_path);
    if let Some(title) = case_safe_title_from_path(&normalized) {
        return title;
    }
    let mut segments: Vec<&str> = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return String::new();
    }

    let folder = segments.remove(0);
    segments.retain(|segment| *segment != "_redirects");
    let filename = segments.last().copied().unwrap_or("");
    let name = decode_segment(strip_known_extensions(filename));
    match folder {
        "Main" => name,
        other => {
            if let Some(rule) = custom_rules
                .iter()
                .find(|rule| rule.folder.eq_ignore_ascii_case(other))
            {
                return format!("{}:{name}", rule.name);
            }
            if matches!(other, "Category" | "File" | "User") {
                return format!("{other}:{name}");
            }
            format!("{other}:{name}")
        }
    }
}

pub fn template_path_to_title(templates_rel_path: &str) -> String {
    let normalized = normalize_separators(templates_rel_path);
    if let Some(title) = case_safe_title_from_path(&normalized) {
        return title;
    }
    let raw_segments: Vec<&str> = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let segments: Vec<&str> = raw_segments
        .iter()
        .copied()
        .filter(|segment| *segment != "_redirects")
        .collect();
    if segments.is_empty() {
        return String::new();
    }

    let category = segments[0];
    let rest = &segments[1..];

    if category == "mediawiki" {
        if rest.is_empty() {
            return "MediaWiki:".to_string();
        }
        let mut parts = Vec::new();
        for (index, segment) in rest.iter().enumerate() {
            let value = if index == rest.len() - 1 {
                strip_subpage_extension(segment)
            } else {
                segment
            };
            parts.push(decode_segment(value));
        }
        return format!("MediaWiki:{}", parts.join("/"));
    }

    if let Some(base_index) = rest
        .iter()
        .position(|segment| segment.starts_with("Template_") || segment.starts_with("Module_"))
    {
        let base = rest[base_index];
        let base_ext = Path::new(base)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        let clean_base = strip_base_extension(base);
        let is_module = clean_base.starts_with("Module_");
        let is_template = clean_base.starts_with("Template_");
        if is_module || is_template {
            let namespace = if is_module { "Module" } else { "Template" };
            let mut base_name = clean_base[if is_module { 7 } else { 9 }..].to_string();
            let mut subpages: Vec<&str> = rest[base_index + 1..].to_vec();
            if is_module
                && subpages.is_empty()
                && base_name.ends_with("_styles")
                && base_ext == "css"
            {
                base_name.truncate(base_name.len().saturating_sub(7));
                subpages = vec!["styles.css"];
            }
            let base_title = decode_segment(&base_name);
            if subpages.is_empty() {
                return format!("{namespace}:{base_title}");
            }
            let mut decoded = Vec::new();
            for (index, segment) in subpages.iter().enumerate() {
                let value = if index == subpages.len() - 1 {
                    strip_subpage_extension(segment)
                } else {
                    segment
                };
                decoded.push(decode_segment(value));
            }
            return format!("{namespace}:{base_title}/{}", decoded.join("/"));
        }
    }

    let filename = rest.last().copied().unwrap_or("");
    let name = strip_known_extensions(filename);
    if let Some(template) = name.strip_prefix("Template_") {
        return format!("Template:{}", decode_segment(template));
    }
    if let Some(module) = name.strip_prefix("Module_") {
        if module.ends_with("_styles")
            && Path::new(filename).extension().and_then(|ext| ext.to_str()) == Some("css")
        {
            let base = &module[..module.len().saturating_sub(7)];
            return format!("Module:{}/styles.css", decode_segment(base));
        }
        return format!("Module:{}", decode_segment(module));
    }

    decode_segment(name)
}

fn relative_from_root(paths: &ResolvedPaths, path: &Path) -> Result<String> {
    let rel = path.strip_prefix(&paths.project_root).with_context(|| {
        format!(
            "failed to derive relative path from root {} for {}",
            paths.project_root.display(),
            path.display()
        )
    })?;
    Ok(display_path(rel))
}

fn rel_from_root(paths: &ResolvedPaths, path: &Path) -> String {
    match path.strip_prefix(&paths.project_root) {
        Ok(rel) => display_path(rel),
        Err(_) => display_path(path),
    }
}

pub fn namespace_from_title(title: &str) -> Namespace {
    if title.starts_with("Category:") {
        Namespace::Category
    } else if title.starts_with("File:") {
        Namespace::File
    } else if title.starts_with("User:") {
        Namespace::User
    } else if title.starts_with("Template:") {
        Namespace::Template
    } else if title.starts_with("Module:") {
        Namespace::Module
    } else if title.starts_with("MediaWiki:") {
        Namespace::MediaWiki
    } else {
        Namespace::Main
    }
}

/// Returns the namespace name as a string, handling both standard and custom namespaces.
/// For standard namespaces, returns the canonical name (e.g. "Category").
/// For custom namespaces (like "Goldenlight:"), extracts the prefix before the colon.
/// For Main namespace titles, returns "Main".
fn namespace_string_from_title(title: &str, custom_rules: &[CustomNamespaceRule]) -> String {
    let ns = namespace_from_title(title);
    if ns != Namespace::Main {
        return ns.as_str().to_string();
    }
    if let Some((prefix, _)) = title.split_once(':') {
        for rule in custom_rules {
            if rule.name.eq_ignore_ascii_case(prefix.trim()) {
                return rule.name.clone();
            }
        }
    }
    "Main".to_string()
}

fn namespace_folder(namespace: Namespace) -> &'static str {
    match namespace {
        Namespace::Main => "Main",
        Namespace::Category => "Category",
        Namespace::File => "File",
        Namespace::User => "User",
        Namespace::Template | Namespace::Module | Namespace::MediaWiki => "Main",
    }
}

fn title_without_namespace(title: &str) -> &str {
    for prefix in NAMESPACE_PREFIXES {
        if let Some(value) = title.strip_prefix(prefix) {
            return value;
        }
    }
    title
}

const NAMESPACE_PREFIXES: &[&str] = &[
    "Category:",
    "File:",
    "User:",
    "Template:",
    "Module:",
    "MediaWiki:",
];

fn title_to_filename(title: &str) -> String {
    title_without_namespace(title)
        .replace(' ', "_")
        .replace('/', "___")
        .replace(':', "--")
}

fn decode_segment(value: &str) -> String {
    value
        .replace("___", "/")
        .replace("--", ":")
        .replace('_', " ")
}

fn strip_known_extensions(value: &str) -> &str {
    split_known_extension(value).0
}

fn strip_base_extension(value: &str) -> &str {
    strip_one_of(value, &[".wiki", ".wikitext", ".lua", ".css"])
}

fn strip_subpage_extension(value: &str) -> &str {
    strip_one_of(value, &[".wiki", ".wikitext", ".lua"])
}

fn strip_one_of<'a>(value: &'a str, extensions: &[&str]) -> &'a str {
    for ext in extensions {
        if let Some(stripped) = value.strip_suffix(ext) {
            return stripped;
        }
    }
    value
}

fn split_known_extension(value: &str) -> (&str, &str) {
    for ext in [".wikitext", ".wiki", ".lua", ".css", ".js"] {
        if let Some(stripped) = value.strip_suffix(ext) {
            return (stripped, ext);
        }
    }
    (value, "")
}

fn case_safe_title_from_path(path: &str) -> Option<String> {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let stem = strip_known_extensions(filename);
    let (_, encoded) = stem.rsplit_once(CASE_SAFE_TITLE_MARKER)?;
    let bytes = hex_decode(encoded)?;
    String::from_utf8(bytes).ok()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(hex_digit(byte >> 4));
        out.push(hex_digit(byte & 0x0f));
    }
    out
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        10..=15 => char::from(b'a' + value - 10),
        _ => unreachable!(),
    }
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let mut chars = value.bytes();
    while let (Some(high), Some(low)) = (chars.next(), chars.next()) {
        bytes.push((hex_value(high)? << 4) | hex_value(low)?);
    }
    Some(bytes)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn file_extension(namespace: Namespace, title: &str) -> &'static str {
    match namespace {
        Namespace::Module => {
            if title.ends_with("/styles.css") {
                ".css"
            } else {
                ".lua"
            }
        }
        Namespace::MediaWiki => {
            if title.ends_with(".css") {
                ".css"
            } else if title.ends_with(".js") {
                ".js"
            } else {
                ".wiki"
            }
        }
        _ => ".wiki",
    }
}

fn template_category_with_db(title: &str, db_path: Option<&std::path::Path>) -> Cow<'static, str> {
    if title.starts_with("MediaWiki:") {
        return Cow::Borrowed("mediawiki");
    }
    if let Some(path) = db_path
        && let Some(category) = template_category_from_db(path, title)
    {
        return Cow::Owned(category);
    }
    Cow::Borrowed("misc")
}

/// Query the template_category_mappings table for a matching prefix.
/// Returns None if the table doesn't exist or no match is found.
fn template_category_from_db(db_path: &std::path::Path, title: &str) -> Option<String> {
    if !db_path.exists() {
        return None;
    }

    let cache_key = normalize_separators(&db_path.to_string_lossy());
    let modified_at = fs::metadata(db_path)
        .ok()
        .and_then(|meta| meta.modified().ok());
    let cache = TEMPLATE_CATEGORY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().ok()?;

    let needs_refresh = guard
        .get(&cache_key)
        .is_none_or(|entry| entry.modified_at != modified_at);
    if needs_refresh {
        let mappings = load_template_category_mappings(db_path)?;
        guard.insert(
            cache_key.clone(),
            CachedTemplateCategoryMappings {
                modified_at,
                mappings,
            },
        );
    }

    let mappings = guard.get(&cache_key)?;
    for (prefix, category) in &mappings.mappings {
        if title.starts_with(prefix) {
            return Some(category.clone());
        }
    }
    None
}

fn load_template_category_mappings(db_path: &std::path::Path) -> Option<Vec<(String, String)>> {
    let connection = rusqlite::Connection::open(db_path).ok()?;
    let exists: i64 = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'template_category_mappings')",
            [],
            |row| row.get(0),
        )
        .ok()?;
    if exists != 1 {
        return None;
    }

    let mut statement = connection
        .prepare(
            "SELECT prefix, category
             FROM template_category_mappings
             ORDER BY length(prefix) DESC, prefix ASC",
        )
        .ok()?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .ok()?;

    let mut out = Vec::new();
    for (prefix, category) in rows.flatten() {
        out.push((prefix, category));
    }
    Some(out)
}

pub fn normalize_separators(path: &str) -> String {
    path.replace('\\', "/")
}

fn display_path(path: &Path) -> String {
    normalize_separators(&path.to_string_lossy())
}

#[cfg(test)]
mod tests;
