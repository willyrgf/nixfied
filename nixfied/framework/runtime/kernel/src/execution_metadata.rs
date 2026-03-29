use super::*;

pub(crate) fn load_execution_metadata(path: &str) -> Result<JsonValue, String> {
    let value = parse_json_file(path, "execution metadata source")?;
    if object_field(&value, "schema").and_then(|schema| object_string(schema, "kind"))
        == Some("nixfied-execution")
    {
        return Ok(value);
    }

    let compiled = object_field(&value, "compiled")
        .ok_or_else(|| "execution metadata source missing object field compiled".to_string())?;
    let execution = object_field(compiled, "execution").ok_or_else(|| {
        "execution metadata source missing object field compiled.execution".to_string()
    })?;
    let kind = object_field(execution, "schema")
        .and_then(|schema| object_string(schema, "kind"))
        .ok_or_else(|| "execution metadata is missing schema.kind".to_string())?;
    if kind != "nixfied-execution" {
        return Err(format!("unsupported execution metadata kind: {}", kind));
    }
    Ok(execution.clone())
}

pub(crate) fn execution_tasks<'a>(
    execution: &'a JsonValue,
) -> Result<&'a serde_json::Map<String, JsonValue>, String> {
    object_field(execution, "tasks")
        .and_then(|tasks| object_field(tasks, "byId"))
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "execution metadata missing object field tasks.byId".to_string())
}

pub(crate) fn execution_workflows<'a>(
    execution: &'a JsonValue,
) -> Result<&'a serde_json::Map<String, JsonValue>, String> {
    object_field(execution, "workflows")
        .and_then(|workflows| object_field(workflows, "byId"))
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "execution metadata missing object field workflows.byId".to_string())
}

pub(crate) fn execution_task<'a>(
    execution: &'a JsonValue,
    task_id: &str,
) -> Result<&'a JsonValue, String> {
    execution_tasks(execution)?
        .get(task_id)
        .ok_or_else(|| format!("unknown task '{}'", task_id))
}

pub(crate) fn execution_workflow<'a>(
    execution: &'a JsonValue,
    workflow_id: &str,
) -> Result<&'a JsonValue, String> {
    execution_workflows(execution)?
        .get(workflow_id)
        .ok_or_else(|| format!("unknown workflow '{}'", workflow_id))
}
