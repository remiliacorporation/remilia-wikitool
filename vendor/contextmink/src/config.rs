use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use globset::{Glob, GlobSet, GlobSetBuilder};

const CONFIG_NAME: &str = ".contextmink.toml";
const BUILTIN_EXCLUDES: &[&str] = &[
    ".git/**",
    "**/.git/**",
    "target/**",
    "**/target/**",
    "node_modules/**",
    "**/node_modules/**",
    ".venv/**",
    "**/.venv/**",
];

#[derive(Debug, Default)]
struct ContextminkConfig {
    profile: Option<String>,
    exclude_globs: Option<Vec<String>>,
    destructive_guard_recursive_delete_fragments: Option<Vec<String>>,
    destructive_guard_delete_fragments: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct DestructiveGuardConfig {
    pub(crate) recursive_delete_fragments: Vec<String>,
    pub(crate) delete_fragments: Vec<String>,
}

#[allow(dead_code)]
pub(crate) struct ContextConfig {
    pub(crate) profile: Option<String>,
    pub(crate) excludes: GlobSet,
    /// Canonical, `/`-normalized directory of the loaded config file.
    /// Exclude globs are matched against paths relative to this root, so
    /// policy holds even when scan roots are absolute or `..`-relative.
    pub(crate) policy_root: Option<String>,
    pub(crate) destructive_guard: DestructiveGuardConfig,
}

pub(crate) fn load_context_config(
    config_path: Option<&Path>,
    no_config: bool,
) -> Result<ContextConfig> {
    let mut raw = ContextminkConfig::default();
    let mut policy_root = None;
    if !no_config {
        let discovered_config = find_config_path();
        let selected_config = config_path.map(Path::to_path_buf).or(discovered_config);
        if let Some(path) = selected_config.as_deref() {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read config {}", path.display()))?;
            raw = parse_config(&text)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            policy_root = path.parent().and_then(canonical_normalized);
        }
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in BUILTIN_EXCLUDES {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid builtin glob {pattern}"))?);
    }
    if let Some(excludes) = &raw.exclude_globs {
        for pattern in excludes {
            builder.add(
                Glob::new(pattern).with_context(|| format!("invalid exclude glob {pattern}"))?,
            );
        }
    }
    Ok(ContextConfig {
        profile: raw.profile,
        excludes: builder
            .build()
            .context("failed to build exclude glob set")?,
        policy_root,
        destructive_guard: DestructiveGuardConfig {
            recursive_delete_fragments: raw
                .destructive_guard_recursive_delete_fragments
                .unwrap_or_default(),
            delete_fragments: raw.destructive_guard_delete_fragments.unwrap_or_default(),
        },
    })
}

/// Canonicalize a path and render it `/`-normalized without the Windows
/// verbatim (`\\?\`) prefix. Returns `None` when the path cannot be
/// canonicalized (nonexistent roots fall back to verbatim path matching).
pub(crate) fn canonical_normalized(path: &Path) -> Option<String> {
    match fs::canonicalize(path) {
        Ok(canonical) => {
            let text = canonical.to_string_lossy().replace('\\', "/");
            Some(
                text.strip_prefix("//?/")
                    .unwrap_or(&text)
                    .trim_end_matches('/')
                    .to_owned(),
            )
        }
        Err(_) => None,
    }
}

/// Bespoke parser for the small `.contextmink.toml` surface: `profile`,
/// `exclude_globs`, and optional destructive-guard fragment lists. Anything
/// else is a hard error so config typos fail fast instead of silently
/// changing scan or spawn scope.
fn parse_config(text: &str) -> Result<ContextminkConfig> {
    let mut config = ContextminkConfig::default();
    let mut lines = text.lines().enumerate();
    while let Some((index, line)) = lines.next() {
        let line_no = index + 1;
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("line {line_no}: expected `key = value`, found {line:?}"))?;
        let key = key.trim();
        let value = value.trim();
        match key {
            "profile" => {
                if config.profile.is_some() {
                    return Err(anyhow!("line {line_no}: duplicate key `profile`"));
                }
                config.profile =
                    Some(parse_string(value).ok_or_else(|| {
                        anyhow!("line {line_no}: profile must be a quoted string")
                    })?);
            }
            "exclude_globs" => {
                if config.exclude_globs.is_some() {
                    return Err(anyhow!("line {line_no}: duplicate key `exclude_globs`"));
                }
                config.exclude_globs = Some(parse_config_array(
                    "exclude_globs",
                    line_no,
                    value,
                    &mut lines,
                )?);
            }
            "destructive_guard_recursive_delete_fragments" => {
                if config
                    .destructive_guard_recursive_delete_fragments
                    .is_some()
                {
                    return Err(anyhow!(
                        "line {line_no}: duplicate key `destructive_guard_recursive_delete_fragments`"
                    ));
                }
                config.destructive_guard_recursive_delete_fragments = Some(parse_config_array(
                    "destructive_guard_recursive_delete_fragments",
                    line_no,
                    value,
                    &mut lines,
                )?);
            }
            "destructive_guard_delete_fragments" => {
                if config.destructive_guard_delete_fragments.is_some() {
                    return Err(anyhow!(
                        "line {line_no}: duplicate key `destructive_guard_delete_fragments`"
                    ));
                }
                config.destructive_guard_delete_fragments = Some(parse_config_array(
                    "destructive_guard_delete_fragments",
                    line_no,
                    value,
                    &mut lines,
                )?);
            }
            other => {
                return Err(anyhow!(
                    "line {line_no}: unknown key `{other}`; contextmink config accepts `profile`, `exclude_globs`, `destructive_guard_recursive_delete_fragments`, and `destructive_guard_delete_fragments`"
                ));
            }
        }
    }
    Ok(config)
}

fn parse_config_array<'a, I>(
    key: &str,
    line_no: usize,
    value: &str,
    lines: &mut I,
) -> Result<Vec<String>>
where
    I: Iterator<Item = (usize, &'a str)>,
{
    let mut body = value.to_owned();
    if !body.starts_with('[') {
        return Err(anyhow!("line {line_no}: {key} must be an array"));
    }
    while !array_is_closed(&body) {
        let Some((next_index, next_line)) = lines.next() else {
            return Err(anyhow!(
                "line {line_no}: {key} array is never closed with `]`"
            ));
        };
        let next_line = strip_comment(next_line).trim();
        if next_line.contains('=') && !next_line.starts_with(['"', '\'', ']', ',']) {
            return Err(anyhow!(
                "line {}: {key} array is never closed with `]`",
                next_index + 1
            ));
        }
        body.push(' ');
        body.push_str(next_line);
    }
    parse_string_array(&body).map_err(|error| anyhow!("{key} starting at line {line_no}: {error}"))
}

/// A `]` outside quoted strings closes an `exclude_globs` array body.
fn array_is_closed(body: &str) -> bool {
    let mut in_string = false;
    let mut quote = '"';
    for ch in body.chars() {
        match ch {
            '"' | '\'' if !in_string => {
                in_string = true;
                quote = ch;
            }
            ch if in_string && ch == quote => in_string = false,
            ']' if !in_string => return true,
            _ => {}
        }
    }
    false
}

/// Strip a `#` comment that is not inside a quoted string.
fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut quote = '"';
    for (offset, ch) in line.char_indices() {
        match ch {
            '"' | '\'' if !in_string => {
                in_string = true;
                quote = ch;
            }
            ch if in_string && ch == quote => in_string = false,
            '#' if !in_string => return &line[..offset],
            _ => {}
        }
    }
    line
}

fn parse_string(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })?;
    if inner.contains(['"', '\'']) {
        return None;
    }
    Some(inner.replace("\\\\", "\\"))
}

fn parse_string_array(body: &str) -> Result<Vec<String>> {
    let body = body.trim();
    let inner = body
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| anyhow!("array must be wrapped in [ and ]"))?;
    let mut values = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        values.push(
            parse_string(item)
                .ok_or_else(|| anyhow!("array entries must be quoted strings, found {item:?}"))?,
        );
    }
    Ok(values)
}

pub(crate) fn find_config_path() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        let candidate = current.join(CONFIG_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
#[path = "config/tests.rs"]
mod tests;
