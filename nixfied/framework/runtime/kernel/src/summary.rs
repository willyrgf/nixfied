use super::*;
use crate::workflow::load_workflow_summary_plan;

pub(crate) fn summary_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "write" => summary_write_command(values),
        "compose" => summary_compose_command(values),
        "collect-steps" => summary_collect_steps_command(values),
        "render-human" => summary_render_human_command(values),
        other => Err(format!("unknown summary subcommand: {}", other)),
    }
}

fn summary_write_command(values: &[String]) -> Result<(), String> {
    if values.len() != 3 {
        return Err(
            "usage: nixfied-kernel summary write <bundle-file> <summary-file> <input-file>"
                .to_string(),
        );
    }

    let input = parse_json_file(&values[2], "summary input file")?;
    let envelope =
        if object_field(&input, "kind").is_some() && object_field(&input, "payload").is_some() {
            input
        } else {
            json!({
                "kind": "workflow-summary",
                "version": 1,
                "payload": input,
            })
        };
    validate_and_write_json(&values[0], "runtime.summary", &values[1], &envelope)?;
    println!("OK: summary write");
    Ok(())
}

fn summary_compose_command(values: &[String]) -> Result<(), String> {
    if values.len() != 24 {
        return Err(
            "usage: nixfied-kernel summary compose <bundle-file> <summary-file> <run-id> <attempt-id> <workflow-id> <mode> <exit-code> <started-at> <finished-at> <duration-seconds> <passed> <failed> <skipped> <canceled> <steps-file> <total-duration> <setup-duration> <steps-duration> <teardown-duration> <accounted-duration> <untracked-duration> <max-workers|empty> <peak-workers|empty> <canceled-count|empty>"
                .to_string(),
        );
    }

    let steps = parse_summary_steps_file(&values[14])?;
    let exit_code = parse_i64_text(&values[6], "summary compose exit-code")?;
    let duration_seconds = parse_i64_text(&values[9], "summary compose duration-seconds")?;
    let passed = parse_i64_text(&values[10], "summary compose passed")?;
    let failed = parse_i64_text(&values[11], "summary compose failed")?;
    let skipped = parse_i64_text(&values[12], "summary compose skipped")?;
    let canceled = parse_i64_text(&values[13], "summary compose canceled")?;
    let total_duration = parse_i64_text(&values[15], "summary compose total-duration")?;
    let setup_duration = parse_i64_text(&values[16], "summary compose setup-duration")?;
    let steps_duration = parse_i64_text(&values[17], "summary compose steps-duration")?;
    let teardown_duration = parse_i64_text(&values[18], "summary compose teardown-duration")?;
    let accounted_duration = parse_i64_text(&values[19], "summary compose accounted-duration")?;
    let untracked_duration = parse_i64_text(&values[20], "summary compose untracked-duration")?;
    let max_workers = optional_i64_json_value(parse_optional_i64(
        &values[21],
        "summary compose max-workers",
    )?);
    let peak_workers = optional_i64_json_value(parse_optional_i64(
        &values[22],
        "summary compose peak-workers",
    )?);
    let canceled_count = optional_i64_json_value(parse_optional_i64(
        &values[23],
        "summary compose canceled-count",
    )?);
    let payload = json!({
        "run_id": values[2],
        "attempt_id": values[3],
        "workflow_id": values[4],
        "mode": values[5],
        "exit_code": exit_code,
        "started_at": values[7],
        "finished_at": values[8],
        "duration_seconds": duration_seconds,
        "counts": {
            "passed": passed,
            "failed": failed,
            "skipped": skipped,
            "canceled": canceled,
        },
        "steps": steps,
        "timing": {
            "total_duration": total_duration,
            "setup_duration": setup_duration,
            "steps_duration": steps_duration,
            "teardown_duration": teardown_duration,
            "accounted_duration": accounted_duration,
            "untracked_duration": untracked_duration,
            "parallelism": {
                "max_workers": max_workers,
                "peak_workers": peak_workers,
                "canceled_count": canceled_count,
            },
        },
    });
    let envelope = json!({
        "kind": "workflow-summary",
        "version": 1,
        "payload": payload,
    });

    validate_and_write_json(&values[0], "runtime.summary", &values[1], &envelope)?;
    println!("OK: summary compose");
    Ok(())
}

fn summary_collect_steps_command(values: &[String]) -> Result<(), String> {
    if values.len() != 6 {
        return Err(
            "usage: nixfied-kernel summary collect-steps <plan-file> <index-file> <run-id> <attempt-id|empty> <steps-file> <export-file>"
                .to_string(),
        );
    }

    let plan = load_workflow_summary_plan(&values[0])?;
    let collected = collect_workflow_summary(&plan, &values[1], &values[2], &values[3])?;
    write_workflow_collected_steps_file(&values[4], &collected.steps)?;
    write_shell_exports(
        &values[5],
        &[
            (
                "WORKFLOW_PASSED_COUNT".to_string(),
                collected.passed.to_string(),
            ),
            (
                "WORKFLOW_FAILED_COUNT".to_string(),
                collected.failed.to_string(),
            ),
            (
                "WORKFLOW_SKIPPED_COUNT".to_string(),
                collected.skipped.to_string(),
            ),
            (
                "WORKFLOW_CANCELED_COUNT".to_string(),
                collected.canceled.to_string(),
            ),
            (
                "WORKFLOW_STEPS_DURATION".to_string(),
                collected.steps_duration.to_string(),
            ),
            (
                "WORKFLOW_PEAK_WORKERS".to_string(),
                collected.peak_workers.to_string(),
            ),
            (
                "WORKFLOW_LEAF_TASK_IDS_LINES".to_string(),
                collected.leaf_task_ids_lines,
            ),
        ],
    )?;
    println!("OK: summary collect-steps");
    Ok(())
}

fn summary_render_human_command(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err("usage: nixfied-kernel summary render-human <summary-file>".to_string());
    }

    let summary = parse_json_file(&values[0], "summary file")?;
    let payload = object_field(&summary, "payload")
        .ok_or_else(|| "summary payload is missing".to_string())?;
    let steps = object_array(payload, "steps").unwrap_or(&[]);
    let counts = object_field(payload, "counts")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "summary counts are missing".to_string())?;
    let timing = object_field(payload, "timing")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "summary timing is missing".to_string())?;
    let total_duration = object_field(payload, "duration_seconds")
        .and_then(json_value_to_i64)
        .unwrap_or(0);
    let exit_code = object_field(payload, "exit_code")
        .and_then(json_value_to_i64)
        .unwrap_or(1);
    let skipped = counts
        .get("skipped")
        .and_then(json_value_to_i64)
        .unwrap_or(0);
    let parallel = timing
        .get("parallelism")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();

    println!();
    println!("------------------------------------------------------------");
    println!("Summary");
    println!("------------------------------------------------------------");
    println!("Source: {}", values[0]);
    for step in steps {
        let status = object_string(step, "status").unwrap_or("failed");
        let marker = match status {
            "passed" => "PASS",
            "skipped" => "SKIP",
            _ => "FAIL",
        };
        let name = object_string(step, "name").unwrap_or("unknown");
        let duration = object_field(step, "duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0);
        println!("  [{}] {} ({}s)", marker, name, duration);
    }
    println!("Total time: {}", format_duration_seconds(total_duration));
    println!(
        "INFO: Time breakdown setup={}s steps={}s teardown={}s accounted={}s untracked={}s",
        timing
            .get("setup_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        timing
            .get("steps_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        timing
            .get("teardown_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        timing
            .get("accounted_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        timing
            .get("untracked_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0)
    );
    println!(
        "INFO: Parallelism max_workers={} peak_workers={} canceled_count={}",
        parallel
            .get("max_workers")
            .map(json_scalar_or_placeholder)
            .unwrap_or_else(|| "?".to_string()),
        parallel
            .get("peak_workers")
            .map(json_scalar_or_placeholder)
            .unwrap_or_else(|| "?".to_string()),
        parallel
            .get("canceled_count")
            .map(json_scalar_or_placeholder)
            .unwrap_or_else(|| "?".to_string())
    );
    if skipped > 0 {
        println!("INFO: SKIP: {} task(s) skipped", skipped);
    }
    if exit_code == 0 {
        println!("OK: Exit code: 0");
    } else {
        println!("ERROR: Exit code: {}", exit_code);
    }
    if let Some(parent) = Path::new(&values[0]).parent() {
        println!();
        println!("Artifacts: {}", parent.display());
    }
    println!("------------------------------------------------------------");
    Ok(())
}

fn workflow_step_status_from_state_reason(state: &str, reason: &str) -> String {
    if state == "canceled"
        && matches!(
            reason,
            "missing-env" | "when-false" | "service-skipped" | "dependency-skipped"
        )
    {
        "skipped".to_string()
    } else {
        state.to_string()
    }
}

fn collect_workflow_summary(
    plan: &WorkflowSummaryPlan,
    index_file: &str,
    run_id: &str,
    attempt_id: &str,
) -> Result<WorkflowCollectedSummary, String> {
    if !Path::new(index_file).exists() {
        return Ok(WorkflowCollectedSummary {
            steps: Vec::new(),
            passed: 0,
            failed: 0,
            skipped: 0,
            canceled: 0,
            steps_duration: 0,
            peak_workers: 0,
            leaf_task_ids_lines: String::new(),
        });
    }

    let content = read_text(index_file)?;
    let mut active_order_seq = BTreeMap::<String, i64>::new();
    let mut active_workflow_id = BTreeMap::<String, String>::new();
    let mut active_running_epoch = BTreeMap::<String, i64>::new();
    let mut terminal_rows = Vec::<WorkflowTerminalRow>::new();
    let mut running_workers = 0i64;
    let mut peak_workers = 0i64;

    for (row_index, line) in content.lines().enumerate() {
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

        let workflow_id = fields[5];
        let task_id = fields[6];
        let state = fields[7];
        let reason = fields[8];
        let exit_code = fields[9];
        if task_id.is_empty() {
            continue;
        }

        let runner_type = plan
            .task_runner_types
            .get(task_id)
            .map(|value| value.as_str())
            .unwrap_or("shell");
        let counts_for_peak = runner_type != "workflowRef";
        if counts_for_peak {
            match state {
                "running" => {
                    running_workers += 1;
                    if running_workers > peak_workers {
                        peak_workers = running_workers;
                    }
                }
                "passed" | "failed" | "canceled" => {
                    if running_workers > 0 {
                        running_workers -= 1;
                    }
                }
                _ => {}
            }
        }

        if !matches!(
            state,
            "queued" | "running" | "passed" | "failed" | "canceled"
        ) {
            continue;
        }

        let key = format!("{}\u{1f}{}", workflow_id, task_id);
        let seq = fields[0].parse::<i64>().ok().unwrap_or(0);
        let ts_epoch = fields[1].parse::<i64>().ok();

        match state {
            "queued" | "running" => {
                if active_order_seq
                    .get(&key)
                    .map(|value| seq < *value)
                    .unwrap_or(true)
                {
                    active_order_seq.insert(key.clone(), seq);
                }
                active_workflow_id.insert(key.clone(), workflow_id.to_string());
                if state == "running" {
                    if let Some(ts_epoch) = ts_epoch {
                        active_running_epoch.insert(key, ts_epoch);
                    }
                }
            }
            "passed" | "failed" | "canceled" => {
                let order = active_order_seq.remove(&key).unwrap_or(seq);
                let entry_workflow_id = active_workflow_id
                    .remove(&key)
                    .unwrap_or_else(|| workflow_id.to_string());
                let duration = match (active_running_epoch.remove(&key), ts_epoch) {
                    (Some(started_at), Some(finished_at)) if finished_at >= started_at => {
                        finished_at - started_at
                    }
                    _ => 0,
                };
                terminal_rows.push(WorkflowTerminalRow {
                    order,
                    row_index,
                    name: task_id.to_string(),
                    workflow_id: entry_workflow_id,
                    state: state.to_string(),
                    duration,
                    reason: reason.to_string(),
                    exit_code: exit_code.to_string(),
                });
            }
            _ => {}
        }
    }

    terminal_rows.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then(left.row_index.cmp(&right.row_index))
    });

    let mut steps = Vec::new();
    let mut passed = 0i64;
    let mut failed = 0i64;
    let mut skipped = 0i64;
    let mut canceled = 0i64;
    let mut steps_duration = 0i64;
    let mut seen_leaf_tasks = BTreeSet::new();
    let mut leaf_task_ids = Vec::new();

    for row in terminal_rows {
        let runner_type = plan
            .task_runner_types
            .get(&row.name)
            .map(|value| value.as_str())
            .unwrap_or("shell");
        if runner_type == "workflowRef" {
            continue;
        }

        let status = workflow_step_status_from_state_reason(&row.state, &row.reason);
        match row.state.as_str() {
            "passed" => passed += 1,
            "failed" => failed += 1,
            "canceled" => {
                if status == "skipped" {
                    skipped += 1;
                } else {
                    canceled += 1;
                }
            }
            _ => {}
        }
        steps_duration += row.duration;
        if seen_leaf_tasks.insert(row.name.clone()) {
            leaf_task_ids.push(row.name.clone());
        }
        steps.push(WorkflowCollectedStep {
            name: row.name,
            status,
            state: row.state,
            duration: row.duration,
            order: row.order,
            workflow_id: row.workflow_id,
            reason: row.reason,
            exit_code: row.exit_code,
        });
    }

    Ok(WorkflowCollectedSummary {
        steps,
        passed,
        failed,
        skipped,
        canceled,
        steps_duration,
        peak_workers,
        leaf_task_ids_lines: leaf_task_ids.join("\n"),
    })
}

fn write_workflow_collected_steps_file(
    path: &str,
    steps: &[WorkflowCollectedStep],
) -> Result<(), String> {
    if path == "-" {
        return Ok(());
    }

    let mut rendered = String::new();
    for step in steps {
        rendered.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            step.name,
            step.status,
            step.duration,
            step.state,
            step.order,
            step.workflow_id,
            step.reason,
            step.exit_code
        ));
    }
    write_text_atomic(path, &rendered)
}

fn parse_summary_steps_file(path: &str) -> Result<Vec<JsonValue>, String> {
    let text = read_text(path)?;
    let mut steps = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() != 8 {
            return Err(format!(
                "summary steps file {} must contain 8 tab-separated fields per line",
                path
            ));
        }
        let duration = parse_i64_text(parts[2], "summary step duration")?;
        let order = parse_i64_text(parts[4], "summary step order")?;
        let workflow_id_val = nullable_string_value(parts[5]);
        let reason = nullable_string_value(parts[6]);
        let exit_code_val =
            optional_i64_json_value(parse_optional_i64(parts[7], "summary step exit_code")?);
        steps.push(json!({
            "name": parts[0],
            "status": parts[1],
            "state": parts[3],
            "duration": duration,
            "order": order,
            "workflow_id": workflow_id_val,
            "reason": reason,
            "exit_code": exit_code_val,
        }));
    }
    Ok(steps)
}
