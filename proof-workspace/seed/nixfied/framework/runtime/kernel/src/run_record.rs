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

    let envelope = create_run_record_envelope(
        &now,
        &values[2],
        &values[3],
        &values[4],
        &values[5],
        &values[6],
        &values[7],
        &values[8],
        parse_bool_flag(&values[9])?,
        args,
    );

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
    apply_run_record_transition(
        &mut envelope,
        state,
        exit_code,
        stop_reason.as_deref(),
        pid,
        pgid,
        &now,
    )?;

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

fn create_run_record_envelope(
    now: &str,
    run_id: &str,
    attempt_id: &str,
    command: &str,
    workflow_id: &str,
    task_id: &str,
    execution_mode: &str,
    process_mode: &str,
    ephemeral_enabled: bool,
    args: JsonValue,
) -> JsonValue {
    let history = vec![run_record_history_entry("queued", now)];
    let workflow_id = nullable_string_value(workflow_id);
    let task_id = nullable_string_value(task_id);
    let payload = json!({
        "run_id": run_id,
        "attempt_id": attempt_id,
        "command": command,
        "workflow_id": workflow_id,
        "task_id": task_id,
        "execution_mode": execution_mode,
        "process_mode": process_mode,
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

    json!({
        "kind": "run-record",
        "version": 1,
        "payload": payload,
    })
}

fn apply_run_record_transition(
    envelope: &mut JsonValue,
    state: &str,
    exit_code: Option<i64>,
    stop_reason: Option<&str>,
    pid: Option<i64>,
    pgid: Option<i64>,
    now: &str,
) -> Result<(), String> {
    let payload = object_field_mut(envelope, "payload")
        .ok_or_else(|| "run-record payload is missing".to_string())?;
    let payload_object = payload
        .as_object_mut()
        .ok_or_else(|| "run-record payload must be an object".to_string())?;

    payload_object.insert("state".to_string(), JsonValue::String(state.to_string()));
    payload_object.insert("updated_at".to_string(), JsonValue::String(now.to_string()));
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
        payload_object.insert("started_at".to_string(), JsonValue::String(now.to_string()));
    }
    if matches!(state, "passed" | "failed" | "canceled") {
        payload_object.insert(
            "finished_at".to_string(),
            JsonValue::String(now.to_string()),
        );
    }
    if let Some(exit_code) = exit_code {
        payload_object.insert(
            "exit_code".to_string(),
            JsonValue::Number(Number::from(exit_code)),
        );
    }
    if let Some(stop_reason) = stop_reason {
        payload_object.insert(
            "stop_reason".to_string(),
            JsonValue::String(stop_reason.to_string()),
        );
    }

    let history_value = payload_object
        .get_mut("history")
        .ok_or_else(|| "run-record history is missing".to_string())?;
    let history = history_value
        .as_array_mut()
        .ok_or_else(|| "run-record history must be an array".to_string())?;
    history.push(run_record_history_entry(state, now));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_record_transition_is_monotonic() {
        let mut envelope = create_run_record_envelope(
            "2026-03-27T10:00:00Z",
            "run-1",
            "attempt-1",
            "run-task",
            "workflow.test.full",
            "task.test.unit",
            "task",
            "foreground",
            true,
            json!(["--summary"]),
        );

        apply_run_record_transition(
            &mut envelope,
            "running",
            None,
            None,
            Some(1001),
            Some(2002),
            "2026-03-27T10:00:05Z",
        )
        .expect("running transition should succeed");
        apply_run_record_transition(
            &mut envelope,
            "failed",
            Some(17),
            Some("signal"),
            None,
            None,
            "2026-03-27T10:00:09Z",
        )
        .expect("failed transition should succeed");

        let payload = envelope["payload"].as_object().expect("payload object");
        assert_eq!(payload["state"], json!("failed"));
        assert_eq!(payload["started_at"], json!("2026-03-27T10:00:05Z"));
        assert_eq!(payload["finished_at"], json!("2026-03-27T10:00:09Z"));
        assert_eq!(payload["pid"], json!(1001));
        assert_eq!(payload["pgid"], json!(2002));
        assert_eq!(payload["exit_code"], json!(17));
        assert_eq!(payload["stop_reason"], json!("signal"));

        let history = payload["history"].as_array().expect("history array");
        let states = history
            .iter()
            .map(|entry| entry["state"].as_str().expect("state"))
            .collect::<Vec<_>>();
        assert_eq!(states, vec!["queued", "running", "failed"]);
    }
}
