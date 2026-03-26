use crate::io_util::read_text;

pub(crate) fn parse_bool_flag(value: &str) -> Result<bool, String> {
    match value {
        "1" | "true" | "TRUE" => Ok(true),
        "0" | "false" | "FALSE" => Ok(false),
        other => Err(format!("expected boolean flag, got {}", other)),
    }
}

pub(crate) fn optional_string_value(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

pub(crate) fn parse_optional_i64(value: &str, label: &str) -> Result<Option<i64>, String> {
    if value.is_empty() || value == "null" {
        Ok(None)
    } else {
        parse_i64_text(value, label).map(Some)
    }
}

pub(crate) fn parse_optional_bool_text(value: &str, label: &str) -> Result<Option<bool>, String> {
    match value {
        "" | "null" => Ok(None),
        "1" | "true" | "TRUE" => Ok(Some(true)),
        "0" | "false" | "FALSE" => Ok(Some(false)),
        other => Err(format!("{} must be bool or empty (got '{}')", label, other)),
    }
}

pub(crate) fn next_flag_value(
    values: &[String],
    index: &mut usize,
    flag: &str,
) -> Result<String, String> {
    let value = values
        .get(*index)
        .ok_or_else(|| format!("missing value for {}", flag))?
        .clone();
    *index += 1;
    Ok(value)
}

pub(crate) fn parse_i64_text(value: &str, label: &str) -> Result<i64, String> {
    value
        .parse::<i64>()
        .map_err(|_| format!("{} must be int (got '{}')", label, value))
}

pub(crate) fn parse_i32_text(value: &str, label: &str) -> Result<i32, String> {
    value
        .parse::<i32>()
        .map_err(|_| format!("{} must be int (got '{}')", label, value))
}

pub(crate) fn parse_tab_separated_name_value_file(
    path: &str,
    label: &str,
) -> Result<Vec<(String, String)>, String> {
    let text = read_text(path)?;
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once('\t').ok_or_else(|| {
            format!(
                "{} {} must contain tab-separated name/value pairs",
                label, path
            )
        })?;
        entries.push((name.to_string(), value.to_string()));
    }
    Ok(entries)
}

pub(crate) fn strip_passthrough_separator(values: &[String]) -> &[String] {
    if values.first().map(|value| value.as_str()) == Some("--") {
        &values[1..]
    } else {
        values
    }
}
