use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::phase1::ResolvedPaths;

const TEMPLATE_CATEGORY_MAPPINGS: [(&str, &str); 49] = [
    ("Template:Cite", "cite"),
    ("Module:Citation", "cite"),
    ("Template:Ref", "reference"),
    ("Template:Efn", "reference"),
    ("Module:Reference", "reference"),
    ("Template:Infobox", "infobox"),
    ("Module:Infobox", "infobox"),
    ("Module:InfoboxImage", "infobox"),
    ("Template:About", "hatnote"),
    ("Template:See also", "hatnote"),
    ("Template:Main", "hatnote"),
    ("Template:Further", "hatnote"),
    ("Template:Hatnote", "hatnote"),
    ("Template:Redirect", "hatnote"),
    ("Template:Distinguish", "hatnote"),
    ("Module:Hatnote", "hatnote"),
    ("Template:Navbox", "navbox"),
    ("Template:Navbar", "navbox"),
    ("Template:Flatlist", "navbox"),
    ("Template:Hlist", "navbox"),
    ("Module:Navbox", "navbox"),
    ("Module:Navbar", "navbox"),
    ("Template:Blockquote", "quotation"),
    ("Template:Cquote", "quotation"),
    ("Template:Quote", "quotation"),
    ("Template:Poem", "quotation"),
    ("Template:Verse", "quotation"),
    ("Module:Quotation", "quotation"),
    ("Template:Ambox", "message"),
    ("Template:Article quality", "message"),
    ("Template:Stub", "message"),
    ("Template:Update", "message"),
    ("Template:Citation needed", "message"),
    ("Template:Cn", "message"),
    ("Template:Clarify", "message"),
    ("Template:When", "message"),
    ("Template:As of", "message"),
    ("Module:Message", "message"),
    ("Template:Sidebar", "sidebar"),
    ("Template:Portal", "sidebar"),
    ("Template:Remilia events", "sidebar"),
    ("Module:Sidebar", "sidebar"),
    ("Template:Repost", "repost"),
    ("Template:Mirror", "repost"),
    ("Template:Goldenlight repost", "repost"),
    ("Module:Repost", "repost"),
    ("Template:Etherscan", "blockchain"),
    ("Template:Explorer", "blockchain"),
    ("Template:OpenSea", "blockchain"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Main,
    Category,
    File,
    User,
    Goldenlight,
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
            Self::Goldenlight => "Goldenlight",
            Self::Template => "Template",
            Self::Module => "Module",
            Self::MediaWiki => "MediaWiki",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub include_content: bool,
    pub include_templates: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            include_content: true,
            include_templates: true,
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
    let mut files = Vec::new();
    if options.include_content && paths.wiki_content_dir.exists() {
        scan_content_files(paths, &mut files)?;
    }
    if options.include_templates && paths.templates_dir.exists() {
        scan_template_files(paths, &mut files)?;
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

pub fn title_to_relative_path(paths: &ResolvedPaths, title: &str, is_redirect: bool) -> String {
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
            let category = template_category(title);
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
        let category = template_category(title);
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

pub fn relative_path_to_title(paths: &ResolvedPaths, relative_path: &str) -> String {
    let normalized = normalize_separators(relative_path);
    let content_rel = rel_from_root(paths, &paths.wiki_content_dir);
    let templates_rel = rel_from_root(paths, &paths.templates_dir);

    if let Some(rest) = normalized.strip_prefix(&format!("{content_rel}/")) {
        return content_path_to_title(rest);
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
    let normalized = normalize_pathbuf(&absolute);
    let allowed = [
        normalize_pathbuf(&paths.wiki_content_dir),
        normalize_pathbuf(&paths.templates_dir),
        normalize_pathbuf(&paths.state_dir),
    ];

    if allowed.iter().any(|prefix| normalized.starts_with(prefix)) {
        return Ok(());
    }

    bail!(
        "path escapes scoped runtime directories: {}\nallowed roots:\n  - {}\n  - {}\n  - {}",
        display_path(&normalized),
        display_path(&allowed[0]),
        display_path(&allowed[1]),
        display_path(&allowed[2])
    )
}

fn scan_content_files(paths: &ResolvedPaths, out: &mut Vec<ScannedFile>) -> Result<()> {
    let content_rel = rel_from_root(paths, &paths.wiki_content_dir);
    for folder in ["Main", "Category", "File", "User", "Goldenlight"] {
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
            out.push(read_scanned_file(paths, path, &relative)?);
        }
    }
    Ok(())
}

fn scan_template_files(paths: &ResolvedPaths, out: &mut Vec<ScannedFile>) -> Result<()> {
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
        out.push(read_scanned_file(paths, path, &relative)?);
    }
    Ok(())
}

fn read_scanned_file(paths: &ResolvedPaths, path: &Path, relative: &str) -> Result<ScannedFile> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    let (is_redirect, redirect_target) = parse_redirect(&content);
    let title = relative_path_to_title(paths, relative);
    let namespace = namespace_from_title(&title).as_str().to_string();

    Ok(ScannedFile {
        relative_path: normalize_separators(relative),
        title,
        namespace,
        is_redirect,
        redirect_target,
        content_hash: compute_hash(&content),
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

fn content_path_to_title(content_rel_path: &str) -> String {
    let normalized = normalize_separators(content_rel_path);
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
    let prefix = match folder {
        "Category" => "Category:",
        "File" => "File:",
        "User" => "User:",
        "Goldenlight" => "Goldenlight:",
        _ => "",
    };
    format!("{prefix}{name}")
}

fn template_path_to_title(templates_rel_path: &str) -> String {
    let normalized = normalize_separators(templates_rel_path);
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

fn compute_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut output = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn parse_redirect(content: &str) -> (bool, Option<String>) {
    let trimmed = content.trim();
    if !trimmed.to_ascii_uppercase().starts_with("#REDIRECT") {
        return (false, None);
    }
    if let Some(start) = trimmed.find("[[")
        && let Some(end) = trimmed[start + 2..].find("]]")
    {
        let target = trimmed[start + 2..start + 2 + end].trim().to_string();
        if !target.is_empty() {
            return (true, Some(target));
        }
    }
    (true, None)
}

fn namespace_from_title(title: &str) -> Namespace {
    if title.starts_with("Category:") {
        Namespace::Category
    } else if title.starts_with("File:") {
        Namespace::File
    } else if title.starts_with("User:") {
        Namespace::User
    } else if title.starts_with("Goldenlight:") {
        Namespace::Goldenlight
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

fn namespace_folder(namespace: Namespace) -> &'static str {
    match namespace {
        Namespace::Main => "Main",
        Namespace::Category => "Category",
        Namespace::File => "File",
        Namespace::User => "User",
        Namespace::Goldenlight => "Goldenlight",
        Namespace::Template | Namespace::Module | Namespace::MediaWiki => "Main",
    }
}

fn title_without_namespace(title: &str) -> &str {
    for prefix in [
        "Category:",
        "File:",
        "User:",
        "Goldenlight:",
        "Template:",
        "Module:",
        "MediaWiki:",
    ] {
        if let Some(value) = title.strip_prefix(prefix) {
            return value;
        }
    }
    title
}

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
    strip_one_of(value, &[".wiki", ".wikitext", ".lua", ".css", ".js"])
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

fn template_category(title: &str) -> &'static str {
    if title.starts_with("MediaWiki:") {
        return "mediawiki";
    }

    for (prefix, category) in TEMPLATE_CATEGORY_MAPPINGS {
        if title.starts_with(prefix) {
            return category;
        }
    }

    if title.starts_with("Template:Translation") || title.starts_with("Module:Translation") {
        return "translations";
    }
    if title.starts_with("Template:Birth date")
        || title.starts_with("Template:Start date")
        || title.starts_with("Template:End date")
        || title.starts_with("Module:Age")
    {
        return "date";
    }
    if title.starts_with("Template:Remilia navigation") {
        return "navigation";
    }

    "misc"
}

fn normalize_separators(path: &str) -> String {
    path.replace('\\', "/")
}

fn display_path(path: &Path) -> String {
    normalize_separators(&path.to_string_lossy())
}

fn normalize_pathbuf(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{
        Namespace, ScanOptions, content_path_to_title, relative_path_to_title, scan_stats,
        template_path_to_title, title_to_relative_path, validate_scoped_path,
    };
    use crate::phase1::{ResolvedPaths, ValueSource};

    fn paths(root: &str) -> ResolvedPaths {
        let project_root = PathBuf::from(root);
        ResolvedPaths {
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool").join("data"),
            db_path: project_root
                .join(".wikitool")
                .join("data")
                .join("wikitool.db"),
            config_path: project_root.join(".wikitool").join("config.toml"),
            parser_config_path: project_root.join(".wikitool").join("remilia-parser.json"),
            project_root,
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn mapping_roundtrip_content_and_templates() {
        let paths = paths("/workspace/project");

        let cases = [
            ("Alpha", false, "wiki_content/Main/Alpha.wiki"),
            ("Category:Test", false, "wiki_content/Category/Test.wiki"),
            (
                "Template:Infobox person",
                false,
                "templates/infobox/Template_Infobox_person.wiki",
            ),
            (
                "Module:Navbar/styles.css",
                false,
                "templates/navbox/Module_Navbar_styles.css",
            ),
            (
                "MediaWiki:Common.css",
                false,
                "templates/mediawiki/Common.css",
            ),
            (
                "Template:Infobox person",
                true,
                "templates/infobox/_redirects/Template_Infobox_person.wiki",
            ),
        ];

        for (title, redirect, expected) in cases {
            let relative = title_to_relative_path(&paths, title, redirect);
            assert_eq!(relative, expected);
            let parsed = relative_path_to_title(&paths, &relative);
            if title == "MediaWiki:Common.css" {
                assert_eq!(parsed, "MediaWiki:Common.css");
            } else {
                assert_eq!(parsed, title);
            }
        }
    }

    #[test]
    fn windows_separator_content_parse() {
        let title = content_path_to_title("Category\\_redirects\\Category_Test.wiki");
        assert_eq!(title, "Category:Category Test");
    }

    #[test]
    fn windows_separator_template_parse() {
        let title = template_path_to_title("navbox\\Module_Navbar\\configuration.lua");
        assert_eq!(title, "Module:Navbar/configuration");
    }

    #[test]
    fn scoped_path_validation_blocks_escaping_path() {
        let paths = paths("/workspace/project");
        let unsafe_path = PathBuf::from("/workspace/secrets/token.txt");
        let error = validate_scoped_path(&paths, &unsafe_path).expect_err("must fail");
        assert!(
            error
                .to_string()
                .contains("path escapes scoped runtime directories")
        );
    }

    #[test]
    fn scan_stats_on_fixture_corpus() {
        let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("fixtures")
            .join("full-refresh");
        let paths = ResolvedPaths {
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("custom").join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool").join("data"),
            db_path: project_root
                .join(".wikitool")
                .join("data")
                .join("wikitool.db"),
            config_path: project_root.join(".wikitool").join("config.toml"),
            parser_config_path: project_root.join(".wikitool").join("remilia-parser.json"),
            project_root,
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        };

        let stats = scan_stats(&paths, &ScanOptions::default()).expect("stats");
        assert_eq!(stats.total_files, 6);
        assert_eq!(stats.content_files, 2);
        assert_eq!(stats.template_files, 4);
        assert_eq!(
            stats.by_namespace,
            BTreeMap::from([
                (Namespace::Category.as_str().to_string(), 1),
                (Namespace::Main.as_str().to_string(), 1),
                (Namespace::Module.as_str().to_string(), 2),
                (Namespace::Template.as_str().to_string(), 2),
            ])
        );
    }
}
