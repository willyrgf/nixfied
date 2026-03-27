use super::*;
use crate::runtime_metadata::{
    load_runtime_metadata, runtime_metadata_task, runtime_metadata_tasks,
    runtime_task_runner_workflow_id,
};

pub(crate) fn task_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "execution-order" => task_execution_order_command(values),
        "exists" => task_exists_command(values),
        "workflow-ref" => task_workflow_ref_command(values),
        "validate-args" => task_validate_args_command(values),
        "render-help" => task_render_help_command(values),
        "load-runtime" => task_load_runtime_command(values),
        "load-hook" => task_load_hook_command(values),
        other => Err(format!("unknown task subcommand: {}", other)),
    }
}

fn task_execution_order_command(values: &[String]) -> Result<(), String> {
    if values.len() != 5 {
        return Err(
            "usage: nixfied-kernel task execution-order <runtime-metadata-file> <skipped-services-file> <task-id> <order-file> <export-file>"
                .to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let plan = load_task_dependency_plan(&metadata)?;
    let skipped_services = load_line_set(&values[1])?;
    let execution_plan = collect_task_execution_plan(&plan, &skipped_services, &values[2])?;
    write_task_execution_plan_file(&values[3], &execution_plan.steps)?;
    write_shell_exports(
        &values[4],
        &[(
            "TASK_EXECUTION_PLAN_SOFT_MISSING_LINES".to_string(),
            execution_plan.soft_missing_lines,
        )],
    )?;
    println!("OK: task execution-order");
    Ok(())
}

fn task_exists_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task exists <runtime-metadata-file> <task-id>".to_string(),
        );
    }
    let metadata = load_runtime_metadata(&values[0])?;
    runtime_metadata_task(&metadata, &values[1])?;
    Ok(())
}

fn task_workflow_ref_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task workflow-ref <runtime-metadata-file> <task-id>".to_string(),
        );
    }
    let metadata = load_runtime_metadata(&values[0])?;
    let task = runtime_metadata_task(&metadata, &values[1])?;
    println!("{}", runtime_task_runner_workflow_id(task));
    Ok(())
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

fn task_render_help_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task render-help <runtime-metadata-file> <task-id>".to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let task = runtime_metadata_task(&metadata, &values[1])?;
    let help_lines = object_field(task, "help")
        .and_then(|help| object_array(help, "lines"))
        .unwrap_or(&[])
        .iter()
        .filter_map(JsonValue::as_str)
        .collect::<Vec<_>>();
    println!("{}", help_lines.join("\n"));
    Ok(())
}

fn task_load_runtime_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel task load-runtime <runtime-metadata-file> <task-id>".to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let task = runtime_metadata_task(&metadata, &values[1])?;
    let runner = object_field(task, "runner")
        .ok_or_else(|| "runtime task descriptor missing object field runner".to_string())?;
    let deps = object_field(task, "deps")
        .ok_or_else(|| "runtime task descriptor missing object field deps".to_string())?;
    let hooks = object_field(task, "hooks")
        .ok_or_else(|| "runtime task descriptor missing object field hooks".to_string())?;
    let produces = object_field(task, "produces")
        .ok_or_else(|| "runtime task descriptor missing object field produces".to_string())?;
    let exports = vec![
        (
            "TASK_RUNNER_TYPE".to_string(),
            object_string(runner, "type").unwrap_or("shell").to_string(),
        ),
        (
            "TASK_RUNNER_COMMAND".to_string(),
            object_string(runner, "command").unwrap_or("").to_string(),
        ),
        (
            "TASK_RUNNER_PACKAGE".to_string(),
            object_string(runner, "package").unwrap_or("").to_string(),
        ),
        (
            "TASK_RUNNER_WORKFLOW_ID".to_string(),
            object_string(runner, "workflowId")
                .unwrap_or("")
                .to_string(),
        ),
        (
            "TASK_RUNTIME_PLAN_SHELL".to_string(),
            object_string(task, "runtimePlanShell")
                .unwrap_or("")
                .to_string(),
        ),
        (
            "TASK_PASS_THROUGH_ENV_NAMES_LINES".to_string(),
            array_strings(task, "passThroughEnvNames").join("\n"),
        ),
        (
            "TASK_PRODUCES_JSON".to_string(),
            render_json_compact(produces),
        ),
        (
            "TASK_MAX_ATTEMPTS".to_string(),
            object_field(task, "maxAttempts")
                .and_then(json_value_to_i64)
                .unwrap_or(1)
                .to_string(),
        ),
        (
            "TASK_RETRY_BACKOFF_LINES".to_string(),
            object_field(task, "retryBackoffValues")
                .and_then(JsonValue::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(json_value_to_plain_string)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
        ),
        (
            "TASK_REQUIRED_SERVICES_LINES".to_string(),
            array_strings(task, "requiredServices").join("\n"),
        ),
        (
            "TASK_CLOSURE_SELECTED_SERVICES_LINES".to_string(),
            array_strings(task, "closureSelectedServices").join("\n"),
        ),
        (
            "TASK_BASE_CLOSURE_SELECTED_SERVICES_LINES".to_string(),
            array_strings(task, "baseClosureSelectedServices").join("\n"),
        ),
        (
            "TASK_NEEDS_LINES".to_string(),
            array_strings(deps, "needs").join("\n"),
        ),
        (
            "TASK_SOFT_NEEDS_LINES".to_string(),
            array_strings(deps, "softNeeds").join("\n"),
        ),
        (
            "TASK_HOOK_COUNT".to_string(),
            object_field(hooks, "count")
                .and_then(json_value_to_i64)
                .unwrap_or(0)
                .to_string(),
        ),
        (
            "TASK_PRE_HOOK_IDS_LINES".to_string(),
            array_strings(hooks, "preIds").join("\n"),
        ),
        (
            "TASK_POST_HOOK_IDS_LINES".to_string(),
            array_strings(hooks, "postIds").join("\n"),
        ),
    ];

    print!("{}", render_shell_exports(&exports));
    Ok(())
}

fn task_load_hook_command(values: &[String]) -> Result<(), String> {
    if values.len() != 4 {
        return Err(
            "usage: nixfied-kernel task load-hook <runtime-metadata-file> <task-id> <pre|post> <hook-id>"
                .to_string(),
        );
    }

    let metadata = load_runtime_metadata(&values[0])?;
    let task = runtime_metadata_task(&metadata, &values[1])?;
    let phase_hooks = object_field(task, "hooks")
        .and_then(|hooks| object_field(hooks, &values[2]))
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("unknown task hook phase '{}'", values[2]))?;
    let hook = phase_hooks.get(&values[3]).ok_or_else(|| {
        format!(
            "unknown task hook '{}:{}:{}'",
            values[1], values[2], values[3]
        )
    })?;
    let exports = vec![
        (
            "HOOK_COMMAND".to_string(),
            object_string(hook, "command").unwrap_or("").to_string(),
        ),
        (
            "HOOK_RUNTIME_PLAN_SHELL".to_string(),
            object_string(hook, "runtimePlanShell")
                .unwrap_or("")
                .to_string(),
        ),
        (
            "HOOK_PASS_THROUGH_ENV_NAMES_LINES".to_string(),
            array_strings(hook, "passThroughEnvNames").join("\n"),
        ),
    ];
    print!("{}", render_shell_exports(&exports));
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

fn collect_task_execution_plan(
    plan: &TaskDependencyPlan,
    skipped_services: &BTreeSet<String>,
    root_task_id: &str,
) -> Result<TaskExecutionPlan, String> {
    let mut steps = Vec::new();
    let mut active = BTreeSet::new();
    let mut emitted = BTreeSet::new();
    let mut soft_missing = BTreeSet::new();

    collect_task_execution_plan_visit(
        plan,
        skipped_services,
        root_task_id,
        None,
        &mut active,
        &mut emitted,
        &mut soft_missing,
        &mut steps,
    )?;

    Ok(TaskExecutionPlan {
        steps,
        soft_missing_lines: soft_missing.into_iter().collect::<Vec<_>>().join("\n"),
    })
}

fn collect_task_execution_plan_visit(
    plan: &TaskDependencyPlan,
    skipped_services: &BTreeSet<String>,
    task_id: &str,
    soft_parent_task: Option<&str>,
    active: &mut BTreeSet<String>,
    emitted: &mut BTreeSet<String>,
    soft_missing: &mut BTreeSet<String>,
    steps: &mut Vec<TaskExecutionStep>,
) -> Result<(), String> {
    if emitted.contains(task_id) {
        return Ok(());
    }
    if active.contains(task_id) {
        return Err(format!("cyclic task dependency detected at '{}'", task_id));
    }

    let task = plan
        .tasks
        .get(task_id)
        .ok_or_else(|| format!("unknown task '{}'", task_id))?;
    active.insert(task_id.to_string());

    if let Some(skip_service) = task
        .required_services
        .iter()
        .find(|service_name| skipped_services.contains(*service_name))
    {
        steps.push(TaskExecutionStep {
            task_id: task_id.to_string(),
            action: "service-skipped".to_string(),
            soft_parent_task: soft_parent_task.unwrap_or("").to_string(),
            skip_service: skip_service.to_string(),
        });
        active.remove(task_id);
        emitted.insert(task_id.to_string());
        return Ok(());
    }

    for dependency in &task.needs {
        collect_task_execution_plan_visit(
            plan,
            skipped_services,
            dependency,
            None,
            active,
            emitted,
            soft_missing,
            steps,
        )?;
    }

    for dependency in &task.soft_needs {
        if !plan.tasks.contains_key(dependency) {
            soft_missing.insert(format!("{}\t{}", task_id, dependency));
            continue;
        }
        collect_task_execution_plan_visit(
            plan,
            skipped_services,
            dependency,
            Some(task_id),
            active,
            emitted,
            soft_missing,
            steps,
        )?;
    }

    steps.push(TaskExecutionStep {
        task_id: task_id.to_string(),
        action: "execute".to_string(),
        soft_parent_task: soft_parent_task.unwrap_or("").to_string(),
        skip_service: String::new(),
    });
    active.remove(task_id);
    emitted.insert(task_id.to_string());
    Ok(())
}

fn write_task_execution_plan_file(path: &str, steps: &[TaskExecutionStep]) -> Result<(), String> {
    if path == "-" {
        return Ok(());
    }

    let mut rendered = String::new();
    for step in steps {
        rendered.push_str(&format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\n",
            step.task_id, step.action, step.soft_parent_task, step.skip_service
        ));
    }
    write_text_atomic(path, &rendered)
}
