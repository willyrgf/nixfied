use nixfied_runtime::registry::session::{
    ExecutionOutcome, close_source_registration, record_execution_outcome,
    record_finalization_complete, record_finalization_unfinished, seal_output,
};
use std::fmt::Display;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use nixfied_manifest::TaskDefaultOutput;
use nixfied_runtime::cancellation::{CancellationToken, ProcessSignalGuard};
use nixfied_runtime::error::error_code_wire;
use nixfied_runtime::execution::{PlanNode, plan};
use nixfied_runtime::output::{
    OutputStream, ProjectionOperation, SourcePresentation, output_projection_io_error,
};
use nixfied_runtime::presenter::{CommandPresenter, PresentationMode, PresenterInit};
use nixfied_runtime::redaction::Redactor;
use nixfied_runtime::registry::{Registry, RegistryIdentity, RegistryReader};
use nixfied_runtime::service::{
    PrepareRunner, ReadyService, RunContext, SelectedEndpoint, ServiceSelection, TaskExecution,
    TaskExecutionError, TaskRun, record_run_created, run_dependent_task_cancellable,
    run_slot_clean, start_service_for_slot,
};
use nixfied_runtime::slot::select_slot;
use nixfied_runtime::state::{
    StateIdentity, apply_retention, derive_host_placement_for_slot, prepare_slot_state,
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
    state: &'a StateIdentity,
    admission: &'a RunAdmission,
    options: &'a RunOptions,
    redactor: &'a Redactor,
    cancellation: &'a CancellationToken,
    run_id: &'a str,
    run_started: Instant,
    registry: Registry,
    started: Vec<ReadyService>,
    extra_services: Vec<ServiceRunOutput>,
    direct_selected: bool,
    evidence: RunEvidence,
    diagnostics: SessionDiagnostics,
    control: nixfied_runtime::session_control::SessionControl,
    presenter: &'a mut Option<CommandPresenter>,
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
        let Some(primary) = self.primary.take() else {
            self.primary = Some(error);
            return;
        };
        self.primary = Some(
            if failure_priority(error.code) > failure_priority(primary.code) {
                error.absorb(primary)
            } else {
                primary.absorb(error)
            },
        );
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
    fn finalize(
        mut self,
        initial_error: Option<RuntimeError>,
        observed: Option<ExecutionOutcome>,
    ) -> Result<RunOutput, RuntimeError> {
        let execution_outcome = observed.unwrap_or_else(|| match initial_error.as_ref() {
            Some(error) if error.code == nixfied_runtime::ErrorCode::Canceled => {
                ExecutionOutcome::Canceled
            }
            Some(_) => ExecutionOutcome::Failed,
            None if self.cancellation.is_canceled() => ExecutionOutcome::Canceled,
            None => ExecutionOutcome::Succeeded,
        });
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
        if let Err(error) = record_execution_outcome(
            &mut self.registry,
            self.run_id,
            self.admission.common().computed_manifest_hash(),
            execution_outcome,
        ) {
            failures.push(error);
        }
        // Presentation is command-owned: no delivery wait precedes teardown.
        record_cancellation_once(self.cancellation, &mut cancellation_recorded, &mut failures);

        let services = {
            let mut services = self.extra_services;
            services.extend(services_output(&self.started));
            services
        };
        let canceled = self.cancellation.is_canceled()
            || failures
                .primary
                .as_ref()
                .is_some_and(|error| error.code == nixfied_runtime::ErrorCode::Canceled);
        let had_initial_failure = !failures.is_empty();
        loop {
            // Observe every remaining service before signaling the next one: a
            // service that exited during teardown fails from its own exit, even
            // under cancellation, and never settles as a deliberate stop.
            while let Some(index) = self.started.iter().position(|service| service.exited()) {
                let service = self.started.remove(index);
                if let Err(error) = service.stop(&mut self.registry, self.options.timeout_ms) {
                    failures.push(error);
                }
            }
            let Some(service) = self.started.pop() else {
                break;
            };
            // The teardown policy is chosen once per service. Late
            // cancellation of an otherwise healthy teardown cancels what
            // remains; a failed session keeps stopping its services.
            let timeout_ms = self.options.timeout_ms;
            let result = if canceled {
                service.cancel(&mut self.registry, timeout_ms, "run canceled")
            } else if had_initial_failure {
                service.stop(&mut self.registry, timeout_ms)
            } else if self.cancellation.is_canceled() {
                service.cancel(
                    &mut self.registry,
                    timeout_ms,
                    "run canceled during shutdown",
                )
            } else {
                service.stop_observing(&mut self.registry, timeout_ms, self.cancellation)
            };
            if let Err(error) = result {
                failures.push(error);
            }
        }
        // Retention requires recorded quiescence: any unresolved process
        // obligation refuses deletion and retains data. Completion is claimed
        // only after every obligation settled.
        let settlement = apply_retention(self.state, &mut self.registry)
            .map(|_| ())
            .inspect_err(|error| failures.push(error.clone()));
        let manifest_hash = self.admission.common().computed_manifest_hash();
        let recorded = match settlement {
            Ok(()) => record_finalization_complete(&mut self.registry, self.run_id, manifest_hash),
            Err(error) => record_finalization_unfinished(
                &mut self.registry,
                self.run_id,
                manifest_hash,
                &error,
            ),
        };
        if let Err(error) = recorded {
            failures.push(error);
        }
        // No workload remains, so no new source may register.
        if let Err(error) =
            close_source_registration(&mut self.registry, self.run_id, manifest_hash)
        {
            failures.push(error);
        }
        // The session's last cancellation observation: a later signal ends
        // the presenter's drain rather than being absorbed by the session.
        if let Some(presenter) = self.presenter.as_mut() {
            presenter.observe_session_signals();
        }
        record_cancellation_once(self.cancellation, &mut cancellation_recorded, &mut failures);
        let duration_ms = elapsed_ms(self.run_started);
        let run_succeeded = failures.is_empty();
        let node_results = self.evidence.nodes();
        let mut publication_complete = true;
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
                // A missing final-result artifact leaves publication unsealed.
                publication_complete = false;
                failures.push(error);
                None
            }
        };
        let footer_succeeded = failures.is_empty();
        write_run_footer(
            &mut self.diagnostics,
            footer_succeeded,
            &node_results,
            duration_ms,
            run_summary_path.as_deref(),
            &self.placement.logs_dir,
        );
        // Close every evidence writer before publishing the seal; a failed
        // close or write leaves the output unsealed rather than claiming it.
        match self.diagnostics.close() {
            Ok(()) if publication_complete => {
                if let Err(error) = seal_output(&mut self.registry, self.run_id, manifest_hash) {
                    failures.push(error);
                }
            }
            Ok(()) => {}
            Err(error) => failures.push(error),
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
        // Remove the session's control endpoint while the slot is still held.
        if let Err(error) = self.control.shutdown() {
            failures.push(error);
        }
        if let Err(error) = self.registry.close() {
            failures.push(error);
        }
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

    /// Structured output alone needs no live presentation.
    fn presentation(self) -> Option<PresentationMode> {
        match self {
            Self::Summary | Self::Both => Some(PresentationMode::Human),
            Self::TaskOutput => Some(PresentationMode::TaskOutput),
            Self::Json => None,
        }
    }
}

fn main() {
    let native_args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if let Some(exit) = nixfied_runtime::launch::dispatch(&native_args) {
        std::process::exit(exit);
    }
    if let Some(exit) = nixfied_runtime::presenter::dispatch(&native_args) {
        std::process::exit(exit);
    }
    if let Some(received) = nixfied_runtime::background::receive(&native_args) {
        let exit = match received {
            Err(exit) => exit,
            Ok((request, establishment)) => match ProcessSignalGuard::install() {
                Ok(signals) => {
                    let exit = match run_background_owner(request, establishment) {
                        Ok(()) => 0,
                        Err(error) => exit_code(&error),
                    };
                    drop(signals);
                    exit
                }
                Err(error) => exit_code(&error),
            },
        };
        std::process::exit(exit);
    }
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
    if parsed_options.daemon {
        return launch_background(args, parsed_options.timeout_ms);
    }
    run_session(parsed_options, new_run_id(), None)
}

/// The launcher validated syntax and option combinations; the owner performs
/// the one authoritative admission. Acknowledgement means establishment, not
/// readiness or task success.
fn launch_background(args: &[String], timeout_ms: u64) -> Result<(), RuntimeError> {
    let runtime = std::env::current_exe().map_err(|_| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::LifecycleFailed,
            "cannot locate the runtime executable for background launch",
        )
    })?;
    let run_id = new_run_id();
    let request = nixfied_runtime::background::Request {
        run_id: run_id.clone(),
        args: args
            .iter()
            .filter(|arg| arg.as_str() != RUN_DAEMON)
            .cloned()
            .collect(),
    };
    // Establishment includes predecessor recovery, bounded by the operation timeout.
    let wait = std::time::Duration::from_millis(timeout_ms)
        .saturating_add(std::time::Duration::from_secs(10));
    use nixfied_runtime::background::LaunchOutcome;
    match nixfied_runtime::background::launch(&runtime, &request, wait)? {
        LaunchOutcome::Established(acknowledgement) => print_json(&acknowledgement),
        LaunchOutcome::Rejected(error) => Err(error),
        LaunchOutcome::Uncertain => Err(RuntimeError::new(
            nixfied_runtime::ErrorCode::LifecycleFailed,
            "the background launch outcome is uncertain; the session may have been established",
        )
        .with_detail("runId", &run_id)),
        LaunchOutcome::Interrupted => {
            Err(nixfied_runtime::cancellation::canceled_error().with_detail("runId", &run_id))
        }
        LaunchOutcome::CanceledAfterEstablishment(acknowledgement) => {
            Err(nixfied_runtime::cancellation::canceled_error()
                .with_detail("runId", &acknowledgement.run_id)
                .with_detail("runDir", &acknowledgement.run_dir))
        }
        LaunchOutcome::CancellationUndelivered {
            acknowledgement,
            cause,
        } => Err(RuntimeError::new(
            nixfied_runtime::ErrorCode::LifecycleFailed,
            "the launcher was interrupted after establishment; its cancellation request did not reach the session",
        )
        .with_detail("runId", &acknowledgement.run_id)
        .with_detail("runDir", &acknowledgement.run_dir)
        .with_cause(cause)),
    }
}

/// The background owner: the same session path with no terminal presenter.
fn run_background_owner(
    request: nixfied_runtime::background::Request,
    mut establishment: nixfied_runtime::background::Establishment,
) -> Result<(), RuntimeError> {
    let result = parse_run_options(&request.args).and_then(|parsed| {
        if parsed.daemon || parsed.output_mode.is_some() {
            return Err(RuntimeError::new(
                nixfied_runtime::ErrorCode::OutputModeConflict,
                "a background session has no output projection",
            ));
        }
        run_session(parsed, request.run_id, Some(&mut establishment))
    });
    if let Err(error) = &result {
        establishment.reject(error);
    }
    result
}

fn run_session(
    parsed_options: ParsedRunOptions,
    run_id: String,
    establishment: Option<&mut nixfied_runtime::background::Establishment>,
) -> Result<(), RuntimeError> {
    let cancellation = CancellationToken::new();
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
    let background = establishment.is_some();
    let options = parsed_options.resolve(output_mode, selected_task, background);
    let redactor = Redactor::from_secrets(admission.secrets());
    let manifest_path = admission.common().manifest_path().to_path_buf();
    let computed_manifest_hash = admission.common().computed_manifest_hash().to_owned();
    let mut presenter = None;
    let result = run_m0_admitted(
        &admission,
        &redactor,
        &options,
        run_id,
        &cancellation,
        &mut presenter,
        establishment,
    );
    // The slot is released. The command may now wait for its presenter to
    // drain retained evidence, with no default deadline; a termination signal
    // ends the drain. Delivery never rewrites the session result.
    let result = match presenter.map(CommandPresenter::finish) {
        None => result,
        Some(delivery) => {
            // Presentation stopped by the same cancellation is not a second
            // failure; interrupted delivery of a successful session is.
            let canceled = matches!(
                &result,
                Err(error) if error.code == nixfied_runtime::ErrorCode::Canceled
            );
            match (result, delivery.into_error(canceled)) {
                (result, None) => result,
                (Ok(_), Some(delivery)) => Err(delivery),
                (Err(error), Some(delivery)) => {
                    let mut failures = FailureAccumulator::new();
                    failures.push(error);
                    failures.push(delivery);
                    Err(failures.primary.expect("a failure was pushed"))
                }
            }
        }
    };
    let output = result.map_err(|error| {
        redactor.redact_error(error.with_manifest_if_missing(manifest_path, computed_manifest_hash))
    })?;
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
    presenter: &mut Option<CommandPresenter>,
    establishment: Option<&mut nixfied_runtime::background::Establishment>,
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
    if options.background {
        reject_interactive_stdin(&plan)?;
    }
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
        presenter,
        establishment,
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
    presenter: &mut Option<CommandPresenter>,
    mut establishment: Option<&mut nixfied_runtime::background::Establishment>,
) -> Result<RunOutput, RuntimeError> {
    let launcher = std::env::current_exe().map_err(|_| {
        RuntimeError::new(
            nixfied_runtime::ErrorCode::ProcEscape,
            "cannot locate workload gate executable",
        )
    })?;
    let manifest = admission.common().manifest();
    let run_started = Instant::now();
    // Exclusive recovery settles every predecessor before the state marker
    // decision, independently of manifest provenance.
    // Observed abandonment before slot acquisition starts no session work.
    if let Some(establishment) = establishment.as_deref_mut() {
        establishment.check_abandonment()?;
    }
    let guard = nixfied_runtime::state::ownership::SlotGuard::acquire(placement, cancellation)?;
    let identity = StateIdentity::from_selected_slot(admission.common(), selected_slot);
    let mut registry = Registry::open_or_create(
        guard,
        &RegistryIdentity::for_slot(
            &manifest.project.project_id,
            selected_slot.environment,
            selected_slot.slot,
            &manifest.runtime_abi,
            &manifest.toolchain_id,
        ),
    )?;
    registry.set_redactor(redactor.clone());
    nixfied_runtime::control::recover_slot(&mut registry, &identity, options.timeout_ms)?;
    // Recovery effects already settled are kept; abandonment observed now
    // still prevents any new generation or session.
    if let Some(establishment) = establishment.as_deref_mut() {
        establishment.check_abandonment()?;
    }
    let preparation = prepare_slot_state(&identity, &mut registry)?;
    // The never-reused evidence directory: an existing one is an identity
    // collision, refused before any session fact is published.
    nixfied_runtime::state::claim_run_evidence(placement)?;

    let direct_selected = admission
        .common()
        .execution_manifest()
        .leaf(options.task.as_str())
        .is_some();

    // The session's cancellation endpoint exists before the session is
    // published, so `down` can reach every session it can select.
    let control = nixfied_runtime::session_control::SessionControl::establish(
        &placement.run_dir,
        cancellation,
    )?;
    // The diagnostic source exists before the commit that registers it, so no
    // fallible step separates establishment from its acknowledgement. Runtime
    // progress is retained evidence from here on; the presenter, not the
    // session owner, shows it while session duties remain.
    let mut diagnostics =
        SessionDiagnostics::create(&placement.run_dir, options.output_mode.emit_summary())?;
    // Observed abandonment before the commit prevents all new work. After the
    // commit the session is established and independent of its launcher.
    if let Some(establishment) = establishment.as_deref_mut() {
        establishment.check_abandonment()?;
    }
    // Record the run row before any service starts, so even a service-less
    // selection (a task tree whose leaves require nothing) leaves durable run
    // evidence for `ps` and recovery. Service transitions require this exact
    // row and never create or repair it themselves.
    record_run_created(&mut registry, run_id, admission, placement)?;
    if preparation.provenance_refreshed {
        diagnostics.write(format_args!(
            "  updated slot provenance from manifest {} (data retained)",
            preparation
                .from_manifest_hash
                .as_deref()
                .unwrap_or("unknown"),
        ));
    }
    if let Some(establishment) = establishment {
        establishment.acknowledge(nixfied_runtime::background::Acknowledgement {
            run_id: run_id.to_owned(),
            run_dir: placement.run_dir.clone(),
            logs_dir: placement.logs_dir.clone(),
        });
    }

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
        state: &identity,
        admission,
        options,
        redactor,
        cancellation,
        run_id,
        run_started,
        registry,
        started: Vec::new(),
        extra_services: Vec::new(),
        direct_selected,
        evidence: RunEvidence::default(),
        diagnostics,
        control,
        presenter,
    };
    macro_rules! finish_run {
        ($error:expr, $extra_services:expr) => {{
            let error = $error;
            session.extra_services = $extra_services;
            return session.finalize(Some(error), None);
        }};
    }
    // The command-owned presenter exists before any workload is released; its
    // establishment failure releases no workload.
    if let Some(mode) = options
        .output_mode
        .presentation()
        .filter(|_| !options.background)
    {
        let init = PresenterInit {
            run_id: run_id.to_owned(),
            run_dir: placement.run_dir.clone(),
            registry_path: placement.registry_path(),
            project_id: manifest.project.project_id.clone(),
            environment: selected_slot.environment.to_owned(),
            slot: i64::from(selected_slot.slot),
            runtime_abi: manifest.runtime_abi.clone(),
            toolchain_id: manifest.toolchain_id.clone(),
            mode,
        };
        match CommandPresenter::spawn(session.registry.authority(), &launcher, &init) {
            Ok(spawned) => *session.presenter = Some(spawned),
            Err(error) => finish_run!(error, Vec::new()),
        }
    }
    for binding in &plan.services {
        let service_name = binding.service.name.as_str();
        session
            .diagnostics
            .write(format_args!("  starting service {service_name}"));

        let prepare_runner: Option<PrepareRunner<'_>> =
            binding.service.prepare.as_ref().map(|_| {
                let context = NodeContext {
                    placement,
                    run: RunContext::new(
                        &launcher,
                        admission,
                        run_id,
                        &placement.state_root,
                        redactor,
                    ),
                    cancellation,
                };
                let evidence = &mut session.evidence;
                let started = &session.started;
                let diagnostics = &mut session.diagnostics;
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
                        )
                        .map_err(|failure| *failure.error)?;
                    }
                    Ok(())
                }) as PrepareRunner<'_>
            });

        let session_checkpoint = || {
            for service in &session.started {
                service.check_liveness()?;
            }
            Ok(())
        };
        let current_service = match start_service_for_slot(
            admission,
            placement,
            &mut session.registry,
            run_id,
            selected_slot,
            ServiceSelection {
                launcher: &launcher,
                service_name,
                endpoint_ports: &binding.endpoint_ports,
                slot_endpoints: &slot_endpoints,
                run_timeout_ms: options.timeout_ms,
                cancellation,
                session_checkpoint: &session_checkpoint,
                prepare_runner,
            },
        ) {
            Ok(service) => service,
            Err(error) => {
                finish_run!(error.with_detail("failedService", service_name), Vec::new());
            }
        };
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
        {
            let name = current_service.service_name();
            let message = match current_service.selected_endpoint() {
                Some(endpoint) => format!(
                    "  service {name} ready at {}:{}",
                    endpoint.host, endpoint.port
                ),
                None => format!("  service {name} ready (endpoint-less)"),
            };
            session.diagnostics.write(message);
        }
        session.started.push(current_service);
    }

    if let Err(error) = cancellation.check() {
        finish_run!(error, Vec::new());
    }

    let context = NodeContext {
        placement,
        run: RunContext::new(
            &launcher,
            admission,
            run_id,
            &placement.state_root,
            redactor,
        ),
        cancellation,
    };
    for (index, node) in plan.nodes.iter().enumerate() {
        if let Err(failure) = execute_node(
            &context,
            &mut session.registry,
            &mut session.evidence,
            &session.started,
            &mut session.diagnostics,
            node,
            NodeRole::Root { direct_selected },
        ) {
            let observed = match failure.observed {
                Some(ExecutionOutcome::Succeeded) if index + 1 == plan.nodes.len() => {
                    Some(ExecutionOutcome::Succeeded)
                }
                Some(ExecutionOutcome::Succeeded) => Some(ExecutionOutcome::Failed),
                outcome => outcome,
            };
            return session.finalize(Some(*failure.error), observed);
        }
    }

    session.finalize(None, Some(ExecutionOutcome::Succeeded))
}

/// A background owner has null stdin; a workload declaring inherited
/// interactive stdin rejects before any workload runs.
fn reject_interactive_stdin(
    plan: &nixfied_runtime::execution::RunPlan<'_>,
) -> Result<(), RuntimeError> {
    let inherits = plan
        .nodes
        .iter()
        .chain(
            plan.services
                .iter()
                .flat_map(|binding| binding.prepare_nodes.iter()),
        )
        .any(|node| node.task.exec.stdin == nixfied_manifest::StdinPolicy::Inherit)
        || plan.services.iter().any(|binding| {
            binding.service.start.exec.stdin == nixfied_manifest::StdinPolicy::Inherit
        });
    if inherits {
        return Err(RuntimeError::new(
            nixfied_runtime::ErrorCode::TaskSelectionInvalid,
            format!("{RUN_DAEMON} cannot run a workload that inherits interactive stdin"),
        ));
    }
    Ok(())
}

fn services_output(services: &[ReadyService]) -> Vec<ServiceRunOutput> {
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

/// The run-owned diagnostic source. While session duties remain, runtime
/// progress lines are retained evidence that the presenter shows on stderr;
/// the owner never writes them to caller streams. The first write failure is
/// kept for finalization and later lines are dropped.
struct SessionDiagnostics {
    file: Option<std::fs::File>,
    enabled: bool,
    failure: Option<RuntimeError>,
}

impl SessionDiagnostics {
    fn create(run_dir: &Path, enabled: bool) -> Result<Self, RuntimeError> {
        use std::os::unix::fs::OpenOptionsExt;
        let path = run_dir.join(nixfied_runtime::output::DIAGNOSTIC_SOURCE);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|error| {
                RuntimeError::new(
                    nixfied_runtime::ErrorCode::StateUnwritable,
                    format!(
                        "failed to create diagnostic source {}: {error}",
                        path.display()
                    ),
                )
            })?;
        Ok(Self {
            file: Some(file),
            enabled,
            failure: None,
        })
    }

    fn write(&mut self, line: impl Display) {
        if !self.enabled || self.failure.is_some() {
            return;
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if let Err(error) = writeln!(file, "{line}") {
            self.failure = Some(diagnostic_error(error));
        }
    }

    /// Close the writer before the output seal; sealed sources never grow.
    fn close(&mut self) -> Result<(), RuntimeError> {
        let closed = match self.file.take() {
            Some(file) => file.sync_all().map_err(diagnostic_error),
            None => Ok(()),
        };
        match self.failure.take() {
            Some(error) => Err(error),
            None => closed,
        }
    }
}

fn diagnostic_error(error: io::Error) -> RuntimeError {
    RuntimeError::new(
        nixfied_runtime::ErrorCode::StateUnwritable,
        format!("failed to write the session diagnostic source: {error}"),
    )
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

#[derive(Debug)]
struct NodeFailure {
    error: Box<RuntimeError>,
    observed: Option<ExecutionOutcome>,
}

impl From<RuntimeError> for NodeFailure {
    fn from(error: RuntimeError) -> Self {
        Self {
            error: Box::new(error),
            observed: None,
        }
    }
}

struct NodeContext<'a> {
    placement: &'a nixfied_runtime::state::HostPlacement,
    run: RunContext<'a>,
    cancellation: &'a CancellationToken,
}

fn execute_node(
    context: &NodeContext<'_>,
    registry: &mut Registry,
    evidence: &mut RunEvidence,
    started: &[ReadyService],
    diagnostics: &mut SessionDiagnostics,
    node: &PlanNode<'_>,
    role: NodeRole,
) -> Result<(), NodeFailure> {
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
    if matches!(role, NodeRole::Prepare) {
        diagnostics.write(format_args!(
            "  prepare node {} ({})",
            node.node_id, task.task_id
        ));
    }
    let presentation = match role {
        NodeRole::Root {
            direct_selected: true,
        } => SourcePresentation::Selected,
        NodeRole::Prepare | NodeRole::Root { .. } => SourcePresentation::Shown,
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
        presentation,
    );
    let (task_run, error) = match result {
        Ok(TaskExecution::Succeeded(completed)) => (completed, None),
        Ok(TaskExecution::Failed { error, evidence }) => (evidence, Some(error)),
        Err(TaskExecutionError::BeforeTerminal(error)) => return Err(decorate(*error).into()),
        Err(TaskExecutionError::ObservedWithoutEvidence { error, outcome }) => {
            return Err(NodeFailure {
                error: Box::new(decorate(*error)),
                observed: Some(outcome),
            });
        }
        Err(TaskExecutionError::AfterTerminal { error, evidence }) => (*evidence, Some(*error)),
    };
    // Checkpoint before settling success: a service exit observed after the
    // task's own exit still fails the node and the session.
    let service_failure = match &error {
        None => started
            .iter()
            .try_for_each(ReadyService::check_liveness)
            .err(),
        Some(_) => None,
    };
    let observed = if task_run.canceled {
        ExecutionOutcome::Canceled
    } else if task_run.success && service_failure.is_none() {
        ExecutionOutcome::Succeeded
    } else {
        ExecutionOutcome::Failed
    };
    let index = EvidenceIndex(evidence.tasks.len());
    evidence.tasks.push(task_run);
    if let NodeRole::Root { direct_selected } = role {
        evidence.root_nodes.push(index);
        if direct_selected {
            evidence.selected_task = Some(index);
        }
        let task_run = &evidence.tasks[index.0];
        if error.is_some() {
            diagnostics.write(format_args!(
                "  fail {} ({}) {} exit={}",
                node.node_id,
                task.task_id,
                human_duration(task_run.duration_ms),
                human_exit_code(task_run.exit_code)
            ));
            diagnostics.write(format_args!(
                "    stderr: {}",
                human_path(&task_run.stderr_path)
            ));
        } else {
            diagnostics.write(format_args!(
                "  ok {} ({}) {}",
                node.node_id,
                task.task_id,
                human_duration(task_run.duration_ms)
            ));
        }
    }
    match (error, service_failure) {
        (Some(error), _) => Err(NodeFailure {
            error: Box::new(decorate(attach_task_evidence(
                error,
                &evidence.tasks[index.0],
            ))),
            observed: Some(observed),
        }),
        (None, Some(error)) => Err(NodeFailure {
            error: Box::new(error),
            observed: Some(observed),
        }),
        (None, None) => Ok(()),
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

fn write_run_footer(
    diagnostics: &mut SessionDiagnostics,
    run_succeeded: bool,
    nodes: &[NodeResult],
    duration_ms: u64,
    run_summary_path: Option<&Path>,
    logs_dir: &Path,
) {
    if run_succeeded {
        diagnostics.write(format_args!(
            "  result: ok {} passed, 0 failed in {}",
            nodes.iter().filter(|node| node.success).count(),
            human_duration(duration_ms)
        ));
    } else if nodes.is_empty() {
        diagnostics.write(format_args!(
            "  result: fail in {}",
            human_duration(duration_ms)
        ));
    } else {
        diagnostics.write(format_args!(
            "  result: fail {} passed, {} failed in {}",
            nodes.iter().filter(|node| node.success).count(),
            nodes.iter().filter(|node| !node.success).count(),
            human_duration(duration_ms)
        ));
    }
    if let Some(path) = run_summary_path {
        diagnostics.write(format_args!("  run-summary: {}", human_path(path)));
    }
    diagnostics.write(format_args!("  logs: {}", human_path(logs_dir)));
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
        let identity = RegistryIdentity::for_slot(
            &manifest.project.project_id,
            selected_slot.environment,
            selected_slot.slot,
            &manifest.runtime_abi,
            &manifest.toolchain_id,
        );
        let state = StateIdentity::from_selected_slot(admission, &selected_slot);
        match command {
            ControlCommand::Ps => {
                let report =
                    match RegistryReader::open_existing(&placement.registry_path(), &identity)? {
                        Some(reader) => nixfied_runtime::control::ps(&reader)?,
                        None => nixfied_runtime::control::PsReport {
                            processes: Vec::new(),
                        },
                    };
                return print_json(&report);
            }
            ControlCommand::Down => {
                return print_json(&nixfied_runtime::control::down(
                    &placement,
                    &identity,
                    &state,
                    options.timeout_ms,
                )?);
            }
            ControlCommand::Clean => {}
        }
        let guard = nixfied_runtime::state::ownership::SlotGuard::acquire(
            &placement,
            &CancellationToken::new(),
        )?;
        let mut registry = Registry::open_or_create(guard, &identity)?;
        let operation = (|| {
            let recovery =
                nixfied_runtime::control::recover_slot(&mut registry, &state, options.timeout_ms)?;
            let cleaned = run_slot_clean(
                admission,
                &mut registry,
                &selected_slot,
                options.cleanup_mode,
            )?;
            // Recovery may already have applied the predecessor's run-scoped
            // retention; report that deletion, not the later absence.
            match (recovery.retention, cleaned) {
                (
                    nixfied_runtime::state::RetentionOutcome::Deleted(deleted),
                    nixfied_runtime::state::CleanupOutcome::Absent { .. },
                ) => print_json(&deleted),
                (_, cleaned) => print_json(&cleaned),
            }
        })();
        nixfied_runtime::error::both(operation, registry.close())
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
    let mut daemon = RUN_DAEMON_INITIAL;
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
            RUN_DAEMON => daemon = true,
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
    if daemon && output_mode.is_some() {
        return Err(RuntimeError::new(
            nixfied_runtime::ErrorCode::OutputModeConflict,
            format!("{RUN_DAEMON} has no output projection; omit {RUN_OUTPUT}"),
        ));
    }
    Ok(ParsedRunOptions {
        daemon,
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
    daemon: bool,
    manifest_path: PathBuf,
    allow_non_store: bool,
    state_base: PathBuf,
    timeout_ms: RunTimeoutMsValue,
    output_mode: Option<RunOutputValue>,
    slot: Option<RuntimeSlotValue>,
    task: Option<String>,
}

impl ParsedRunOptions {
    fn resolve(
        self,
        output_mode: RunOutputValue,
        task: nixfied_manifest::TaskId,
        background: bool,
    ) -> RunOptions {
        RunOptions {
            background,
            state_base: self.state_base,
            timeout_ms: self.timeout_ms,
            output_mode,
            slot: self.slot,
            task,
        }
    }
}

struct RunOptions {
    /// A background owner retains evidence without a terminal presenter.
    background: bool,
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
