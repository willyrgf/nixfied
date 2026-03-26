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
    let pass_through_env_json: Map<String, JsonValue> = pass_through_env
        .into_iter()
        .map(|(key, value)| (key, JsonValue::String(value)))
        .collect();
    let argv: Vec<JsonValue> = strip_passthrough_separator(&values[8..])
        .iter()
        .map(|value| json!(value))
        .collect();
    let workflow_id = nullable_string_value(&values[3]);
    let task_id = nullable_string_value(&values[4]);
    let envelope = json!({
        "model_eval_hash": values[0],
        "runtime_hash": values[1],
        "run_kind": values[2],
        "workflow_id": workflow_id,
        "task_id": task_id,
        "slot": values[5],
        "env": values[6],
        "pass_through_env": pass_through_env_json,
        "argv": argv,
    });

    println!("{}", render_json_compact(&envelope));
    Ok(())
}
