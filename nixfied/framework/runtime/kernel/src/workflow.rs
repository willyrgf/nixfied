use super::*;

use std::collections::BTreeMap;

pub(crate) trait WorkflowUnitStateCommon {
    fn state(&self) -> &str;
    fn set_state(&mut self, value: &str);
    fn set_cancel_reason(&mut self, value: &str);
    fn set_cancel_extra_key(&mut self, value: &str);
    fn set_cancel_extra_value(&mut self, value: &str);
    fn dependents(&self) -> &[String];
    fn needs_left(&self) -> i64;
    fn set_needs_left(&mut self, value: i64);
    fn task_id(&self) -> &str;
}

pub(crate) fn mark_canceled<U: WorkflowUnitStateCommon>(
    units: &mut BTreeMap<String, U>,
    unit_name: &str,
    reason: &str,
    extra_key: &str,
    extra_value: &str,
) {
    let Some(entry) = units.get_mut(unit_name) else {
        return;
    };
    if entry.state() != "pending" && entry.state() != "ready" {
        return;
    }
    entry.set_state("cancel-pending");
    entry.set_cancel_reason(reason);
    entry.set_cancel_extra_key(extra_key);
    entry.set_cancel_extra_value(extra_value);
}

pub(crate) fn mark_dependency_canceled_recursive<U: WorkflowUnitStateCommon>(
    units: &mut BTreeMap<String, U>,
    unit_name: &str,
    reason: &str,
    dependency_task_id: &str,
) {
    mark_canceled(units, unit_name, reason, "dependency", dependency_task_id);
    let dependents = units
        .get(unit_name)
        .map(|entry| entry.dependents().to_vec())
        .unwrap_or_default();
    for dependent in dependents {
        mark_dependency_canceled_recursive(units, &dependent, reason, dependency_task_id);
    }
}

pub(crate) fn apply_passed_transition<U: WorkflowUnitStateCommon>(
    units: &mut BTreeMap<String, U>,
    unit_name: &str,
) -> Result<(), String> {
    let dependents = units
        .get(unit_name)
        .map(|unit| unit.dependents().to_vec())
        .ok_or_else(|| format!("unknown workflow unit '{}'", unit_name))?;
    let unit = units
        .get_mut(unit_name)
        .ok_or_else(|| format!("unknown workflow unit '{}'", unit_name))?;
    unit.set_state("passed");
    for dependent in dependents {
        if let Some(entry) = units.get_mut(&dependent) {
            if entry.state() == "pending" && entry.needs_left() > 0 {
                entry.set_needs_left(entry.needs_left() - 1);
                if entry.needs_left() == 0 {
                    entry.set_state("ready");
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn mark_failed_unit<U: WorkflowUnitStateCommon>(
    units: &mut BTreeMap<String, U>,
    unit_name: &str,
) -> Result<(Vec<String>, String), String> {
    let dependents = units
        .get(unit_name)
        .map(|unit| unit.dependents().to_vec())
        .ok_or_else(|| format!("unknown workflow unit '{}'", unit_name))?;
    let task_id = units
        .get(unit_name)
        .map(|unit| unit.task_id().to_string())
        .unwrap_or_default();
    let unit = units
        .get_mut(unit_name)
        .ok_or_else(|| format!("unknown workflow unit '{}'", unit_name))?;
    unit.set_state("failed");
    Ok((dependents, task_id))
}

pub(crate) fn workflow_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "serial-init" => workflow_serial_init_command(values),
        "serial-next" => workflow_serial_next_command(values),
        "serial-transition" => workflow_serial_transition_command(values),
        "parallel-init" => workflow_parallel_init_command(values),
        "parallel-next" => workflow_parallel_next_command(values),
        "parallel-transition" => workflow_parallel_transition_command(values),
        other => Err(format!("unknown workflow subcommand: {}", other)),
    }
}

pub(crate) fn workflow_serial_init_command(values: &[String]) -> Result<(), String> {
    if values.len() != 5 {
        return Err(
            "usage: nixfied-kernel workflow serial-init <plan-file> <skipped-services-file> <workflow-id> <fail-fast> <state-file>"
                .to_string(),
        );
    }

    let plan = load_workflow_scheduler_plan(&values[0])?;
    let skipped_services = load_line_set(&values[1])?;
    let workflow = plan
        .workflows
        .get(&values[2])
        .ok_or_else(|| format!("unknown workflow '{}'", values[2]))?;
    let state = build_workflow_serial_state(
        workflow,
        &values[2],
        parse_bool_flag(&values[3])?,
        &skipped_services,
    );
    write_workflow_serial_state(&values[4], &state)?;
    println!("OK: workflow serial-init");
    Ok(())
}

pub(crate) fn workflow_serial_next_command(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err("usage: nixfied-kernel workflow serial-next <state-file>".to_string());
    }

    let state = load_workflow_serial_state(&values[0])?;
    match workflow_serial_next_action(&state) {
        WorkflowSerialAction::Execute {
            unit_name,
            task_id,
            selected_services_csv,
        } => println!(
            "execute\u{1f}{}\u{1f}{}\u{1f}{}",
            unit_name, task_id, selected_services_csv
        ),
        WorkflowSerialAction::Cancel {
            unit_name,
            task_id,
            reason,
            extra_key,
            extra_value,
        } => println!(
            "cancel\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            unit_name, task_id, reason, extra_key, extra_value
        ),
        WorkflowSerialAction::Done { workflow_status } => {
            println!("done\u{1f}{}", workflow_status)
        }
    }
    Ok(())
}

pub(crate) fn workflow_serial_transition_command(values: &[String]) -> Result<(), String> {
    if values.len() != 7 {
        return Err(
            "usage: nixfied-kernel workflow serial-transition <state-file> <unit-name> <status> <exit-code|empty> <reason|empty> <extra-key|empty> <extra-value|empty>"
                .to_string(),
        );
    }

    let mut state = load_workflow_serial_state(&values[0])?;
    workflow_serial_transition(
        &mut state, &values[1], &values[2], &values[3], &values[4], &values[5], &values[6],
    )?;
    write_workflow_serial_state(&values[0], &state)?;
    println!("OK: workflow serial-transition");
    Ok(())
}

pub(crate) fn workflow_parallel_init_command(values: &[String]) -> Result<(), String> {
    if values.len() != 6 {
        return Err(
            "usage: nixfied-kernel workflow parallel-init <plan-file> <skipped-services-file> <workflow-id> <fail-fast> <max-workers> <state-file>"
                .to_string(),
        );
    }

    let plan = load_workflow_scheduler_plan(&values[0])?;
    let skipped_services = load_line_set(&values[1])?;
    let workflow = plan
        .workflows
        .get(&values[2])
        .ok_or_else(|| format!("unknown workflow '{}'", values[2]))?;
    let max_workers = parse_i64_text(&values[4], "workflow parallel max-workers")?;
    let state = build_workflow_parallel_state(
        workflow,
        &values[2],
        parse_bool_flag(&values[3])?,
        max_workers,
        &skipped_services,
    );
    write_workflow_parallel_state(&values[5], &state)?;
    println!("OK: workflow parallel-init");
    Ok(())
}

pub(crate) fn workflow_parallel_next_command(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err("usage: nixfied-kernel workflow parallel-next <state-file>".to_string());
    }

    let mut state = load_workflow_parallel_state(&values[0])?;
    let action = workflow_parallel_next_action(&mut state);
    write_workflow_parallel_state(&values[0], &state)?;
    match action {
        WorkflowParallelAction::Start {
            unit_name,
            task_id,
            selected_services_csv,
            produces_json,
        } => println!(
            "start\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            unit_name, task_id, selected_services_csv, produces_json
        ),
        WorkflowParallelAction::Cancel {
            unit_name,
            task_id,
            reason,
            extra_key,
            extra_value,
        } => println!(
            "cancel\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            unit_name, task_id, reason, extra_key, extra_value
        ),
        WorkflowParallelAction::SignalRunning { unit_name } => {
            println!("signal-running\u{1f}{}", unit_name)
        }
        WorkflowParallelAction::Wait => println!("wait"),
        WorkflowParallelAction::Done { workflow_status } => {
            println!("done\u{1f}{}", workflow_status)
        }
    }
    Ok(())
}

pub(crate) fn workflow_parallel_transition_command(values: &[String]) -> Result<(), String> {
    if values.len() != 7 {
        return Err(
            "usage: nixfied-kernel workflow parallel-transition <state-file> <unit-name> <status> <exit-code|empty> <reason|empty> <extra-key|empty> <extra-value|empty>"
                .to_string(),
        );
    }

    let mut state = load_workflow_parallel_state(&values[0])?;
    workflow_parallel_transition(
        &mut state, &values[1], &values[2], &values[3], &values[4], &values[5], &values[6],
    )?;
    write_workflow_parallel_state(&values[0], &state)?;
    println!("OK: workflow parallel-transition");
    Ok(())
}

pub(crate) fn load_workflow_scheduler_plan(path: &str) -> Result<WorkflowSchedulerPlan, String> {
    let value = parse_json_file(path, "workflow scheduler plan")?;
    let kind = required_string_field(&value, "kind", "workflow scheduler plan")?;
    if kind != "nixfied-workflow-scheduler-plan" {
        return Err(format!(
            "unsupported workflow scheduler plan kind: {}",
            kind
        ));
    }
    let version = object_field(&value, "version")
        .and_then(json_value_to_i64)
        .ok_or_else(|| "workflow scheduler plan missing integer field version".to_string())?;
    if version != 1 {
        return Err(format!(
            "workflow scheduler plan version must be 1 (got {})",
            version
        ));
    }

    let workflows_value = object_field(&value, "workflows")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "workflow scheduler plan missing object field workflows".to_string())?;
    let mut workflows = BTreeMap::new();
    for (workflow_id, workflow_value) in workflows_value {
        let units_value = object_field(workflow_value, "units")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| {
                format!(
                    "workflow scheduler plan workflow '{}' missing array field units",
                    workflow_id
                )
            })?;
        let mut units = Vec::new();
        for unit_value in units_value {
            units.push(WorkflowSchedulerUnitPlan {
                name: required_string_field(unit_value, "name", "workflow scheduler unit")?
                    .to_string(),
                task_id: required_string_field(unit_value, "taskId", "workflow scheduler unit")?
                    .to_string(),
                needs: array_strings(unit_value, "needs"),
                locks: array_strings(unit_value, "locks"),
                required_services: array_strings(unit_value, "requiredServices"),
                skip_if_missing_env: array_strings(unit_value, "skipIfMissingEnv"),
                when_env_present: array_strings(unit_value, "whenEnvPresent"),
                when_env_equals: object_string_map(
                    unit_value,
                    "whenEnvEquals",
                    "workflow scheduler unit",
                )?,
                selected_services_csv: object_string(unit_value, "selectedServicesCsv")
                    .unwrap_or("")
                    .to_string(),
                produces_json: object_string(unit_value, "producesJson")
                    .unwrap_or("{}")
                    .to_string(),
            });
        }
        workflows.insert(workflow_id.clone(), WorkflowSchedulerWorkflow { units });
    }
    Ok(WorkflowSchedulerPlan { workflows })
}

pub(crate) fn build_workflow_serial_state(
    workflow: &WorkflowSchedulerWorkflow,
    workflow_id: &str,
    fail_fast: bool,
    skipped_services: &BTreeSet<String>,
) -> WorkflowSerialState {
    let mut units = BTreeMap::new();
    let mut order = Vec::new();

    for unit in &workflow.units {
        order.push(unit.name.clone());
        units.insert(
            unit.name.clone(),
            WorkflowSerialUnitState {
                name: unit.name.clone(),
                task_id: unit.task_id.clone(),
                needs_left: unit.needs.len() as i64,
                dependents: Vec::new(),
                state: "pending".to_string(),
                cancel_reason: String::new(),
                cancel_extra_key: String::new(),
                cancel_extra_value: String::new(),
                selected_services_csv: unit.selected_services_csv.clone(),
            },
        );
    }

    for unit in &workflow.units {
        for dependency in &unit.needs {
            if let Some(dep_state) = units.get_mut(dependency) {
                dep_state.dependents.push(unit.name.clone());
            }
        }
    }

    for unit in &workflow.units {
        if let Some(reason) = workflow_unit_direct_cancel_reason(unit, skipped_services) {
            workflow_serial_mark_canceled(&mut units, &unit.name, &reason.0, &reason.1, &reason.2);
            let task_id = units
                .get(&unit.name)
                .map(|entry| entry.task_id.clone())
                .unwrap_or_default();
            let dependents = units
                .get(&unit.name)
                .map(|entry| entry.dependents.clone())
                .unwrap_or_default();
            for dependent in dependents {
                workflow_serial_mark_dependency_canceled_recursive(
                    &mut units,
                    &dependent,
                    "dependency-skipped",
                    &task_id,
                );
            }
        }
    }

    for unit in &workflow.units {
        if matches!(
            units.get(&unit.name).map(|entry| entry.state.as_str()),
            Some("pending")
        ) && unit.needs.is_empty()
        {
            if let Some(entry) = units.get_mut(&unit.name) {
                entry.state = "ready".to_string();
            }
        }
    }

    WorkflowSerialState {
        workflow_id: workflow_id.to_string(),
        fail_fast,
        workflow_status: 0,
        halted: false,
        order,
        units,
    }
}

pub(crate) fn workflow_unit_direct_cancel_reason(
    unit: &WorkflowSchedulerUnitPlan,
    skipped_services: &BTreeSet<String>,
) -> Option<(String, String, String)> {
    let missing = unit
        .skip_if_missing_env
        .iter()
        .filter(|env_name| env::var(env_name.as_str()).unwrap_or_default().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Some((
            "missing-env".to_string(),
            "missing".to_string(),
            missing.join(","),
        ));
    }

    if let Some(service_name) = unit
        .required_services
        .iter()
        .find(|service_name| skipped_services.contains(*service_name))
    {
        return Some((
            "service-skipped".to_string(),
            "serviceName".to_string(),
            service_name.to_string(),
        ));
    }

    let when_env_present_missing = unit
        .when_env_present
        .iter()
        .any(|env_name| env::var(env_name.as_str()).unwrap_or_default().is_empty());
    if when_env_present_missing {
        return Some(("when-false".to_string(), String::new(), String::new()));
    }

    let when_env_equals_matches = unit
        .when_env_equals
        .iter()
        .all(|(env_name, expected)| env::var(env_name.as_str()).unwrap_or_default() == *expected);
    if !unit.when_env_equals.is_empty() && !when_env_equals_matches {
        return Some(("when-false".to_string(), String::new(), String::new()));
    }

    None
}

pub(crate) fn workflow_serial_mark_canceled(
    units: &mut BTreeMap<String, WorkflowSerialUnitState>,
    unit_name: &str,
    reason: &str,
    extra_key: &str,
    extra_value: &str,
) {
    workflow::mark_canceled(units, unit_name, reason, extra_key, extra_value);
}

pub(crate) fn workflow_serial_mark_dependency_canceled_recursive(
    units: &mut BTreeMap<String, WorkflowSerialUnitState>,
    unit_name: &str,
    reason: &str,
    dependency_task_id: &str,
) {
    workflow::mark_dependency_canceled_recursive(units, unit_name, reason, dependency_task_id);
}

pub(crate) fn workflow_serial_next_action(state: &WorkflowSerialState) -> WorkflowSerialAction {
    if state.halted {
        return WorkflowSerialAction::Done {
            workflow_status: state.workflow_status,
        };
    }

    for unit_name in &state.order {
        let Some(unit) = state.units.get(unit_name) else {
            continue;
        };
        match unit.state.as_str() {
            "cancel-pending" => {
                return WorkflowSerialAction::Cancel {
                    unit_name: unit.name.clone(),
                    task_id: unit.task_id.clone(),
                    reason: unit.cancel_reason.clone(),
                    extra_key: unit.cancel_extra_key.clone(),
                    extra_value: unit.cancel_extra_value.clone(),
                }
            }
            "ready" => {
                return WorkflowSerialAction::Execute {
                    unit_name: unit.name.clone(),
                    task_id: unit.task_id.clone(),
                    selected_services_csv: unit.selected_services_csv.clone(),
                }
            }
            _ => {}
        }
    }

    WorkflowSerialAction::Done {
        workflow_status: state.workflow_status,
    }
}

pub(crate) fn workflow_serial_transition(
    state: &mut WorkflowSerialState,
    unit_name: &str,
    status: &str,
    exit_code_text: &str,
    reason: &str,
    extra_key: &str,
    extra_value: &str,
) -> Result<(), String> {
    match status {
        "canceled" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow serial unit '{}'", unit_name))?;
            unit.set_state("canceled");
            if !reason.is_empty() {
                unit.set_cancel_reason(reason);
                unit.set_cancel_extra_key(extra_key);
                unit.set_cancel_extra_value(extra_value);
            }
        }
        "passed" => {
            workflow::apply_passed_transition(&mut state.units, unit_name)?;
        }
        "failed" => {
            let exit_code = if exit_code_text.is_empty() {
                1
            } else {
                parse_i64_text(exit_code_text, "workflow serial failed exit-code")?
            };
            let (dependents, task_id) = workflow::mark_failed_unit(&mut state.units, unit_name)?;
            if state.workflow_status == 0 {
                state.workflow_status = exit_code.max(1);
            }
            if state.fail_fast {
                state.halted = true;
            } else {
                for dependent in dependents {
                    workflow_serial_mark_dependency_canceled_recursive(
                        &mut state.units,
                        &dependent,
                        "dependency-not-passed",
                        &task_id,
                    );
                }
            }
        }
        other => {
            return Err(format!(
                "unsupported workflow serial transition status '{}'",
                other
            ))
        }
    }

    Ok(())
}

pub(crate) fn load_workflow_serial_state(path: &str) -> Result<WorkflowSerialState, String> {
    let value = parse_json_file(path, "workflow serial state")?;
    let kind = required_string_field(&value, "kind", "workflow serial state")?;
    if kind != "nixfied-workflow-serial-state" {
        return Err(format!("unsupported workflow serial state kind: {}", kind));
    }
    let version = object_field(&value, "version")
        .and_then(json_value_to_i64)
        .ok_or_else(|| "workflow serial state missing integer field version".to_string())?;
    if version != 1 {
        return Err(format!(
            "workflow serial state version must be 1 (got {})",
            version
        ));
    }

    let order = object_field(&value, "order")
        .and_then(JsonValue::as_array)
        .map_or(&[] as &[JsonValue], |v| v)
        .iter()
        .filter_map(JsonValue::as_str)
        .map(|item| item.to_string())
        .collect::<Vec<_>>();
    let units_value = object_field(&value, "units")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "workflow serial state missing object field units".to_string())?;
    let mut units = BTreeMap::new();
    for (unit_name, unit_value) in units_value {
        units.insert(
            unit_name.clone(),
            WorkflowSerialUnitState {
                name: required_string_field(unit_value, "name", "workflow serial unit")?
                    .to_string(),
                task_id: required_string_field(unit_value, "taskId", "workflow serial unit")?
                    .to_string(),
                needs_left: object_field(unit_value, "needsLeft")
                    .and_then(json_value_to_i64)
                    .unwrap_or(0),
                dependents: object_field(unit_value, "dependents")
                    .and_then(JsonValue::as_array)
                    .map_or(&[] as &[JsonValue], |v| v)
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .map(|item| item.to_string())
                    .collect(),
                state: required_string_field(unit_value, "state", "workflow serial unit")?
                    .to_string(),
                cancel_reason: object_string(unit_value, "cancelReason")
                    .unwrap_or("")
                    .to_string(),
                cancel_extra_key: object_string(unit_value, "cancelExtraKey")
                    .unwrap_or("")
                    .to_string(),
                cancel_extra_value: object_string(unit_value, "cancelExtraValue")
                    .unwrap_or("")
                    .to_string(),
                selected_services_csv: object_string(unit_value, "selectedServicesCsv")
                    .unwrap_or("")
                    .to_string(),
            },
        );
    }

    Ok(WorkflowSerialState {
        workflow_id: required_string_field(&value, "workflowId", "workflow serial state")?
            .to_string(),
        fail_fast: object_field(&value, "failFast")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        workflow_status: object_field(&value, "workflowStatus")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        halted: object_field(&value, "halted")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        order,
        units,
    })
}

pub(crate) fn write_workflow_serial_state(
    path: &str,
    state: &WorkflowSerialState,
) -> Result<(), String> {
    let mut units = Map::new();
    for (unit_name, unit) in &state.units {
        let dependents: Vec<JsonValue> = unit.dependents.iter().map(|item| json!(item)).collect();
        units.insert(
            unit_name.clone(),
            json!({
                "name": unit.name,
                "taskId": unit.task_id,
                "needsLeft": unit.needs_left,
                "dependents": dependents,
                "state": unit.state,
                "cancelReason": unit.cancel_reason,
                "cancelExtraKey": unit.cancel_extra_key,
                "cancelExtraValue": unit.cancel_extra_value,
                "selectedServicesCsv": unit.selected_services_csv,
            }),
        );
    }
    let order: Vec<JsonValue> = state.order.iter().map(|item| json!(item)).collect();
    let value = json!({
        "kind": "nixfied-workflow-serial-state",
        "version": 1,
        "workflowId": state.workflow_id,
        "failFast": state.fail_fast,
        "workflowStatus": state.workflow_status,
        "halted": state.halted,
        "order": order,
        "units": JsonValue::Object(units),
    });
    write_text_atomic(path, &format!("{}\n", render_json_compact(&value)))
}

pub(crate) fn build_workflow_parallel_state(
    workflow: &WorkflowSchedulerWorkflow,
    workflow_id: &str,
    fail_fast: bool,
    max_workers: i64,
    skipped_services: &BTreeSet<String>,
) -> WorkflowParallelState {
    let mut units = BTreeMap::new();
    let mut order = Vec::new();

    for unit in &workflow.units {
        order.push(unit.name.clone());
        units.insert(
            unit.name.clone(),
            WorkflowParallelUnitState {
                name: unit.name.clone(),
                task_id: unit.task_id.clone(),
                needs_left: unit.needs.len() as i64,
                dependents: Vec::new(),
                locks: unit.locks.clone(),
                state: "pending".to_string(),
                cancel_reason: String::new(),
                cancel_extra_key: String::new(),
                cancel_extra_value: String::new(),
                selected_services_csv: unit.selected_services_csv.clone(),
                produces_json: unit.produces_json.clone(),
            },
        );
    }

    for unit in &workflow.units {
        for dependency in &unit.needs {
            if let Some(dep_state) = units.get_mut(dependency) {
                dep_state.dependents.push(unit.name.clone());
            }
        }
    }

    for unit in &workflow.units {
        if let Some(reason) = workflow_unit_direct_cancel_reason(unit, skipped_services) {
            workflow_parallel_mark_canceled(
                &mut units, &unit.name, &reason.0, &reason.1, &reason.2,
            );
            let task_id = units
                .get(&unit.name)
                .map(|entry| entry.task_id.clone())
                .unwrap_or_default();
            let dependents = units
                .get(&unit.name)
                .map(|entry| entry.dependents.clone())
                .unwrap_or_default();
            for dependent in dependents {
                workflow_parallel_mark_dependency_canceled_recursive(
                    &mut units,
                    &dependent,
                    "dependency-skipped",
                    &task_id,
                );
            }
        }
    }

    for unit in &workflow.units {
        if matches!(
            units.get(&unit.name).map(|entry| entry.state.as_str()),
            Some("pending")
        ) && unit.needs.is_empty()
        {
            if let Some(entry) = units.get_mut(&unit.name) {
                entry.state = "ready".to_string();
            }
        }
    }

    WorkflowParallelState {
        workflow_id: workflow_id.to_string(),
        fail_fast,
        max_workers: max_workers.max(1),
        workflow_status: 0,
        stop_scheduling: false,
        order,
        units,
    }
}

pub(crate) fn workflow_parallel_mark_canceled(
    units: &mut BTreeMap<String, WorkflowParallelUnitState>,
    unit_name: &str,
    reason: &str,
    extra_key: &str,
    extra_value: &str,
) {
    workflow::mark_canceled(units, unit_name, reason, extra_key, extra_value);
}

pub(crate) fn workflow_parallel_mark_dependency_canceled_recursive(
    units: &mut BTreeMap<String, WorkflowParallelUnitState>,
    unit_name: &str,
    reason: &str,
    dependency_task_id: &str,
) {
    workflow::mark_dependency_canceled_recursive(units, unit_name, reason, dependency_task_id);
}

pub(crate) fn workflow_parallel_mark_all_pending_ready(
    units: &mut BTreeMap<String, WorkflowParallelUnitState>,
    reason: &str,
) {
    let unit_names = units.keys().cloned().collect::<Vec<_>>();
    for unit_name in unit_names {
        workflow_parallel_mark_canceled(units, &unit_name, reason, "", "");
    }
}

pub(crate) fn workflow_parallel_request_running_cancel(
    units: &mut BTreeMap<String, WorkflowParallelUnitState>,
    reason: &str,
) {
    for unit in units.values_mut() {
        if unit.state == "running" {
            unit.state = "cancel-running-requested".to_string();
            unit.cancel_reason = reason.to_string();
            unit.cancel_extra_key = String::new();
            unit.cancel_extra_value = String::new();
        }
    }
}

pub(crate) fn workflow_parallel_unit_holds_locks(state: &str) -> bool {
    matches!(
        state,
        "running" | "cancel-running-requested" | "cancel-running-signaled"
    )
}

pub(crate) fn workflow_parallel_running_count(state: &WorkflowParallelState) -> i64 {
    state
        .units
        .values()
        .filter(|unit| workflow_parallel_unit_holds_locks(&unit.state))
        .count() as i64
}

pub(crate) fn workflow_parallel_completed_count(state: &WorkflowParallelState) -> i64 {
    state
        .units
        .values()
        .filter(|unit| matches!(unit.state.as_str(), "passed" | "failed" | "canceled"))
        .count() as i64
}

pub(crate) fn workflow_parallel_unit_has_lock_conflict(
    state: &WorkflowParallelState,
    unit_name: &str,
) -> bool {
    let Some(candidate) = state.units.get(unit_name) else {
        return true;
    };
    for (other_name, other_unit) in &state.units {
        if other_name == unit_name || !workflow_parallel_unit_holds_locks(&other_unit.state) {
            continue;
        }
        for lock in &candidate.locks {
            if other_unit.locks.iter().any(|other_lock| other_lock == lock) {
                return true;
            }
        }
    }
    false
}

pub(crate) fn workflow_parallel_mark_blocked_if_needed(state: &mut WorkflowParallelState) {
    if state.stop_scheduling {
        return;
    }
    if workflow_parallel_running_count(state) > 0 {
        return;
    }
    if workflow_parallel_completed_count(state) >= state.order.len() as i64 {
        return;
    }
    workflow_parallel_mark_all_pending_ready(&mut state.units, "blocked");
    if state.workflow_status == 0 {
        state.workflow_status = 1;
    }
}

pub(crate) fn workflow_parallel_next_action(
    state: &mut WorkflowParallelState,
) -> WorkflowParallelAction {
    for unit_name in &state.order {
        let Some(unit) = state.units.get(unit_name) else {
            continue;
        };
        if unit.state == "cancel-running-requested" {
            return WorkflowParallelAction::SignalRunning {
                unit_name: unit.name.clone(),
            };
        }
    }

    for unit_name in &state.order {
        let Some(unit) = state.units.get(unit_name) else {
            continue;
        };
        if unit.state == "cancel-pending" {
            return WorkflowParallelAction::Cancel {
                unit_name: unit.name.clone(),
                task_id: unit.task_id.clone(),
                reason: unit.cancel_reason.clone(),
                extra_key: unit.cancel_extra_key.clone(),
                extra_value: unit.cancel_extra_value.clone(),
            };
        }
    }

    if !state.stop_scheduling && workflow_parallel_running_count(state) < state.max_workers {
        for unit_name in &state.order {
            let Some(unit) = state.units.get(unit_name) else {
                continue;
            };
            if unit.state != "ready" || workflow_parallel_unit_has_lock_conflict(state, unit_name) {
                continue;
            }
            return WorkflowParallelAction::Start {
                unit_name: unit.name.clone(),
                task_id: unit.task_id.clone(),
                selected_services_csv: unit.selected_services_csv.clone(),
                produces_json: unit.produces_json.clone(),
            };
        }
    }

    workflow_parallel_mark_blocked_if_needed(state);

    for unit_name in &state.order {
        let Some(unit) = state.units.get(unit_name) else {
            continue;
        };
        if unit.state == "cancel-pending" {
            return WorkflowParallelAction::Cancel {
                unit_name: unit.name.clone(),
                task_id: unit.task_id.clone(),
                reason: unit.cancel_reason.clone(),
                extra_key: unit.cancel_extra_key.clone(),
                extra_value: unit.cancel_extra_value.clone(),
            };
        }
    }

    if workflow_parallel_running_count(state) > 0 {
        WorkflowParallelAction::Wait
    } else {
        WorkflowParallelAction::Done {
            workflow_status: state.workflow_status,
        }
    }
}

pub(crate) fn workflow_parallel_transition(
    state: &mut WorkflowParallelState,
    unit_name: &str,
    status: &str,
    exit_code_text: &str,
    reason: &str,
    extra_key: &str,
    extra_value: &str,
) -> Result<(), String> {
    match status {
        "started" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            unit.set_state("running");
        }
        "canceled" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            unit.set_state("canceled");
            if !reason.is_empty() {
                unit.set_cancel_reason(reason);
                unit.set_cancel_extra_key(extra_key);
                unit.set_cancel_extra_value(extra_value);
            }
        }
        "signal-sent" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            if unit.state == "cancel-running-requested" {
                unit.set_state("cancel-running-signaled");
            }
        }
        "canceled-running" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            unit.set_state("canceled");
        }
        "passed" => {
            workflow::apply_passed_transition(&mut state.units, unit_name)?;
        }
        "failed" => {
            let exit_code = if exit_code_text.is_empty() {
                1
            } else {
                parse_i64_text(exit_code_text, "workflow parallel failed exit-code")?
            };
            let (dependents, task_id) = workflow::mark_failed_unit(&mut state.units, unit_name)?;
            if state.workflow_status == 0 {
                state.workflow_status = exit_code.max(1);
            }
            if state.fail_fast {
                state.stop_scheduling = true;
                workflow_parallel_request_running_cancel(&mut state.units, "fail-fast-running");
                workflow_parallel_mark_all_pending_ready(&mut state.units, "fail-fast");
            } else {
                for dependent in dependents {
                    workflow_parallel_mark_dependency_canceled_recursive(
                        &mut state.units,
                        &dependent,
                        "dependency-not-passed",
                        &task_id,
                    );
                }
            }
        }
        other => {
            return Err(format!(
                "unsupported workflow parallel transition status '{}'",
                other
            ))
        }
    }

    Ok(())
}

pub(crate) fn load_workflow_parallel_state(path: &str) -> Result<WorkflowParallelState, String> {
    let value = parse_json_file(path, "workflow parallel state")?;
    let kind = required_string_field(&value, "kind", "workflow parallel state")?;
    if kind != "nixfied-workflow-parallel-state" {
        return Err(format!(
            "unsupported workflow parallel state kind: {}",
            kind
        ));
    }
    let version = object_field(&value, "version")
        .and_then(json_value_to_i64)
        .ok_or_else(|| "workflow parallel state missing integer field version".to_string())?;
    if version != 1 {
        return Err(format!(
            "workflow parallel state version must be 1 (got {})",
            version
        ));
    }

    let order = object_field(&value, "order")
        .and_then(JsonValue::as_array)
        .map_or(&[] as &[JsonValue], |v| v)
        .iter()
        .filter_map(JsonValue::as_str)
        .map(|item| item.to_string())
        .collect::<Vec<_>>();
    let units_value = object_field(&value, "units")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "workflow parallel state missing object field units".to_string())?;
    let mut units = BTreeMap::new();
    for (unit_name, unit_value) in units_value {
        units.insert(
            unit_name.clone(),
            WorkflowParallelUnitState {
                name: required_string_field(unit_value, "name", "workflow parallel unit")?
                    .to_string(),
                task_id: required_string_field(unit_value, "taskId", "workflow parallel unit")?
                    .to_string(),
                needs_left: object_field(unit_value, "needsLeft")
                    .and_then(json_value_to_i64)
                    .unwrap_or(0),
                dependents: object_field(unit_value, "dependents")
                    .and_then(JsonValue::as_array)
                    .map_or(&[] as &[JsonValue], |v| v)
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .map(|item| item.to_string())
                    .collect(),
                locks: object_field(unit_value, "locks")
                    .and_then(JsonValue::as_array)
                    .map_or(&[] as &[JsonValue], |v| v)
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .map(|item| item.to_string())
                    .collect(),
                state: required_string_field(unit_value, "state", "workflow parallel unit")?
                    .to_string(),
                cancel_reason: object_string(unit_value, "cancelReason")
                    .unwrap_or("")
                    .to_string(),
                cancel_extra_key: object_string(unit_value, "cancelExtraKey")
                    .unwrap_or("")
                    .to_string(),
                cancel_extra_value: object_string(unit_value, "cancelExtraValue")
                    .unwrap_or("")
                    .to_string(),
                selected_services_csv: object_string(unit_value, "selectedServicesCsv")
                    .unwrap_or("")
                    .to_string(),
                produces_json: object_string(unit_value, "producesJson")
                    .unwrap_or("{}")
                    .to_string(),
            },
        );
    }

    Ok(WorkflowParallelState {
        workflow_id: required_string_field(&value, "workflowId", "workflow parallel state")?
            .to_string(),
        fail_fast: object_field(&value, "failFast")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        max_workers: object_field(&value, "maxWorkers")
            .and_then(json_value_to_i64)
            .unwrap_or(1),
        workflow_status: object_field(&value, "workflowStatus")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        stop_scheduling: object_field(&value, "stopScheduling")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        order,
        units,
    })
}

pub(crate) fn write_workflow_parallel_state(
    path: &str,
    state: &WorkflowParallelState,
) -> Result<(), String> {
    let mut units = Map::new();
    for (unit_name, unit) in &state.units {
        let dependents: Vec<JsonValue> = unit.dependents.iter().map(|item| json!(item)).collect();
        let locks: Vec<JsonValue> = unit.locks.iter().map(|item| json!(item)).collect();
        units.insert(
            unit_name.clone(),
            json!({
                "name": unit.name,
                "taskId": unit.task_id,
                "needsLeft": unit.needs_left,
                "dependents": dependents,
                "locks": locks,
                "state": unit.state,
                "cancelReason": unit.cancel_reason,
                "cancelExtraKey": unit.cancel_extra_key,
                "cancelExtraValue": unit.cancel_extra_value,
                "selectedServicesCsv": unit.selected_services_csv,
                "producesJson": unit.produces_json,
            }),
        );
    }
    let order: Vec<JsonValue> = state.order.iter().map(|item| json!(item)).collect();
    let value = json!({
        "kind": "nixfied-workflow-parallel-state",
        "version": 1,
        "workflowId": state.workflow_id,
        "failFast": state.fail_fast,
        "maxWorkers": state.max_workers,
        "workflowStatus": state.workflow_status,
        "stopScheduling": state.stop_scheduling,
        "order": order,
        "units": JsonValue::Object(units),
    });
    write_text_atomic(path, &format!("{}\n", render_json_compact(&value)))
}

pub(crate) fn load_workflow_summary_plan(path: &str) -> Result<WorkflowSummaryPlan, String> {
    let value = parse_json_file(path, "workflow summary plan")?;
    let kind = required_string_field(&value, "kind", "workflow summary plan")?;
    if kind != "nixfied-workflow-summary-plan" {
        return Err(format!("unsupported workflow summary plan kind: {}", kind));
    }
    let version = object_field(&value, "version")
        .and_then(json_value_to_i64)
        .ok_or_else(|| "workflow summary plan missing integer field version".to_string())?;
    if version != 1 {
        return Err(format!(
            "workflow summary plan version must be 1 (got {})",
            version
        ));
    }
    Ok(WorkflowSummaryPlan {
        task_runner_types: object_string_map(&value, "taskRunnerTypes", "workflow summary plan")?,
    })
}
