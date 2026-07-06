use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::encoding::read_required_text;
use crate::files::display_path;
use crate::output::clamp_text;
use crate::output::{base_receipt, emit_json_checked, write_receipt_checked};

const JSON_SMALL_NODE_LIMIT: usize = 80;
const JSON_SMALL_STRING_CHAR_LIMIT: usize = 4096;

/// One `--where key=value` / `--where-contains key=text` predicate.
#[derive(Debug)]
pub(crate) struct WherePredicate {
    pub(crate) field: String,
    pub(crate) expected: String,
    pub(crate) contains: bool,
}

pub(crate) fn parse_where_predicates(
    exact: &[String],
    contains: &[String],
) -> Result<Vec<WherePredicate>> {
    let mut predicates = Vec::with_capacity(exact.len() + contains.len());
    for (values, is_contains, flag) in [
        (exact, false, "--where"),
        (contains, true, "--where-contains"),
    ] {
        for raw in values {
            let (field, expected) = raw
                .split_once('=')
                .ok_or_else(|| anyhow!("{flag} requires FIELD=VALUE, found {raw:?}"))?;
            if field.is_empty() {
                return Err(anyhow!("{flag} requires a non-empty field name: {raw:?}"));
            }
            predicates.push(WherePredicate {
                field: normalize_json_selector_arg(field),
                expected: expected.to_owned(),
                contains: is_contains,
            });
        }
    }
    Ok(predicates)
}

/// Compare a row field against a predicate. Strings compare by their
/// contents (no JSON quotes); other scalars by their JSON rendering.
fn where_predicate_matches(row: &Value, predicate: &WherePredicate) -> Result<bool> {
    let Some(value) = json_select_field(row, &predicate.field)? else {
        return Ok(false);
    };
    let rendered = match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    Ok(if predicate.contains {
        rendered.contains(&predicate.expected)
    } else {
        rendered == predicate.expected
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_json_find(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    key_contains: &[String],
    key_regex: Option<&str>,
    path_contains: &[String],
    path_regex: Option<&str>,
    value_contains: &[String],
    max: usize,
    max_value_chars: usize,
) -> Result<()> {
    if key_contains.is_empty()
        && key_regex.is_none()
        && path_contains.is_empty()
        && path_regex.is_none()
        && value_contains.is_empty()
    {
        return Err(anyhow!(
            "json-find requires --key-contains, --key-regex, --path-contains, --path-regex, or --value-contains"
        ));
    }
    let key_re = key_regex
        .map(Regex::new)
        .transpose()
        .context("invalid key regex")?;
    let path_re = path_regex
        .map(Regex::new)
        .transpose()
        .context("invalid path regex")?;
    let document = read_required_text(file)
        .with_context(|| format!("failed to read {}", file.display()))?
        .0;
    let (document, input_format) = parse_json_or_jsonl(&document)?;
    let mut rows = Vec::new();
    let mut total_matches = 0usize;
    walk_json("$", None, &document, &mut |path, key, value| {
        if let Some(key_re) = &key_re
            && !key.is_some_and(|key| key_re.is_match(key))
        {
            return;
        }
        if !key_contains.is_empty() && !key.is_some_and(|key| contains_any(key, key_contains)) {
            return;
        }
        if let Some(path_re) = &path_re
            && !path_re.is_match(path)
        {
            return;
        }
        if !path_contains.is_empty() && !contains_any(path, path_contains) {
            return;
        }
        let summary = value_summary(value, max_value_chars);
        if !value_contains.is_empty() && !contains_any(&summary, value_contains) {
            return;
        }
        total_matches += 1;
        if rows.len() < max {
            rows.push((path.to_owned(), summary));
        }
    });
    let shown = rows.len();
    let truncated = shown < total_matches;
    let cap_reason = if truncated { Some("max") } else { None };
    if cli.json {
        let mut map = base_receipt(
            "json-find",
            config.profile.as_deref(),
            "matches",
            shown,
            total_matches,
            truncated,
            cap_reason,
        );
        map.insert("path".to_string(), json!(display_path(file)));
        map.insert("input_format".to_string(), json!(input_format));
        map.insert(
            "matches".to_string(),
            json!(
                rows.iter()
                    .take(shown)
                    .map(|(path, value)| json!({
                        "path": path,
                        "value": value,
                    }))
                    .collect::<Vec<_>>()
            ),
        );
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        if rows.is_empty() {
            writeln!(stdout, "no_matches")?;
        }
        for (path, value) in rows.iter().take(shown) {
            writeln!(stdout, "{path} = {value}")?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped json matches at {max}; narrow the selector."
            )?;
        }
        write_receipt_checked(
            cli,
            base_receipt(
                "json-find",
                config.profile.as_deref(),
                "matches",
                shown,
                total_matches,
                truncated,
                cap_reason,
            ),
        )
    }
}

fn parse_json_or_jsonl(text: &str) -> Result<(Value, &'static str)> {
    match serde_json::from_str::<Value>(text) {
        Ok(value) => Ok((value, "json")),
        Err(json_error) => {
            let whole_document_error = json_error.to_string();
            let mut rows = Vec::new();
            let mut saw_line = false;
            for (index, line) in text.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                saw_line = true;
                let value: Value = serde_json::from_str(trimmed).with_context(|| {
                    format!(
                        "failed to parse JSON (whole document: {whole_document_error}); \
                             failed to parse JSONL line {}",
                        index + 1
                    )
                })?;
                rows.push(value);
            }
            if saw_line {
                Ok((Value::Array(rows), "jsonl"))
            } else {
                Err(json_error).context("failed to parse JSON")
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_json_select(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    array: Option<&str>,
    fields: &[String],
    where_exact: &[String],
    where_contains: &[String],
    keys: bool,
    max: usize,
    max_value_chars: usize,
) -> Result<()> {
    if max == 0 {
        return Err(anyhow!("json-select --max must be greater than zero"));
    }
    let array = array.map(normalize_json_selector_arg);
    let fields = expand_json_select_fields(fields);
    if keys && !fields.is_empty() {
        return Err(anyhow!(
            "json-select --keys reports row shape and cannot be combined with --field/--fields"
        ));
    }
    let predicates = parse_where_predicates(where_exact, where_contains)?;

    // Every selector that can silently produce nothing is typo-audited: a
    // field or predicate field that is null/missing in every scanned row is
    // reported instead of quietly projecting `null`.
    let mut audited_fields: Vec<String> = fields.clone();
    for predicate in &predicates {
        if !audited_fields.contains(&predicate.field) {
            audited_fields.push(predicate.field.clone());
        }
    }
    let mut field_seen_non_null = vec![false; audited_fields.len()];

    let is_jsonl_named = file
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"));
    let mut kept_rows: Vec<Value> = Vec::new();
    let mut key_stats: std::collections::BTreeMap<String, JsonKeyStat> =
        std::collections::BTreeMap::new();
    let mut non_object_rows = 0usize;
    let mut rows_scanned = 0usize;
    let mut rows_matched = 0usize;
    let input_format;
    if is_jsonl_named && array.is_none() {
        // Stream `.jsonl` row-by-row instead of materializing the whole file.
        let handle = std::fs::File::open(file)
            .with_context(|| format!("failed to open {}", file.display()))?;
        let reader = io::BufReader::new(handle);
        let stream = serde_json::Deserializer::from_reader(reader).into_iter::<Value>();
        for (index, row) in stream.enumerate() {
            let row = row.with_context(|| {
                format!(
                    "failed to parse JSONL value {} in {}",
                    index + 1,
                    file.display()
                )
            })?;
            rows_scanned += 1;
            audit_fields(&row, &audited_fields, &mut field_seen_non_null)?;
            if !row_matches_predicates(&row, &predicates)? {
                continue;
            }
            rows_matched += 1;
            if keys {
                collect_row_keys(&row, &mut key_stats, &mut non_object_rows);
            } else if kept_rows.len() < max {
                kept_rows.push(row);
            }
        }
        input_format = "jsonl";
    } else {
        let text = read_required_text(file)
            .with_context(|| format!("failed to read {}", file.display()))?
            .0;
        let (document, parsed_format) = parse_json_or_jsonl(&text)?;
        input_format = parsed_format;
        let rows: Vec<&Value> = if let Some(pointer) = array.as_deref() {
            let selected = json_select_field(&document, pointer)?
                .ok_or_else(|| anyhow!("json-select --array selector did not match: {pointer}"))?;
            selected
                .as_array()
                .ok_or_else(|| {
                    anyhow!("json-select --array selector must resolve to an array: {pointer}")
                })?
                .iter()
                .collect()
        } else if input_format == "jsonl" {
            document
                .as_array()
                .expect("JSONL parser returns an array")
                .iter()
                .collect()
        } else {
            vec![&document]
        };
        for row in rows {
            rows_scanned += 1;
            audit_fields(row, &audited_fields, &mut field_seen_non_null)?;
            if !row_matches_predicates(row, &predicates)? {
                continue;
            }
            rows_matched += 1;
            if keys {
                collect_row_keys(row, &mut key_stats, &mut non_object_rows);
            } else if kept_rows.len() < max {
                kept_rows.push(row.clone());
            }
        }
    }

    let all_null_fields = audited_fields
        .iter()
        .zip(&field_seen_non_null)
        .filter_map(|(field, seen)| (rows_scanned > 0 && !seen).then_some(field.clone()))
        .collect::<Vec<_>>();

    if keys {
        return render_json_select_keys(
            cli,
            config,
            file,
            array.as_deref(),
            input_format,
            &key_stats,
            non_object_rows,
            rows_scanned,
            rows_matched,
            &all_null_fields,
            max,
        );
    }

    let shown = kept_rows.len();
    let truncated = shown < rows_matched;
    let cap_reason = if truncated { Some("max") } else { None };
    let where_labels = predicates
        .iter()
        .map(|predicate| {
            format!(
                "{}{}{}",
                predicate.field,
                if predicate.contains { "~=" } else { "=" },
                predicate.expected
            )
        })
        .collect::<Vec<_>>();
    let mut map = base_receipt(
        "json-select",
        config.profile.as_deref(),
        "rows",
        shown,
        rows_matched,
        truncated,
        cap_reason,
    );
    map.insert("path".to_string(), json!(display_path(file)));
    map.insert("array".to_string(), json!(array.as_deref()));
    map.insert("input_format".to_string(), json!(input_format));
    map.insert("fields".to_string(), json!(fields));
    map.insert("where".to_string(), json!(where_labels));
    map.insert("rows_scanned".to_string(), json!(rows_scanned));
    map.insert("all_null_fields".to_string(), json!(all_null_fields));
    if cli.json {
        map.insert(
            "rows".to_string(),
            json!(
                kept_rows
                    .iter()
                    .enumerate()
                    .map(|(index, row)| json_select_row(index, row, &fields, max_value_chars))
                    .collect::<Result<Vec<_>>>()?
            ),
        );
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        let source = array.as_deref().unwrap_or(if input_format == "jsonl" {
            "jsonl"
        } else {
            "$"
        });
        let mut header = format!("[contextmink] json-select source={source}");
        if !fields.is_empty() {
            header.push_str(&format!(" fields={}", fields.join(",")));
        }
        if !where_labels.is_empty() {
            header.push_str(&format!(" where={}", where_labels.join(",")));
        }
        writeln!(stdout, "{header}")?;
        if kept_rows.is_empty() {
            writeln!(stdout, "no_rows")?;
        }
        for (index, row) in kept_rows.iter().enumerate() {
            if fields.is_empty() {
                writeln!(stdout, "{index}: {}", value_summary(row, max_value_chars))?;
                continue;
            }
            let mut parts = Vec::with_capacity(fields.len());
            for field in &fields {
                let summary = json_select_field(row, field.as_str())?
                    .map(|value| value_summary(value, max_value_chars))
                    .unwrap_or_else(|| "null".to_owned());
                parts.push(format!("{field}={summary}"));
            }
            writeln!(stdout, "{index}: {}", parts.join(" "))?;
        }
        if !all_null_fields.is_empty() {
            writeln!(
                stdout,
                "[contextmink] warning: field(s) {} were null or missing in all {rows_scanned} scanned row(s); check the field selector against the document shape.",
                all_null_fields
                    .iter()
                    .map(|field| field.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped json rows at {max}; narrow the selector."
            )?;
        }
        write_receipt_checked(cli, map)
    }
}

#[derive(Debug, Default)]
struct JsonKeyStat {
    present: usize,
    non_null: usize,
    types: std::collections::BTreeSet<&'static str>,
}

fn collect_row_keys(
    row: &Value,
    key_stats: &mut std::collections::BTreeMap<String, JsonKeyStat>,
    non_object_rows: &mut usize,
) {
    let Some(object) = row.as_object() else {
        *non_object_rows += 1;
        return;
    };
    for (key, value) in object {
        let stat = key_stats.entry(key.clone()).or_default();
        stat.present += 1;
        if !value.is_null() {
            stat.non_null += 1;
        }
        stat.types.insert(json_value_type_name(value));
    }
}

fn json_value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `--keys` output: the union of top-level row keys with presence counts and
/// value types, so an unknown row shape is discoverable in one call instead
/// of a guess → all-null warning → slice → retry loop.
#[allow(clippy::too_many_arguments)]
fn render_json_select_keys(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    array: Option<&str>,
    input_format: &str,
    key_stats: &std::collections::BTreeMap<String, JsonKeyStat>,
    non_object_rows: usize,
    rows_scanned: usize,
    rows_matched: usize,
    all_null_fields: &[String],
    max: usize,
) -> Result<()> {
    let total = key_stats.len();
    let shown = total.min(max);
    let truncated = shown < total;
    let cap_reason = if truncated { Some("max") } else { None };
    let mut map = base_receipt(
        "json-select",
        config.profile.as_deref(),
        "keys",
        shown,
        total,
        truncated,
        cap_reason,
    );
    map.insert("path".to_string(), json!(display_path(file)));
    map.insert("array".to_string(), json!(array));
    map.insert("input_format".to_string(), json!(input_format));
    map.insert("keys_mode".to_string(), json!(true));
    map.insert("rows_scanned".to_string(), json!(rows_scanned));
    map.insert("rows_matched".to_string(), json!(rows_matched));
    map.insert("non_object_rows".to_string(), json!(non_object_rows));
    map.insert("all_null_fields".to_string(), json!(all_null_fields));
    let key_rows = key_stats
        .iter()
        .take(shown)
        .map(|(key, stat)| {
            json!({
                "key": key,
                "present": stat.present,
                "non_null": stat.non_null,
                "types": stat.types.iter().collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    if cli.json {
        map.insert("keys".to_string(), json!(key_rows));
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        writeln!(
            stdout,
            "[contextmink] json-select keys source={} rows={rows_matched}",
            array.unwrap_or(if input_format == "jsonl" {
                "jsonl"
            } else {
                "$"
            })
        )?;
        if key_rows.is_empty() {
            writeln!(stdout, "no_keys")?;
        }
        for (index, row) in key_rows.iter().enumerate() {
            writeln!(
                stdout,
                "{index}: {} present={} non_null={} types={}",
                row["key"].as_str().unwrap_or_default(),
                row["present"],
                row["non_null"],
                row["types"]
                    .as_array()
                    .map(|types| {
                        types
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("|")
                    })
                    .unwrap_or_default()
            )?;
        }
        if non_object_rows > 0 {
            writeln!(
                stdout,
                "[contextmink] {non_object_rows} scanned row(s) were not JSON objects and carry no keys."
            )?;
        }
        if !all_null_fields.is_empty() {
            writeln!(
                stdout,
                "[contextmink] warning: field(s) {} were null or missing in all {rows_scanned} scanned row(s); check the field selector against the document shape.",
                all_null_fields.join(", ")
            )?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped keys at {max}; raise --max or filter rows."
            )?;
        }
        write_receipt_checked(cli, map)
    }
}

fn expand_json_select_fields(fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .flat_map(|field| field.split(','))
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(normalize_json_selector_arg)
        .collect()
}

fn audit_fields(
    row: &Value,
    audited_fields: &[String],
    field_seen_non_null: &mut [bool],
) -> Result<()> {
    for (field, seen) in audited_fields.iter().zip(field_seen_non_null.iter_mut()) {
        if !*seen && json_select_field(row, field)?.is_some_and(|value| !value.is_null()) {
            *seen = true;
        }
    }
    Ok(())
}

fn row_matches_predicates(row: &Value, predicates: &[WherePredicate]) -> Result<bool> {
    for predicate in predicates {
        if !where_predicate_matches(row, predicate)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn normalize_json_selector_arg(selector: &str) -> String {
    msys_git_root()
        .and_then(|git_root| normalize_msys_converted_json_selector(selector, &git_root))
        .or_else(|| normalize_msys_drive_git_selector(selector))
        .unwrap_or_else(|| selector.to_owned())
}

fn msys_git_root() -> Option<String> {
    let exe_path = std::env::var_os("EXEPATH")?;
    let exe_path = exe_path.to_string_lossy().replace('\\', "/");
    let exe_path = exe_path.trim_end_matches('/');
    Some(exe_path.strip_suffix("/bin").unwrap_or(exe_path).to_owned())
}

fn normalize_msys_converted_json_selector(selector: &str, git_root: &str) -> Option<String> {
    if selector == "$" || selector.is_empty() || selector.starts_with('/') {
        return None;
    }
    let normalized_selector = selector.replace('\\', "/");
    let normalized_root = git_root.replace('\\', "/");
    let rest = normalized_selector.strip_prefix(normalized_root.trim_end_matches('/'))?;
    if rest.starts_with('/') && rest.len() > 1 {
        Some(rest.to_owned())
    } else {
        None
    }
}

fn normalize_msys_drive_git_selector(selector: &str) -> Option<String> {
    if selector == "$" || selector.is_empty() || selector.starts_with('/') {
        return None;
    }
    let normalized = selector.replace('\\', "/");
    let git_marker = normalized.rfind("/Git/")?;
    let rest = &normalized[git_marker + "/Git".len()..];
    if rest.starts_with('/') && rest.len() > 1 {
        Some(rest.to_owned())
    } else {
        None
    }
}

fn json_select_row(
    index: usize,
    row: &Value,
    fields: &[String],
    max_value_chars: usize,
) -> Result<Value> {
    if fields.is_empty() {
        return Ok(json!({
            "row": index,
            "value": value_summary(row, max_value_chars),
        }));
    }
    let mut output_fields = serde_json::Map::new();
    for field in fields {
        let summary = json_select_field(row, field.as_str())?
            .map(|value| value_summary(value, max_value_chars))
            .unwrap_or_else(|| "null".to_owned());
        output_fields.insert(field.clone(), json!(summary));
    }
    Ok(json!({
        "row": index,
        "fields": output_fields,
    }))
}

fn json_select_field<'a>(row: &'a Value, selector: &str) -> Result<Option<&'a Value>> {
    if selector == "$" || selector.starts_with('/') || selector.is_empty() {
        return json_pointer_lookup(row, selector);
    }
    Ok(row.as_object().and_then(|map| map.get(selector)))
}

fn json_pointer_lookup<'a>(value: &'a Value, pointer: &str) -> Result<Option<&'a Value>> {
    if pointer.is_empty() || pointer == "$" {
        return Ok(Some(value));
    }
    if !pointer.starts_with('/') {
        return Err(anyhow!(
            "JSON pointer must be empty, $, or start with /: {pointer}"
        ));
    }
    let mut current = value;
    for raw_token in pointer[1..].split('/') {
        let token = decode_json_pointer_token(raw_token)?;
        match current {
            Value::Object(map) => {
                let Some(next) = map.get(&token) else {
                    return Ok(None);
                };
                current = next;
            }
            Value::Array(values) => {
                let index = token
                    .parse::<usize>()
                    .with_context(|| format!("invalid JSON array index in pointer: {token}"))?;
                let Some(next) = values.get(index) else {
                    return Ok(None);
                };
                current = next;
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(current))
}

fn decode_json_pointer_token(token: &str) -> Result<String> {
    let mut output = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch != '~' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('0') => output.push('~'),
            Some('1') => output.push('/'),
            Some(other) => {
                return Err(anyhow!(
                    "invalid JSON pointer escape: ~{other}; expected ~0 or ~1"
                ));
            }
            None => {
                return Err(anyhow!(
                    "invalid JSON pointer escape at end of token; expected ~0 or ~1"
                ));
            }
        }
    }
    Ok(output)
}

fn walk_json<'a>(
    path: &str,
    key: Option<&'a str>,
    value: &'a Value,
    visit: &mut impl FnMut(&str, Option<&'a str>, &'a Value),
) {
    visit(path, key, value);
    match value {
        Value::Object(map) => {
            for (child_key, child) in map {
                let child_path = if is_json_identifier(child_key) {
                    format!("{path}.{child_key}")
                } else {
                    format!(
                        "{path}[{}]",
                        serde_json::to_string(child_key).unwrap_or_default()
                    )
                };
                walk_json(&child_path, Some(child_key.as_str()), child, visit);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                walk_json(&child_path, None, child, visit);
            }
        }
        _ => {}
    }
}

pub(crate) fn contains_any(value: &str, needles: &[String]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn value_summary(value: &Value, max_chars: usize) -> String {
    match value {
        Value::String(value) => clamp_text(&format!("{value:?}"), max_chars),
        Value::Null | Value::Bool(_) | Value::Number(_) => value.to_string(),
        Value::Array(values) => {
            if is_small_json(value) {
                clamp_text(
                    &serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_owned()),
                    max_chars,
                )
            } else {
                format!("<array:{} items>", values.len())
            }
        }
        Value::Object(map) => {
            if is_small_json(value) {
                clamp_text(
                    &serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_owned()),
                    max_chars,
                )
            } else {
                let sample_keys = map.keys().take(5).cloned().collect::<Vec<_>>();
                format!(
                    "<object:{} keys sample={}>",
                    map.len(),
                    serde_json::to_string(&sample_keys).unwrap_or_else(|_| "[]".to_owned())
                )
            }
        }
    }
}

fn is_small_json(value: &Value) -> bool {
    let mut nodes = 0usize;
    let mut string_chars = 0usize;
    json_fits_budget(value, &mut nodes, &mut string_chars)
}

fn json_fits_budget(value: &Value, nodes: &mut usize, string_chars: &mut usize) -> bool {
    *nodes += 1;
    if *nodes > JSON_SMALL_NODE_LIMIT {
        return false;
    }
    match value {
        Value::String(value) => {
            *string_chars += value.chars().count();
            *string_chars <= JSON_SMALL_STRING_CHAR_LIMIT
        }
        Value::Array(values) => values
            .iter()
            .all(|value| json_fits_budget(value, nodes, string_chars)),
        Value::Object(map) => map
            .values()
            .all(|value| json_fits_budget(value, nodes, string_chars)),
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
    }
}

fn is_json_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests;
