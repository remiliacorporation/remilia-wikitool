use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::filesystem::{ScanOptions, scan_files, title_to_relative_path};
use crate::runtime::ResolvedPaths;

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

pub fn lint_modules(paths: &ResolvedPaths, title: Option<&str>) -> Result<LuaLintReport> {
    let Some(selene_path) = find_selene_path(&paths.project_root) else {
        return Ok(LuaLintReport {
            selene_available: false,
            selene_path: None,
            config_path: resolve_selene_config_path(&paths.project_root)
                .map(|path| normalize_path(&path)),
            inspected_modules: 0,
            total_errors: 0,
            total_warnings: 0,
            results: Vec::new(),
        });
    };
    let config_path = resolve_selene_config_path(&paths.project_root);

    let mut results = Vec::new();
    let mut inspected_modules = 0usize;

    if let Some(title) = title {
        let normalized = normalize_module_title(title);
        let relative_path = title_to_relative_path(paths, &normalized, false)?;
        let absolute_path = absolute_from_relative(paths, &relative_path);
        if !absolute_path.exists() {
            bail!("module not found: {normalized}");
        }
        let content = fs::read_to_string(&absolute_path)
            .with_context(|| format!("failed to read {}", absolute_path.display()))?;
        let result = lint_lua_content(
            &selene_path,
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
            let content = fs::read_to_string(&absolute_path)
                .with_context(|| format!("failed to read {}", absolute_path.display()))?;
            let result = lint_lua_content(
                &selene_path,
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
        selene_path: Some(normalize_path(&selene_path)),
        config_path: config_path.map(|path| normalize_path(&path)),
        inspected_modules,
        total_errors,
        total_warnings,
        results,
    })
}

fn lint_lua_content(
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

pub fn find_selene_path(project_root: &Path) -> Option<PathBuf> {
    if let Some(env_path) = env::var("SELENE_PATH")
        .ok()
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| path.exists())
    {
        return Some(env_path);
    }

    let binary_name = if cfg!(windows) {
        "selene.exe"
    } else {
        "selene"
    };
    let local_tools = project_root.join("tools").join(binary_name);
    if local_tools.exists() {
        return Some(local_tools);
    }

    let names = if cfg!(windows) {
        vec!["selene.exe", "selene.cmd", "selene"]
    } else {
        vec!["selene"]
    };

    let path_var = env::var("PATH").ok()?;
    let separator = if cfg!(windows) { ';' } else { ':' };
    for part in path_var.split(separator) {
        let candidate_dir = PathBuf::from(strip_wrapping_quotes(part.trim()));
        if candidate_dir.as_os_str().is_empty() {
            continue;
        }
        for name in &names {
            let candidate = candidate_dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn resolve_selene_config_path(project_root: &Path) -> Option<PathBuf> {
    if let Some(env_path) = env::var("SELENE_CONFIG_PATH")
        .ok()
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| path.exists())
    {
        return Some(env_path);
    }

    let mut cursor = Some(project_root);
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

fn strip_wrapping_quotes(value: &str) -> &str {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{LuaLintSeverity, normalize_module_title, parse_selene_output};

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
}
