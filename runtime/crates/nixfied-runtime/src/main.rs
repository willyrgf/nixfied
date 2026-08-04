use std::fmt::Display;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use nixfied_model::ServiceLifetime;
use nixfied_runtime::cancellation::{CancellationToken, ProcessSignalGuard};
use nixfied_runtime::error::RuntimeCause;
use nixfied_runtime::execution::plan;
use nixfied_runtime::output::{EvidenceMode, ReplaySinks, ReplayTicket};
use nixfied_runtime::redaction::Redactor;
use nixfied_runtime::registry::{Registry, RegistryIdentity, RunLeaseHeartbeat};
use nixfied_runtime::service::{
    PrepareRunner, PrepareTaskError, RunContext, SelectedEndpoint, ServiceSelection,
    StartedService, TaskExecution, TaskExecutionError, TaskRun, mark_run_completed,
    mark_run_failed, record_run_created, run_dependent_task_cancellable, run_slot_clean,
    start_service_for_slot,
};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{
    StateIdentity, derive_host_placement_for_slot, materialize_registry_root, prepare_slot_state,
    state_base_from_env,
};
use nixfied_runtime::{
    Admission, AdmissionContext, RuntimeError, StoreOriginPolicy, parse_loaded_model,
    read_raw_model,
};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckOutput {
    model_path: PathBuf,
    computed_model_hash: String,
    raw_len: usize,
    project_id: String,
    runtime_abi: String,
    toolchain_id: String,
    target_system: String,
    environment: String,
    slot: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceRunOutput {
    service_id: String,
    service_instance_id: String,
    process_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_endpoint: Option<SelectedEndpoint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeResult {
    node_id: String,
    task_id: String,
    success: bool,
    exit_code: Option<i32>,
    duration_ms: u64,
    // Where the node's full execution detail lives — the task's captured stdout,
    // stderr, and summary — so the summary links each node straight to its
    // evidence without a separate lookup.
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    summary_path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunOutput {
    run_id: String,
    model_path: PathBuf,
    computed_model_hash: String,
    duration_ms: u64,
    services: Vec<ServiceRunOutput>,
    tasks: Vec<TaskRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<TaskRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    nodes: Vec<NodeResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_summary_path: Option<PathBuf>,
}

enum ReplayPlan {
    None,
    Selected(ReplayTicket),
}

struct RunSession<'a> {
    placement: &'a nixfied_runtime::state::HostPlacement,
    admission: &'a Admission,
    options: &'a RunOptions,
    redactor: &'a Redactor,
    cancellation: &'a CancellationToken,
    run_id: &'a str,
    run_started: Instant,
    registry: Registry,
    started: Vec<StartedService>,
    extra_services: Vec<ServiceRunOutput>,
    service_lifetime: ServiceLifetime,
    direct_selected: bool,
    lease: Option<RunLeaseHeartbeat>,
    task_runs: Vec<TaskRun>,
    selected_task_run: Option<TaskRun>,
    node_results: Vec<NodeResult>,
    replay: ReplayPlan,
    diagnostic_failures: Vec<RuntimeError>,
}

struct FailureAccumulator {
    primary: Option<RuntimeError>,
    causes: Vec<RuntimeCause>,
}

impl FailureAccumulator {
    fn new() -> Self {
        Self {
            primary: None,
            causes: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.primary.is_none()
    }

    fn push(&mut self, error: RuntimeError) {
        let Some(mut primary) = self.primary.take() else {
            self.primary = Some(error);
            return;
        };
        if failure_priority(error.code) > failure_priority(primary.code) {
            let mut error = error;
            let incoming = std::mem::take(&mut error.causes);
            error.causes.extend(*incoming);
            error.causes.extend(primary.causes.drain(..));
            error.causes.extend(self.causes.drain(..));
            error.causes.push(RuntimeCause::from_error(primary));
            self.primary = Some(error);
        } else {
            let mut error = error;
            let incoming = std::mem::take(&mut error.causes);
            self.causes.extend(*incoming);
            self.causes.push(RuntimeCause::from_error(error));
            self.primary = Some(primary);
        }
    }

    fn finish(self, output: RunOutput) -> Result<RunOutput, RuntimeError> {
        let Some(mut primary) = self.primary else {
            return Ok(output);
        };
        primary.causes.extend(self.causes);
        Err(primary)
    }
}

fn failure_priority(code: nixfied_runtime::ErrorCode) -> u8 {
    match code {
        nixfied_runtime::ErrorCode::OutputProjectionFailed => 2,
        nixfied_runtime::ErrorCode::TaskFailed
        | nixfied_runtime::ErrorCode::Canceled
        | nixfied_runtime::ErrorCode::DependencyUnavailable => 1,
        _ => 3,
    }
}

impl<'a> RunSession<'a> {
    fn finalize(mut self, initial_error: Option<RuntimeError>) -> Result<RunOutput, RuntimeError> {
        let had_initial_outcome = initial_error.is_some();
        let cancellation_seen = self.cancellation.is_canceled();
        let mut cancellation_recorded = initial_error
            .as_ref()
            .is_some_and(|error| error.code == nixfied_runtime::ErrorCode::Canceled);
        let mut failures = FailureAccumulator::new();
        if let Some(error) = initial_error {
            failures.push(error);
        }
        if !had_initial_outcome && cancellation_seen {
            failures.push(nixfied_runtime::cancellation::canceled_error());
            cancellation_recorded = true;
        }
        for error in self.diagnostic_failures.drain(..) {
            failures.push(error);
        }

        if let ReplayPlan::Selected(ticket) = std::mem::replace(&mut self.replay, ReplayPlan::None)
            && let Some(error) = ticket.replay(ReplaySinks::stdio()).into_error()
        {
            failures.push(error);
        }
        if self.cancellation.is_canceled() && !cancellation_recorded {
            failures.push(nixfied_runtime::cancellation::canceled_error());
        }

        let services = {
            let mut services = self.extra_services;
            services.extend(services_output(&self.started));
            services
        };
        let mut canceled = self.cancellation.is_canceled()
            || failures
                .primary
                .as_ref()
                .is_some_and(|error| error.code == nixfied_runtime::ErrorCode::Canceled);
        let had_initial_failure = !failures.is_empty();
        while let Some(mut service) = self.started.pop() {
            let result = if canceled {
                service.cancel(&mut self.registry, self.options.timeout_ms, "run canceled")
            } else if had_initial_failure {
                service.stop(&mut self.registry, self.options.timeout_ms)
            } else if self.service_lifetime == ServiceLifetime::RunScoped {
                service.stop_cancellable(
                    &mut self.registry,
                    self.options.timeout_ms,
                    self.cancellation,
                )
            } else {
                service.stand(&mut self.registry)
            };
            if let Err(error) = result {
                failures.push(error);
            }
        }
        if let Some(lease) = self.lease.take()
            && let Err(error) = lease.stop()
        {
            failures.push(error);
        }
        if self.cancellation.is_canceled() && !cancellation_recorded {
            failures.push(nixfied_runtime::cancellation::canceled_error());
        }
        canceled |= self.cancellation.is_canceled()
            || failures
                .primary
                .as_ref()
                .is_some_and(|error| error.code == nixfied_runtime::ErrorCode::Canceled);

        let registry_result = if failures.is_empty() {
            mark_run_completed(&mut self.registry, self.run_id)
        } else {
            mark_run_failed(&mut self.registry, self.run_id, canceled)
        };
        if let Err(error) = registry_result {
            failures.push(error);
        }

        let duration_ms = elapsed_ms(self.run_started);
        let run_succeeded = failures.is_empty();
        let run_summary_path = match write_run_summary(RunSummary {
            placement: self.placement,
            run_id: self.run_id,
            run_succeeded,
            duration_ms,
            nodes: &self.node_results,
            services: &services,
            tasks: &self.task_runs,
            redactor: self.redactor,
        }) {
            Ok(path) => Some(path),
            Err(error) => {
                failures.push(error);
                None
            }
        };
        let footer_succeeded = failures.is_empty();
        if let Err(error) = print_run_footer(
            self.options.output_mode,
            footer_succeeded,
            &self.node_results,
            duration_ms,
            run_summary_path.as_deref(),
            &self.placement.logs_dir,
        ) {
            failures.push(error);
        }

        let primary_task = if self.direct_selected {
            self.selected_task_run.clone()
        } else {
            self.task_runs.last().cloned()
        };
        let output = RunOutput {
            run_id: self.run_id.to_string(),
            model_path: self.admission.model_path.clone(),
            computed_model_hash: self.admission.computed_model_hash.clone(),
            duration_ms,
            services,
            summary_path: primary_task.as_ref().map(|task| task.summary_path.clone()),
            task: primary_task,
            tasks: self.task_runs,
            nodes: self.node_results,
            run_summary_path: run_summary_path.clone(),
        };
        match failures.finish(output) {
            Ok(output) => Ok(output),
            Err(error) => Err(with_failure_summary(error, run_summary_path)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeSelection {
    slot: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunOutputMode {
    Summary,
    Json,
    Both,
    TaskOutput,
}

impl RunOutputMode {
    fn emit_summary(self) -> bool {
        matches!(self, Self::Summary | Self::Both | Self::TaskOutput)
    }

    fn emit_json(self) -> bool {
        matches!(self, Self::Json | Self::Both)
    }

    fn is_task_output(self) -> bool {
        matches!(self, Self::TaskOutput)
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let exit = match ProcessSignalGuard::install() {
        Ok(signals) => {
            let exit = match run(&args) {
                Ok(()) => 0,
                Err(error) => {
                    let exit = exit_code(&error);
                    match print_error(&args, &error) {
                        Ok(()) => exit,
                        Err(projection_error) => exit_code(&projection_error),
                    }
                }
            };
            // Restore SIGINT, SIGTERM, SIGHUP, and SIGPIPE before the final
            // process exit. `std::process::exit` does not run Drop handlers.
            drop(signals);
            exit
        }
        Err(error) => {
            let exit = exit_code(&error);
            let _ = print_error(&args, &error);
            exit
        }
    };
    std::process::exit(exit);
}

fn run(args: &[String]) -> Result<(), RuntimeError> {
    let command = args.first().map(String::as_str).unwrap_or("check");
    match command {
        "check" => check(args.get(1..).unwrap_or(&[])),
        "run" => run_m0(args.get(1..).unwrap_or(&[])),
        "ps" => run_control(ControlCommand::Ps, args.get(1..).unwrap_or(&[])),
        "down" => run_control(ControlCommand::Down, args.get(1..).unwrap_or(&[])),
        "clean" => run_control(ControlCommand::Clean, args.get(1..).unwrap_or(&[])),
        _ => Err(RuntimeError::unsupported_feature(
            "runtime.command",
            format!("unsupported runtime command: {command}"),
        )
        .with_detail("command", command)),
    }
}

fn print_error(args: &[String], error: &RuntimeError) -> Result<(), RuntimeError> {
    match error_output_projection(args) {
        ErrorOutputProjection::Human => print_human_error(error),
        ErrorOutputProjection::Json => print_json_error(error),
        ErrorOutputProjection::Both => {
            print_human_error(error)?;
            print_json_error(error)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorOutputProjection {
    Human,
    Json,
    Both,
}

fn error_output_projection(args: &[String]) -> ErrorOutputProjection {
    if args.first().map(String::as_str) != Some("run") {
        return ErrorOutputProjection::Human;
    }
    let args = args.get(1..).unwrap_or(&[]);
    let mut json = false;
    let mut both = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--both" => both = true,
            "--json" => json = true,
            "--output" => {
                if let Some(value) = args.get(index + 1).map(String::as_str) {
                    match value {
                        "both" => both = true,
                        "json" => json = true,
                        _ => {}
                    }
                    index += 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    if both {
        ErrorOutputProjection::Both
    } else if json {
        ErrorOutputProjection::Json
    } else {
        ErrorOutputProjection::Human
    }
}

fn print_json_error(error: &RuntimeError) -> Result<(), RuntimeError> {
    write_stderr_line(serde_json::to_string(error).unwrap_or_else(|_| error.to_string()))
}

fn print_human_error(error: &RuntimeError) -> Result<(), RuntimeError> {
    write_stderr_line(format!(
        "error: {}: {}",
        error_code_wire(error),
        error.message
    ))?;
    for cause in error.causes.iter() {
        write_stderr_line(format!(
            "  cause: {}: {}",
            error_code_wire_value(cause.code),
            cause.message
        ))?;
    }
    if let Some(projections) = error.details.get("projections").and_then(Value::as_array) {
        for projection in projections {
            let stream = projection
                .get("stream")
                .and_then(Value::as_str)
                .unwrap_or("output");
            let operation = projection
                .get("operation")
                .and_then(Value::as_str)
                .unwrap_or("projection");
            let kind = projection
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("io");
            write_stderr_line(format!(
                "  projection: {stream} {operation} failed ({kind})"
            ))?;
        }
    }
    print_path_detail(error, "state-root", "stateRoot")?;
    print_path_detail(error, "registry-dir", "registryDir")?;
    print_path_detail(error, "registry", "registryPath")?;
    print_path_detail(error, "logs", "logsDir")?;
    print_path_detail(error, "run-summary", "runSummaryPath")?;
    if let Some(expected) = registry_identity_detail(error, "expectedRegistryIdentity") {
        write_stderr_line(format!("expected: {expected}"))?;
    }
    if let Some(found) = registry_identity_detail(error, "foundRegistryIdentity") {
        write_stderr_line(format!("found: {found}"))?;
    }
    if let Some(fields) = mismatched_fields_detail(error) {
        write_stderr_line(format!("mismatch: {fields}"))?;
    }
    print_recovery_hint(error)
}

fn print_path_detail(error: &RuntimeError, label: &str, key: &str) -> Result<(), RuntimeError> {
    if let Some(value) = string_detail(error, key) {
        write_stderr_line(format!("{label}: {}", human_path(Path::new(value))))?;
    }
    Ok(())
}

fn registry_identity_detail(error: &RuntimeError, key: &str) -> Option<String> {
    let value = error.details.get(key)?;
    Some(format!(
        "projectId={} environment={} slot={} runtimeAbi={} toolchainId={}",
        scalar_detail(value.get("projectId")),
        scalar_detail(value.get("environment")),
        scalar_detail(value.get("slot")),
        scalar_detail(value.get("runtimeAbi")),
        scalar_detail(value.get("toolchainId")),
    ))
}

fn scalar_detail(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => "unknown".to_string(),
    }
}

fn mismatched_fields_detail(error: &RuntimeError) -> Option<String> {
    let fields = error.details.get("mismatchedFields")?.as_array()?;
    let fields = fields.iter().filter_map(Value::as_str).collect::<Vec<_>>();
    (!fields.is_empty()).then(|| fields.join(", "))
}

fn print_recovery_hint(error: &RuntimeError) -> Result<(), RuntimeError> {
    if !matches!(
        error.code,
        nixfied_runtime::ErrorCode::StateUnowned | nixfied_runtime::ErrorCode::RuntimeAbiMismatch
    ) {
        return Ok(());
    }
    if string_detail(error, "stateRoot").is_none() && string_detail(error, "registryDir").is_none()
    {
        return Ok(());
    }
    write_stderr_line("hint: do not delete the whole Nixfied state base")?;
    write_stderr_line(
        "hint: after confirming no owned processes are live, reset only the state-root and registry-dir above",
    )
}

fn write_stderr_line(line: impl Display) -> Result<(), RuntimeError> {
    writeln!(io::stderr().lock(), "{line}")
        .map_err(|error| output_projection_io_error("stderr", "write", "<stderr>", error))
}

fn string_detail<'a>(error: &'a RuntimeError, key: &str) -> Option<&'a str> {
    error.details.get(key).and_then(Value::as_str)
}

const CHECK_HELP: &str = "\
Admit the compiled Nixfied model without executing tasks.

Usage:
  nix run .#model-check -- [options]

Options:
  --slot <number>  Select a declared project slot
  -h, --help       Show this help";

fn check(args: &[String]) -> Result<(), RuntimeError> {
    if print_help_if_requested(args, CHECK_HELP) {
        return Ok(());
    }
    let mut model_path = None;
    let mut allow_non_store = false;
    let mut slot = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--model" => {
                index += 1;
                model_path = args.get(index).map(PathBuf::from);
            }
            "--allow-non-store-model" => {
                allow_non_store = true;
            }
            "--slot" => {
                index += 1;
                slot = Some(parse_slot_arg(args.get(index), "--slot")?);
            }
            other => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::ModelAdmission,
                    format!("unknown check argument: {other}"),
                ));
            }
        }
        index += 1;
    }
    let model_path = model_path.ok_or_else(|| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            "missing --model path",
        )
    })?;
    let (loaded, admission) = load_admitted_model(model_path, allow_non_store)?;
    let selected_slot = select_slot(&loaded.model, slot).map_err(post_admission_error)?;
    let output = CheckOutput {
        model_path: admission.model_path,
        computed_model_hash: admission.computed_model_hash,
        raw_len: admission.raw_len,
        project_id: admission.project_id,
        runtime_abi: admission.runtime_abi,
        toolchain_id: admission.toolchain_id,
        target_system: admission.target_system,
        environment: selected_slot.environment.to_string(),
        slot: selected_slot.slot,
    };
    let output = serde_json::to_string_pretty(&output).map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::LifecycleFailed,
            error.to_string(),
        )
    })?;
    write_stdout_line(&output)
}

fn run_m0(args: &[String]) -> Result<(), RuntimeError> {
    if print_help_if_requested(args, RUN_HELP) {
        return Ok(());
    }
    let options = parse_run_options(args)?;
    let cancellation = CancellationToken::new();
    let run_id = new_run_id();
    let (loaded, admission) =
        load_admitted_model(options.model_path.clone(), options.allow_non_store)?;
    validate_run_selection(
        &admission.execution_model,
        options.output_mode,
        options.task.as_deref(),
    )?;
    let redactor = Redactor::from_secrets(&admission.secrets);
    let model_path = admission.model_path.clone();
    let computed_model_hash = admission.computed_model_hash.clone();
    let output = run_m0_admitted(
        &loaded.model,
        &admission,
        &redactor,
        &options,
        run_id,
        &cancellation,
    )
    .map_err(|error| {
        redactor.redact_error(error.with_model_if_missing(model_path, computed_model_hash))
    })?;
    if options.output_mode.emit_json() {
        print_json_redacted(&output, &redactor)?;
    }
    Ok(())
}

fn run_m0_admitted(
    model: &nixfied_model::Model,
    admission: &Admission,
    redactor: &Redactor,
    options: &RunOptions,
    run_id: String,
    cancellation: &CancellationToken,
) -> Result<RunOutput, RuntimeError> {
    cancellation.check()?;
    let selected_slot = select_slot(model, options.selection.slot).map_err(post_admission_error)?;
    let placement =
        derive_host_placement_for_slot(model, &selected_slot, &run_id, &options.state_base)
            .map_err(post_admission_error)?;
    // Every failure past this point carries the run's identity and state paths:
    // the operator must be able to find the evidence without re-deriving the
    // placement by hand.
    run_m0_placed(
        model,
        admission,
        redactor,
        options,
        &run_id,
        &selected_slot,
        &placement,
        cancellation,
    )
    .map_err(|error| enrich_run_error(error, &run_id, &placement, &selected_slot))
}

/// Attach the run identity and state paths to a run error, preserving any
/// details the failure site already recorded.
fn enrich_run_error(
    error: RuntimeError,
    run_id: &str,
    placement: &nixfied_runtime::state::HostPlacement,
    selected_slot: &nixfied_runtime::slot::SelectedSlot<'_>,
) -> RuntimeError {
    enrich_placed_error(error, placement, selected_slot)
        .with_detail("runId", run_id)
        .with_detail("runDir", &placement.run_dir)
        .with_detail("logsDir", &placement.logs_dir)
}

/// Lowering and admission prove that the execution plan is concrete. Any
/// defensive invariant error encountered after that boundary is therefore a
/// runtime lifecycle failure, never another model-admission failure.
fn post_admission_error(error: RuntimeError) -> RuntimeError {
    if error.code != nixfied_runtime::ErrorCode::ModelAdmission {
        return error;
    }
    RuntimeError::new(
        nixfied_runtime::ErrorCode::LifecycleFailed,
        format!("admitted execution invariant failed: {}", error.message),
    )
    .with_details(error.details)
}

/// Attach selected slot placement to errors from run/control execution. These
/// paths are runtime materialization details, so the runtime reports them
/// directly instead of asking operators to re-derive them.
fn enrich_placed_error(
    error: RuntimeError,
    placement: &nixfied_runtime::state::HostPlacement,
    selected_slot: &nixfied_runtime::slot::SelectedSlot<'_>,
) -> RuntimeError {
    error
        .with_detail("environment", selected_slot.environment)
        .with_detail("slot", selected_slot.slot)
        .with_detail("stateBase", &placement.state_base)
        .with_detail("stateRoot", &placement.state_root)
        .with_detail("registryDir", &placement.registry_dir)
        .with_detail("registryPath", placement.registry_path())
}

#[allow(clippy::too_many_arguments)]
fn run_m0_placed(
    model: &nixfied_model::Model,
    admission: &Admission,
    redactor: &Redactor,
    options: &RunOptions,
    run_id: &str,
    selected_slot: &nixfied_runtime::slot::SelectedSlot<'_>,
    placement: &nixfied_runtime::state::HostPlacement,
    cancellation: &CancellationToken,
) -> Result<RunOutput, RuntimeError> {
    let run_started = Instant::now();
    let mut diagnostic_failures: Vec<RuntimeError> = Vec::new();
    // The registry opens before the marker decision: when the slot was last
    // used by a different model build, the upgrade path needs registry evidence
    // to tear down what that build left running.
    materialize_registry_root(placement)?;
    let identity = StateIdentity::from_selected_slot(model, admission, selected_slot);
    let mut registry = Registry::open_or_create(
        placement.registry_path(),
        &RegistryIdentity::for_slot(
            &model.project.project_id,
            selected_slot.environment,
            selected_slot.slot,
            &model.runtime_abi,
            &model.toolchain_id,
        ),
    )?;
    registry.set_redactor(redactor.clone());
    let _ = nixfied_runtime::control::reconcile_registry(&mut registry)?;
    let upgrade = prepare_slot_state(placement, &identity, &mut registry, options.timeout_ms)?;
    if upgrade.upgraded
        && options.output_mode.emit_summary()
        && let Err(error) = write_diagnostic(
            options.output_mode,
            format_args!(
                "  upgraded slot state from model {} (state {})",
                upgrade.from_model_hash.as_deref().unwrap_or("unknown"),
                if upgrade.cleaned {
                    "cleaned: state epoch changed"
                } else {
                    "preserved"
                }
            ),
        )
    {
        diagnostic_failures.push(error);
    }

    // A run drives one selected task: its flattened nodes plus the derived
    // service union, started eagerly. The plan (service ports + node order) is
    // a pure function of the lowered model, the task, and the slot, already
    // proven feasible at admission. `run` with no selection refuses and lists
    // the declared tasks — there is no implicit default.
    let Some(task_name) = options.task.as_deref() else {
        return Err(RuntimeError::new(
            nixfied_runtime::ErrorCode::LifecycleFailed,
            "admitted run selection lost its task identity",
        ));
    };
    let selected_task = nixfied_model::TaskId::new(task_name);
    let plan = plan(
        &admission.execution_model,
        &selected_task,
        selected_slot.slot,
    )
    .map_err(post_admission_error)?;
    let direct_selected = admission.execution_model.tasks.contains_key(&selected_task);

    // Record the run row before any service starts, so even a service-less
    // selection (a task tree whose leaves require nothing) leaves durable run
    // evidence for `ps`/reconcile. Service transitions require this exact row
    // and never create or repair it themselves.
    record_run_created(&mut registry, run_id, admission, placement)?;

    // The slot's cross-service endpoint map, known deterministically before
    // anything spawns: `${port:<serviceId>}` substitution addresses each service's
    // primary endpoint by service id.
    let slot_endpoints: nixfied_runtime::service::SlotEndpoints = plan
        .services
        .iter()
        .filter_map(|binding| {
            let service = admission
                .execution_model
                .services
                .get(binding.service_name.as_str())?;
            let primary_id = service.primary_endpoint.as_ref()?;
            let primary = service.endpoints.get(primary_id)?;
            let port = *binding.endpoint_ports.get(primary_id)?;
            Some((
                binding.service_name.clone(),
                nixfied_runtime::service::SelectedEndpoint {
                    endpoint_id: primary.endpoint_id.clone(),
                    host: primary.host,
                    port,
                },
            ))
        })
        .collect();

    // Start each required service on its planned port block, waiting readiness
    // then health before the next.
    let mut started: Vec<StartedService> = Vec::new();
    let mut lease: Option<RunLeaseHeartbeat> = None;
    let mut prepare_runs: Vec<TaskRun> = Vec::new();
    let mut task_runs: Vec<TaskRun> = Vec::new();
    let mut selected_task_run: Option<TaskRun> = None;
    let mut node_results: Vec<NodeResult> = Vec::new();
    let mut replay_ticket: Option<ReplayTicket> = None;
    macro_rules! finish_run {
        ($error:expr, $extra_services:expr) => {{
            task_runs.extend(prepare_runs.drain(..));
            let session = RunSession {
                placement,
                admission,
                options,
                redactor,
                cancellation,
                run_id,
                run_started,
                registry,
                started,
                extra_services: $extra_services,
                service_lifetime: plan.service_lifetime,
                direct_selected,
                lease,
                task_runs,
                selected_task_run,
                node_results,
                replay: replay_ticket
                    .map(ReplayPlan::Selected)
                    .unwrap_or(ReplayPlan::None),
                diagnostic_failures,
            };
            return session.finalize(Some($error));
        }};
    }
    let source_root = match admission.require_source() {
        Ok(source) => source.observed_root.clone(),
        Err(error) => finish_run!(error, Vec::new()),
    };
    for binding in &plan.services {
        let service_name = binding.service_name.as_str();
        if options.output_mode.emit_summary()
            && let Err(error) = write_diagnostic(
                options.output_mode,
                format_args!("  starting service {service_name}"),
            )
        {
            diagnostic_failures.push(error);
        }

        // prepare-as-task: the runner executes the prepare task's flattened
        // nodes inside the service reservation, resolving each leaf's
        // requirements against the services already started (the combined
        // connectsTo + prepare-requires ordering guarantees they are ready).
        let Some(service_def) = admission.execution_model.services.get(service_name) else {
            finish_run!(
                RuntimeError::new(
                    nixfied_runtime::ErrorCode::LifecycleFailed,
                    format!("admitted service {service_name} is missing"),
                ),
                Vec::new()
            );
        };
        let output_mode = options.output_mode;
        let prepare_runner: Option<PrepareRunner<'_>> =
            service_def.prepare.clone().map(|prepare_task| {
                let started_services = &started;
                let source_root = source_root.clone();
                let diagnostic_failures = &mut diagnostic_failures;
                Box::new(move |registry: &mut Registry| -> Result<Vec<TaskRun>, PrepareTaskError> {
                    let mut task_runs = Vec::new();
                    let nodes = match nixfied_runtime::execution::flatten_task(
                        &admission.execution_model,
                        &prepare_task,
                    ) {
                        Ok(nodes) => nodes,
                        Err(error) => {
                            return Err(PrepareTaskError::new(
                                post_admission_error(error),
                                task_runs,
                            ));
                        }
                    };
                    for node in nodes {
                        let Some(task) = admission.execution_model.tasks.get(node.task_id.as_str())
                        else {
                            return Err(PrepareTaskError::new(
                                RuntimeError::new(
                                    nixfied_runtime::ErrorCode::LifecycleFailed,
                                    format!("admitted task {} is missing", node.task_id),
                                ),
                                task_runs,
                            ));
                        };
                        let mut dependencies: Vec<&StartedService> = Vec::new();
                        for name in &task.requires {
                            let Some(dependency) = started_services
                                .iter()
                                .find(|service| service.service_name() == name.as_str())
                            else {
                                return Err(PrepareTaskError::new(
                                    RuntimeError::new(
                                        nixfied_runtime::ErrorCode::DependencyUnavailable,
                                        format!(
                                            "prepare node {} requires service {name} which is not started yet",
                                            node.node_id
                                        ),
                                    ),
                                    task_runs,
                                ));
                            };
                            dependencies.push(dependency);
                        }
                        if output_mode.emit_summary()
                            && let Err(error) = write_diagnostic(
                                output_mode,
                                format_args!(
                                    "  prepare node {} ({})",
                                    node.node_id, node.task_id
                                ),
                            )
                        {
                            diagnostic_failures.push(error);
                        }
                        let task_result = run_dependent_task_cancellable(
                            placement,
                            registry,
                            RunContext {
                                run_id,
                                computed_model_hash: &admission.computed_model_hash,
                                source_root: &source_root,
                                state_root: &placement.state_root,
                                secrets: &admission.secrets,
                                redactor,
                            },
                            &dependencies,
                            node.node_id.as_str(),
                            task,
                            cancellation,
                            EvidenceMode::CaptureOnly,
                        );
                        match task_result {
                            Ok(TaskExecution::Succeeded(evidence)) => {
                                let (task_run, _) = evidence.into_task_and_replay();
                                task_runs.push(task_run);
                            }
                            Ok(TaskExecution::Failed { error, evidence }) => {
                                let (task_run, _) = evidence.into_task_and_replay();
                                task_runs.push(task_run.clone());
                                return Err(PrepareTaskError::new(
                                    attach_task_evidence(error, &task_run),
                                    task_runs,
                                ));
                            }
                            Err(TaskExecutionError::BeforeTerminal(error)) => {
                                return Err(PrepareTaskError::new(*error, task_runs));
                            }
                            Err(TaskExecutionError::AfterTerminal { error, evidence }) => {
                                let error = *error;
                                let evidence = *evidence;
                                let (task_run, _) = evidence.into_task_and_replay();
                                task_runs.push(task_run.clone());
                                return Err(PrepareTaskError::new(
                                    attach_task_evidence(error, &task_run),
                                    task_runs,
                                ));
                            }
                        }
                    }
                    Ok(task_runs)
                }) as PrepareRunner<'_>
            });

        let mut current_service = match start_service_for_slot(
            admission,
            placement,
            &mut registry,
            run_id,
            selected_slot,
            ServiceSelection {
                service_name,
                service_lifetime: plan.service_lifetime,
                endpoint_ports: &binding.endpoint_ports,
                slot_endpoints: &slot_endpoints,
                run_timeout_ms: options.timeout_ms,
                cancellation,
                prepare_runner,
            },
        ) {
            Ok(service) => service,
            Err(start_error) => {
                let (error, evidence) = start_error.into_parts();
                task_runs.extend(evidence);
                finish_run!(error.with_detail("failedService", service_name), Vec::new());
            }
        };
        if lease.is_none() {
            lease = Some(RunLeaseHeartbeat::start(
                placement.registry_path().to_path_buf(),
                registry.identity().clone(),
                current_service.run_id.clone(),
                current_service.owner_token.clone(),
            ));
        }
        let startup_result = current_service
            .wait_for_probe_ready_cancellable(&mut registry, cancellation)
            .and_then(|()| current_service.check_health_cancellable(&mut registry, cancellation));
        if let Err(error) = startup_result {
            let failed_service_output = ServiceRunOutput {
                service_id: current_service.service_name().to_string(),
                service_instance_id: current_service.service_instance_id.clone(),
                process_key: current_service.process_key.clone(),
                selected_endpoint: current_service.selected_endpoint().cloned(),
            };
            let error =
                current_service.finalize_failed_start(&mut registry, options.timeout_ms, error);
            finish_run!(
                error.with_detail("failedService", service_name),
                vec![failed_service_output]
            );
        }
        if options.output_mode.emit_summary() {
            match current_service.selected_endpoint() {
                Some(endpoint) => {
                    if let Err(error) = write_diagnostic(
                        options.output_mode,
                        format_args!(
                            "  service {} ready at {}:{}",
                            current_service.service_name(),
                            endpoint.host,
                            endpoint.port
                        ),
                    ) {
                        diagnostic_failures.push(error);
                    }
                }
                None => {
                    if let Err(error) = write_diagnostic(
                        options.output_mode,
                        format_args!(
                            "  service {} ready (endpoint-less)",
                            current_service.service_name()
                        ),
                    ) {
                        diagnostic_failures.push(error);
                    }
                }
            }
        }
        started.push(current_service);
    }

    task_runs.append(&mut prepare_runs);
    if let Err(error) = cancellation.check() {
        finish_run!(error, Vec::new());
    }

    // Run each flattened node in dependency order, gating each leaf on the
    // readiness of its declared service requirements (the first provides
    // ${port}/${host} substitution). The plan's order already honors the
    // composite's step dependencies.
    for node in &plan.nodes {
        let task_id = &node.task_id;
        let Some(task) = admission.execution_model.tasks.get(task_id.as_str()) else {
            finish_run!(
                RuntimeError::new(
                    nixfied_runtime::ErrorCode::LifecycleFailed,
                    format!("admitted task {task_id} is missing"),
                ),
                Vec::new()
            );
        };
        // Resolve every service this task depends on to its started instance (the
        // first is the primary, providing ${port}/${host}). A task may declare
        // zero services — it runs in the run context alone.
        let mut dep_indices = Vec::new();
        let mut missing_dependency = None;
        for name in &task.requires {
            match started
                .iter()
                .position(|service| service.service_name() == name.as_str())
            {
                Some(index) => dep_indices.push(index),
                None => {
                    missing_dependency = Some(name.clone());
                    break;
                }
            }
        }
        if let Some(name) = missing_dependency {
            let error = RuntimeError::new(
                nixfied_runtime::ErrorCode::DependencyUnavailable,
                format!("task {task_id} depends on service {name} which was not started"),
            )
            .with_detail("failedNodeId", node.node_id.as_str());
            finish_run!(error, Vec::new());
        }
        let dependencies: Vec<&StartedService> =
            dep_indices.iter().map(|&index| &started[index]).collect();
        let run_context = RunContext {
            run_id,
            computed_model_hash: &admission.computed_model_hash,
            source_root: &source_root,
            state_root: &placement.state_root,
            secrets: &admission.secrets,
            redactor,
        };
        let task_result = run_dependent_task_cancellable(
            placement,
            &mut registry,
            run_context,
            &dependencies,
            node.node_id.as_str(),
            task,
            cancellation,
            if options.output_mode.is_task_output() {
                EvidenceMode::ReplaySelected
            } else {
                EvidenceMode::CaptureOnly
            },
        );
        match task_result {
            Ok(TaskExecution::Succeeded(evidence)) => {
                let (task_run, ticket) = evidence.into_task_and_replay();
                replay_ticket = ticket.or(replay_ticket);
                if direct_selected {
                    selected_task_run = Some(task_run.clone());
                }
                if options.output_mode.emit_summary()
                    && let Err(error) = write_diagnostic(
                        options.output_mode,
                        format_args!(
                            "  ok {} ({task_id}) {}",
                            node.node_id,
                            human_duration(task_run.duration_ms)
                        ),
                    )
                {
                    diagnostic_failures.push(error);
                }
                node_results.push(NodeResult {
                    node_id: node.node_id.as_str().to_string(),
                    task_id: task_id.as_str().to_string(),
                    success: task_run.success,
                    exit_code: task_run.exit_code,
                    duration_ms: task_run.duration_ms,
                    stdout_path: task_run.stdout_path.clone(),
                    stderr_path: task_run.stderr_path.clone(),
                    summary_path: task_run.summary_path.clone(),
                });
                task_runs.push(task_run);
            }
            Ok(TaskExecution::Failed { error, evidence }) => {
                let (task_run, ticket) = evidence.into_task_and_replay();
                replay_ticket = ticket.or(replay_ticket);
                if direct_selected {
                    selected_task_run = Some(task_run.clone());
                }
                let error = task_failure_with_evidence(
                    error,
                    node.node_id.as_str(),
                    task_id.as_str(),
                    &task_run,
                    options.output_mode,
                    &mut node_results,
                    &mut task_runs,
                    &mut diagnostic_failures,
                );
                let error = error.with_detail("failedNodeId", node.node_id.as_str());
                finish_run!(error, Vec::new());
            }
            Err(TaskExecutionError::BeforeTerminal(error)) => {
                let error = (*error).with_detail("failedNodeId", node.node_id.as_str());
                finish_run!(error, Vec::new());
            }
            Err(TaskExecutionError::AfterTerminal { error, evidence }) => {
                let error = *error;
                let evidence = *evidence;
                let (task_run, ticket) = evidence.into_task_and_replay();
                replay_ticket = ticket.or(replay_ticket);
                if direct_selected {
                    selected_task_run = Some(task_run.clone());
                }
                let error = task_failure_with_evidence(
                    error,
                    node.node_id.as_str(),
                    task_id.as_str(),
                    &task_run,
                    options.output_mode,
                    &mut node_results,
                    &mut task_runs,
                    &mut diagnostic_failures,
                )
                .with_detail("failedNodeId", node.node_id.as_str());
                finish_run!(error, Vec::new());
            }
        }
    }

    if cancellation.is_canceled() {
        finish_run!(nixfied_runtime::cancellation::canceled_error(), Vec::new());
    }
    let session = RunSession {
        placement,
        admission,
        options,
        redactor,
        cancellation,
        run_id,
        run_started,
        registry,
        started,
        extra_services: Vec::new(),
        service_lifetime: plan.service_lifetime,
        direct_selected,
        lease,
        task_runs,
        selected_task_run,
        node_results,
        replay: replay_ticket
            .map(ReplayPlan::Selected)
            .unwrap_or(ReplayPlan::None),
        diagnostic_failures,
    };
    session.finalize(None)
}

fn services_output(started: &[StartedService]) -> Vec<ServiceRunOutput> {
    started
        .iter()
        .map(|service| ServiceRunOutput {
            service_id: service.service_name().to_string(),
            service_instance_id: service.service_instance_id.clone(),
            process_key: service.process_key.clone(),
            selected_endpoint: service.selected_endpoint().cloned(),
        })
        .collect()
}

fn write_diagnostic(output_mode: RunOutputMode, line: impl Display) -> Result<(), RuntimeError> {
    if !output_mode.emit_summary() {
        return Ok(());
    }
    writeln!(io::stderr().lock(), "{line}")
        .map_err(|error| output_projection_io_error("stderr", "write", "<stderr>", error))
}

fn write_stdout_line(line: &str) -> Result<(), RuntimeError> {
    writeln!(io::stdout().lock(), "{line}")
        .map_err(|error| output_projection_io_error("stdout", "write", "<stdout>", error))
}

fn output_projection_io_error(
    stream: &str,
    operation: &str,
    path: &str,
    error: io::Error,
) -> RuntimeError {
    let kind = match error.kind() {
        io::ErrorKind::BrokenPipe => "broken-pipe",
        io::ErrorKind::PermissionDenied => "permission-denied",
        io::ErrorKind::Interrupted => "interrupted",
        _ => "io",
    };
    RuntimeError::new(
        nixfied_runtime::ErrorCode::OutputProjectionFailed,
        "runtime output projection failed",
    )
    .with_detail(
        "projections",
        vec![json!({
            "stream": stream,
            "operation": operation,
            "kind": kind,
            "path": path,
            "bytesWritten": 0,
        })],
    )
}

#[allow(clippy::too_many_arguments)]
fn task_failure_with_evidence(
    error: RuntimeError,
    node_id: &str,
    task_id: &str,
    task_run: &TaskRun,
    output_mode: RunOutputMode,
    node_results: &mut Vec<NodeResult>,
    task_runs: &mut Vec<TaskRun>,
    diagnostic_failures: &mut Vec<RuntimeError>,
) -> RuntimeError {
    if output_mode.emit_summary() {
        if let Err(error) = write_diagnostic(
            output_mode,
            format_args!(
                "  fail {node_id} ({task_id}) {} exit={}",
                human_duration(task_run.duration_ms),
                human_exit_code(task_run.exit_code)
            ),
        ) {
            diagnostic_failures.push(error);
        }
        if let Err(error) = write_diagnostic(
            output_mode,
            format_args!("    stderr: {}", human_path(&task_run.stderr_path)),
        ) {
            diagnostic_failures.push(error);
        }
    }
    node_results.push(NodeResult {
        node_id: node_id.to_string(),
        task_id: task_id.to_string(),
        success: task_run.success,
        exit_code: task_run.exit_code,
        duration_ms: task_run.duration_ms,
        stdout_path: task_run.stdout_path.clone(),
        stderr_path: task_run.stderr_path.clone(),
        summary_path: task_run.summary_path.clone(),
    });
    task_runs.push(task_run.clone());
    attach_task_evidence(error, task_run)
}

fn attach_task_evidence(mut error: RuntimeError, task_run: &TaskRun) -> RuntimeError {
    error = error
        .with_detail("taskRun", task_run)
        .with_detail("stdoutPath", &task_run.stdout_path)
        .with_detail("stderrPath", &task_run.stderr_path)
        .with_detail("summaryPath", &task_run.summary_path);
    error
}

fn with_failure_summary(error: RuntimeError, summary_path: Option<PathBuf>) -> RuntimeError {
    match summary_path {
        Some(path) => error.with_detail("runSummaryPath", &path),
        None => error,
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn human_duration(duration_ms: u64) -> String {
    if duration_ms < 1000 {
        format!("{duration_ms}ms")
    } else {
        format!("{:.2}s", duration_ms as f64 / 1000.0)
    }
}

fn human_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('{', "(")
        .replace('}', ")")
}

fn human_exit_code(exit_code: Option<i32>) -> String {
    exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn print_run_footer(
    output_mode: RunOutputMode,
    run_succeeded: bool,
    nodes: &[NodeResult],
    duration_ms: u64,
    run_summary_path: Option<&Path>,
    logs_dir: &Path,
) -> Result<(), RuntimeError> {
    if !output_mode.emit_summary() {
        return Ok(());
    }
    if run_succeeded {
        write_diagnostic(
            output_mode,
            format_args!(
                "  result: ok {} passed, 0 failed in {}",
                nodes.iter().filter(|node| node.success).count(),
                human_duration(duration_ms)
            ),
        )?;
    } else if nodes.is_empty() {
        write_diagnostic(
            output_mode,
            format_args!("  result: fail in {}", human_duration(duration_ms)),
        )?;
    } else {
        write_diagnostic(
            output_mode,
            format_args!(
                "  result: fail {} passed, {} failed in {}",
                nodes.iter().filter(|node| node.success).count(),
                nodes.iter().filter(|node| !node.success).count(),
                human_duration(duration_ms)
            ),
        )?;
    }
    if let Some(path) = run_summary_path {
        write_diagnostic(
            output_mode,
            format_args!("  run-summary: {}", human_path(path)),
        )?;
    }
    write_diagnostic(
        output_mode,
        format_args!("  logs: {}", human_path(logs_dir)),
    )
}

struct RunSummary<'a> {
    placement: &'a nixfied_runtime::state::HostPlacement,
    run_id: &'a str,
    run_succeeded: bool,
    duration_ms: u64,
    nodes: &'a [NodeResult],
    services: &'a [ServiceRunOutput],
    tasks: &'a [TaskRun],
    redactor: &'a Redactor,
}

/// Write the aggregate run summary: the run id, overall success, the services
/// started (with their resolved endpoints), and the per-node and per-task
/// results — a complete, inspectable record of the run.
fn write_run_summary(input: RunSummary<'_>) -> Result<PathBuf, RuntimeError> {
    let path = input.placement.artifacts_dir.join("run-summary.json");
    let mut summary = serde_json::json!({
        "runId": input.run_id,
        "success": input.run_succeeded && input.nodes.iter().all(|node| node.success),
        "durationMs": input.duration_ms,
        "services": input.services,
        "nodes": input.nodes,
        "tasks": input.tasks,
    });
    input.redactor.redact_value(&mut summary);
    let bytes = serde_json::to_vec_pretty(&summary).map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::LifecycleFailed,
            error.to_string(),
        )
    })?;
    std::fs::write(&path, bytes).map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::StateUnwritable,
            format!("failed to write run summary {}: {error}", path.display()),
        )
    })?;
    Ok(path)
}

#[derive(Debug, Clone, Copy)]
enum ControlCommand {
    Ps,
    Down,
    Clean,
}

struct ControlOptions {
    model_path: PathBuf,
    allow_non_store: bool,
    state_base: PathBuf,
    timeout_ms: u64,
    selection: RuntimeSelection,
    cleanup_mode: nixfied_runtime::state::CleanupMode,
}

struct RunOptions {
    model_path: PathBuf,
    allow_non_store: bool,
    state_base: PathBuf,
    timeout_ms: u64,
    output_mode: RunOutputMode,
    selection: RuntimeSelection,
    task: Option<String>,
}

const RUN_HELP: &str = "\
Run one declared task and its required services.

Usage:
  nix run .#run -- --task <id> [options]
  nix run .#<verb> -- [options]

Options:
  --task <id>             Select a declared task (the exported verb preselects it)
  --slot <number>         Select a declared project slot
  --timeout-ms <number>   Set the runtime operation timeout in milliseconds
  --output <mode>         Select summary, json, both, or task-output (default: summary)
                          task-output requires one directly selected leaf and replays redacted output
  --summary               Emit the human summary
  --json                  Emit structured JSON
  --both                  Emit both projections
  -h, --help              Show this help";

const PS_HELP: &str = "\
Reconcile and report Nixfied-owned processes for a slot.

Usage:
  nix run .#ps -- [options]

Options:
  --slot <number>  Select a declared project slot
  -h, --help       Show this help";

const DOWN_HELP: &str = "\
Stop Nixfied-owned process groups for a slot.

Usage:
  nix run .#down -- [options]

Options:
  --slot <number>        Select a declared project slot
  --timeout-ms <number>  Set the stop timeout in milliseconds
  -h, --help             Show this help";

const CLEAN_HELP: &str = "\
Safely clean Nixfied-owned state for a slot.

Usage:
  nix run .#clean -- [options]

Options:
  --slot <number>  Select a declared project slot
  --purge          Relax only the protected/persistent cleanup policy gate
  -h, --help       Show this help";

fn print_help_if_requested(args: &[String], help: &str) -> bool {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        println!("{help}");
        true
    } else {
        false
    }
}

fn control_help(command: ControlCommand) -> &'static str {
    match command {
        ControlCommand::Ps => PS_HELP,
        ControlCommand::Down => DOWN_HELP,
        ControlCommand::Clean => CLEAN_HELP,
    }
}

/// The refusal for a missing or unknown task selection: name every declared
/// task so the operator can pick one.
fn selection_required_error(model: &nixfied_runtime::execution::ExecutionModel) -> RuntimeError {
    let declared: Vec<&str> = model
        .tasks
        .keys()
        .chain(model.composites.keys())
        .map(|task| task.as_str())
        .collect();
    RuntimeError::new(
        nixfied_runtime::ErrorCode::TaskSelectionInvalid,
        format!(
            "run requires --task <id>; declared tasks: {}",
            declared.join(", ")
        ),
    )
    .with_detail("declaredTasks", &declared)
}

fn validate_run_selection(
    model: &nixfied_runtime::execution::ExecutionModel,
    output_mode: RunOutputMode,
    task: Option<&str>,
) -> Result<(), RuntimeError> {
    let Some(task) = task else {
        return Err(selection_required_error(model));
    };
    let task_id = nixfied_model::TaskId::new(task);
    if model.tasks.contains_key(&task_id) {
        return validate_selection_nodes(model, &task_id, task);
    }
    if let Some(composite) = model.composites.get(&task_id) {
        if output_mode.is_task_output() {
            return Err(RuntimeError::new(
                nixfied_runtime::ErrorCode::TaskSelectionInvalid,
                format!("task-output requires a directly selected leaf; {task} is composite"),
            )
            .with_detail("task", task)
            .with_detail("compositeSteps", composite.steps.len()));
        }
        return validate_selection_nodes(model, &task_id, task);
    }
    Err(selection_required_error(model).with_detail("unknownTask", task))
}

fn validate_selection_nodes(
    model: &nixfied_runtime::execution::ExecutionModel,
    task_id: &nixfied_model::TaskId,
    task: &str,
) -> Result<(), RuntimeError> {
    let nodes =
        nixfied_runtime::execution::flatten_task(model, task_id).map_err(post_admission_error)?;
    if nodes.is_empty() {
        return Err(RuntimeError::new(
            nixfied_runtime::ErrorCode::TaskSelectionInvalid,
            format!("task {task} has no executable nodes"),
        )
        .with_detail("task", task));
    }
    Ok(())
}

fn parse_run_output_mode(value: &str) -> Result<RunOutputMode, RuntimeError> {
    match value {
        "summary" => Ok(RunOutputMode::Summary),
        "json" => Ok(RunOutputMode::Json),
        "both" => Ok(RunOutputMode::Both),
        "task-output" => Ok(RunOutputMode::TaskOutput),
        other => Err(RuntimeError::new(
            nixfied_runtime::ErrorCode::OutputModeInvalid,
            format!("invalid --output value {other}: expected summary, json, both, or task-output"),
        )),
    }
}

fn run_control(command: ControlCommand, args: &[String]) -> Result<(), RuntimeError> {
    if print_help_if_requested(args, control_help(command)) {
        return Ok(());
    }
    let options = parse_control_options(command, args)?;
    let (loaded, admission) =
        load_admitted_model_for_control(options.model_path.clone(), options.allow_non_store)?;
    let model_path = admission.model_path.clone();
    let computed_model_hash = admission.computed_model_hash.clone();
    run_control_admitted(command, &loaded.model, &admission, &options)
        .map_err(|error| error.with_model_if_missing(model_path, computed_model_hash))
}

fn run_control_admitted(
    command: ControlCommand,
    model: &nixfied_model::Model,
    admission: &Admission,
    options: &ControlOptions,
) -> Result<(), RuntimeError> {
    let selected_slot = select_slot(model, options.selection.slot).map_err(post_admission_error)?;
    let placement =
        derive_host_placement_for_slot(model, &selected_slot, "control", &options.state_base)
            .map_err(post_admission_error)?;
    let result = (|| {
        let mut registry = Registry::open_or_create(
            placement.registry_path(),
            &RegistryIdentity::for_slot(
                &model.project.project_id,
                selected_slot.environment,
                selected_slot.slot,
                &model.runtime_abi,
                &model.toolchain_id,
            ),
        )?;
        match command {
            ControlCommand::Ps => print_json(&nixfied_runtime::control::ps(&mut registry)?),
            ControlCommand::Down => {
                print_json(&nixfied_runtime::control::down_owned_process_groups(
                    &mut registry,
                    options.timeout_ms,
                )?)
            }
            ControlCommand::Clean => print_json(&run_slot_clean(
                model,
                admission,
                &placement,
                &mut registry,
                &selected_slot,
                options.cleanup_mode,
            )?),
        }
    })();
    result.map_err(|error| enrich_placed_error(error, &placement, &selected_slot))
}

fn parse_run_options(args: &[String]) -> Result<RunOptions, RuntimeError> {
    let mut model_path = None;
    let mut allow_non_store = false;
    let mut state_base = None;
    let mut timeout_ms = 5000;
    let mut output_mode = RunOutputMode::Summary;
    let mut saw_task_output = false;
    let mut saw_metadata_output = false;
    let mut slot = None;
    let mut task = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--model" => {
                index += 1;
                model_path = args.get(index).map(PathBuf::from);
            }
            "--allow-non-store-model" => {
                allow_non_store = true;
            }
            "--state-base" => {
                index += 1;
                state_base = args.get(index).map(PathBuf::from);
            }
            "--slot" => {
                index += 1;
                slot = Some(parse_slot_arg(args.get(index), "--slot")?);
            }
            "--task" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| {
                        RuntimeError::new(
                            nixfied_runtime::ErrorCode::TaskSelectionInvalid,
                            "missing --task value",
                        )
                    })?
                    .clone();
                if task.replace(value).is_some() {
                    return Err(RuntimeError::new(
                        nixfied_runtime::ErrorCode::TaskSelectionInvalid,
                        "--task may be specified only once",
                    ));
                }
            }
            "--timeout-ms" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::ModelAdmission,
                        "missing --timeout-ms value",
                    )
                })?;
                timeout_ms = value.parse::<u64>().map_err(|error| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::ModelAdmission,
                        format!("invalid --timeout-ms value {value}: {error}"),
                    )
                })?;
            }
            "--output" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::OutputModeInvalid,
                        "missing --output value",
                    )
                })?;
                let parsed = parse_run_output_mode(value)?;
                if parsed.is_task_output() {
                    if saw_metadata_output {
                        return Err(RuntimeError::new(
                            nixfied_runtime::ErrorCode::OutputModeConflict,
                            "task-output cannot be combined with a metadata output mode",
                        ));
                    }
                    saw_task_output = true;
                } else {
                    if saw_task_output {
                        return Err(RuntimeError::new(
                            nixfied_runtime::ErrorCode::OutputModeConflict,
                            "task-output cannot be combined with a metadata output mode",
                        ));
                    }
                    saw_metadata_output = true;
                }
                output_mode = parsed;
            }
            "--summary" => {
                if saw_task_output {
                    return Err(RuntimeError::new(
                        nixfied_runtime::ErrorCode::OutputModeConflict,
                        "task-output cannot be combined with a metadata output mode",
                    ));
                }
                saw_metadata_output = true;
                output_mode = RunOutputMode::Summary;
            }
            "--json" => {
                if saw_task_output {
                    return Err(RuntimeError::new(
                        nixfied_runtime::ErrorCode::OutputModeConflict,
                        "task-output cannot be combined with a metadata output mode",
                    ));
                }
                saw_metadata_output = true;
                output_mode = RunOutputMode::Json;
            }
            "--both" => {
                if saw_task_output {
                    return Err(RuntimeError::new(
                        nixfied_runtime::ErrorCode::OutputModeConflict,
                        "task-output cannot be combined with a metadata output mode",
                    ));
                }
                saw_metadata_output = true;
                output_mode = RunOutputMode::Both;
            }
            "--task-output" => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::OutputModeInvalid,
                    "unknown output flag --task-output; use --output task-output",
                ));
            }
            other => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::ModelAdmission,
                    format!("unknown run argument: {other}"),
                ));
            }
        }
        index += 1;
    }
    let model_path = model_path.ok_or_else(|| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            "missing --model path",
        )
    })?;
    let state_base = state_base.map(Ok).unwrap_or_else(state_base_from_env)?;
    Ok(RunOptions {
        model_path,
        allow_non_store,
        state_base,
        timeout_ms,
        output_mode,
        selection: RuntimeSelection { slot },
        task,
    })
}

fn parse_control_options(
    command: ControlCommand,
    args: &[String],
) -> Result<ControlOptions, RuntimeError> {
    let mut model_path = None;
    let mut allow_non_store = false;
    let mut state_base = None;
    let mut timeout_ms = 5000;
    let mut slot = None;
    let mut cleanup_mode = nixfied_runtime::state::CleanupMode::Standard;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--model" => {
                index += 1;
                model_path = args.get(index).map(PathBuf::from);
            }
            "--allow-non-store-model" => {
                allow_non_store = true;
            }
            "--state-base" => {
                index += 1;
                state_base = args.get(index).map(PathBuf::from);
            }
            "--slot" => {
                index += 1;
                slot = Some(parse_slot_arg(args.get(index), "--slot")?);
            }
            "--purge" if matches!(command, ControlCommand::Clean) => {
                cleanup_mode = nixfied_runtime::state::CleanupMode::Purge;
            }
            "--timeout-ms" if matches!(command, ControlCommand::Down) => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::ModelAdmission,
                        "missing --timeout-ms value",
                    )
                })?;
                timeout_ms = value.parse::<u64>().map_err(|error| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::ModelAdmission,
                        format!("invalid --timeout-ms value {value}: {error}"),
                    )
                })?;
            }
            other => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::ModelAdmission,
                    format!("unknown control argument: {other}"),
                ));
            }
        }
        index += 1;
    }
    let model_path = model_path.ok_or_else(|| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            "missing --model path",
        )
    })?;
    let state_base = state_base.map(Ok).unwrap_or_else(state_base_from_env)?;
    Ok(ControlOptions {
        model_path,
        allow_non_store,
        state_base,
        timeout_ms,
        selection: RuntimeSelection { slot },
        cleanup_mode,
    })
}

fn parse_slot_arg(value: Option<&String>, flag: &str) -> Result<u32, RuntimeError> {
    let value = value.ok_or_else(|| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            format!("missing {flag} value"),
        )
    })?;
    value.parse::<u32>().map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ModelAdmission,
            format!("invalid {flag} value {value}: {error}"),
        )
    })
}

fn new_run_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("run-{}-{now}", std::process::id())
}

fn load_admitted_model(
    model_path: PathBuf,
    allow_non_store: bool,
) -> Result<(nixfied_runtime::model_loader::LoadedModel, Admission), RuntimeError> {
    load_model_admitted(model_path, allow_non_store, true)
}

/// Load and admit a model for a recovery/control command (`ps`/`down`/`clean`)
/// without resolving the live workspace, so control can reconcile, stop, and
/// clean a slot from the store model and registry even when run outside the
/// project root or after the workspace has moved or been deleted.
fn load_admitted_model_for_control(
    model_path: PathBuf,
    allow_non_store: bool,
) -> Result<(nixfied_runtime::model_loader::LoadedModel, Admission), RuntimeError> {
    load_model_admitted(model_path, allow_non_store, false)
}

fn load_model_admitted(
    model_path: PathBuf,
    allow_non_store: bool,
    resolve_source: bool,
) -> Result<(nixfied_runtime::model_loader::LoadedModel, Admission), RuntimeError> {
    let policy = if allow_non_store {
        StoreOriginPolicy::AllowNonStoreForTests
    } else {
        StoreOriginPolicy::RequireStore
    };
    let context = AdmissionContext::current(policy);
    let raw_model = read_raw_model(&model_path)?;
    nixfied_runtime::admission::origin::check_raw_store_origin(&raw_model, &context)?;
    let loaded = parse_loaded_model(raw_model)?;
    let admission = if resolve_source {
        Admission::check(&loaded, &context)?
    } else {
        Admission::check_for_control(&loaded, &context)?
    };
    warn_on_ephemeral_port_overlap(&loaded.model)?;
    Ok((loaded, admission))
}

/// Warn (stderr, non-fatal) when a slot's candidate port window overlaps the
/// host's ephemeral port range: the kernel hands out ports in that range to
/// any process, so a deterministic window inside it can collide with unrelated
/// ephemeral allocations. The range is host state only Linux exposes a stable
/// path for; elsewhere the check is silently skipped.
fn warn_on_ephemeral_port_overlap(model: &nixfied_model::Model) -> Result<(), RuntimeError> {
    let Some((low, high)) = host_ephemeral_port_range() else {
        return Ok(());
    };
    for placement in model.placement.slot_placements.values() {
        let window = &placement.candidate_ports;
        if u32::from(window.start) <= high && u32::from(window.end) >= low {
            write_stderr_line(format_args!(
                "warning: slot {} candidate port window {}-{} overlaps the host ephemeral port range {low}-{high}; deterministic ports may collide with ephemeral allocations (set nixfied.placement.ports.base outside the range)",
                placement.slot, window.start, window.end
            ))?;
        }
    }
    Ok(())
}

fn host_ephemeral_port_range() -> Option<(u32, u32)> {
    let contents = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range").ok()?;
    let mut parts = contents.split_whitespace();
    let low = parts.next()?.parse().ok()?;
    let high = parts.next()?.parse().ok()?;
    (low <= high).then_some((low, high))
}

fn print_json(value: &impl Serialize) -> Result<(), RuntimeError> {
    print_json_redacted(value, &Redactor::empty())
}

fn error_code_wire(error: &RuntimeError) -> String {
    error_code_wire_value(error.code)
}

fn error_code_wire_value(code: nixfied_runtime::ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{code:?}"))
}

fn print_json_redacted(value: &impl Serialize, redactor: &Redactor) -> Result<(), RuntimeError> {
    let mut value = serde_json::to_value(value).map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::LifecycleFailed,
            error.to_string(),
        )
    })?;
    redactor.redact_value(&mut value);
    let rendered = serde_json::to_string_pretty(&value).map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::LifecycleFailed,
            error.to_string(),
        )
    })?;
    write_stdout_line(&rendered)
}

fn exit_code(error: &RuntimeError) -> i32 {
    match error.code {
        nixfied_runtime::ErrorCode::RuntimeAbiMismatch => 12,
        nixfied_runtime::ErrorCode::ModelNotStoreOutput => 13,
        nixfied_runtime::ErrorCode::ModelInvalid => 14,
        nixfied_runtime::ErrorCode::PlatformUnsupported => 15,
        nixfied_runtime::ErrorCode::ClosureMissing => 16,
        nixfied_runtime::ErrorCode::SourceMismatch => 17,
        nixfied_runtime::ErrorCode::ModelAdmission => 18,
        nixfied_runtime::ErrorCode::RegistryCorrupt => 19,
        nixfied_runtime::ErrorCode::StateUnwritable => 20,
        nixfied_runtime::ErrorCode::StateUnowned => 21,
        nixfied_runtime::ErrorCode::CleanupRefused => 22,
        nixfied_runtime::ErrorCode::PortConflict => 23,
        nixfied_runtime::ErrorCode::PortUnverifiable => 24,
        nixfied_runtime::ErrorCode::ProcEscape => 25,
        nixfied_runtime::ErrorCode::ReadinessTimeout => 26,
        nixfied_runtime::ErrorCode::Canceled => 27,
        nixfied_runtime::ErrorCode::LeaseStale => 28,
        nixfied_runtime::ErrorCode::LeaseConflict => 29,
        nixfied_runtime::ErrorCode::TaskFailed => 30,
        nixfied_runtime::ErrorCode::LifecycleFailed => 31,
        nixfied_runtime::ErrorCode::DependencyUnavailable => 32,
        nixfied_runtime::ErrorCode::SecretUnavailable => 33,
        nixfied_runtime::ErrorCode::SecretLeakBlocked => 34,
        nixfied_runtime::ErrorCode::OutputModeInvalid => 35,
        nixfied_runtime::ErrorCode::OutputModeConflict => 36,
        nixfied_runtime::ErrorCode::TaskSelectionInvalid => 37,
        nixfied_runtime::ErrorCode::OutputProjectionFailed => 38,
    }
}
