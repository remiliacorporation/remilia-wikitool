use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub const NO_MIGRATIONS_POLICY_MESSAGE: &str = "Database migrations are disabled in full-cutover mode. Delete .wikitool/data/wikitool.db and run `wikitool pull --full --all`.";

const EMBEDDED_PARSER_CONFIG: &str = include_str!("../../../config/remilia-parser.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    Flag,
    Env,
    Heuristic,
    Default,
}

impl ValueSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Env => "env",
            Self::Heuristic => "heuristic",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PathOverrides {
    pub project_root: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub config: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResolutionContext {
    pub cwd: PathBuf,
    pub executable_dir: Option<PathBuf>,
}

impl ResolutionContext {
    pub fn from_process() -> Result<Self> {
        let cwd = env::current_dir().context("failed to read current directory")?;
        let executable_dir = env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        Ok(Self {
            cwd,
            executable_dir,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedPaths {
    pub project_root: PathBuf,
    pub wiki_content_dir: PathBuf,
    pub templates_dir: PathBuf,
    pub state_dir: PathBuf,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub config_path: PathBuf,
    pub parser_config_path: PathBuf,
    pub root_source: ValueSource,
    pub data_source: ValueSource,
    pub config_source: ValueSource,
}

impl ResolvedPaths {
    pub fn diagnostics(&self) -> String {
        format!(
            "project_root={} ({})\nstate_dir={}\nwiki_content_dir={}\ntemplates_dir={}\ndata_dir={} ({})\nconfig_path={} ({})\nparser_config_path={}\npolicy={}",
            normalize_for_display(&self.project_root),
            self.root_source.as_str(),
            normalize_for_display(&self.state_dir),
            normalize_for_display(&self.wiki_content_dir),
            normalize_for_display(&self.templates_dir),
            normalize_for_display(&self.data_dir),
            self.data_source.as_str(),
            normalize_for_display(&self.config_path),
            self.config_source.as_str(),
            normalize_for_display(&self.parser_config_path),
            NO_MIGRATIONS_POLICY_MESSAGE
        )
    }
}

#[derive(Debug, Clone)]
pub struct InitOptions {
    pub include_templates: bool,
    pub materialize_config: bool,
    pub materialize_parser_config: bool,
    pub force: bool,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            include_templates: false,
            materialize_config: true,
            materialize_parser_config: true,
            force: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InitReport {
    pub created_dirs: Vec<PathBuf>,
    pub wrote_config: bool,
    pub wrote_parser_config: bool,
}

pub fn resolve_paths(
    context: &ResolutionContext,
    overrides: &PathOverrides,
) -> Result<ResolvedPaths> {
    resolve_paths_with_lookup(context, overrides, |key| env::var(key).ok())
}

fn resolve_paths_with_lookup<F>(
    context: &ResolutionContext,
    overrides: &PathOverrides,
    lookup_env: F,
) -> Result<ResolvedPaths>
where
    F: Fn(&str) -> Option<String>,
{
    let (project_root, root_source) = resolve_project_root(context, overrides, &lookup_env)
        .context("failed to resolve project root")?;
    reject_legacy_layout(&project_root)?;

    let state_dir = project_root.join(".wikitool");
    let wiki_content_dir = project_root.join("wiki_content");
    let templates_dir = project_root.join("templates");
    let parser_config_path = state_dir.join("remilia-parser.json");

    let (data_dir, data_source) = if let Some(path) = overrides.data_dir.as_deref() {
        (
            absolutize_from_project(path, &project_root),
            ValueSource::Flag,
        )
    } else if let Some(value) = lookup_env("WIKITOOL_DATA_DIR") {
        (
            absolutize_from_project(Path::new(value.trim()), &project_root),
            ValueSource::Env,
        )
    } else {
        (state_dir.join("data"), ValueSource::Default)
    };

    let (config_path, config_source) = if let Some(path) = overrides.config.as_deref() {
        (
            absolutize_from_project(path, &project_root),
            ValueSource::Flag,
        )
    } else if let Some(value) = lookup_env("WIKITOOL_CONFIG") {
        (
            absolutize_from_project(Path::new(value.trim()), &project_root),
            ValueSource::Env,
        )
    } else {
        (state_dir.join("config.toml"), ValueSource::Default)
    };

    Ok(ResolvedPaths {
        db_path: data_dir.join("wikitool.db"),
        project_root,
        wiki_content_dir,
        templates_dir,
        state_dir,
        data_dir,
        config_path,
        parser_config_path,
        root_source,
        data_source,
        config_source,
    })
}

pub fn init_layout(paths: &ResolvedPaths, options: &InitOptions) -> Result<InitReport> {
    let mut created_dirs = Vec::new();

    let mut required_dirs = vec![
        paths.wiki_content_dir.clone(),
        paths.state_dir.clone(),
        paths.data_dir.clone(),
        paths.state_dir.join("auth"),
        paths.state_dir.join("cache"),
        paths.state_dir.join("logs"),
        paths.state_dir.join("tmp"),
        paths.state_dir.join("exports"),
        paths.state_dir.join("backups"),
    ];
    if options.include_templates {
        required_dirs.push(paths.templates_dir.clone());
    }

    for dir in &required_dirs {
        if !dir.exists() {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
            created_dirs.push(dir.clone());
        }
    }

    let wrote_config = if options.materialize_config {
        write_text_file(
            &paths.config_path,
            &render_materialized_config(paths, options.include_templates),
            options.force,
        )?
    } else {
        false
    };

    let wrote_parser_config = if options.materialize_parser_config {
        materialize_parser_config(paths, options.force)?
    } else {
        false
    };

    Ok(InitReport {
        created_dirs,
        wrote_config,
        wrote_parser_config,
    })
}

pub fn materialize_parser_config(paths: &ResolvedPaths, force: bool) -> Result<bool> {
    write_text_file(&paths.parser_config_path, EMBEDDED_PARSER_CONFIG, force)
}

pub fn embedded_parser_config() -> &'static str {
    EMBEDDED_PARSER_CONFIG
}

pub fn render_materialized_config(paths: &ResolvedPaths, include_templates: bool) -> String {
    let project_root = normalize_for_display(&paths.project_root);
    let wiki_content_dir = normalize_for_display(&paths.wiki_content_dir);
    let templates_dir = normalize_for_display(&paths.templates_dir);
    let state_dir = normalize_for_display(&paths.state_dir);
    let data_dir = normalize_for_display(&paths.data_dir);
    let db_path = normalize_for_display(&paths.db_path);
    let parser_config_path = normalize_for_display(&paths.parser_config_path);

    format!(
        "# wikitool runtime configuration (materialized by `wikitool init`)\n# full-cutover mode: database migrations are intentionally disabled\n# policy: delete DB and repull after binary/schema changes\n\n[paths]\nproject_root = \"{project_root}\"\nwiki_content_dir = \"{wiki_content_dir}\"\ntemplates_dir = \"{templates_dir}\"\nstate_dir = \"{state_dir}\"\ndata_dir = \"{data_dir}\"\ndb_path = \"{db_path}\"\nparser_config_path = \"{parser_config_path}\"\n\n[features]\ntemplates_enabled = {include_templates}\n\n[database]\nmigrations = \"disabled\"\nreset_strategy = \"delete_db_and_repull\"\n",
    )
}

pub fn lsp_settings_json(paths: &ResolvedPaths) -> String {
    let parser_path = normalize_for_display(&paths.parser_config_path);
    format!(
        "{{\n  \"wikiparser.articlePath\": \"https://wiki.remilia.org/wiki/$1\",\n  \"wikiparser.config\": \"{parser_path}\",\n  \"wikiparser.linter.enable\": true,\n  \"wikiparser.linter.severity\": \"errors and warnings\",\n  \"wikiparser.inlay\": true,\n  \"wikiparser.completion\": true,\n  \"wikiparser.color\": true,\n  \"wikiparser.hover\": true,\n  \"wikiparser.signature\": true\n}}"
    )
}

fn resolve_project_root<F>(
    context: &ResolutionContext,
    overrides: &PathOverrides,
    lookup_env: &F,
) -> Result<(PathBuf, ValueSource)>
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(path) = overrides.project_root.as_deref() {
        return Ok((absolutize(path, &context.cwd), ValueSource::Flag));
    }

    if let Some(value) = lookup_env("WIKITOOL_PROJECT_ROOT") {
        return Ok((
            absolutize(Path::new(value.trim()), &context.cwd),
            ValueSource::Env,
        ));
    }

    let root = detect_project_root_heuristic(&context.cwd, context.executable_dir.as_deref());
    Ok((root, ValueSource::Heuristic))
}

fn detect_project_root_heuristic(cwd: &Path, executable_dir: Option<&Path>) -> PathBuf {
    let mut seen = HashSet::new();
    for candidate in candidate_roots(cwd, executable_dir) {
        let key = normalize_for_display(&candidate);
        if !seen.insert(key) {
            continue;
        }
        if candidate.join("wiki_content").exists() {
            return candidate;
        }
    }
    cwd.to_path_buf()
}

fn candidate_roots(cwd: &Path, executable_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut out = ancestors(cwd);
    if let Some(exe_dir) = executable_dir {
        out.extend(ancestors(exe_dir));
    }
    out
}

fn ancestors(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        out.push(current.to_path_buf());
        cursor = current.parent();
    }
    out
}

fn absolutize(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn absolutize_from_project(path: &Path, project_root: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

fn write_text_file(path: &Path, content: &str, force: bool) -> Result<bool> {
    if path.exists() && !force {
        return Ok(false);
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

fn reject_legacy_layout(project_root: &Path) -> Result<()> {
    let legacy_wikitool = project_root.join("custom").join("wikitool");
    let legacy_templates = project_root.join("custom").join("templates");

    let mut found = Vec::new();
    if legacy_wikitool.exists() {
        found.push(legacy_wikitool);
    }
    if legacy_templates.exists() {
        found.push(legacy_templates);
    }

    if found.is_empty() {
        return Ok(());
    }

    let found_lines = found
        .iter()
        .map(|path| format!("  - {}", normalize_for_display(path)))
        .collect::<Vec<_>>()
        .join("\n");

    bail!(
        "Legacy runtime layout detected under custom/*:\n{found_lines}\nFull cutover mode only supports project-root runtime layout (wiki_content/, templates/, .wikitool/).\nMigration tooling is intentionally disabled.\nRecommended cutover:\n  1) Initialize a clean project root: wikitool init --project-root <new-root> --templates\n  2) Pull fresh state from live wiki: wikitool pull --full --all"
    );
}

fn normalize_for_display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use tempfile::tempdir;

    use super::{
        InitOptions, PathOverrides, ResolutionContext, ValueSource, init_layout,
        resolve_paths_with_lookup,
    };

    #[test]
    fn resolve_paths_prefers_flag_over_env() {
        let temp = tempdir().expect("tempdir");
        let cwd = temp.path().join("cwd");
        let from_flag = temp.path().join("flag-root");
        fs::create_dir_all(&cwd).expect("create cwd");

        let overrides = PathOverrides {
            project_root: Some(from_flag.clone()),
            ..PathOverrides::default()
        };
        let context = ResolutionContext {
            cwd: cwd.clone(),
            executable_dir: None,
        };

        let env = HashMap::from([(
            "WIKITOOL_PROJECT_ROOT".to_string(),
            temp.path().join("env-root").to_string_lossy().to_string(),
        )]);

        let resolved = resolve_paths_with_lookup(&context, &overrides, |key| env.get(key).cloned())
            .expect("resolve paths");
        assert_eq!(resolved.project_root, from_flag);
        assert_eq!(resolved.root_source, ValueSource::Flag);
    }

    #[test]
    fn resolve_paths_rejects_legacy_layout() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("legacy-root");
        fs::create_dir_all(root.join("custom").join("wikitool")).expect("legacy path");

        let context = ResolutionContext {
            cwd: root.clone(),
            executable_dir: None,
        };
        let overrides = PathOverrides {
            project_root: Some(root.clone()),
            ..PathOverrides::default()
        };
        let err = resolve_paths_with_lookup(&context, &overrides, |_| None).expect_err("must fail");
        let message = err.to_string();
        assert!(message.contains("Legacy runtime layout detected"));
        assert!(message.contains("Migration tooling is intentionally disabled"));
    }

    #[test]
    fn init_layout_creates_expected_dirs_and_files() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create root");

        let context = ResolutionContext {
            cwd: root.clone(),
            executable_dir: None,
        };
        let overrides = PathOverrides {
            project_root: Some(root.clone()),
            ..PathOverrides::default()
        };
        let paths = resolve_paths_with_lookup(&context, &overrides, |_| None).expect("resolve");

        let report = init_layout(
            &paths,
            &InitOptions {
                include_templates: true,
                ..InitOptions::default()
            },
        )
        .expect("init");

        assert!(!report.created_dirs.is_empty());
        assert!(paths.wiki_content_dir.exists());
        assert!(paths.templates_dir.exists());
        assert!(paths.state_dir.exists());
        assert!(paths.data_dir.exists());
        assert!(paths.config_path.exists());
        assert!(paths.parser_config_path.exists());
    }
}
