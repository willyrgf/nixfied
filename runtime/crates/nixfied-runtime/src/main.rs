use std::fmt::Display;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use nixfied_manifest::{ServiceLifetime, TaskDefaultOutput};
use nixfied_runtime::cancellation::{CancellationToken, ProcessSignalGuard};
use nixfied_runtime::error::{RuntimeCause, error_code_wire};
use nixfied_runtime::execution::{PlanNode, plan};
use nixfied_runtime::output::{
    EvidenceMode, OutputStream, ProjectionOperation, ReplaySinks, ReplayTicket,
    output_projection_io_error,
};
use nixfied_runtime::redaction::Redactor;
use nixfied_runtime::registry::{Registry, RegistryIdentity, RunLeaseHeartbeat};
use nixfied_runtime::service::{
    PrepareRunner, RunContext, SelectedEndpoint, ServiceSelection, StartedService, TaskExecution,
    TaskExecutionError, TaskRun, mark_run_completed, mark_run_failed, record_run_created,
    run_dependent_task_cancellable, run_slot_clean, start_service_for_slot,
};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{
    StateIdentity, derive_host_placement_for_slot, materialize_registry_root, prepare_slot_state,
    state_base_from_env,
};
use nixfied_runtime::{
    AdmissionContext, ControlAdmission, RunAdmission, RuntimeError, StoreOriginPolicy,
};
use serde::Serialize;
use serde_json::Value;

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckOutput {
    manifest_path: PathBuf,
    computed_manifest_hash: String,
    raw_len: usize,
    project_id: String,
    runtime_abi: String,
    toolchain_id: String,
    target_system: String,
    environment: String,
    slot: u32,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceRunOutput {
    service_id: String,
    service_instance_id: String,
    process_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_endpoint: Option<SelectedEndpoint>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeResult {
    node_id: String,
    task_id: String,
    success: bool,
    exit_code: Option<i32>,
    duration_ms: u64,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    summary_path: PathBuf,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RunOutput {
    run_id: String,
    manifest_path: PathBuf,
    computed_manifest_hash: String,
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RunSummaryOutput<'a> {
    run_id: &'a str,
    success: bool,
    duration_ms: u64,
    services: &'a [ServiceRunOutput],
    nodes: &'a [NodeResult],
    tasks: &'a [TaskRun],
}

include!("generated/main.rs");
include!("generated/commands.rs");

struct RunSession<'a> {
    placement: &'a nixfied_runtime::state::HostPlacement,
    admission: &'a RunAdmission,
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
    evidence: RunEvidence,
    diagnostic_failures: Vec<RuntimeError>,
}

struct FailureAccumulator {
    primary: Option<RuntimeError>,
}

impl FailureAccumulator {
    fn new() -> Self {
        Self { primary: None }
    }

    fn is_empty(&self) -> bool {
        self.primary.is_none()
    }

    fn push(&mut self, error: RuntimeError) {
        let Some(mut primary) = self.primary.take() else {
            self.primary = Some(error);
            return;
        };
        let mut error = error;
        if failure_priority(error.code) > failure_priority(primary.code) {
            error.causes.extend(primary.causes.drain(..));
            error.causes.push(RuntimeCause::from_error(primary));
            self.primary = Some(error);
        } else {
            primary.causes.extend(error.causes.drain(..));
            primary.causes.push(RuntimeCause::from_error(error));
            self.primary = Some(primary);
        }
    }

    fn finish(self, output: RunOutput) -> Result<RunOutput, RuntimeError> {
        let Some(primary) = self.primary else {
            return Ok(output);
        };
        Err(primary)
    }
}

fn record_cancellation_once(
    cancellation: &CancellationToken,
    recorded: &mut bool,
    failures: &mut FailureAccumulator,
) {
    if !*recorded && cancellation.is_canceled() {
        failures.push(nixfied_runtime::cancellation::canceled_error());
        *recorded = true;
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
        let mut cancellation_recorded = initial_error
            .as_ref()
            .is_some_and(|error| error.code == nixfied_runtime::ErrorCode::Canceled);
        let mut failures = FailureAccumulator::new();
        if let Some(error) = initial_error {
            failures.push(error);
        }
        if !had_initial_outcome {
            record_cancellation_once(self.cancellation, &mut cancellation_recorded, &mut failures);
        }
        for error in self.diagnostic_failures.drain(..) {
            failures.push(error);
        }

        if let Some(ticket) = self.evidence.replay.take()
            && let Some(error) = ticket.replay(ReplaySinks::stdio()).into_error()
        {
            failures.push(error);
        }
        record_cancellation_once(self.cancellation, &mut cancellation_recorded, &mut failures);

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
        while let Some(service) = self.started.pop() {
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
                service.stand(&mut self.registry, self.options.timeout_ms)
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
        record_cancellation_once(self.cancellation, &mut cancellation_recorded, &mut failures);
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
        let node_results = self.evidence.nodes();
        let run_summary_path = match write_run_summary(RunSummary {
            placement: self.placement,
            run_id: self.run_id,
            run_succeeded,
            duration_ms,
            nodes: &node_results,
            services: &services,
            tasks: &self.evidence.tasks,
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
            &node_results,
            duration_ms,
            run_summary_path.as_deref(),
            &self.placement.logs_dir,
        ) {
            failures.push(error);
        }

        let primary_task = if self.direct_selected {
            self.evidence
                .selected_task
                .map(|index| self.evidence.tasks[index.0].clone())
        } else {
            self.evidence.tasks.last().cloned()
        };
        let output = RunOutput {
            run_id: self.run_id.to_string(),
            manifest_path: self.admission.common().manifest_path().to_path_buf(),
            computed_manifest_hash: self.admission.common().computed_manifest_hash().to_owned(),
            duration_ms,
            services,
            summary_path: primary_task.as_ref().map(|task| task.summary_path.clone()),
            task: primary_task,
            tasks: self.evidence.tasks,
            nodes: node_results,
            run_summary_path: run_summary_path.clone(),
        };
        match failures.finish(output) {
            Ok(output) => Ok(output),
            Err(error) => Err(with_failure_summary(error, run_summary_path)),
        }
    }
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
    let command = args.first().map(String::as_str).unwrap_or(CHECK_COMMAND);
    match command {
        CHECK_COMMAND => check(args.get(1..).unwrap_or(&[])),
        RUN_COMMAND => run_m0(args.get(1..).unwrap_or(&[])),
        PS_COMMAND => run_control(ControlCommand::Ps, args.get(1..).unwrap_or(&[])),
        DOWN_COMMAND => run_control(ControlCommand::Down, args.get(1..).unwrap_or(&[])),
        CLEAN_COMMAND => run_control(ControlCommand::Clean, args.get(1..).unwrap_or(&[])),
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
    if args.first().map(String::as_str) != Some(RUN_COMMAND) {
        return ErrorOutputProjection::Human;
    }
    let args = args.get(1..).unwrap_or(&[]);
    let mut mode = None;
    let mut index = 0;
    while index < args.len() {
        if args[index].as_str() == RUN_OUTPUT
            && let Some(value) = args.get(index + 1).map(String::as_str)
        {
            mode = Some(value);
            index += 1;
        }
        index += 1;
    }
    match mode {
        Some(RUN_OUTPUT_MODE_BOTH) => ErrorOutputProjection::Both,
        Some(RUN_OUTPUT_MODE_JSON) => ErrorOutputProjection::Json,
        _ => ErrorOutputProjection::Human,
    }
}

fn print_json_error(error: &RuntimeError) -> Result<(), RuntimeError> {
    write_stderr_line(serde_json::to_string(error).unwrap_or_else(|_| error.to_string()))
}

fn print_human_error(error: &RuntimeError) -> Result<(), RuntimeError> {
    write_stderr_line(format!(
        "error: {}: {}",
        error_code_wire(error.code),
        error.message
    ))?;
    for cause in error.causes.iter() {
        write_stderr_line(format!(
            "  cause: {}: {}",
            error_code_wire(cause.code),
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
    writeln!(io::stderr().lock(), "{line}").map_err(|error| {
        output_projection_io_error(
            OutputStream::Stderr,
            ProjectionOperation::Write,
            "<stderr>",
            error,
        )
    })
}

fn string_detail<'a>(error: &'a RuntimeError, key: &str) -> Option<&'a str> {
    error.details.get(key).and_then(Value::as_str)
}

fn check(args: &[String]) -> Result<(), RuntimeError> {
    if print_help_if_requested(args, CHECK_HELP) {
        return Ok(());
    }
    let mut common = ManifestOptions::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            _ if common.consume(args, &mut index)? => {}
            other => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::ManifestAdmission,
                    format!("unknown check argument: {other}"),
                ));
            }
        }
        index += 1;
    }
    let (manifest_path, allow_non_store, slot) = common.finish()?;
    let admission = load_admitted_manifest(manifest_path, allow_non_store)?;
    let selected_slot =
        select_slot(admission.common().manifest(), slot).map_err(post_admission_error)?;
    let output = CheckOutput {
        manifest_path: admission.common().manifest_path().to_path_buf(),
        computed_manifest_hash: admission.common().computed_manifest_hash().to_owned(),
        raw_len: admission.common().raw_len(),
        project_id: admission.common().project_id().to_owned(),
        runtime_abi: admission.common().runtime_abi().to_owned(),
        toolchain_id: admission.common().toolchain_id().to_owned(),
        target_system: admission.common().target_system().to_owned(),
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
    let parsed_options = parse_run_options(args)?;
    let cancellation = CancellationToken::new();
    let run_id = new_run_id();
    let admission = load_admitted_manifest(
        parsed_options.manifest_path.clone(),
        parsed_options.allow_non_store,
    )?;
    let output_mode = resolve_run_output_mode(
        admission.common().manifest(),
        parsed_options.output_mode,
        parsed_options.task.as_deref(),
    );
    let selected_task = validate_run_selection(
        admission.common().execution_manifest(),
        output_mode,
        parsed_options.task.as_deref(),
    )?;
    let options = parsed_options.resolve(output_mode, selected_task);
    let redactor = Redactor::from_secrets(admission.secrets());
    let manifest_path = admission.common().manifest_path().to_path_buf();
    let computed_manifest_hash = admission.common().computed_manifest_hash().to_owned();
    let output = run_m0_admitted(&admission, &redactor, &options, run_id, &cancellation).map_err(
        |error| {
            redactor
                .redact_error(error.with_manifest_if_missing(manifest_path, computed_manifest_hash))
        },
    )?;
    if options.output_mode.emit_json() {
        print_json_redacted(&output, &redactor)?;
    }
    Ok(())
}

fn run_m0_admitted(
    admission: &RunAdmission,
    redactor: &Redactor,
    options: &RunOptions,
    run_id: String,
    cancellation: &CancellationToken,
) -> Result<RunOutput, RuntimeError> {
    let manifest = admission.common().manifest();
    cancellation.check()?;
    let selected_slot = select_slot(manifest, options.slot).map_err(post_admission_error)?;
    let plan = plan(
        admission.common().execution_manifest(),
        &options.task,
        selected_slot.slot,
    )
    .map_err(post_admission_error)?;
    let placement =
        derive_host_placement_for_slot(manifest, &selected_slot, &run_id, &options.state_base)
            .map_err(post_admission_error)?;
    // Every failure past this point carries the run's identity and state paths:
    // the operator must be able to find the evidence without re-deriving the
    // placement by hand.
    run_m0_placed(
        admission,
        &plan,
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
/// runtime lifecycle failure, never another manifest-admission failure.
fn post_admission_error(error: RuntimeError) -> RuntimeError {
    if error.code != nixfied_runtime::ErrorCode::ManifestAdmission {
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
    admission: &RunAdmission,
    plan: &nixfied_runtime::execution::RunPlan<'_>,
    redactor: &Redactor,
    options: &RunOptions,
    run_id: &str,
    selected_slot: &nixfied_runtime::slot::SelectedSlot<'_>,
    placement: &nixfied_runtime::state::HostPlacement,
    cancellation: &CancellationToken,
) -> Result<RunOutput, RuntimeError> {
    let manifest = admission.common().manifest();
    let run_started = Instant::now();
    let mut diagnostic_failures: Vec<RuntimeError> = Vec::new();
    // The registry opens before the marker decision: when the slot was last
    // used by a different manifest build, the upgrade path needs registry evidence
    // to tear down what that build left running.
    materialize_registry_root(placement)?;
    let identity = StateIdentity::from_selected_slot(admission.common(), selected_slot);
    let mut registry = Registry::open_or_create(
        placement.registry_path(),
        &RegistryIdentity::for_slot(
            &manifest.project.project_id,
            selected_slot.environment,
            selected_slot.slot,
            &manifest.runtime_abi,
            &manifest.toolchain_id,
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
                "  upgraded slot state from manifest {} (state {})",
                upgrade.from_manifest_hash.as_deref().unwrap_or("unknown"),
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

    let direct_selected = admission
        .common()
        .execution_manifest()
        .leaf(options.task.as_str())
        .is_some();

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
            let service = binding.service;
            let primary_id = service.primary_endpoint.as_ref()?;
            let primary = service.endpoints.get(primary_id)?;
            let port = *binding.endpoint_ports.get(primary_id)?;
            Some((
                binding.service.name.clone(),
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
    let mut session = RunSession {
        placement,
        admission,
        options,
        redactor,
        cancellation,
        run_id,
        run_started,
        registry,
        started: Vec::new(),
        extra_services: Vec::new(),
        service_lifetime: plan.service_lifetime,
        direct_selected,
        lease: None,
        evidence: RunEvidence::default(),
        diagnostic_failures,
    };
    macro_rules! finish_run {
        ($error:expr, $extra_services:expr) => {{
            let error = $error;
            session.extra_services = $extra_services;
            return session.finalize(Some(error));
        }};
    }
    for binding in &plan.services {
        let service_name = binding.service.name.as_str();
        if options.output_mode.emit_summary()
            && let Err(error) = write_diagnostic(
                options.output_mode,
                format_args!("  starting service {service_name}"),
            )
        {
            session.diagnostic_failures.push(error);
        }

        let prepare_runner: Option<PrepareRunner<'_>> =
            binding.service.prepare.as_ref().map(|_| {
                let context = NodeContext {
                    placement,
                    run: RunContext::new(admission, run_id, &placement.state_root, redactor),
                    cancellation,
                    output_mode: options.output_mode,
                };
                let evidence = &mut session.evidence;
                let started = &session.started;
                let diagnostics = &mut session.diagnostic_failures;
                Box::new(move |registry: &mut Registry| {
                    for node in &binding.prepare_nodes {
                        execute_node(
                            &context,
                            registry,
                            evidence,
                            started,
                            diagnostics,
                            node,
                            NodeRole::Prepare,
                        )?;
                    }
                    Ok(())
                }) as PrepareRunner<'_>
            });

        let current_service = match start_service_for_slot(
            admission,
            placement,
            &mut session.registry,
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
            Err(error) => {
                finish_run!(error.with_detail("failedService", service_name), Vec::new());
            }
        };
        if session.lease.is_none() {
            session.lease = Some(RunLeaseHeartbeat::start(
                placement.registry_path().to_path_buf(),
                session.registry.identity().clone(),
                current_service.info().run_id.clone(),
                current_service.info().owner_token.clone(),
            ));
        }
        let mut checkpoint = || {
            for service in &session.started {
                service.check_liveness()?;
            }
            Ok(())
        };
        let mut current_service =
            match current_service.ready(&mut session.registry, cancellation, &mut checkpoint) {
                Ok(service) => service,
                Err(failure) => {
                    let (service, error) = failure.into_parts();
                    let failed_service_output = service_output(service.info());
                    let error = service.finalize_failed_start(
                        &mut session.registry,
                        options.timeout_ms,
                        error,
                    );
                    finish_run!(
                        error.with_detail("failedService", service_name),
                        vec![failed_service_output]
                    );
                }
            };
        let startup_result =
            current_service.check_health(&mut session.registry, cancellation, &mut checkpoint);
        if let Err(error) = startup_result {
            let failed_service_output = service_output(current_service.info());
            let error = current_service.finalize_failed_start(
                &mut session.registry,
                options.timeout_ms,
                error,
            );
            finish_run!(
                error.with_detail("failedService", service_name),
                vec![failed_service_output]
            );
        }
        if options.output_mode.emit_summary() {
            let name = current_service.service_name();
            let message = match current_service.selected_endpoint() {
                Some(endpoint) => format!(
                    "  service {name} ready at {}:{}",
                    endpoint.host, endpoint.port
                ),
                None => format!("  service {name} ready (endpoint-less)"),
            };
            if let Err(error) = write_diagnostic(options.output_mode, message) {
                session.diagnostic_failures.push(error);
            }
        }
        session.started.push(current_service);
    }

    if let Err(error) = cancellation.check() {
        finish_run!(error, Vec::new());
    }

    let context = NodeContext {
        placement,
        run: RunContext::new(admission, run_id, &placement.state_root, redactor),
        cancellation,
        output_mode: options.output_mode,
    };
    for node in &plan.nodes {
        if let Err(error) = execute_node(
            &context,
            &mut session.registry,
            &mut session.evidence,
            &session.started,
            &mut session.diagnostic_failures,
            node,
            NodeRole::Root { direct_selected },
        ) {
            finish_run!(error, Vec::new());
        }
    }

    if cancellation.is_canceled() {
        finish_run!(nixfied_runtime::cancellation::canceled_error(), Vec::new());
    }
    session.finalize(None)
}

fn services_output(services: &[StartedService]) -> Vec<ServiceRunOutput> {
    services
        .iter()
        .map(|service| service_output(service.info()))
        .collect()
}

fn service_output(info: &nixfied_runtime::service::ServiceInfo) -> ServiceRunOutput {
    ServiceRunOutput {
        service_id: info.service_name().to_string(),
        service_instance_id: info.service_instance_id.clone(),
        process_key: info.process_key.clone(),
        selected_endpoint: info.selected_endpoint().cloned(),
    }
}

fn write_diagnostic(output_mode: RunOutputMode, line: impl Display) -> Result<(), RuntimeError> {
    if !output_mode.emit_summary() {
        return Ok(());
    }
    write_stderr_line(line)
}

fn write_stdout_line(line: &str) -> Result<(), RuntimeError> {
    writeln!(io::stdout().lock(), "{line}").map_err(|error| {
        output_projection_io_error(
            OutputStream::Stdout,
            ProjectionOperation::Write,
            "<stdout>",
            error,
        )
    })
}

#[derive(Clone, Copy)]
struct EvidenceIndex(usize);

#[derive(Default)]
struct RunEvidence {
    next_occurrence: u64,
    tasks: Vec<TaskRun>,
    root_nodes: Vec<EvidenceIndex>,
    selected_task: Option<EvidenceIndex>,
    replay: Option<ReplayTicket>,
}

impl RunEvidence {
    fn allocate_occurrence(&mut self) -> Result<u64, RuntimeError> {
        let occurrence = self.next_occurrence;
        self.next_occurrence = occurrence.checked_add(1).ok_or_else(|| {
            RuntimeError::new(
                nixfied_runtime::ErrorCode::StateUnwritable,
                "task occurrence sequence exhausted",
            )
        })?;
        Ok(occurrence)
    }

    fn nodes(&self) -> Vec<NodeResult> {
        self.root_nodes
            .iter()
            .map(|index| {
                let task = &self.tasks[index.0];
                NodeResult {
                    node_id: task.step_path.clone(),
                    task_id: task.task_id.clone(),
                    success: task.success,
                    exit_code: task.exit_code,
                    duration_ms: task.duration_ms,
                    stdout_path: task.stdout_path.clone(),
                    stderr_path: task.stderr_path.clone(),
                    summary_path: task.summary_path.clone(),
                }
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
enum NodeRole {
    Prepare,
    Root { direct_selected: bool },
}

struct NodeContext<'a> {
    placement: &'a nixfied_runtime::state::HostPlacement,
    run: RunContext<'a>,
    cancellation: &'a CancellationToken,
    output_mode: RunOutputMode,
}

fn execute_node(
    context: &NodeContext<'_>,
    registry: &mut Registry,
    evidence: &mut RunEvidence,
    started: &[StartedService],
    diagnostic_failures: &mut Vec<RuntimeError>,
    node: &PlanNode<'_>,
    role: NodeRole,
) -> Result<(), RuntimeError> {
    let decorate = |error: RuntimeError| match role {
        NodeRole::Prepare => error,
        NodeRole::Root { .. } => error.with_detail("failedNodeId", node.node_id.as_str()),
    };
    let occurrence = evidence.allocate_occurrence().map_err(decorate)?;
    let task = node.task;
    for name in &task.requires {
        started
            .iter()
            .find(|service| service.service_name() == name.as_str())
            .ok_or_else(|| {
                decorate(RuntimeError::new(
                    nixfied_runtime::ErrorCode::DependencyUnavailable,
                    match role {
                        NodeRole::Prepare => format!(
                            "prepare node {} requires service {name} which is not started yet",
                            node.node_id
                        ),
                        NodeRole::Root { .. } => format!(
                            "task {} depends on service {name} which was not started",
                            task.task_id
                        ),
                    },
                ))
            })?;
    }
    if matches!(role, NodeRole::Prepare)
        && context.output_mode.emit_summary()
        && let Err(error) = write_diagnostic(
            context.output_mode,
            format_args!("  prepare node {} ({})", node.node_id, task.task_id),
        )
    {
        diagnostic_failures.push(error);
    }
    let mode = match role {
        NodeRole::Root {
            direct_selected: true,
        } if context.output_mode.is_task_output() => EvidenceMode::ReplaySelected,
        NodeRole::Prepare | NodeRole::Root { .. } => EvidenceMode::CaptureOnly,
    };
    let result = run_dependent_task_cancellable(
        context.placement,
        registry,
        context.run,
        &started.iter().collect::<Vec<_>>(),
        node.node_id.as_str(),
        occurrence,
        task,
        context.cancellation,
        mode,
    );
    let (completed, error) = match result {
        Ok(TaskExecution::Succeeded(completed)) => (completed, None),
        Ok(TaskExecution::Failed { error, evidence }) => (evidence, Some(error)),
        Err(TaskExecutionError::BeforeTerminal(error)) => return Err(decorate(*error)),
        Err(TaskExecutionError::AfterTerminal { error, evidence }) => (*evidence, Some(*error)),
    };
    let (task_run, ticket) = completed.into_task_and_replay();
    let index = EvidenceIndex(evidence.tasks.len());
    evidence.tasks.push(task_run);
    evidence.replay = ticket.or(evidence.replay.take());
    if let NodeRole::Root { direct_selected } = role {
        evidence.root_nodes.push(index);
        if direct_selected {
            evidence.selected_task = Some(index);
        }
        let task_run = &evidence.tasks[index.0];
        if context.output_mode.emit_summary() {
            let diagnostic = if error.is_some() {
                write_diagnostic(
                    context.output_mode,
                    format_args!(
                        "  fail {} ({}) {} exit={}",
                        node.node_id,
                        task.task_id,
                        human_duration(task_run.duration_ms),
                        human_exit_code(task_run.exit_code)
                    ),
                )
            } else {
                write_diagnostic(
                    context.output_mode,
                    format_args!(
                        "  ok {} ({}) {}",
                        node.node_id,
                        task.task_id,
                        human_duration(task_run.duration_ms)
                    ),
                )
            };
            if let Err(error) = diagnostic {
                diagnostic_failures.push(error);
            }
            if error.is_some()
                && let Err(error) = write_diagnostic(
                    context.output_mode,
                    format_args!("    stderr: {}", human_path(&task_run.stderr_path)),
                )
            {
                diagnostic_failures.push(error);
            }
        }
    }
    match error {
        Some(error) => Err(decorate(attach_task_evidence(
            error,
            &evidence.tasks[index.0],
        ))),
        None => Ok(()),
    }
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
    let mut summary = serde_json::json!(RunSummaryOutput {
        run_id: input.run_id,
        success: input.run_succeeded && input.nodes.iter().all(|node| node.success),
        duration_ms: input.duration_ms,
        services: input.services,
        nodes: input.nodes,
        tasks: input.tasks,
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
    manifest_path: PathBuf,
    allow_non_store: bool,
    state_base: PathBuf,
    timeout_ms: u64,
    slot: Option<RuntimeSlotValue>,
    cleanup_mode: nixfied_runtime::state::CleanupMode,
}

fn print_help_if_requested(args: &[String], help: &str) -> bool {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), HELP_SHORT | HELP_LONG))
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
fn selection_required_error(
    manifest: &nixfied_runtime::execution::ExecutionManifest,
) -> RuntimeError {
    let declared: Vec<&str> = manifest.task_ids().map(|task| task.as_str()).collect();
    RuntimeError::new(
        nixfied_runtime::ErrorCode::TaskSelectionInvalid,
        format!(
            "{RUN_COMMAND} requires {RUN_TASK} <id>; declared tasks: {}",
            declared.join(", ")
        ),
    )
    .with_detail("declaredTasks", &declared)
}

fn validate_run_selection(
    manifest: &nixfied_runtime::execution::ExecutionManifest,
    output_mode: RunOutputMode,
    task: Option<&str>,
) -> Result<nixfied_manifest::TaskId, RuntimeError> {
    let Some(task) = task else {
        return Err(selection_required_error(manifest));
    };
    let task_id = nixfied_manifest::TaskId::new(task);
    if manifest.leaf(task).is_some() {
        return Ok(task_id);
    }
    if let Some(nixfied_runtime::execution::ExecutableTask::Composite(composite)) =
        manifest.tasks().get(&task_id)
    {
        if output_mode.is_task_output() {
            return Err(RuntimeError::new(
                nixfied_runtime::ErrorCode::TaskSelectionInvalid,
                format!("task-output requires a directly selected leaf; {task} is composite"),
            )
            .with_detail("task", task)
            .with_detail("compositeSteps", composite.steps.len()));
        }
        return Ok(task_id);
    }
    Err(selection_required_error(manifest).with_detail("unknownTask", task))
}

fn resolve_run_output_mode(
    manifest: &nixfied_manifest::Manifest,
    explicit: Option<RunOutputMode>,
    task: Option<&str>,
) -> RunOutputMode {
    explicit.unwrap_or_else(|| {
        task.and_then(|task| manifest.tasks.get(task))
            .map(|task| match task.default_output {
                TaskDefaultOutput::Summary => RunOutputMode::Summary,
                TaskDefaultOutput::TaskOutput => RunOutputMode::TaskOutput,
            })
            .unwrap_or(RunOutputMode::Summary)
    })
}

fn parse_run_output_mode(value: &str) -> Result<RunOutputMode, RuntimeError> {
    match value {
        RUN_OUTPUT_MODE_SUMMARY => Ok(RunOutputMode::Summary),
        RUN_OUTPUT_MODE_JSON => Ok(RunOutputMode::Json),
        RUN_OUTPUT_MODE_BOTH => Ok(RunOutputMode::Both),
        RUN_OUTPUT_MODE_TASK_OUTPUT => Ok(RunOutputMode::TaskOutput),
        other => Err(RuntimeError::new(
            nixfied_runtime::ErrorCode::OutputModeInvalid,
            format!("invalid {RUN_OUTPUT} value {other}: expected {RUN_OUTPUT_MODE_CHOICES}"),
        )),
    }
}

fn run_control(command: ControlCommand, args: &[String]) -> Result<(), RuntimeError> {
    if print_help_if_requested(args, control_help(command)) {
        return Ok(());
    }
    let options = parse_control_options(command, args)?;
    let admission =
        load_admitted_manifest_for_control(options.manifest_path.clone(), options.allow_non_store)?;
    let manifest_path = admission.manifest_path().to_path_buf();
    let computed_manifest_hash = admission.computed_manifest_hash().to_owned();
    run_control_admitted(command, &admission, &options)
        .map_err(|error| error.with_manifest_if_missing(manifest_path, computed_manifest_hash))
}

fn run_control_admitted(
    command: ControlCommand,
    admission: &ControlAdmission,
    options: &ControlOptions,
) -> Result<(), RuntimeError> {
    let manifest = admission.manifest();
    let selected_slot = select_slot(manifest, options.slot).map_err(post_admission_error)?;
    let placement =
        derive_host_placement_for_slot(manifest, &selected_slot, "control", &options.state_base)
            .map_err(post_admission_error)?;
    let result = (|| {
        let mut registry = Registry::open_or_create(
            placement.registry_path(),
            &RegistryIdentity::for_slot(
                &manifest.project.project_id,
                selected_slot.environment,
                selected_slot.slot,
                &manifest.runtime_abi,
                &manifest.toolchain_id,
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

struct ManifestOptions {
    manifest_path: Option<PathBuf>,
    allow_non_store: bool,
    slot: Option<RuntimeSlotValue>,
}

impl ManifestOptions {
    fn new() -> Self {
        Self {
            manifest_path: RUNTIME_MANIFEST_INITIAL.map(PathBuf::from),
            allow_non_store: RUNTIME_ALLOW_NON_STORE_MANIFEST_INITIAL,
            slot: RUNTIME_SLOT_INITIAL,
        }
    }

    fn consume(&mut self, args: &[String], index: &mut usize) -> Result<bool, RuntimeError> {
        match args[*index].as_str() {
            RUNTIME_MANIFEST => {
                *index += 1;
                self.manifest_path = args.get(*index).map(PathBuf::from);
            }
            RUNTIME_ALLOW_NON_STORE_MANIFEST => self.allow_non_store = true,
            RUNTIME_SLOT => {
                *index += 1;
                self.slot = Some(parse_integer_arg(args.get(*index), RUNTIME_SLOT)?);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn finish(self) -> Result<(PathBuf, bool, Option<RuntimeSlotValue>), RuntimeError> {
        let manifest_path = self.manifest_path.ok_or_else(|| {
            RuntimeError::new(
                nixfied_runtime::ErrorCode::ManifestAdmission,
                format!("missing {RUNTIME_MANIFEST} path"),
            )
        })?;
        Ok((manifest_path, self.allow_non_store, self.slot))
    }
}

fn parse_run_options(args: &[String]) -> Result<ParsedRunOptions, RuntimeError> {
    let mut common = ManifestOptions::new();
    let mut state_base = RUNTIME_STATE_BASE_INITIAL.map(PathBuf::from);
    let mut timeout_ms = RUN_TIMEOUT_MS_INITIAL;
    let mut output_mode = RUN_OUTPUT_INITIAL;
    let mut task = RUN_TASK_INITIAL.map(str::to_string);
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            _ if common.consume(args, &mut index)? => {}
            RUNTIME_STATE_BASE => {
                index += 1;
                state_base = args.get(index).map(PathBuf::from);
            }
            RUN_TASK => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| {
                        RuntimeError::new(
                            nixfied_runtime::ErrorCode::TaskSelectionInvalid,
                            format!("missing {RUN_TASK} value"),
                        )
                    })?
                    .clone();
                if task.replace(value).is_some() {
                    return Err(RuntimeError::new(
                        nixfied_runtime::ErrorCode::TaskSelectionInvalid,
                        format!("{RUN_TASK} may be specified only once"),
                    ));
                }
            }
            RUN_TIMEOUT_MS => {
                index += 1;
                timeout_ms =
                    parse_integer_arg::<RunTimeoutMsValue>(args.get(index), RUN_TIMEOUT_MS)?;
            }
            RUN_OUTPUT => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    RuntimeError::new(
                        nixfied_runtime::ErrorCode::OutputModeInvalid,
                        format!("missing {RUN_OUTPUT} value"),
                    )
                })?;
                let parsed = parse_run_output_mode(value)?;
                if output_mode.replace(parsed).is_some() {
                    return Err(RuntimeError::new(
                        nixfied_runtime::ErrorCode::OutputModeConflict,
                        format!("{RUN_OUTPUT} may be specified only once"),
                    ));
                }
            }
            "--summary" | "--json" | "--both" | "--task-output" => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::OutputModeInvalid,
                    format!(
                        "unsupported output flag {}; use {RUN_OUTPUT} <mode>",
                        args[index]
                    ),
                ));
            }
            other => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::ManifestAdmission,
                    format!("unknown run argument: {other}"),
                ));
            }
        }
        index += 1;
    }
    let (manifest_path, allow_non_store, slot) = common.finish()?;
    let state_base = state_base.map(Ok).unwrap_or_else(state_base_from_env)?;
    Ok(ParsedRunOptions {
        manifest_path,
        allow_non_store,
        state_base,
        timeout_ms,
        output_mode,
        slot,
        task,
    })
}

struct ParsedRunOptions {
    manifest_path: PathBuf,
    allow_non_store: bool,
    state_base: PathBuf,
    timeout_ms: RunTimeoutMsValue,
    output_mode: Option<RunOutputValue>,
    slot: Option<RuntimeSlotValue>,
    task: Option<String>,
}

impl ParsedRunOptions {
    fn resolve(self, output_mode: RunOutputValue, task: nixfied_manifest::TaskId) -> RunOptions {
        RunOptions {
            state_base: self.state_base,
            timeout_ms: self.timeout_ms,
            output_mode,
            slot: self.slot,
            task,
        }
    }
}

struct RunOptions {
    state_base: PathBuf,
    timeout_ms: RunTimeoutMsValue,
    output_mode: RunOutputValue,
    slot: Option<RuntimeSlotValue>,
    task: nixfied_manifest::TaskId,
}

fn parse_control_options(
    command: ControlCommand,
    args: &[String],
) -> Result<ControlOptions, RuntimeError> {
    let mut common = ManifestOptions::new();
    let mut state_base = RUNTIME_STATE_BASE_INITIAL.map(PathBuf::from);
    let mut timeout_ms = DOWN_TIMEOUT_MS_INITIAL;
    let mut cleanup_mode = if CLEAN_PURGE_INITIAL {
        nixfied_runtime::state::CleanupMode::Purge
    } else {
        nixfied_runtime::state::CleanupMode::Standard
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            _ if common.consume(args, &mut index)? => {}
            value if value == RUNTIME_STATE_BASE => {
                index += 1;
                state_base = args.get(index).map(PathBuf::from);
            }
            CLEAN_PURGE if matches!(command, ControlCommand::Clean) => {
                cleanup_mode = nixfied_runtime::state::CleanupMode::Purge;
            }
            DOWN_TIMEOUT_MS if matches!(command, ControlCommand::Down) => {
                index += 1;
                timeout_ms =
                    parse_integer_arg::<DownTimeoutMsValue>(args.get(index), DOWN_TIMEOUT_MS)?;
            }
            other => {
                return Err(RuntimeError::new(
                    nixfied_runtime::ErrorCode::ManifestAdmission,
                    format!("unknown control argument: {other}"),
                ));
            }
        }
        index += 1;
    }
    let (manifest_path, allow_non_store, slot) = common.finish()?;
    let state_base = state_base.map(Ok).unwrap_or_else(state_base_from_env)?;
    Ok(ControlOptions {
        manifest_path,
        allow_non_store,
        state_base,
        timeout_ms,
        slot,
        cleanup_mode,
    })
}

fn parse_integer_arg<T: std::str::FromStr>(
    value: Option<&String>,
    flag: &str,
) -> Result<T, RuntimeError>
where
    T::Err: Display,
{
    let value = value.ok_or_else(|| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ManifestAdmission,
            format!("missing {flag} value"),
        )
    })?;
    value.parse::<T>().map_err(|error| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ManifestAdmission,
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

fn admission_context(allow_non_store: bool) -> AdmissionContext {
    AdmissionContext::current(if allow_non_store {
        StoreOriginPolicy::AllowNonStoreForTests
    } else {
        StoreOriginPolicy::RequireStore
    })
}

fn load_admitted_manifest(
    manifest_path: PathBuf,
    allow_non_store: bool,
) -> Result<RunAdmission, RuntimeError> {
    let admission =
        nixfied_runtime::admit_run(&manifest_path, &admission_context(allow_non_store))?;
    warn_on_ephemeral_port_overlap(admission.common().manifest())?;
    Ok(admission)
}

fn load_admitted_manifest_for_control(
    manifest_path: PathBuf,
    allow_non_store: bool,
) -> Result<ControlAdmission, RuntimeError> {
    let admission =
        nixfied_runtime::admit_control(&manifest_path, &admission_context(allow_non_store))?;
    warn_on_ephemeral_port_overlap(admission.manifest())?;
    Ok(admission)
}

/// Warn (stderr, non-fatal) when a slot's candidate port window overlaps the
/// host's ephemeral port range: the kernel hands out ports in that range to
/// any process, so a deterministic window inside it can collide with unrelated
/// ephemeral allocations. The range is host state only Linux exposes a stable
/// path for; elsewhere the check is silently skipped.
fn warn_on_ephemeral_port_overlap(
    manifest: &nixfied_manifest::Manifest,
) -> Result<(), RuntimeError> {
    let Some((low, high)) = host_ephemeral_port_range() else {
        return Ok(());
    };
    for placement in manifest.placement.slot_placements.values() {
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
        nixfied_runtime::ErrorCode::ManifestNotStoreOutput => 13,
        nixfied_runtime::ErrorCode::ManifestInvalid => 14,
        nixfied_runtime::ErrorCode::PlatformUnsupported => 15,
        nixfied_runtime::ErrorCode::ClosureMissing => 16,
        nixfied_runtime::ErrorCode::SourceMismatch => 17,
        nixfied_runtime::ErrorCode::ManifestAdmission => 18,
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
