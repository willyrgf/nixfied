use super::*;

use crate::parse_util::strip_passthrough_separator;

pub(crate) fn run_id_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "envelope" => run_id_envelope_command(values),
        other => Err(format!("unknown run-id subcommand: {}", other)),
    }
}

pub(crate) fn run_id_envelope_command(values: &[String]) -> Result<(), String> {
    if values.len() < 8 {
        return Err(
            "usage: nixfied-kernel run-id envelope <model-eval-hash> <runtime-hash> <run-kind> <workflow-id> <task-id> <slot> <env> <pass-through-env-file> [-- <args...>]"
                .to_string(),
        );
    }

    let pass_through_env =
        parse_tab_separated_name_value_file(&values[7], "run-id pass-through env")?;
    let envelope = build_run_id_envelope(
        &values[0],
        &values[1],
        &values[2],
        &values[3],
        &values[4],
        &values[5],
        &values[6],
        &pass_through_env,
        &values[8..],
    );

    println!("{}", render_json_compact(&envelope));
    Ok(())
}

pub(crate) fn build_run_id_envelope(
    model_eval_hash: &str,
    runtime_hash: &str,
    run_kind: &str,
    workflow_id: &str,
    task_id: &str,
    slot: &str,
    env_name: &str,
    pass_through_env: &[(String, String)],
    argv_values: &[String],
) -> JsonValue {
    let pass_through_env_json: Map<String, JsonValue> = pass_through_env
        .iter()
        .map(|(key, value)| (key.clone(), JsonValue::String(value.clone())))
        .collect();
    let argv: Vec<JsonValue> = strip_passthrough_separator(argv_values)
        .iter()
        .map(|value| json!(value))
        .collect();
    let workflow_id = nullable_string_value(workflow_id);
    let task_id = nullable_string_value(task_id);

    json!({
        "model_eval_hash": model_eval_hash,
        "runtime_hash": runtime_hash,
        "run_kind": run_kind,
        "workflow_id": workflow_id,
        "task_id": task_id,
        "slot": slot,
        "env": env_name,
        "pass_through_env": pass_through_env_json,
        "argv": argv,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_envelope_normalizes_empty_ids_and_strips_passthrough_separator() {
        let argv = vec![
            "--".to_string(),
            "task.check".to_string(),
            "--summary".to_string(),
        ];
        let envelope = build_run_id_envelope(
            "eval-hash",
            "runtime-hash",
            "workflow",
            "",
            "",
            "0",
            "dev",
            &[
                ("FOO".to_string(), "bar".to_string()),
                ("BAR".to_string(), "baz".to_string()),
            ],
            &argv,
        );

        assert_eq!(envelope["workflow_id"], JsonValue::Null);
        assert_eq!(envelope["task_id"], JsonValue::Null);
        assert_eq!(envelope["argv"], json!(["task.check", "--summary"]));
        assert_eq!(envelope["pass_through_env"]["FOO"], json!("bar"));
        assert_eq!(envelope["pass_through_env"]["BAR"], json!("baz"));
    }
}
