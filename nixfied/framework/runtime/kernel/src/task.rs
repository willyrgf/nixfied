use super::*;

pub(crate) fn task_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "execution-order" => task_execution_order_command(values),
        other => Err(format!("unknown task subcommand: {}", other)),
    }
}

fn task_execution_order_command(values: &[String]) -> Result<(), String> {
    if values.len() != 5 {
        return Err(
            "usage: nixfied-kernel task execution-order <plan-file> <skipped-services-file> <task-id> <order-file> <export-file>"
                .to_string(),
        );
    }

    let plan = load_task_dependency_plan(&values[0])?;
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

fn load_task_dependency_plan(path: &str) -> Result<TaskDependencyPlan, String> {
    let value = parse_json_file(path, "task dependency plan")?;
    let kind = required_string_field(&value, "kind", "task dependency plan")?;
    if kind != "nixfied-task-dependency-plan" {
        return Err(format!("unsupported task dependency plan kind: {}", kind));
    }
    let version = object_field(&value, "version")
        .and_then(json_value_to_i64)
        .ok_or_else(|| "task dependency plan missing integer field version".to_string())?;
    if version != 1 {
        return Err(format!(
            "task dependency plan version must be 1 (got {})",
            version
        ));
    }

    let mut tasks = BTreeMap::new();
    let task_entries = object_field(&value, "tasks")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "task dependency plan missing object field tasks".to_string())?;
    for (task_id, entry) in task_entries {
        tasks.insert(
            task_id.clone(),
            TaskDependencyEntry {
                needs: array_strings(entry, "needs"),
                soft_needs: array_strings(entry, "softNeeds"),
                required_services: array_strings(entry, "requiredServices"),
            },
        );
    }

    Ok(TaskDependencyPlan { tasks })
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
