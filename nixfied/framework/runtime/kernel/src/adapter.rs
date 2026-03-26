use super::*;
use crate::validation::resolve_json_path;

pub(crate) fn adapter_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "decode" => adapter_decode_command(values),
        other => Err(format!("unknown adapter subcommand: {}", other)),
    }
}

fn adapter_decode_command(values: &[String]) -> Result<(), String> {
    let kind = values
        .first()
        .ok_or_else(|| "usage: nixfied-kernel adapter decode <kind> ...".to_string())?;
    match kind.as_str() {
        "supervisor-status" => adapter_decode_supervisor_status(&values[1..]),
        "helios-finalized-slot" => adapter_decode_helios_finalized_slot(&values[1..]),
        "helios-checkpoint-root" => adapter_decode_helios_checkpoint_root(&values[1..]),
        other => Err(format!("unknown adapter decode kind: {}", other)),
    }
}

fn adapter_decode_supervisor_status(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err(
            "usage: nixfied-kernel adapter decode supervisor-status <json-file>".to_string(),
        );
    }
    let payload = parse_json_file(&values[0], "supervisor payload")?;
    let rows = payload
        .as_array()
        .ok_or_else(|| "supervisor payload must be an array".to_string())?;
    for row in rows {
        let object = row
            .as_object()
            .ok_or_else(|| "supervisor process row must be an object".to_string())?;
        println!(
            "{}\t{}\t{}\t{}",
            object.get("name").and_then(JsonValue::as_str).unwrap_or(""),
            object
                .get("status")
                .and_then(JsonValue::as_str)
                .unwrap_or(""),
            object
                .get("is_running")
                .and_then(JsonValue::as_bool)
                .map(|value| value.to_string())
                .unwrap_or_default(),
            object
                .get("is_ready")
                .and_then(JsonValue::as_bool)
                .map(|value| value.to_string())
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn adapter_decode_helios_finalized_slot(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err(
            "usage: nixfied-kernel adapter decode helios-finalized-slot <json-file>".to_string(),
        );
    }
    let payload = parse_json_file(&values[0], "helios finalized payload")?;
    let selected = resolve_json_path(&payload, ".data.header.message.slot")
        .ok_or_else(|| "helios finalized slot is missing".to_string())?;
    let number = json_value_to_number_string(selected)
        .ok_or_else(|| "helios finalized slot is not numeric".to_string())?;
    println!("{}", number);
    Ok(())
}

fn adapter_decode_helios_checkpoint_root(values: &[String]) -> Result<(), String> {
    if values.is_empty() || values.len() > 2 {
        return Err(
            "usage: nixfied-kernel adapter decode helios-checkpoint-root <json-file> [--empty-ok]"
                .to_string(),
        );
    }
    let empty_ok = values
        .get(1)
        .map(|value| value == "--empty-ok")
        .unwrap_or(false);
    let payload = parse_json_file(&values[0], "helios checkpoint payload")?;
    let selected = resolve_json_path(&payload, ".data.root");
    match selected.and_then(JsonValue::as_str) {
        Some(value) => {
            println!("{}", value);
            Ok(())
        }
        None if empty_ok => Ok(()),
        None => Err("helios checkpoint root is missing".to_string()),
    }
}
