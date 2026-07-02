use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::MediaWikiClient;

#[derive(Debug, Deserialize, Default)]
struct CargoQueryResponse {
    #[serde(default)]
    cargoquery: Vec<CargoCountRow>,
}

#[derive(Debug, Deserialize, Default)]
struct CargoCountRow {
    #[serde(default)]
    title: CargoCountTitle,
}

#[derive(Debug, Deserialize, Default)]
struct CargoCountTitle {
    n: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
struct CargoTablesResponse {
    #[serde(default)]
    cargotables: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CargoFieldsResponse {
    #[serde(default)]
    cargofields: BTreeMap<String, CargoFieldPayload>,
}

#[derive(Debug, Deserialize, Default)]
struct CargoFieldPayload {
    #[serde(rename = "type")]
    field_type: Option<String>,
    #[serde(rename = "isList")]
    is_list: Option<Value>,
    delimiter: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CargoField {
    pub name: String,
    pub field_type: String,
    pub is_list: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CargoRowsOptions {
    pub table: String,
    /// Fields to select; when empty the caller should populate from the table schema.
    pub fields: Vec<String>,
    pub where_clause: Option<String>,
    pub order_by: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Deserialize, Default)]
struct CargoRowsResponse {
    #[serde(default)]
    cargoquery: Vec<CargoRowPayload>,
}

#[derive(Debug, Deserialize, Default)]
struct CargoRowPayload {
    #[serde(default)]
    title: BTreeMap<String, Value>,
}

pub fn cargo_count_rows(client: &mut MediaWikiClient, table: &str) -> Result<u64> {
    let response = client.request_json_get(&[
        ("action", "cargoquery".to_string()),
        ("tables", table.to_string()),
        ("fields", "COUNT(*)=n".to_string()),
        ("limit", "1".to_string()),
    ])?;
    decode_cargo_count_response(table, response)
}

fn decode_cargo_count_response(table: &str, response: Value) -> Result<u64> {
    let parsed: CargoQueryResponse =
        serde_json::from_value(response).context("failed to decode Cargo query count response")?;
    let value = parsed
        .cargoquery
        .first()
        .and_then(|row| row.title.n.as_ref())
        .ok_or_else(|| anyhow::anyhow!("Cargo query count for {table} returned no count row"))?;
    parse_count_value(value).with_context(|| format!("parse Cargo query count for {table}"))
}

pub fn cargo_list_tables(client: &mut MediaWikiClient) -> Result<Vec<String>> {
    let response = client.request_json_get(&[("action", "cargotables".to_string())])?;
    decode_cargo_tables_response(response)
}

fn decode_cargo_tables_response(response: Value) -> Result<Vec<String>> {
    let parsed: CargoTablesResponse =
        serde_json::from_value(response).context("failed to decode Cargo tables response")?;
    let mut tables = parsed.cargotables;
    tables.sort();
    Ok(tables)
}

pub fn cargo_table_fields(client: &mut MediaWikiClient, table: &str) -> Result<Vec<CargoField>> {
    let response = client.request_json_get(&[
        ("action", "cargofields".to_string()),
        ("table", table.to_string()),
    ])?;
    decode_cargo_fields_response(table, response)
}

fn decode_cargo_fields_response(table: &str, response: Value) -> Result<Vec<CargoField>> {
    let parsed: CargoFieldsResponse = serde_json::from_value(response)
        .with_context(|| format!("failed to decode Cargo fields response for {table}"))?;
    if parsed.cargofields.is_empty() {
        bail!("Cargo table {table} reported no fields; the table may not exist");
    }
    Ok(parsed
        .cargofields
        .into_iter()
        .map(|(name, payload)| CargoField {
            name,
            field_type: payload.field_type.unwrap_or_else(|| "String".to_string()),
            // The API marks list fields with an empty-string marker value.
            is_list: payload.is_list.is_some(),
            delimiter: payload.delimiter,
        })
        .collect())
}

pub fn cargo_query_rows(
    client: &mut MediaWikiClient,
    options: &CargoRowsOptions,
) -> Result<Vec<BTreeMap<String, Value>>> {
    if options.fields.is_empty() {
        bail!("cargo_query_rows requires at least one field");
    }
    // Alias every field to itself: without an explicit alias Cargo title-cases
    // response keys (underscores become spaces), which would make row keys
    // diverge from the schema field names.
    let fields = options
        .fields
        .iter()
        .map(|field| format!("{field}={field}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut params = vec![
        ("action", "cargoquery".to_string()),
        ("tables", options.table.clone()),
        ("fields", fields),
        ("limit", options.limit.max(1).to_string()),
    ];
    if options.offset > 0 {
        params.push(("offset", options.offset.to_string()));
    }
    if let Some(where_clause) = &options.where_clause {
        params.push(("where", where_clause.clone()));
    }
    if let Some(order_by) = &options.order_by {
        params.push(("order_by", order_by.clone()));
    }
    let response = client.request_json_get(&params)?;
    decode_cargo_rows_response(&options.table, response)
}

fn decode_cargo_rows_response(
    table: &str,
    response: Value,
) -> Result<Vec<BTreeMap<String, Value>>> {
    let parsed: CargoRowsResponse = serde_json::from_value(response)
        .with_context(|| format!("failed to decode Cargo rows response for {table}"))?;
    Ok(parsed.cargoquery.into_iter().map(|row| row.title).collect())
}

fn parse_count_value(value: &Value) -> Result<u64> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("count is not an unsigned integer: {number}")),
        Value::String(text) => text
            .parse::<u64>()
            .with_context(|| format!("count is not an unsigned integer: {text}")),
        _ => bail!("count has unsupported JSON shape: {value}"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn decodes_cargo_count_string() {
        let response = json!({
            "cargoquery": [
                { "title": { "n": "520" } }
            ]
        });

        assert_eq!(
            decode_cargo_count_response("TraitMetadata", response).unwrap(),
            520
        );
    }

    #[test]
    fn decodes_cargo_count_number() {
        let response = json!({
            "cargoquery": [
                { "title": { "n": 0 } }
            ]
        });

        assert_eq!(decode_cargo_count_response("Tokens", response).unwrap(), 0);
    }

    #[test]
    fn rejects_missing_cargo_count() {
        let response = json!({ "cargoquery": [] });

        let error = decode_cargo_count_response("Traits", response).unwrap_err();
        assert!(error.to_string().contains("returned no count row"));
    }

    #[test]
    fn decodes_cargo_tables_sorted() {
        let response = json!({ "cargotables": ["Traits", "Tokens", "TraitMetadata"] });
        assert_eq!(
            decode_cargo_tables_response(response).unwrap(),
            vec!["Tokens", "TraitMetadata", "Traits"]
        );
    }

    #[test]
    fn decodes_cargo_fields_with_list_markers() {
        let response = json!({
            "cargofields": {
                "trait_name": { "type": "String" },
                "layer_order": { "type": "Integer" },
                "variants": { "type": "String", "isList": "", "delimiter": ";" }
            }
        });
        let fields = decode_cargo_fields_response("Traits", response).unwrap();
        assert_eq!(fields.len(), 3);
        let variants = fields
            .iter()
            .find(|field| field.name == "variants")
            .expect("variants field");
        assert!(variants.is_list);
        assert_eq!(variants.delimiter.as_deref(), Some(";"));
        let layer = fields
            .iter()
            .find(|field| field.name == "layer_order")
            .expect("layer field");
        assert_eq!(layer.field_type, "Integer");
        assert!(!layer.is_list);
    }

    #[test]
    fn rejects_cargo_fields_for_unknown_table() {
        let response = json!({ "cargofields": {} });
        let error = decode_cargo_fields_response("Nope", response).unwrap_err();
        assert!(error.to_string().contains("reported no fields"));
    }

    #[test]
    fn decodes_cargo_rows_preserving_field_values() {
        let response = json!({
            "cargoquery": [
                { "title": { "trait_name": "Bowl Cut", "token_count": "512" } },
                { "title": { "trait_name": "Halo", "token_count": "77" } }
            ]
        });
        let rows = decode_cargo_rows_response("Traits", response).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].get("trait_name").and_then(|value| value.as_str()),
            Some("Bowl Cut")
        );
    }
}
