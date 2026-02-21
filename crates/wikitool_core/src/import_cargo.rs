use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::filesystem::{NamespaceMapper, validate_scoped_path};
use crate::runtime::ResolvedPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSourceType {
    Csv,
    Json,
}

impl ImportSourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }

    pub fn resolve(path: &str, explicit: Option<&str>) -> Option<Self> {
        if let Some(value) = explicit {
            let value = value.trim();
            if value.eq_ignore_ascii_case("csv") {
                return Some(Self::Csv);
            }
            if value.eq_ignore_ascii_case("json") {
                return Some(Self::Json);
            }
            return None;
        }

        if path.to_ascii_lowercase().ends_with(".csv") {
            return Some(Self::Csv);
        }
        if path.to_ascii_lowercase().ends_with(".json") {
            return Some(Self::Json);
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportUpdateMode {
    Create,
    Update,
    Upsert,
}

impl ImportUpdateMode {
    pub fn parse(value: &str) -> Self {
        if value.eq_ignore_ascii_case("update") {
            return Self::Update;
        }
        if value.eq_ignore_ascii_case("upsert") {
            return Self::Upsert;
        }
        Self::Create
    }
}

#[derive(Debug, Clone)]
pub struct CargoImportOptions {
    pub table_name: String,
    pub template_name: Option<String>,
    pub title_field: Option<String>,
    pub title_prefix: Option<String>,
    pub update_mode: ImportUpdateMode,
    pub category_name: Option<String>,
    pub article_header: bool,
    pub write: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportError {
    pub row: usize,
    pub message: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportPageAction {
    Create,
    Update,
    Skip,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPageResult {
    pub title: String,
    pub relative_path: String,
    pub action: ImportPageAction,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ImportResult {
    pub pages_created: Vec<String>,
    pub pages_updated: Vec<String>,
    pub pages_skipped: Vec<String>,
    pub errors: Vec<ImportError>,
    pub pages: Vec<ImportPageResult>,
}

type CargoRow = Vec<(String, String)>;

pub fn import_to_cargo(
    paths: &ResolvedPaths,
    source_path: &Path,
    source_type: ImportSourceType,
    options: &CargoImportOptions,
) -> Result<ImportResult> {
    let source_content = fs::read_to_string(source_path)
        .with_context(|| format!("failed to read import source {}", source_path.display()))?;
    let rows = match source_type {
        ImportSourceType::Csv => parse_csv(&source_content),
        ImportSourceType::Json => parse_json(&source_content)?,
    };

    let mut result = ImportResult::default();
    let namespace_mapper = NamespaceMapper::load(paths)?;

    for (index, row) in rows.iter().enumerate() {
        let row_number = index + 1;
        let Some(title) = resolve_title(row, options) else {
            result.errors.push(ImportError {
                row: row_number,
                message: "Missing title field".to_string(),
                title: None,
            });
            continue;
        };

        let relative_path = namespace_mapper.title_to_relative_path(paths, &title, false);
        let absolute_path = absolute_from_relative(paths, &relative_path);
        validate_scoped_path(paths, &absolute_path)?;
        let exists = absolute_path.exists();

        if options.update_mode == ImportUpdateMode::Create && exists {
            result.pages_skipped.push(title.clone());
            result.pages.push(ImportPageResult {
                title,
                relative_path,
                action: ImportPageAction::Skip,
                content: None,
            });
            continue;
        }
        if options.update_mode == ImportUpdateMode::Update && !exists {
            result.pages_skipped.push(title.clone());
            result.pages.push(ImportPageResult {
                title,
                relative_path,
                action: ImportPageAction::Skip,
                content: None,
            });
            continue;
        }

        let content = generate_cargo_page(row, options, &title);
        let action = if exists {
            ImportPageAction::Update
        } else {
            ImportPageAction::Create
        };

        if options.write {
            if let Some(parent) = absolute_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "failed to create import output directory {}",
                        parent.display()
                    )
                })?;
            }
            fs::write(&absolute_path, &content)
                .with_context(|| format!("failed to write {}", absolute_path.display()))?;
        }

        match action {
            ImportPageAction::Create => result.pages_created.push(title.clone()),
            ImportPageAction::Update => result.pages_updated.push(title.clone()),
            ImportPageAction::Skip => {}
        }
        result.pages.push(ImportPageResult {
            title,
            relative_path,
            action,
            content: if options.write { None } else { Some(content) },
        });
    }

    Ok(result)
}

pub fn parse_csv(content: &str) -> Vec<CargoRow> {
    let rows = parse_csv_rows(strip_bom(content), ',');
    if rows.is_empty() {
        return Vec::new();
    }

    let headers = rows[0]
        .iter()
        .map(|value| value.trim().to_string())
        .collect::<Vec<_>>();

    let mut output = Vec::new();
    for row in rows.iter().skip(1) {
        if row.iter().all(|value| value.trim().is_empty()) {
            continue;
        }
        let mut mapped = Vec::new();
        for (index, header) in headers.iter().enumerate() {
            if header.is_empty() {
                continue;
            }
            mapped.push((header.clone(), row.get(index).cloned().unwrap_or_default()));
        }
        output.push(mapped);
    }
    output
}

pub fn parse_json(content: &str) -> Result<Vec<CargoRow>> {
    let trimmed = strip_bom(content).trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let parsed: serde_json::Value =
        serde_json::from_str(trimmed).context("failed to parse JSON import source")?;
    let Some(rows) = parsed.as_array() else {
        return Ok(Vec::new());
    };

    let mut output = Vec::new();
    for row in rows {
        let Some(object) = row.as_object() else {
            continue;
        };
        let mut mapped = Vec::new();
        for (key, value) in object {
            let value = match value {
                serde_json::Value::Null => String::new(),
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            mapped.push((key.clone(), value));
        }
        output.push(mapped);
    }
    Ok(output)
}

pub fn generate_cargo_page(row: &CargoRow, options: &CargoImportOptions, title: &str) -> String {
    let mut blocks = Vec::new();
    if options.article_header && is_main_namespace(title) {
        let mut header_lines = Vec::new();
        if let Some(shortdesc) = pick_shortdesc(row, title) {
            let mut truncated = shortdesc;
            if truncated.chars().count() > 100 {
                truncated = truncated.chars().take(100).collect();
            }
            header_lines.push(format!("{{{{SHORTDESC:{truncated}}}}}"));
        }
        header_lines.push("{{Article quality|unverified}}".to_string());
        blocks.push(header_lines.join("\n"));
    }

    let params = row
        .iter()
        .map(|(key, value)| format!("|{key}={}", escape_cargo_value(value)))
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(template_name) = options.template_name.as_deref() {
        if params.is_empty() {
            blocks.push(format!("{{{{{template_name}}}}}"));
        } else {
            blocks.push(format!("{{{{{template_name}\n{params}}}}}"));
        }
    } else if params.is_empty() {
        blocks.push(format!(
            "{{{{#cargo_store:_table={}}}}}",
            options.table_name
        ));
    } else {
        blocks.push(format!(
            "{{{{#cargo_store:_table={}\n{params}}}}}",
            options.table_name
        ));
    }

    if let Some(category_name) = options.category_name.as_deref() {
        blocks.push(format!("[[Category:{category_name}]]"));
    }

    blocks.join("\n\n")
}

fn resolve_title(row: &CargoRow, options: &CargoImportOptions) -> Option<String> {
    let title_field = options.title_field.clone().or_else(|| {
        if row_has_key(row, "title") {
            Some("title".to_string())
        } else if row_has_key(row, "name") {
            Some("name".to_string())
        } else {
            None
        }
    })?;
    let value = row_value(row, &title_field)?;
    let base = value.trim();
    if base.is_empty() {
        return None;
    }
    let prefix = options.title_prefix.as_deref().unwrap_or("");
    if prefix.is_empty() {
        Some(base.to_string())
    } else {
        Some(format!("{prefix}{base}"))
    }
}

fn row_has_key(row: &CargoRow, key: &str) -> bool {
    row.iter().any(|(name, _)| name == key)
}

fn row_value<'a>(row: &'a CargoRow, key: &str) -> Option<&'a str> {
    row.iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn pick_shortdesc(row: &CargoRow, title: &str) -> Option<String> {
    for key in ["shortdesc", "description", "name"] {
        if let Some(value) = row_value(row, key)
            && !value.trim().is_empty()
        {
            return Some(value.trim().to_string());
        }
    }
    if title.trim().is_empty() {
        None
    } else {
        Some(title.trim().to_string())
    }
}

fn escape_cargo_value(value: &str) -> String {
    if value.contains('|') || value.contains("}}") {
        format!("<nowiki>{value}</nowiki>")
    } else {
        value.to_string()
    }
}

fn parse_csv_rows(content: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let chars = content.chars().collect::<Vec<_>>();
    let mut index = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        if in_quotes {
            if ch == '"' {
                if index + 1 < chars.len() && chars[index + 1] == '"' {
                    field.push('"');
                    index += 2;
                    continue;
                }
                in_quotes = false;
                index += 1;
                continue;
            }
            field.push(ch);
            index += 1;
            continue;
        }

        if ch == '"' {
            in_quotes = true;
            index += 1;
            continue;
        }
        if ch == delimiter {
            row.push(field);
            field = String::new();
            index += 1;
            continue;
        }
        if ch == '\n' || ch == '\r' {
            row.push(field);
            field = String::new();
            if ch == '\r' && index + 1 < chars.len() && chars[index + 1] == '\n' {
                index += 1;
            }
            rows.push(row);
            row = Vec::new();
            index += 1;
            continue;
        }
        field.push(ch);
        index += 1;
    }

    row.push(field);
    if row.len() > 1 || row.first().is_some_and(|value| !value.trim().is_empty()) {
        rows.push(row);
    }
    rows
}

fn strip_bom(content: &str) -> &str {
    if content.as_bytes().starts_with(&[0xEF, 0xBB, 0xBF]) {
        &content[3..]
    } else {
        content
    }
}

fn is_main_namespace(title: &str) -> bool {
    !title.contains(':')
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{
        CargoImportOptions, ImportSourceType, ImportUpdateMode, generate_cargo_page,
        import_to_cargo, parse_csv, parse_json,
    };
    use crate::runtime::{ResolvedPaths, ValueSource};

    fn paths(project_root: PathBuf) -> ResolvedPaths {
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
            project_root,
            root_source: ValueSource::Flag,
            data_source: ValueSource::Default,
            config_source: ValueSource::Default,
        }
    }

    #[test]
    fn parse_csv_supports_quotes_and_newlines() {
        let rows = parse_csv(
            "title,description\nAlpha,\"line 1\nline 2\"\n\"Beta\",\"has \"\"quotes\"\"\"",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].0, "title");
        assert_eq!(rows[0][0].1, "Alpha");
        assert_eq!(rows[0][1].1, "line 1\nline 2");
        assert_eq!(rows[1][0].1, "Beta");
        assert_eq!(rows[1][1].1, "has \"quotes\"");
    }

    #[test]
    fn parse_json_maps_objects_to_rows() {
        let rows =
            parse_json(r#"[{"title":"Alpha","n":5},{"title":"Beta","x":null}]"#).expect("json");
        assert_eq!(rows.len(), 2);
        assert!(
            rows[0]
                .iter()
                .any(|(key, value)| key == "title" && value == "Alpha")
        );
        assert!(
            rows[1]
                .iter()
                .any(|(key, value)| key == "x" && value.is_empty())
        );
    }

    #[test]
    fn generate_cargo_page_emits_header_and_nowiki_escape() {
        let row = vec![
            ("name".to_string(), "Alpha".to_string()),
            ("payload".to_string(), "left|right".to_string()),
        ];
        let content = generate_cargo_page(
            &row,
            &CargoImportOptions {
                table_name: "Items".to_string(),
                template_name: None,
                title_field: Some("name".to_string()),
                title_prefix: None,
                update_mode: ImportUpdateMode::Create,
                category_name: Some("Cargo".to_string()),
                article_header: true,
                write: false,
            },
            "Alpha",
        );
        assert!(content.contains("{{SHORTDESC:Alpha}}"));
        assert!(content.contains("<nowiki>left|right</nowiki>"));
        assert!(content.contains("[[Category:Cargo]]"));
    }

    #[test]
    fn import_to_cargo_writes_and_respects_update_mode() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("project");
        let paths = paths(root.clone());
        std::fs::create_dir_all(&paths.wiki_content_dir).expect("content dir");
        std::fs::create_dir_all(&paths.state_dir).expect("state dir");
        let source = root.join("import.csv");
        std::fs::write(&source, "title,description\nAlpha,first\nBeta,second").expect("source");

        let created = import_to_cargo(
            &paths,
            &source,
            ImportSourceType::Csv,
            &CargoImportOptions {
                table_name: "Items".to_string(),
                template_name: None,
                title_field: None,
                title_prefix: None,
                update_mode: ImportUpdateMode::Create,
                category_name: None,
                article_header: false,
                write: true,
            },
        )
        .expect("import create");
        assert_eq!(created.pages_created.len(), 2);

        let second = import_to_cargo(
            &paths,
            &source,
            ImportSourceType::Csv,
            &CargoImportOptions {
                table_name: "Items".to_string(),
                template_name: None,
                title_field: None,
                title_prefix: None,
                update_mode: ImportUpdateMode::Create,
                category_name: None,
                article_header: false,
                write: true,
            },
        )
        .expect("import second");
        assert_eq!(second.pages_skipped.len(), 2);
    }
}
