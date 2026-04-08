use super::*;

pub(crate) fn registry_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "append" => registry_append_command(values),
        "replay" => registry_replay_command(values),
        "terminal" => registry_terminal_command(values),
        "runtime-status" => registry_runtime_status_command(values),
        other => Err(format!("unknown registry subcommand: {}", other)),
    }
}

pub(crate) fn registry_append_event_internal(
    bundle_path: &str,
    root: &str,
    run_id: &str,
    attempt_id: &str,
    workflow_id: &str,
    task_id: &str,
    state: &str,
    detail: &JsonValue,
) -> Result<String, String> {
    let seq_file = format!("{}/.seq", root);
    let events_file = format!("{}/events.ndjson", root);
    let index_file = format!("{}/events.index.tsv", root);
    fs::create_dir_all(root).map_err(|err| format!("failed to create {}: {}", root, err))?;

    let seq = fs::read_to_string(&seq_file)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(0)
        + 1;
    write_text_atomic(&seq_file, &seq.to_string())?;

    let ts = current_utc_timestamp()?;
    let ts_epoch = current_epoch_seconds()?;
    let detail_reason = registry_detail_reason(detail);
    let detail_exit_code = registry_detail_exit_code(detail);
    let attempt_id_val = nullable_string_value(attempt_id);
    let workflow_id_val = nullable_string_value(workflow_id);
    let task_id_val = nullable_string_value(task_id);
    let envelope = json!({
        "kind": "runtime-event",
        "version": 1,
        "payload": {
            "runId": run_id,
            "attemptId": attempt_id_val,
            "workflowId": workflow_id_val,
            "taskId": task_id_val,
            "seq": seq,
            "ts": ts,
            "state": state,
            "detail": detail,
        },
    });
    validate_and_write_json(bundle_path, "runtime.registryEvent", "-", &envelope)?;

    let rendered = render_json_compact(&envelope);
    append_line(&events_file, &rendered)?;
    append_line(
        &index_file,
        &format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            seq,
            ts_epoch,
            ts,
            run_id,
            attempt_id,
            workflow_id,
            task_id,
            state,
            detail_reason,
            detail_exit_code
        ),
    )?;

    Ok(rendered)
}

fn registry_append_command(values: &[String]) -> Result<(), String> {
    if values.len() != 9 {
        return Err(
            "usage: nixfied-kernel registry append <bundle-file> <root> <run-id> <attempt-id> <workflow-id> <task-id> <state> <detail-file> <export-file>"
                .to_string(),
        );
    }

    let rendered = registry_append_event_internal(
        &values[0],
        &values[1],
        &values[2],
        &values[3],
        &values[4],
        &values[5],
        &values[6],
        &parse_json_file(&values[7], "registry event detail file")?,
    )?;
    write_shell_exports(
        &values[8],
        &[
            (
                "REGISTRY_APPEND_LAST_SEQ".to_string(),
                fs::read_to_string(format!("{}/.seq", &values[1]))
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            ),
            (
                "REGISTRY_APPEND_LAST_EVENT_JSON".to_string(),
                rendered.to_string(),
            ),
        ],
    )?;
    println!("OK: registry append");
    Ok(())
}

fn registry_detail_reason(detail: &JsonValue) -> String {
    object_string(detail, "reason").unwrap_or("").to_string()
}

fn registry_detail_exit_code(detail: &JsonValue) -> String {
    object_field(detail, "exitCode")
        .and_then(json_value_to_i64)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn registry_replay_command(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err("usage: nixfied-kernel registry replay <root>".to_string());
    }

    let index_file = format!("{}/events.index.tsv", values[0]);
    if !Path::new(&index_file).exists() {
        println!("{{}}");
        return Ok(());
    }

    let content = read_text(&index_file)?;
    let replay = registry_replay_map_from_index_text(&content);

    println!("{}", render_json_compact(&JsonValue::Object(replay)));
    Ok(())
}

fn registry_terminal_command(values: &[String]) -> Result<(), String> {
    if values.len() != 3 {
        return Err(
            "usage: nixfied-kernel registry terminal <index-file> <run-id> <attempt-id|empty>"
                .to_string(),
        );
    }

    let content = read_text(&values[0])?;
    let run_id = values[1].as_str();
    let attempt_id = values[2].as_str();
    let (state, exit_code) = registry_terminal_from_index_text(&content, run_id, attempt_id);
    println!("{}\t{}", state, exit_code);

    Ok(())
}

fn registry_runtime_status_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel registry runtime-status <service-index-file> <slot-index-file>"
                .to_string(),
        );
    }

    let service_event = registry_latest_event_from_index(&values[0])?;
    let slot_event = registry_latest_event_from_index(&values[1])?;

    print!(
        "{}",
        render_shell_exports(&registry_runtime_status_exports(
            service_event.as_ref(),
            slot_event.as_ref(),
        )?)
    );
    Ok(())
}

fn registry_latest_event_from_index(path: &str) -> Result<Option<JsonValue>, String> {
    if path.is_empty() || !Path::new(path).exists() {
        return Ok(None);
    }

    let content = read_text(path)?;
    registry_latest_event_from_index_text(path, &content)
}

fn registry_latest_event_from_index_text(
    path: &str,
    content: &str,
) -> Result<Option<JsonValue>, String> {
    let mut latest = None::<(i64, JsonValue)>;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let Some(seq_raw) = parts.next() else {
            continue;
        };
        let Some(event_json) = parts.next() else {
            continue;
        };
        let Ok(seq) = seq_raw.parse::<i64>() else {
            continue;
        };
        let event = parse_json(event_json).map_err(|err| {
            format!(
                "registry runtime-status index {} contains invalid json: {}",
                path, err
            )
        })?;
        let should_replace = match latest.as_ref() {
            Some((latest_seq, _)) => seq >= *latest_seq,
            None => true,
        };
        if should_replace {
            latest = Some((seq, event));
        }
    }
    Ok(latest.map(|(_, event)| event))
}

fn registry_replay_map_from_index_text(content: &str) -> Map<String, JsonValue> {
    let mut replay = Map::new();
    for line in content.lines() {
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() < 8 {
            continue;
        }
        let workflow_id = parts[5];
        let task_id = parts[6];
        let state = parts[7];
        let key = if !task_id.is_empty() {
            format!("task:{}", task_id)
        } else if !workflow_id.is_empty() {
            format!("workflow:{}", workflow_id)
        } else {
            continue;
        };
        replay.insert(key, JsonValue::String(state.to_string()));
    }
    replay
}

fn registry_terminal_from_index_text(
    content: &str,
    run_id: &str,
    attempt_id: &str,
) -> (String, String) {
    let mut terminal_state = None::<String>;
    let mut exit_code = None::<String>;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }

        if fields[3] != run_id {
            continue;
        }
        if !attempt_id.is_empty() && fields[4] != attempt_id {
            continue;
        }

        match fields[7] {
            "passed" | "failed" | "canceled" => {
                terminal_state = Some(fields[7].to_string());
                exit_code = Some(fields[9].to_string());
            }
            _ => {}
        }
    }

    match terminal_state.as_deref() {
        Some("passed") => ("passed".to_string(), "0".to_string()),
        Some("canceled") => ("canceled".to_string(), "130".to_string()),
        Some("failed") => (
            "failed".to_string(),
            exit_code
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "1".to_string()),
        ),
        _ => ("unknown".to_string(), "1".to_string()),
    }
}

fn registry_runtime_status_exports(
    service_event: Option<&JsonValue>,
    slot_event: Option<&JsonValue>,
) -> Result<Vec<(String, String)>, String> {
    let mut registry_found = "0".to_string();
    let mut registry_running = "false".to_string();
    let mut registry_state = "unknown".to_string();
    let mut owner_run_id = String::new();
    let mut owner_scope = String::new();
    let mut ephemeral_root = String::new();
    let mut wait_reason = String::new();
    let mut log_path = String::new();

    if let Some(event) = service_event {
        let payload = object_field(event, "payload")
            .ok_or_else(|| "registry runtime-status service event missing payload".to_string())?;
        registry_found = "1".to_string();
        registry_state =
            required_string_field(payload, "state", "registry runtime-status payload")?.to_string();
        owner_run_id = object_string(payload, "runId").unwrap_or("").to_string();
        registry_running = if matches!(
            registry_state.as_str(),
            "starting" | "running" | "ready" | "degraded" | "waiting" | "busy"
        ) {
            "true".to_string()
        } else {
            "false".to_string()
        };

        if let Some(detail) = object_field(payload, "detail") {
            owner_scope = object_string(detail, "ownerScope")
                .unwrap_or("")
                .to_string();
            ephemeral_root = object_string(detail, "ephemeralRoot")
                .unwrap_or("")
                .to_string();
            wait_reason = object_string(detail, "waitReason")
                .unwrap_or("")
                .to_string();
            log_path = object_string(detail, "logPath").unwrap_or("").to_string();
        }
    }

    let slot_owner = if let Some(event) = slot_event {
        let payload = object_field(event, "payload")
            .ok_or_else(|| "registry runtime-status slot event missing payload".to_string())?;
        let state =
            required_string_field(payload, "state", "registry runtime-status slot payload")?;
        if state == "released" {
            String::new()
        } else {
            object_string(payload, "runId").unwrap_or("").to_string()
        }
    } else {
        String::new()
    };

    Ok(vec![
        ("REGISTRY_FOUND".to_string(), registry_found),
        ("REGISTRY_RUNNING".to_string(), registry_running),
        ("REGISTRY_STATE".to_string(), registry_state),
        ("OWNER_RUN_ID".to_string(), owner_run_id),
        ("OWNER_SCOPE".to_string(), owner_scope),
        ("EPHEMERAL_ROOT".to_string(), ephemeral_root),
        ("WAIT_REASON".to_string(), wait_reason),
        ("LOG_PATH".to_string(), log_path),
        ("SLOT_OWNER".to_string(), slot_owner),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn exports_to_map(exports: Vec<(String, String)>) -> BTreeMap<String, String> {
        exports.into_iter().collect()
    }

    #[test]
    fn registry_projects_replay_terminal_and_runtime_status() {
        let index = concat!(
            "1\t10\tworkflow\trun-1\tattempt-1\tworkflow.test.full\ttask.alpha\tqueued\t\t\n",
            "2\t11\tworkflow\trun-1\tattempt-1\tworkflow.test.full\ttask.alpha\trunning\t\t\n",
            "3\t12\tworkflow\trun-1\tattempt-1\tworkflow.test.full\ttask.alpha\tpassed\t\t0\n",
            "4\t13\tworkflow\trun-1\tattempt-1\tworkflow.test.full\t\tfailed\t\t7\n",
        );

        let replay = registry_replay_map_from_index_text(index);
        assert_eq!(replay["task:task.alpha"], json!("passed"));
        assert_eq!(replay["workflow:workflow.test.full"], json!("failed"));

        let terminal = registry_terminal_from_index_text(index, "run-1", "attempt-1");
        assert_eq!(terminal, ("failed".to_string(), "7".to_string()));

        let exports = exports_to_map(
            registry_runtime_status_exports(
                Some(&json!({
                    "payload": {
                        "state": "waiting",
                        "runId": "run-1",
                        "detail": {
                            "ownerScope": "workflow",
                            "ephemeralRoot": "/tmp/run-1",
                            "waitReason": "dependency",
                            "logPath": "/tmp/run-1/service.log"
                        }
                    }
                })),
                Some(&json!({
                    "payload": {
                        "state": "claimed",
                        "runId": "run-1"
                    }
                })),
            )
            .expect("runtime status exports should succeed"),
        );
        assert_eq!(exports["REGISTRY_FOUND"], "1");
        assert_eq!(exports["REGISTRY_RUNNING"], "true");
        assert_eq!(exports["REGISTRY_STATE"], "waiting");
        assert_eq!(exports["OWNER_RUN_ID"], "run-1");
        assert_eq!(exports["WAIT_REASON"], "dependency");
        assert_eq!(exports["SLOT_OWNER"], "run-1");
    }

    #[test]
    fn registry_runtime_status_uses_highest_seq_not_last_line() {
        let latest = registry_latest_event_from_index_text(
            "service-events.tsv",
            concat!(
                "11\t{\"payload\":{\"state\":\"stopped\",\"runId\":\"run-stop\"}}\n",
                "10\t{\"payload\":{\"state\":\"ready\",\"runId\":\"run-ready\"}}\n",
            ),
        )
        .expect("latest event parse should succeed")
        .expect("latest event should exist");

        assert_eq!(latest["payload"]["state"], json!("stopped"));
        assert_eq!(latest["payload"]["runId"], json!("run-stop"));
    }
}
