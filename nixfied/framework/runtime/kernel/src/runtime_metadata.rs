use super::*;

pub(crate) fn load_runtime_metadata(path: &str) -> Result<JsonValue, String> {
    let value = parse_json_file(path, "runtime metadata source")?;
    if object_field(&value, "schema").and_then(|schema| object_string(schema, "kind"))
        == Some("nixfied-runtime-metadata")
    {
        return Ok(value);
    }

    let compiled = object_field(&value, "compiled")
        .ok_or_else(|| "runtime metadata source missing object field compiled".to_string())?;
    let metadata = object_field(compiled, "runtimeMetadata").ok_or_else(|| {
        "runtime metadata source missing object field compiled.runtimeMetadata".to_string()
    })?;
    let kind = object_field(metadata, "schema")
        .and_then(|schema| object_string(schema, "kind"))
        .ok_or_else(|| "runtime metadata is missing schema.kind".to_string())?;
    if kind != "nixfied-runtime-metadata" {
        return Err(format!("unsupported runtime metadata kind: {}", kind));
    }
    Ok(metadata.clone())
}

pub(crate) fn runtime_metadata_tasks<'a>(
    metadata: &'a JsonValue,
) -> Result<&'a serde_json::Map<String, JsonValue>, String> {
    object_field(metadata, "tasks")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "runtime metadata missing object field tasks".to_string())
}

pub(crate) fn runtime_metadata_workflows<'a>(
    metadata: &'a JsonValue,
) -> Result<&'a serde_json::Map<String, JsonValue>, String> {
    object_field(metadata, "workflows")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "runtime metadata missing object field workflows".to_string())
}

pub(crate) fn runtime_metadata_task<'a>(
    metadata: &'a JsonValue,
    task_id: &str,
) -> Result<&'a JsonValue, String> {
    runtime_metadata_tasks(metadata)?
        .get(task_id)
        .ok_or_else(|| format!("unknown task '{}'", task_id))
}

pub(crate) fn runtime_metadata_workflow<'a>(
    metadata: &'a JsonValue,
    workflow_id: &str,
) -> Result<&'a JsonValue, String> {
    runtime_metadata_workflows(metadata)?
        .get(workflow_id)
        .ok_or_else(|| format!("unknown workflow '{}'", workflow_id))
}
