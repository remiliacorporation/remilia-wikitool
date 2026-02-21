use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::filesystem::{
    content_path_to_title, namespace_from_title, normalize_separators, template_path_to_title,
};

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
    "fetch",
    "export",
    "db stats",
    "db sync",
    "db migrate",
    "docs import",
    "docs import-technical",
    "docs list",
    "docs update",
    "docs remove",
    "docs search",
    "docs generate-reference",
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
        namespace: namespace_from_title(&title).as_str().to_string(),
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

/// Strip the content_dir prefix and delegate to filesystem::content_path_to_title.
fn filepath_to_title_content(relative_path: &str, content_dir: &str) -> String {
    let normalized_path = normalize_separators(relative_path);
    let content_prefix = normalize_separators(content_dir);
    let without_base = normalized_path
        .strip_prefix(&(content_prefix.clone() + "/"))
        .unwrap_or(&normalized_path);
    content_path_to_title(without_base)
}

/// Strip the templates_dir prefix and delegate to filesystem::template_path_to_title.
fn filepath_to_title_template(relative_path: &str, templates_dir: &str) -> String {
    let normalized_path = normalize_separators(relative_path);
    let templates_prefix = normalize_separators(templates_dir);
    let without_base = normalized_path
        .strip_prefix(&(templates_prefix.clone() + "/"))
        .unwrap_or(&normalized_path);
    template_path_to_title(without_base)
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

fn normalize_path(path: impl AsRef<Path>) -> String {
    let joined = path
        .as_ref()
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/");
    normalize_separators(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::Namespace;

    #[test]
    fn namespace_from_title_correctness() {
        assert_eq!(namespace_from_title("Alpha"), Namespace::Main);
        assert_eq!(namespace_from_title("Category:Test"), Namespace::Category);
        assert_eq!(
            namespace_from_title("Template:Infobox person"),
            Namespace::Template
        );
        assert_eq!(namespace_from_title("Module:Navbar"), Namespace::Module);
        assert_eq!(
            namespace_from_title("MediaWiki:Common.css"),
            Namespace::MediaWiki
        );
        assert_eq!(namespace_from_title("File:Logo.png"), Namespace::File);
        assert_eq!(namespace_from_title("User:Admin"), Namespace::User);
        // Custom namespaces (like Goldenlight) map to Main in the enum;
        // they are handled via config, not hardcoded variants.
        assert_eq!(namespace_from_title("Goldenlight:Test"), Namespace::Main);
    }

    #[test]
    fn filepath_to_title_content_basic() {
        assert_eq!(
            filepath_to_title_content("wiki_content/Main/Alpha.wiki", "wiki_content"),
            "Alpha"
        );
        assert_eq!(
            filepath_to_title_content("wiki_content/Category/Test.wiki", "wiki_content"),
            "Category:Test"
        );
        assert_eq!(
            filepath_to_title_content("wiki_content/Main/_redirects/Old_Name.wiki", "wiki_content"),
            "Old Name"
        );
    }

    #[test]
    fn filepath_to_title_template_basic() {
        assert_eq!(
            filepath_to_title_template(
                "templates/infobox/Template_Infobox_person.wiki",
                "templates"
            ),
            "Template:Infobox person"
        );
        assert_eq!(
            filepath_to_title_template("templates/navbox/Module_Navbar.lua", "templates"),
            "Module:Navbar"
        );
        assert_eq!(
            filepath_to_title_template("templates/mediawiki/Common.css", "templates"),
            "MediaWiki:Common.css"
        );
    }

    #[test]
    fn filepath_roundtrip_consistency() {
        // Content paths should produce titles that re-derive the same namespace
        let content_cases = [
            ("wiki_content/Main/Alpha.wiki", "Alpha", "Main"),
            (
                "wiki_content/Category/Test.wiki",
                "Category:Test",
                "Category",
            ),
        ];

        for (path, expected_title, expected_ns) in content_cases {
            let title = filepath_to_title_content(path, "wiki_content");
            assert_eq!(title, expected_title);
            assert_eq!(namespace_from_title(&title).as_str(), expected_ns);
        }

        // Custom namespaces work for filepath → title parsing via folder name
        let title = filepath_to_title_content("wiki_content/Goldenlight/Page.wiki", "wiki_content");
        assert_eq!(title, "Goldenlight:Page");
        // But namespace_from_title returns Main (custom ns not in enum)
        assert_eq!(namespace_from_title(&title), Namespace::Main);
    }

    #[test]
    fn command_surface_includes_required_entries() {
        let surface = command_surface();
        for required in ["fetch", "export", "docs generate-reference"] {
            assert!(
                surface.contains(&required.to_string()),
                "COMMAND_SURFACE missing: {required}"
            );
        }
    }

    #[test]
    fn command_surface_excludes_internal_commands() {
        let surface = command_surface();
        for internal in ["release", "dev", "contracts"] {
            assert!(
                !surface.iter().any(|cmd| cmd == internal),
                "COMMAND_SURFACE should not contain internal command: {internal}"
            );
        }
    }
}
