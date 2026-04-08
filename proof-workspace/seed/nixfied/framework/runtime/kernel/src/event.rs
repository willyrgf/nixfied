use super::*;

pub(crate) fn event_detail_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "render" => event_detail_render_command(values),
        other => Err(format!("unknown event-detail subcommand: {}", other)),
    }
}

pub(crate) fn event_state_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "derive" => event_state_derive_command(values),
        other => Err(format!("unknown event-state subcommand: {}", other)),
    }
}

pub(crate) fn event_state_derive_command(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err("usage: nixfied-kernel event-state derive <event-type>".to_string());
    }

    println!("{}", derived_registry_state_for_event_type(&values[0]));
    Ok(())
}

pub(crate) fn derived_registry_state_for_event_type(event_type: &str) -> &'static str {
    match event_type {
        "slot_acquired" => "busy",
        "slot_released" => "released",
        "service_starting" => "starting",
        "service_ready" => "ready",
        "service_stopped" => "stopped",
        "service_orphaned" => "orphaned",
        "service_degraded" => "degraded",
        "readiness_progress" => "waiting",
        _ => "unknown",
    }
}

pub(crate) fn event_detail_render_command(values: &[String]) -> Result<(), String> {
    let kind = values.first().ok_or_else(|| {
        "usage: nixfied-kernel event-detail render <kind> [--field value ...]".to_string()
    })?;

    let mut event_type = None;
    let mut command_name = None;
    let mut project_id = None;
    let mut service = None;
    let mut slot = None;
    let mut env_name = None;
    let mut profile = None;
    let mut pid = None;
    let mut pgid = None;
    let mut _plan_id = None;
    let mut _unit_id = None;
    let mut attempt = None;
    let mut owner_scope = None;
    let mut reuse_policy = None;
    let mut discovery_scope = None;
    let mut ephemeral_root = None;
    let mut readiness_health = None;
    let mut readiness_ready = None;
    let mut last_error = None;
    let mut wait_reason = None;
    let mut log_path = None;
    let mut mode = None;
    let mut suffix_reason = None;
    let mut produces = None;
    let mut exit_code = None;
    let mut reason = None;
    let mut dependency = None;
    let mut service_name = None;
    let mut signal = None;
    let mut missing = None;
    let mut run_id = None;
    let mut workflow_id = None;
    let mut task_id = None;
    let mut target = None;
    let mut index = 1usize;

    while index < values.len() {
        let flag = values[index].as_str();
        index += 1;
        let value = next_flag_value(values, &mut index, flag)?;
        match flag {
            "--event-type" => event_type = optional_string_value(&value),
            "--command-name" => command_name = optional_string_value(&value),
            "--project-id" => project_id = optional_string_value(&value),
            "--service" => service = optional_string_value(&value),
            "--slot" => slot = optional_string_value(&value),
            "--env" => env_name = optional_string_value(&value),
            "--profile" => profile = optional_string_value(&value),
            "--pid" => pid = parse_optional_i64(&value, "event-detail pid")?,
            "--pgid" => pgid = parse_optional_i64(&value, "event-detail pgid")?,
            "--plan-id" => _plan_id = optional_string_value(&value),
            "--unit-id" => _unit_id = optional_string_value(&value),
            "--attempt" => attempt = parse_optional_i64(&value, "event-detail attempt")?,
            "--owner-scope" => owner_scope = optional_string_value(&value),
            "--reuse-policy" => reuse_policy = optional_string_value(&value),
            "--discovery-scope" => discovery_scope = optional_string_value(&value),
            "--ephemeral-root" => ephemeral_root = optional_string_value(&value),
            "--readiness-health" => {
                readiness_health =
                    parse_optional_bool_text(&value, "event-detail readiness-health")?
            }
            "--readiness-ready" => {
                readiness_ready = parse_optional_bool_text(&value, "event-detail readiness-ready")?
            }
            "--last-error" => last_error = optional_string_value(&value),
            "--wait-reason" => wait_reason = optional_string_value(&value),
            "--log-path" => log_path = optional_string_value(&value),
            "--mode" => mode = optional_string_value(&value),
            "--suffix-reason" => suffix_reason = optional_string_value(&value),
            "--produces-json" => {
                produces = Some(
                    parse_json(&value)
                        .map_err(|err| format!("event-detail produces json is invalid: {}", err))?,
                )
            }
            "--exit-code" => exit_code = parse_optional_i64(&value, "event-detail exit-code")?,
            "--reason" => reason = optional_string_value(&value),
            "--dependency" => dependency = optional_string_value(&value),
            "--service-name" => service_name = optional_string_value(&value),
            "--signal" => signal = optional_string_value(&value),
            "--missing" => missing = optional_string_value(&value),
            "--run-id" => run_id = optional_string_value(&value),
            "--workflow-id" => workflow_id = optional_string_value(&value),
            "--task-id" => task_id = optional_string_value(&value),
            "--target" => target = optional_string_value(&value),
            other => return Err(format!("unknown event-detail arg: {}", other)),
        }
    }

    let mut fields = Map::new();
    fields.insert("kind".to_string(), JsonValue::String(kind.clone()));

    match kind.as_str() {
        "slotLifecycle" => {
            insert_optional_string_field(&mut fields, "eventType", event_type);
            insert_optional_string_field(&mut fields, "commandName", command_name);
            insert_optional_string_field(&mut fields, "projectId", project_id);
            insert_optional_string_field(&mut fields, "slot", slot);
            insert_optional_string_field(&mut fields, "env", env_name);
            insert_optional_string_field(&mut fields, "profile", profile);
            insert_optional_number_field(&mut fields, "pid", pid);
            insert_optional_number_field(&mut fields, "pgid", pgid);
            if readiness_health.is_some() || readiness_ready.is_some() || last_error.is_some() {
                let mut readiness = Map::new();
                insert_optional_bool_field(&mut readiness, "healthOk", readiness_health);
                insert_optional_bool_field(&mut readiness, "readyOk", readiness_ready);
                insert_optional_string_field(&mut readiness, "lastError", last_error);
                fields.insert("readiness".to_string(), JsonValue::Object(readiness));
            }
            insert_optional_string_field(&mut fields, "waitReason", wait_reason);
            insert_optional_string_field(&mut fields, "logPath", log_path);
            insert_optional_string_field(&mut fields, "mode", mode);
            insert_optional_string_field(&mut fields, "suffixReason", suffix_reason);
            insert_optional_json_field(&mut fields, "produces", produces);
            insert_optional_number_field(&mut fields, "exitCode", exit_code);
        }
        "serviceLifecycle" => {
            insert_optional_string_field(&mut fields, "eventType", event_type);
            insert_optional_string_field(&mut fields, "service", service);
            insert_optional_string_field(&mut fields, "commandName", command_name);
            insert_optional_string_field(&mut fields, "ownerScope", owner_scope);
            insert_optional_string_field(&mut fields, "reusePolicy", reuse_policy);
            insert_optional_string_field(&mut fields, "discoveryScope", discovery_scope);
            insert_optional_string_field(&mut fields, "ephemeralRoot", ephemeral_root);
            insert_optional_string_field(&mut fields, "waitReason", wait_reason);
            insert_optional_string_field(&mut fields, "logPath", log_path);
            insert_optional_json_field(&mut fields, "produces", produces);
            insert_optional_number_field(&mut fields, "exitCode", exit_code);
            insert_optional_string_field(&mut fields, "reason", reason);
            insert_optional_string_field(&mut fields, "dependency", dependency);
            insert_optional_string_field(&mut fields, "serviceName", service_name);
            insert_optional_string_field(&mut fields, "signal", signal);
            insert_optional_string_field(&mut fields, "missing", missing);
        }
        "workflowLifecycle" => {
            insert_optional_string_field(&mut fields, "commandName", command_name);
            insert_optional_string_field(&mut fields, "workflowId", workflow_id);
            insert_optional_string_field(&mut fields, "runId", run_id);
            insert_optional_number_field(&mut fields, "attempt", attempt);
        }
        "taskLifecycle" => {
            insert_optional_string_field(&mut fields, "commandName", command_name);
            insert_optional_string_field(&mut fields, "taskId", task_id);
            insert_optional_string_field(&mut fields, "runId", run_id);
            insert_optional_number_field(&mut fields, "attempt", attempt);
            insert_optional_number_field(&mut fields, "exitCode", exit_code);
            insert_optional_string_field(&mut fields, "reason", reason);
        }
        "controlSignal" => {
            insert_optional_string_field(&mut fields, "signal", signal);
            insert_optional_string_field(&mut fields, "reason", reason);
            insert_optional_string_field(&mut fields, "target", target);
            insert_optional_string_field(&mut fields, "runId", run_id);
        }
        other => return Err(format!("unknown event-detail kind: {}", other)),
    }

    println!("{}", render_json_compact(&JsonValue::Object(fields)));
    Ok(())
}
