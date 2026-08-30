use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use clap::ValueEnum;
use wikitool_core::catalog::content_index::StoredIndexStats;
use wikitool_core::config::{WikiConfig, load_config, wiki_target_warnings_for_config};
use wikitool_core::filesystem::ScanStats;
use wikitool_core::runtime::{PathOverrides, ResolutionContext, ResolvedPaths, resolve_paths};
use wikitool_core::schema::{DatabaseSchemaState, schema_state};
use wikitool_core::site::resolve_docs_profile_with_config;
use wikitool_core::source::{ExportFormat, ExternalFetchFormat};

use crate::RuntimeOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum OutputFormat {
    Text,
    Json,
}

impl OutputFormat {
    pub(crate) fn is_json(self) -> bool {
        self == Self::Json
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum FetchContentFormat {
    Wikitext,
    Html,
    RenderedHtml,
}

impl FetchContentFormat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Wikitext => "wikitext",
            Self::Html => "html",
            Self::RenderedHtml => "rendered-html",
        }
    }
}

impl From<FetchContentFormat> for ExternalFetchFormat {
    fn from(value: FetchContentFormat) -> Self {
        match value {
            FetchContentFormat::Wikitext => Self::Wikitext,
            FetchContentFormat::Html | FetchContentFormat::RenderedHtml => Self::Html,
        }
    }
}

impl std::fmt::Display for FetchContentFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ExportContentFormat {
    Markdown,
    Wikitext,
}

impl ExportContentFormat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Wikitext => "wikitext",
        }
    }
}

impl From<ExportContentFormat> for ExportFormat {
    fn from(value: ExportContentFormat) -> Self {
        match value {
            ExportContentFormat::Markdown => Self::Markdown,
            ExportContentFormat::Wikitext => Self::Wikitext,
        }
    }
}

impl std::fmt::Display for ExportContentFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(feature = "maintainer")]
use std::fs;
#[cfg(feature = "maintainer")]
use std::path::PathBuf;

#[cfg(feature = "maintainer")]
use anyhow::bail;

#[cfg(feature = "maintainer")]
pub(crate) fn resolve_default_true_flag(
    enabled: bool,
    disabled: bool,
    label: &str,
) -> Result<bool> {
    if enabled && disabled {
        anyhow::bail!("invalid options for {label}: enable and disable flags both set");
    }
    if disabled {
        return Ok(false);
    }
    Ok(true)
}

pub(crate) fn prompt_yes_no(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush().context("failed to flush stdout")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read confirmation input")?;
    let normalized = input.trim().to_ascii_lowercase();
    Ok(matches!(normalized.as_str(), "y" | "yes"))
}

#[cfg(feature = "maintainer")]
pub(crate) fn resolve_git_hooks_dir(repo_root: &Path) -> Result<Option<PathBuf>> {
    let git_path = repo_root.join(".git");
    if git_path.is_dir() {
        let hooks_dir = git_path.join("hooks");
        if hooks_dir.is_dir() {
            return Ok(Some(hooks_dir));
        }
        return Ok(None);
    }

    if !git_path.is_file() {
        return Ok(None);
    }

    let pointer = fs::read_to_string(&git_path)
        .with_context(|| format!("failed to read {}", normalize_path(&git_path)))?;
    let git_dir = parse_gitdir_pointer(repo_root, &pointer).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported .git pointer format in {}",
            normalize_path(&git_path)
        )
    })?;
    let hooks_dir = git_dir.join("hooks");
    if hooks_dir.is_dir() {
        return Ok(Some(hooks_dir));
    }
    Ok(None)
}

#[cfg(feature = "maintainer")]
pub(crate) fn reset_directory(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove {}", normalize_path(path)))?;
    }
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", normalize_path(path)))
}

#[cfg(feature = "maintainer")]
pub(crate) fn validate_release_output(repo_root: &Path, output: &Path, label: &str) -> Result<()> {
    let repo_root = fs::canonicalize(repo_root).with_context(|| {
        format!(
            "failed to resolve repository root {}",
            normalize_path(repo_root)
        )
    })?;
    let output = resolve_path_through_existing_ancestor(output)?;
    if output.parent().is_none() || repo_root.starts_with(&output) {
        bail!(
            "{label} must not replace the repository, an ancestor, or a filesystem root: {}",
            normalize_path(&output)
        );
    }
    if output.starts_with(&repo_root) && !output.starts_with(repo_root.join("dist")) {
        bail!(
            "{label} inside the repository must remain under dist/: {}",
            normalize_path(&output)
        );
    }
    Ok(())
}

#[cfg(feature = "maintainer")]
fn resolve_path_through_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)
        .with_context(|| format!("failed to resolve output path {}", normalize_path(path)))?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    bail!(
                        "output path escapes its filesystem root: {}",
                        normalize_path(path)
                    );
                }
            }
            std::path::Component::CurDir => {}
            _ => normalized.push(component.as_os_str()),
        }
    }

    let mut cursor = normalized;
    let mut missing = Vec::new();
    while !cursor.exists() {
        let name = cursor.file_name().with_context(|| {
            format!(
                "output path has no existing ancestor: {}",
                normalize_path(path)
            )
        })?;
        missing.push(name.to_os_string());
        cursor = cursor
            .parent()
            .context("output path has no parent directory")?
            .to_path_buf();
    }
    let mut resolved = fs::canonicalize(&cursor).with_context(|| {
        format!(
            "failed to resolve output ancestor {}",
            normalize_path(&cursor)
        )
    })?;
    for name in missing.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

#[cfg(feature = "maintainer")]
pub(crate) fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_file() {
        bail!("file not found: {}", normalize_path(source));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", normalize_path(parent)))?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "failed to copy {} -> {}",
            normalize_path(source),
            normalize_path(destination)
        )
    })?;
    Ok(())
}

#[cfg(feature = "maintainer")]
pub(crate) fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("directory not found: {}", normalize_path(source));
    }
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", normalize_path(destination)))?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read {}", normalize_path(source)))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to read metadata {}", normalize_path(&source_path)))?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            copy_file(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

#[cfg(feature = "maintainer")]
pub(crate) fn copy_dir_contents(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("directory not found: {}", normalize_path(source));
    }
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", normalize_path(destination)))?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read {}", normalize_path(source)))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to read metadata {}", normalize_path(&source_path)))?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            copy_file(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

#[cfg(feature = "maintainer")]
fn parse_gitdir_pointer(repo_root: &Path, raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    let remainder = trimmed.strip_prefix("gitdir:")?.trim();
    let candidate = PathBuf::from(remainder);
    if candidate.is_absolute() {
        return Some(candidate);
    }
    Some(repo_root.join(candidate))
}

#[cfg(unix)]
#[cfg(feature = "maintainer")]
pub(crate) fn set_executable_if_unix(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata {}", normalize_path(path)))?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set permissions {}", normalize_path(path)))?;
    Ok(())
}

#[cfg(not(unix))]
#[cfg(feature = "maintainer")]
pub(crate) fn set_executable_if_unix(_path: &Path) -> Result<()> {
    Ok(())
}

pub(crate) fn normalize_option(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn normalize_title_query(value: &str) -> String {
    value.replace('_', " ").trim().to_string()
}

pub(crate) fn collapse_whitespace(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
                previous_was_space = true;
            }
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }
    output.trim().to_string()
}

pub(crate) fn resolve_runtime_paths(runtime: &RuntimeOptions) -> Result<ResolvedPaths> {
    let context = ResolutionContext::from_process()?;
    let overrides = PathOverrides {
        project_root: runtime.project_root.clone(),
        data_dir: runtime.data_dir.clone(),
        config: runtime.config.clone(),
    };

    let initial = resolve_paths(&context, &overrides)?;
    let project_env = initial.project_root.join(".env");
    if project_env.exists() {
        // The selected project owns its environment overlay. Searching upward
        // from the process CWD can import a different repository's wiki target
        // even when --project-root was explicit. Existing process variables
        // remain the explicit, temporary override described by `config show`.
        let _ = dotenvy::from_path(&project_env);
    }

    let mut anchored_overrides = overrides;
    anchored_overrides.project_root = Some(initial.project_root.clone());
    let mut resolved = resolve_paths(&context, &anchored_overrides)?;
    resolved.root_source = initial.root_source;
    Ok(resolved)
}

pub(crate) fn resolve_runtime_with_config(
    runtime: &RuntimeOptions,
) -> Result<(ResolvedPaths, WikiConfig)> {
    let paths = resolve_runtime_paths(runtime)?;
    let config = load_config(&paths.config_path)
        .with_context(|| format!("failed to load {}", normalize_path(&paths.config_path)))?;
    warn_wiki_target_overrides(&config);
    Ok((paths, config))
}

pub(crate) fn resolve_runtime_with_docs_profile(
    runtime: &RuntimeOptions,
    requested: Option<&str>,
) -> Result<(ResolvedPaths, WikiConfig, String)> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let docs_profile = resolve_docs_profile_with_config(&paths, &config, requested)?;
    Ok((paths, config, docs_profile))
}

fn warn_wiki_target_overrides(config: &WikiConfig) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if std::env::var("WIKITOOL_SILENT")
        .ok()
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
        || WARNED.swap(true, Ordering::Relaxed)
    {
        return;
    }
    for warning in wiki_target_warnings_for_config(config) {
        eprintln!("wikitool: {warning}");
    }
}

#[cfg(feature = "maintainer")]
pub(crate) fn resolve_repo_root(value: Option<PathBuf>) -> Result<PathBuf> {
    let repo_root = match value {
        Some(path) => path,
        None => std::env::current_dir().context("failed to resolve current directory")?,
    };
    if !repo_root.exists() {
        bail!("path does not exist: {}", normalize_path(&repo_root));
    }
    fs::canonicalize(&repo_root)
        .with_context(|| format!("failed to canonicalize {}", normalize_path(&repo_root)))
}

pub(crate) fn print_scan_stats(prefix: &str, stats: &ScanStats) {
    println!("{prefix}.total_files: {}", stats.total_files);
    println!("{prefix}.content_files: {}", stats.content_files);
    println!("{prefix}.template_files: {}", stats.template_files);
    println!("{prefix}.redirects: {}", stats.redirects);
    if stats.by_namespace.is_empty() {
        println!("{prefix}.by_namespace: <empty>");
    } else {
        for (namespace, count) in &stats.by_namespace {
            println!("{prefix}.namespace.{namespace}: {count}");
        }
    }
}

pub(crate) fn print_database_schema_status(paths: &ResolvedPaths) {
    match schema_state(paths) {
        Ok(DatabaseSchemaState::Missing) => {
            println!("database.schema: absent");
        }
        Ok(DatabaseSchemaState::Ready) => {
            println!("database.schema: ready");
        }
        Ok(DatabaseSchemaState::Incompatible { reason }) => {
            println!("database.schema: incompatible");
            println!("database.schema_error: {reason}");
        }
        Err(error) => {
            println!("database.schema: unknown");
            println!("database.schema_error: {error}");
        }
    }
}

pub(crate) fn print_stored_index_stats(prefix: &str, stats: &StoredIndexStats) {
    println!("{prefix}.indexed_rows: {}", stats.indexed_rows);
    println!("{prefix}.redirects: {}", stats.redirects);
    if stats.by_namespace.is_empty() {
        println!("{prefix}.by_namespace: <empty>");
    } else {
        for (namespace, count) in &stats.by_namespace {
            println!("{prefix}.namespace.{namespace}: {count}");
        }
    }
}

pub(crate) fn print_string_list(prefix: &str, values: &[String]) {
    println!("{prefix}.count: {}", values.len());
    if values.is_empty() {
        println!("{prefix}: <none>");
        return;
    }
    for value in values {
        println!("{prefix}.item: {value}");
    }
}

pub(crate) fn normalize_path(path: impl AsRef<Path>) -> String {
    let mut value = path.as_ref().to_string_lossy().replace('\\', "/");
    if let Some(stripped) = value.strip_prefix("//?/") {
        value = stripped.to_string();
    }
    value
}

pub(crate) fn path_is_under_directory(candidate: &Path, directory: &Path) -> bool {
    let candidate = normalize_path(candidate);
    let directory = normalize_path(directory);
    candidate == directory || candidate.starts_with(&format!("{directory}/"))
}

pub(crate) fn format_flag(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(all(test, feature = "maintainer"))]
mod maintainer_tests {
    use super::*;

    #[test]
    fn release_outputs_cannot_replace_source_or_ancestor_directories() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let repo = sandbox.path().join("checkout");
        fs::create_dir_all(repo.join("dist")).expect("repository");

        assert!(validate_release_output(&repo, &repo, "output").is_err());
        assert!(validate_release_output(&repo, sandbox.path(), "output").is_err());
        assert!(validate_release_output(&repo, &repo.join("agent-pack"), "output").is_err());
        assert!(validate_release_output(&repo, &repo.join("dist/agent"), "output").is_ok());
        assert!(validate_release_output(&repo, &sandbox.path().join("external"), "output").is_ok());
    }
}
