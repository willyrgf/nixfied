use super::*;

pub(crate) fn run_record_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "create" => run_record_create_command(values),
        "read" => run_record_read_command(values),
        "transition" => run_record_transition_command(values),
        other => Err(format!("unknown run-record subcommand: {}", other)),
    }
}

fn run_record_create_command(values: &[String]) -> Result<(), String> {
    if values.len() < 10 {
        return Err(
            "usage: nixfied-kernel run-record create <bundle-file> <run-file> <run-id> <attempt-id> <command> <workflow-id> <task-id> <execution-mode> <process-mode> <ephemeral-enabled> [-- <args...>]"
                .to_string(),
        );
    }

    let bundle_path = &values[0];
    let run_file = &values[1];
    let now = current_utc_timestamp()?;
    let args = JsonValue::Array(
        strip_passthrough_separator(&values[10..])
            .iter()
            .map(|value| JsonValue::String(value.clone()))
            .collect(),
    );

    let history = vec![run_record_history_entry("queued", &now)];
    let workflow_id = nullable_string_value(&values[5]);
    let task_id = nullable_string_value(&values[6]);
    let ephemeral_enabled = parse_bool_flag(&values[9])?;
    let payload = json!({
        "run_id": values[2],
        "attempt_id": values[3],
        "command": values[4],
        "workflow_id": workflow_id,
        "task_id": task_id,
        "execution_mode": values[7],
        "process_mode": values[8],
        "ephemeral_enabled": ephemeral_enabled,
        "state": "queued",
        "pid": null,
        "pgid": null,
        "exit_code": null,
        "stop_reason": null,
        "created_at": now,
        "started_at": null,
        "finished_at": null,
        "updated_at": now,
        "args": args,
        "history": history,
    });
    let envelope = json!({
        "kind": "run-record",
        "version": 1,
        "payload": payload,
    });

    validate_and_write_json(bundle_path, "runtime.runRecord", run_file, &envelope)?;
    println!("OK: run-record create");
    Ok(())
}

fn run_record_transition_command(values: &[String]) -> Result<(), String> {
    if values.len() != 7 {
        return Err(
            "usage: nixfied-kernel run-record transition <bundle-file> <run-file> <state> <exit-code|empty> <stop-reason|empty> <pid|empty> <pgid|empty>"
                .to_string(),
        );
    }

    let bundle_path = &values[0];
    let run_file = &values[1];
    let state = &values[2];
    let exit_code = parse_optional_i64(&values[3], "run-record exit_code")?;
    let stop_reason = optional_string_value(&values[4]);
    let pid = parse_optional_i64(&values[5], "run-record pid")?;
    let pgid = parse_optional_i64(&values[6], "run-record pgid")?;
    let now = current_utc_timestamp()?;

    let mut envelope = parse_json_file(run_file, "run-record file")?;
    let payload = object_field_mut(&mut envelope, "payload")
        .ok_or_else(|| "run-record payload is missing".to_string())?;
    let payload_object = payload
        .as_object_mut()
        .ok_or_else(|| "run-record payload must be an object".to_string())?;

    payload_object.insert("state".to_string(), JsonValue::String(state.clone()));
    payload_object.insert("updated_at".to_string(), JsonValue::String(now.clone()));
    if let Some(pid) = pid {
        payload_object.insert("pid".to_string(), JsonValue::Number(Number::from(pid)));
    }
    if let Some(pgid) = pgid {
        payload_object.insert("pgid".to_string(), JsonValue::Number(Number::from(pgid)));
    }
    if payload_object
        .get("started_at")
        .map(|value| matches!(value, JsonValue::Null))
        .unwrap_or(true)
        && state == "running"
    {
        payload_object.insert("started_at".to_string(), JsonValue::String(now.clone()));
    }
    if matches!(state.as_str(), "passed" | "failed" | "canceled") {
        payload_object.insert("finished_at".to_string(), JsonValue::String(now.clone()));
    }
    if let Some(exit_code) = exit_code {
        payload_object.insert(
            "exit_code".to_string(),
            JsonValue::Number(Number::from(exit_code)),
        );
    }
    if let Some(stop_reason) = stop_reason {
        payload_object.insert("stop_reason".to_string(), JsonValue::String(stop_reason));
    }

    let history_value = payload_object
        .get_mut("history")
        .ok_or_else(|| "run-record history is missing".to_string())?;
    let history = history_value
        .as_array_mut()
        .ok_or_else(|| "run-record history must be an array".to_string())?;
    history.push(run_record_history_entry(state, &now));

    validate_and_write_json(bundle_path, "runtime.runRecord", run_file, &envelope)?;
    println!("OK: run-record transition");
    Ok(())
}

fn run_record_read_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err("usage: nixfied-kernel run-record read <run-file> <field>".to_string());
    }

    let envelope = parse_json_file(&values[0], "run-record file")?;
    let payload = object_field(&envelope, "payload")
        .ok_or_else(|| "run-record payload is missing".to_string())?;
    let field = values[1].as_str();
    let rendered = match field {
        "state" | "attempt_id" | "command" | "process_mode" => {
            required_string_field(payload, field, "run-record payload")?.to_string()
        }
        "pid" | "pgid" => object_field(payload, field)
            .and_then(json_value_to_plain_string)
            .unwrap_or_default(),
        other => return Err(format!("unknown run-record read field: {}", other)),
    };

    println!("{}", rendered);
    Ok(())
}
