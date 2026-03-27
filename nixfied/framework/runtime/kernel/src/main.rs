use serde_json::{json, Map, Number, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::{self, Command, Stdio};
use std::time::Duration;

mod adapter;
mod event;
mod io_util;
mod json_util;
mod machine_output;
mod parse_util;
mod policy;
mod probe;
mod registry;
mod run_id;
mod run_record;
mod runtime_metadata;
mod summary;
mod task;
mod validation;
mod workflow;

use io_util::*;
use json_util::*;
use parse_util::*;
use validation::validate_json_value_against_contract;
use workflow::WorkflowUnitStateCommon;

fn main() {
    if let Err(err) = run() {
        eprintln!("ERROR: {}", err);
        process::exit(1);
    }
}

const SUB_COMMANDS: &[(&str, fn(&str, &[String]) -> Result<(), String>)] = &[
    ("run-id", run_id::run_id_command),
    ("event-detail", event::event_detail_command),
    ("event-state", event::event_state_command),
    ("service-policy", policy::service_policy_command),
    ("run-record", run_record::run_record_command),
    ("task", task::task_command),
    ("workflow", workflow::workflow_command),
    ("registry", registry::registry_command),
    ("summary", summary::summary_command),
    ("adapter", adapter::adapter_command),
    ("probe", probe::probe_command),
    ("machine-output", machine_output::machine_output_command),
];

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
            let remaining = args.collect::<Vec<_>>();
            validate_input_command(&plan_path, &mode, &remaining)
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
        other => {
            if let Some((_, handler)) = SUB_COMMANDS.iter().find(|(name, _)| *name == other) {
                let subcommand = args.next().ok_or_else(usage)?;
                let values = args.collect::<Vec<_>>();
                handler(&subcommand, &values)
            } else {
                Err(format!("unknown command: {}\n{}", other, usage()))
            }
        }
    }
}

fn usage() -> String {
    [
        "usage: nixfied-kernel <command> [args...]",
        "",
        "commands:",
        "  validate-payload <bundle-file> <contract-ref> <payload-file>",
        "  validate-artifact <bundle-file> <contract-ref> <payload-file>",
        "  validate-input <plan-file> <env|args> [-- <args...>]",
        "  validate-scalar <spec-file> <value>",
        "  validate-exit <plan-file> <exit-code>",
        "  run-id <envelope> ...",
        "  event-detail <render> ...",
        "  event-state <derive> ...",
        "  service-policy <runtime-event|start-service|fixture-keep-running> ...",
        "  run-record <create|transition> ...",
        "  task <execution-order|exists|workflow-ref|validate-args|render-help|load-runtime|load-hook> ...",
        "  workflow <resolve-mode|load-runtime|run> ...",
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

impl WorkflowUnitStateCommon for WorkflowSerialUnitState {
    fn state(&self) -> &str {
        &self.state
    }

    fn set_state(&mut self, value: &str) {
        self.state = value.to_string();
    }

    fn set_cancel_reason(&mut self, value: &str) {
        self.cancel_reason = value.to_string();
    }

    fn set_cancel_extra_key(&mut self, value: &str) {
        self.cancel_extra_key = value.to_string();
    }

    fn set_cancel_extra_value(&mut self, value: &str) {
        self.cancel_extra_value = value.to_string();
    }

    fn dependents(&self) -> &[String] {
        &self.dependents
    }

    fn needs_left(&self) -> i64 {
        self.needs_left
    }

    fn set_needs_left(&mut self, value: i64) {
        self.needs_left = value;
    }

    fn task_id(&self) -> &str {
        &self.task_id
    }
}

impl WorkflowUnitStateCommon for WorkflowParallelUnitState {
    fn state(&self) -> &str {
        &self.state
    }

    fn set_state(&mut self, value: &str) {
        self.state = value.to_string();
    }

    fn set_cancel_reason(&mut self, value: &str) {
        self.cancel_reason = value.to_string();
    }

    fn set_cancel_extra_key(&mut self, value: &str) {
        self.cancel_extra_key = value.to_string();
    }

    fn set_cancel_extra_value(&mut self, value: &str) {
        self.cancel_extra_value = value.to_string();
    }

    fn dependents(&self) -> &[String] {
        &self.dependents
    }

    fn needs_left(&self) -> i64 {
        self.needs_left
    }

    fn set_needs_left(&mut self, value: i64) {
        self.needs_left = value;
    }

    fn task_id(&self) -> &str {
        &self.task_id
    }
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

fn validate_input_command(plan_path: &str, mode: &str, remaining: &[String]) -> Result<(), String> {
    validation::validate_input_command(plan_path, mode, remaining)
}

fn validate_scalar_command(spec_path: &str, value: &str) -> Result<(), String> {
    validation::validate_scalar_command(spec_path, value)
}

fn validate_exit_command(plan_path: &str, exit_code_text: &str) -> Result<(), String> {
    validation::validate_exit_command(plan_path, exit_code_text)
}

fn validate_file_command(
    command: &str,
    bundle_path: &str,
    contract_ref: &str,
    payload_path: &str,
) -> Result<(), String> {
    validation::validate_file_command(command, bundle_path, contract_ref, payload_path)
}

pub(crate) fn validate_and_write_json(
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

pub(crate) fn parse_json_file(path: &str, label: &str) -> Result<JsonValue, String> {
    let text = read_text(path)?;
    parse_json(&text).map_err(|err| format!("{} {} is not valid JSON: {}", label, path, err))
}

pub(crate) fn run_record_history_entry(state: &str, at: &str) -> JsonValue {
    json!({ "state": state, "at": at })
}

pub(crate) fn load_line_set(path: &str) -> Result<BTreeSet<String>, String> {
    let text = read_text(path)?;
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect())
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
        JsonValue::Number(number) => number.to_string(),
        JsonValue::Array(_) | JsonValue::Object(_) => render_json_compact(value),
    }
}

fn required_port_from_env(name: &str) -> Result<String, String> {
    let value = env::var(name).map_err(|_| format!("required env var missing name={}", name))?;
    if value.trim().is_empty() {
        return Err(format!("required env var empty name={}", name));
    }
    let port = parse_i64_text(&value, &format!("env:{}", name))?;
    if !(1..=65535).contains(&port) {
        return Err(format!(
            "env:{} must be port 1-65535 (got '{}')",
            name, value
        ));
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

fn is_hex_prefixed(value: &str) -> bool {
    value.len() >= 3
        && value.starts_with("0x")
        && value.chars().skip(2).all(|ch| ch.is_ascii_hexdigit())
}
