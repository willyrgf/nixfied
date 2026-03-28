use super::*;
use crate::registry::registry_append_event_internal;
use crate::runtime_metadata::{
    load_runtime_metadata, runtime_metadata_task, runtime_metadata_tasks,
};

use std::collections::{BTreeMap, BTreeSet};
use std::process::{self, Command};

pub(crate) fn task_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "run" => task_run_command(values),
        "validate-args" => task_validate_args_command(values),
        other => Err(format!("unknown task subcommand: {}", other)),
    }
}

#[derive(Clone, Copy)]
enum TaskVisitKind {
    Passed,
    ServiceSkipped,
    Failed,
}

#[derive(Clone, Copy)]
struct TaskVisitOutcome {
    exit_code: i64,
    kind: TaskVisitKind,
}

impl TaskVisitOutcome {
    fn passed() -> Self {
        Self {
            exit_code: 0,
            kind: TaskVisitKind::Passed,
        }
    }

    fn service_skipped() -> Self {
        Self {
            exit_code: 3,
            kind: TaskVisitKind::ServiceSkipped,
        }
    }

    fn failed(exit_code: i64) -> Self {
        Self {
            exit_code,
            kind: TaskVisitKind::Failed,
        }
    }
}

struct TaskRunContext<'a> {
    metadata: &'a JsonValue,
    bundle_path: &'a str,
    registry_root: &'a str,
    run_id: &'a str,
    attempt_id: &'a str,
    skipped_services: &'a BTreeSet<String>,
    task_adapter: &'a str,
    passthrough_args: &'a [String],
}

fn task_run_command(values: &[String]) -> Result<(), String> {
    if values.len() < 8 {
        return Err(
            "usage: nixfied-kernel task run <runtime-metadata-file> <bundle-file> <registry-root> <run-id> <attempt-id|empty> <task-id> <skipped-services-file> <task-adapter> [-- <args...>]"
                .to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let plan = load_task_dependency_plan(&metadata)?;
    let skipped_services = load_line_set(&values[6])?;
    let passthrough_args = strip_passthrough_separator(&values[8..]).to_vec();

    let context = TaskRunContext {
        metadata: &metadata,
        bundle_path: &values[1],
        registry_root: &values[2],
        run_id: &values[3],
        attempt_id: &values[4],
        skipped_services: &skipped_services,
        task_adapter: &values[7],
        passthrough_args: &passthrough_args,
    };

    let mut active = BTreeSet::new();
    let mut results = BTreeMap::new();
    let outcome = task_run_visit(&context, &plan, &values[5], &mut active, &mut results)?;
    let exit_code = match outcome.kind {
        TaskVisitKind::ServiceSkipped => 0,
        TaskVisitKind::Passed => 0,
        TaskVisitKind::Failed => outcome.exit_code,
    };

    process::exit(exit_code.clamp(0, 255) as i32)
}

fn task_validate_args_command(values: &[String]) -> Result<(), String> {
    if values.len() < 2 {
        return Err(
            "usage: nixfied-kernel task validate-args <runtime-metadata-file> <task-id> [-- <args...>]"
                .to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let task = runtime_metadata_task(&metadata, &values[1])?;
    let parser = object_string(task, "parser").unwrap_or("typed");
    let allow_unknown = object_bool(task, "allowUnknown").unwrap_or(false);
    if parser != "typed" || allow_unknown {
        println!("OK: task validate-args");
        return Ok(());
    }

    let args = strip_passthrough_separator(&values[2..]);
    validate_typed_task_args(task, args)?;
    println!("OK: task validate-args");
    Ok(())
}

fn load_task_dependency_plan(metadata: &JsonValue) -> Result<TaskDependencyPlan, String> {
    let mut tasks = BTreeMap::new();
    for (task_id, task_value) in runtime_metadata_tasks(metadata)? {
        let deps = object_field(task_value, "deps")
            .ok_or_else(|| format!("runtime task '{}' missing object field deps", task_id))?;
        tasks.insert(
            task_id.clone(),
            TaskDependencyEntry {
                needs: array_strings(deps, "needs"),
                soft_needs: array_strings(deps, "softNeeds"),
                required_services: array_strings(task_value, "requiredServices"),
            },
        );
    }
    Ok(TaskDependencyPlan { tasks })
}

fn validate_typed_task_args(task: &JsonValue, args: &[String]) -> Result<(), String> {
    let has_positional = object_bool(task, "hasPositional").unwrap_or(false);
    let long_kinds = object_field(task, "args")
        .and_then(|args_meta| object_field(args_meta, "longKinds"))
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    let short_kinds = object_field(task, "args")
        .and_then(|args_meta| object_field(args_meta, "shortKinds"))
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();

    let mut parse_options = true;
    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        index += 1;

        if !parse_options {
            continue;
        }

        if arg == "--" {
            parse_options = false;
            continue;
        }

        match arg.as_str() {
            "--help" | "-h" => continue,
            _ => {}
        }

        if let Some((key, _)) = arg.split_once('=') {
            if key.starts_with("--") {
                if matches!(key, "--run-id-file" | "--summary-file") {
                    continue;
                }
                let kind = long_kinds
                    .get(key)
                    .and_then(JsonValue::as_str)
                    .unwrap_or("");
                if kind == "option" || kind == "flag" {
                    continue;
                }
                return Err(format!("unknown option '{}'", key));
            }
        }

        if arg.starts_with("--") {
            if matches!(arg.as_str(), "--run-id-file" | "--summary-file") {
                if index >= args.len() {
                    return Err(format!("option '{}' requires a value", arg));
                }
                index += 1;
                continue;
            }
            let kind = long_kinds
                .get(arg)
                .and_then(JsonValue::as_str)
                .unwrap_or("");
            if kind == "flag" {
                continue;
            }
            if kind == "option" {
                if index >= args.len() {
                    return Err(format!("option '{}' requires a value", arg));
                }
                index += 1;
                continue;
            }
            return Err(format!("unknown option '{}'", arg));
        }

        if arg.starts_with('-') && arg.len() > 1 {
            if arg.len() != 2 {
                return Err(format!("unknown option '{}'", arg));
            }
            let kind = short_kinds
                .get(arg)
                .and_then(JsonValue::as_str)
                .unwrap_or("");
            if kind == "flag" {
                continue;
            }
            if kind == "option" {
                if index >= args.len() {
                    return Err(format!("option '{}' requires a value", arg));
                }
                index += 1;
                continue;
            }
            return Err(format!("unknown option '{}'", arg));
        }

        if !has_positional {
            return Err(format!("unexpected positional argument '{}'", arg));
        }
    }

    Ok(())
}

fn task_run_visit(
    context: &TaskRunContext,
    plan: &TaskDependencyPlan,
    task_id: &str,
    active: &mut BTreeSet<String>,
    results: &mut BTreeMap<String, TaskVisitOutcome>,
) -> Result<TaskVisitOutcome, String> {
    if let Some(outcome) = results.get(task_id) {
        return Ok(*outcome);
    }
    if active.contains(task_id) {
        return Err(format!("cyclic task dependency detected at '{}'", task_id));
    }

    let task_plan = plan
        .tasks
        .get(task_id)
        .ok_or_else(|| format!("unknown task '{}'", task_id))?;
    let task = runtime_metadata_task(context.metadata, task_id)?;
    active.insert(task_id.to_string());

    if let Some(skip_service) = task_plan
        .required_services
        .iter()
        .find(|service_name| context.skipped_services.contains(*service_name))
    {
        println!(
            "SKIP: task '{}' is skipped because service '{}' has a skip flag enabled",
            task_id, skip_service
        );
        task_append_event(
            context,
            task_id,
            "canceled",
            &task_cancel_detail("service-skipped", "serviceName", skip_service),
        )?;
        let outcome = TaskVisitOutcome::service_skipped();
        active.remove(task_id);
        results.insert(task_id.to_string(), outcome);
        return Ok(outcome);
    }

    for dependency in &task_plan.needs {
        let dependency_outcome = task_run_visit(context, plan, dependency, active, results)?;
        if dependency_outcome.exit_code != 0 {
            let outcome = TaskVisitOutcome::failed(dependency_outcome.exit_code);
            active.remove(task_id);
            results.insert(task_id.to_string(), outcome);
            return Ok(outcome);
        }
    }

    for dependency in &task_plan.soft_needs {
        if !plan.tasks.contains_key(dependency) {
            println!(
                "WARN: task '{}' soft dependency '{}' is not defined",
                task_id, dependency
            );
            continue;
        }

        let dependency_outcome = task_run_visit(context, plan, dependency, active, results)?;
        if dependency_outcome.exit_code != 0 {
            println!(
                "WARN: task '{}' soft dependency '{}' failed exitCode={}",
                task_id, dependency, dependency_outcome.exit_code
            );
        }
    }

    task_append_event(context, task_id, "running", &task_mode_detail())?;
    let exit_code = task_run_task_adapter(context.task_adapter, task_id, context.passthrough_args)?;
    let outcome = if exit_code == 0 {
        task_append_event(context, task_id, "passed", &task_pass_detail(task))?;
        TaskVisitOutcome::passed()
    } else {
        task_append_event(context, task_id, "failed", &task_exit_detail(exit_code))?;
        TaskVisitOutcome::failed(exit_code)
    };

    active.remove(task_id);
    results.insert(task_id.to_string(), outcome);
    Ok(outcome)
}

fn task_run_task_adapter(
    task_adapter: &str,
    task_id: &str,
    passthrough_args: &[String],
) -> Result<i64, String> {
    let mut command = Command::new(task_adapter);
    command.arg(task_id).arg("--");
    for arg in passthrough_args {
        command.arg(arg);
    }
    let status = command
        .status()
        .map_err(|err| format!("failed to run task adapter '{}': {}", task_adapter, err))?;
    Ok(status.code().unwrap_or(1) as i64)
}

fn task_append_event(
    context: &TaskRunContext,
    task_id: &str,
    state: &str,
    detail: &JsonValue,
) -> Result<(), String> {
    registry_append_event_internal(
        context.bundle_path,
        context.registry_root,
        context.run_id,
        context.attempt_id,
        "",
        task_id,
        state,
        detail,
    )?;
    Ok(())
}

fn task_mode_detail() -> JsonValue {
    json!({
        "kind": "slotLifecycle",
        "mode": "task",
    })
}

fn task_pass_detail(task: &JsonValue) -> JsonValue {
    let produces = object_field(task, "produces")
        .cloned()
        .unwrap_or_else(|| json!({}));
    json!({
        "kind": "slotLifecycle",
        "mode": "task",
        "produces": produces,
    })
}

fn task_exit_detail(exit_code: i64) -> JsonValue {
    json!({
        "kind": "slotLifecycle",
        "mode": "task",
        "exitCode": exit_code,
    })
}

fn task_cancel_detail(reason: &str, extra_key: &str, extra_value: &str) -> JsonValue {
    let mut detail = json!({
        "kind": "serviceLifecycle",
        "reason": reason,
    });
    if extra_key == "serviceName" {
        if let Some(fields) = detail.as_object_mut() {
            fields.insert(
                "serviceName".to_string(),
                JsonValue::String(extra_value.to_string()),
            );
        }
    }
    detail
}
