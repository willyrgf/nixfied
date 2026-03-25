use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::{self, Command, Stdio};
use std::time::Duration;

fn main() {
    if let Err(err) = run() {
        eprintln!("ERROR: {}", err);
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(usage)?;

    match command.as_str() {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "validate-payload" | "validate-artifact" => {
            let bundle_path = args.next().ok_or_else(usage)?;
            let contract_ref = args.next().ok_or_else(usage)?;
            let payload_path = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            validate_file_command(&command, &bundle_path, &contract_ref, &payload_path)
        }
        "validate-input" => {
            let plan_path = args.next().ok_or_else(usage)?;
            let mode = args.next().ok_or_else(usage)?;
            let export_path = args.next().ok_or_else(usage)?;
            let remaining = args.collect::<Vec<_>>();
            validate_input_command(&plan_path, &mode, &export_path, &remaining)
        }
        "validate-scalar" => {
            let spec_path = args.next().ok_or_else(usage)?;
            let value = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            validate_scalar_command(&spec_path, &value)
        }
        "validate-exit" => {
            let plan_path = args.next().ok_or_else(usage)?;
            let exit_code = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            validate_exit_command(&plan_path, &exit_code)
        }
        "run-id" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            run_id_command(&subcommand, &values)
        }
        "event-detail" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            event_detail_command(&subcommand, &values)
        }
        "run-record" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            run_record_command(&subcommand, &values)
        }
        "task" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            task_command(&subcommand, &values)
        }
        "workflow" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            workflow_command(&subcommand, &values)
        }
        "registry" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            registry_command(&subcommand, &values)
        }
        "summary" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            summary_command(&subcommand, &values)
        }
        "adapter" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            adapter_command(&subcommand, &values)
        }
        "probe" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            probe_command(&subcommand, &values)
        }
        "machine-output" => {
            let subcommand = args.next().ok_or_else(usage)?;
            let values = args.collect::<Vec<_>>();
            machine_output_command(&subcommand, &values)
        }
        other => Err(format!("unknown command: {}\n{}", other, usage())),
    }
}

fn usage() -> String {
    [
        "usage: nixfied-kernel <command> [args...]",
        "",
        "commands:",
        "  validate-payload <bundle-file> <contract-ref> <payload-file>",
        "  validate-artifact <bundle-file> <contract-ref> <payload-file>",
        "  validate-input <plan-file> <env|args> <export-file> [-- <args...>]",
        "  validate-scalar <spec-file> <value>",
        "  validate-exit <plan-file> <exit-code>",
        "  run-id <envelope> ...",
        "  event-detail <render> ...",
        "  run-record <create|transition> ...",
        "  task <execution-order> ...",
        "  workflow <serial-init|serial-next|serial-transition|parallel-init|parallel-next|parallel-transition> ...",
        "  registry <append|replay|terminal|runtime-status> ...",
        "  summary <write|compose|collect-steps|render-human> ...",
        "  adapter decode <kind> ...",
        "  probe evaluate <plan-file> [payload-file] [export-file]",
        "  machine-output run <plan-file> [-- <args...>]",
        "  help",
    ]
    .join("\n")
}

fn print_help() {
    println!("{}", usage());
}

#[derive(Clone)]
struct ScalarSpec {
    type_name: String,
    values: Vec<String>,
    min: Option<i64>,
    max: Option<i64>,
}

#[derive(Clone)]
struct CommandArgSpec {
    name: String,
    kind: String,
    scalar: ScalarSpec,
    long: String,
    short: String,
    required: bool,
}

#[derive(Clone)]
struct CommandEnvSpec {
    name: String,
    scalar: ScalarSpec,
    required: bool,
    default: Option<String>,
    aliases: Vec<String>,
}

#[derive(Clone)]
struct CommandRuntimePlan {
    allow_unknown_args: bool,
    args: Vec<CommandArgSpec>,
    env: Vec<CommandEnvSpec>,
    failure_codes: BTreeSet<i32>,
}

#[derive(Clone)]
struct ProbePlan {
    probe_kind: String,
    export_var: Option<String>,
}

#[derive(Clone)]
struct ProbeExecutionPlan {
    mode: String,
    service_name: String,
    source_env_var: String,
    curl_bin: String,
    runtime_shell_bin: String,
    pg_is_ready_bin: String,
    psql_bin: String,
    steps: Vec<ProbeExecutionStep>,
}

#[derive(Clone)]
struct ProbeExecutionStep {
    kind: String,
    service_label: String,
    phase_label: String,
    success_label: String,
    failure_label: String,
    host: Option<String>,
    scheme: Option<String>,
    path: Option<String>,
    method: Option<String>,
    port_env_var: Option<String>,
    execution_port_env_var: Option<String>,
    source_kinds: BTreeMap<String, String>,
    readiness_profile: Option<String>,
    require_not_syncing: bool,
    allow_local_health_fallback: bool,
    disallow_source_kinds: Vec<String>,
    max_time_seconds: Option<i64>,
    database: Option<String>,
    query: Option<String>,
    failure_suffix: Option<String>,
    command: Option<String>,
}

#[derive(Clone)]
struct MachineOutputPlan {
    app_id: String,
    target_app_id: String,
    contract_ref: String,
    bundle_file: String,
    target_program: String,
    setup_programs: Vec<String>,
    teardown_programs: Vec<String>,
    target_args: Vec<String>,
}

#[derive(Clone)]
struct TaskDependencyPlan {
    tasks: BTreeMap<String, TaskDependencyEntry>,
}

#[derive(Clone)]
struct TaskDependencyEntry {
    needs: Vec<String>,
    soft_needs: Vec<String>,
    required_services: Vec<String>,
}

struct TaskExecutionPlan {
    steps: Vec<TaskExecutionStep>,
    soft_missing_lines: String,
}

struct TaskExecutionStep {
    task_id: String,
    action: String,
    soft_parent_task: String,
    skip_service: String,
}

#[derive(Clone)]
struct WorkflowSchedulerPlan {
    workflows: BTreeMap<String, WorkflowSchedulerWorkflow>,
}

#[derive(Clone)]
struct WorkflowSchedulerWorkflow {
    units: Vec<WorkflowSchedulerUnitPlan>,
}

#[derive(Clone)]
struct WorkflowSchedulerUnitPlan {
    name: String,
    task_id: String,
    needs: Vec<String>,
    locks: Vec<String>,
    required_services: Vec<String>,
    skip_if_missing_env: Vec<String>,
    when_env_present: Vec<String>,
    when_env_equals: BTreeMap<String, String>,
    selected_services_csv: String,
    produces_json: String,
}

struct WorkflowSerialState {
    workflow_id: String,
    fail_fast: bool,
    workflow_status: i64,
    halted: bool,
    order: Vec<String>,
    units: BTreeMap<String, WorkflowSerialUnitState>,
}

struct WorkflowSerialUnitState {
    name: String,
    task_id: String,
    needs_left: i64,
    dependents: Vec<String>,
    state: String,
    cancel_reason: String,
    cancel_extra_key: String,
    cancel_extra_value: String,
    selected_services_csv: String,
}

struct WorkflowParallelState {
    workflow_id: String,
    fail_fast: bool,
    max_workers: i64,
    workflow_status: i64,
    stop_scheduling: bool,
    order: Vec<String>,
    units: BTreeMap<String, WorkflowParallelUnitState>,
}

struct WorkflowParallelUnitState {
    name: String,
    task_id: String,
    needs_left: i64,
    dependents: Vec<String>,
    locks: Vec<String>,
    state: String,
    cancel_reason: String,
    cancel_extra_key: String,
    cancel_extra_value: String,
    selected_services_csv: String,
    produces_json: String,
}

#[derive(Clone)]
struct WorkflowSummaryPlan {
    task_runner_types: BTreeMap<String, String>,
}

#[derive(Clone)]
struct WorkflowCollectedStep {
    name: String,
    status: String,
    state: String,
    duration: i64,
    order: i64,
    workflow_id: String,
    reason: String,
    exit_code: String,
}

struct WorkflowCollectedSummary {
    steps: Vec<WorkflowCollectedStep>,
    passed: i64,
    failed: i64,
    skipped: i64,
    canceled: i64,
    steps_duration: i64,
    peak_workers: i64,
    leaf_task_ids_lines: String,
}

struct WorkflowTerminalRow {
    order: i64,
    row_index: usize,
    name: String,
    workflow_id: String,
    state: String,
    duration: i64,
    reason: String,
    exit_code: String,
}

fn validate_input_command(
    plan_path: &str,
    mode: &str,
    export_path: &str,
    remaining: &[String],
) -> Result<(), String> {
    let plan = load_command_runtime_plan(plan_path)?;
    let exports = match mode {
        "env" => validate_input_env(&plan)?,
        "args" => {
            let values = strip_passthrough_separator(remaining);
            validate_input_args(&plan, values)?
        }
        other => {
            return Err(format!(
                "validate-input mode must be env or args (got {})",
                other
            ))
        }
    };

    write_shell_exports(export_path, &exports)?;
    println!("OK: validate-input mode={}", mode);
    Ok(())
}

fn validate_scalar_command(spec_path: &str, value: &str) -> Result<(), String> {
    let spec = load_scalar_spec(spec_path)?;
    validate_scalar_value(&spec, value, "scalar")?;
    println!("OK: validate-scalar");
    Ok(())
}

fn validate_exit_command(plan_path: &str, exit_code_text: &str) -> Result<(), String> {
    let plan = load_command_runtime_plan(plan_path)?;
    let exit_code = parse_i32_text(exit_code_text, "exit code")?;
    if exit_code == 0 || plan.failure_codes.contains(&exit_code) {
        println!("OK: validate-exit");
        Ok(())
    } else {
        Err(format!("undeclared exit code code={}", exit_code))
    }
}

fn run_id_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "envelope" => run_id_envelope_command(values),
        other => Err(format!("unknown run-id subcommand: {}", other)),
    }
}

fn run_id_envelope_command(values: &[String]) -> Result<(), String> {
    if values.len() < 8 {
        return Err(
            "usage: nixfied-kernel run-id envelope <model-eval-hash> <runtime-hash> <run-kind> <workflow-id> <task-id> <slot> <env> <pass-through-env-file> [-- <args...>]"
                .to_string(),
        );
    }

    let pass_through_env =
        parse_tab_separated_name_value_file(&values[7], "run-id pass-through env")?;
    let pass_through_env_json = JsonValue::Object(
        pass_through_env
            .into_iter()
            .map(|(key, value)| (key, JsonValue::String(value)))
            .collect(),
    );
    let argv = strip_passthrough_separator(&values[8..])
        .iter()
        .map(|value| JsonValue::String(value.clone()))
        .collect::<Vec<_>>();
    let envelope = JsonValue::Object(BTreeMap::from([
        (
            "model_eval_hash".to_string(),
            JsonValue::String(values[0].clone()),
        ),
        (
            "runtime_hash".to_string(),
            JsonValue::String(values[1].clone()),
        ),
        ("run_kind".to_string(), JsonValue::String(values[2].clone())),
        ("workflow_id".to_string(), nullable_string_value(&values[3])),
        ("task_id".to_string(), nullable_string_value(&values[4])),
        ("slot".to_string(), JsonValue::String(values[5].clone())),
        ("env".to_string(), JsonValue::String(values[6].clone())),
        ("pass_through_env".to_string(), pass_through_env_json),
        ("argv".to_string(), JsonValue::Array(argv)),
    ]));

    println!("{}", render_json_compact(&envelope));
    Ok(())
}

fn event_detail_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "render" => event_detail_render_command(values),
        other => Err(format!("unknown event-detail subcommand: {}", other)),
    }
}

fn event_detail_render_command(values: &[String]) -> Result<(), String> {
    let kind = values.first().ok_or_else(|| {
        "usage: nixfied-kernel event-detail render <kind> [--field value ...]".to_string()
    })?;

    let mut event_type = None;
    let mut command_name = None;
    let mut project_id = None;
    let mut service = None;
    let mut slot = None;
    let mut env_name = None;
    let mut profile = None;
    let mut pid = None;
    let mut pgid = None;
    let mut plan_id = None;
    let mut unit_id = None;
    let mut attempt = None;
    let mut owner_scope = None;
    let mut reuse_policy = None;
    let mut discovery_scope = None;
    let mut ephemeral_root = None;
    let mut readiness_health = None;
    let mut readiness_ready = None;
    let mut last_error = None;
    let mut wait_reason = None;
    let mut log_path = None;
    let mut mode = None;
    let mut suffix_reason = None;
    let mut produces = None;
    let mut exit_code = None;
    let mut reason = None;
    let mut dependency = None;
    let mut service_name = None;
    let mut signal = None;
    let mut missing = None;
    let mut run_id = None;
    let mut workflow_id = None;
    let mut task_id = None;
    let mut target = None;
    let mut index = 1usize;

    while index < values.len() {
        let flag = values[index].as_str();
        index += 1;
        let value = next_flag_value(values, &mut index, flag)?;
        match flag {
            "--event-type" => event_type = optional_string_value(&value),
            "--command-name" => command_name = optional_string_value(&value),
            "--project-id" => project_id = optional_string_value(&value),
            "--service" => service = optional_string_value(&value),
            "--slot" => slot = optional_string_value(&value),
            "--env" => env_name = optional_string_value(&value),
            "--profile" => profile = optional_string_value(&value),
            "--pid" => pid = parse_optional_i64(&value, "event-detail pid")?,
            "--pgid" => pgid = parse_optional_i64(&value, "event-detail pgid")?,
            "--plan-id" => plan_id = optional_string_value(&value),
            "--unit-id" => unit_id = optional_string_value(&value),
            "--attempt" => attempt = parse_optional_i64(&value, "event-detail attempt")?,
            "--owner-scope" => owner_scope = optional_string_value(&value),
            "--reuse-policy" => reuse_policy = optional_string_value(&value),
            "--discovery-scope" => discovery_scope = optional_string_value(&value),
            "--ephemeral-root" => ephemeral_root = optional_string_value(&value),
            "--readiness-health" => {
                readiness_health =
                    parse_optional_bool_text(&value, "event-detail readiness-health")?
            }
            "--readiness-ready" => {
                readiness_ready = parse_optional_bool_text(&value, "event-detail readiness-ready")?
            }
            "--last-error" => last_error = optional_string_value(&value),
            "--wait-reason" => wait_reason = optional_string_value(&value),
            "--log-path" => log_path = optional_string_value(&value),
            "--mode" => mode = optional_string_value(&value),
            "--suffix-reason" => suffix_reason = optional_string_value(&value),
            "--produces-json" => {
                produces = Some(
                    parse_json(&value)
                        .map_err(|err| format!("event-detail produces json is invalid: {}", err))?,
                )
            }
            "--exit-code" => exit_code = parse_optional_i64(&value, "event-detail exit-code")?,
            "--reason" => reason = optional_string_value(&value),
            "--dependency" => dependency = optional_string_value(&value),
            "--service-name" => service_name = optional_string_value(&value),
            "--signal" => signal = optional_string_value(&value),
            "--missing" => missing = optional_string_value(&value),
            "--run-id" => run_id = optional_string_value(&value),
            "--workflow-id" => workflow_id = optional_string_value(&value),
            "--task-id" => task_id = optional_string_value(&value),
            "--target" => target = optional_string_value(&value),
            other => return Err(format!("unknown event-detail arg: {}", other)),
        }
    }

    let mut fields = BTreeMap::new();
    fields.insert("kind".to_string(), JsonValue::String(kind.clone()));

    match kind.as_str() {
        "slotLifecycle" => {
            insert_optional_string_field(&mut fields, "eventType", event_type);
            insert_optional_string_field(&mut fields, "commandName", command_name);
            insert_optional_string_field(&mut fields, "projectId", project_id);
            insert_optional_string_field(&mut fields, "slot", slot);
            insert_optional_string_field(&mut fields, "env", env_name);
            insert_optional_string_field(&mut fields, "profile", profile);
            insert_optional_number_field(&mut fields, "pid", pid);
            insert_optional_number_field(&mut fields, "pgid", pgid);
            if readiness_health.is_some() || readiness_ready.is_some() || last_error.is_some() {
                let mut readiness = BTreeMap::new();
                insert_optional_bool_field(&mut readiness, "healthOk", readiness_health);
                insert_optional_bool_field(&mut readiness, "readyOk", readiness_ready);
                insert_optional_string_field(&mut readiness, "lastError", last_error);
                fields.insert("readiness".to_string(), JsonValue::Object(readiness));
            }
            insert_optional_string_field(&mut fields, "waitReason", wait_reason);
            insert_optional_string_field(&mut fields, "logPath", log_path);
            insert_optional_string_field(&mut fields, "mode", mode);
            insert_optional_string_field(&mut fields, "suffixReason", suffix_reason);
            insert_optional_json_field(&mut fields, "produces", produces);
            insert_optional_number_field(&mut fields, "exitCode", exit_code);
        }
        "serviceLifecycle" => {
            insert_optional_string_field(&mut fields, "eventType", event_type);
            insert_optional_string_field(&mut fields, "service", service);
            insert_optional_string_field(&mut fields, "commandName", command_name);
            insert_optional_string_field(&mut fields, "ownerScope", owner_scope);
            insert_optional_string_field(&mut fields, "reusePolicy", reuse_policy);
            insert_optional_string_field(&mut fields, "discoveryScope", discovery_scope);
            insert_optional_string_field(&mut fields, "ephemeralRoot", ephemeral_root);
            insert_optional_string_field(&mut fields, "waitReason", wait_reason);
            insert_optional_string_field(&mut fields, "logPath", log_path);
            insert_optional_json_field(&mut fields, "produces", produces);
            insert_optional_number_field(&mut fields, "exitCode", exit_code);
            insert_optional_string_field(&mut fields, "reason", reason);
            insert_optional_string_field(&mut fields, "dependency", dependency);
            insert_optional_string_field(&mut fields, "serviceName", service_name);
            insert_optional_string_field(&mut fields, "signal", signal);
            insert_optional_string_field(&mut fields, "missing", missing);
        }
        "workflowLifecycle" => {
            insert_optional_string_field(&mut fields, "commandName", command_name);
            insert_optional_string_field(&mut fields, "workflowId", workflow_id);
            insert_optional_string_field(&mut fields, "runId", run_id);
            insert_optional_number_field(&mut fields, "attempt", attempt);
        }
        "taskLifecycle" => {
            insert_optional_string_field(&mut fields, "commandName", command_name);
            insert_optional_string_field(&mut fields, "taskId", task_id);
            insert_optional_string_field(&mut fields, "runId", run_id);
            insert_optional_number_field(&mut fields, "attempt", attempt);
            insert_optional_number_field(&mut fields, "exitCode", exit_code);
            insert_optional_string_field(&mut fields, "reason", reason);
        }
        "controlSignal" => {
            insert_optional_string_field(&mut fields, "signal", signal);
            insert_optional_string_field(&mut fields, "reason", reason);
            insert_optional_string_field(&mut fields, "target", target);
            insert_optional_string_field(&mut fields, "runId", run_id);
        }
        other => return Err(format!("unknown event-detail kind: {}", other)),
    }

    println!("{}", render_json_compact(&JsonValue::Object(fields)));
    Ok(())
}

fn load_command_runtime_plan(path: &str) -> Result<CommandRuntimePlan, String> {
    let text = read_text(path)?;
    let value = parse_json(&text)
        .map_err(|err| format!("runtime plan {} is not valid JSON: {}", path, err))?;
    parse_command_runtime_plan(&value)
}

fn parse_command_runtime_plan(value: &JsonValue) -> Result<CommandRuntimePlan, String> {
    value
        .as_object()
        .ok_or_else(|| "runtime plan must be an object".to_string())?;

    let allow_unknown_args = object_bool(value, "allowUnknownArgs").unwrap_or(false);
    let mut args = Vec::new();
    let mut env_specs = Vec::new();
    let mut failure_codes = BTreeSet::new();

    for item in object_array(value, "args").unwrap_or(&[]) {
        args.push(parse_command_arg_spec(item)?);
    }

    for item in object_array(value, "env").unwrap_or(&[]) {
        env_specs.push(parse_command_env_spec(item)?);
    }

    if let Some(codes) = object_field(value, "failureCodes").and_then(JsonValue::as_object) {
        for code in codes.values() {
            let number = json_value_to_i32(code)
                .ok_or_else(|| "runtime plan failureCodes values must be integers".to_string())?;
            failure_codes.insert(number);
        }
    }

    Ok(CommandRuntimePlan {
        allow_unknown_args,
        args,
        env: env_specs,
        failure_codes,
    })
}

fn parse_command_arg_spec(value: &JsonValue) -> Result<CommandArgSpec, String> {
    Ok(CommandArgSpec {
        name: required_string_field(value, "name", "arg spec")?.to_string(),
        kind: required_string_field(value, "kind", "arg spec")?.to_string(),
        scalar: parse_scalar_spec(value)?,
        long: object_string(value, "long").unwrap_or("").to_string(),
        short: object_string(value, "short").unwrap_or("").to_string(),
        required: object_bool(value, "required").unwrap_or(false),
    })
}

fn parse_command_env_spec(value: &JsonValue) -> Result<CommandEnvSpec, String> {
    Ok(CommandEnvSpec {
        name: required_string_field(value, "name", "env spec")?.to_string(),
        scalar: parse_scalar_spec(value)?,
        required: object_bool(value, "required").unwrap_or(false),
        default: object_field(value, "default").and_then(json_value_to_plain_string),
        aliases: array_strings(value, "aliases"),
    })
}

fn load_scalar_spec(path: &str) -> Result<ScalarSpec, String> {
    let text = read_text(path)?;
    let value = parse_json(&text)
        .map_err(|err| format!("scalar spec {} is not valid JSON: {}", path, err))?;
    parse_scalar_spec(&value)
}

fn parse_scalar_spec(value: &JsonValue) -> Result<ScalarSpec, String> {
    Ok(ScalarSpec {
        type_name: object_string(value, "type").unwrap_or("string").to_string(),
        values: array_strings(value, "values"),
        min: object_field(value, "min").and_then(json_value_to_i64),
        max: object_field(value, "max").and_then(json_value_to_i64),
    })
}

fn validate_input_env(plan: &CommandRuntimePlan) -> Result<Vec<(String, String)>, String> {
    let mut exports = Vec::new();

    for spec in &plan.env {
        let value = resolve_env_spec_value(spec)?;
        if value.is_none() && spec.required {
            return Err(format!("required env var missing name={}", spec.name));
        }

        if let Some(value) = value {
            validate_scalar_value(&spec.scalar, &value, &format!("env:{}", spec.name))?;
            exports.push((spec.name.clone(), value.clone()));
            for alias in &spec.aliases {
                exports.push((alias.clone(), value.clone()));
            }
        }
    }

    Ok(exports)
}

fn validate_input_args(
    plan: &CommandRuntimePlan,
    args: &[String],
) -> Result<Vec<(String, String)>, String> {
    let mut exports = Vec::new();
    let mut seen = BTreeSet::new();
    let mut positional_values = Vec::new();
    let mut index = 0usize;
    let mut parse_options = true;

    let spec_by_name = plan
        .args
        .iter()
        .map(|spec| (spec.name.as_str(), spec))
        .collect::<BTreeMap<_, _>>();
    let long_to_name = plan
        .args
        .iter()
        .filter(|spec| !spec.long.is_empty())
        .map(|spec| (spec.long.as_str(), spec.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let short_to_name = plan
        .args
        .iter()
        .filter(|spec| !spec.short.is_empty())
        .map(|spec| (spec.short.as_str(), spec.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let positional_specs = plan
        .args
        .iter()
        .filter(|spec| spec.kind == "positional")
        .collect::<Vec<_>>();

    while index < args.len() {
        let token = &args[index];
        index += 1;

        if !parse_options {
            positional_values.push(token.clone());
            continue;
        }

        if token == "--" {
            parse_options = false;
            continue;
        }

        if let Some((name_token, value)) = token.split_once('=') {
            if name_token.starts_with("--") {
                let Some(name) = long_to_name.get(name_token).copied() else {
                    if plan.allow_unknown_args {
                        continue;
                    }
                    return Err(format!("unknown option token={}", name_token));
                };
                let spec = spec_by_name.get(name).copied().unwrap();
                if spec.kind == "flag" {
                    return Err(format!("flag does not accept a value token={}", name_token));
                }
                validate_scalar_value(&spec.scalar, value, &format!("arg:{}", spec.name))?;
                seen.insert(spec.name.clone());
                exports.push((export_arg_name(&spec.name), value.to_string()));
                continue;
            }
        }

        if token.starts_with("--") {
            let Some(name) = long_to_name.get(token.as_str()).copied() else {
                if plan.allow_unknown_args {
                    continue;
                }
                return Err(format!("unknown option token={}", token));
            };
            let spec = spec_by_name.get(name).copied().unwrap();
            if spec.kind == "flag" {
                seen.insert(spec.name.clone());
                exports.push((export_arg_name(&spec.name), "true".to_string()));
                continue;
            }
            if index >= args.len() {
                return Err(format!("option requires value token={}", token));
            }
            let value = &args[index];
            index += 1;
            validate_scalar_value(&spec.scalar, value, &format!("arg:{}", spec.name))?;
            seen.insert(spec.name.clone());
            exports.push((export_arg_name(&spec.name), value.clone()));
            continue;
        }

        if token.starts_with('-') && token.len() > 1 {
            if let Some(name) = short_to_name.get(token.as_str()).copied() {
                let spec = spec_by_name.get(name).copied().unwrap();
                if spec.kind == "flag" {
                    seen.insert(spec.name.clone());
                    exports.push((export_arg_name(&spec.name), "true".to_string()));
                    continue;
                }
                if index >= args.len() {
                    return Err(format!("option requires value token={}", token));
                }
                let value = &args[index];
                index += 1;
                validate_scalar_value(&spec.scalar, value, &format!("arg:{}", spec.name))?;
                seen.insert(spec.name.clone());
                exports.push((export_arg_name(&spec.name), value.clone()));
                continue;
            }

            if token.len() > 2 {
                let mut cluster_specs = Vec::new();
                let mut cluster_valid = true;
                for short in token[1..].chars() {
                    let short_token = format!("-{}", short);
                    let Some(name) = short_to_name.get(short_token.as_str()).copied() else {
                        cluster_valid = false;
                        break;
                    };
                    let spec = spec_by_name.get(name).copied().unwrap();
                    if spec.kind != "flag" {
                        cluster_valid = false;
                        break;
                    }
                    cluster_specs.push(spec);
                }
                if cluster_valid {
                    for spec in cluster_specs {
                        seen.insert(spec.name.clone());
                        exports.push((export_arg_name(&spec.name), "true".to_string()));
                    }
                    continue;
                }
            }

            if plan.allow_unknown_args {
                continue;
            }
            return Err(format!("unknown option token={}", token));
        }

        positional_values.push(token.clone());
    }

    for (position, spec) in positional_specs.iter().enumerate() {
        if let Some(value) = positional_values.get(position) {
            validate_scalar_value(&spec.scalar, value, &format!("arg:{}", spec.name))?;
            seen.insert(spec.name.clone());
            exports.push((export_arg_name(&spec.name), value.clone()));
        } else if spec.required {
            return Err(format!(
                "missing required positional arg name={}",
                spec.name
            ));
        }
    }

    if positional_values.len() > positional_specs.len() && !plan.allow_unknown_args {
        return Err(format!(
            "unexpected positional args count={}",
            positional_values.len() - positional_specs.len()
        ));
    }

    for spec in &plan.args {
        if spec.required && !seen.contains(&spec.name) {
            return Err(format!("missing required arg name={}", spec.name));
        }
    }

    Ok(exports)
}

fn resolve_env_spec_value(spec: &CommandEnvSpec) -> Result<Option<String>, String> {
    let canonical = env::var(&spec.name).ok();
    let aliases = spec
        .aliases
        .iter()
        .filter_map(|alias| env::var(alias).ok().map(|value| (alias.clone(), value)))
        .collect::<Vec<_>>();

    if let Some(value) = &canonical {
        if value.is_empty() && is_runtime_primitive_env(&spec.name) {
            return Err(format!(
                "env:{} cannot be empty when set; unset {} to use defaults",
                spec.name, spec.name
            ));
        }
    }

    for (alias, value) in &aliases {
        if value.is_empty() && is_runtime_primitive_env(&spec.name) {
            return Err(format!(
                "env:{} alias={} cannot be empty when set; unset {} to use defaults",
                spec.name, alias, alias
            ));
        }
    }

    let canonical_non_empty = canonical.as_ref().filter(|value| !value.is_empty());
    let alias_non_empty = aliases
        .iter()
        .find(|(_, value)| !value.is_empty())
        .map(|(_, value)| value);

    if let (Some(left), Some(right)) = (canonical_non_empty, alias_non_empty) {
        if left != right {
            return Err(format!(
                "env:{} has conflicting values between {} and alias; set one variable or use matching values",
                spec.name, spec.name
            ));
        }
    }

    let mut first_alias: Option<(&str, &String)> = None;
    for (alias, value) in &aliases {
        if value.is_empty() {
            continue;
        }
        if let Some((previous_alias, previous_value)) = first_alias {
            if previous_value != value {
                return Err(format!(
                    "env:{} has conflicting alias values alias={} and alias={}; set one alias or use matching values",
                    spec.name, previous_alias, alias
                ));
            }
        } else {
            first_alias = Some((alias.as_str(), value));
        }
    }

    Ok(canonical_non_empty
        .cloned()
        .or_else(|| first_alias.map(|(_, value)| value.clone()))
        .or_else(|| spec.default.clone()))
}

fn validate_scalar_value(spec: &ScalarSpec, value: &str, label: &str) -> Result<(), String> {
    match spec.type_name.as_str() {
        "string" => {}
        "bool" => match value {
            "1" | "0" | "true" | "false" | "TRUE" | "FALSE" | "yes" | "YES" | "no" | "NO"
            | "on" | "ON" => {}
            _ => return Err(format!("{} must be bool (got '{}')", label, value)),
        },
        "int" => {
            let number = parse_i64_text(value, label)?;
            validate_numeric_range(number, spec, label)?;
        }
        "durationSec" => {
            let number = parse_i64_text(value, label)?;
            if number < 0 {
                return Err(format!("{} must be >= 0 seconds (got '{}')", label, value));
            }
            validate_numeric_range(number, spec, label)?;
        }
        "enum" => {
            if !spec.values.iter().any(|item| item == value) {
                return Err(format!(
                    "{} must be one of {} (got '{}')",
                    label,
                    spec.values.join(","),
                    value
                ));
            }
        }
        "pathAbs" => {
            if !value.starts_with('/') {
                return Err(format!(
                    "{} must be an absolute path (got '{}')",
                    label, value
                ));
            }
        }
        "pathRel" => {
            if value.is_empty() || value.starts_with('/') {
                return Err(format!(
                    "{} must be a relative path (got '{}')",
                    label, value
                ));
            }
        }
        "port" => {
            let number = parse_i64_text(value, label)?;
            if !(1..=65535).contains(&number) {
                return Err(format!("{} must be port 1-65535 (got '{}')", label, value));
            }
            validate_numeric_range(number, spec, label)?;
        }
        "json" => {
            parse_json(value).map_err(|_| format!("{} must be valid json", label))?;
        }
        other => return Err(format!("unsupported type={} for {}", other, label)),
    }

    Ok(())
}

fn validate_numeric_range(value: i64, spec: &ScalarSpec, label: &str) -> Result<(), String> {
    if let Some(minimum) = spec.min {
        if value < minimum {
            return Err(format!(
                "{} must be >= {} (got '{}')",
                label, minimum, value
            ));
        }
    }
    if let Some(maximum) = spec.max {
        if value > maximum {
            return Err(format!(
                "{} must be <= {} (got '{}')",
                label, maximum, value
            ));
        }
    }
    Ok(())
}

fn strip_passthrough_separator(values: &[String]) -> &[String] {
    if values.first().map(|value| value.as_str()) == Some("--") {
        &values[1..]
    } else {
        values
    }
}

fn export_arg_name(name: &str) -> String {
    format!("NIXFIED_ARG_{}", sanitize_name(name))
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '.' | ':' | '/' | '-' => '_',
            other => other.to_ascii_uppercase(),
        })
        .collect()
}

fn is_runtime_primitive_env(name: &str) -> bool {
    matches!(name, "LOG_LEVEL" | "OUTPUT_MODE")
}

fn write_shell_exports(path: &str, values: &[(String, String)]) -> Result<(), String> {
    let mut rendered = String::new();
    for (key, value) in values {
        rendered.push_str("export ");
        rendered.push_str(key);
        rendered.push('=');
        rendered.push_str(&shell_quote(value));
        rendered.push('\n');
    }
    write_text_atomic(path, &rendered)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn write_text_atomic(path: &str, contents: &str) -> Result<(), String> {
    let target = Path::new(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {}", parent.display(), err))?;
    }

    let tmp_path = format!("{}.tmp.{}", path, process::id());
    fs::write(&tmp_path, contents)
        .map_err(|err| format!("failed to write {}: {}", tmp_path, err))?;
    fs::rename(&tmp_path, path)
        .map_err(|err| format!("failed to move {} into {}: {}", tmp_path, path, err))
}

fn required_string_field<'a>(
    value: &'a JsonValue,
    key: &str,
    label: &str,
) -> Result<&'a str, String> {
    object_string(value, key).ok_or_else(|| format!("{} missing string field {}", label, key))
}

fn array_strings(value: &JsonValue, key: &str) -> Vec<String> {
    object_array(value, key)
        .unwrap_or(&[])
        .iter()
        .filter_map(JsonValue::as_string)
        .map(|item| item.to_string())
        .collect()
}

fn object_string_map(
    value: &JsonValue,
    key: &str,
    label: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    let Some(entries) = object_field(value, key).and_then(JsonValue::as_object) else {
        return Ok(result);
    };
    for (entry_key, entry_value) in entries {
        let Some(entry_text) = entry_value.as_string() else {
            return Err(format!(
                "{} field {} must contain only string values",
                label, key
            ));
        };
        result.insert(entry_key.clone(), entry_text.to_string());
    }
    Ok(result)
}

fn json_value_to_plain_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Null => None,
        JsonValue::String(value) => Some(value.clone()),
        JsonValue::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
        JsonValue::Number(number) => Some(number.raw.clone()),
        JsonValue::Array(_) | JsonValue::Object(_) => None,
    }
}

fn json_value_to_i64(value: &JsonValue) -> Option<i64> {
    match value {
        JsonValue::Number(number) if number.integer => {
            number.int_value.and_then(|value| i64::try_from(value).ok())
        }
        JsonValue::String(value) => value.parse::<i64>().ok(),
        _ => None,
    }
}

fn json_value_to_i32(value: &JsonValue) -> Option<i32> {
    json_value_to_i64(value).and_then(|value| i32::try_from(value).ok())
}

fn parse_i64_text(value: &str, label: &str) -> Result<i64, String> {
    value
        .parse::<i64>()
        .map_err(|_| format!("{} must be int (got '{}')", label, value))
}

fn parse_i32_text(value: &str, label: &str) -> Result<i32, String> {
    value
        .parse::<i32>()
        .map_err(|_| format!("{} must be int (got '{}')", label, value))
}

fn validate_file_command(
    command: &str,
    bundle_path: &str,
    contract_ref: &str,
    payload_path: &str,
) -> Result<(), String> {
    validate_json_file_against_contract(bundle_path, contract_ref, payload_path)?;
    println!("OK: {} contract={}", command, contract_ref);
    Ok(())
}

fn validate_json_file_against_contract(
    bundle_path: &str,
    contract_ref: &str,
    payload_path: &str,
) -> Result<(), String> {
    let bundle_text = read_text(bundle_path)?;
    let payload_text = read_text(payload_path)?;
    let bundle = parse_json(&bundle_text)
        .map_err(|err| format!("bundle {} is not valid JSON: {}", bundle_path, err))?;
    let payload = parse_json(&payload_text)
        .map_err(|err| format!("payload {} is not valid JSON: {}", payload_path, err))?;
    validate_json_value_against_contract(&bundle, contract_ref, &payload)
}

fn validate_json_value_against_contract(
    bundle: &JsonValue,
    contract_ref: &str,
    payload: &JsonValue,
) -> Result<(), String> {
    let context = ValidationContext::new(bundle);
    let schema = context
        .resolve_contract_ref(contract_ref)
        .map_err(|err| format!("contract {} could not be resolved: {}", contract_ref, err))?;

    let mut path = Vec::new();
    let mut ref_stack = Vec::new();
    validate_schema(&context, schema, payload, &mut path, &mut ref_stack)
        .map_err(|err| format!("contract {} failed validation: {}", contract_ref, err))
}

fn run_record_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "create" => run_record_create_command(values),
        "read" => run_record_read_command(values),
        "transition" => run_record_transition_command(values),
        other => Err(format!("unknown run-record subcommand: {}", other)),
    }
}

fn run_record_create_command(values: &[String]) -> Result<(), String> {
    if values.len() < 10 {
        return Err(
            "usage: nixfied-kernel run-record create <bundle-file> <run-file> <run-id> <attempt-id> <command> <workflow-id> <task-id> <execution-mode> <process-mode> <ephemeral-enabled> [-- <args...>]"
                .to_string(),
        );
    }

    let bundle_path = &values[0];
    let run_file = &values[1];
    let now = current_utc_timestamp()?;
    let args = JsonValue::Array(
        strip_passthrough_separator(&values[10..])
            .iter()
            .map(|value| JsonValue::String(value.clone()))
            .collect(),
    );

    let history = JsonValue::Array(vec![run_record_history_entry("queued", &now)]);
    let payload = JsonValue::Object(BTreeMap::from([
        ("run_id".to_string(), JsonValue::String(values[2].clone())),
        (
            "attempt_id".to_string(),
            JsonValue::String(values[3].clone()),
        ),
        ("command".to_string(), JsonValue::String(values[4].clone())),
        ("workflow_id".to_string(), nullable_string_value(&values[5])),
        ("task_id".to_string(), nullable_string_value(&values[6])),
        (
            "execution_mode".to_string(),
            JsonValue::String(values[7].clone()),
        ),
        (
            "process_mode".to_string(),
            JsonValue::String(values[8].clone()),
        ),
        (
            "ephemeral_enabled".to_string(),
            JsonValue::Bool(parse_bool_flag(&values[9])?),
        ),
        ("state".to_string(), JsonValue::String("queued".to_string())),
        ("pid".to_string(), JsonValue::Null),
        ("pgid".to_string(), JsonValue::Null),
        ("exit_code".to_string(), JsonValue::Null),
        ("stop_reason".to_string(), JsonValue::Null),
        ("created_at".to_string(), JsonValue::String(now.clone())),
        ("started_at".to_string(), JsonValue::Null),
        ("finished_at".to_string(), JsonValue::Null),
        ("updated_at".to_string(), JsonValue::String(now)),
        ("args".to_string(), args),
        ("history".to_string(), history),
    ]));
    let envelope = JsonValue::Object(BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("run-record".to_string()),
        ),
        (
            "version".to_string(),
            JsonValue::Number(JsonNumber::from_int(1)),
        ),
        ("payload".to_string(), payload),
    ]));

    validate_and_write_json(bundle_path, "runtime.runRecord", run_file, &envelope)?;
    println!("OK: run-record create");
    Ok(())
}

fn run_record_transition_command(values: &[String]) -> Result<(), String> {
    if values.len() != 7 {
        return Err(
            "usage: nixfied-kernel run-record transition <bundle-file> <run-file> <state> <exit-code|empty> <stop-reason|empty> <pid|empty> <pgid|empty>"
                .to_string(),
        );
    }

    let bundle_path = &values[0];
    let run_file = &values[1];
    let state = &values[2];
    let exit_code = parse_optional_i64(&values[3], "run-record exit_code")?;
    let stop_reason = optional_string_value(&values[4]);
    let pid = parse_optional_i64(&values[5], "run-record pid")?;
    let pgid = parse_optional_i64(&values[6], "run-record pgid")?;
    let now = current_utc_timestamp()?;

    let mut envelope = parse_json_file(run_file, "run-record file")?;
    let payload = object_field_mut(&mut envelope, "payload")
        .ok_or_else(|| "run-record payload is missing".to_string())?;
    let payload_object = payload
        .as_object_mut()
        .ok_or_else(|| "run-record payload must be an object".to_string())?;

    payload_object.insert("state".to_string(), JsonValue::String(state.clone()));
    payload_object.insert("updated_at".to_string(), JsonValue::String(now.clone()));
    if let Some(pid) = pid {
        payload_object.insert(
            "pid".to_string(),
            JsonValue::Number(JsonNumber::from_int(pid)),
        );
    }
    if let Some(pgid) = pgid {
        payload_object.insert(
            "pgid".to_string(),
            JsonValue::Number(JsonNumber::from_int(pgid)),
        );
    }
    if payload_object
        .get("started_at")
        .map(|value| matches!(value, JsonValue::Null))
        .unwrap_or(true)
        && state == "running"
    {
        payload_object.insert("started_at".to_string(), JsonValue::String(now.clone()));
    }
    if matches!(state.as_str(), "passed" | "failed" | "canceled") {
        payload_object.insert("finished_at".to_string(), JsonValue::String(now.clone()));
    }
    if let Some(exit_code) = exit_code {
        payload_object.insert(
            "exit_code".to_string(),
            JsonValue::Number(JsonNumber::from_int(exit_code)),
        );
    }
    if let Some(stop_reason) = stop_reason {
        payload_object.insert("stop_reason".to_string(), JsonValue::String(stop_reason));
    }

    let history_value = payload_object
        .get_mut("history")
        .ok_or_else(|| "run-record history is missing".to_string())?;
    let history = history_value
        .as_array_mut()
        .ok_or_else(|| "run-record history must be an array".to_string())?;
    history.push(run_record_history_entry(state, &now));

    validate_and_write_json(bundle_path, "runtime.runRecord", run_file, &envelope)?;
    println!("OK: run-record transition");
    Ok(())
}

fn run_record_read_command(values: &[String]) -> Result<(), String> {
    if values.len() != 2 {
        return Err(
            "usage: nixfied-kernel run-record read <run-file> <field>".to_string(),
        );
    }

    let envelope = parse_json_file(&values[0], "run-record file")?;
    let payload = object_field(&envelope, "payload")
        .ok_or_else(|| "run-record payload is missing".to_string())?;
    let field = values[1].as_str();
    let rendered = match field {
        "state" | "attempt_id" | "command" | "process_mode" => {
            required_string_field(payload, field, "run-record payload")?.to_string()
        }
        "pid" | "pgid" => object_field(payload, field)
            .and_then(json_value_to_plain_string)
            .unwrap_or_default(),
        other => return Err(format!("unknown run-record read field: {}", other)),
    };

    println!("{}", rendered);
    Ok(())
}

fn task_command(subcommand: &str, values: &[String]) -> Result<(), String> {
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

fn workflow_command(subcommand: &str, values: &[String]) -> Result<(), String> {
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

fn workflow_serial_init_command(values: &[String]) -> Result<(), String> {
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
    let state = build_workflow_serial_state(workflow, &values[2], parse_bool_flag(&values[3])?, &skipped_services);
    write_workflow_serial_state(&values[4], &state)?;
    println!("OK: workflow serial-init");
    Ok(())
}

fn workflow_serial_next_command(values: &[String]) -> Result<(), String> {
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

fn workflow_serial_transition_command(values: &[String]) -> Result<(), String> {
    if values.len() != 7 {
        return Err(
            "usage: nixfied-kernel workflow serial-transition <state-file> <unit-name> <status> <exit-code|empty> <reason|empty> <extra-key|empty> <extra-value|empty>"
                .to_string(),
        );
    }

    let mut state = load_workflow_serial_state(&values[0])?;
    workflow_serial_transition(
        &mut state,
        &values[1],
        &values[2],
        &values[3],
        &values[4],
        &values[5],
        &values[6],
    )?;
    write_workflow_serial_state(&values[0], &state)?;
    println!("OK: workflow serial-transition");
    Ok(())
}

fn workflow_parallel_init_command(values: &[String]) -> Result<(), String> {
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

fn workflow_parallel_next_command(values: &[String]) -> Result<(), String> {
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

fn workflow_parallel_transition_command(values: &[String]) -> Result<(), String> {
    if values.len() != 7 {
        return Err(
            "usage: nixfied-kernel workflow parallel-transition <state-file> <unit-name> <status> <exit-code|empty> <reason|empty> <extra-key|empty> <extra-value|empty>"
                .to_string(),
        );
    }

    let mut state = load_workflow_parallel_state(&values[0])?;
    workflow_parallel_transition(
        &mut state,
        &values[1],
        &values[2],
        &values[3],
        &values[4],
        &values[5],
        &values[6],
    )?;
    write_workflow_parallel_state(&values[0], &state)?;
    println!("OK: workflow parallel-transition");
    Ok(())
}

fn registry_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "append" => registry_append_command(values),
        "replay" => registry_replay_command(values),
        "terminal" => registry_terminal_command(values),
        "runtime-status" => registry_runtime_status_command(values),
        other => Err(format!("unknown registry subcommand: {}", other)),
    }
}

fn registry_append_command(values: &[String]) -> Result<(), String> {
    if values.len() != 9 {
        return Err(
            "usage: nixfied-kernel registry append <bundle-file> <root> <run-id> <attempt-id> <workflow-id> <task-id> <state> <detail-file> <export-file>"
                .to_string(),
        );
    }

    let bundle_path = &values[0];
    let root = &values[1];
    let seq_file = format!("{}/.seq", root);
    let events_file = format!("{}/events.ndjson", root);
    let index_file = format!("{}/events.index.tsv", root);
    fs::create_dir_all(root).map_err(|err| format!("failed to create {}: {}", root, err))?;

    let seq = fs::read_to_string(&seq_file)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(0)
        + 1;
    write_text_atomic(&seq_file, &seq.to_string())?;

    let ts = current_utc_timestamp()?;
    let ts_epoch = current_epoch_seconds()?;
    let detail = parse_json_file(&values[7], "registry event detail file")?;
    let detail_reason = registry_detail_reason(&detail);
    let detail_exit_code = registry_detail_exit_code(&detail);
    let envelope = JsonValue::Object(BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("runtime-event".to_string()),
        ),
        (
            "version".to_string(),
            JsonValue::Number(JsonNumber::from_int(1)),
        ),
        (
            "payload".to_string(),
            JsonValue::Object(BTreeMap::from([
                ("runId".to_string(), JsonValue::String(values[2].clone())),
                ("attemptId".to_string(), nullable_string_value(&values[3])),
                ("workflowId".to_string(), nullable_string_value(&values[4])),
                ("taskId".to_string(), nullable_string_value(&values[5])),
                (
                    "seq".to_string(),
                    JsonValue::Number(JsonNumber::from_int(seq)),
                ),
                ("ts".to_string(), JsonValue::String(ts.clone())),
                ("state".to_string(), JsonValue::String(values[6].clone())),
                ("detail".to_string(), detail),
            ])),
        ),
    ]));
    validate_and_write_json(bundle_path, "runtime.registryEvent", "-", &envelope)?;

    let rendered = render_json_compact(&envelope);
    append_line(&events_file, &rendered)?;
    append_line(
        &index_file,
        &format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            seq,
            ts_epoch,
            ts,
            values[2],
            values[3],
            values[4],
            values[5],
            values[6],
            detail_reason,
            detail_exit_code
        ),
    )?;
    write_shell_exports(
        &values[8],
        &[
            ("REGISTRY_APPEND_LAST_SEQ".to_string(), seq.to_string()),
            (
                "REGISTRY_APPEND_LAST_EVENT_JSON".to_string(),
                rendered.to_string(),
            ),
        ],
    )?;
    println!("OK: registry append");
    Ok(())
}

fn registry_detail_reason(detail: &JsonValue) -> String {
    object_string(detail, "reason").unwrap_or("").to_string()
}

fn registry_detail_exit_code(detail: &JsonValue) -> String {
    object_field(detail, "exitCode")
        .and_then(json_value_to_i64)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn registry_replay_command(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err("usage: nixfied-kernel registry replay <root>".to_string());
    }

    let index_file = format!("{}/events.index.tsv", values[0]);
    if !Path::new(&index_file).exists() {
        println!("{{}}");
        return Ok(());
    }

    let content = read_text(&index_file)?;
    let mut replay = BTreeMap::new();
    for line in content.lines() {
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() < 8 {
            continue;
        }
        let workflow_id = parts[5];
        let task_id = parts[6];
        let state = parts[7];
        let key = if !task_id.is_empty() {
            format!("task:{}", task_id)
        } else if !workflow_id.is_empty() {
            format!("workflow:{}", workflow_id)
        } else {
            continue;
        };
        replay.insert(key, JsonValue::String(state.to_string()));
    }

    println!("{}", render_json_compact(&JsonValue::Object(replay)));
    Ok(())
}

fn registry_terminal_command(values: &[String]) -> Result<(), String> {
    if values.len() != 3 {
        return Err(
            "usage: nixfied-kernel registry terminal <index-file> <run-id> <attempt-id|empty>"
                .to_string(),
        );
    }

    let content = read_text(&values[0])?;
    let run_id = values[1].as_str();
    let attempt_id = values[2].as_str();
    let mut terminal_state = None::<String>;
    let mut exit_code = None::<String>;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }

        if fields[3] != run_id {
            continue;
        }
        if !attempt_id.is_empty() && fields[4] != attempt_id {
            continue;
        }

        match fields[7] {
            "passed" | "failed" | "canceled" => {
                terminal_state = Some(fields[7].to_string());
                exit_code = Some(fields[9].to_string());
            }
            _ => {}
        }
    }

    match terminal_state.as_deref() {
        Some("passed") => println!("passed\t0"),
        Some("canceled") => println!("canceled\t130"),
        Some("failed") => {
            let rendered_code = exit_code
                .as_deref()
                .filter(|value| !value.is_empty())
                .unwrap_or("1");
            println!("failed\t{}", rendered_code);
        }
        _ => println!("unknown\t1"),
    }

    Ok(())
}

fn registry_runtime_status_command(values: &[String]) -> Result<(), String> {
    if values.len() != 3 {
        return Err(
            "usage: nixfied-kernel registry runtime-status <service-index-file> <slot-index-file> <export-file>"
                .to_string(),
        );
    }

    let service_event = registry_latest_event_from_index(&values[0])?;
    let slot_event = registry_latest_event_from_index(&values[1])?;

    let mut registry_found = "0".to_string();
    let mut registry_running = "false".to_string();
    let mut registry_state = "unknown".to_string();
    let mut owner_run_id = String::new();
    let mut owner_scope = String::new();
    let mut ephemeral_root = String::new();
    let mut wait_reason = String::new();
    let mut log_path = String::new();

    if let Some(event) = service_event.as_ref() {
        let payload = object_field(event, "payload")
            .ok_or_else(|| "registry runtime-status service event missing payload".to_string())?;
        registry_found = "1".to_string();
        registry_state = required_string_field(payload, "state", "registry runtime-status payload")?
            .to_string();
        owner_run_id = object_string(payload, "runId").unwrap_or("").to_string();
        registry_running = if matches!(
            registry_state.as_str(),
            "starting" | "running" | "ready" | "degraded" | "waiting" | "busy"
        ) {
            "true".to_string()
        } else {
            "false".to_string()
        };

        if let Some(detail) = object_field(payload, "detail") {
            owner_scope = object_string(detail, "ownerScope").unwrap_or("").to_string();
            ephemeral_root = object_string(detail, "ephemeralRoot")
                .unwrap_or("")
                .to_string();
            wait_reason = object_string(detail, "waitReason").unwrap_or("").to_string();
            log_path = object_string(detail, "logPath").unwrap_or("").to_string();
        }
    }

    let slot_owner = if let Some(event) = slot_event.as_ref() {
        let payload = object_field(event, "payload")
            .ok_or_else(|| "registry runtime-status slot event missing payload".to_string())?;
        let state =
            required_string_field(payload, "state", "registry runtime-status slot payload")?;
        if state == "released" {
            String::new()
        } else {
            object_string(payload, "runId").unwrap_or("").to_string()
        }
    } else {
        String::new()
    };

    write_shell_exports(
        &values[2],
        &[
            ("REGISTRY_FOUND".to_string(), registry_found),
            ("REGISTRY_RUNNING".to_string(), registry_running),
            ("REGISTRY_STATE".to_string(), registry_state),
            ("OWNER_RUN_ID".to_string(), owner_run_id),
            ("OWNER_SCOPE".to_string(), owner_scope),
            ("EPHEMERAL_ROOT".to_string(), ephemeral_root),
            ("WAIT_REASON".to_string(), wait_reason),
            ("LOG_PATH".to_string(), log_path),
            ("SLOT_OWNER".to_string(), slot_owner),
        ],
    )?;
    println!("OK: registry runtime-status");
    Ok(())
}

fn registry_latest_event_from_index(path: &str) -> Result<Option<JsonValue>, String> {
    if path.is_empty() || !Path::new(path).exists() {
        return Ok(None);
    }

    let content = read_text(path)?;
    let mut latest = None;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let _seq = parts.next();
        let Some(event_json) = parts.next() else {
            continue;
        };
        latest = Some(
            parse_json(event_json).map_err(|err| {
                format!(
                    "registry runtime-status index {} contains invalid json: {}",
                    path, err
                )
            })?,
        );
    }
    Ok(latest)
}

fn summary_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "write" => summary_write_command(values),
        "compose" => summary_compose_command(values),
        "collect-steps" => summary_collect_steps_command(values),
        "render-human" => summary_render_human_command(values),
        other => Err(format!("unknown summary subcommand: {}", other)),
    }
}

fn summary_write_command(values: &[String]) -> Result<(), String> {
    if values.len() != 3 {
        return Err(
            "usage: nixfied-kernel summary write <bundle-file> <summary-file> <input-file>"
                .to_string(),
        );
    }

    let input = parse_json_file(&values[2], "summary input file")?;
    let envelope =
        if object_field(&input, "kind").is_some() && object_field(&input, "payload").is_some() {
            input
        } else {
            JsonValue::Object(BTreeMap::from([
                (
                    "kind".to_string(),
                    JsonValue::String("workflow-summary".to_string()),
                ),
                (
                    "version".to_string(),
                    JsonValue::Number(JsonNumber::from_int(1)),
                ),
                ("payload".to_string(), input),
            ]))
        };
    validate_and_write_json(&values[0], "runtime.summary", &values[1], &envelope)?;
    println!("OK: summary write");
    Ok(())
}

fn summary_compose_command(values: &[String]) -> Result<(), String> {
    if values.len() != 24 {
        return Err(
            "usage: nixfied-kernel summary compose <bundle-file> <summary-file> <run-id> <attempt-id> <workflow-id> <mode> <exit-code> <started-at> <finished-at> <duration-seconds> <passed> <failed> <skipped> <canceled> <steps-file> <total-duration> <setup-duration> <steps-duration> <teardown-duration> <accounted-duration> <untracked-duration> <max-workers|empty> <peak-workers|empty> <canceled-count|empty>"
                .to_string(),
        );
    }

    let steps = parse_summary_steps_file(&values[14])?;
    let payload = JsonValue::Object(BTreeMap::from([
        ("run_id".to_string(), JsonValue::String(values[2].clone())),
        (
            "attempt_id".to_string(),
            JsonValue::String(values[3].clone()),
        ),
        (
            "workflow_id".to_string(),
            JsonValue::String(values[4].clone()),
        ),
        ("mode".to_string(), JsonValue::String(values[5].clone())),
        (
            "exit_code".to_string(),
            JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                &values[6],
                "summary compose exit-code",
            )?)),
        ),
        (
            "started_at".to_string(),
            JsonValue::String(values[7].clone()),
        ),
        (
            "finished_at".to_string(),
            JsonValue::String(values[8].clone()),
        ),
        (
            "duration_seconds".to_string(),
            JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                &values[9],
                "summary compose duration-seconds",
            )?)),
        ),
        (
            "counts".to_string(),
            JsonValue::Object(BTreeMap::from([
                (
                    "passed".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[10],
                        "summary compose passed",
                    )?)),
                ),
                (
                    "failed".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[11],
                        "summary compose failed",
                    )?)),
                ),
                (
                    "skipped".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[12],
                        "summary compose skipped",
                    )?)),
                ),
                (
                    "canceled".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[13],
                        "summary compose canceled",
                    )?)),
                ),
            ])),
        ),
        ("steps".to_string(), JsonValue::Array(steps)),
        (
            "timing".to_string(),
            JsonValue::Object(BTreeMap::from([
                (
                    "total_duration".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[15],
                        "summary compose total-duration",
                    )?)),
                ),
                (
                    "setup_duration".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[16],
                        "summary compose setup-duration",
                    )?)),
                ),
                (
                    "steps_duration".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[17],
                        "summary compose steps-duration",
                    )?)),
                ),
                (
                    "teardown_duration".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[18],
                        "summary compose teardown-duration",
                    )?)),
                ),
                (
                    "accounted_duration".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[19],
                        "summary compose accounted-duration",
                    )?)),
                ),
                (
                    "untracked_duration".to_string(),
                    JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                        &values[20],
                        "summary compose untracked-duration",
                    )?)),
                ),
                (
                    "parallelism".to_string(),
                    JsonValue::Object(BTreeMap::from([
                        (
                            "max_workers".to_string(),
                            optional_i64_json_value(parse_optional_i64(
                                &values[21],
                                "summary compose max-workers",
                            )?),
                        ),
                        (
                            "peak_workers".to_string(),
                            optional_i64_json_value(parse_optional_i64(
                                &values[22],
                                "summary compose peak-workers",
                            )?),
                        ),
                        (
                            "canceled_count".to_string(),
                            optional_i64_json_value(parse_optional_i64(
                                &values[23],
                                "summary compose canceled-count",
                            )?),
                        ),
                    ])),
                ),
            ])),
        ),
    ]));
    let envelope = JsonValue::Object(BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("workflow-summary".to_string()),
        ),
        (
            "version".to_string(),
            JsonValue::Number(JsonNumber::from_int(1)),
        ),
        ("payload".to_string(), payload),
    ]));

    validate_and_write_json(&values[0], "runtime.summary", &values[1], &envelope)?;
    println!("OK: summary compose");
    Ok(())
}

fn summary_collect_steps_command(values: &[String]) -> Result<(), String> {
    if values.len() != 6 {
        return Err(
            "usage: nixfied-kernel summary collect-steps <plan-file> <index-file> <run-id> <attempt-id|empty> <steps-file> <export-file>"
                .to_string(),
        );
    }

    let plan = load_workflow_summary_plan(&values[0])?;
    let collected = collect_workflow_summary(&plan, &values[1], &values[2], &values[3])?;
    write_workflow_collected_steps_file(&values[4], &collected.steps)?;
    write_shell_exports(
        &values[5],
        &[
            (
                "WORKFLOW_PASSED_COUNT".to_string(),
                collected.passed.to_string(),
            ),
            (
                "WORKFLOW_FAILED_COUNT".to_string(),
                collected.failed.to_string(),
            ),
            (
                "WORKFLOW_SKIPPED_COUNT".to_string(),
                collected.skipped.to_string(),
            ),
            (
                "WORKFLOW_CANCELED_COUNT".to_string(),
                collected.canceled.to_string(),
            ),
            (
                "WORKFLOW_STEPS_DURATION".to_string(),
                collected.steps_duration.to_string(),
            ),
            (
                "WORKFLOW_PEAK_WORKERS".to_string(),
                collected.peak_workers.to_string(),
            ),
            (
                "WORKFLOW_LEAF_TASK_IDS_LINES".to_string(),
                collected.leaf_task_ids_lines,
            ),
        ],
    )?;
    println!("OK: summary collect-steps");
    Ok(())
}

fn summary_render_human_command(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err("usage: nixfied-kernel summary render-human <summary-file>".to_string());
    }

    let summary = parse_json_file(&values[0], "summary file")?;
    let payload = object_field(&summary, "payload")
        .ok_or_else(|| "summary payload is missing".to_string())?;
    let steps = object_array(payload, "steps").unwrap_or(&[]);
    let counts = object_field(payload, "counts")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "summary counts are missing".to_string())?;
    let timing = object_field(payload, "timing")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "summary timing is missing".to_string())?;
    let total_duration = object_field(payload, "duration_seconds")
        .and_then(json_value_to_i64)
        .unwrap_or(0);
    let exit_code = object_field(payload, "exit_code")
        .and_then(json_value_to_i64)
        .unwrap_or(1);
    let skipped = counts
        .get("skipped")
        .and_then(json_value_to_i64)
        .unwrap_or(0);
    let parallel = timing
        .get("parallelism")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();

    println!();
    println!("------------------------------------------------------------");
    println!("Summary");
    println!("------------------------------------------------------------");
    println!("Source: {}", values[0]);
    for step in steps {
        let status = object_string(step, "status").unwrap_or("failed");
        let marker = match status {
            "passed" => "PASS",
            "skipped" => "SKIP",
            _ => "FAIL",
        };
        let name = object_string(step, "name").unwrap_or("unknown");
        let duration = object_field(step, "duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0);
        println!("  [{}] {} ({}s)", marker, name, duration);
    }
    println!("Total time: {}", format_duration_seconds(total_duration));
    println!(
        "INFO: Time breakdown setup={}s steps={}s teardown={}s accounted={}s untracked={}s",
        timing
            .get("setup_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        timing
            .get("steps_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        timing
            .get("teardown_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        timing
            .get("accounted_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0),
        timing
            .get("untracked_duration")
            .and_then(json_value_to_i64)
            .unwrap_or(0)
    );
    println!(
        "INFO: Parallelism max_workers={} peak_workers={} canceled_count={}",
        parallel
            .get("max_workers")
            .map(json_scalar_or_placeholder)
            .unwrap_or_else(|| "?".to_string()),
        parallel
            .get("peak_workers")
            .map(json_scalar_or_placeholder)
            .unwrap_or_else(|| "?".to_string()),
        parallel
            .get("canceled_count")
            .map(json_scalar_or_placeholder)
            .unwrap_or_else(|| "?".to_string())
    );
    if skipped > 0 {
        println!("INFO: SKIP: {} task(s) skipped", skipped);
    }
    if exit_code == 0 {
        println!("OK: Exit code: 0");
    } else {
        println!("ERROR: Exit code: {}", exit_code);
    }
    if let Some(parent) = Path::new(&values[0]).parent() {
        println!();
        println!("Artifacts: {}", parent.display());
    }
    println!("------------------------------------------------------------");
    Ok(())
}

fn validate_and_write_json(
    bundle_path: &str,
    contract_ref: &str,
    output_path: &str,
    value: &JsonValue,
) -> Result<(), String> {
    let bundle = parse_json_file(bundle_path, "validation bundle")?;
    validate_json_value_against_contract(&bundle, contract_ref, value)?;
    if output_path != "-" {
        write_text_atomic(output_path, &format!("{}\n", render_json_compact(value)))?;
    }
    Ok(())
}

fn parse_json_file(path: &str, label: &str) -> Result<JsonValue, String> {
    let text = read_text(path)?;
    parse_json(&text).map_err(|err| format!("{} {} is not valid JSON: {}", label, path, err))
}

fn run_record_history_entry(state: &str, at: &str) -> JsonValue {
    JsonValue::Object(BTreeMap::from([
        ("state".to_string(), JsonValue::String(state.to_string())),
        ("at".to_string(), JsonValue::String(at.to_string())),
    ]))
}

fn parse_bool_flag(value: &str) -> Result<bool, String> {
    match value {
        "1" | "true" | "TRUE" => Ok(true),
        "0" | "false" | "FALSE" => Ok(false),
        other => Err(format!("expected boolean flag, got {}", other)),
    }
}

fn nullable_string_value(value: &str) -> JsonValue {
    if value.is_empty() {
        JsonValue::Null
    } else {
        JsonValue::String(value.to_string())
    }
}

fn optional_string_value(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_optional_i64(value: &str, label: &str) -> Result<Option<i64>, String> {
    if value.is_empty() || value == "null" {
        Ok(None)
    } else {
        parse_i64_text(value, label).map(Some)
    }
}

fn parse_optional_bool_text(value: &str, label: &str) -> Result<Option<bool>, String> {
    match value {
        "" | "null" => Ok(None),
        "1" | "true" | "TRUE" => Ok(Some(true)),
        "0" | "false" | "FALSE" => Ok(Some(false)),
        other => Err(format!("{} must be bool or empty (got '{}')", label, other)),
    }
}

fn next_flag_value(values: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    let value = values
        .get(*index)
        .ok_or_else(|| format!("missing value for {}", flag))?
        .clone();
    *index += 1;
    Ok(value)
}

fn insert_optional_string_field(
    fields: &mut BTreeMap<String, JsonValue>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        fields.insert(key.to_string(), JsonValue::String(value));
    }
}

fn insert_optional_number_field(
    fields: &mut BTreeMap<String, JsonValue>,
    key: &str,
    value: Option<i64>,
) {
    if let Some(value) = value {
        fields.insert(
            key.to_string(),
            JsonValue::Number(JsonNumber::from_int(value)),
        );
    }
}

fn insert_optional_bool_field(
    fields: &mut BTreeMap<String, JsonValue>,
    key: &str,
    value: Option<bool>,
) {
    if let Some(value) = value {
        fields.insert(key.to_string(), JsonValue::Bool(value));
    }
}

fn insert_optional_json_field(
    fields: &mut BTreeMap<String, JsonValue>,
    key: &str,
    value: Option<JsonValue>,
) {
    if let Some(value) = value {
        fields.insert(key.to_string(), value);
    }
}

fn optional_i64_json_value(value: Option<i64>) -> JsonValue {
    match value {
        Some(value) => JsonValue::Number(JsonNumber::from_int(value)),
        None => JsonValue::Null,
    }
}

fn parse_tab_separated_name_value_file(
    path: &str,
    label: &str,
) -> Result<Vec<(String, String)>, String> {
    let text = read_text(path)?;
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once('\t').ok_or_else(|| {
            format!(
                "{} {} must contain tab-separated name/value pairs",
                label, path
            )
        })?;
        entries.push((name.to_string(), value.to_string()));
    }
    Ok(entries)
}

fn load_line_set(path: &str) -> Result<BTreeSet<String>, String> {
    let text = read_text(path)?;
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect())
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

enum WorkflowSerialAction {
    Execute {
        unit_name: String,
        task_id: String,
        selected_services_csv: String,
    },
    Cancel {
        unit_name: String,
        task_id: String,
        reason: String,
        extra_key: String,
        extra_value: String,
    },
    Done {
        workflow_status: i64,
    },
}

enum WorkflowParallelAction {
    Start {
        unit_name: String,
        task_id: String,
        selected_services_csv: String,
        produces_json: String,
    },
    Cancel {
        unit_name: String,
        task_id: String,
        reason: String,
        extra_key: String,
        extra_value: String,
    },
    SignalRunning {
        unit_name: String,
    },
    Wait,
    Done {
        workflow_status: i64,
    },
}

fn load_workflow_scheduler_plan(path: &str) -> Result<WorkflowSchedulerPlan, String> {
    let value = parse_json_file(path, "workflow scheduler plan")?;
    let kind = required_string_field(&value, "kind", "workflow scheduler plan")?;
    if kind != "nixfied-workflow-scheduler-plan" {
        return Err(format!("unsupported workflow scheduler plan kind: {}", kind));
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

fn build_workflow_serial_state(
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

fn workflow_unit_direct_cancel_reason(
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

    let when_env_equals_matches = unit.when_env_equals.iter().all(|(env_name, expected)| {
        env::var(env_name.as_str()).unwrap_or_default() == *expected
    });
    if !unit.when_env_equals.is_empty() && !when_env_equals_matches {
        return Some(("when-false".to_string(), String::new(), String::new()));
    }

    None
}

fn workflow_serial_mark_canceled(
    units: &mut BTreeMap<String, WorkflowSerialUnitState>,
    unit_name: &str,
    reason: &str,
    extra_key: &str,
    extra_value: &str,
) {
    let Some(entry) = units.get_mut(unit_name) else {
        return;
    };
    if entry.state != "pending" && entry.state != "ready" {
        return;
    }
    entry.state = "cancel-pending".to_string();
    entry.cancel_reason = reason.to_string();
    entry.cancel_extra_key = extra_key.to_string();
    entry.cancel_extra_value = extra_value.to_string();
}

fn workflow_serial_mark_dependency_canceled_recursive(
    units: &mut BTreeMap<String, WorkflowSerialUnitState>,
    unit_name: &str,
    reason: &str,
    dependency_task_id: &str,
) {
    workflow_serial_mark_canceled(units, unit_name, reason, "dependency", dependency_task_id);
    let dependents = units
        .get(unit_name)
        .map(|entry| entry.dependents.clone())
        .unwrap_or_default();
    for dependent in dependents {
        workflow_serial_mark_dependency_canceled_recursive(
            units,
            &dependent,
            reason,
            dependency_task_id,
        );
    }
}

fn workflow_serial_next_action(state: &WorkflowSerialState) -> WorkflowSerialAction {
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

fn workflow_serial_transition(
    state: &mut WorkflowSerialState,
    unit_name: &str,
    status: &str,
    exit_code_text: &str,
    reason: &str,
    extra_key: &str,
    extra_value: &str,
) -> Result<(), String> {
    let dependents = state
        .units
        .get(unit_name)
        .map(|unit| unit.dependents.clone())
        .ok_or_else(|| format!("unknown workflow serial unit '{}'", unit_name))?;
    let task_id = state
        .units
        .get(unit_name)
        .map(|unit| unit.task_id.clone())
        .unwrap_or_default();

    match status {
        "canceled" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow serial unit '{}'", unit_name))?;
            unit.state = "canceled".to_string();
            if !reason.is_empty() {
                unit.cancel_reason = reason.to_string();
                unit.cancel_extra_key = extra_key.to_string();
                unit.cancel_extra_value = extra_value.to_string();
            }
        }
        "passed" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow serial unit '{}'", unit_name))?;
            unit.state = "passed".to_string();
            for dependent in dependents {
                if let Some(entry) = state.units.get_mut(&dependent) {
                    if entry.state == "pending" && entry.needs_left > 0 {
                        entry.needs_left -= 1;
                        if entry.needs_left == 0 {
                            entry.state = "ready".to_string();
                        }
                    }
                }
            }
        }
        "failed" => {
            let exit_code = if exit_code_text.is_empty() {
                1
            } else {
                parse_i64_text(exit_code_text, "workflow serial failed exit-code")?
            };
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow serial unit '{}'", unit_name))?;
            unit.state = "failed".to_string();
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

fn load_workflow_serial_state(path: &str) -> Result<WorkflowSerialState, String> {
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
        .unwrap_or(&[])
        .iter()
        .filter_map(JsonValue::as_string)
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
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(JsonValue::as_string)
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

fn write_workflow_serial_state(path: &str, state: &WorkflowSerialState) -> Result<(), String> {
    let mut units = BTreeMap::new();
    for (unit_name, unit) in &state.units {
        units.insert(
            unit_name.clone(),
            JsonValue::Object(BTreeMap::from([
                ("name".to_string(), JsonValue::String(unit.name.clone())),
                ("taskId".to_string(), JsonValue::String(unit.task_id.clone())),
                (
                    "needsLeft".to_string(),
                    JsonValue::Number(JsonNumber::from_int(unit.needs_left)),
                ),
                (
                    "dependents".to_string(),
                    JsonValue::Array(
                        unit.dependents
                            .iter()
                            .map(|item| JsonValue::String(item.clone()))
                            .collect(),
                    ),
                ),
                ("state".to_string(), JsonValue::String(unit.state.clone())),
                (
                    "cancelReason".to_string(),
                    JsonValue::String(unit.cancel_reason.clone()),
                ),
                (
                    "cancelExtraKey".to_string(),
                    JsonValue::String(unit.cancel_extra_key.clone()),
                ),
                (
                    "cancelExtraValue".to_string(),
                    JsonValue::String(unit.cancel_extra_value.clone()),
                ),
                (
                    "selectedServicesCsv".to_string(),
                    JsonValue::String(unit.selected_services_csv.clone()),
                ),
            ])),
        );
    }

    let value = JsonValue::Object(BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("nixfied-workflow-serial-state".to_string()),
        ),
        (
            "version".to_string(),
            JsonValue::Number(JsonNumber::from_int(1)),
        ),
        (
            "workflowId".to_string(),
            JsonValue::String(state.workflow_id.clone()),
        ),
        ("failFast".to_string(), JsonValue::Bool(state.fail_fast)),
        (
            "workflowStatus".to_string(),
            JsonValue::Number(JsonNumber::from_int(state.workflow_status)),
        ),
        ("halted".to_string(), JsonValue::Bool(state.halted)),
        (
            "order".to_string(),
            JsonValue::Array(
                state
                    .order
                    .iter()
                    .map(|item| JsonValue::String(item.clone()))
                    .collect(),
            ),
        ),
        ("units".to_string(), JsonValue::Object(units)),
    ]));
    write_text_atomic(path, &format!("{}\n", render_json_compact(&value)))
}

fn build_workflow_parallel_state(
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
            workflow_parallel_mark_canceled(&mut units, &unit.name, &reason.0, &reason.1, &reason.2);
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

fn workflow_parallel_mark_canceled(
    units: &mut BTreeMap<String, WorkflowParallelUnitState>,
    unit_name: &str,
    reason: &str,
    extra_key: &str,
    extra_value: &str,
) {
    let Some(entry) = units.get_mut(unit_name) else {
        return;
    };
    if entry.state != "pending" && entry.state != "ready" {
        return;
    }
    entry.state = "cancel-pending".to_string();
    entry.cancel_reason = reason.to_string();
    entry.cancel_extra_key = extra_key.to_string();
    entry.cancel_extra_value = extra_value.to_string();
}

fn workflow_parallel_mark_dependency_canceled_recursive(
    units: &mut BTreeMap<String, WorkflowParallelUnitState>,
    unit_name: &str,
    reason: &str,
    dependency_task_id: &str,
) {
    workflow_parallel_mark_canceled(units, unit_name, reason, "dependency", dependency_task_id);
    let dependents = units
        .get(unit_name)
        .map(|entry| entry.dependents.clone())
        .unwrap_or_default();
    for dependent in dependents {
        workflow_parallel_mark_dependency_canceled_recursive(
            units,
            &dependent,
            reason,
            dependency_task_id,
        );
    }
}

fn workflow_parallel_mark_all_pending_ready(
    units: &mut BTreeMap<String, WorkflowParallelUnitState>,
    reason: &str,
) {
    let unit_names = units.keys().cloned().collect::<Vec<_>>();
    for unit_name in unit_names {
        workflow_parallel_mark_canceled(units, &unit_name, reason, "", "");
    }
}

fn workflow_parallel_request_running_cancel(
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

fn workflow_parallel_unit_holds_locks(state: &str) -> bool {
    matches!(
        state,
        "running" | "cancel-running-requested" | "cancel-running-signaled"
    )
}

fn workflow_parallel_running_count(state: &WorkflowParallelState) -> i64 {
    state
        .units
        .values()
        .filter(|unit| workflow_parallel_unit_holds_locks(&unit.state))
        .count() as i64
}

fn workflow_parallel_completed_count(state: &WorkflowParallelState) -> i64 {
    state
        .units
        .values()
        .filter(|unit| matches!(unit.state.as_str(), "passed" | "failed" | "canceled"))
        .count() as i64
}

fn workflow_parallel_unit_has_lock_conflict(
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

fn workflow_parallel_mark_blocked_if_needed(state: &mut WorkflowParallelState) {
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

fn workflow_parallel_next_action(state: &mut WorkflowParallelState) -> WorkflowParallelAction {
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

fn workflow_parallel_transition(
    state: &mut WorkflowParallelState,
    unit_name: &str,
    status: &str,
    exit_code_text: &str,
    reason: &str,
    extra_key: &str,
    extra_value: &str,
) -> Result<(), String> {
    let dependents = state
        .units
        .get(unit_name)
        .map(|unit| unit.dependents.clone())
        .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
    let task_id = state
        .units
        .get(unit_name)
        .map(|unit| unit.task_id.clone())
        .unwrap_or_default();

    match status {
        "started" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            unit.state = "running".to_string();
        }
        "canceled" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            unit.state = "canceled".to_string();
            if !reason.is_empty() {
                unit.cancel_reason = reason.to_string();
                unit.cancel_extra_key = extra_key.to_string();
                unit.cancel_extra_value = extra_value.to_string();
            }
        }
        "signal-sent" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            if unit.state == "cancel-running-requested" {
                unit.state = "cancel-running-signaled".to_string();
            }
        }
        "canceled-running" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            unit.state = "canceled".to_string();
        }
        "passed" => {
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            unit.state = "passed".to_string();
            for dependent in dependents {
                if let Some(entry) = state.units.get_mut(&dependent) {
                    if entry.state == "pending" && entry.needs_left > 0 {
                        entry.needs_left -= 1;
                        if entry.needs_left == 0 {
                            entry.state = "ready".to_string();
                        }
                    }
                }
            }
        }
        "failed" => {
            let exit_code = if exit_code_text.is_empty() {
                1
            } else {
                parse_i64_text(exit_code_text, "workflow parallel failed exit-code")?
            };
            let unit = state
                .units
                .get_mut(unit_name)
                .ok_or_else(|| format!("unknown workflow parallel unit '{}'", unit_name))?;
            unit.state = "failed".to_string();
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

fn load_workflow_parallel_state(path: &str) -> Result<WorkflowParallelState, String> {
    let value = parse_json_file(path, "workflow parallel state")?;
    let kind = required_string_field(&value, "kind", "workflow parallel state")?;
    if kind != "nixfied-workflow-parallel-state" {
        return Err(format!("unsupported workflow parallel state kind: {}", kind));
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
        .unwrap_or(&[])
        .iter()
        .filter_map(JsonValue::as_string)
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
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(JsonValue::as_string)
                    .map(|item| item.to_string())
                    .collect(),
                locks: object_field(unit_value, "locks")
                    .and_then(JsonValue::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(JsonValue::as_string)
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

fn write_workflow_parallel_state(path: &str, state: &WorkflowParallelState) -> Result<(), String> {
    let mut units = BTreeMap::new();
    for (unit_name, unit) in &state.units {
        units.insert(
            unit_name.clone(),
            JsonValue::Object(BTreeMap::from([
                ("name".to_string(), JsonValue::String(unit.name.clone())),
                ("taskId".to_string(), JsonValue::String(unit.task_id.clone())),
                (
                    "needsLeft".to_string(),
                    JsonValue::Number(JsonNumber::from_int(unit.needs_left)),
                ),
                (
                    "dependents".to_string(),
                    JsonValue::Array(
                        unit.dependents
                            .iter()
                            .map(|item| JsonValue::String(item.clone()))
                            .collect(),
                    ),
                ),
                (
                    "locks".to_string(),
                    JsonValue::Array(
                        unit.locks
                            .iter()
                            .map(|item| JsonValue::String(item.clone()))
                            .collect(),
                    ),
                ),
                ("state".to_string(), JsonValue::String(unit.state.clone())),
                (
                    "cancelReason".to_string(),
                    JsonValue::String(unit.cancel_reason.clone()),
                ),
                (
                    "cancelExtraKey".to_string(),
                    JsonValue::String(unit.cancel_extra_key.clone()),
                ),
                (
                    "cancelExtraValue".to_string(),
                    JsonValue::String(unit.cancel_extra_value.clone()),
                ),
                (
                    "selectedServicesCsv".to_string(),
                    JsonValue::String(unit.selected_services_csv.clone()),
                ),
                (
                    "producesJson".to_string(),
                    JsonValue::String(unit.produces_json.clone()),
                ),
            ])),
        );
    }

    let value = JsonValue::Object(BTreeMap::from([
        (
            "kind".to_string(),
            JsonValue::String("nixfied-workflow-parallel-state".to_string()),
        ),
        (
            "version".to_string(),
            JsonValue::Number(JsonNumber::from_int(1)),
        ),
        (
            "workflowId".to_string(),
            JsonValue::String(state.workflow_id.clone()),
        ),
        ("failFast".to_string(), JsonValue::Bool(state.fail_fast)),
        (
            "maxWorkers".to_string(),
            JsonValue::Number(JsonNumber::from_int(state.max_workers)),
        ),
        (
            "workflowStatus".to_string(),
            JsonValue::Number(JsonNumber::from_int(state.workflow_status)),
        ),
        (
            "stopScheduling".to_string(),
            JsonValue::Bool(state.stop_scheduling),
        ),
        (
            "order".to_string(),
            JsonValue::Array(
                state
                    .order
                    .iter()
                    .map(|item| JsonValue::String(item.clone()))
                    .collect(),
            ),
        ),
        ("units".to_string(), JsonValue::Object(units)),
    ]));
    write_text_atomic(path, &format!("{}\n", render_json_compact(&value)))
}

fn load_workflow_summary_plan(path: &str) -> Result<WorkflowSummaryPlan, String> {
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

fn workflow_step_status_from_state_reason(state: &str, reason: &str) -> String {
    if state == "canceled"
        && matches!(
            reason,
            "missing-env" | "when-false" | "service-skipped" | "dependency-skipped"
        )
    {
        "skipped".to_string()
    } else {
        state.to_string()
    }
}

fn collect_workflow_summary(
    plan: &WorkflowSummaryPlan,
    index_file: &str,
    run_id: &str,
    attempt_id: &str,
) -> Result<WorkflowCollectedSummary, String> {
    if !Path::new(index_file).exists() {
        return Ok(WorkflowCollectedSummary {
            steps: Vec::new(),
            passed: 0,
            failed: 0,
            skipped: 0,
            canceled: 0,
            steps_duration: 0,
            peak_workers: 0,
            leaf_task_ids_lines: String::new(),
        });
    }

    let content = read_text(index_file)?;
    let mut active_order_seq = BTreeMap::<String, i64>::new();
    let mut active_workflow_id = BTreeMap::<String, String>::new();
    let mut active_running_epoch = BTreeMap::<String, i64>::new();
    let mut terminal_rows = Vec::<WorkflowTerminalRow>::new();
    let mut running_workers = 0i64;
    let mut peak_workers = 0i64;

    for (row_index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }

        if fields[3] != run_id {
            continue;
        }
        if !attempt_id.is_empty() && fields[4] != attempt_id {
            continue;
        }

        let workflow_id = fields[5];
        let task_id = fields[6];
        let state = fields[7];
        let reason = fields[8];
        let exit_code = fields[9];
        if task_id.is_empty() {
            continue;
        }

        let runner_type = plan
            .task_runner_types
            .get(task_id)
            .map(|value| value.as_str())
            .unwrap_or("shell");
        let counts_for_peak = runner_type != "workflowRef";
        if counts_for_peak {
            match state {
                "running" => {
                    running_workers += 1;
                    if running_workers > peak_workers {
                        peak_workers = running_workers;
                    }
                }
                "passed" | "failed" | "canceled" => {
                    if running_workers > 0 {
                        running_workers -= 1;
                    }
                }
                _ => {}
            }
        }

        if !matches!(state, "queued" | "running" | "passed" | "failed" | "canceled") {
            continue;
        }

        let key = format!("{}\u{1f}{}", workflow_id, task_id);
        let seq = fields[0].parse::<i64>().ok().unwrap_or(0);
        let ts_epoch = fields[1].parse::<i64>().ok();

        match state {
            "queued" | "running" => {
                if active_order_seq
                    .get(&key)
                    .map(|value| seq < *value)
                    .unwrap_or(true)
                {
                    active_order_seq.insert(key.clone(), seq);
                }
                active_workflow_id.insert(key.clone(), workflow_id.to_string());
                if state == "running" {
                    if let Some(ts_epoch) = ts_epoch {
                        active_running_epoch.insert(key, ts_epoch);
                    }
                }
            }
            "passed" | "failed" | "canceled" => {
                let order = active_order_seq.remove(&key).unwrap_or(seq);
                let entry_workflow_id = active_workflow_id
                    .remove(&key)
                    .unwrap_or_else(|| workflow_id.to_string());
                let duration = match (active_running_epoch.remove(&key), ts_epoch) {
                    (Some(started_at), Some(finished_at)) if finished_at >= started_at => {
                        finished_at - started_at
                    }
                    _ => 0,
                };
                terminal_rows.push(WorkflowTerminalRow {
                    order,
                    row_index,
                    name: task_id.to_string(),
                    workflow_id: entry_workflow_id,
                    state: state.to_string(),
                    duration,
                    reason: reason.to_string(),
                    exit_code: exit_code.to_string(),
                });
            }
            _ => {}
        }
    }

    terminal_rows.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then(left.row_index.cmp(&right.row_index))
    });

    let mut steps = Vec::new();
    let mut passed = 0i64;
    let mut failed = 0i64;
    let mut skipped = 0i64;
    let mut canceled = 0i64;
    let mut steps_duration = 0i64;
    let mut seen_leaf_tasks = BTreeSet::new();
    let mut leaf_task_ids = Vec::new();

    for row in terminal_rows {
        let runner_type = plan
            .task_runner_types
            .get(&row.name)
            .map(|value| value.as_str())
            .unwrap_or("shell");
        if runner_type == "workflowRef" {
            continue;
        }

        let status = workflow_step_status_from_state_reason(&row.state, &row.reason);
        match row.state.as_str() {
            "passed" => passed += 1,
            "failed" => failed += 1,
            "canceled" => {
                if status == "skipped" {
                    skipped += 1;
                } else {
                    canceled += 1;
                }
            }
            _ => {}
        }
        steps_duration += row.duration;
        if seen_leaf_tasks.insert(row.name.clone()) {
            leaf_task_ids.push(row.name.clone());
        }
        steps.push(WorkflowCollectedStep {
            name: row.name,
            status,
            state: row.state,
            duration: row.duration,
            order: row.order,
            workflow_id: row.workflow_id,
            reason: row.reason,
            exit_code: row.exit_code,
        });
    }

    Ok(WorkflowCollectedSummary {
        steps,
        passed,
        failed,
        skipped,
        canceled,
        steps_duration,
        peak_workers,
        leaf_task_ids_lines: leaf_task_ids.join("\n"),
    })
}

fn write_workflow_collected_steps_file(
    path: &str,
    steps: &[WorkflowCollectedStep],
) -> Result<(), String> {
    if path == "-" {
        return Ok(());
    }

    let mut rendered = String::new();
    for step in steps {
        rendered.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            step.name,
            step.status,
            step.duration,
            step.state,
            step.order,
            step.workflow_id,
            step.reason,
            step.exit_code
        ));
    }
    write_text_atomic(path, &rendered)
}

fn parse_summary_steps_file(path: &str) -> Result<Vec<JsonValue>, String> {
    let text = read_text(path)?;
    let mut steps = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() != 8 {
            return Err(format!(
                "summary steps file {} must contain 8 tab-separated fields per line",
                path
            ));
        }
        steps.push(JsonValue::Object(BTreeMap::from([
            ("name".to_string(), JsonValue::String(parts[0].to_string())),
            (
                "status".to_string(),
                JsonValue::String(parts[1].to_string()),
            ),
            ("state".to_string(), JsonValue::String(parts[3].to_string())),
            (
                "duration".to_string(),
                JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                    parts[2],
                    "summary step duration",
                )?)),
            ),
            (
                "order".to_string(),
                JsonValue::Number(JsonNumber::from_int(parse_i64_text(
                    parts[4],
                    "summary step order",
                )?)),
            ),
            ("workflow_id".to_string(), nullable_string_value(parts[5])),
            ("reason".to_string(), nullable_string_value(parts[6])),
            (
                "exit_code".to_string(),
                optional_i64_json_value(parse_optional_i64(parts[7], "summary step exit_code")?),
            ),
        ])));
    }
    Ok(steps)
}

fn append_line(path: &str, line: &str) -> Result<(), String> {
    let target = Path::new(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {}", parent.display(), err))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("failed to open {}: {}", path, err))?;
    file.write_all(line.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|err| format!("failed to append {}: {}", path, err))
}

fn current_utc_timestamp() -> Result<String, String> {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .map_err(|err| format!("failed to run date: {}", err))?;
    if !output.status.success() {
        return Err("failed to compute UTC timestamp".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn current_epoch_seconds() -> Result<i64, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| format!("failed to compute epoch seconds: {}", err))?;
    Ok(i64::try_from(now.as_secs()).unwrap_or(i64::MAX))
}

fn print_json_scalar(value: &JsonValue) {
    match value {
        JsonValue::Null => println!(),
        JsonValue::Bool(value) => println!("{}", value),
        JsonValue::String(value) => println!("{}", value),
        JsonValue::Number(number) => println!("{}", number.raw),
        JsonValue::Array(_) | JsonValue::Object(_) => println!("{}", render_json_compact(value)),
    }
}

fn format_duration_seconds(seconds: i64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

fn json_scalar_or_placeholder(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "?".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::String(value) => value.clone(),
        JsonValue::Number(number) => number.raw.clone(),
        JsonValue::Array(_) | JsonValue::Object(_) => render_json_compact(value),
    }
}

fn adapter_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "decode" => adapter_decode_command(values),
        other => Err(format!("unknown adapter subcommand: {}", other)),
    }
}

fn adapter_decode_command(values: &[String]) -> Result<(), String> {
    let kind = values
        .first()
        .ok_or_else(|| "usage: nixfied-kernel adapter decode <kind> ...".to_string())?;
    match kind.as_str() {
        "supervisor-status" => adapter_decode_supervisor_status(&values[1..]),
        "helios-finalized-slot" => adapter_decode_helios_finalized_slot(&values[1..]),
        "helios-checkpoint-root" => adapter_decode_helios_checkpoint_root(&values[1..]),
        other => Err(format!("unknown adapter decode kind: {}", other)),
    }
}

fn adapter_decode_supervisor_status(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err(
            "usage: nixfied-kernel adapter decode supervisor-status <json-file>".to_string(),
        );
    }
    let payload = parse_json_file(&values[0], "supervisor payload")?;
    let rows = payload
        .as_array()
        .ok_or_else(|| "supervisor payload must be an array".to_string())?;
    for row in rows {
        let object = row
            .as_object()
            .ok_or_else(|| "supervisor process row must be an object".to_string())?;
        println!(
            "{}\t{}\t{}\t{}",
            object
                .get("name")
                .and_then(JsonValue::as_string)
                .unwrap_or(""),
            object
                .get("status")
                .and_then(JsonValue::as_string)
                .unwrap_or(""),
            object
                .get("is_running")
                .and_then(JsonValue::as_bool)
                .map(|value| value.to_string())
                .unwrap_or_default(),
            object
                .get("is_ready")
                .and_then(JsonValue::as_bool)
                .map(|value| value.to_string())
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn adapter_decode_helios_finalized_slot(values: &[String]) -> Result<(), String> {
    if values.len() != 1 {
        return Err(
            "usage: nixfied-kernel adapter decode helios-finalized-slot <json-file>".to_string(),
        );
    }
    let payload = parse_json_file(&values[0], "helios finalized payload")?;
    let selected = resolve_json_path(&payload, ".data.header.message.slot")
        .ok_or_else(|| "helios finalized slot is missing".to_string())?;
    let number = json_value_to_number_string(selected)
        .ok_or_else(|| "helios finalized slot is not numeric".to_string())?;
    println!("{}", number);
    Ok(())
}

fn adapter_decode_helios_checkpoint_root(values: &[String]) -> Result<(), String> {
    if values.is_empty() || values.len() > 2 {
        return Err(
            "usage: nixfied-kernel adapter decode helios-checkpoint-root <json-file> [--empty-ok]"
                .to_string(),
        );
    }
    let empty_ok = values
        .get(1)
        .map(|value| value == "--empty-ok")
        .unwrap_or(false);
    let payload = parse_json_file(&values[0], "helios checkpoint payload")?;
    let selected = resolve_json_path(&payload, ".data.root");
    match selected.and_then(JsonValue::as_string) {
        Some(value) => {
            println!("{}", value);
            Ok(())
        }
        None if empty_ok => Ok(()),
        None => Err("helios checkpoint root is missing".to_string()),
    }
}

fn probe_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "evaluate" => probe_evaluate_command(values),
        "jsonrpc" => probe_jsonrpc_command(values),
        other => Err(format!("unknown probe subcommand: {}", other)),
    }
}

fn probe_evaluate_command(values: &[String]) -> Result<(), String> {
    let plan_path = values.first().ok_or_else(|| {
        "usage: nixfied-kernel probe evaluate <plan-file> [payload-file] [export-file]"
            .to_string()
    })?;
    if values.len() > 3 {
        return Err(
            "usage: nixfied-kernel probe evaluate <plan-file> [payload-file] [export-file]"
                .to_string(),
        );
    }

    let plan_value = parse_json_file(plan_path, "probe plan")?;
    let plan_kind = required_string_field(&plan_value, "kind", "probe plan")?;
    match plan_kind {
        "nixfied-probe-plan" => {
            let payload_path = values.get(1).ok_or_else(|| {
                "usage: nixfied-kernel probe evaluate <plan-file> <payload-file> [export-file]"
                    .to_string()
            })?;
            let export_path = values.get(2).map(|value| value.as_str());
            let plan = load_probe_plan_from_value(&plan_value)?;
            let payload = parse_json_file(payload_path, "probe payload")?;
            let exports = probe_plan_exports(&plan, &payload)?;

            if let Some(export_path) = export_path {
                write_shell_exports(export_path, &exports)?;
            } else if !exports.is_empty() {
                return Err("probe evaluate requires export-file when plan emits exports".to_string());
            }

            println!("OK: probe evaluate kind={}", plan.probe_kind);
            Ok(())
        }
        "nixfied-probe-execution-plan" => {
            if values.len() != 1 {
                return Err(
                    "usage: nixfied-kernel probe evaluate <execution-plan-file>".to_string(),
                );
            }
            let plan = load_probe_execution_plan_from_value(&plan_value)?;
            execute_probe_execution_plan(&plan)
        }
        other => Err(format!("unknown probe plan kind: {}", other)),
    }
}

fn probe_jsonrpc_command(values: &[String]) -> Result<(), String> {
    if values.len() != 5 {
        return Err(
            "usage: nixfied-kernel probe jsonrpc <plan-file> <curl-bin> <url> <method> <max-time>"
                .to_string(),
        );
    }

    let max_time = parse_i64_text(&values[4], "probe jsonrpc max-time")?;
    if max_time < 1 {
        return Err(format!(
            "probe jsonrpc max-time must be positive, got {}",
            max_time
        ));
    }

    let plan = load_probe_plan(&values[0])?;
    let payload = request_jsonrpc_payload(&values[1], &values[2], &values[3], max_time)?;
    let exports = probe_plan_exports(&plan, &payload)?;

    if let Some((_, value)) = exports.first() {
        println!("{}", value);
    }

    Ok(())
}

fn execute_probe_execution_plan(plan: &ProbeExecutionPlan) -> Result<(), String> {
    let _ = (&plan.mode, &plan.service_name);
    let probe_source = env::var(&plan.source_env_var)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unspecified".to_string());

    for step in &plan.steps {
        execute_probe_execution_step(plan, step, &probe_source)?;
    }

    Ok(())
}

fn execute_probe_execution_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    match step.kind.as_str() {
        "tcp" => execute_probe_tcp_step(step, probe_source),
        "http" => execute_probe_http_step(plan, step, probe_source),
        "jsonrpc" => execute_probe_jsonrpc_step(plan, step, probe_source),
        "postgres-pg-isready" => execute_probe_pg_isready_step(plan, step, probe_source),
        "postgres-query" => execute_probe_postgres_query_step(plan, step, probe_source),
        "helios-ready" => execute_probe_helios_ready_step(plan, step, probe_source),
        "exec" => execute_probe_exec_step(plan, step, probe_source),
        other => Err(format!("unsupported probe execution step kind={}", other)),
    }
}

fn execute_probe_tcp_step(step: &ProbeExecutionStep, probe_source: &str) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;

    println!(
        "INFO: checking {} {} port={} source={}",
        step.service_label, step.phase_label, port, probe_source
    );

    let addresses = format!("{}:{}", host, port)
        .to_socket_addrs()
        .map_err(|err| {
            format!(
                "probe tcp address resolution failed host={} port={} err={}",
                host, port, err
            )
        })?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(format!(
            "{} {} port={} (no resolved address)",
            step.service_label, step.failure_label, port
        ));
    }

    let timeout = Duration::from_secs(2);
    if addresses
        .iter()
        .any(|address| TcpStream::connect_timeout(address, timeout).is_ok())
    {
        println!(
            "OK: {} {} port={}",
            step.service_label, step.success_label, port
        );
        Ok(())
    } else {
        Err(format!(
            "{} {} port={}",
            step.service_label, step.failure_label, port
        ))
    }
}

fn execute_probe_http_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let scheme = probe_step_required_field(step, "scheme", step.scheme.as_deref())?;
    let path = probe_step_required_field(step, "path", step.path.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let max_time = probe_step_max_time(step)?;
    let url = build_probe_url(scheme, host, &port, path);

    println!(
        "INFO: checking {} {} url={} source={}",
        step.service_label, step.phase_label, url, probe_source
    );

    let args = vec![
        "-fsS".to_string(),
        "--max-time".to_string(),
        max_time.to_string(),
        url.clone(),
    ];
    let output = run_captured_program(&plan.curl_bin, &args, &[])?;
    if output.status.success() {
        println!(
            "OK: {} {} url={}",
            step.service_label, step.success_label, url
        );
        Ok(())
    } else {
        let exit_code = output.status.code().unwrap_or(1);
        let detail = captured_output_detail(&output);
        Err(format!(
            "{} {} url={} exit={}{}",
            step.service_label,
            step.failure_label,
            url,
            exit_code,
            if detail.is_empty() {
                String::new()
            } else {
                format!(" detail={}", detail)
            }
        ))
    }
}

fn execute_probe_jsonrpc_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let scheme = probe_step_required_field(step, "scheme", step.scheme.as_deref())?;
    let method = probe_step_required_field(step, "method", step.method.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let max_time = probe_step_max_time(step)?;
    let url = build_probe_url(scheme, host, &port, "");

    println!(
        "INFO: checking {} {} port={} source={}",
        step.service_label, step.phase_label, port, probe_source
    );

    let payload = request_jsonrpc_payload(&plan.curl_bin, &url, method, max_time)?;
    if matches!(
        resolve_json_path(&payload, ".result"),
        Some(JsonValue::Null) | None
    ) {
        Err(format!(
            "{} {} port={}",
            step.service_label, step.failure_label, port
        ))
    } else {
        println!(
            "OK: {} {} port={}",
            step.service_label, step.success_label, port
        );
        Ok(())
    }
}

fn execute_probe_pg_isready_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let failure_suffix = step.failure_suffix.as_deref().unwrap_or("");

    println!(
        "INFO: checking {} {} port={} source={}",
        step.service_label, step.phase_label, port, probe_source
    );

    let args = vec![
        "-U".to_string(),
        "postgres".to_string(),
        "-h".to_string(),
        host.to_string(),
        "-p".to_string(),
        port.clone(),
        "-q".to_string(),
    ];
    let output = run_captured_program(&plan.pg_is_ready_bin, &args, &[])?;
    if output.status.success() {
        println!(
            "OK: {} {} port={}",
            step.service_label, step.success_label, port
        );
        Ok(())
    } else {
        Err(format!(
            "{} {} port={}{}",
            step.service_label, step.failure_label, port, failure_suffix
        ))
    }
}

fn execute_probe_postgres_query_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let database = probe_step_required_field(step, "database", step.database.as_deref())?;
    let query = probe_step_required_field(step, "query", step.query.as_deref())?;
    let failure_suffix = step.failure_suffix.as_deref().unwrap_or("");

    println!(
        "INFO: checking {} {} port={} source={}",
        step.service_label, step.phase_label, port, probe_source
    );

    let args = vec![
        "-h".to_string(),
        host.to_string(),
        "-p".to_string(),
        port.clone(),
        "-U".to_string(),
        "postgres".to_string(),
        "-d".to_string(),
        database.to_string(),
        "-Atqc".to_string(),
        query.to_string(),
    ];
    let output = run_captured_program(&plan.psql_bin, &args, &[])?;
    if output.status.success() {
        println!(
            "OK: {} {} port={}",
            step.service_label, step.success_label, port
        );
        Ok(())
    } else {
        Err(format!(
            "{} {} port={}{}",
            step.service_label, step.failure_label, port, failure_suffix
        ))
    }
}

fn execute_probe_helios_ready_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let host = probe_step_required_field(step, "host", step.host.as_deref())?;
    let port_env_var = probe_step_required_field(step, "portEnvVar", step.port_env_var.as_deref())?;
    let port = required_port_from_env(port_env_var)?;
    let _ = step
        .execution_port_env_var
        .as_deref()
        .map(required_port_from_env)
        .transpose()?;
    let max_time = probe_step_max_time(step)?;
    let profile = step.readiness_profile.as_deref().unwrap_or("fast");
    let source_kind = step
        .source_kinds
        .get(probe_source)
        .map(|value| value.as_str())
        .unwrap_or("unknown");
    let url = build_probe_url("http", host, &port, "");

    println!(
        "INFO: checking {} {} port={} source={} source_kind={} profile={}",
        step.service_label, step.phase_label, port, probe_source, source_kind, profile
    );

    if step
        .disallow_source_kinds
        .iter()
        .any(|value| value == source_kind)
    {
        return Err(format!(
            "{} {} port={} source={} source_kind={} profile={} (source kind disallowed)",
            step.service_label, step.failure_label, port, probe_source, source_kind, profile
        ));
    }

    let block_number = request_jsonrpc_payload(&plan.curl_bin, &url, "eth_blockNumber", max_time)
        .ok()
        .and_then(|payload| resolve_json_path(&payload, ".result").cloned())
        .and_then(|value| match value {
            JsonValue::String(text) if is_hex_prefixed(&text) => Some(text),
            _ => None,
        });
    let block_number_valid = block_number.is_some();

    if let Some(block_number) = &block_number {
        println!(
            "OK: {} {} port={} block_number={}",
            step.service_label, step.success_label, port, block_number
        );
    } else if step.allow_local_health_fallback
        && !step.require_not_syncing
        && request_jsonrpc_payload(&plan.curl_bin, &url, "eth_chainId", max_time)
            .ok()
            .and_then(|payload| resolve_json_path(&payload, ".result").cloned())
            .filter(|value| !matches!(value, JsonValue::Null))
            .is_some()
    {
        println!(
            "OK: {} {} port={} mode=local_chainid_fallback",
            step.service_label, step.success_label, port
        );
        return Ok(());
    } else if step.require_not_syncing {
        println!(
            "WARN: {} block number unavailable port={} source={} source_kind={} profile={}; continuing to sync gate",
            step.service_label, port, probe_source, source_kind, profile
        );
    } else {
        return Err(format!(
            "{} {} port={} source={} source_kind={} (invalid eth_blockNumber result)",
            step.service_label, step.failure_label, port, probe_source, source_kind
        ));
    }

    if step.require_not_syncing {
        let syncing_payload =
            request_jsonrpc_payload(&plan.curl_bin, &url, "eth_syncing", max_time).ok();
        let syncing_value = syncing_payload
            .as_ref()
            .and_then(|payload| resolve_json_path(payload, ".result"));
        let syncing_result = syncing_value.map(render_json_compact).unwrap_or_default();
        if !matches!(syncing_value, Some(JsonValue::Bool(false))) {
            return Err(format!(
                "{} {} port={} source={} source_kind={} profile={} (eth_syncing={})",
                step.service_label,
                step.failure_label,
                port,
                probe_source,
                source_kind,
                profile,
                syncing_result
            ));
        }
        if !block_number_valid {
            return Err(format!(
                "{} {} port={} source={} source_kind={} profile={} (invalid eth_blockNumber result)",
                step.service_label,
                step.failure_label,
                port,
                probe_source,
                source_kind,
                profile
            ));
        }
        println!("OK: {} sync status ready port={}", step.service_label, port);
    } else {
        println!("SKIP: helios sync gate disabled profile={}", profile);
    }

    Ok(())
}

fn execute_probe_exec_step(
    plan: &ProbeExecutionPlan,
    step: &ProbeExecutionStep,
    probe_source: &str,
) -> Result<(), String> {
    let command = probe_step_required_field(step, "command", step.command.as_deref())?;

    println!(
        "INFO: checking {} {} source={} kind=exec",
        step.service_label, step.phase_label, probe_source
    );

    let status = run_streaming_program(
        &plan.runtime_shell_bin,
        &["-c".to_string(), command.to_string()],
        &[],
    )?;
    if status.success() {
        println!("OK: {} {}", step.service_label, step.success_label);
        Ok(())
    } else {
        Err(format!("{} {}", step.service_label, step.failure_label))
    }
}

fn machine_output_command(subcommand: &str, values: &[String]) -> Result<(), String> {
    match subcommand {
        "run" => machine_output_run_command(values),
        other => Err(format!("unknown machine-output subcommand: {}", other)),
    }
}

fn machine_output_run_command(values: &[String]) -> Result<(), String> {
    let plan_path = values.first().ok_or_else(|| {
        "usage: nixfied-kernel machine-output run <plan-file> [-- <args...>]".to_string()
    })?;
    let plan = load_machine_output_plan(plan_path)?;
    let remaining = values[1..].to_vec();
    let user_args = strip_passthrough_separator(&remaining).to_vec();
    let work_dir = create_temp_dir("nixfied-machine-output")?;

    for (index, setup_program) in plan.setup_programs.iter().enumerate() {
        let output = run_captured_program(setup_program, &[], &[])?;
        if output.status.success() {
            render_captured_logs("INFO", &format!("setup app {}", index + 1), &output);
        } else {
            render_captured_logs("ERROR", &format!("setup app {}", index + 1), &output);
            machine_output_fail(
                &plan,
                "setup",
                "machine-output-setup-failed",
                &format!("setup app {} failed", index + 1),
                "",
                output.status.code().unwrap_or(1),
            );
        }
    }

    let payload_file = format!("{}/payload.json", work_dir);
    let mut target_args = plan.target_args.clone();
    target_args.extend(user_args.iter().cloned());
    let output = run_captured_program(
        &plan.target_program,
        &target_args,
        &[(
            "NIXFIED_MACHINE_OUTPUT_FILE".to_string(),
            payload_file.clone(),
        )],
    )?;
    if output.status.success() {
        render_captured_logs("INFO", "target app", &output);
    } else {
        render_captured_logs("ERROR", "target app", &output);
        machine_output_fail(
            &plan,
            "target",
            "machine-output-target-failed",
            &format!("target app '{}' failed", plan.target_app_id),
            &plan.target_app_id,
            output.status.code().unwrap_or(1),
        );
    }

    let payload_text = match fs::read_to_string(&payload_file) {
        Ok(text) if !text.trim().is_empty() => text,
        _ => {
            machine_output_fail(
                &plan,
                "validation",
                "machine-output-validation-failed",
                &format!(
                    "target app '{}' did not write machine payload to declared file",
                    plan.target_app_id
                ),
                &plan.target_app_id,
                1,
            );
        }
    };
    let payload = parse_json(&payload_text).unwrap_or_else(|_| {
        machine_output_fail(
            &plan,
            "validation",
            "machine-output-validation-failed",
            &format!(
                "target app '{}' did not satisfy contract '{}'",
                plan.target_app_id, plan.contract_ref
            ),
            &plan.target_app_id,
            1,
        );
    });
    let bundle = parse_json_file(&plan.bundle_file, "machine-output validation bundle")
        .unwrap_or_else(|err| panic!("{}", err));
    if let Err(err) = validate_json_value_against_contract(&bundle, &plan.contract_ref, &payload) {
        eprintln!("ERROR: {}", err);
        machine_output_fail(
            &plan,
            "validation",
            "machine-output-validation-failed",
            &format!(
                "target app '{}' did not satisfy contract '{}'",
                plan.target_app_id, plan.contract_ref
            ),
            &plan.target_app_id,
            1,
        );
    }

    for (index, teardown_program) in plan.teardown_programs.iter().enumerate() {
        let output = run_captured_program(teardown_program, &[], &[])?;
        if output.status.success() {
            render_captured_logs("INFO", &format!("teardown app {}", index + 1), &output);
        } else {
            render_captured_logs("ERROR", &format!("teardown app {}", index + 1), &output);
            machine_output_fail(
                &plan,
                "teardown",
                "machine-output-teardown-failed",
                &format!("teardown app {} failed", index + 1),
                "",
                output.status.code().unwrap_or(1),
            );
        }
    }

    print!("{}", payload_text);
    Ok(())
}

fn request_jsonrpc_payload(
    curl_bin: &str,
    url: &str,
    method: &str,
    max_time: i64,
) -> Result<JsonValue, String> {
    let request_body = render_json_compact(&JsonValue::Object(BTreeMap::from([
        ("jsonrpc".to_string(), JsonValue::String("2.0".to_string())),
        ("id".to_string(), JsonValue::Number(JsonNumber::from_int(1))),
        ("method".to_string(), JsonValue::String(method.to_string())),
        ("params".to_string(), JsonValue::Array(Vec::new())),
    ])));
    let args = vec![
        "-fsS".to_string(),
        "--max-time".to_string(),
        max_time.to_string(),
        "-H".to_string(),
        "content-type: application/json".to_string(),
        "--data".to_string(),
        request_body,
        url.to_string(),
    ];
    let output = run_captured_program(curl_bin, &args, &[])?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };
        let exit_code = output.status.code().unwrap_or(1);
        return Err(format!(
            "probe jsonrpc request failed url={} method={} exit={}{}",
            url,
            method,
            exit_code,
            if detail.is_empty() {
                String::new()
            } else {
                format!(" detail={}", detail)
            }
        ));
    }

    let response = String::from_utf8(output.stdout)
        .map_err(|err| format!("probe jsonrpc response is not valid UTF-8: {}", err))?;
    parse_json(&response)
        .map_err(|err| format!("probe jsonrpc response is not valid JSON: {}", err))
}

fn probe_plan_exports(
    plan: &ProbePlan,
    payload: &JsonValue,
) -> Result<Vec<(String, String)>, String> {
    let result = resolve_json_path(payload, ".result");

    match plan.probe_kind.as_str() {
        "jsonrpc-result-present" => {
            if matches!(result, Some(JsonValue::Null) | None) {
                return Err("probe result is missing".to_string());
            }
            Ok(Vec::new())
        }
        "jsonrpc-result-hex" => {
            let value = result
                .and_then(JsonValue::as_string)
                .ok_or_else(|| "probe result must be a hex string".to_string())?;
            if !is_hex_prefixed(value) {
                return Err(format!("probe result must be hex, got {}", value));
            }
            Ok(vec![(
                plan.export_var.clone().ok_or_else(|| {
                    "probe plan jsonrpc-result-hex requires exportVar".to_string()
                })?,
                value.to_string(),
            )])
        }
        "jsonrpc-result-compact" => {
            let value = result.ok_or_else(|| "probe result is missing".to_string())?;
            if matches!(value, JsonValue::Null) {
                return Err("probe result is missing".to_string());
            }
            Ok(vec![(
                plan.export_var.clone().ok_or_else(|| {
                    "probe plan jsonrpc-result-compact requires exportVar".to_string()
                })?,
                render_json_compact(value),
            )])
        }
        "jsonrpc-result-bool-false" => match result {
            Some(JsonValue::Bool(false)) => Ok(Vec::new()),
            _ => Err("probe result must be false".to_string()),
        },
        other => Err(format!("unknown probe evaluate kind: {}", other)),
    }
}

fn probe_step_required_field<'a>(
    step: &ProbeExecutionStep,
    field: &str,
    value: Option<&'a str>,
) -> Result<&'a str, String> {
    value.ok_or_else(|| {
        format!(
            "probe execution step kind={} missing field {}",
            step.kind, field
        )
    })
}

fn probe_step_max_time(step: &ProbeExecutionStep) -> Result<i64, String> {
    let Some(max_time) = step.max_time_seconds else {
        return Err(format!(
            "probe execution step kind={} missing field maxTimeSeconds",
            step.kind
        ));
    };
    if max_time < 1 {
        return Err(format!(
            "probe execution step kind={} maxTimeSeconds must be positive, got {}",
            step.kind, max_time
        ));
    }
    Ok(max_time)
}

fn required_port_from_env(name: &str) -> Result<String, String> {
    let value = env::var(name).map_err(|_| format!("required env var missing name={}", name))?;
    if value.trim().is_empty() {
        return Err(format!("required env var empty name={}", name));
    }
    let port = parse_i64_text(&value, &format!("env:{}", name))?;
    if !(1..=65535).contains(&port) {
        return Err(format!("env:{} must be port 1-65535 (got '{}')", name, value));
    }
    Ok(port.to_string())
}

fn build_probe_url(scheme: &str, host: &str, port: &str, path: &str) -> String {
    format!("{}://{}:{}{}", scheme, host, port, path)
}

fn captured_output_detail(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        stderr.trim().to_string()
    } else {
        stdout.trim().to_string()
    }
}

fn load_machine_output_plan(path: &str) -> Result<MachineOutputPlan, String> {
    let value = parse_json_file(path, "machine-output plan")?;
    Ok(MachineOutputPlan {
        app_id: required_string_field(&value, "appId", "machine-output plan")?.to_string(),
        target_app_id: required_string_field(&value, "targetAppId", "machine-output plan")?
            .to_string(),
        contract_ref: required_string_field(&value, "contractRef", "machine-output plan")?
            .to_string(),
        bundle_file: required_string_field(&value, "bundleFile", "machine-output plan")?
            .to_string(),
        target_program: required_string_field(&value, "targetProgram", "machine-output plan")?
            .to_string(),
        setup_programs: array_strings(&value, "setupPrograms"),
        teardown_programs: array_strings(&value, "teardownPrograms"),
        target_args: array_strings(&value, "targetArgs"),
    })
}

fn load_probe_plan(path: &str) -> Result<ProbePlan, String> {
    let value = parse_json_file(path, "probe plan")?;
    load_probe_plan_from_value(&value)
}

fn load_probe_plan_from_value(value: &JsonValue) -> Result<ProbePlan, String> {
    let kind = required_string_field(value, "kind", "probe plan")?;
    if kind != "nixfied-probe-plan" {
        return Err(format!("unsupported probe plan kind: {}", kind));
    }
    Ok(ProbePlan {
        probe_kind: required_string_field(value, "probeKind", "probe plan")?.to_string(),
        export_var: object_string(value, "exportVar").map(|value| value.to_string()),
    })
}

fn load_probe_execution_plan_from_value(value: &JsonValue) -> Result<ProbeExecutionPlan, String> {
    let kind = required_string_field(value, "kind", "probe execution plan")?;
    if kind != "nixfied-probe-execution-plan" {
        return Err(format!("unsupported probe execution plan kind: {}", kind));
    }

    let version = object_field(value, "version")
        .and_then(json_value_to_i64)
        .ok_or_else(|| "probe execution plan missing integer field version".to_string())?;
    if version != 1 {
        return Err(format!(
            "probe execution plan version must be 1 (got {})",
            version
        ));
    }

    let mut steps = Vec::new();
    for (index, step_value) in object_array(value, "steps")
        .ok_or_else(|| "probe execution plan missing array field steps".to_string())?
        .iter()
        .enumerate()
    {
        steps.push(parse_probe_execution_step(step_value, index + 1)?);
    }

    Ok(ProbeExecutionPlan {
        mode: required_string_field(value, "mode", "probe execution plan")?.to_string(),
        service_name: required_string_field(value, "serviceName", "probe execution plan")?
            .to_string(),
        source_env_var: required_string_field(value, "sourceEnvVar", "probe execution plan")?
            .to_string(),
        curl_bin: required_string_field(value, "curlBin", "probe execution plan")?.to_string(),
        runtime_shell_bin: required_string_field(
            value,
            "runtimeShellBin",
            "probe execution plan",
        )?
        .to_string(),
        pg_is_ready_bin: required_string_field(value, "pgIsReadyBin", "probe execution plan")?
            .to_string(),
        psql_bin: required_string_field(value, "psqlBin", "probe execution plan")?.to_string(),
        steps,
    })
}

fn parse_probe_execution_step(
    value: &JsonValue,
    index: usize,
) -> Result<ProbeExecutionStep, String> {
    let label = format!("probe execution step {}", index);
    Ok(ProbeExecutionStep {
        kind: required_string_field(value, "kind", &label)?.to_string(),
        service_label: required_string_field(value, "serviceLabel", &label)?.to_string(),
        phase_label: required_string_field(value, "phaseLabel", &label)?.to_string(),
        success_label: required_string_field(value, "successLabel", &label)?.to_string(),
        failure_label: required_string_field(value, "failureLabel", &label)?.to_string(),
        host: object_string(value, "host").map(|text| text.to_string()),
        scheme: object_string(value, "scheme").map(|text| text.to_string()),
        path: object_string(value, "path").map(|text| text.to_string()),
        method: object_string(value, "method").map(|text| text.to_string()),
        port_env_var: object_string(value, "portEnvVar").map(|text| text.to_string()),
        execution_port_env_var: object_string(value, "executionPortEnvVar")
            .map(|text| text.to_string()),
        source_kinds: object_string_map(value, "sourceKinds", &label)?,
        readiness_profile: object_string(value, "readinessProfile").map(|text| text.to_string()),
        require_not_syncing: object_bool(value, "requireNotSyncing").unwrap_or(false),
        allow_local_health_fallback: object_bool(value, "allowLocalHealthFallback")
            .unwrap_or(false),
        disallow_source_kinds: array_strings(value, "disallowSourceKinds"),
        max_time_seconds: object_field(value, "maxTimeSeconds").and_then(json_value_to_i64),
        database: object_string(value, "database").map(|text| text.to_string()),
        query: object_string(value, "query").map(|text| text.to_string()),
        failure_suffix: object_string(value, "failureSuffix").map(|text| text.to_string()),
        command: object_string(value, "command").map(|text| text.to_string()),
    })
}

fn create_temp_dir(prefix: &str) -> Result<String, String> {
    let base = env::temp_dir();
    let candidate = base.join(format!(
        "{}.{}.{}",
        prefix,
        process::id(),
        current_epoch_seconds()?
    ));
    fs::create_dir_all(&candidate)
        .map_err(|err| format!("failed to create {}: {}", candidate.display(), err))?;
    Ok(candidate.display().to_string())
}

fn run_captured_program(
    program: &str,
    args: &[String],
    envs: &[(String, String)],
) -> Result<std::process::Output, String> {
    let mut command = Command::new(program);
    command.args(args);
    command.stdin(Stdio::null());
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .output()
        .map_err(|err| format!("failed to run {}: {}", program, err))
}

fn run_streaming_program(
    program: &str,
    args: &[String],
    envs: &[(String, String)],
) -> Result<std::process::ExitStatus, String> {
    let mut command = Command::new(program);
    command.args(args);
    command.stdin(Stdio::null());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .status()
        .map_err(|err| format!("failed to run {}: {}", program, err))
}

fn render_captured_logs(level: &str, label: &str, output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.is_empty() {
        eprintln!("{}: {} stdout:", level, label);
        eprint!("{}", stdout);
    }
    if !stderr.is_empty() {
        eprintln!("{}: {} stderr:", level, label);
        eprint!("{}", stderr);
    }
}

fn machine_output_fail(
    plan: &MachineOutputPlan,
    stage: &str,
    code: &str,
    message: &str,
    failed_app_id: &str,
    exit_code: i32,
) -> ! {
    let payload = JsonValue::Object(BTreeMap::from([
        ("ok".to_string(), JsonValue::Bool(false)),
        ("appId".to_string(), JsonValue::String(plan.app_id.clone())),
        (
            "targetAppId".to_string(),
            JsonValue::String(plan.target_app_id.clone()),
        ),
        ("stage".to_string(), JsonValue::String(stage.to_string())),
        ("code".to_string(), JsonValue::String(code.to_string())),
        (
            "message".to_string(),
            JsonValue::String(message.to_string()),
        ),
        (
            "failedAppId".to_string(),
            nullable_string_value(failed_app_id),
        ),
        (
            "contractRef".to_string(),
            nullable_string_value(&plan.contract_ref),
        ),
        (
            "validator".to_string(),
            if stage == "validation" {
                JsonValue::String("nixfied-kernel".to_string())
            } else {
                JsonValue::Null
            },
        ),
        (
            "exitCode".to_string(),
            JsonValue::Number(JsonNumber::from_int(exit_code as i64)),
        ),
    ]));
    println!("{}", render_json_compact(&payload));
    process::exit(exit_code.max(1));
}

fn is_hex_prefixed(value: &str) -> bool {
    value.len() >= 3
        && value.starts_with("0x")
        && value.chars().skip(2).all(|ch| ch.is_ascii_hexdigit())
}

fn read_text(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|err| format!("failed to read stdin: {}", err))?;
        return Ok(buffer);
    }

    fs::read_to_string(path).map_err(|err| format!("failed to read {}: {}", path, err))
}

#[derive(Clone, Debug)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(JsonNumber),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

#[derive(Clone, Debug)]
struct JsonNumber {
    raw: String,
    integer: bool,
    int_value: Option<i128>,
    float_value: f64,
}

impl JsonValue {
    fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(value) => Some(value),
            _ => None,
        }
    }

    fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(value) => Some(value),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(value) => Some(value.as_slice()),
            _ => None,
        }
    }

    fn as_array_mut(&mut self) -> Option<&mut Vec<JsonValue>> {
        match self {
            JsonValue::Array(value) => Some(value),
            _ => None,
        }
    }

    fn as_string(&self) -> Option<&str> {
        match self {
            JsonValue::String(value) => Some(value.as_str()),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(value) => Some(*value),
            _ => None,
        }
    }

    fn as_number(&self) -> Option<&JsonNumber> {
        match self {
            JsonValue::Number(value) => Some(value),
            _ => None,
        }
    }
}

impl JsonNumber {
    fn from_int(value: i64) -> Self {
        Self {
            raw: value.to_string(),
            integer: true,
            int_value: Some(value as i128),
            float_value: value as f64,
        }
    }
}

fn parse_json(input: &str) -> Result<JsonValue, String> {
    let mut parser = Parser::new(input);
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.peek().is_some() {
        return Err(parser.error("trailing content after JSON value"));
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Parser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn error(&self, message: impl Into<String>) -> String {
        format!("line {} column {}: {}", self.line, self.col, message.into())
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\n' | '\t' | '\r')) {
            let _ = self.next();
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_whitespace();
        match self.peek() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => Ok(JsonValue::String(self.parse_string()?)),
            Some('t') => {
                self.expect_keyword("true")?;
                Ok(JsonValue::Bool(true))
            }
            Some('f') => {
                self.expect_keyword("false")?;
                Ok(JsonValue::Bool(false))
            }
            Some('n') => {
                self.expect_keyword("null")?;
                Ok(JsonValue::Null)
            }
            Some('-') | Some('0'..='9') => Ok(JsonValue::Number(self.parse_number()?)),
            Some(ch) => Err(self.error(format!("unexpected character '{}'", ch))),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), String> {
        for expected in keyword.chars() {
            match self.next() {
                Some(actual) if actual == expected => {}
                Some(actual) => {
                    return Err(self.error(format!(
                        "expected keyword {}, got character '{}'",
                        keyword, actual
                    )))
                }
                None => return Err(self.error(format!("expected keyword {}", keyword))),
            }
        }
        Ok(())
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect_char('"')?;
        let mut out = String::new();
        loop {
            let ch = self
                .next()
                .ok_or_else(|| self.error("unterminated string literal"))?;
            match ch {
                '"' => break,
                '\\' => out.push(self.parse_escape_sequence()?),
                '\u{0000}'..='\u{001F}' => {
                    return Err(self.error("unescaped control character in string"))
                }
                other => out.push(other),
            }
        }
        Ok(out)
    }

    fn parse_escape_sequence(&mut self) -> Result<char, String> {
        match self.next() {
            Some('"') => Ok('"'),
            Some('\\') => Ok('\\'),
            Some('/') => Ok('/'),
            Some('b') => Ok('\u{0008}'),
            Some('f') => Ok('\u{000c}'),
            Some('n') => Ok('\n'),
            Some('r') => Ok('\r'),
            Some('t') => Ok('\t'),
            Some('u') => self.parse_unicode_escape(),
            Some(other) => Err(self.error(format!("invalid escape sequence '\\{}'", other))),
            None => Err(self.error("unterminated escape sequence")),
        }
    }

    fn parse_unicode_escape(&mut self) -> Result<char, String> {
        let first = self.parse_hex_quad()?;
        if (0xD800..=0xDBFF).contains(&first) {
            self.expect_char('\\')?;
            self.expect_char('u')?;
            let second = self.parse_hex_quad()?;
            if !(0xDC00..=0xDFFF).contains(&second) {
                return Err(self.error("invalid UTF-16 surrogate pair"));
            }
            let codepoint =
                0x10000 + ((((first - 0xD800) as u32) << 10) | ((second - 0xDC00) as u32));
            return char::from_u32(codepoint)
                .ok_or_else(|| self.error("invalid Unicode escape sequence"));
        }
        if (0xDC00..=0xDFFF).contains(&first) {
            return Err(self.error("unexpected low surrogate in Unicode escape"));
        }
        char::from_u32(first as u32).ok_or_else(|| self.error("invalid Unicode escape sequence"))
    }

    fn parse_hex_quad(&mut self) -> Result<u16, String> {
        let mut value = 0u16;
        for _ in 0..4 {
            let ch = self
                .next()
                .ok_or_else(|| self.error("unexpected end of Unicode escape"))?;
            let digit = ch
                .to_digit(16)
                .ok_or_else(|| self.error("invalid hex digit in Unicode escape"))?;
            value = (value << 4) | digit as u16;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<JsonNumber, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            let _ = self.next();
        }

        match self.peek() {
            Some('0') => {
                let _ = self.next();
                if matches!(self.peek(), Some('0'..='9')) {
                    return Err(self.error("leading zeros are not allowed in JSON numbers"));
                }
            }
            Some('1'..='9') => {
                let _ = self.next();
                while matches!(self.peek(), Some('0'..='9')) {
                    let _ = self.next();
                }
            }
            _ => return Err(self.error("invalid JSON number")),
        }

        if self.peek() == Some('.') {
            let _ = self.next();
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(self.error("fractional part requires digits"));
            }
            while matches!(self.peek(), Some('0'..='9')) {
                let _ = self.next();
            }
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            let _ = self.next();
            if matches!(self.peek(), Some('+' | '-')) {
                let _ = self.next();
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(self.error("exponent requires digits"));
            }
            while matches!(self.peek(), Some('0'..='9')) {
                let _ = self.next();
            }
        }

        let raw: String = self.chars[start..self.pos].iter().collect();
        let integer = !raw.contains('.') && !raw.contains('e') && !raw.contains('E');
        let float_value = raw
            .parse::<f64>()
            .map_err(|err| self.error(format!("invalid JSON number: {}", err)))?;
        let int_value = if integer {
            raw.parse::<i128>().ok()
        } else {
            None
        };

        Ok(JsonNumber {
            raw,
            integer,
            int_value,
            float_value,
        })
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.expect_char('[')?;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.peek() == Some(']') {
            let _ = self.next();
            return Ok(JsonValue::Array(values));
        }

        loop {
            values.push(self.parse_value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    let _ = self.next();
                    self.skip_whitespace();
                }
                Some(']') => {
                    let _ = self.next();
                    break;
                }
                Some(other) => {
                    return Err(self.error(format!(
                        "expected ',' or ']' after array item, found '{}'",
                        other
                    )))
                }
                None => return Err(self.error("unterminated array")),
            }
        }

        Ok(JsonValue::Array(values))
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect_char('{')?;
        self.skip_whitespace();
        let mut values = BTreeMap::new();
        if self.peek() == Some('}') {
            let _ = self.next();
            return Ok(JsonValue::Object(values));
        }

        loop {
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect_char(':')?;
            let value = self.parse_value()?;
            values.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    let _ = self.next();
                    self.skip_whitespace();
                }
                Some('}') => {
                    let _ = self.next();
                    break;
                }
                Some(other) => {
                    return Err(self.error(format!(
                        "expected ',' or '}}' after object item, found '{}'",
                        other
                    )))
                }
                None => return Err(self.error("unterminated object")),
            }
        }

        Ok(JsonValue::Object(values))
    }

    fn expect_char(&mut self, expected: char) -> Result<(), String> {
        match self.next() {
            Some(actual) if actual == expected => Ok(()),
            Some(actual) => Err(self.error(format!("expected '{}', found '{}'", expected, actual))),
            None => Err(self.error(format!("expected '{}'", expected))),
        }
    }
}

#[derive(Clone, Debug)]
enum PathSegment {
    Field(String),
    Index(usize),
}

fn format_path(path: &[PathSegment]) -> String {
    let mut out = String::from("$");
    for segment in path {
        match segment {
            PathSegment::Field(field) => {
                if is_identifier(field) {
                    out.push('.');
                    out.push_str(field);
                } else {
                    out.push_str("['");
                    out.push_str(&escape_path_key(field));
                    out.push_str("']");
                }
            }
            PathSegment::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
        }
    }
    out
}

fn escape_path_key(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            other => out.push(other),
        }
    }
    out
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

struct ValidationContext<'a> {
    root: &'a JsonValue,
    definition_maps: Vec<&'a BTreeMap<String, JsonValue>>,
}

impl<'a> ValidationContext<'a> {
    fn new(root: &'a JsonValue) -> Self {
        let mut definition_maps = Vec::new();
        if let Some(object) = root.as_object() {
            if let Some(definitions) = object.get("definitions").and_then(JsonValue::as_object) {
                definition_maps.push(definitions);
            }
            if let Some(bundle) = object.get("bundle").and_then(JsonValue::as_object) {
                if let Some(definitions) = bundle.get("definitions").and_then(JsonValue::as_object)
                {
                    definition_maps.push(definitions);
                }
            }
            if let Some(validation) = object.get("validation").and_then(JsonValue::as_object) {
                if let Some(definitions) =
                    validation.get("definitions").and_then(JsonValue::as_object)
                {
                    definition_maps.push(definitions);
                }
            }
        }

        Self {
            root,
            definition_maps,
        }
    }

    fn resolve_contract_ref(&self, contract_ref: &str) -> Result<&'a JsonValue, String> {
        if contract_ref.starts_with('#') || contract_ref.starts_with('/') {
            if let Some(value) = resolve_json_pointer(self.root, contract_ref) {
                return Ok(value);
            }
        }

        let mut candidates = Vec::new();
        if !candidates.iter().any(|entry| entry == contract_ref) {
            candidates.push(contract_ref.to_string());
        }
        add_candidate(&mut candidates, contract_ref);
        if let Some(stripped) = contract_ref.strip_prefix('#') {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix("#/") {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix('/') {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix("#/definitions/") {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix("/definitions/") {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.strip_prefix("definitions/") {
            add_candidate(&mut candidates, stripped);
        }
        if let Some(stripped) = contract_ref.rsplit('/').next() {
            add_candidate(&mut candidates, stripped);
        }

        for candidate in candidates {
            for definitions in &self.definition_maps {
                if let Some(value) = definitions.get(&candidate) {
                    return Ok(value);
                }
            }
        }

        let known = self
            .definition_maps
            .iter()
            .flat_map(|definitions| definitions.keys().cloned())
            .collect::<Vec<_>>()
            .join(", ");

        Err(if known.is_empty() {
            format!("reference {} was not found", contract_ref)
        } else {
            format!(
                "reference {} was not found; known definitions: {}",
                contract_ref, known
            )
        })
    }
}

fn add_candidate(candidates: &mut Vec<String>, candidate: &str) {
    let value = candidate.trim_start_matches('#').trim_start_matches('/');
    if !value.is_empty() && !candidates.iter().any(|entry| entry == value) {
        candidates.push(value.to_string());
    }
}

fn resolve_json_pointer<'a>(value: &'a JsonValue, pointer: &str) -> Option<&'a JsonValue> {
    let mut current = value;
    let pointer = pointer.strip_prefix('#').unwrap_or(pointer);
    if pointer.is_empty() {
        return Some(current);
    }
    let pointer = pointer.strip_prefix('/').unwrap_or(pointer);
    if pointer.is_empty() {
        return Some(current);
    }

    for token in pointer.split('/') {
        let token = token.replace("~1", "/").replace("~0", "~");
        current = match current {
            JsonValue::Object(map) => map.get(&token)?,
            JsonValue::Array(array) => {
                let index = token.parse::<usize>().ok()?;
                array.get(index)?
            }
            _ => return None,
        };
    }

    Some(current)
}

fn validate_schema(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let current = unwrap_schema_reference(context, schema, ref_stack)?;
    let kind = schema_kind(current);

    match kind.as_deref() {
        Some("any") | Some("anything") | Some("unknown") => Ok(()),
        Some("bool") | Some("boolean") => validate_bool(value, path),
        Some("null") => validate_null(value, path),
        Some("string") => validate_string(current, value, path),
        Some("integer") | Some("int") | Some("int64") => validate_integer(current, value, path),
        Some("number") | Some("float") | Some("double") => validate_number(current, value, path),
        Some("literal") | Some("const") => validate_literal(current, value, path),
        Some("enum") => validate_enum(current, value, path),
        Some("list") | Some("array") => validate_list(context, current, value, path, ref_stack),
        Some("map") => validate_map(context, current, value, path, ref_stack),
        Some("record") => validate_record(context, current, value, path, ref_stack),
        Some("union") | Some("oneof") | Some("anyof") => {
            validate_union(context, current, value, path, ref_stack)
        }
        Some("taggedunion") => validate_tagged_union(context, current, value, path, ref_stack),
        Some("object") => validate_object_alias(context, current, value, path, ref_stack),
        Some(other) => Err(error_at(
            path,
            format!("unsupported schema kind '{}'", other),
        )),
        None => {
            if object_field(current, "schema").is_some() {
                validate_schema(
                    context,
                    object_field(current, "schema").unwrap(),
                    value,
                    path,
                    ref_stack,
                )
            } else {
                Err(error_at(path, "schema is missing a kind".to_string()))
            }
        }
    }
}

fn unwrap_schema_reference<'a>(
    context: &ValidationContext<'a>,
    schema: &'a JsonValue,
    ref_stack: &mut Vec<String>,
) -> Result<&'a JsonValue, String> {
    let mut current = schema;
    let mut pushed = 0usize;
    loop {
        let kind = schema_kind(current);
        let reference = if matches!(kind.as_deref(), Some("ref")) || kind.is_none() {
            object_string(current, "ref")
                .or_else(|| object_string(current, "name"))
                .map(|value| value.to_string())
        } else {
            None
        };

        let Some(reference) = reference else {
            for _ in 0..pushed {
                let _ = ref_stack.pop();
            }
            return Ok(current);
        };

        if ref_stack.iter().any(|seen| seen == &reference) {
            for _ in 0..pushed {
                let _ = ref_stack.pop();
            }
            return Err(format!("cyclic reference detected at {}", reference));
        }

        ref_stack.push(reference.clone());
        pushed += 1;
        match context.resolve_contract_ref(&reference) {
            Ok(next) => current = next,
            Err(err) => {
                for _ in 0..pushed {
                    let _ = ref_stack.pop();
                }
                return Err(err);
            }
        }
    }
}

fn error_at(path: &[PathSegment], message: impl Into<String>) -> String {
    format!("path={} {}", format_path(path), message.into())
}

fn validate_bool(value: &JsonValue, path: &[PathSegment]) -> Result<(), String> {
    match value {
        JsonValue::Bool(_) => Ok(()),
        other => Err(error_at(
            path,
            format!("expected bool, found {}", value_type(other)),
        )),
    }
}

fn validate_null(value: &JsonValue, path: &[PathSegment]) -> Result<(), String> {
    match value {
        JsonValue::Null => Ok(()),
        other => Err(error_at(
            path,
            format!("expected null, found {}", value_type(other)),
        )),
    }
}

fn validate_string(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let string = match value {
        JsonValue::String(value) => value,
        other => {
            return Err(error_at(
                path,
                format!("expected string, found {}", value_type(other)),
            ))
        }
    };

    if let Some(min_length) = schema_number(schema, "minLength") {
        if (string.chars().count() as f64) < min_length {
            return Err(error_at(
                path,
                format!("string is shorter than minLength {}", min_length),
            ));
        }
    }

    if let Some(max_length) = schema_number(schema, "maxLength") {
        if (string.chars().count() as f64) > max_length {
            return Err(error_at(
                path,
                format!("string is longer than maxLength {}", max_length),
            ));
        }
    }

    if let Some(patterns) = schema_patterns(schema) {
        for pattern in patterns {
            let compiled = SimplePattern::compile(&pattern).map_err(|err| {
                error_at(path, format!("invalid string pattern {}: {}", pattern, err))
            })?;
            if !compiled.matches(string) {
                return Err(error_at(
                    path,
                    format!("string does not match pattern {}", pattern),
                ));
            }
        }
    }

    Ok(())
}

fn validate_integer(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let number = match value {
        JsonValue::Number(value) if value.integer => value,
        JsonValue::Number(other) => {
            return Err(error_at(
                path,
                format!("expected integer, found number {}", other.raw),
            ))
        }
        other => {
            return Err(error_at(
                path,
                format!("expected integer, found {}", value_type(other)),
            ))
        }
    };

    validate_numeric_bounds(schema, number.float_value, path)?;
    Ok(())
}

fn validate_number(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let number = match value {
        JsonValue::Number(value) => value,
        other => {
            return Err(error_at(
                path,
                format!("expected number, found {}", value_type(other)),
            ))
        }
    };

    validate_numeric_bounds(schema, number.float_value, path)?;
    Ok(())
}

fn validate_literal(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let expected = object_field(schema, "value")
        .or_else(|| object_field(schema, "literal"))
        .or_else(|| object_field(schema, "const"));

    let Some(expected) = expected else {
        return Err(error_at(
            path,
            "literal schema is missing a value".to_string(),
        ));
    };

    if json_equal(expected, value) {
        Ok(())
    } else {
        Err(error_at(
            path,
            format!(
                "expected literal {}, found {}",
                json_preview(expected),
                json_preview(value)
            ),
        ))
    }
}

fn validate_enum(
    schema: &JsonValue,
    value: &JsonValue,
    path: &[PathSegment],
) -> Result<(), String> {
    let Some(values) = schema_enum_values(schema) else {
        return Err(error_at(path, "enum schema is missing values".to_string()));
    };

    if values.iter().any(|candidate| json_equal(candidate, value)) {
        Ok(())
    } else {
        Err(error_at(
            path,
            format!("value {} is not in enum", json_preview(value)),
        ))
    }
}

fn validate_list(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let items = match value {
        JsonValue::Array(values) => values,
        other => {
            return Err(error_at(
                path,
                format!("expected array, found {}", value_type(other)),
            ))
        }
    };

    if let Some(tuple_items) = object_array(schema, "items") {
        if items.len() < tuple_items.len() {
            return Err(error_at(
                path,
                format!(
                    "array has {} items but schema requires {}",
                    items.len(),
                    tuple_items.len()
                ),
            ));
        }

        for (index, tuple_schema) in tuple_items.iter().enumerate() {
            path.push(PathSegment::Index(index));
            let result = validate_schema(context, tuple_schema, &items[index], path, ref_stack);
            path.pop();
            result?;
        }

        if items.len() > tuple_items.len() {
            if let Some(rest_schema) =
                object_field(schema, "rest").or_else(|| object_field(schema, "additionalItems"))
            {
                for (index, item) in items.iter().enumerate().skip(tuple_items.len()) {
                    path.push(PathSegment::Index(index));
                    let result = validate_schema(context, rest_schema, item, path, ref_stack);
                    path.pop();
                    result?;
                }
            } else {
                return Err(error_at(
                    path,
                    format!(
                        "array has {} items but schema only allows {}",
                        items.len(),
                        tuple_items.len()
                    ),
                ));
            }
        }
    } else if let Some(item_schema) = object_field(schema, "elem")
        .or_else(|| object_field(schema, "item"))
        .or_else(|| object_field(schema, "element"))
        .or_else(|| object_field(schema, "schema"))
    {
        for (index, item) in items.iter().enumerate() {
            path.push(PathSegment::Index(index));
            let result = validate_schema(context, item_schema, item, path, ref_stack);
            path.pop();
            result?;
        }
    }

    if let Some(min_items) = schema_number(schema, "minItems") {
        if (items.len() as f64) < min_items {
            return Err(error_at(
                path,
                format!("array has fewer than minItems {}", min_items),
            ));
        }
    }

    if let Some(max_items) = schema_number(schema, "maxItems") {
        if (items.len() as f64) > max_items {
            return Err(error_at(
                path,
                format!("array has more than maxItems {}", max_items),
            ));
        }
    }

    if object_bool(schema, "uniqueItems").unwrap_or(false) {
        for left in 0..items.len() {
            for right in (left + 1)..items.len() {
                if json_equal(&items[left], &items[right]) {
                    return Err(error_at(path, "array items are not unique".to_string()));
                }
            }
        }
    }

    Ok(())
}

fn validate_map(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let object = match value {
        JsonValue::Object(value) => value,
        other => {
            return Err(error_at(
                path,
                format!("expected object, found {}", value_type(other)),
            ))
        }
    };

    let value_schema = object_field(schema, "value")
        .or_else(|| object_field(schema, "valueSchema"))
        .or_else(|| object_field(schema, "schema"));

    let key_pattern = object_string(schema, "keyPattern")
        .or_else(|| object_string(schema, "pattern"))
        .or_else(|| object_string(schema, "keyRegex"));

    let compiled_key_pattern =
        if let Some(pattern) = key_pattern {
            Some(SimplePattern::compile(pattern).map_err(|err| {
                error_at(path, format!("invalid key pattern {}: {}", pattern, err))
            })?)
        } else {
            None
        };

    for (key, item) in object {
        if let Some(pattern) = &compiled_key_pattern {
            if !pattern.matches(key) {
                return Err(error_at(
                    path,
                    format!("key {} does not match key pattern", key),
                ));
            }
        }

        if let Some(item_schema) = value_schema {
            path.push(PathSegment::Field(key.clone()));
            let result = validate_schema(context, item_schema, item, path, ref_stack);
            path.pop();
            result?;
        }
    }

    Ok(())
}

fn validate_record(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let object = match value {
        JsonValue::Object(value) => value,
        other => {
            return Err(error_at(
                path,
                format!("expected object, found {}", value_type(other)),
            ))
        }
    };

    let (fields, closed) = record_fields(schema)?;

    for (field_name, field_spec) in &fields {
        match object.get(field_name) {
            Some(item) => {
                path.push(PathSegment::Field(field_name.clone()));
                let result = validate_schema(context, field_spec.schema, item, path, ref_stack);
                path.pop();
                result?;
            }
            None if field_spec.required => {
                return Err(error_at(
                    path,
                    format!("missing required field {}", field_name),
                ));
            }
            None => {}
        }
    }

    if closed {
        for key in object.keys() {
            if !fields.contains_key(key) {
                return Err(error_at(path, format!("unknown field {}", key)));
            }
        }
    }

    Ok(())
}

fn validate_object_alias(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    if has_record_shape(schema) {
        validate_record(context, schema, value, path, ref_stack)
    } else {
        validate_map(context, schema, value, path, ref_stack)
    }
}

fn validate_union(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let variants = schema_variants(schema)
        .ok_or_else(|| error_at(path, "union schema is missing variants".to_string()))?;

    let mut last_error = None;
    for variant in variants {
        let mut candidate_path = path.clone();
        match validate_schema(context, variant, value, &mut candidate_path, ref_stack) {
            Ok(()) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
    }

    Err(error_at(
        path,
        format!(
            "value did not match any union variant{}",
            last_error
                .as_ref()
                .map(|err| format!("; last error: {}", err))
                .unwrap_or_default()
        ),
    ))
}

fn validate_tagged_union(
    context: &ValidationContext<'_>,
    schema: &JsonValue,
    value: &JsonValue,
    path: &mut Vec<PathSegment>,
    ref_stack: &mut Vec<String>,
) -> Result<(), String> {
    let object = match value {
        JsonValue::Object(value) => value,
        other => {
            return Err(error_at(
                path,
                format!("expected object, found {}", value_type(other)),
            ))
        }
    };

    let tag_field = object_string(schema, "tag")
        .or_else(|| object_string(schema, "tagField"))
        .or_else(|| object_string(schema, "discriminator"))
        .unwrap_or("tag");
    let tag_value = object
        .get(tag_field)
        .ok_or_else(|| error_at(path, format!("missing tagged union field {}", tag_field)))?;

    let tag_key = scalar_key(tag_value).ok_or_else(|| {
        error_at(
            path,
            format!("tag field {} must be a scalar value", tag_field),
        )
    })?;

    let variants = tagged_union_variants(schema)
        .ok_or_else(|| error_at(path, "tagged union schema is missing variants".to_string()))?;

    let Some(variant_schema) = variants.get(&tag_key) else {
        return Err(error_at(
            path,
            format!("unknown tagged union variant {}", tag_key),
        ));
    };

    let variant_schema = *variant_schema;
    let variant_payload = if should_strip_tag_field(variant_schema, tag_field) {
        let mut filtered = BTreeMap::new();
        for (key, item) in object {
            if key != tag_field {
                filtered.insert(key.clone(), item.clone());
            }
        }
        JsonValue::Object(filtered)
    } else {
        value.clone()
    };

    validate_schema(context, variant_schema, &variant_payload, path, ref_stack)
}

fn should_strip_tag_field(schema: &JsonValue, tag_field: &str) -> bool {
    if !matches!(schema_kind(schema).as_deref(), Some("record")) {
        return false;
    }

    let Ok((fields, _)) = record_fields(schema) else {
        return false;
    };

    !fields.contains_key(tag_field)
}

fn validate_numeric_bounds(
    schema: &JsonValue,
    value: f64,
    path: &[PathSegment],
) -> Result<(), String> {
    if let Some(minimum) = schema_number(schema, "minimum") {
        if value < minimum {
            return Err(error_at(
                path,
                format!("value {} is below minimum {}", value, minimum),
            ));
        }
    }

    if let Some(maximum) = schema_number(schema, "maximum") {
        if value > maximum {
            return Err(error_at(
                path,
                format!("value {} is above maximum {}", value, maximum),
            ));
        }
    }

    if let Some(exclusive_minimum) = schema_number(schema, "exclusiveMinimum") {
        if value <= exclusive_minimum {
            return Err(error_at(
                path,
                format!(
                    "value {} is not greater than exclusiveMinimum {}",
                    value, exclusive_minimum
                ),
            ));
        }
    }

    if let Some(exclusive_maximum) = schema_number(schema, "exclusiveMaximum") {
        if value >= exclusive_maximum {
            return Err(error_at(
                path,
                format!(
                    "value {} is not less than exclusiveMaximum {}",
                    value, exclusive_maximum
                ),
            ));
        }
    }

    if let Some(multiple_of) = schema_number(schema, "multipleOf") {
        if multiple_of != 0.0 {
            let ratio = value / multiple_of;
            if (ratio - ratio.round()).abs() > 1e-9 {
                return Err(error_at(
                    path,
                    format!("value {} is not a multiple of {}", value, multiple_of),
                ));
            }
        }
    }

    Ok(())
}

fn record_fields<'a>(
    schema: &'a JsonValue,
) -> Result<(BTreeMap<String, RecordField<'a>>, bool), String> {
    let closed = object_bool(schema, "closed")
        .or_else(|| object_bool(schema, "open").map(|value| !value))
        .or_else(|| object_bool(schema, "additionalProperties").map(|value| !value))
        .unwrap_or(true);

    let mut fields = BTreeMap::new();
    let required_names = object_array(schema, "required")
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_string)
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if let Some(field_values) = object_field(schema, "fields").and_then(JsonValue::as_object) {
        for (name, field_value) in field_values {
            let schema = field_schema(field_value)
                .ok_or_else(|| format!("record field {} does not contain a schema", name))?;
            let required = object_bool(field_value, "required")
                .or_else(|| {
                    if required_names.iter().any(|entry| entry == name) {
                        Some(true)
                    } else {
                        None
                    }
                })
                .unwrap_or(true);
            fields.insert(name.clone(), RecordField { schema, required });
        }
    } else if let Some(field_values) = object_array(schema, "fields") {
        for field_value in field_values {
            let (name, schema, required) = parse_record_field(field_value, &required_names)?;
            fields.insert(name, RecordField { schema, required });
        }
    } else if let Some(field_values) =
        object_field(schema, "properties").and_then(JsonValue::as_object)
    {
        for (name, schema_value) in field_values {
            let schema = field_schema(schema_value)
                .ok_or_else(|| format!("record field {} does not contain a schema", name))?;
            let required = object_bool(schema_value, "required")
                .or_else(|| {
                    if required_names.iter().any(|entry| entry == name) {
                        Some(true)
                    } else {
                        None
                    }
                })
                .unwrap_or(true);
            fields.insert(name.clone(), RecordField { schema, required });
        }
    }

    Ok((fields, closed))
}

struct RecordField<'a> {
    schema: &'a JsonValue,
    required: bool,
}

fn parse_record_field<'a>(
    field_value: &'a JsonValue,
    required_names: &[String],
) -> Result<(String, &'a JsonValue, bool), String> {
    let object = field_value
        .as_object()
        .ok_or_else(|| "record field definition must be an object".to_string())?;

    let name = object_string(field_value, "name")
        .or_else(|| object_string(field_value, "key"))
        .or_else(|| object_string(field_value, "field"))
        .ok_or_else(|| "record field definition is missing a name".to_string())?;

    let schema = field_schema(field_value)
        .ok_or_else(|| format!("record field {} does not contain a schema", name))?;

    let required = object_bool(field_value, "required")
        .or_else(|| {
            if required_names.iter().any(|entry| entry == name) {
                Some(true)
            } else {
                None
            }
        })
        .unwrap_or(true);

    if object.get("schema").is_none() && !looks_like_schema(field_value) {
        return Err(format!("record field {} does not contain a schema", name));
    }

    Ok((name.to_string(), schema, required))
}

fn field_schema<'a>(value: &'a JsonValue) -> Option<&'a JsonValue> {
    if let Some(schema) = object_field(value, "schema") {
        Some(schema)
    } else if looks_like_schema(value) {
        Some(value)
    } else {
        None
    }
}

fn has_record_shape(schema: &JsonValue) -> bool {
    object_field(schema, "fields").is_some()
        || object_field(schema, "properties").is_some()
        || object_field(schema, "closed").is_some()
        || object_field(schema, "open").is_some()
        || object_field(schema, "additionalProperties").is_some()
}

fn looks_like_schema(value: &JsonValue) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("kind")
        || object.contains_key("type")
        || object.contains_key("ref")
        || object.contains_key("schema")
        || object.contains_key("fields")
        || object.contains_key("properties")
        || object.contains_key("items")
        || object.contains_key("elem")
        || object.contains_key("item")
        || object.contains_key("element")
        || object.contains_key("variants")
        || object.contains_key("values")
        || object.contains_key("options")
        || object.contains_key("literal")
        || object.contains_key("const")
        || object.contains_key("pattern")
        || object.contains_key("patterns")
        || object.contains_key("format")
        || object.contains_key("minimum")
        || object.contains_key("maximum")
        || object.contains_key("exclusiveMinimum")
        || object.contains_key("exclusiveMaximum")
        || object.contains_key("multipleOf")
}

fn schema_kind(schema: &JsonValue) -> Option<String> {
    let object = schema.as_object()?;
    object
        .get("kind")
        .or_else(|| object.get("type"))
        .and_then(JsonValue::as_string)
        .map(|value| value.to_ascii_lowercase())
        .or_else(|| infer_kind(schema))
}

fn infer_kind(schema: &JsonValue) -> Option<String> {
    let object = schema.as_object()?;
    if object.contains_key("fields")
        || object.contains_key("properties")
        || object.contains_key("closed")
        || object.contains_key("open")
        || object.contains_key("additionalProperties")
    {
        return Some("record".to_string());
    }
    if object.contains_key("items")
        || object.contains_key("item")
        || object.contains_key("element")
        || object.contains_key("elem")
    {
        return Some("list".to_string());
    }
    if object.contains_key("variants") {
        if object.contains_key("tag")
            || object.contains_key("tagField")
            || object.contains_key("discriminator")
        {
            return Some("taggedunion".to_string());
        }
        return Some("union".to_string());
    }
    if object.contains_key("options") {
        return Some("union".to_string());
    }
    if object.contains_key("values") {
        return Some("enum".to_string());
    }
    if object.contains_key("literal") || object.contains_key("const") {
        return Some("literal".to_string());
    }
    if object.contains_key("valueSchema")
        || object.contains_key("keyPattern")
        || object.contains_key("keyRegex")
    {
        return Some("map".to_string());
    }
    if object.contains_key("pattern")
        || object.contains_key("patterns")
        || object.contains_key("format")
        || object.contains_key("minLength")
        || object.contains_key("maxLength")
    {
        return Some("string".to_string());
    }
    if object.contains_key("minimum")
        || object.contains_key("maximum")
        || object.contains_key("exclusiveMinimum")
        || object.contains_key("exclusiveMaximum")
        || object.contains_key("multipleOf")
    {
        return Some("number".to_string());
    }
    if object.contains_key("ref") {
        return Some("ref".to_string());
    }
    None
}

fn object_field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    value.as_object()?.get(key)
}

fn object_field_mut<'a>(value: &'a mut JsonValue, key: &str) -> Option<&'a mut JsonValue> {
    value.as_object_mut()?.get_mut(key)
}

fn object_string<'a>(value: &'a JsonValue, key: &str) -> Option<&'a str> {
    object_field(value, key)?.as_string()
}

fn object_bool(value: &JsonValue, key: &str) -> Option<bool> {
    object_field(value, key)?.as_bool()
}

fn object_number<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonNumber> {
    object_field(value, key)?.as_number()
}

fn object_array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    object_field(value, key)?.as_array()
}

fn schema_number(schema: &JsonValue, key: &str) -> Option<f64> {
    match object_field(schema, key)? {
        JsonValue::Number(number) => Some(number.float_value),
        JsonValue::String(value) => value.parse::<f64>().ok(),
        _ => None,
    }
}

fn schema_patterns(schema: &JsonValue) -> Option<Vec<String>> {
    let mut patterns = Vec::new();
    if let Some(pattern) = object_string(schema, "pattern") {
        patterns.push(pattern.to_string());
    }
    if let Some(array) = object_array(schema, "patterns") {
        for item in array {
            if let Some(pattern) = item.as_string() {
                patterns.push(pattern.to_string());
            }
        }
    }
    if let Some(format) = object_string(schema, "format") {
        if let Some(pattern) = known_format_pattern(format) {
            patterns.push(pattern.to_string());
        }
    }
    if patterns.is_empty() {
        None
    } else {
        Some(patterns)
    }
}

fn known_format_pattern(format: &str) -> Option<&'static str> {
    match format.to_ascii_lowercase().as_str() {
        "identifier" | "name" | "slug" => Some("^[A-Za-z0-9][A-Za-z0-9._-]*$"),
        "timestamp" | "utc-timestamp" | "iso8601-utc" | "rfc3339-utc" => {
            Some("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
        }
        _ => None,
    }
}

fn schema_enum_values(schema: &JsonValue) -> Option<Vec<JsonValue>> {
    if let Some(array) = object_array(schema, "values") {
        return Some(array.iter().cloned().collect());
    }
    if let Some(array) = object_array(schema, "variants") {
        return Some(array.iter().cloned().collect());
    }
    if let Some(array) = object_array(schema, "options") {
        return Some(array.iter().cloned().collect());
    }
    if let Some(object) = object_field(schema, "values")
        .and_then(JsonValue::as_object)
        .or_else(|| object_field(schema, "variants").and_then(JsonValue::as_object))
        .or_else(|| object_field(schema, "options").and_then(JsonValue::as_object))
    {
        let mut values = Vec::new();
        for (key, value) in object {
            match value {
                JsonValue::Null
                | JsonValue::Bool(_)
                | JsonValue::String(_)
                | JsonValue::Number(_) => values.push(value.clone()),
                JsonValue::Array(_) | JsonValue::Object(_) => {
                    values.push(JsonValue::String(key.clone()))
                }
            }
        }
        return Some(values);
    }
    None
}

fn schema_variants(schema: &JsonValue) -> Option<Vec<&JsonValue>> {
    if let Some(object) = object_field(schema, "variants").and_then(JsonValue::as_object) {
        return Some(object.values().collect());
    }
    if let Some(array) = object_array(schema, "variants") {
        return Some(array.iter().collect());
    }
    if let Some(object) = object_field(schema, "options").and_then(JsonValue::as_object) {
        return Some(object.values().collect());
    }
    if let Some(array) = object_array(schema, "options") {
        return Some(array.iter().collect());
    }
    if let Some(array) = object_array(schema, "oneOf") {
        return Some(array.iter().collect());
    }
    if let Some(array) = object_array(schema, "anyOf") {
        return Some(array.iter().collect());
    }
    None
}

fn tagged_union_variants(schema: &JsonValue) -> Option<BTreeMap<String, &JsonValue>> {
    let mut variants = BTreeMap::new();
    if let Some(object) = object_field(schema, "variants").and_then(JsonValue::as_object) {
        for (tag, variant) in object {
            if let Some(schema) = tagged_union_variant_schema(variant) {
                variants.insert(tag.clone(), schema);
            } else {
                variants.insert(tag.clone(), variant);
            }
        }
        return Some(variants);
    }

    if let Some(array) = object_array(schema, "variants") {
        for variant in array {
            let tag = tagged_union_variant_tag(variant)?;
            let schema = tagged_union_variant_schema(variant).unwrap_or(variant);
            variants.insert(tag, schema);
        }
        return Some(variants);
    }

    None
}

fn tagged_union_variant_tag(value: &JsonValue) -> Option<String> {
    if let Some(tag) = object_string(value, "tag") {
        return Some(tag.to_string());
    }
    if let Some(tag) = object_string(value, "value") {
        return Some(tag.to_string());
    }
    if let Some(tag) = object_string(value, "name") {
        return Some(tag.to_string());
    }
    if let Some(tag) = object_string(value, "key") {
        return Some(tag.to_string());
    }
    if let Some(tag) = object_field(value, "literal") {
        return scalar_key(tag);
    }
    None
}

fn tagged_union_variant_schema(value: &JsonValue) -> Option<&JsonValue> {
    object_field(value, "schema")
        .or_else(|| object_field(value, "valueSchema"))
        .or_else(|| object_field(value, "payload"))
        .or_else(|| object_field(value, "content"))
}

fn scalar_key(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => Some(value.clone()),
        JsonValue::Bool(value) => Some(value.to_string()),
        JsonValue::Null => Some("null".to_string()),
        JsonValue::Number(number) if number.integer => {
            if let Some(value) = number.int_value {
                Some(value.to_string())
            } else {
                Some(number.raw.clone())
            }
        }
        JsonValue::Number(number) => Some(number.raw.clone()),
        _ => None,
    }
}

fn json_equal(left: &JsonValue, right: &JsonValue) -> bool {
    match (left, right) {
        (JsonValue::Null, JsonValue::Null) => true,
        (JsonValue::Bool(a), JsonValue::Bool(b)) => a == b,
        (JsonValue::String(a), JsonValue::String(b)) => a == b,
        (JsonValue::Number(a), JsonValue::Number(b)) => {
            if let (Some(ai), Some(bi)) = (a.int_value, b.int_value) {
                ai == bi
            } else {
                a.float_value == b.float_value
            }
        }
        (JsonValue::Array(a), JsonValue::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(left, right)| json_equal(left, right))
        }
        (JsonValue::Object(a), JsonValue::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key)
                        .map(|other| json_equal(value, other))
                        .unwrap_or(false)
                })
        }
        _ => false,
    }
}

fn json_preview(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::String(value) => format!("{:?}", value),
        JsonValue::Number(number) => number.raw.clone(),
        JsonValue::Array(values) => format!("[{} items]", values.len()),
        JsonValue::Object(values) => format!("{{{} keys}}", values.len()),
    }
}

fn resolve_json_path<'a>(value: &'a JsonValue, path_expr: &str) -> Option<&'a JsonValue> {
    let path_expr = path_expr.trim();
    if path_expr.is_empty() || path_expr == "." {
        return Some(value);
    }

    let path_expr = path_expr.strip_prefix('.').unwrap_or(path_expr);
    if path_expr.is_empty() {
        return Some(value);
    }

    let mut current = value;
    for segment in path_expr.split('.') {
        if segment.is_empty() {
            continue;
        }
        current = match current {
            JsonValue::Object(map) => map.get(segment)?,
            JsonValue::Array(items) => {
                let index = segment.parse::<usize>().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }

    Some(current)
}

fn json_value_to_number_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Number(number) => Some(number.raw.clone()),
        JsonValue::String(text) => {
            if let Ok(value) = text.parse::<i128>() {
                Some(value.to_string())
            } else if let Ok(value) = text.parse::<f64>() {
                if value.is_finite() {
                    Some(if value.fract() == 0.0 {
                        format!("{:.0}", value)
                    } else {
                        value.to_string()
                    })
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn render_json_compact(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(number) => number.raw.clone(),
        JsonValue::String(value) => format!("\"{}\"", escape_json_string(value)),
        JsonValue::Array(values) => {
            let rendered = values
                .iter()
                .map(render_json_compact)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{}]", rendered)
        }
        JsonValue::Object(values) => {
            let rendered = values
                .iter()
                .map(|(key, item)| {
                    format!(
                        "\"{}\":{}",
                        escape_json_string(key),
                        render_json_compact(item)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{}}}", rendered)
        }
    }
}

fn escape_json_string(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch < ' ' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn value_type(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::String(_) => "string",
        JsonValue::Number(number) if number.integer => "integer",
        JsonValue::Number(_) => "number",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

struct SimplePattern {
    pieces: Vec<PatternPiece>,
}

#[derive(Clone)]
struct PatternPiece {
    atom: PatternAtom,
    min: usize,
    max: Option<usize>,
}

#[derive(Clone)]
enum PatternAtom {
    Literal(char),
    Any,
    Class(CharClass),
}

#[derive(Clone)]
struct CharClass {
    negated: bool,
    items: Vec<CharClassItem>,
}

#[derive(Clone)]
enum CharClassItem {
    Single(char),
    Range(char, char),
    Digit,
    Word,
    Space,
}

impl SimplePattern {
    fn compile(pattern: &str) -> Result<Self, String> {
        let mut source = pattern.trim();
        if source.starts_with('^') && source.ends_with('$') && source.len() >= 2 {
            source = &source[1..source.len() - 1];
        }

        let mut chars = source.chars().peekable();
        let mut pieces = Vec::new();
        while let Some(ch) = chars.next() {
            let atom = match ch {
                '.' => PatternAtom::Any,
                '\\' => PatternAtom::Literal(parse_regex_escape(&mut chars)?),
                '[' => PatternAtom::Class(parse_char_class(&mut chars)?),
                '*' | '+' | '?' | '{' => {
                    return Err(format!("unexpected quantifier '{}' without atom", ch))
                }
                other => PatternAtom::Literal(other),
            };

            let (min, max) = parse_quantifier(&mut chars)?;
            pieces.push(PatternPiece { atom, min, max });
        }

        Ok(Self { pieces })
    }

    fn matches(&self, value: &str) -> bool {
        let chars: Vec<char> = value.chars().collect();
        matches_from(&self.pieces, 0, &chars, 0)
    }
}

fn matches_from(pieces: &[PatternPiece], index: usize, chars: &[char], position: usize) -> bool {
    if index == pieces.len() {
        return position == chars.len();
    }

    let piece = &pieces[index];
    let remaining = chars.len().saturating_sub(position);
    let maximum = piece.max.unwrap_or(remaining).min(remaining);
    let mut counts = Vec::new();
    for count in piece.min..=maximum {
        counts.push(count);
    }

    for count in counts.into_iter().rev() {
        if matches_piece(piece, chars, position, count)
            && matches_from(pieces, index + 1, chars, position + count)
        {
            return true;
        }
    }

    false
}

fn matches_piece(piece: &PatternPiece, chars: &[char], position: usize, count: usize) -> bool {
    for offset in 0..count {
        let Some(ch) = chars.get(position + offset) else {
            return false;
        };
        if !matches_atom(&piece.atom, *ch) {
            return false;
        }
    }
    true
}

fn matches_atom(atom: &PatternAtom, ch: char) -> bool {
    match atom {
        PatternAtom::Literal(expected) => *expected == ch,
        PatternAtom::Any => true,
        PatternAtom::Class(class) => class.matches(ch),
    }
}

fn parse_quantifier(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<(usize, Option<usize>), String> {
    match chars.peek().copied() {
        Some('*') => {
            let _ = chars.next();
            Ok((0, None))
        }
        Some('+') => {
            let _ = chars.next();
            Ok((1, None))
        }
        Some('?') => {
            let _ = chars.next();
            Ok((0, Some(1)))
        }
        Some('{') => {
            let _ = chars.next();
            let minimum = parse_number_literal(chars)?;
            let maximum = match chars.peek().copied() {
                Some(',') => {
                    let _ = chars.next();
                    if matches!(chars.peek(), Some('}')) {
                        None
                    } else {
                        Some(parse_number_literal(chars)?)
                    }
                }
                _ => Some(minimum),
            };
            if chars.next() != Some('}') {
                return Err("unterminated quantifier".to_string());
            }
            if let Some(maximum) = maximum {
                if maximum < minimum {
                    return Err("quantifier maximum is smaller than minimum".to_string());
                }
            }
            Ok((minimum, maximum))
        }
        _ => Ok((1, Some(1))),
    }
}

fn parse_number_literal(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<usize, String> {
    let mut digits = String::new();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            let _ = chars.next();
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return Err("expected number in quantifier".to_string());
    }
    digits
        .parse::<usize>()
        .map_err(|err| format!("invalid quantifier number: {}", err))
}

fn parse_char_class(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<CharClass, String> {
    let mut negated = false;
    if chars.peek() == Some(&'^') {
        negated = true;
        let _ = chars.next();
    }

    let mut items = Vec::new();
    let mut first = true;
    while let Some(ch) = chars.next() {
        if ch == ']' && !first {
            return Ok(CharClass { negated, items });
        }
        first = false;

        let item = if ch == '\\' {
            parse_char_class_escape(chars)?
        } else {
            CharClassItem::Single(ch)
        };

        if let Some('-') = chars.peek().copied() {
            let mut clone = chars.clone();
            let _ = clone.next();
            if let Some(']') = clone.peek().copied() {
                items.push(item);
                items.push(CharClassItem::Single('-'));
                let _ = chars.next();
                continue;
            }

            let _ = chars.next();
            let end = match chars.next() {
                Some(']') | None => return Err("unterminated character class range".to_string()),
                Some('\\') => parse_char_class_escape(chars)?,
                Some(other) => CharClassItem::Single(other),
            };

            let (start_char, end_char) = match (item, end) {
                (CharClassItem::Single(start), CharClassItem::Single(end)) => (start, end),
                _ => {
                    return Err("character class ranges only support literal endpoints".to_string())
                }
            };

            if start_char > end_char {
                return Err("character class range is reversed".to_string());
            }
            items.push(CharClassItem::Range(start_char, end_char));
        } else {
            items.push(item);
        }
    }

    Err("unterminated character class".to_string())
}

fn parse_char_class_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<CharClassItem, String> {
    match chars.next() {
        Some('d') => Ok(CharClassItem::Digit),
        Some('w') => Ok(CharClassItem::Word),
        Some('s') => Ok(CharClassItem::Space),
        Some('n') => Ok(CharClassItem::Single('\n')),
        Some('r') => Ok(CharClassItem::Single('\r')),
        Some('t') => Ok(CharClassItem::Single('\t')),
        Some(other) => Ok(CharClassItem::Single(other)),
        None => Err("unterminated escape in character class".to_string()),
    }
}

fn parse_regex_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<char, String> {
    match chars.next() {
        Some('d') => Err("regex shorthand \\d must be used inside a character class".to_string()),
        Some('w') => Err("regex shorthand \\w must be used inside a character class".to_string()),
        Some('s') => Err("regex shorthand \\s must be used inside a character class".to_string()),
        Some('n') => Ok('\n'),
        Some('r') => Ok('\r'),
        Some('t') => Ok('\t'),
        Some(other) => Ok(other),
        None => Err("unterminated regex escape".to_string()),
    }
}

impl CharClass {
    fn matches(&self, ch: char) -> bool {
        let matched = self.items.iter().any(|item| match item {
            CharClassItem::Single(expected) => *expected == ch,
            CharClassItem::Range(start, end) => *start <= ch && ch <= *end,
            CharClassItem::Digit => ch.is_ascii_digit(),
            CharClassItem::Word => ch.is_ascii_alphanumeric() || ch == '_',
            CharClassItem::Space => ch.is_whitespace(),
        });

        if self.negated {
            !matched
        } else {
            matched
        }
    }
}
