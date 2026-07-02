use anyhow::{Context, Result, bail};
use serde::Deserialize;
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
}
