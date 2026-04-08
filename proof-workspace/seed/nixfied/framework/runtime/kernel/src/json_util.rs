use serde_json::{Map, Number, Value as JsonValue};
use std::collections::BTreeMap;

pub(crate) fn parse_json(input: &str) -> Result<JsonValue, String> {
    serde_json::from_str(input).map_err(|err| format!("{}", err))
}

pub(crate) fn render_json_compact(value: &JsonValue) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

pub(crate) fn object_field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    value.as_object()?.get(key)
}

pub(crate) fn object_field_mut<'a>(
    value: &'a mut JsonValue,
    key: &str,
) -> Option<&'a mut JsonValue> {
    value.as_object_mut()?.get_mut(key)
}

pub(crate) fn object_string<'a>(value: &'a JsonValue, key: &str) -> Option<&'a str> {
    object_field(value, key)?.as_str()
}

pub(crate) fn object_bool(value: &JsonValue, key: &str) -> Option<bool> {
    object_field(value, key)?.as_bool()
}

pub(crate) fn object_array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    object_field(value, key)?.as_array().map(|v| v.as_slice())
}

pub(crate) fn required_string_field<'a>(
    value: &'a JsonValue,
    key: &str,
    label: &str,
) -> Result<&'a str, String> {
    object_string(value, key).ok_or_else(|| format!("{} missing string field {}", label, key))
}

pub(crate) fn array_strings(value: &JsonValue, key: &str) -> Vec<String> {
    object_array(value, key)
        .unwrap_or(&[])
        .iter()
        .filter_map(JsonValue::as_str)
        .map(|item| item.to_string())
        .collect()
}

pub(crate) fn object_string_map(
    value: &JsonValue,
    key: &str,
    label: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    let Some(entries) = object_field(value, key).and_then(JsonValue::as_object) else {
        return Ok(result);
    };
    for (entry_key, entry_value) in entries {
        let Some(entry_text) = entry_value.as_str() else {
            return Err(format!(
                "{} field {} must contain only string values",
                label, key
            ));
        };
        result.insert(entry_key.clone(), entry_text.to_string());
    }
    Ok(result)
}

pub(crate) fn json_value_to_plain_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Null => None,
        JsonValue::String(value) => Some(value.clone()),
        JsonValue::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
        JsonValue::Number(number) => Some(number.to_string()),
        JsonValue::Array(_) | JsonValue::Object(_) => None,
    }
}

pub(crate) fn json_value_to_i64(value: &JsonValue) -> Option<i64> {
    match value {
        JsonValue::Number(n) => n.as_i64(),
        JsonValue::String(value) => value.parse::<i64>().ok(),
        _ => None,
    }
}

pub(crate) fn json_value_to_i32(value: &JsonValue) -> Option<i32> {
    json_value_to_i64(value).and_then(|value| i32::try_from(value).ok())
}

pub(crate) fn json_value_to_number_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Number(number) => Some(number.to_string()),
        JsonValue::String(text) => {
            if let Ok(value) = text.parse::<i128>() {
                Some(value.to_string())
            } else if let Ok(value) = text.parse::<f64>() {
                if value.is_finite() {
                    Some(if value.fract() == 0.0 {
                        format!("{:.0}", value)
                    } else {
                        value.to_string()
                    })
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn nullable_string_value(value: &str) -> JsonValue {
    if value.is_empty() {
        JsonValue::Null
    } else {
        JsonValue::String(value.to_string())
    }
}

pub(crate) fn optional_i64_json_value(value: Option<i64>) -> JsonValue {
    match value {
        Some(value) => JsonValue::Number(Number::from(value)),
        None => JsonValue::Null,
    }
}

pub(crate) fn insert_optional_string_field(
    fields: &mut Map<String, JsonValue>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        fields.insert(key.to_string(), JsonValue::String(value));
    }
}

pub(crate) fn insert_optional_number_field(
    fields: &mut Map<String, JsonValue>,
    key: &str,
    value: Option<i64>,
) {
    if let Some(value) = value {
        fields.insert(key.to_string(), JsonValue::Number(Number::from(value)));
    }
}

pub(crate) fn insert_optional_bool_field(
    fields: &mut Map<String, JsonValue>,
    key: &str,
    value: Option<bool>,
) {
    if let Some(value) = value {
        fields.insert(key.to_string(), JsonValue::Bool(value));
    }
}

pub(crate) fn insert_optional_json_field(
    fields: &mut Map<String, JsonValue>,
    key: &str,
    value: Option<JsonValue>,
) {
    if let Some(value) = value {
        fields.insert(key.to_string(), value);
    }
}
