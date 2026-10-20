//! Stable envelopes for clients; domain-specific record fields remain extensible.
use super::*;

pub(super) fn schema(name: &str) -> serde_json::Map<String, Value> {
    let (required, fields) = match name {
        "query_catalog" => (
            vec![
                "status",
                "action",
                "results",
                "candidates",
                "matched_count",
                "truncated",
            ],
            json!({
                "status": {"enum": ["resolved", "ambiguous", "not_found"]}, "action": {"type": "string"},
                "results": {"type": "array", "items": {"type": "object"}}, "candidates": {"type": "array", "items": {"type": "object"}},
                "matched_count": {"type": "integer", "minimum": 0}, "truncated": {"type": "boolean"}
            }),
        ),
        "get_entity" => (
            vec!["key", "kind", "name"],
            json!({"key": {"type": "string"}, "kind": {"type": "string"}, "name": {"type": "string"}}),
        ),
        "calculate_recipe" => (
            vec![
                "recipe_id",
                "materials",
                "requested_quantity",
                "complete",
                "total",
            ],
            json!({
                "recipe_id": {"type": "integer"}, "materials": {"type": "array", "items": {"type": "object"}},
                "requested_quantity": {"type": "integer", "minimum": 1}, "complete": {"type": "boolean"}, "total": {"type": ["integer", "null"]}
            }),
        ),
        "catalog_info" => (
            vec!["read_only", "counts", "categories", "filters", "relations"],
            json!({
                "read_only": {"const": true}, "counts": {"type": "object"}, "categories": {"type": "array", "items": {"type": "string"}},
                "filters": {"type": "object"}, "relations": {"type": "object"}
            }),
        ),
        _ => (
            vec!["results", "total", "offset", "next_offset", "next_call"],
            json!({
                "results": {"type": "array", "items": {"type": "object"}}, "total": {"type": "integer", "minimum": 0},
                "offset": {"type": "integer", "minimum": 0}, "next_offset": {"type": ["integer", "null"]}, "next_call": {"type": ["object", "null"]}
            }),
        ),
    };
    let mut properties = fields.as_object().unwrap().clone();
    properties.insert("catalog_version".into(), json!({"type": "string"}));
    let mut required: Vec<_> = required.into_iter().map(Value::from).collect();
    required.push(json!("catalog_version"));
    json!({"type": "object", "oneOf": [
        {"type": "object", "required": required, "properties": properties, "not": {"required": ["error"]}},
        {"type": "object", "required": ["error", "error_code"], "properties": {"error": {"type": "string"}, "error_code": {"type": "string"}}}
    ]}).as_object().unwrap().clone()
}

pub(super) fn error(message: String) -> Value {
    let folded = message.to_lowercase();
    let code = if folded.contains("not found") || folded.contains("找不到") {
        "NOT_FOUND"
    } else if folded.contains("unsupported")
        || folded.contains("does not record")
        || folded.contains("not supported")
    {
        "UNSUPPORTED_QUERY"
    } else if folded.contains("invalid")
        || folded.contains("must")
        || folded.contains("require")
        || folded.starts_with("use ")
        || folded.starts_with("provide ")
        || folded.contains("does not match")
        || folded.contains("does not produce")
        || folded.contains("sources apply")
        || folded.starts_with("unknown ")
        || folded.contains("sections apply")
    {
        "INVALID_ARGUMENTS"
    } else {
        "QUERY_FAILED"
    };
    json!({"error": message, "error_code": code})
}
