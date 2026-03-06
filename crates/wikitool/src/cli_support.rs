use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use wikitool_core::config::{WikiConfig, load_config};
use wikitool_core::filesystem::ScanStats;
use wikitool_core::index::StoredIndexStats;
use wikitool_core::runtime::{PathOverrides, ResolutionContext, ResolvedPaths, resolve_paths};
use wikitool_core::schema::{DatabaseSchemaState, schema_state};

use crate::RuntimeOptions;

pub(crate) fn resolve_default_true_flag(
    enabled: bool,
    disabled: bool,
    label: &str,
) -> Result<bool> {
    if enabled && disabled {
        bail!("invalid options for {label}: enable and disable flags both set");
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

pub(crate) fn detect_host_context_root(
    repo_root: &Path,
    explicit: Option<&Path>,
) -> Result<Option<PathBuf>> {
    let _ = repo_root;
    let Some(path) = explicit else {
        return Ok(None);
    };

    let root = fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize {}", normalize_path(path)))?;
    if !root.join("CLAUDE.md").is_file()
        || !root.join(".claude/rules").is_dir()
        || !root.join(".claude/skills").is_dir()
    {
        bail!(
            "invalid host project root {}: expected CLAUDE.md and .claude/{{rules,skills}}",
            normalize_path(&root)
        );
    }
    Ok(Some(root))
}

pub(crate) fn ensure_files_identical(left: &Path, right: &Path, label: &str) -> Result<()> {
    let left_bytes =
        fs::read(left).with_context(|| format!("failed to read {}", normalize_path(left)))?;
    let right_bytes =
        fs::read(right).with_context(|| format!("failed to read {}", normalize_path(right)))?;
    if left_bytes != right_bytes {
        bail!(
            "{label}: {} and {} must match",
            normalize_path(left),
            normalize_path(right)
        );
    }
    Ok(())
}

pub(crate) fn reset_directory(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove {}", normalize_path(path)))?;
    }
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", normalize_path(path)))
}

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

pub(crate) fn paths_equivalent(left: &Path, right: &Path) -> Result<bool> {
    let left = fs::canonicalize(left)
        .with_context(|| format!("failed to canonicalize {}", normalize_path(left)))?;
    let right = fs::canonicalize(right)
        .with_context(|| format!("failed to canonicalize {}", normalize_path(right)))?;
    Ok(left == right)
}

pub(crate) fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

#[cfg(unix)]
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
    dotenvy::dotenv().ok();

    let context = ResolutionContext::from_process()?;
    let overrides = PathOverrides {
        project_root: runtime.project_root.clone(),
        data_dir: runtime.data_dir.clone(),
        config: runtime.config.clone(),
    };

    let initial = resolve_paths(&context, &overrides)?;
    let project_env = initial.project_root.join(".env");
    if project_env.exists() {
        let _ = dotenvy::from_path_override(&project_env);
    }

    resolve_paths(&context, &overrides)
}

pub(crate) fn resolve_runtime_with_config(
    runtime: &RuntimeOptions,
) -> Result<(ResolvedPaths, WikiConfig)> {
    let paths = resolve_runtime_paths(runtime)?;
    let config = load_config(&paths.config_path)
        .with_context(|| format!("failed to load {}", normalize_path(&paths.config_path)))?;
    Ok((paths, config))
}

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

pub(crate) fn format_flag(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
