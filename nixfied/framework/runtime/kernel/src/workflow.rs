use super::*;
use crate::registry::registry_append_event_internal;
use crate::runtime_metadata::{
    load_runtime_metadata, runtime_metadata_task, runtime_metadata_workflow,
};

use std::collections::BTreeMap;
use std::process::{self, Command};

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
        "env-names" => workflow_env_names_command(values),
        "handoff" => workflow_handoff_command(values),
        "resolve-mode" => workflow_resolve_mode_command(values),
        "run" => workflow_run_command(values),
        other => Err(format!("unknown workflow subcommand: {}", other)),
    }
}

pub(crate) fn workflow_family_from_id(workflow_id: &str) -> Option<String> {
    let mut parts = workflow_id.split('.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("workflow"), Some(family), Some(_mode)) if !family.is_empty() => {
            Some(family.to_string())
        }
        _ => None,
    }
}

fn workflow_family_modes(metadata: &JsonValue, family: &str) -> Vec<String> {
    object_field(metadata, "workflowFamilies")
        .and_then(|families| object_field(families, family))
        .map(|entry| array_strings(entry, "modes"))
        .unwrap_or_default()
}

pub(crate) fn workflow_resolve_mode_id(
    metadata: &JsonValue,
    workflow_id: &str,
    mode_override: &str,
) -> Result<String, String> {
    runtime_metadata_workflow(metadata, workflow_id)?;
    if mode_override.is_empty() {
        return Ok(workflow_id.to_string());
    }

    let family = workflow_family_from_id(workflow_id)
        .ok_or_else(|| format!("workflow '{}' does not support mode overrides", workflow_id))?;
    let candidate = format!("workflow.{}.{}", family, mode_override);
    if runtime_metadata_workflow(metadata, &candidate).is_ok() {
        return Ok(candidate);
    }

    let expected_modes = workflow_family_modes(metadata, &family).join("|");
    if expected_modes.is_empty() {
        Err(format!("unknown mode '{}'", mode_override))
    } else {
        Err(format!(
            "unknown mode '{}' (expected: {})",
            mode_override, expected_modes
        ))
    }
}

fn workflow_resolve_mode_command(values: &[String]) -> Result<(), String> {
    if values.len() != 3 {
        return Err(
            "usage: nixfied-kernel workflow resolve-mode <runtime-metadata-file> <workflow-id> <mode-override>"
                .to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let resolved = workflow_resolve_mode_id(&metadata, &values[1], &values[2])?;
    println!("{}", resolved);
    Ok(())
}

fn workflow_handoff_command(values: &[String]) -> Result<(), String> {
    if values.len() != 3 {
        return Err(
            "usage: nixfied-kernel workflow handoff <runtime-metadata-file> <workflow-id> <output-dir>"
                .to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let workflow_id = &values[1];
    let output_dir = &values[2];
    let workflow = runtime_metadata_workflow(&metadata, workflow_id)?;
    let logging = object_field(workflow, "logging")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let exports = vec![
        ("NIXFIED_WORKFLOW_ID".to_string(), workflow_id.to_string()),
        (
            "NIXFIED_WORKFLOW_MODE_NAME".to_string(),
            object_string(workflow, "mode")
                .unwrap_or("custom")
                .to_string(),
        ),
        (
            "NIXFIED_WORKFLOW_ARTIFACTS_ROOT".to_string(),
            object_string(workflow, "artifactsRoot")
                .unwrap_or("")
                .to_string(),
        ),
        (
            "NIXFIED_WORKFLOW_EPHEMERAL_FLAG".to_string(),
            if object_bool(workflow, "ephemeralEnabled").unwrap_or(false) {
                "1".to_string()
            } else {
                "0".to_string()
            },
        ),
        (
            "NIXFIED_WORKFLOW_LOGGING_LEVEL_DEFAULT".to_string(),
            object_string(&logging, "levelDefault")
                .unwrap_or("")
                .to_string(),
        ),
        (
            "NIXFIED_WORKFLOW_LOGGING_OUTPUT_DEFAULT".to_string(),
            object_string(&logging, "outputDefault")
                .unwrap_or("")
                .to_string(),
        ),
        (
            "NIXFIED_WORKFLOW_PARALLEL_ENABLED".to_string(),
            if object_bool(workflow, "parallelEnabled").unwrap_or(false) {
                "true".to_string()
            } else {
                "false".to_string()
            },
        ),
        (
            "NIXFIED_WORKFLOW_MAX_WORKERS".to_string(),
            workflow
                .get("maxWorkers")
                .and_then(JsonValue::as_i64)
                .unwrap_or(1)
                .max(1)
                .to_string(),
        ),
        (
            "NIXFIED_WORKFLOW_WRITE_SUMMARY".to_string(),
            if object_bool(workflow, "writeSummary").unwrap_or(false) {
                "true".to_string()
            } else {
                "false".to_string()
            },
        ),
    ];
    write_shell_exports(&format!("{}/exports.sh", output_dir), &exports)?;
    write_lines_atomic(
        &format!("{}/unit-closure-selected-services.txt", output_dir),
        &array_strings(workflow, "unitClosureSelectedServices"),
    )?;
    Ok(())
}

fn workflow_env_names_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel workflow env-names <runtime-metadata-file> <workflow-id>"
                .to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let mut env_names = std::collections::BTreeSet::new();
    let mut seen_tasks = std::collections::BTreeSet::new();
    let mut seen_workflows = std::collections::BTreeSet::new();
    workflow_collect_env_names(
        &metadata,
        &values[1],
        &mut seen_tasks,
        &mut seen_workflows,
        &mut env_names,
    )?;
    for env_name in env_names {
        println!("{}", env_name);
    }
    Ok(())
}

fn workflow_collect_env_names(
    metadata: &JsonValue,
    workflow_id: &str,
    seen_tasks: &mut std::collections::BTreeSet<String>,
    seen_workflows: &mut std::collections::BTreeSet<String>,
    env_names: &mut std::collections::BTreeSet<String>,
) -> Result<(), String> {
    if workflow_id.is_empty() || !seen_workflows.insert(workflow_id.to_string()) {
        return Ok(());
    }

    let workflow = runtime_metadata_workflow(metadata, workflow_id)?;
    for task_id in workflow_phase_tasks(workflow, "preRun") {
        workflow_collect_task_env_names(metadata, &task_id, seen_tasks, seen_workflows, env_names)?;
    }

    if let Some(plan) = object_field(workflow, "plan").and_then(JsonValue::as_array) {
        for unit in plan {
            let task_id = object_string(unit, "taskId").unwrap_or("");
            if !task_id.is_empty() {
                workflow_collect_task_env_names(
                    metadata,
                    task_id,
                    seen_tasks,
                    seen_workflows,
                    env_names,
                )?;
            }
        }
    }

    for task_id in workflow_phase_tasks(workflow, "postRun") {
        workflow_collect_task_env_names(metadata, &task_id, seen_tasks, seen_workflows, env_names)?;
    }
    Ok(())
}

fn workflow_collect_task_env_names(
    metadata: &JsonValue,
    task_id: &str,
    seen_tasks: &mut std::collections::BTreeSet<String>,
    seen_workflows: &mut std::collections::BTreeSet<String>,
    env_names: &mut std::collections::BTreeSet<String>,
) -> Result<(), String> {
    if task_id.is_empty() || !seen_tasks.insert(task_id.to_string()) {
        return Ok(());
    }

    let task = runtime_metadata_task(metadata, task_id)?;
    env_names.extend(array_strings(task, "passThroughEnvNames"));

    if let Some(hooks) = object_field(task, "hooks") {
        for phase_key in ["pre", "post"] {
            if let Some(phase_hooks) = object_field(hooks, phase_key).and_then(JsonValue::as_object)
            {
                for hook in phase_hooks.values() {
                    env_names.extend(array_strings(hook, "passThroughEnvNames"));
                }
            }
        }
    }

    if let Some(deps) = object_field(task, "deps") {
        for dependency in array_strings(deps, "needs") {
            workflow_collect_task_env_names(
                metadata,
                &dependency,
                seen_tasks,
                seen_workflows,
                env_names,
            )?;
        }
        for dependency in array_strings(deps, "softNeeds") {
            workflow_collect_task_env_names(
                metadata,
                &dependency,
                seen_tasks,
                seen_workflows,
                env_names,
            )?;
        }
    }

    if let Some(runner) = object_field(task, "runner") {
        if object_string(runner, "type") == Some("workflowRef") {
            let nested_workflow_id = object_string(runner, "workflowId").unwrap_or("");
            if !nested_workflow_id.is_empty() {
                workflow_collect_env_names(
                    metadata,
                    nested_workflow_id,
                    seen_tasks,
                    seen_workflows,
                    env_names,
                )?;
            }
        }
    }

    Ok(())
}

#[derive(Clone)]
struct WorkflowPhaseServiceSetEntry {
    service_set_id: String,
    service_set_name: String,
    operation: String,
    selected_services_csv: String,
}

fn workflow_scheduler_from_runtime(
    workflow: &JsonValue,
) -> Result<WorkflowSchedulerWorkflow, String> {
    let units = object_field(workflow, "plan")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| "runtime workflow descriptor missing array field plan".to_string())?
        .iter()
        .map(|unit| {
            Ok(WorkflowSchedulerUnitPlan {
                name: required_string_field(unit, "name", "runtime workflow unit")?.to_string(),
                task_id: required_string_field(unit, "taskId", "runtime workflow unit")?
                    .to_string(),
                needs: array_strings(unit, "needs"),
                locks: array_strings(unit, "locks"),
                required_services: array_strings(unit, "requiredServices"),
                skip_if_missing_env: array_strings(unit, "skipIfMissingEnv"),
                when_env_present: object_field(unit, "when")
                    .map(|when| array_strings(when, "envPresent"))
                    .unwrap_or_default(),
                when_env_equals: object_field(unit, "when")
                    .map(|when| object_string_map(when, "envEquals", "runtime workflow unit"))
                    .transpose()?
                    .unwrap_or_default(),
                selected_services_csv: array_strings(unit, "selectedServices").join(","),
                produces_json: object_field(unit, "produces")
                    .map(render_json_compact)
                    .unwrap_or_else(|| "{}".to_string()),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(WorkflowSchedulerWorkflow { units })
}

fn workflow_phase_tasks(workflow: &JsonValue, phase_key: &str) -> Vec<String> {
    object_field(workflow, "phases")
        .and_then(|phases| object_field(phases, phase_key))
        .map(|phase| array_strings(phase, "tasks"))
        .unwrap_or_default()
}

fn workflow_phase_service_sets(
    workflow: &JsonValue,
    phase_key: &str,
) -> Vec<WorkflowPhaseServiceSetEntry> {
    object_field(workflow, "phases")
        .and_then(|phases| object_field(phases, phase_key))
        .and_then(|phase| object_field(phase, "serviceSets"))
        .and_then(JsonValue::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| WorkflowPhaseServiceSetEntry {
                    service_set_id: object_string(entry, "serviceSetId")
                        .unwrap_or("")
                        .to_string(),
                    service_set_name: object_string(entry, "serviceSetName")
                        .unwrap_or("")
                        .to_string(),
                    operation: object_string(entry, "operation").unwrap_or("").to_string(),
                    selected_services_csv: array_strings(entry, "selectedServices").join(","),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn workflow_task_mode_detail() -> JsonValue {
    json!({
        "kind": "slotLifecycle",
        "mode": "task",
    })
}

fn workflow_task_pass_detail(produces_json: &str) -> JsonValue {
    let produces = parse_json(produces_json).unwrap_or_else(|_| json!({}));
    json!({
        "kind": "slotLifecycle",
        "mode": "task",
        "produces": produces,
    })
}

fn workflow_exit_detail(mode: &str, exit_code: i64) -> JsonValue {
    json!({
        "kind": "slotLifecycle",
        "mode": mode,
        "exitCode": exit_code,
    })
}

fn workflow_cancel_detail(reason: &str, extra_key: &str, extra_value: &str) -> JsonValue {
    let mut detail = json!({
        "kind": "serviceLifecycle",
        "reason": reason,
    });
    if extra_key == "dependency" {
        if let Some(fields) = detail.as_object_mut() {
            fields.insert(
                "dependency".to_string(),
                JsonValue::String(extra_value.to_string()),
            );
        }
    } else if extra_key == "serviceName" {
        if let Some(fields) = detail.as_object_mut() {
            fields.insert(
                "serviceName".to_string(),
                JsonValue::String(extra_value.to_string()),
            );
        }
    } else if extra_key == "missing" {
        if let Some(fields) = detail.as_object_mut() {
            fields.insert(
                "missing".to_string(),
                JsonValue::String(extra_value.to_string()),
            );
        }
    }
    detail
}

fn workflow_phase_service_set_detail(
    phase_key: &str,
    service_set_id: &str,
    service_set_name: &str,
    operation: &str,
    exit_code: Option<i64>,
) -> JsonValue {
    let mut detail = json!({
        "kind": "serviceLifecycle",
        "eventType": phase_key,
        "service": service_set_name,
        "commandName": operation,
        "ownerScope": service_set_id,
    });
    if let Some(exit_code) = exit_code {
        if let Some(fields) = detail.as_object_mut() {
            fields.insert(
                "exitCode".to_string(),
                JsonValue::Number(Number::from(exit_code)),
            );
        }
    }
    detail
}

fn workflow_append_event(
    bundle_path: &str,
    registry_root: &str,
    run_id: &str,
    attempt_id: &str,
    workflow_id: &str,
    task_id: &str,
    state: &str,
    detail: &JsonValue,
) -> Result<(), String> {
    registry_append_event_internal(
        bundle_path,
        registry_root,
        run_id,
        attempt_id,
        workflow_id,
        task_id,
        state,
        detail,
    )?;
    Ok(())
}

fn workflow_spawn_task_adapter(
    task_adapter: &str,
    task_id: &str,
    workflow_id: &str,
    selected_services_csv: &str,
    passthrough_args: &[String],
) -> Result<std::process::Child, String> {
    let mut command = Command::new(task_adapter);
    command
        .arg(task_id)
        .arg(workflow_id)
        .arg(selected_services_csv)
        .arg("--");
    for arg in passthrough_args {
        command.arg(arg);
    }
    command
        .spawn()
        .map_err(|err| format!("failed to spawn task adapter '{}': {}", task_adapter, err))
}

fn workflow_run_task_adapter(
    task_adapter: &str,
    task_id: &str,
    workflow_id: &str,
    selected_services_csv: &str,
    passthrough_args: &[String],
) -> Result<i64, String> {
    let status = workflow_spawn_task_adapter(
        task_adapter,
        task_id,
        workflow_id,
        selected_services_csv,
        passthrough_args,
    )?
    .wait()
    .map_err(|err| format!("failed to wait for task adapter '{}': {}", task_id, err))?;
    Ok(status.code().unwrap_or(1) as i64)
}

fn workflow_run_service_set_adapter(
    service_set_adapter: &str,
    entry: &WorkflowPhaseServiceSetEntry,
) -> Result<i64, String> {
    let status = Command::new(service_set_adapter)
        .arg(&entry.service_set_id)
        .arg(&entry.operation)
        .arg(&entry.selected_services_csv)
        .status()
        .map_err(|err| {
            format!(
                "failed to spawn service-set adapter '{}:{}': {}",
                entry.service_set_id, entry.operation, err
            )
        })?;
    Ok(status.code().unwrap_or(1) as i64)
}

fn workflow_send_signal(pid: u32, signal_name: &str) {
    let _ = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("kill -{} {}", signal_name, pid))
        .status();
}

fn workflow_wait_for_any_child(
    running: &mut BTreeMap<String, WorkflowRunningUnit>,
) -> Result<Option<(String, i64)>, String> {
    loop {
        let unit_names = running.keys().cloned().collect::<Vec<_>>();
        for unit_name in unit_names {
            let Some(unit) = running.get_mut(&unit_name) else {
                continue;
            };
            match unit.child.try_wait() {
                Ok(Some(status)) => {
                    let exit_code = status.code().unwrap_or(1) as i64;
                    return Ok(Some((unit_name, exit_code)));
                }
                Ok(None) => {}
                Err(err) => {
                    return Err(format!(
                        "failed to query workflow child status unit='{}': {}",
                        unit_name, err
                    ))
                }
            }
        }
        if running.is_empty() {
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

struct WorkflowRunningUnit {
    task_id: String,
    produces_json: String,
    child: std::process::Child,
    cancel_requested: bool,
}

fn workflow_first_skipped_required_service(
    metadata: &JsonValue,
    task_id: &str,
    skipped_services: &BTreeSet<String>,
) -> Option<String> {
    let task = runtime_metadata_task(metadata, task_id).ok()?;
    array_strings(task, "requiredServices")
        .into_iter()
        .find(|service_name| skipped_services.contains(service_name))
}

fn workflow_phase_task_selected_services_csv(workflow: &JsonValue, task_id: &str) -> String {
    match task_id {
        "task.ops.ready" | "task.ops.health" => {
            array_strings(workflow, "unitClosureSelectedServices").join(",")
        }
        _ => String::new(),
    }
}

fn workflow_run_phase(
    metadata: &JsonValue,
    bundle_path: &str,
    registry_root: &str,
    run_id: &str,
    attempt_id: &str,
    workflow_id: &str,
    workflow: &JsonValue,
    skipped_services: &BTreeSet<String>,
    task_adapter: &str,
    service_set_adapter: &str,
    phase_key: &str,
    passthrough_args: &[String],
) -> Result<i64, String> {
    let task_phase_first = phase_key == "postRun";

    let run_phase_tasks = |phase_tasks: Vec<String>| -> Result<i64, String> {
        for phase_task in phase_tasks {
            if let Some(service_name) =
                workflow_first_skipped_required_service(metadata, &phase_task, skipped_services)
            {
                workflow_append_event(
                    bundle_path,
                    registry_root,
                    run_id,
                    attempt_id,
                    workflow_id,
                    &phase_task,
                    "canceled",
                    &workflow_cancel_detail("service-skipped", "serviceName", &service_name),
                )?;
                continue;
            }

            workflow_append_event(
                bundle_path,
                registry_root,
                run_id,
                attempt_id,
                workflow_id,
                &phase_task,
                "queued",
                &workflow_task_mode_detail(),
            )?;
            workflow_append_event(
                bundle_path,
                registry_root,
                run_id,
                attempt_id,
                workflow_id,
                &phase_task,
                "running",
                &workflow_task_mode_detail(),
            )?;
            let selected_services_csv =
                workflow_phase_task_selected_services_csv(workflow, &phase_task);
            let exit_code = workflow_run_task_adapter(
                task_adapter,
                &phase_task,
                workflow_id,
                &selected_services_csv,
                passthrough_args,
            )?;
            if exit_code == 0 {
                let produces_json = runtime_metadata_task(metadata, &phase_task)
                    .ok()
                    .and_then(|task| object_field(task, "produces"))
                    .map(render_json_compact)
                    .unwrap_or_else(|| "{}".to_string());
                workflow_append_event(
                    bundle_path,
                    registry_root,
                    run_id,
                    attempt_id,
                    workflow_id,
                    &phase_task,
                    "passed",
                    &workflow_task_pass_detail(&produces_json),
                )?;
            } else {
                workflow_append_event(
                    bundle_path,
                    registry_root,
                    run_id,
                    attempt_id,
                    workflow_id,
                    &phase_task,
                    "failed",
                    &workflow_exit_detail("task", exit_code),
                )?;
                return Ok(exit_code);
            }
        }
        Ok(0)
    };

    let run_phase_service_sets =
        |entries: Vec<WorkflowPhaseServiceSetEntry>| -> Result<i64, String> {
            for entry in entries {
                let phase_entry_id = format!("{}:{}", entry.service_set_id, entry.operation);
                let detail = workflow_phase_service_set_detail(
                    phase_key,
                    &entry.service_set_id,
                    &entry.service_set_name,
                    &entry.operation,
                    None,
                );
                workflow_append_event(
                    bundle_path,
                    registry_root,
                    run_id,
                    attempt_id,
                    workflow_id,
                    &phase_entry_id,
                    "queued",
                    &detail,
                )?;
                workflow_append_event(
                    bundle_path,
                    registry_root,
                    run_id,
                    attempt_id,
                    workflow_id,
                    &phase_entry_id,
                    "running",
                    &detail,
                )?;
                let exit_code = workflow_run_service_set_adapter(service_set_adapter, &entry)?;
                if exit_code == 0 {
                    workflow_append_event(
                        bundle_path,
                        registry_root,
                        run_id,
                        attempt_id,
                        workflow_id,
                        &phase_entry_id,
                        "passed",
                        &detail,
                    )?;
                } else {
                    workflow_append_event(
                        bundle_path,
                        registry_root,
                        run_id,
                        attempt_id,
                        workflow_id,
                        &phase_entry_id,
                        "failed",
                        &workflow_phase_service_set_detail(
                            phase_key,
                            &entry.service_set_id,
                            &entry.service_set_name,
                            &entry.operation,
                            Some(exit_code),
                        ),
                    )?;
                    return Ok(exit_code);
                }
            }
            Ok(0)
        };

    if task_phase_first {
        let status = run_phase_tasks(workflow_phase_tasks(workflow, phase_key))?;
        if status != 0 {
            return Ok(status);
        }
        run_phase_service_sets(workflow_phase_service_sets(workflow, phase_key))
    } else {
        let status = run_phase_service_sets(workflow_phase_service_sets(workflow, phase_key))?;
        if status != 0 {
            return Ok(status);
        }
        run_phase_tasks(workflow_phase_tasks(workflow, phase_key))
    }
}

fn workflow_run_serial(
    bundle_path: &str,
    registry_root: &str,
    run_id: &str,
    attempt_id: &str,
    workflow_id: &str,
    workflow_plan: &WorkflowSchedulerWorkflow,
    fail_fast: bool,
    skipped_services: &BTreeSet<String>,
    task_adapter: &str,
    passthrough_args: &[String],
) -> Result<i64, String> {
    let mut state = build_workflow_serial_state(workflow_plan, fail_fast, skipped_services);

    loop {
        match workflow_serial_next_action(&state) {
            WorkflowSerialAction::Execute {
                unit_name,
                task_id,
                selected_services_csv,
            } => {
                workflow_append_event(
                    bundle_path,
                    registry_root,
                    run_id,
                    attempt_id,
                    workflow_id,
                    &task_id,
                    "queued",
                    &workflow_task_mode_detail(),
                )?;
                workflow_append_event(
                    bundle_path,
                    registry_root,
                    run_id,
                    attempt_id,
                    workflow_id,
                    &task_id,
                    "running",
                    &workflow_task_mode_detail(),
                )?;
                let exit_code = workflow_run_task_adapter(
                    task_adapter,
                    &task_id,
                    workflow_id,
                    &selected_services_csv,
                    passthrough_args,
                )?;
                if exit_code == 0 {
                    let produces_json = workflow_plan
                        .units
                        .iter()
                        .find(|unit| unit.name == unit_name)
                        .map(|unit| unit.produces_json.clone())
                        .unwrap_or_else(|| "{}".to_string());
                    workflow_append_event(
                        bundle_path,
                        registry_root,
                        run_id,
                        attempt_id,
                        workflow_id,
                        &task_id,
                        "passed",
                        &workflow_task_pass_detail(&produces_json),
                    )?;
                    workflow_serial_transition(&mut state, &unit_name, "passed", "0", "", "", "")?;
                } else {
                    workflow_append_event(
                        bundle_path,
                        registry_root,
                        run_id,
                        attempt_id,
                        workflow_id,
                        &task_id,
                        "failed",
                        &workflow_exit_detail("task", exit_code),
                    )?;
                    workflow_serial_transition(
                        &mut state,
                        &unit_name,
                        "failed",
                        &exit_code.to_string(),
                        "",
                        "",
                        "",
                    )?;
                }
            }
            WorkflowSerialAction::Cancel {
                unit_name,
                task_id,
                reason,
                extra_key,
                extra_value,
            } => {
                workflow_append_event(
                    bundle_path,
                    registry_root,
                    run_id,
                    attempt_id,
                    workflow_id,
                    &task_id,
                    "canceled",
                    &workflow_cancel_detail(&reason, &extra_key, &extra_value),
                )?;
                workflow_serial_transition(
                    &mut state,
                    &unit_name,
                    "canceled",
                    "",
                    &reason,
                    &extra_key,
                    &extra_value,
                )?;
            }
            WorkflowSerialAction::Done { workflow_status } => return Ok(workflow_status),
        }
    }
}

fn workflow_run_parallel(
    bundle_path: &str,
    registry_root: &str,
    run_id: &str,
    attempt_id: &str,
    workflow_id: &str,
    workflow_plan: &WorkflowSchedulerWorkflow,
    fail_fast: bool,
    max_workers: i64,
    skipped_services: &BTreeSet<String>,
    task_adapter: &str,
    passthrough_args: &[String],
) -> Result<i64, String> {
    let mut state =
        build_workflow_parallel_state(workflow_plan, fail_fast, max_workers, skipped_services);
    let mut running = BTreeMap::<String, WorkflowRunningUnit>::new();

    loop {
        loop {
            match workflow_parallel_next_action(&mut state) {
                WorkflowParallelAction::Start {
                    unit_name,
                    task_id,
                    selected_services_csv,
                    produces_json,
                } => {
                    workflow_append_event(
                        bundle_path,
                        registry_root,
                        run_id,
                        attempt_id,
                        workflow_id,
                        &task_id,
                        "queued",
                        &workflow_task_mode_detail(),
                    )?;
                    workflow_append_event(
                        bundle_path,
                        registry_root,
                        run_id,
                        attempt_id,
                        workflow_id,
                        &task_id,
                        "running",
                        &workflow_task_mode_detail(),
                    )?;
                    let child = workflow_spawn_task_adapter(
                        task_adapter,
                        &task_id,
                        workflow_id,
                        &selected_services_csv,
                        passthrough_args,
                    )?;
                    workflow_parallel_transition(
                        &mut state, &unit_name, "started", "", "", "", "",
                    )?;
                    running.insert(
                        unit_name,
                        WorkflowRunningUnit {
                            task_id,
                            produces_json,
                            child,
                            cancel_requested: false,
                        },
                    );
                }
                WorkflowParallelAction::Cancel {
                    unit_name,
                    task_id,
                    reason,
                    extra_key,
                    extra_value,
                } => {
                    workflow_append_event(
                        bundle_path,
                        registry_root,
                        run_id,
                        attempt_id,
                        workflow_id,
                        &task_id,
                        "canceled",
                        &workflow_cancel_detail(&reason, &extra_key, &extra_value),
                    )?;
                    workflow_parallel_transition(
                        &mut state,
                        &unit_name,
                        "canceled",
                        "",
                        &reason,
                        &extra_key,
                        &extra_value,
                    )?;
                }
                WorkflowParallelAction::SignalRunning { unit_name } => {
                    if let Some(unit) = running.get_mut(&unit_name) {
                        workflow_send_signal(unit.child.id(), "TERM");
                        unit.cancel_requested = true;
                    }
                    workflow_parallel_transition(
                        &mut state,
                        &unit_name,
                        "signal-sent",
                        "",
                        "",
                        "",
                        "",
                    )?;
                }
                WorkflowParallelAction::Wait => break,
                WorkflowParallelAction::Done { workflow_status } => return Ok(workflow_status),
            }
        }

        let Some((done_unit, exit_code)) = workflow_wait_for_any_child(&mut running)? else {
            return Err("parallel workflow reached wait state without running units".to_string());
        };
        let Some(mut unit) = running.remove(&done_unit) else {
            continue;
        };
        if unit.cancel_requested {
            workflow_append_event(
                bundle_path,
                registry_root,
                run_id,
                attempt_id,
                workflow_id,
                &unit.task_id,
                "canceled",
                &workflow_cancel_detail("fail-fast-running", "", ""),
            )?;
            workflow_parallel_transition(
                &mut state,
                &done_unit,
                "canceled-running",
                &exit_code.to_string(),
                "fail-fast-running",
                "",
                "",
            )?;
        } else if exit_code == 0 {
            workflow_append_event(
                bundle_path,
                registry_root,
                run_id,
                attempt_id,
                workflow_id,
                &unit.task_id,
                "passed",
                &workflow_task_pass_detail(&unit.produces_json),
            )?;
            workflow_parallel_transition(&mut state, &done_unit, "passed", "0", "", "", "")?;
        } else {
            workflow_append_event(
                bundle_path,
                registry_root,
                run_id,
                attempt_id,
                workflow_id,
                &unit.task_id,
                "failed",
                &workflow_exit_detail("task", exit_code),
            )?;
            workflow_parallel_transition(
                &mut state,
                &done_unit,
                "failed",
                &exit_code.to_string(),
                "",
                "",
                "",
            )?;
        }
        let _ = unit.child.wait();
    }
}

fn workflow_run_command(values: &[String]) -> Result<(), String> {
    if values.len() < 11 {
        return Err(
            "usage: nixfied-kernel workflow run <runtime-metadata-file> <bundle-file> <registry-root> <run-id> <attempt-id|empty> <workflow-id> <skipped-services-file> <task-adapter> <service-set-adapter> <run-parallel> <max-workers> [-- <args...>]"
                .to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let workflow = runtime_metadata_workflow(&metadata, &values[5])?;
    let workflow_plan = workflow_scheduler_from_runtime(workflow)?;
    let skipped_services = load_line_set(&values[6])?;
    let task_adapter = &values[7];
    let service_set_adapter = &values[8];
    let run_parallel = parse_bool_flag(&values[9])?;
    let max_workers = parse_i64_text(&values[10], "workflow run max-workers")?;
    let passthrough_args = strip_passthrough_separator(&values[11..]).to_vec();
    let fail_fast = object_bool(workflow, "failFast").unwrap_or(false);

    workflow_append_event(
        &values[1],
        &values[2],
        &values[3],
        &values[4],
        &values[5],
        "",
        "queued",
        &json!({
            "kind": "slotLifecycle",
            "mode": "workflow",
        }),
    )?;

    let mut workflow_status = workflow_run_phase(
        &metadata,
        &values[1],
        &values[2],
        &values[3],
        &values[4],
        &values[5],
        workflow,
        &skipped_services,
        task_adapter,
        service_set_adapter,
        "preRun",
        &passthrough_args,
    )?;

    if workflow_status == 0 {
        workflow_status = if run_parallel {
            workflow_run_parallel(
                &values[1],
                &values[2],
                &values[3],
                &values[4],
                &values[5],
                &workflow_plan,
                fail_fast,
                max_workers,
                &skipped_services,
                task_adapter,
                &passthrough_args,
            )?
        } else {
            workflow_run_serial(
                &values[1],
                &values[2],
                &values[3],
                &values[4],
                &values[5],
                &workflow_plan,
                fail_fast,
                &skipped_services,
                task_adapter,
                &passthrough_args,
            )?
        };
    }

    if workflow_status == 0 || object_bool(workflow, "postRunAlways").unwrap_or(false) {
        let post_status = workflow_run_phase(
            &metadata,
            &values[1],
            &values[2],
            &values[3],
            &values[4],
            &values[5],
            workflow,
            &skipped_services,
            task_adapter,
            service_set_adapter,
            "postRun",
            &passthrough_args,
        )?;
        if workflow_status == 0 && post_status != 0 {
            workflow_status = post_status;
        }
    }

    if workflow_status == 0 {
        workflow_append_event(
            &values[1],
            &values[2],
            &values[3],
            &values[4],
            &values[5],
            "",
            "passed",
            &json!({
                "kind": "slotLifecycle",
                "mode": "workflow",
            }),
        )?;
    } else {
        workflow_append_event(
            &values[1],
            &values[2],
            &values[3],
            &values[4],
            &values[5],
            "",
            "failed",
            &workflow_exit_detail("workflow", workflow_status),
        )?;
    }

    process::exit(workflow_status.clamp(0, 255) as i32)
}

pub(crate) fn build_workflow_serial_state(
    workflow: &WorkflowSchedulerWorkflow,
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

pub(crate) fn build_workflow_parallel_state(
    workflow: &WorkflowSchedulerWorkflow,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn workflow_unit(name: &str, task_id: &str) -> WorkflowSchedulerUnitPlan {
        WorkflowSchedulerUnitPlan {
            name: name.to_string(),
            task_id: task_id.to_string(),
            needs: Vec::new(),
            locks: Vec::new(),
            required_services: Vec::new(),
            skip_if_missing_env: Vec::new(),
            when_env_present: Vec::new(),
            when_env_equals: BTreeMap::new(),
            selected_services_csv: String::new(),
            produces_json: String::new(),
        }
    }

    #[test]
    fn workflow_serial_skip_propagates_dependency_skips() {
        let mut root = workflow_unit("root", "task.root");
        root.required_services = vec!["postgres".to_string()];

        let mut child = workflow_unit("child", "task.child");
        child.needs = vec!["root".to_string()];

        let mut tail = workflow_unit("tail", "task.tail");
        tail.needs = vec!["child".to_string()];

        let workflow = WorkflowSchedulerWorkflow {
            units: vec![root, child, tail],
        };
        let skipped_services = BTreeSet::from(["postgres".to_string()]);
        let mut state = build_workflow_serial_state(&workflow, false, &skipped_services);

        assert_eq!(state.units["root"].state, "cancel-pending");
        assert_eq!(state.units["root"].cancel_reason, "service-skipped");
        assert_eq!(state.units["root"].cancel_extra_key, "serviceName");
        assert_eq!(state.units["root"].cancel_extra_value, "postgres");
        assert_eq!(state.units["child"].state, "cancel-pending");
        assert_eq!(state.units["child"].cancel_reason, "dependency-skipped");
        assert_eq!(state.units["child"].cancel_extra_value, "task.root");
        assert_eq!(state.units["tail"].state, "cancel-pending");
        assert_eq!(state.units["tail"].cancel_reason, "dependency-skipped");

        match workflow_serial_next_action(&state) {
            WorkflowSerialAction::Cancel {
                unit_name, reason, ..
            } => {
                assert_eq!(unit_name, "root");
                assert_eq!(reason, "service-skipped");
            }
            other => panic!("expected root cancel action, got {:?}", action_name(&other)),
        }

        workflow_serial_transition(&mut state, "root", "canceled", "", "", "", "")
            .expect("root cancel should succeed");
        workflow_serial_transition(&mut state, "child", "canceled", "", "", "", "")
            .expect("child cancel should succeed");
        workflow_serial_transition(&mut state, "tail", "canceled", "", "", "", "")
            .expect("tail cancel should succeed");

        match workflow_serial_next_action(&state) {
            WorkflowSerialAction::Done { workflow_status } => assert_eq!(workflow_status, 0),
            other => panic!("expected workflow to finish, got {:?}", action_name(&other)),
        }
    }

    #[test]
    fn workflow_parallel_lock_conflict_and_fail_fast() {
        let mut first = workflow_unit("first", "task.first");
        first.locks = vec!["db".to_string()];

        let mut second = workflow_unit("second", "task.second");
        second.locks = vec!["db".to_string()];

        let third = workflow_unit("third", "task.third");

        let workflow = WorkflowSchedulerWorkflow {
            units: vec![first, second, third],
        };
        let mut state = build_workflow_parallel_state(&workflow, true, 2, &BTreeSet::new());

        match workflow_parallel_next_action(&mut state) {
            WorkflowParallelAction::Start { unit_name, .. } => assert_eq!(unit_name, "first"),
            other => panic!(
                "expected first unit to start, got {:?}",
                action_name(&other)
            ),
        }
        workflow_parallel_transition(&mut state, "first", "started", "", "", "", "")
            .expect("first start should succeed");

        match workflow_parallel_next_action(&mut state) {
            WorkflowParallelAction::Start { unit_name, .. } => assert_eq!(unit_name, "third"),
            other => panic!(
                "expected third unit to start, got {:?}",
                action_name(&other)
            ),
        }
        workflow_parallel_transition(&mut state, "third", "started", "", "", "", "")
            .expect("third start should succeed");

        workflow_parallel_transition(&mut state, "first", "failed", "7", "", "", "")
            .expect("failure transition should succeed");

        assert_eq!(state.workflow_status, 7);
        assert!(state.stop_scheduling);
        assert_eq!(state.units["first"].state, "failed");
        assert_eq!(state.units["third"].state, "cancel-running-requested");
        assert_eq!(state.units["third"].cancel_reason, "fail-fast-running");
        assert_eq!(state.units["second"].state, "cancel-pending");
        assert_eq!(state.units["second"].cancel_reason, "fail-fast");

        match workflow_parallel_next_action(&mut state) {
            WorkflowParallelAction::SignalRunning { unit_name } => assert_eq!(unit_name, "third"),
            other => panic!(
                "expected running unit cancellation signal, got {:?}",
                action_name(&other)
            ),
        }
        workflow_parallel_transition(&mut state, "third", "signal-sent", "", "", "", "")
            .expect("signal-sent transition should succeed");
        workflow_parallel_transition(&mut state, "third", "canceled-running", "", "", "", "")
            .expect("canceled-running transition should succeed");

        match workflow_parallel_next_action(&mut state) {
            WorkflowParallelAction::Cancel {
                unit_name, reason, ..
            } => {
                assert_eq!(unit_name, "second");
                assert_eq!(reason, "fail-fast");
            }
            other => panic!(
                "expected pending unit cancellation, got {:?}",
                action_name(&other)
            ),
        }
    }

    fn action_name(action: &impl std::fmt::Debug) -> String {
        format!("{action:?}")
    }
}
