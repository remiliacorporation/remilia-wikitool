use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use full_moon::parse;
use selene_lib::lints::Severity as SeleneSeverity;
use selene_lib::{Checker, CheckerConfig, standard_library::StandardLibrary};
use serde::Serialize;

use crate::filesystem::{ScanOptions, scan_files, title_to_relative_path};
use crate::runtime::ResolvedPaths;

const EMBEDDED_SELENE_BACKEND: &str = "embedded:selene-lib";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LuaLintSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct LuaLintIssue {
    pub line: usize,
    pub column: usize,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub code: String,
    pub message: String,
    pub severity: LuaLintSeverity,
}

#[derive(Debug, Clone, Serialize)]
pub struct LuaLintResult {
    pub title: String,
    pub errors: Vec<LuaLintIssue>,
    pub warnings: Vec<LuaLintIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LuaLintReport {
    pub selene_available: bool,
    pub selene_path: Option<String>,
    pub config_path: Option<String>,
    pub inspected_modules: usize,
    pub total_errors: usize,
    pub total_warnings: usize,
    pub results: Vec<LuaLintResult>,
}

enum LuaLintBackend {
    External(PathBuf),
    Embedded(Box<EmbeddedSelene>),
}

struct EmbeddedSelene {
    checker: Checker<serde_json::Value>,
}

#[derive(Debug)]
struct ByteLineIndex {
    line_starts: Vec<usize>,
}

impl ByteLineIndex {
    fn new(content: &str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in content.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self { line_starts }
    }

    fn locate(&self, content: &str, byte: u32) -> (usize, usize) {
        let byte = clamp_to_char_boundary(content, usize::try_from(byte).unwrap_or(content.len()));
        let line_index = match self.line_starts.binary_search(&byte) {
            Ok(index) => index,
            Err(0) => 0,
            Err(index) => index.saturating_sub(1),
        };
        let line_start = self.line_starts[line_index];
        let column = content[line_start..byte].chars().count() + 1;
        (line_index + 1, column)
    }
}

pub fn lint_modules(paths: &ResolvedPaths, title: Option<&str>) -> Result<LuaLintReport> {
    let paths = paths.clone();
    let title = title.map(ToString::to_string);
    std::thread::Builder::new()
        .name("wikitool-lua-lint".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || lint_modules_with_large_stack(&paths, title.as_deref()))
        .context("failed to spawn Lua lint worker")?
        .join()
        .map_err(|_| anyhow::anyhow!("Lua lint worker panicked"))?
}

fn lint_modules_with_large_stack(
    paths: &ResolvedPaths,
    title: Option<&str>,
) -> Result<LuaLintReport> {
    let config_path = resolve_selene_config_path(&paths.project_root);
    let backend = if let Some(selene_path) = resolve_selene_override_path() {
        LuaLintBackend::External(selene_path)
    } else {
        LuaLintBackend::Embedded(Box::new(load_embedded_selene(
            &paths.project_root,
            config_path.as_deref(),
        )?))
    };

    let mut results = Vec::new();
    let mut inspected_modules = 0usize;

    if let Some(title) = title {
        let normalized = normalize_module_title(title);
        let relative_path = title_to_relative_path(paths, &normalized, false)?;
        let absolute_path = absolute_from_relative(paths, &relative_path);
        if !absolute_path.exists() {
            bail!("module not found: {normalized}");
        }
        if !is_lua_module_path(&absolute_path) {
            bail!("module is not a Lua source file: {normalized}");
        }
        let content = fs::read_to_string(&absolute_path)
            .with_context(|| format!("failed to read {}", absolute_path.display()))?;
        let result = lint_lua_content(
            &backend,
            config_path.as_deref(),
            &content,
            &normalized,
            &paths.project_root,
        )?;
        inspected_modules = 1;
        results.push(result);
    } else {
        let scanned = scan_files(
            paths,
            &ScanOptions {
                include_content: false,
                include_templates: true,
                ..ScanOptions::default()
            },
        )?;

        for file in scanned
            .into_iter()
            .filter(|file| file.namespace == "Module" && !file.is_redirect)
        {
            let absolute_path = absolute_from_relative(paths, &file.relative_path);
            if !is_lua_module_path(&absolute_path) {
                continue;
            }
            let content = fs::read_to_string(&absolute_path)
                .with_context(|| format!("failed to read {}", absolute_path.display()))?;
            let result = lint_lua_content(
                &backend,
                config_path.as_deref(),
                &content,
                &file.title,
                &paths.project_root,
            )?;
            inspected_modules += 1;
            if !result.errors.is_empty() || !result.warnings.is_empty() {
                results.push(result);
            }
        }
    }

    let total_errors = results.iter().map(|item| item.errors.len()).sum::<usize>();
    let total_warnings = results
        .iter()
        .map(|item| item.warnings.len())
        .sum::<usize>();
    Ok(LuaLintReport {
        selene_available: true,
        selene_path: Some(match &backend {
            LuaLintBackend::External(path) => normalize_path(path),
            LuaLintBackend::Embedded(_) => EMBEDDED_SELENE_BACKEND.to_string(),
        }),
        config_path: config_path.map(|path| normalize_path(&path)),
        inspected_modules,
        total_errors,
        total_warnings,
        results,
    })
}

fn lint_lua_content(
    backend: &LuaLintBackend,
    config_path: Option<&Path>,
    content: &str,
    title: &str,
    project_root: &Path,
) -> Result<LuaLintResult> {
    match backend {
        LuaLintBackend::External(selene_path) => {
            lint_lua_content_external(selene_path, config_path, content, title, project_root)
        }
        LuaLintBackend::Embedded(embedded) => lint_lua_content_embedded(embedded, content, title),
    }
}

fn lint_lua_content_external(
    selene_path: &Path,
    config_path: Option<&Path>,
    content: &str,
    title: &str,
    project_root: &Path,
) -> Result<LuaLintResult> {
    let scratch_dir = env::temp_dir().join("wikitool-lint");
    fs::create_dir_all(&scratch_dir)
        .with_context(|| format!("failed to create {}", scratch_dir.display()))?;
    let temp_file = scratch_dir.join(format!("{}.lua", sanitize_title(title)));
    fs::write(&temp_file, content)
        .with_context(|| format!("failed to write {}", temp_file.display()))?;

    let mut command = Command::new(selene_path);
    command.arg(&temp_file);
    command.arg("--display-style");
    command.arg("json");
    if let Some(config_path) = config_path {
        command.arg("--config");
        command.arg(config_path);
    }
    command.current_dir(project_root);

    let output = command
        .output()
        .with_context(|| format!("failed to execute {}", selene_path.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let parsed = parse_selene_output(
        if stdout.trim().is_empty() {
            &stderr
        } else {
            &stdout
        },
        title,
    )?;

    let _ = fs::remove_file(&temp_file);
    Ok(parsed)
}

fn lint_lua_content_embedded(
    embedded: &EmbeddedSelene,
    content: &str,
    title: &str,
) -> Result<LuaLintResult> {
    let ast = match parse(content) {
        Ok(ast) => ast,
        Err(error) => {
            return Ok(LuaLintResult {
                title: title.to_string(),
                errors: vec![LuaLintIssue {
                    line: 1,
                    column: 1,
                    end_line: None,
                    end_column: None,
                    code: "parse_error".to_string(),
                    message: format!("failed to parse Lua module: {error:?}"),
                    severity: LuaLintSeverity::Error,
                }],
                warnings: Vec::new(),
            });
        }
    };

    let line_index = ByteLineIndex::new(content);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for diagnostic in embedded.checker.test_on(&ast) {
        let issue = issue_from_embedded_diagnostic(content, &line_index, diagnostic);
        if issue.severity == LuaLintSeverity::Error {
            errors.push(issue);
        } else {
            warnings.push(issue);
        }
    }
    errors.sort_by(issue_sort_key);
    warnings.sort_by(issue_sort_key);

    Ok(LuaLintResult {
        title: title.to_string(),
        errors,
        warnings,
    })
}

fn load_embedded_selene(project_root: &Path, config_path: Option<&Path>) -> Result<EmbeddedSelene> {
    let config = load_selene_checker_config(config_path)?;
    let standard_library = load_selene_standard_library(project_root, config_path, config.std())?;
    let checker = Checker::<serde_json::Value>::new(config, standard_library)
        .map_err(|error| anyhow::anyhow!("failed to initialize Selene checker: {error}"))?;
    Ok(EmbeddedSelene { checker })
}

fn load_selene_checker_config(
    config_path: Option<&Path>,
) -> Result<CheckerConfig<serde_json::Value>> {
    let Some(config_path) = config_path else {
        return Ok(CheckerConfig::default());
    };
    let content = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    toml::from_str::<CheckerConfig<serde_json::Value>>(&content)
        .with_context(|| format!("failed to parse {}", config_path.display()))
}

fn load_selene_standard_library(
    project_root: &Path,
    config_path: Option<&Path>,
    std_spec: &str,
) -> Result<StandardLibrary> {
    let config_dir = config_path
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project_root.join("config"));
    let mut loaded = None::<StandardLibrary>;
    for component in std_spec
        .split('+')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let standard_library = load_selene_standard_library_component(&config_dir, component)?;
        match &mut loaded {
            Some(existing) => existing.extend(standard_library),
            None => loaded = Some(standard_library),
        }
    }
    loaded.ok_or_else(|| anyhow::anyhow!("failed to resolve standard library spec: {std_spec}"))
}

fn load_selene_standard_library_component(
    config_dir: &Path,
    component: &str,
) -> Result<StandardLibrary> {
    if let Some(standard_library) = StandardLibrary::from_name(component) {
        return Ok(standard_library);
    }

    let standard_library_path = resolve_selene_standard_library_path(config_dir, component)
        .ok_or_else(|| anyhow::anyhow!("unknown Selene standard library: {component}"))?;
    let content = fs::read_to_string(&standard_library_path)
        .with_context(|| format!("failed to read {}", standard_library_path.display()))?;
    let mut standard_library = match standard_library_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "toml" => toml::from_str::<StandardLibrary>(&content).with_context(|| {
            format!(
                "failed to parse Selene standard library {}",
                standard_library_path.display()
            )
        })?,
        _ => serde_yaml::from_str::<StandardLibrary>(&content).with_context(|| {
            format!(
                "failed to parse Selene standard library {}",
                standard_library_path.display()
            )
        })?,
    };
    if let Some(base_name) = standard_library
        .base
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        let base = load_selene_standard_library_component(config_dir, base_name.trim())?;
        standard_library.extend(base);
    }
    Ok(standard_library)
}

fn resolve_selene_standard_library_path(config_dir: &Path, component: &str) -> Option<PathBuf> {
    let explicit = PathBuf::from(component);
    if explicit.is_absolute() && explicit.exists() {
        return Some(explicit);
    }

    let mut candidates = Vec::new();
    candidates.push(config_dir.join(component));
    if explicit.extension().is_none() {
        candidates.push(config_dir.join(format!("{component}.yml")));
        candidates.push(config_dir.join(format!("{component}.yaml")));
        candidates.push(config_dir.join(format!("{component}.toml")));
    }

    candidates.into_iter().find(|candidate| candidate.exists())
}

fn issue_from_embedded_diagnostic(
    content: &str,
    line_index: &ByteLineIndex,
    diagnostic: selene_lib::CheckerDiagnostic,
) -> LuaLintIssue {
    let (line, column) = line_index.locate(content, diagnostic.diagnostic.primary_label.range.0);
    let (end_line, end_column) =
        line_index.locate(content, diagnostic.diagnostic.primary_label.range.1);
    let message = if diagnostic.diagnostic.notes.is_empty() {
        diagnostic.diagnostic.message
    } else {
        format!(
            "{} ({})",
            diagnostic.diagnostic.message,
            diagnostic.diagnostic.notes.join("; ")
        )
    };

    LuaLintIssue {
        line,
        column,
        end_line: Some(end_line),
        end_column: Some(end_column),
        code: diagnostic.diagnostic.code.to_string(),
        message,
        severity: match diagnostic.severity {
            SeleneSeverity::Error => LuaLintSeverity::Error,
            SeleneSeverity::Allow | SeleneSeverity::Warning => LuaLintSeverity::Warning,
        },
    }
}

fn issue_sort_key(left: &LuaLintIssue, right: &LuaLintIssue) -> std::cmp::Ordering {
    left.line
        .cmp(&right.line)
        .then_with(|| left.column.cmp(&right.column))
        .then_with(|| left.code.cmp(&right.code))
        .then_with(|| left.message.cmp(&right.message))
}

fn clamp_to_char_boundary(content: &str, index: usize) -> usize {
    let mut index = index.min(content.len());
    while index > 0 && !content.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn resolve_selene_override_path() -> Option<PathBuf> {
    env::var("SELENE_PATH")
        .ok()
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| path.exists())
}

fn resolve_selene_config_path(project_root: &Path) -> Option<PathBuf> {
    if let Some(env_path) = env::var("SELENE_CONFIG_PATH")
        .ok()
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| path.exists())
    {
        return Some(env_path);
    }

    if let Some(executable_dir) = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        && let Some(config_path) = search_selene_config_ancestors(&executable_dir)
    {
        return Some(config_path);
    }

    search_selene_config_ancestors(project_root)
}

pub fn parse_selene_output(output: &str, title: &str) -> Result<LuaLintResult> {
    if output.trim().is_empty() {
        return Ok(LuaLintResult {
            title: title.to_string(),
            errors: Vec::new(),
            warnings: Vec::new(),
        });
    }

    let mut diagnostics = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("Results:") {
            break;
        }
        if is_summary_line(trimmed) {
            continue;
        }
        if trimmed.starts_with('{')
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
        {
            diagnostics.push(value);
        }
    }
    if diagnostics.is_empty()
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output)
    {
        if let Some(array) = parsed.as_array() {
            diagnostics.extend(array.iter().cloned());
        } else if let Some(array) = parsed.get("diagnostics").and_then(|value| value.as_array()) {
            diagnostics.extend(array.iter().cloned());
        } else if let Some(array) = parsed.get("results").and_then(|value| value.as_array()) {
            diagnostics.extend(array.iter().cloned());
        }
    }

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for diag in diagnostics {
        let Some(object) = diag.as_object() else {
            continue;
        };
        let severity = parse_severity(
            object.get("severity"),
            object.get("level"),
            object.get("kind"),
            object.get("type"),
        );
        let code = parse_string(object.get("code"))
            .or_else(|| parse_string(object.get("rule")))
            .unwrap_or_else(|| "selene".to_string());
        let message = parse_string(object.get("message"))
            .or_else(|| parse_string(object.get("msg")))
            .unwrap_or_else(|| "Lua lint issue".to_string());

        let line = read_number(&[
            object.get("line"),
            object.get("startLine"),
            object.get("start_line"),
            object
                .get("primary_label")
                .and_then(|value| value.get("span"))
                .and_then(|value| value.get("start_line")),
            object.get("position").and_then(|value| value.get("line")),
        ]);
        let column = read_number(&[
            object.get("column"),
            object.get("col"),
            object.get("startColumn"),
            object.get("start_column"),
            object
                .get("primary_label")
                .and_then(|value| value.get("span"))
                .and_then(|value| value.get("start_column")),
            object.get("position").and_then(|value| value.get("col")),
        ]);
        let end_line = read_number(&[
            object.get("endLine"),
            object.get("end_line"),
            object
                .get("primary_label")
                .and_then(|value| value.get("span"))
                .and_then(|value| value.get("end_line")),
        ]);
        let end_column = read_number(&[
            object.get("endColumn"),
            object.get("end_column"),
            object
                .get("primary_label")
                .and_then(|value| value.get("span"))
                .and_then(|value| value.get("end_column")),
        ]);

        let issue = LuaLintIssue {
            line: line.and_then(|value| value.checked_add(1)).unwrap_or(1),
            column: column.and_then(|value| value.checked_add(1)).unwrap_or(1),
            end_line: end_line.and_then(|value| value.checked_add(1)),
            end_column: end_column.and_then(|value| value.checked_add(1)),
            code,
            message,
            severity,
        };
        if issue.severity == LuaLintSeverity::Error {
            errors.push(issue);
        } else {
            warnings.push(issue);
        }
    }

    Ok(LuaLintResult {
        title: title.to_string(),
        errors,
        warnings,
    })
}

fn parse_severity(
    first: Option<&serde_json::Value>,
    second: Option<&serde_json::Value>,
    third: Option<&serde_json::Value>,
    fourth: Option<&serde_json::Value>,
) -> LuaLintSeverity {
    for value in [first, second, third, fourth].into_iter().flatten() {
        if let Some(text) = value.as_str()
            && text.to_ascii_lowercase().contains("error")
        {
            return LuaLintSeverity::Error;
        }
    }
    LuaLintSeverity::Warning
}

fn parse_string(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
}

fn read_number(values: &[Option<&serde_json::Value>]) -> Option<usize> {
    for value in values.iter().flatten() {
        if let Some(number) = value.as_u64()
            && let Ok(number) = usize::try_from(number)
        {
            return Some(number);
        }
        if let Some(number) = value.as_i64()
            && number >= 0
            && let Ok(number) = usize::try_from(number as u64)
        {
            return Some(number);
        }
    }
    None
}

fn is_summary_line(line: &str) -> bool {
    let mut chars = line.chars().peekable();
    let mut saw_digit = false;
    while let Some(ch) = chars.peek() {
        if ch.is_ascii_digit() {
            saw_digit = true;
            chars.next();
        } else {
            break;
        }
    }
    if !saw_digit {
        return false;
    }
    while let Some(ch) = chars.peek() {
        if ch.is_ascii_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    let remainder = chars.collect::<String>().to_ascii_lowercase();
    remainder == "error"
        || remainder == "errors"
        || remainder == "warning"
        || remainder == "warnings"
        || remainder == "parse error"
        || remainder == "parse errors"
}

fn normalize_module_title(title: &str) -> String {
    let normalized = title.replace('_', " ");
    if normalized.starts_with("Module:") {
        normalized
    } else {
        format!("Module:{}", normalized.trim())
    }
}

fn sanitize_title(title: &str) -> String {
    let mut output = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "module".to_string()
    } else {
        output.chars().take(120).collect()
    }
}

fn absolute_from_relative(paths: &ResolvedPaths, relative: &str) -> PathBuf {
    let mut output = paths.project_root.clone();
    for segment in relative.split('/') {
        if !segment.is_empty() {
            output.push(segment);
        }
    }
    output
}

fn is_lua_module_path(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("lua")
}

fn search_selene_config_ancestors(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(current) = cursor {
        let config = current.join("config").join("selene.toml");
        if config.exists() {
            return Some(config);
        }
        let legacy = current
            .join("custom")
            .join("wikitool")
            .join("config")
            .join("selene.toml");
        if legacy.exists() {
            return Some(legacy);
        }
        cursor = current.parent();
    }
    None
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{
        EMBEDDED_SELENE_BACKEND, LuaLintSeverity, lint_modules, normalize_module_title,
        parse_selene_output,
    };
    use crate::runtime::{ResolvedPaths, ValueSource};

    fn paths(project_root: &Path) -> ResolvedPaths {
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
            parser_config_path: project_root
                .join(".wikitool")
                .join(crate::runtime::PARSER_CONFIG_FILENAME),
            project_root: project_root.to_path_buf(),
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn parse_selene_ndjson_payload() {
        let output = r#"{"severity":"error","code":"foo","message":"bad","line":0,"column":1}
{"severity":"warning","code":"bar","message":"meh","primary_label":{"span":{"start_line":2,"start_column":4}}}
Results:
1 errors
1 warnings
"#;
        let parsed = parse_selene_output(output, "Module:Alpha").expect("parse");
        assert_eq!(parsed.errors.len(), 1);
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.errors[0].line, 1);
        assert_eq!(parsed.errors[0].column, 2);
        assert_eq!(parsed.warnings[0].line, 3);
        assert_eq!(parsed.warnings[0].column, 5);
        assert_eq!(parsed.errors[0].severity, LuaLintSeverity::Error);
    }

    #[test]
    fn parse_selene_array_payload() {
        let output =
            r#"[{"severity":"warning","message":"msg","rule":"unused","line":5,"column":0}]"#;
        let parsed = parse_selene_output(output, "Module:Beta").expect("parse");
        assert_eq!(parsed.errors.len(), 0);
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.warnings[0].line, 6);
        assert_eq!(parsed.warnings[0].column, 1);
    }

    #[test]
    fn normalize_module_title_adds_prefix_and_spaces() {
        assert_eq!(normalize_module_title("Foo_Bar"), "Module:Foo Bar");
        assert_eq!(normalize_module_title("Module:Foo_Bar"), "Module:Foo Bar");
    }

    #[test]
    fn embedded_lint_uses_local_mw_standard_library() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        let paths = paths(&project_root);
        fs::create_dir_all(paths.templates_dir.join("misc")).expect("templates dir");
        fs::create_dir_all(project_root.join("config")).expect("config dir");
        fs::write(
            project_root.join("config").join("selene.toml"),
            r#"
std = "mw"

[lints]
global_usage = "warn"
"#,
        )
        .expect("write selene config");
        fs::write(
            project_root.join("config").join("mw.yml"),
            r#"
name: mw
base: lua51

globals:
  mw:
    any: true
"#,
        )
        .expect("write mw std");
        fs::write(
            paths.templates_dir.join("misc").join("Module_Test.lua"),
            r#"
local trimmed = mw.text.trim("  x  ")
return trimmed
"#,
        )
        .expect("write module");

        let report = lint_modules(&paths, Some("Module:Test")).expect("lint report");
        assert!(report.selene_available);
        assert_eq!(report.selene_path.as_deref(), Some(EMBEDDED_SELENE_BACKEND));
        assert_eq!(report.inspected_modules, 1);
        assert_eq!(report.total_errors, 0);
    }
}
