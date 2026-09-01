use serde_json::{Value as JsonValue};

/// Parse a JSON string into a formatted value
pub fn json_parse(input: &str) -> anyhow::Result<JsonValue> {
    serde_json::from_str(input).map_err(|e| anyhow::anyhow!("{}", e))
}

/// Serialize a value to a JSON string
pub fn json_stringify(value: &JsonValue) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// Extract a field from a JSON string
pub fn json_get(input: &str, key: &str) -> anyhow::Result<String> {
    let value: JsonValue = serde_json::from_str(input)?;
    match &value {
        JsonValue::Object(map) => {
            map.get(key)
                .map(|v| v.to_string())
                .ok_or_else(|| anyhow::anyhow!("Key '{}' not found", key))
        }
        JsonValue::Array(arr) => {
            key.parse::<usize>()
                .map(|i| arr.get(i).map(|v| v.to_string()).unwrap_or_default())
                .map_err(|_| anyhow::anyhow!("Invalid array index"))
        }
        _ => Err(anyhow::anyhow!("Not an object or array")),
    }
}

/// Extract a nested field using dot notation (e.g., "data.users.0.name")
pub fn json_path(input: &str, path: &str) -> anyhow::Result<String> {
    let value: JsonValue = serde_json::from_str(input)?;
    let mut current = &value;
    for key in path.split('.') {
        match current {
            JsonValue::Object(map) => {
                current = map.get(key).ok_or_else(|| anyhow::anyhow!("Key '{}' not found", key))?;
            }
            JsonValue::Array(arr) => {
                let idx: usize = key.parse().map_err(|_| anyhow::anyhow!("Invalid array index: {}", key))?;
                current = arr.get(idx).ok_or_else(|| anyhow::anyhow!("Index {} out of bounds", idx))?;
            }
            _ => return Err(anyhow::anyhow!("Cannot navigate into non-object/non-array")),
        }
    }
    Ok(current.to_string())
}

/// Get all keys from a JSON object
pub fn json_keys(input: &str) -> anyhow::Result<Vec<String>> {
    let value: JsonValue = serde_json::from_str(input)?;
    match &value {
        JsonValue::Object(map) => Ok(map.keys().cloned().collect()),
        _ => Err(anyhow::anyhow!("Not a JSON object")),
    }
}

/// Count elements in a JSON array
pub fn json_len(input: &str) -> anyhow::Result<usize> {
    let value: JsonValue = serde_json::from_str(input)?;
    match &value {
        JsonValue::Array(arr) => Ok(arr.len()),
        JsonValue::Object(map) => Ok(map.len()),
        JsonValue::String(s) => Ok(s.len()),
        _ => Err(anyhow::anyhow!("Not countable")),
    }
}

/// Extract all values matching a key from a JSON structure (recursive)
pub fn json_find_all(input: &str, key: &str) -> Vec<String> {
    let value: JsonValue = serde_json::from_str(input).unwrap_or(JsonValue::Null);
    let mut results = vec![];
    find_key_recursive(&value, key, &mut results);
    results
}

fn find_key_recursive(value: &JsonValue, key: &str, results: &mut Vec<String>) {
    match value {
        JsonValue::Object(map) => {
            for (k, v) in map {
                if k == key {
                    results.push(v.to_string());
                }
                find_key_recursive(v, key, results);
            }
        }
        JsonValue::Array(arr) => {
            for item in arr {
                find_key_recursive(item, key, results);
            }
        }
        _ => {}
    }
}

/// Create a JSON object from key-value pairs
pub fn json_make_object(pairs: Vec<(String, String)>) -> String {
    let mut map = serde_json::Map::new();
    for (k, v) in pairs {
        // Try to parse the value as JSON, otherwise treat as string
        let json_val: JsonValue = serde_json::from_str(&v).unwrap_or(JsonValue::String(v));
        map.insert(k, json_val);
    }
    serde_json::to_string(&JsonValue::Object(map)).unwrap_or_default()
}