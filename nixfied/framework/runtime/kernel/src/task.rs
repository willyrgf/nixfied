use super::*;
use crate::execution_metadata::{
    execution_task, execution_tasks, execution_workflow, load_execution_metadata,
};
use crate::registry::registry_append_event_internal;

use std::collections::{BTreeMap, BTreeSet};
use std::process::{self, Command};

pub(crate) fn task_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "export" => task_export_command(values),
        "help" => task_help_command(values),
        "runtime-plan" => task_runtime_plan_command(values),
        "base-closure-selected-services" => task_base_closure_selected_services_command(values),
        "selected-services-csv" => task_selected_services_csv_command(values),
        "retry-backoff-values" => task_retry_backoff_values_command(values),
        "hook-ids" => task_hook_ids_command(values),
        "hook-export" => task_hook_export_command(values),
        "env-names" => task_env_names_command(values),
        "run" => task_run_command(values),
        "validate-args" => task_validate_args_command(values),
        other => Err(format!("unknown task subcommand: {}", other)),
    }
}

#[derive(Clone, Copy, Debug)]
enum TaskVisitKind {
    Passed,
    ServiceSkipped,
    Failed,
}

#[derive(Clone, Copy, Debug)]
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
            "usage: nixfied-kernel task run <execution-source-file> <bundle-file> <registry-root> <run-id> <attempt-id|empty> <task-id> <skipped-services-file> <task-adapter> [-- <args...>]"
                .to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
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
            "usage: nixfied-kernel task validate-args <execution-source-file> <task-id> [-- <args...>]"
                .to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let task = execution_task(&metadata, &values[1])?;
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

fn task_exports(task_id: &str, task: &JsonValue) -> Result<Vec<(String, String)>, String> {
    let runner = object_field(task, "runner")
        .ok_or_else(|| format!("runtime task '{}' missing object field runner", task_id))?;
    let hooks = object_field(task, "hooks")
        .ok_or_else(|| format!("runtime task '{}' missing object field hooks", task_id))?;
    let produces = object_field(task, "produces")
        .cloned()
        .unwrap_or_else(|| json!({}));

    Ok(vec![
        ("NIXFIED_TASK_ID".to_string(), task_id.to_string()),
        (
            "NIXFIED_TASK_RUNNER_TYPE".to_string(),
            object_string(runner, "type").unwrap_or("shell").to_string(),
        ),
        (
            "NIXFIED_TASK_RUNNER_COMMAND".to_string(),
            object_string(runner, "command").unwrap_or("").to_string(),
        ),
        (
            "NIXFIED_TASK_RUNNER_PACKAGE".to_string(),
            object_string(runner, "package").unwrap_or("").to_string(),
        ),
        (
            "NIXFIED_TASK_RUNNER_WORKFLOW_ID".to_string(),
            object_string(runner, "workflowId")
                .unwrap_or("")
                .to_string(),
        ),
        (
            "NIXFIED_TASK_MAX_ATTEMPTS".to_string(),
            task.get("maxAttempts")
                .and_then(JsonValue::as_i64)
                .unwrap_or(1)
                .max(1)
                .to_string(),
        ),
        (
            "NIXFIED_TASK_HOOK_COUNT".to_string(),
            hooks
                .get("count")
                .and_then(JsonValue::as_i64)
                .unwrap_or(0)
                .max(0)
                .to_string(),
        ),
        (
            "NIXFIED_TASK_PRODUCES_JSON".to_string(),
            render_json_compact(&produces),
        ),
    ])
}

fn task_hook_exports(hook: &JsonValue) -> Vec<(String, String)> {
    vec![
        (
            "NIXFIED_TASK_HOOK_COMMAND".to_string(),
            object_string(hook, "command").unwrap_or("").to_string(),
        ),
        (
            "NIXFIED_TASK_HOOK_RUNTIME_PLAN_SHELL".to_string(),
            object_string(hook, "runtimePlanShell")
                .unwrap_or("")
                .to_string(),
        ),
    ]
}

fn task_hook_phase<'a>(hooks: &'a JsonValue, phase_key: &str) -> Result<&'a JsonValue, String> {
    object_field(hooks, phase_key)
        .ok_or_else(|| format!("runtime task hooks missing object field {}", phase_key))
}

fn task_hook_phase_ids_key(phase_key: &str) -> Result<&'static str, String> {
    match phase_key {
        "pre" => Ok("preIds"),
        "post" => Ok("postIds"),
        other => Err(format!("unsupported task hook phase '{}'", other)),
    }
}

fn task_export_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task export <execution-source-file> <task-id>".to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let task = execution_task(&metadata, &values[1])?;
    let exports = task_exports(&values[1], task)?;
    print!("{}", render_shell_exports(&exports));
    Ok(())
}

fn task_help_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task help <execution-source-file> <task-id>".to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let task = execution_task(&metadata, &values[1])?;
    let help = object_field(task, "help")
        .cloned()
        .unwrap_or_else(|| json!({}));
    print!("{}", render_lines(&array_strings(&help, "lines")));
    Ok(())
}

fn task_runtime_plan_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task runtime-plan <execution-source-file> <task-id>".to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let task = execution_task(&metadata, &values[1])?;
    print!("{}", object_string(task, "runtimePlanShell").unwrap_or(""));
    Ok(())
}

fn task_base_closure_selected_services_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task base-closure-selected-services <execution-source-file> <task-id>"
                .to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let task = execution_task(&metadata, &values[1])?;
    print!(
        "{}",
        render_lines(&array_strings(task, "baseClosureSelectedServices"))
    );
    Ok(())
}

fn task_selected_services_csv_command(values: &[String]) -> Result<(), String> {
    if values.len() < 2 {
        return Err(
            "usage: nixfied-kernel task selected-services-csv <execution-source-file> <task-id> [-- <args...>]"
                .to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let task = execution_task(&metadata, &values[1])?;
    let mut selected_services = BTreeSet::new();

    for service_name in array_strings(task, "baseClosureSelectedServices") {
        if !service_name.is_empty() {
            selected_services.insert(service_name);
        }
    }

    if let Some(runner) = object_field(task, "runner") {
        let workflow_id = object_string(runner, "workflowId").unwrap_or("");
        if !workflow_id.is_empty() {
            let resolved_workflow_id = crate::workflow::workflow_resolved_id_from_args_lenient(
                &metadata,
                workflow_id,
                strip_passthrough_separator(&values[2..]),
            )?;
            let workflow = execution_workflow(&metadata, &resolved_workflow_id)?;
            for service_name in array_strings(workflow, "unitClosureSelectedServices") {
                if !service_name.is_empty() {
                    selected_services.insert(service_name);
                }
            }
        }
    }

    let services_csv = selected_services.into_iter().collect::<Vec<_>>().join(",");
    print!("{}", services_csv);
    Ok(())
}

fn task_retry_backoff_values_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task retry-backoff-values <execution-source-file> <task-id>"
                .to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let task = execution_task(&metadata, &values[1])?;
    print!("{}", render_lines(&task_retry_backoff_values(task)));
    Ok(())
}

fn task_hook_ids_command(values: &[String]) -> Result<(), String> {
    if values.len() != 3 {
        return Err(
            "usage: nixfied-kernel task hook-ids <execution-source-file> <task-id> <pre|post>"
                .to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let task = execution_task(&metadata, &values[1])?;
    let hooks = object_field(task, "hooks")
        .ok_or_else(|| format!("runtime task '{}' missing object field hooks", values[1]))?;
    let ids_key = task_hook_phase_ids_key(&values[2])?;
    print!("{}", render_lines(&array_strings(hooks, ids_key)));
    Ok(())
}

fn task_hook_export_command(values: &[String]) -> Result<(), String> {
    if values.len() != 4 {
        return Err(
            "usage: nixfied-kernel task hook-export <execution-source-file> <task-id> <pre|post> <hook-id>"
                .to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let task = execution_task(&metadata, &values[1])?;
    let hooks = object_field(task, "hooks")
        .ok_or_else(|| format!("runtime task '{}' missing object field hooks", values[1]))?;
    let phase_hooks = task_hook_phase(hooks, &values[2])?
        .as_object()
        .cloned()
        .unwrap_or_default();
    let hook = phase_hooks.get(&values[3]).ok_or_else(|| {
        format!(
            "unknown task hook '{}:{}:{}'",
            values[1], values[2], values[3]
        )
    })?;
    print!("{}", render_shell_exports(&task_hook_exports(hook)));
    Ok(())
}

fn task_env_names_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task env-names <execution-source-file> <task-id>".to_string(),
        );
    }

    let metadata = load_execution_metadata(&values[0])?;
    let mut env_names = BTreeSet::new();
    let mut seen_tasks = BTreeSet::new();
    let mut seen_workflows = BTreeSet::new();
    collect_task_env_names(
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

fn task_retry_backoff_values(task: &JsonValue) -> Vec<String> {
    object_array(task, "retryBackoffValues")
        .unwrap_or(&[])
        .iter()
        .filter_map(json_value_to_plain_string)
        .collect()
}

fn collect_task_env_names(
    metadata: &JsonValue,
    task_id: &str,
    seen_tasks: &mut BTreeSet<String>,
    seen_workflows: &mut BTreeSet<String>,
    env_names: &mut BTreeSet<String>,
) -> Result<(), String> {
    if task_id.is_empty() || !seen_tasks.insert(task_id.to_string()) {
        return Ok(());
    }

    let task = execution_task(metadata, task_id)?;
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
            collect_task_env_names(metadata, &dependency, seen_tasks, seen_workflows, env_names)?;
        }
        for dependency in array_strings(deps, "softNeeds") {
            collect_task_env_names(metadata, &dependency, seen_tasks, seen_workflows, env_names)?;
        }
    }

    if let Some(runner) = object_field(task, "runner") {
        if object_string(runner, "type") == Some("workflowRef") {
            let workflow_id = object_string(runner, "workflowId").unwrap_or("");
            if !workflow_id.is_empty() {
                collect_workflow_env_names(
                    metadata,
                    workflow_id,
                    seen_tasks,
                    seen_workflows,
                    env_names,
                )?;
            }
        }
    }

    Ok(())
}

fn collect_workflow_env_names(
    metadata: &JsonValue,
    workflow_id: &str,
    seen_tasks: &mut BTreeSet<String>,
    seen_workflows: &mut BTreeSet<String>,
    env_names: &mut BTreeSet<String>,
) -> Result<(), String> {
    if workflow_id.is_empty() || !seen_workflows.insert(workflow_id.to_string()) {
        return Ok(());
    }

    let workflow = execution_workflow(metadata, workflow_id)?;
    for dependency in task_workflow_phase_tasks(workflow, "preRun") {
        collect_task_env_names(metadata, &dependency, seen_tasks, seen_workflows, env_names)?;
    }

    if let Some(plan) = object_field(workflow, "plan").and_then(JsonValue::as_array) {
        for unit in plan {
            let task_id = object_string(unit, "taskId").unwrap_or("");
            if !task_id.is_empty() {
                collect_task_env_names(metadata, task_id, seen_tasks, seen_workflows, env_names)?;
            }
        }
    }

    for dependency in task_workflow_phase_tasks(workflow, "postRun") {
        collect_task_env_names(metadata, &dependency, seen_tasks, seen_workflows, env_names)?;
    }

    Ok(())
}

fn task_workflow_phase_tasks(workflow: &JsonValue, phase_key: &str) -> Vec<String> {
    object_field(workflow, "phases")
        .and_then(|phases| object_field(phases, phase_key))
        .map(|phase| array_strings(phase, "tasks"))
        .unwrap_or_default()
}

fn load_task_dependency_plan(metadata: &JsonValue) -> Result<TaskDependencyPlan, String> {
    let mut tasks = BTreeMap::new();
    for (task_id, task_value) in execution_tasks(metadata)? {
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
    let task = execution_task(context.metadata, task_id)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct TaskRunFixture {
        root: PathBuf,
        bundle_path: PathBuf,
        registry_root: PathBuf,
        adapter_path: PathBuf,
        adapter_log_path: PathBuf,
    }

    impl TaskRunFixture {
        fn new(exit_codes: &[(&str, i64)]) -> Self {
            let root = unique_temp_dir("kernel-task-tests");
            let bundle_path = root.join("bundle.json");
            let registry_root = root.join("registry");
            let adapter_path = root.join("task-adapter.sh");
            let adapter_log_path = root.join("task-adapter.log");

            write_text(
                &bundle_path,
                &render_json_compact(&json!({
                    "definitions": {
                        "runtime.registryEvent": {
                            "type": "record",
                            "closed": false,
                            "fields": {}
                        }
                    }
                })),
            );
            fs::create_dir_all(&registry_root).expect("registry root should be created");
            write_task_adapter_script(&adapter_path, &adapter_log_path, exit_codes);

            Self {
                root,
                bundle_path,
                registry_root,
                adapter_path,
                adapter_log_path,
            }
        }

        fn context<'a>(
            &'a self,
            metadata: &'a JsonValue,
            skipped_services: &'a BTreeSet<String>,
        ) -> TaskRunContext<'a> {
            TaskRunContext {
                metadata,
                bundle_path: self
                    .bundle_path
                    .to_str()
                    .expect("bundle path should be utf-8"),
                registry_root: self
                    .registry_root
                    .to_str()
                    .expect("registry root should be utf-8"),
                run_id: "run-1",
                attempt_id: "attempt-1",
                skipped_services,
                task_adapter: self
                    .adapter_path
                    .to_str()
                    .expect("adapter path should be utf-8"),
                passthrough_args: &[],
            }
        }

        fn adapter_log_lines(&self) -> Vec<String> {
            read_lines(&self.adapter_log_path)
        }

        fn registry_index_lines(&self) -> Vec<String> {
            read_lines(&self.registry_root.join("events.index.tsv"))
        }
    }

    impl Drop for TaskRunFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let seq = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            nanos + seq as u128
        ));
        fs::create_dir_all(&path).expect("fixture temp dir should be created");
        path
    }

    fn write_text(path: &Path, contents: &str) {
        fs::write(path, contents).unwrap_or_else(|err| {
            panic!("failed to write {}: {}", path.display(), err);
        });
    }

    fn write_task_adapter_script(path: &Path, log_path: &Path, exit_codes: &[(&str, i64)]) {
        let mut script = String::from("#!/bin/sh\nset -eu\n");
        script.push_str(&format!(
            "printf '%s\\n' \"$1\" >> '{}'\n",
            log_path.display()
        ));
        script.push_str("case \"$1\" in\n");
        for (task_id, exit_code) in exit_codes {
            script.push_str(&format!("  '{}') exit {} ;;\n", task_id, exit_code));
        }
        script.push_str("  *) exit 0 ;;\n");
        script.push_str("esac\n");

        write_text(path, &script);
        let mut permissions = fs::metadata(path)
            .expect("adapter metadata should exist")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("adapter should be executable");
    }

    fn read_lines(path: &Path) -> Vec<String> {
        match fs::read_to_string(path) {
            Ok(contents) => contents
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<_>>(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(err) => panic!("failed to read {}: {}", path.display(), err),
        }
    }

    fn shell_task(needs: &[&str], soft_needs: &[&str], required_services: &[&str]) -> JsonValue {
        json!({
            "deps": {
                "needs": needs,
                "softNeeds": soft_needs,
            },
            "requiredServices": required_services,
            "produces": {},
        })
    }

    fn execution_metadata(tasks: &[(&str, JsonValue)]) -> JsonValue {
        let task_map = tasks
            .iter()
            .map(|(task_id, task)| ((*task_id).to_string(), task.clone()))
            .collect::<serde_json::Map<_, _>>();
        json!({
            "schema": {
                "kind": "nixfied-execution",
                "version": 1,
            },
            "tasks": {
                "byId": task_map,
            },
            "workflows": {
                "byId": {},
            },
        })
    }

    #[test]
    fn task_run_visit_detects_cycles_before_execution() {
        let metadata = execution_metadata(&[
            ("task.root", shell_task(&["task.dep"], &[], &[])),
            ("task.dep", shell_task(&["task.root"], &[], &[])),
        ]);
        let plan = load_task_dependency_plan(&metadata).expect("plan should load");
        let fixture = TaskRunFixture::new(&[]);
        let skipped_services = BTreeSet::new();
        let context = fixture.context(&metadata, &skipped_services);
        let mut active = BTreeSet::new();
        let mut results = BTreeMap::new();

        let err = task_run_visit(&context, &plan, "task.root", &mut active, &mut results)
            .expect_err("cyclic dependency should fail");

        assert!(err.contains("cyclic task dependency detected"));
        assert!(fixture.adapter_log_lines().is_empty());
        assert!(fixture.registry_index_lines().is_empty());
        assert!(results.is_empty());
    }

    #[test]
    fn task_run_visit_propagates_hard_dependency_failure() {
        let metadata = execution_metadata(&[
            ("task.root", shell_task(&["task.dep"], &[], &[])),
            ("task.dep", shell_task(&[], &[], &[])),
        ]);
        let plan = load_task_dependency_plan(&metadata).expect("plan should load");
        let fixture = TaskRunFixture::new(&[("task.dep", 17)]);
        let skipped_services = BTreeSet::new();
        let context = fixture.context(&metadata, &skipped_services);
        let mut active = BTreeSet::new();
        let mut results = BTreeMap::new();

        let outcome = task_run_visit(&context, &plan, "task.root", &mut active, &mut results)
            .expect("hard dependency failure should return an outcome");

        assert_eq!(outcome.exit_code, 17);
        assert!(matches!(outcome.kind, TaskVisitKind::Failed));
        assert_eq!(fixture.adapter_log_lines(), vec!["task.dep"]);
        assert_eq!(results["task.dep"].exit_code, 17);
        assert_eq!(results["task.root"].exit_code, 17);
        let index = fixture.registry_index_lines();
        assert_eq!(index.len(), 2);
        assert!(index[0].contains("\trun-1\tattempt-1\t\ttask.dep\trunning\t\t"));
        assert!(index[1].contains("\trun-1\tattempt-1\t\ttask.dep\tfailed\t\t17"));
    }

    #[test]
    fn task_run_visit_allows_soft_dependency_failure_and_runs_root() {
        let metadata = execution_metadata(&[
            ("task.root", shell_task(&[], &["task.soft"], &[])),
            ("task.soft", shell_task(&[], &[], &[])),
        ]);
        let plan = load_task_dependency_plan(&metadata).expect("plan should load");
        let fixture = TaskRunFixture::new(&[("task.soft", 9)]);
        let skipped_services = BTreeSet::new();
        let context = fixture.context(&metadata, &skipped_services);
        let mut active = BTreeSet::new();
        let mut results = BTreeMap::new();

        let outcome = task_run_visit(&context, &plan, "task.root", &mut active, &mut results)
            .expect("soft dependency failure should not fail the root task");

        assert_eq!(outcome.exit_code, 0);
        assert!(matches!(outcome.kind, TaskVisitKind::Passed));
        assert_eq!(fixture.adapter_log_lines(), vec!["task.soft", "task.root"]);
        assert_eq!(results["task.soft"].exit_code, 9);
        assert_eq!(results["task.root"].exit_code, 0);
        let index = fixture.registry_index_lines();
        assert_eq!(index.len(), 4);
        assert!(index[1].contains("\ttask.soft\tfailed\t\t9"));
        assert!(index[2].contains("\ttask.root\trunning\t\t"));
        assert!(index[3].contains("\ttask.root\tpassed\t\t"));
    }

    #[test]
    fn task_run_visit_skips_required_service_without_running_adapter() {
        let metadata =
            execution_metadata(&[("task.root", shell_task(&[], &[], &["service.redis"]))]);
        let plan = load_task_dependency_plan(&metadata).expect("plan should load");
        let fixture = TaskRunFixture::new(&[]);
        let skipped_services = BTreeSet::from(["service.redis".to_string()]);
        let context = fixture.context(&metadata, &skipped_services);
        let mut active = BTreeSet::new();
        let mut results = BTreeMap::new();

        let outcome = task_run_visit(&context, &plan, "task.root", &mut active, &mut results)
            .expect("service skip should return an outcome");

        assert_eq!(outcome.exit_code, 3);
        assert!(matches!(outcome.kind, TaskVisitKind::ServiceSkipped));
        assert!(fixture.adapter_log_lines().is_empty());
        assert_eq!(results["task.root"].exit_code, 3);
        let index = fixture.registry_index_lines();
        assert_eq!(index.len(), 1);
        assert!(index[0].contains("\ttask.root\tcanceled\tservice-skipped\t"));
    }
}
