use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

const CONTENT_EXTENSIONS: [&str; 1] = [".wiki"];
const TEMPLATE_EXTENSIONS: [&str; 5] = [".wiki", ".wikitext", ".lua", ".css", ".js"];

pub const COMMAND_SURFACE: &[&str] = &[
    "init",
    "pull",
    "push",
    "diff",
    "status",
    "context",
    "search",
    "search-external",
    "db stats",
    "db sync",
    "db migrate",
    "docs import",
    "docs import-technical",
    "docs list",
    "docs update",
    "docs remove",
    "docs search",
    "lsp:generate-config",
    "lsp:status",
    "lsp:info",
    "validate",
    "lint",
    "seo inspect",
    "net inspect",
    "perf lighthouse",
    "import cargo",
    "delete",
    "index rebuild",
    "index stats",
    "index chunks",
    "index backlinks",
    "index orphans",
    "index prune-categories",
    "workflow bootstrap",
    "workflow full-refresh",
    "workflow authoring-pack",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotFile {
    pub relative_path: String,
    pub title: String,
    pub namespace: String,
    pub is_redirect: bool,
    pub redirect_target: Option<String>,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub runtime: String,
    pub fixture_root: String,
    pub content_file_count: usize,
    pub template_file_count: usize,
    pub files: Vec<SnapshotFile>,
}

pub fn command_surface() -> Vec<String> {
    COMMAND_SURFACE
        .iter()
        .map(|item| (*item).to_string())
        .collect()
}

pub fn generate_fixture_snapshot(
    project_root: &Path,
    content_dir: &str,
    templates_dir: &str,
) -> Result<RuntimeSnapshot> {
    let project_root = project_root
        .canonicalize()
        .with_context(|| format!("failed to resolve project root {}", project_root.display()))?;

    let content_root = project_root.join(content_dir);
    let templates_root = project_root.join(templates_dir);

    let mut files = Vec::new();

    let mut content_files = collect_files(&project_root, &content_root, &CONTENT_EXTENSIONS)?;
    let mut template_files = collect_files(&project_root, &templates_root, &TEMPLATE_EXTENSIONS)?;

    content_files.sort();
    template_files.sort();

    for relative_path in &content_files {
        files.push(build_snapshot_file(
            &project_root,
            relative_path,
            filepath_to_title_content(relative_path, content_dir),
        )?);
    }

    for relative_path in &template_files {
        files.push(build_snapshot_file(
            &project_root,
            relative_path,
            filepath_to_title_template(relative_path, templates_dir),
        )?);
    }

    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    Ok(RuntimeSnapshot {
        runtime: "rust".to_string(),
        fixture_root: normalize_path(project_root),
        content_file_count: content_files.len(),
        template_file_count: template_files.len(),
        files,
    })
}

fn collect_files(
    project_root: &Path,
    base: &Path,
    allowed_extensions: &[&str],
) -> Result<Vec<String>> {
    if !base.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for entry in WalkDir::new(base) {
        let entry = entry.with_context(|| format!("failed to walk {}", base.display()))?;
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        if !has_allowed_extension(path, allowed_extensions) {
            continue;
        }
        let relative = path.strip_prefix(project_root).with_context(|| {
            format!(
                "failed to strip project root {} from {}",
                project_root.display(),
                path.display()
            )
        })?;
        out.push(normalize_path(relative));
    }

    Ok(out)
}

fn build_snapshot_file(
    project_root: &Path,
    relative_path: &str,
    title: String,
) -> Result<SnapshotFile> {
    let full_path = project_root.join(relative_path_to_os_path(relative_path));
    let content = fs::read_to_string(&full_path)
        .with_context(|| format!("failed to read fixture file {}", full_path.display()))?;
    let (is_redirect, redirect_target) = parse_redirect(&content);
    Ok(SnapshotFile {
        relative_path: normalize_separators(relative_path),
        namespace: namespace_from_title(&title).to_string(),
        title,
        is_redirect,
        redirect_target,
        content_hash: compute_hash(&content),
    })
}

fn relative_path_to_os_path(path: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.replace('/', "\\"))
    } else {
        PathBuf::from(path)
    }
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

fn compute_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut output = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn namespace_from_title(title: &str) -> &'static str {
    if title.starts_with("Category:") {
        "Category"
    } else if title.starts_with("Template:") {
        "Template"
    } else if title.starts_with("Module:") {
        "Module"
    } else if title.starts_with("MediaWiki:") {
        "MediaWiki"
    } else if title.starts_with("File:") {
        "File"
    } else if title.starts_with("User:") {
        "User"
    } else if title.starts_with("Goldenlight:") {
        "Goldenlight"
    } else {
        "Main"
    }
}

fn filepath_to_title_content(relative_path: &str, content_dir: &str) -> String {
    let normalized_path = normalize_separators(relative_path);
    let content_prefix = normalize_separators(content_dir);
    let without_base = normalized_path
        .strip_prefix(&(content_prefix + "/"))
        .unwrap_or(&normalized_path);

    let mut segments: Vec<&str> = without_base
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty() {
        return decode_segment(strip_known_extensions(without_base));
    }

    let namespace_folder = segments.remove(0);
    segments.retain(|segment| *segment != "_redirects");
    let filename = segments.last().copied().unwrap_or(without_base);
    let title_name = decode_segment(strip_known_extensions(filename));

    let prefix = match namespace_folder {
        "Category" => "Category:",
        "File" => "File:",
        "User" => "User:",
        "Goldenlight" => "Goldenlight:",
        _ => "",
    };

    format!("{prefix}{title_name}")
}

fn filepath_to_title_template(relative_path: &str, templates_dir: &str) -> String {
    let normalized_path = normalize_separators(relative_path);
    let templates_prefix = normalize_separators(templates_dir);
    let without_base = normalized_path
        .strip_prefix(&(templates_prefix + "/"))
        .unwrap_or(&normalized_path);

    let raw_segments: Vec<&str> = without_base
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let mut segments: Vec<&str> = raw_segments
        .into_iter()
        .filter(|segment| *segment != "_redirects")
        .collect();

    if segments.is_empty() {
        return decode_segment(strip_known_extensions(without_base));
    }

    let category = segments.remove(0);
    let rest = segments;

    if category == "mediawiki" {
        if rest.is_empty() {
            return format!(
                "MediaWiki:{}",
                decode_segment(strip_known_extensions(
                    Path::new(&normalized_path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(without_base)
                ))
            );
        }

        let mut subpages = Vec::new();
        for (index, segment) in rest.iter().enumerate() {
            let value = if index == rest.len() - 1 {
                strip_subpage_extension(segment)
            } else {
                segment
            };
            subpages.push(decode_segment(value));
        }
        return format!("MediaWiki:{}", subpages.join("/"));
    }

    if let Some(base_index) = rest
        .iter()
        .position(|segment| segment.starts_with("Template_") || segment.starts_with("Module_"))
    {
        let base_segment = rest[base_index];
        let base_ext = extension_of(base_segment);
        let base_clean = strip_base_extension(base_segment);
        let is_module = base_clean.starts_with("Module_");
        let is_template = base_clean.starts_with("Template_");
        if is_module || is_template {
            let prefix_len = if is_module { 7 } else { 9 };
            let mut base_name_raw = base_clean[prefix_len..].to_string();
            let mut subpage_segments: Vec<&str> = rest[base_index + 1..].to_vec();

            if is_module
                && subpage_segments.is_empty()
                && base_name_raw.ends_with("_styles")
                && base_ext == "css"
            {
                base_name_raw.truncate(base_name_raw.len().saturating_sub(7));
                subpage_segments = vec!["styles.css"];
            }

            let namespace = if is_module { "Module" } else { "Template" };
            let base_title = decode_segment(&base_name_raw);
            if subpage_segments.is_empty() {
                return format!("{namespace}:{base_title}");
            }

            let mut subpages = Vec::new();
            for (index, segment) in subpage_segments.iter().enumerate() {
                let value = if index == subpage_segments.len() - 1 {
                    strip_subpage_extension(segment)
                } else {
                    segment
                };
                subpages.push(decode_segment(value));
            }

            return format!("{namespace}:{base_title}/{}", subpages.join("/"));
        }
    }

    let filename = Path::new(&normalized_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(without_base);
    let name_without_ext = strip_known_extensions(filename);

    if let Some(module_name) = name_without_ext.strip_prefix("Module_") {
        if module_name.ends_with("_styles") && extension_of(filename) == "css" {
            let base = decode_segment(&module_name[..module_name.len().saturating_sub(7)]);
            return format!("Module:{base}/styles.css");
        }
        return format!("Module:{}", decode_segment(module_name));
    }

    if let Some(template_name) = name_without_ext.strip_prefix("Template_") {
        return format!("Template:{}", decode_segment(template_name));
    }

    decode_segment(name_without_ext)
}

fn extension_of(path: &str) -> &str {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
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
    for extension in extensions {
        if let Some(stripped) = value.strip_suffix(extension) {
            return stripped;
        }
    }
    value
}

fn decode_segment(value: &str) -> String {
    value
        .replace("___", "/")
        .replace("--", ":")
        .replace('_', " ")
}

fn has_allowed_extension(path: &Path, allowed_extensions: &[&str]) -> bool {
    let ext = path
        .extension()
        .and_then(|item| item.to_str())
        .map(|item| format!(".{}", item.to_ascii_lowercase()));
    match ext {
        Some(ext) => allowed_extensions.iter().any(|allowed| *allowed == ext),
        None => false,
    }
}

fn normalize_separators(path: &str) -> String {
    path.replace('\\', "/")
}

fn normalize_path(path: impl AsRef<Path>) -> String {
    let joined = path
        .as_ref()
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/");
    normalize_separators(&joined)
}
