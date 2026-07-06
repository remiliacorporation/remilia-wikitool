use std::cmp::min;
use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::encoding::read_required_text;
use crate::files::display_path;
use crate::json_tools::contains_any;
use crate::output::{base_receipt, clamp_text, emit_json_checked, write_receipt_checked};
use crate::text::collect_single_text_source;

#[derive(Debug)]
struct SqliteTableSummary {
    schema: String,
    name: String,
    kind: String,
    column_count_declared: i64,
    without_rowid: bool,
    strict: bool,
    columns: Vec<SqliteColumnSummary>,
    indexes: Vec<SqliteIndexSummary>,
    columns_total: usize,
    indexes_total: usize,
    detail_elided: bool,
}

#[derive(Debug)]
struct SqliteColumnSummary {
    name: String,
    type_name: String,
    not_null: bool,
    default_value: Option<String>,
    primary_key_rank: i64,
    hidden: i64,
    foreign_key: Option<SqliteForeignKeySummary>,
}

#[derive(Clone, Debug)]
struct SqliteForeignKeySummary {
    table: String,
    column: String,
}

#[derive(Debug)]
struct SqliteIndexSummary {
    name: String,
    unique: bool,
    origin: String,
    partial: bool,
    columns: Vec<String>,
}

#[derive(Debug)]
struct SqliteFileParam {
    sql_name: String,
    path: PathBuf,
    format: &'static str,
    value: String,
    /// Top-level value count when the bound document is an array (json_each
    /// row cardinality); None for a non-array JSON document.
    values: Option<usize>,
    source_bytes: u64,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_sqlite(
    cli: &Cli,
    config: &ContextConfig,
    db: &Path,
    sql: Option<&str>,
    sql_file: Option<&Path>,
    json_params: &[String],
    jsonl_params: &[String],
    max_param_bytes: u64,
    max_rows: usize,
    max_scan_rows: usize,
    timeout_secs: u64,
    max_value_chars: usize,
) -> Result<()> {
    if max_rows == 0 {
        return Err(anyhow!("sqlite --max-rows must be greater than zero"));
    }
    if max_scan_rows == 0 {
        return Err(anyhow!("sqlite --max-scan-rows must be greater than zero"));
    }
    if max_scan_rows < max_rows {
        return Err(anyhow!(
            "sqlite --max-scan-rows must be greater than or equal to --max-rows"
        ));
    }
    if max_param_bytes == 0 {
        return Err(anyhow!(
            "sqlite --max-param-bytes must be greater than zero"
        ));
    }
    let sql = collect_single_text_source("sqlite SQL", sql, sql_file, false)?;
    if sql.trim().is_empty() {
        return Err(anyhow!("sqlite SQL must not be empty"));
    }
    let params = collect_sqlite_file_params(json_params, jsonl_params, max_param_bytes)?;
    let conn = open_sqlite_readonly(db)?;
    let _watchdog = QueryWatchdog::arm(&conn, timeout_secs);
    let mut stmt = conn.prepare(&sql).context("failed to prepare sqlite SQL")?;
    if stmt.parameter_count() != 0 && params.is_empty() {
        return Err(anyhow!(
            "sqlite query contains parameters; bind named JSON inputs with --json-param or --jsonl-param"
        ));
    }
    if !stmt.readonly() {
        return Err(anyhow!("sqlite command only accepts read-only statements"));
    }
    bind_sqlite_file_params(&mut stmt, &params)?;
    let column_count = stmt.column_count();
    let columns = stmt
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut row_iter = stmt.raw_query();
    let mut rendered_rows = Vec::new();
    let mut json_rows = Vec::new();
    let mut total_seen = 0usize;
    let mut scan_truncated = false;
    while let Some(row) = row_iter
        .next()
        .map_err(|error| annotate_interrupt(error, timeout_secs))?
    {
        total_seen += 1;
        if total_seen <= max_rows {
            let mut rendered = Vec::with_capacity(column_count);
            let mut fields = serde_json::Map::new();
            for (index, column) in columns.iter().enumerate() {
                let summary = sqlite_value_summary(row.get_ref(index)?, max_value_chars);
                rendered.push((column.clone(), summary.clone()));
                fields.insert(column.clone(), json!(summary));
            }
            rendered_rows.push(rendered);
            json_rows.push(json!({
                "row": total_seen - 1,
                "fields": fields,
            }));
        }
        if total_seen > max_scan_rows {
            scan_truncated = true;
            break;
        }
    }
    let shown = rendered_rows.len();
    let cap_reason = if scan_truncated {
        Some("scan")
    } else if shown < total_seen {
        Some("rows")
    } else {
        None
    };
    if cli.json {
        let mut map = base_receipt(
            "sqlite",
            config.profile.as_deref(),
            "rows",
            shown,
            total_seen,
            cap_reason.is_some(),
            cap_reason,
        );
        map.insert("db".to_string(), json!(display_path(db)));
        map.insert("columns".to_string(), json!(columns));
        map.insert("params".to_string(), sqlite_param_receipt_rows(&params));
        map.insert("rows_scanned".to_string(), json!(total_seen));
        map.insert(
            "rows_total_is_lower_bound".to_string(),
            json!(scan_truncated),
        );
        map.insert("rows".to_string(), json!(json_rows));
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        writeln!(
            stdout,
            "[contextmink] sqlite db={} columns={}",
            display_path(db),
            columns.join(",")
        )?;
        if rendered_rows.is_empty() {
            writeln!(stdout, "no_rows")?;
        }
        for (row_index, fields) in rendered_rows.iter().enumerate() {
            let rendered = fields
                .iter()
                .map(|(column, value)| format!("{column}={value}"))
                .collect::<Vec<_>>()
                .join(" ");
            writeln!(stdout, "{row_index}: {rendered}")?;
        }
        if scan_truncated {
            writeln!(
                stdout,
                "[contextmink] capped sqlite scan at {max_scan_rows} rows; add WHERE/LIMIT or narrow the query before treating this as complete."
            )?;
        } else if shown < total_seen {
            writeln!(
                stdout,
                "[contextmink] capped sqlite output at {max_rows} rows; increase --max-rows or narrow the query."
            )?;
        }
        let mut map = base_receipt(
            "sqlite",
            config.profile.as_deref(),
            "rows",
            shown,
            total_seen,
            cap_reason.is_some(),
            cap_reason,
        );
        map.insert("columns".to_string(), json!(columns));
        map.insert("params".to_string(), sqlite_param_receipt_rows(&params));
        map.insert("rows_scanned".to_string(), json!(total_seen));
        map.insert(
            "rows_total_is_lower_bound".to_string(),
            json!(scan_truncated),
        );
        write_receipt_checked(cli, map)
    }
}

fn collect_sqlite_file_params(
    json_params: &[String],
    jsonl_params: &[String],
    max_param_bytes: u64,
) -> Result<Vec<SqliteFileParam>> {
    let mut params = Vec::with_capacity(json_params.len() + jsonl_params.len());
    let mut names = BTreeSet::new();
    for raw in json_params {
        let (sql_name, path) = parse_sqlite_file_param(raw, "--json-param")?;
        if !names.insert(sql_name.clone()) {
            return Err(anyhow!("duplicate sqlite parameter binding {sql_name}"));
        }
        params.push(load_sqlite_json_param(
            sql_name,
            path,
            "json",
            max_param_bytes,
        )?);
    }
    for raw in jsonl_params {
        let (sql_name, path) = parse_sqlite_file_param(raw, "--jsonl-param")?;
        if !names.insert(sql_name.clone()) {
            return Err(anyhow!("duplicate sqlite parameter binding {sql_name}"));
        }
        params.push(load_sqlite_json_param(
            sql_name,
            path,
            "jsonl",
            max_param_bytes,
        )?);
    }
    Ok(params)
}

fn parse_sqlite_file_param(raw: &str, flag: &str) -> Result<(String, PathBuf)> {
    let (name, path) = raw
        .split_once('=')
        .ok_or_else(|| anyhow!("{flag} requires NAME=FILE, found {raw:?}"))?;
    let sql_name = normalize_sqlite_param_name(name, flag)?;
    let path = path.trim();
    if path.is_empty() {
        return Err(anyhow!("{flag} requires a non-empty FILE path: {raw:?}"));
    }
    Ok((sql_name, PathBuf::from(path)))
}

fn normalize_sqlite_param_name(name: &str, flag: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("{flag} requires a non-empty parameter name"));
    }
    let mut chars = name.chars();
    let first = chars.next().expect("empty name checked above");
    let (prefix, body) = if matches!(first, ':' | '@' | '$') {
        (first, chars.as_str())
    } else {
        (':', name)
    };
    if body.is_empty() {
        return Err(anyhow!("{flag} requires a name after {prefix:?}"));
    }
    if body
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_'))
    {
        return Err(anyhow!(
            "{flag} parameter name {name:?} may contain only ASCII letters, digits, and underscores"
        ));
    }
    Ok(format!("{prefix}{body}"))
}

fn load_sqlite_json_param(
    sql_name: String,
    path: PathBuf,
    format: &'static str,
    max_param_bytes: u64,
) -> Result<SqliteFileParam> {
    let metadata =
        std::fs::metadata(&path).with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.len() > max_param_bytes {
        return Err(anyhow!(
            "{} is {} bytes, larger than sqlite --max-param-bytes {}",
            path.display(),
            metadata.len(),
            max_param_bytes
        ));
    }
    let (text, _) = read_required_text(&path)
        .with_context(|| format!("failed to read sqlite parameter {}", path.display()))?;
    let (value, values) = match format {
        "json" => {
            let value: Value = serde_json::from_str(&text)
                .map_err(|error| json_param_parse_error(&path, &text, error))?;
            let values = value.as_array().map(Vec::len);
            (
                serde_json::to_string(&value).context("failed to serialize JSON parameter")?,
                values,
            )
        }
        "jsonl" => jsonl_to_json_array_text(&path, &text)?,
        _ => unreachable!("sqlite param formats are fixed by callers"),
    };
    Ok(SqliteFileParam {
        sql_name,
        path,
        format,
        value,
        values,
        source_bytes: metadata.len(),
    })
}

/// A `--json-param` file that fails as a single JSON document but parses as
/// multiple JSONL values is almost certainly a JSONL worklist bound with the
/// wrong flag; teach the fix instead of surfacing a bare serde error.
fn json_param_parse_error(path: &Path, text: &str, error: serde_json::Error) -> anyhow::Error {
    let jsonl_values = serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .take_while(|row| row.is_ok())
        .count();
    if jsonl_values > 1 {
        return anyhow!(
            "{} is not a single JSON document but parses as {} JSONL values; bind it with --jsonl-param instead",
            path.display(),
            jsonl_values
        );
    }
    anyhow::Error::new(error).context(format!("failed to parse JSON parameter {}", path.display()))
}

fn jsonl_to_json_array_text(path: &Path, text: &str) -> Result<(String, Option<usize>)> {
    let stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    let mut rows = Vec::new();
    for (index, row) in stream.enumerate() {
        rows.push(row.with_context(|| {
            format!(
                "failed to parse JSONL value {} in {}",
                index + 1,
                path.display()
            )
        })?);
    }
    // A lone top-level array is a plain JSON array file: wrapping it would
    // bind [[...]] and json_each would silently see one row instead of N.
    if let [Value::Array(inner)] = rows.as_slice() {
        return Err(anyhow!(
            "{} holds a single top-level JSON array ({} elements); binding it as JSONL would wrap it to one json_each row — use --json-param instead",
            path.display(),
            inner.len()
        ));
    }
    let values = Some(rows.len());
    Ok((
        serde_json::to_string(&rows)
            .context("failed to serialize JSONL parameter as JSON array")?,
        values,
    ))
}

fn bind_sqlite_file_params(
    stmt: &mut rusqlite::Statement<'_>,
    params: &[SqliteFileParam],
) -> Result<()> {
    let mut bound_indexes = BTreeSet::new();
    for param in params {
        let index = stmt
            .parameter_index(&param.sql_name)
            .with_context(|| format!("failed to inspect sqlite parameter {}", param.sql_name))?
            .ok_or_else(|| {
                anyhow!(
                    "sqlite parameter binding {} was supplied but is not used by the SQL",
                    param.sql_name
                )
            })?;
        stmt.raw_bind_parameter(index, param.value.as_str())
            .with_context(|| format!("failed to bind sqlite parameter {}", param.sql_name))?;
        bound_indexes.insert(index);
    }

    for index in 1..=stmt.parameter_count() {
        if bound_indexes.contains(&index) {
            continue;
        }
        let name = stmt.parameter_name(index).ok_or_else(|| {
            anyhow!(
                "anonymous sqlite parameter at index {index} is unsupported; use a named parameter like :input"
            )
        })?;
        return Err(anyhow!(
            "unbound sqlite parameter {name}; provide --json-param or --jsonl-param"
        ));
    }
    Ok(())
}

fn sqlite_param_receipt_rows(params: &[SqliteFileParam]) -> Value {
    json!(
        params
            .iter()
            .map(|param| json!({
                "name": param.sql_name,
                "path": display_path(&param.path),
                "format": param.format,
                "values": param.values,
                "source_bytes": param.source_bytes,
            }))
            .collect::<Vec<_>>()
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_sqlite_schema(
    cli: &Cli,
    config: &ContextConfig,
    db: &Path,
    requested_tables: &[String],
    name_contains: &[String],
    include_shadow: bool,
    include_system: bool,
    max_tables: usize,
    max_columns: usize,
    max_indexes: usize,
    max_line_chars: usize,
) -> Result<()> {
    if max_tables == 0 {
        return Err(anyhow!(
            "sqlite-schema --max-tables must be greater than zero"
        ));
    }
    let conn = open_sqlite_readonly(db)?;
    let requested = requested_tables.iter().collect::<BTreeSet<_>>();
    let mut stmt = conn
        .prepare(
            "SELECT schema, name, type, ncol, wr, strict \
             FROM pragma_table_list \
             ORDER BY schema, name",
        )
        .context("failed to prepare sqlite schema query")?;
    let mut table_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)? != 0,
                row.get::<_, i64>(5)? != 0,
            ))
        })
        .context("failed to query sqlite schema")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to read sqlite schema rows")?;
    table_rows.retain(|(_, name, kind, _, _, _)| {
        if !include_system && name.starts_with("sqlite_") {
            return false;
        }
        if !include_shadow && kind == "shadow" {
            return false;
        }
        if !requested.is_empty() && !requested.contains(name) {
            return false;
        }
        if !name_contains.is_empty() && !contains_any(name, name_contains) {
            return false;
        }
        true
    });
    let total_tables = table_rows.len();
    let shown_tables = min(total_tables, max_tables);
    let mut remaining_columns = max_columns;
    let mut remaining_indexes = max_indexes;
    let mut columns_total = 0usize;
    let mut columns_shown = 0usize;
    let mut indexes_total = 0usize;
    let mut indexes_shown = 0usize;
    let mut summaries = Vec::with_capacity(shown_tables);
    let mut tables_detail_elided = 0usize;
    for (schema, name, kind, column_count_declared, without_rowid, strict) in
        table_rows.into_iter().take(shown_tables)
    {
        let all_columns = sqlite_schema_columns(&conn, &schema, &name)?;
        let all_indexes = sqlite_schema_indexes(&conn, &schema, &name)?;
        let all_columns_len = all_columns.len();
        let all_indexes_len = all_indexes.len();
        columns_total += all_columns_len;
        indexes_total += all_indexes_len;
        // Table-atomic budget: a table either shows its complete column and
        // index detail or none of it. A partially-columned table with its
        // indexes still attached reads as complete to anyone slicing the
        // middle of the output.
        let detail_elided =
            all_columns_len > remaining_columns || all_indexes_len > remaining_indexes;
        let (columns_take, indexes_take) = if detail_elided {
            tables_detail_elided += 1;
            (0, 0)
        } else {
            (all_columns_len, all_indexes_len)
        };
        columns_shown += columns_take;
        indexes_shown += indexes_take;
        remaining_columns = remaining_columns.saturating_sub(columns_take);
        remaining_indexes = remaining_indexes.saturating_sub(indexes_take);
        summaries.push(SqliteTableSummary {
            schema,
            name,
            kind,
            column_count_declared,
            without_rowid,
            strict,
            columns: all_columns.into_iter().take(columns_take).collect(),
            indexes: all_indexes.into_iter().take(indexes_take).collect(),
            columns_total: all_columns_len,
            indexes_total: all_indexes_len,
            detail_elided,
        });
    }
    let columns_truncated = columns_shown < columns_total;
    let indexes_truncated = indexes_shown < indexes_total;
    let truncated = shown_tables < total_tables || columns_truncated || indexes_truncated;
    let cap_reason = if shown_tables < total_tables {
        Some("tables")
    } else if columns_truncated {
        Some("columns")
    } else if indexes_truncated {
        Some("indexes")
    } else {
        None
    };
    if cli.json {
        let mut map = base_receipt(
            "sqlite-schema",
            config.profile.as_deref(),
            "tables",
            shown_tables,
            total_tables,
            truncated,
            cap_reason,
        );
        map.insert("db".to_string(), json!(display_path(db)));
        map.insert("columns_shown".to_string(), json!(columns_shown));
        map.insert("columns_total".to_string(), json!(columns_total));
        map.insert("indexes_shown".to_string(), json!(indexes_shown));
        map.insert("indexes_total".to_string(), json!(indexes_total));
        map.insert(
            "tables_detail_elided".to_string(),
            json!(tables_detail_elided),
        );
        map.insert(
            "tables".to_string(),
            Value::Array(
                summaries
                    .iter()
                    .map(sqlite_table_summary_json)
                    .collect::<Vec<_>>(),
            ),
        );
        return emit_json_checked(cli, Value::Object(map));
    }
    let mut stdout = io::stdout();
    writeln!(
        stdout,
        "[contextmink] sqlite-schema db={}",
        display_path(db)
    )?;
    if summaries.is_empty() {
        writeln!(stdout, "no_tables")?;
    }
    for table in &summaries {
        writeln!(
            stdout,
            "{}.{} type={} ncol={} strict={} without_rowid={}",
            table.schema,
            table.name,
            table.kind,
            table.column_count_declared,
            table.strict,
            table.without_rowid
        )?;
        for column in &table.columns {
            writeln!(
                stdout,
                "  column {}",
                clamp_text(&sqlite_column_summary_human(column), max_line_chars)
            )?;
        }
        for index in &table.indexes {
            writeln!(
                stdout,
                "  index {}",
                clamp_text(&sqlite_index_summary_human(index), max_line_chars)
            )?;
        }
        if table.detail_elided {
            writeln!(
                stdout,
                "  (detail elided: {} columns, {} indexes over budget; rerun with --table {})",
                table.columns_total, table.indexes_total, table.name
            )?;
        }
    }
    if truncated {
        writeln!(
            stdout,
            "[contextmink] capped sqlite schema output at tables={max_tables} columns={max_columns} indexes={max_indexes}; narrow with --table or --name-contains."
        )?;
    }
    let mut map = base_receipt(
        "sqlite-schema",
        config.profile.as_deref(),
        "tables",
        shown_tables,
        total_tables,
        truncated,
        cap_reason,
    );
    map.insert("columns_shown".to_string(), json!(columns_shown));
    map.insert("columns_total".to_string(), json!(columns_total));
    map.insert("indexes_shown".to_string(), json!(indexes_shown));
    map.insert("indexes_total".to_string(), json!(indexes_total));
    map.insert(
        "tables_detail_elided".to_string(),
        json!(tables_detail_elided),
    );
    write_receipt_checked(cli, map)
}

fn sqlite_schema_columns(
    conn: &Connection,
    schema_name: &str,
    table_name: &str,
) -> Result<Vec<SqliteColumnSummary>> {
    let mut fks = HashMap::new();
    let mut fk_stmt = conn
        .prepare("SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list(?, ?)")
        .context("failed to prepare sqlite foreign-key query")?;
    let fk_rows = fk_stmt
        .query_map([table_name, schema_name], |row| {
            Ok((
                row.get::<_, String>(0)?,
                SqliteForeignKeySummary {
                    table: row.get::<_, String>(1)?,
                    column: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                },
            ))
        })
        .with_context(|| format!("failed to query foreign keys for {table_name}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| format!("failed to read foreign keys for {table_name}"))?;
    for (column, fk) in fk_rows {
        fks.insert(column, fk);
    }

    let mut stmt = conn
        .prepare(
            "SELECT name, lower(type), \"notnull\", dflt_value, pk, hidden \
             FROM pragma_table_xinfo(?, ?) \
             ORDER BY cid",
        )
        .context("failed to prepare sqlite column query")?;
    stmt.query_map([table_name, schema_name], |row| {
        let name = row.get::<_, String>(0)?;
        Ok(SqliteColumnSummary {
            foreign_key: fks.get(&name).cloned(),
            name,
            type_name: row.get::<_, String>(1)?,
            not_null: row.get::<_, i64>(2)? != 0,
            default_value: row.get::<_, Option<String>>(3)?,
            primary_key_rank: row.get::<_, i64>(4)?,
            hidden: row.get::<_, i64>(5)?,
        })
    })
    .with_context(|| format!("failed to query columns for {table_name}"))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .with_context(|| format!("failed to read columns for {table_name}"))
}

fn sqlite_schema_indexes(
    conn: &Connection,
    schema_name: &str,
    table_name: &str,
) -> Result<Vec<SqliteIndexSummary>> {
    let mut stmt = conn
        .prepare(
            "SELECT name, \"unique\", origin, partial FROM pragma_index_list(?, ?) ORDER BY seq",
        )
        .context("failed to prepare sqlite index query")?;
    let mut indexes = Vec::new();
    for row in stmt
        .query_map([table_name, schema_name], |row| {
            Ok(SqliteIndexSummary {
                name: row.get::<_, String>(0)?,
                unique: row.get::<_, i64>(1)? != 0,
                origin: row.get::<_, String>(2)?,
                partial: row.get::<_, i64>(3)? != 0,
                columns: Vec::new(),
            })
        })
        .with_context(|| format!("failed to query indexes for {table_name}"))?
    {
        let mut index = row.with_context(|| format!("failed to read index for {table_name}"))?;
        let mut col_stmt = conn
            .prepare("SELECT cid, name FROM pragma_index_xinfo(?, ?) WHERE key != 0 ORDER BY seqno")
            .with_context(|| format!("failed to prepare index-column query for {}", index.name))?;
        index.columns = col_stmt
            .query_map([index.name.as_str(), schema_name], |row| {
                let cid = row.get::<_, i64>(0)?;
                let name = row.get::<_, Option<String>>(1)?;
                Ok(name.unwrap_or_else(|| match cid {
                    -2 => "<expr>".to_owned(),
                    -1 => "<rowid>".to_owned(),
                    _ => "<unknown>".to_owned(),
                }))
            })
            .with_context(|| format!("failed to query columns for index {}", index.name))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .with_context(|| format!("failed to read columns for index {}", index.name))?;
        indexes.push(index);
    }
    Ok(indexes)
}

fn sqlite_table_summary_json(table: &SqliteTableSummary) -> Value {
    json!({
        "schema": table.schema,
        "name": table.name,
        "type": table.kind,
        "ncol": table.column_count_declared,
        "strict": table.strict,
        "without_rowid": table.without_rowid,
        "columns_total": table.columns_total,
        "indexes_total": table.indexes_total,
        "detail_elided": table.detail_elided,
        "columns": table.columns.iter().map(|column| {
            json!({
                "name": column.name,
                "type": column.type_name,
                "not_null": column.not_null,
                "default": column.default_value,
                "primary_key_rank": column.primary_key_rank,
                "hidden": column.hidden,
                "foreign_key": column.foreign_key.as_ref().map(|fk| json!({
                    "table": fk.table,
                    "column": fk.column,
                })),
            })
        }).collect::<Vec<_>>(),
        "indexes": table.indexes.iter().map(|index| {
            json!({
                "name": index.name,
                "unique": index.unique,
                "origin": index.origin,
                "partial": index.partial,
                "columns": index.columns,
            })
        }).collect::<Vec<_>>(),
    })
}

fn sqlite_column_summary_human(column: &SqliteColumnSummary) -> String {
    let mut parts = vec![format!("{} {}", column.name, column.type_name)];
    if column.not_null {
        parts.push("not_null".to_owned());
    }
    if column.primary_key_rank != 0 {
        parts.push(format!("pk#{}", column.primary_key_rank));
    }
    if column.hidden != 0 {
        parts.push(format!("hidden#{}", column.hidden));
    }
    if let Some(default) = &column.default_value {
        parts.push(format!("default={default:?}"));
    }
    if let Some(fk) = &column.foreign_key {
        parts.push(format!("fk={}.{}", fk.table, fk.column));
    }
    parts.join(" ")
}

fn sqlite_index_summary_human(index: &SqliteIndexSummary) -> String {
    let mut parts = vec![format!("{}({})", index.name, index.columns.join(","))];
    if index.unique {
        parts.push("unique".to_owned());
    }
    if index.partial {
        parts.push("partial".to_owned());
    }
    parts.push(format!("origin={}", index.origin));
    parts.join(" ")
}

/// Interrupts a running query after a wall-clock budget so a runaway scan
/// fails with an accountable error instead of hanging until the calling
/// shell kills the process without a receipt.
struct QueryWatchdog {
    cancel: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl QueryWatchdog {
    fn arm(conn: &Connection, timeout_secs: u64) -> Self {
        if timeout_secs == 0 {
            return Self {
                cancel: None,
                thread: None,
            };
        }
        let handle = conn.get_interrupt_handle();
        let (cancel, cancelled) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            // A disconnect (watchdog dropped) means the query finished.
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                cancelled.recv_timeout(std::time::Duration::from_secs(timeout_secs))
            {
                handle.interrupt();
            }
        });
        Self {
            cancel: Some(cancel),
            thread: Some(thread),
        }
    }
}

impl Drop for QueryWatchdog {
    fn drop(&mut self) {
        drop(self.cancel.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join(); // guardrail: allow-ignore-result watchdog thread cannot fail meaningfully after cancellation
        }
    }
}

fn annotate_interrupt(error: rusqlite::Error, timeout_secs: u64) -> anyhow::Error {
    let interrupted = matches!(
        &error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == rusqlite::ErrorCode::OperationInterrupted
    );
    if interrupted {
        anyhow::Error::new(error).context(format!(
            "sqlite query interrupted after --timeout-secs {timeout_secs}; narrow the query (WHERE/LIMIT) or raise --timeout-secs (0 disables)"
        ))
    } else {
        anyhow::Error::new(error).context("failed to read sqlite row")
    }
}

fn open_sqlite_readonly(db: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("failed to open sqlite DB {}", db.display()))?;
    // A concurrent writer committing (rollback/TRUNCATE journals) briefly
    // locks readers out; wait instead of failing with SQLITE_BUSY.
    conn.busy_timeout(std::time::Duration::from_millis(5000))
        .context("failed to set sqlite busy timeout")?;
    conn.execute_batch("PRAGMA query_only = ON")
        .context("failed to enable sqlite query_only mode")?;
    register_hexint(&conn)?;
    Ok(conn)
}

/// `hexint(x)`: parse a `0x`-prefixed hex string (or a plain decimal digit
/// string) to INTEGER; integers pass through, NULL stays NULL. SQLite's own
/// CAST cannot parse hex, and inspection data often carries address-like
/// identifiers as `0x...` strings while tables store integer columns. This
/// bridges the two inside an indexed join instead of forcing scratch
/// conversion outside SQL.
fn register_hexint(conn: &Connection) -> Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "hexint",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let value = ctx.get_raw(0);
            match value {
                ValueRef::Null => Ok(rusqlite::types::Value::Null),
                ValueRef::Integer(value) => Ok(rusqlite::types::Value::Integer(value)),
                ValueRef::Text(bytes) => {
                    let text = std::str::from_utf8(bytes).map_err(|_| {
                        rusqlite::Error::UserFunctionError("hexint: text is not UTF-8".into())
                    })?;
                    let trimmed = text.trim();
                    let parsed = if let Some(hex) = trimmed
                        .strip_prefix("0x")
                        .or_else(|| trimmed.strip_prefix("0X"))
                    {
                        i64::from_str_radix(hex, 16)
                    } else {
                        trimmed.parse::<i64>()
                    };
                    parsed.map(rusqlite::types::Value::Integer).map_err(|_| {
                        rusqlite::Error::UserFunctionError(
                            format!("hexint: cannot parse {trimmed:?} as 0x-hex or decimal").into(),
                        )
                    })
                }
                other => Err(rusqlite::Error::UserFunctionError(
                    format!("hexint: unsupported input type {}", other.data_type()).into(),
                )),
            }
        },
    )
    .context("failed to register hexint SQL function")
}

fn sqlite_value_summary(value: ValueRef<'_>, max_chars: usize) -> String {
    match value {
        ValueRef::Null => "null".to_owned(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => value.to_string(),
        ValueRef::Text(value) => {
            let value = String::from_utf8_lossy(value);
            clamp_text(&format!("{value:?}"), max_chars)
        }
        ValueRef::Blob(value) => format!("<blob:{} bytes>", value.len()),
    }
}
