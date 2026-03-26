use super::*;
use crate::validation::resolve_json_path;

pub(crate) fn probe_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "evaluate" => probe_evaluate_command(values),
        "jsonrpc" => probe_jsonrpc_command(values),
        other => Err(format!("unknown probe subcommand: {}", other)),
    }
}

fn probe_evaluate_command(values: &[String]) -> Result<(), String> {
    let plan_path = values.first().ok_or_else(|| {
        "usage: nixfied-kernel probe evaluate <plan-file> [payload-file] [export-file]".to_string()
    })?;
    if values.len() > 3 {
        return Err(
            "usage: nixfied-kernel probe evaluate <plan-file> [payload-file] [export-file]"
                .to_string(),
        );
    }

    let plan_value = parse_json_file(plan_path, "probe plan")?;
    let plan_kind = required_string_field(&plan_value, "kind", "probe plan")?;
    match plan_kind {
        "nixfied-probe-plan" => {
            let payload_path = values.get(1).ok_or_else(|| {
                "usage: nixfied-kernel probe evaluate <plan-file> <payload-file> [export-file]"
                    .to_string()
            })?;
            let export_path = values.get(2).map(|value| value.as_str());
            let plan = load_probe_plan_from_value(&plan_value)?;
            let payload = parse_json_file(payload_path, "probe payload")?;
            let exports = probe_plan_exports(&plan, &payload)?;

            if let Some(export_path) = export_path {
                write_shell_exports(export_path, &exports)?;
            } else if !exports.is_empty() {
                return Err(
                    "probe evaluate requires export-file when plan emits exports".to_string(),
                );
            }

            println!("OK: probe evaluate kind={}", plan.probe_kind);
            Ok(())
        }
        "nixfied-probe-execution-plan" => {
            if values.len() != 1 {
                return Err(
                    "usage: nixfied-kernel probe evaluate <execution-plan-file>".to_string()
                );
            }
            let plan = load_probe_execution_plan_from_value(&plan_value)?;
            execute_probe_execution_plan(&plan)
        }
        other => Err(format!("unknown probe plan kind: {}", other)),
    }
}

fn probe_jsonrpc_command(values: &[String]) -> Result<(), String> {
    if values.len() != 5 {
        return Err(
            "usage: nixfied-kernel probe jsonrpc <plan-file> <curl-bin> <url> <method> <max-time>"
                .to_string(),
        );
    }

    let max_time = parse_i64_text(&values[4], "probe jsonrpc max-time")?;
    if max_time < 1 {
        return Err(format!(
            "probe jsonrpc max-time must be positive, got {}",
            max_time
        ));
    }

    let plan = load_probe_plan(&values[0])?;
    let payload = request_jsonrpc_payload(&values[1], &values[2], &values[3], max_time)?;
    let exports = probe_plan_exports(&plan, &payload)?;

    if let Some((_, value)) = exports.first() {
        println!("{}", value);
    }

    Ok(())
}

fn execute_probe_execution_plan(plan: &ProbeExecutionPlan) -> Result<(), String> {
    let _ = (&plan.mode, &plan.service_name);
    let probe_source = env::var(&plan.source_env_var)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unspecified".to_string());

    for step in &plan.steps {
        execute_probe_execution_step(plan, step, &probe_source)?;
    }

    Ok(())
}

fn execute_probe_execution_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    match step.kind.as_str() {
        "tcp" => execute_probe_tcp_step(step, probe_source),
        "http" => execute_probe_http_step(plan, step, probe_source),
        "jsonrpc" => execute_probe_jsonrpc_step(plan, step, probe_source),
        "postgres-pg-isready" => execute_probe_pg_isready_step(plan, step, probe_source),
        "postgres-query" => execute_probe_postgres_query_step(plan, step, probe_source),
        "helios-ready" => execute_probe_helios_ready_step(plan, step, probe_source),
        "exec" => execute_probe_exec_step(plan, step, probe_source),
        other => Err(format!("unsupported probe execution step kind={}", other)),
    }
}

fn execute_probe_tcp_step(step: &ProbeExecutionStep, probe_source: &str) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;

    println!(
        "INFO: checking {} {} port={} source={}",
        step.service_label, step.phase_label, port, probe_source
    );

    let addresses = format!("{}:{}", host, port)
        .to_socket_addrs()
        .map_err(|err| {
            format!(
                "probe tcp address resolution failed host={} port={} err={}",
                host, port, err
            )
        })?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(format!(
            "{} {} port={} (no resolved address)",
            step.service_label, step.failure_label, port
        ));
    }

    let timeout = Duration::from_secs(2);
    if addresses
        .iter()
        .any(|address| TcpStream::connect_timeout(address, timeout).is_ok())
    {
        println!(
            "OK: {} {} port={}",
            step.service_label, step.success_label, port
        );
        Ok(())
    } else {
        Err(format!(
            "{} {} port={}",
            step.service_label, step.failure_label, port
        ))
    }
}

fn execute_probe_http_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let scheme = probe_step_required_field(step, "scheme", step.scheme.as_deref())?;
    let path = probe_step_required_field(step, "path", step.path.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let max_time = probe_step_max_time(step)?;
    let url = build_probe_url(scheme, host, &port, path);

    println!(
        "INFO: checking {} {} url={} source={}",
        step.service_label, step.phase_label, url, probe_source
    );

    let args = vec![
        "-fsS".to_string(),
        "--max-time".to_string(),
        max_time.to_string(),
        url.clone(),
    ];
    let output = run_captured_program(&plan.curl_bin, &args, &[])?;
    if output.status.success() {
        println!(
            "OK: {} {} url={}",
            step.service_label, step.success_label, url
        );
        Ok(())
    } else {
        let exit_code = output.status.code().unwrap_or(1);
        let detail = captured_output_detail(&output);
        Err(format!(
            "{} {} url={} exit={}{}",
            step.service_label,
            step.failure_label,
            url,
            exit_code,
            if detail.is_empty() {
                String::new()
            } else {
                format!(" detail={}", detail)
            }
        ))
    }
}

fn execute_probe_jsonrpc_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let scheme = probe_step_required_field(step, "scheme", step.scheme.as_deref())?;
    let method = probe_step_required_field(step, "method", step.method.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let max_time = probe_step_max_time(step)?;
    let url = build_probe_url(scheme, host, &port, "");

    println!(
        "INFO: checking {} {} port={} source={}",
        step.service_label, step.phase_label, port, probe_source
    );

    let payload = request_jsonrpc_payload(&plan.curl_bin, &url, method, max_time)?;
    if matches!(
        resolve_json_path(&payload, ".result"),
        Some(JsonValue::Null) | None
    ) {
        Err(format!(
            "{} {} port={}",
            step.service_label, step.failure_label, port
        ))
    } else {
        println!(
            "OK: {} {} port={}",
            step.service_label, step.success_label, port
        );
        Ok(())
    }
}

fn execute_probe_pg_isready_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let failure_suffix = step.failure_suffix.as_deref().unwrap_or("");

    println!(
        "INFO: checking {} {} port={} source={}",
        step.service_label, step.phase_label, port, probe_source
    );

    let args = vec![
        "-U".to_string(),
        "postgres".to_string(),
        "-h".to_string(),
        host.to_string(),
        "-p".to_string(),
        port.clone(),
        "-q".to_string(),
    ];
    let output = run_captured_program(&plan.pg_is_ready_bin, &args, &[])?;
    if output.status.success() {
        println!(
            "OK: {} {} port={}",
            step.service_label, step.success_label, port
        );
        Ok(())
    } else {
        Err(format!(
            "{} {} port={}{}",
            step.service_label, step.failure_label, port, failure_suffix
        ))
    }
}

fn execute_probe_postgres_query_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let database = probe_step_required_field(step, "database", step.database.as_deref())?;
    let query = probe_step_required_field(step, "query", step.query.as_deref())?;
    let failure_suffix = step.failure_suffix.as_deref().unwrap_or("");

    println!(
        "INFO: checking {} {} port={} source={}",
        step.service_label, step.phase_label, port, probe_source
    );

    let args = vec![
        "-h".to_string(),
        host.to_string(),
        "-p".to_string(),
        port.clone(),
        "-U".to_string(),
        "postgres".to_string(),
        "-d".to_string(),
        database.to_string(),
        "-Atqc".to_string(),
        query.to_string(),
    ];
    let output = run_captured_program(&plan.psql_bin, &args, &[])?;
    if output.status.success() {
        println!(
            "OK: {} {} port={}",
            step.service_label, step.success_label, port
        );
        Ok(())
    } else {
        Err(format!(
            "{} {} port={}{}",
            step.service_label, step.failure_label, port, failure_suffix
        ))
    }
}

fn execute_probe_helios_ready_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let _ = step
        .execution_port_env_var
        .as_deref()
        .map(required_port_from_env)
        .transpose()?;
    let max_time = probe_step_max_time(step)?;
    let profile = step.readiness_profile.as_deref().unwrap_or("fast");
    let source_kind = step
        .source_kinds
        .get(probe_source)
        .map(|value| value.as_str())
        .unwrap_or("unknown");
    let url = build_probe_url("http", host, &port, "");

    println!(
        "INFO: checking {} {} port={} source={} source_kind={} profile={}",
        step.service_label, step.phase_label, port, probe_source, source_kind, profile
    );

    if step
        .disallow_source_kinds
        .iter()
        .any(|value| value == source_kind)
    {
        return Err(format!(
            "{} {} port={} source={} source_kind={} profile={} (source kind disallowed)",
            step.service_label, step.failure_label, port, probe_source, source_kind, profile
        ));
    }

    let block_number = request_jsonrpc_payload(&plan.curl_bin, &url, "eth_blockNumber", max_time)
        .ok()
        .and_then(|payload| resolve_json_path(&payload, ".result").cloned())
        .and_then(|value| match value {
            JsonValue::String(text) if is_hex_prefixed(&text) => Some(text),
            _ => None,
        });
    let block_number_valid = block_number.is_some();

    if let Some(block_number) = &block_number {
        println!(
            "OK: {} {} port={} block_number={}",
            step.service_label, step.success_label, port, block_number
        );
    } else if step.allow_local_health_fallback
        && !step.require_not_syncing
        && request_jsonrpc_payload(&plan.curl_bin, &url, "eth_chainId", max_time)
            .ok()
            .and_then(|payload| resolve_json_path(&payload, ".result").cloned())
            .filter(|value| !matches!(value, JsonValue::Null))
            .is_some()
    {
        println!(
            "OK: {} {} port={} mode=local_chainid_fallback",
            step.service_label, step.success_label, port
        );
        return Ok(());
    } else if step.require_not_syncing {
        println!(
            "WARN: {} block number unavailable port={} source={} source_kind={} profile={}; continuing to sync gate",
            step.service_label, port, probe_source, source_kind, profile
        );
    } else {
        return Err(format!(
            "{} {} port={} source={} source_kind={} (invalid eth_blockNumber result)",
            step.service_label, step.failure_label, port, probe_source, source_kind
        ));
    }

    if step.require_not_syncing {
        let syncing_payload =
            request_jsonrpc_payload(&plan.curl_bin, &url, "eth_syncing", max_time).ok();
        let syncing_value = syncing_payload
            .as_ref()
            .and_then(|payload| resolve_json_path(payload, ".result"));
        let syncing_result = syncing_value.map(render_json_compact).unwrap_or_default();
        if !matches!(syncing_value, Some(JsonValue::Bool(false))) {
            return Err(format!(
                "{} {} port={} source={} source_kind={} profile={} (eth_syncing={})",
                step.service_label,
                step.failure_label,
                port,
                probe_source,
                source_kind,
                profile,
                syncing_result
            ));
        }
        if !block_number_valid {
            return Err(format!(
                "{} {} port={} source={} source_kind={} profile={} (invalid eth_blockNumber result)",
                step.service_label,
                step.failure_label,
                port,
                probe_source,
                source_kind,
                profile
            ));
        }
        println!("OK: {} sync status ready port={}", step.service_label, port);
    } else {
        println!("SKIP: helios sync gate disabled profile={}", profile);
    }

    Ok(())
}

fn execute_probe_exec_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let command = probe_step_required_field(step, "command", step.command.as_deref())?;

    println!(
        "INFO: checking {} {} source={} kind=exec",
        step.service_label, step.phase_label, probe_source
    );

    let status = run_streaming_program(
        &plan.runtime_shell_bin,
        &["-c".to_string(), command.to_string()],
        &[],
    )?;
    if status.success() {
        println!("OK: {} {}", step.service_label, step.success_label);
        Ok(())
    } else {
        Err(format!("{} {}", step.service_label, step.failure_label))
    }
}

fn request_jsonrpc_payload(
    curl_bin: &str,
    url: &str,
    method: &str,
    max_time: i64,
) -> Result<JsonValue, String> {
    let request_body = render_json_compact(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": [],
    }));
    let args = vec![
        "-fsS".to_string(),
        "--max-time".to_string(),
        max_time.to_string(),
        "-H".to_string(),
        "content-type: application/json".to_string(),
        "--data".to_string(),
        request_body,
        url.to_string(),
    ];
    let output = run_captured_program(curl_bin, &args, &[])?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        let exit_code = output.status.code().unwrap_or(1);
        return Err(format!(
            "probe jsonrpc request failed url={} method={} exit={}{}",
            url,
            method,
            exit_code,
            if detail.is_empty() {
                String::new()
            } else {
                format!(" detail={}", detail)
            }
        ));
    }

    let response = String::from_utf8(output.stdout)
        .map_err(|err| format!("probe jsonrpc response is not valid UTF-8: {}", err))?;
    parse_json(&response)
        .map_err(|err| format!("probe jsonrpc response is not valid JSON: {}", err))
}

fn probe_plan_exports(
    plan: &ProbePlan,
    payload: &JsonValue,
) -> Result<Vec<(String, String)>, String> {
    let result = resolve_json_path(payload, ".result");

    match plan.probe_kind.as_str() {
        "jsonrpc-result-present" => {
            if matches!(result, Some(JsonValue::Null) | None) {
                return Err("probe result is missing".to_string());
            }
            Ok(Vec::new())
        }
        "jsonrpc-result-hex" => {
            let value = result
                .and_then(JsonValue::as_str)
                .ok_or_else(|| "probe result must be a hex string".to_string())?;
            if !is_hex_prefixed(value) {
                return Err(format!("probe result must be hex, got {}", value));
            }
            Ok(vec![(
                plan.export_var.clone().ok_or_else(|| {
                    "probe plan jsonrpc-result-hex requires exportVar".to_string()
                })?,
                value.to_string(),
            )])
        }
        "jsonrpc-result-compact" => {
            let value = result.ok_or_else(|| "probe result is missing".to_string())?;
            if matches!(value, JsonValue::Null) {
                return Err("probe result is missing".to_string());
            }
            Ok(vec![(
                plan.export_var.clone().ok_or_else(|| {
                    "probe plan jsonrpc-result-compact requires exportVar".to_string()
                })?,
                render_json_compact(value),
            )])
        }
        "jsonrpc-result-bool-false" => match result {
            Some(JsonValue::Bool(false)) => Ok(Vec::new()),
            _ => Err("probe result must be false".to_string()),
        },
        other => Err(format!("unknown probe evaluate kind: {}", other)),
    }
}

fn probe_step_required_field<'a>(
    step: &ProbeExecutionStep,
    field: &str,
    value: Option<&'a str>,
) -> Result<&'a str, String> {
    value.ok_or_else(|| {
        format!(
            "probe execution step kind={} missing field {}",
            step.kind, field
        )
    })
}

fn probe_step_max_time(step: &ProbeExecutionStep) -> Result<i64, String> {
    let Some(max_time) = step.max_time_seconds else {
        return Err(format!(
            "probe execution step kind={} missing field maxTimeSeconds",
            step.kind
        ));
    };
    if max_time < 1 {
        return Err(format!(
            "probe execution step kind={} maxTimeSeconds must be positive, got {}",
            step.kind, max_time
        ));
    }
    Ok(max_time)
}

fn load_probe_plan(path: &str) -> Result<ProbePlan, String> {
    let value = parse_json_file(path, "probe plan")?;
    load_probe_plan_from_value(&value)
}

fn load_probe_plan_from_value(value: &JsonValue) -> Result<ProbePlan, String> {
    let kind = required_string_field(value, "kind", "probe plan")?;
    if kind != "nixfied-probe-plan" {
        return Err(format!("unsupported probe plan kind: {}", kind));
    }
    Ok(ProbePlan {
        probe_kind: required_string_field(value, "probeKind", "probe plan")?.to_string(),
        export_var: object_string(value, "exportVar").map(|value| value.to_string()),
    })
}

fn load_probe_execution_plan_from_value(value: &JsonValue) -> Result<ProbeExecutionPlan, String> {
    let kind = required_string_field(value, "kind", "probe execution plan")?;
    if kind != "nixfied-probe-execution-plan" {
        return Err(format!("unsupported probe execution plan kind: {}", kind));
    }

    let version = object_field(value, "version")
        .and_then(json_value_to_i64)
        .ok_or_else(|| "probe execution plan missing integer field version".to_string())?;
    if version != 1 {
        return Err(format!(
            "probe execution plan version must be 1 (got {})",
            version
        ));
    }

    let mut steps = Vec::new();
    for (index, step_value) in object_array(value, "steps")
        .ok_or_else(|| "probe execution plan missing array field steps".to_string())?
        .iter()
        .enumerate()
    {
        steps.push(parse_probe_execution_step(step_value, index + 1)?);
    }

    Ok(ProbeExecutionPlan {
        mode: required_string_field(value, "mode", "probe execution plan")?.to_string(),
        service_name: required_string_field(value, "serviceName", "probe execution plan")?
            .to_string(),
        source_env_var: required_string_field(value, "sourceEnvVar", "probe execution plan")?
            .to_string(),
        curl_bin: required_string_field(value, "curlBin", "probe execution plan")?.to_string(),
        runtime_shell_bin: required_string_field(value, "runtimeShellBin", "probe execution plan")?
            .to_string(),
        pg_is_ready_bin: required_string_field(value, "pgIsReadyBin", "probe execution plan")?
            .to_string(),
        psql_bin: required_string_field(value, "psqlBin", "probe execution plan")?.to_string(),
        steps,
    })
}

fn parse_probe_execution_step(
    value: &JsonValue,
    index: usize,
) -> Result<ProbeExecutionStep, String> {
    let label = format!("probe execution step {}", index);
    Ok(ProbeExecutionStep {
        kind: required_string_field(value, "kind", &label)?.to_string(),
        service_label: required_string_field(value, "serviceLabel", &label)?.to_string(),
        phase_label: required_string_field(value, "phaseLabel", &label)?.to_string(),
        success_label: required_string_field(value, "successLabel", &label)?.to_string(),
        failure_label: required_string_field(value, "failureLabel", &label)?.to_string(),
        host: object_string(value, "host").map(|text| text.to_string()),
        scheme: object_string(value, "scheme").map(|text| text.to_string()),
        path: object_string(value, "path").map(|text| text.to_string()),
        method: object_string(value, "method").map(|text| text.to_string()),
        port_env_var: object_string(value, "portEnvVar").map(|text| text.to_string()),
        execution_port_env_var: object_string(value, "executionPortEnvVar")
            .map(|text| text.to_string()),
        source_kinds: object_string_map(value, "sourceKinds", &label)?,
        readiness_profile: object_string(value, "readinessProfile").map(|text| text.to_string()),
        require_not_syncing: object_bool(value, "requireNotSyncing").unwrap_or(false),
        allow_local_health_fallback: object_bool(value, "allowLocalHealthFallback")
            .unwrap_or(false),
        disallow_source_kinds: array_strings(value, "disallowSourceKinds"),
        max_time_seconds: object_field(value, "maxTimeSeconds").and_then(json_value_to_i64),
        database: object_string(value, "database").map(|text| text.to_string()),
        query: object_string(value, "query").map(|text| text.to_string()),
        failure_suffix: object_string(value, "failureSuffix").map(|text| text.to_string()),
        command: object_string(value, "command").map(|text| text.to_string()),
    })
}
