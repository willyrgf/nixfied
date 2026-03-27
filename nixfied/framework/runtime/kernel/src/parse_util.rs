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
    parse_tab_separated_name_value_text(&text, label, path)
}

pub(crate) fn parse_tab_separated_name_value_text(
    text: &str,
    label: &str,
    source: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once('\t').ok_or_else(|| {
            format!(
                "{} {} must contain tab-separated name/value pairs",
                label, source
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tab_separated_name_value_text() {
        let entries = parse_tab_separated_name_value_text("FOO\tbar\nBAR\tbaz\n", "env", "fixture")
            .expect("entries should parse");
        assert_eq!(
            entries,
            vec![
                ("FOO".to_string(), "bar".to_string()),
                ("BAR".to_string(), "baz".to_string()),
            ]
        );
    }

    #[test]
    fn rejects_invalid_tab_separated_name_value_text() {
        let err = parse_tab_separated_name_value_text("FOO=bar\n", "env", "fixture")
            .expect_err("invalid lines should fail");
        assert_eq!(
            err,
            "env fixture must contain tab-separated name/value pairs"
        );
    }

    #[test]
    fn strips_passthrough_separator_only_when_present() {
        let with_separator = vec!["--".to_string(), "alpha".to_string(), "beta".to_string()];
        let without_separator = vec!["alpha".to_string(), "beta".to_string()];

        assert_eq!(
            strip_passthrough_separator(&with_separator),
            &["alpha".to_string(), "beta".to_string()]
        );
        assert_eq!(
            strip_passthrough_separator(&without_separator),
            &without_separator
        );
    }
}
