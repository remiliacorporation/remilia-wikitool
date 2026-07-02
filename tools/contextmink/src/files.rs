use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::config::{ContextConfig, canonical_normalized};

#[derive(Debug)]
pub(crate) struct CollectedFiles {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) total_seen: usize,
    pub(crate) truncated: bool,
    /// Git-ignored nested repository roots that were entered anyway, as
    /// display paths. Empty when no supplement ran.
    pub(crate) nested_repos_entered: Vec<String>,
}

pub(crate) struct CollectOptions<'a> {
    pub(crate) globs: &'a [String],
    pub(crate) extensions: &'a [String],
    pub(crate) with_excluded: bool,
    pub(crate) with_git_ignored: bool,
    pub(crate) skip_nested_repos: bool,
    pub(crate) max_scan_files: usize,
}

/// How many directory levels below a pruned (git-ignored) directory the
/// nested-repo supplement probes for a `.git` marker. Repos nested deeper
/// under an ignored plain directory must be passed as explicit roots.
const NESTED_REPO_PROBE_DEPTH: usize = 1;

/// Bound on nested-repo recursion (a repo inside a repo inside a repo...).
const NESTED_REPO_MAX_RECURSION: usize = 4;

pub(crate) fn collect_files(
    paths: &[PathBuf],
    config: &ContextConfig,
    options: &CollectOptions<'_>,
) -> Result<CollectedFiles> {
    let include_matcher = build_optional_globset(options.globs)?;
    let extension_matcher = normalize_extensions(options.extensions);
    let explicit_excluded_roots = explicit_excluded_roots(paths, config, options.with_excluded);
    let mut state = CollectState {
        files: Vec::new(),
        seen: HashSet::new(),
        total_seen: 0,
        truncated: false,
        nested_repos_entered: Vec::new(),
    };
    for root in paths {
        if root.is_file() {
            let mapper = PolicyMapper::for_root(root, config);
            if file_is_included(
                root,
                &mapper,
                &include_matcher,
                &extension_matcher,
                config,
                options.with_excluded,
                &explicit_excluded_roots,
            ) {
                state.push_file(root.to_path_buf(), options.max_scan_files);
            }
            if state.truncated {
                break;
            }
            continue;
        }
        walk_root(
            root,
            config,
            options,
            &include_matcher,
            &extension_matcher,
            &explicit_excluded_roots,
            &mut state,
            0,
        )?;
        if state.truncated {
            break;
        }
    }
    state.files.sort();
    state.files.dedup();
    state.nested_repos_entered.sort();
    state.nested_repos_entered.dedup();
    Ok(CollectedFiles {
        files: state.files,
        total_seen: state.total_seen,
        truncated: state.truncated,
        nested_repos_entered: state.nested_repos_entered,
    })
}

/// Maps walker paths (as spelled from a scan root) onto policy paths
/// relative to the config root, so exclude globs anchored at the repository
/// root hold no matter how the root argument was spelled (absolute paths,
/// `..`-relative paths, or scans launched from a subdirectory).
#[derive(Clone)]
struct PolicyMapper {
    given_root: String,
    /// Config-root-relative prefix for this scan root; `None` when no config
    /// root applies (no config, or the root lives outside the config tree),
    /// which falls back to matching the path exactly as spelled.
    policy_prefix: Option<String>,
}

impl PolicyMapper {
    fn for_root(root: &Path, config: &ContextConfig) -> Self {
        let given_root = trim_normalized_path(&normalize_path(root));
        let policy_prefix = match (&config.policy_root, canonical_normalized(root)) {
            (Some(policy_root), Some(canonical_root)) => {
                if canonical_root == *policy_root {
                    Some(String::new())
                } else {
                    canonical_root
                        .strip_prefix(&format!("{policy_root}/"))
                        .map(str::to_owned)
                }
            }
            _ => None,
        };
        Self {
            given_root,
            policy_prefix,
        }
    }

    /// Policy path used for exclude matching. `normalized` must come from
    /// `normalize_path` on a path yielded under this mapper's root.
    fn policy_path(&self, normalized: &str) -> String {
        let trimmed = trim_normalized_path(normalized);
        let Some(prefix) = &self.policy_prefix else {
            return trimmed;
        };
        let relative = if self.given_root.is_empty() {
            trimmed.as_str()
        } else {
            trimmed
                .strip_prefix(&self.given_root)
                .map(|rest| rest.trim_start_matches('/'))
                .unwrap_or(trimmed.as_str())
        };
        if prefix.is_empty() {
            relative.to_owned()
        } else if relative.is_empty() {
            prefix.clone()
        } else {
            format!("{prefix}/{relative}")
        }
    }
}

struct CollectState {
    files: Vec<PathBuf>,
    seen: HashSet<PathBuf>,
    total_seen: usize,
    truncated: bool,
    nested_repos_entered: Vec<String>,
}

impl CollectState {
    fn push_file(&mut self, candidate: PathBuf, max_scan_files: usize) {
        if !self.seen.insert(candidate.clone()) {
            return;
        }
        self.total_seen += 1;
        if self.files.len() < max_scan_files {
            self.files.push(candidate);
        } else {
            self.truncated = true;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_root(
    root: &Path,
    config: &ContextConfig,
    options: &CollectOptions<'_>,
    include_matcher: &Option<GlobSet>,
    extension_matcher: &[String],
    explicit_excluded_roots: &[String],
    state: &mut CollectState,
    nesting: usize,
) -> Result<()> {
    let mapper = PolicyMapper::for_root(root, config);
    let mut walk = WalkBuilder::new(root);
    walk.hidden(false)
        .ignore(!options.with_git_ignored)
        .git_ignore(!options.with_git_ignored)
        .git_exclude(!options.with_git_ignored)
        .parents(!options.with_git_ignored);
    if !options.with_excluded {
        let excludes = config.excludes.clone();
        let explicit_roots = explicit_excluded_roots.to_vec();
        let filter_mapper = mapper.clone();
        walk.filter_entry(move |entry| {
            let policy = filter_mapper.policy_path(&normalize_path(entry.path()));
            if is_under_explicit_excluded_root(&policy, &explicit_roots) {
                return true;
            }
            if entry.file_type().is_some_and(|kind| kind.is_dir()) {
                if policy.is_empty() || policy == "." {
                    return true;
                }
                let probe = format!("{policy}/__contextmink_probe__");
                !excludes.is_match(&policy) && !excludes.is_match(&probe)
            } else {
                !excludes.is_match(&policy)
            }
        });
    }
    let mut visited_dirs: HashSet<PathBuf> = HashSet::new();
    for entry in walk.build() {
        let entry = entry?;
        match entry.file_type() {
            Some(kind) if kind.is_dir() => {
                visited_dirs.insert(entry.path().to_path_buf());
            }
            Some(kind) if kind.is_file() => {
                if !file_is_included(
                    entry.path(),
                    &mapper,
                    include_matcher,
                    extension_matcher,
                    config,
                    options.with_excluded,
                    explicit_excluded_roots,
                ) {
                    continue;
                }
                state.push_file(entry.path().to_path_buf(), options.max_scan_files);
                if state.truncated {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    // Nested-repo supplement: a directory pruned by Git ignore rules that is
    // itself a git repository root is a workspace member (multi-repo
    // workspaces routinely git-ignore sibling repos for repo separation), not
    // a generated artifact. Enter it with its own ignore rules and disclose
    // the entry in the receipt.
    if options.with_git_ignored
        || options.skip_nested_repos
        || state.truncated
        || nesting >= NESTED_REPO_MAX_RECURSION
    {
        return Ok(());
    }
    let mut nested_roots = Vec::new();
    for dir in &visited_dirs {
        collect_pruned_repo_roots(
            dir,
            &mapper,
            &visited_dirs,
            config,
            options.with_excluded,
            explicit_excluded_roots,
            &mut nested_roots,
        );
    }
    nested_roots.sort();
    for nested_root in nested_roots {
        state.nested_repos_entered.push(display_path(&nested_root));
        walk_root(
            &nested_root,
            config,
            options,
            include_matcher,
            extension_matcher,
            explicit_excluded_roots,
            state,
            nesting + 1,
        )?;
        if state.truncated {
            return Ok(());
        }
    }
    Ok(())
}

/// Find git-repo roots among the unvisited (walker-pruned) children of a
/// visited directory. Pruned plain directories are probed one level deeper
/// so a repo directly under an ignored grouping directory is still found.
#[allow(clippy::too_many_arguments)]
fn collect_pruned_repo_roots(
    dir: &Path,
    mapper: &PolicyMapper,
    visited_dirs: &HashSet<PathBuf>,
    config: &ContextConfig,
    with_excluded: bool,
    explicit_excluded_roots: &[String],
    output: &mut Vec<PathBuf>,
) {
    probe_children_for_repos(
        dir,
        mapper,
        visited_dirs,
        config,
        with_excluded,
        explicit_excluded_roots,
        NESTED_REPO_PROBE_DEPTH,
        output,
    );
}

#[allow(clippy::too_many_arguments)]
fn probe_children_for_repos(
    dir: &Path,
    mapper: &PolicyMapper,
    visited_dirs: &HashSet<PathBuf>,
    config: &ContextConfig,
    with_excluded: bool,
    explicit_excluded_roots: &[String],
    remaining_probe_depth: usize,
    output: &mut Vec<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        if visited_dirs.contains(&path) {
            continue;
        }
        if path.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
        if !with_excluded {
            let policy = mapper.policy_path(&normalize_path(&path));
            if !is_under_explicit_excluded_root(&policy, explicit_excluded_roots) {
                let probe = format!("{policy}/__contextmink_probe__");
                if config.excludes.is_match(&policy) || config.excludes.is_match(&probe) {
                    continue;
                }
            }
        }
        if path.join(".git").exists() {
            output.push(path);
        } else if remaining_probe_depth > 0 {
            probe_children_for_repos(
                &path,
                mapper,
                visited_dirs,
                config,
                with_excluded,
                explicit_excluded_roots,
                remaining_probe_depth - 1,
                output,
            );
        }
    }
}

fn explicit_excluded_roots(
    paths: &[PathBuf],
    config: &ContextConfig,
    with_excluded: bool,
) -> Vec<String> {
    if with_excluded {
        return Vec::new();
    }
    paths
        .iter()
        .filter_map(|path| {
            let mapper = PolicyMapper::for_root(path, config);
            let policy = mapper.policy_path(&normalize_path(path));
            if policy.is_empty() || policy == "." {
                return None;
            }
            let probe = format!("{policy}/__contextmink_probe__");
            if config.excludes.is_match(&policy) || config.excludes.is_match(&probe) {
                Some(policy)
            } else {
                None
            }
        })
        .collect()
}

fn file_is_included(
    path: &Path,
    mapper: &PolicyMapper,
    include_matcher: &Option<GlobSet>,
    extension_matcher: &[String],
    config: &ContextConfig,
    with_excluded: bool,
    explicit_excluded_roots: &[String],
) -> bool {
    let normalized = normalize_path(path);
    if !with_excluded {
        let policy = mapper.policy_path(&normalized);
        if config.excludes.is_match(&policy)
            && !is_under_explicit_excluded_root(&policy, explicit_excluded_roots)
        {
            return false;
        }
    }
    if let Some(include_matcher) = include_matcher {
        let basename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !include_matcher.is_match(&normalized) && !include_matcher.is_match(basename) {
            return false;
        }
    }
    if !extension_matcher.is_empty() {
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            return false;
        };
        if !extension_matcher
            .iter()
            .any(|expected| extension.eq_ignore_ascii_case(expected))
        {
            return false;
        }
    }
    true
}

fn is_under_explicit_excluded_root(path: &str, explicit_excluded_roots: &[String]) -> bool {
    let normalized = trim_normalized_path(path);
    explicit_excluded_roots
        .iter()
        .any(|root| normalized == *root || normalized.starts_with(&format!("{root}/")))
}

fn trim_normalized_path(path: &str) -> String {
    path.trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

fn build_optional_globset(globs: &[String]) -> Result<Option<GlobSet>> {
    if globs.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in globs {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid include glob {pattern}"))?);
    }
    Ok(Some(
        builder
            .build()
            .context("failed to build include glob set")?,
    ))
}

fn normalize_extensions(extensions: &[String]) -> Vec<String> {
    extensions
        .iter()
        .filter_map(|extension| {
            let extension = extension.trim().trim_start_matches('.');
            if extension.is_empty() {
                None
            } else {
                Some(extension.to_ascii_lowercase())
            }
        })
        .collect()
}

fn normalize_path(path: &Path) -> String {
    let path = path.strip_prefix(".").unwrap_or(path);
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
