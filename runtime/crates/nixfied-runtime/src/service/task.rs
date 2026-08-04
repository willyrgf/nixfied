use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::admission::secrets::ResolvedSecrets;
use crate::cancellation::{CancellationToken, canceled_error};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{ExecTask, ResolvedInvocation};
use crate::output::{EvidenceMode, ReplayTicket};
use crate::redaction::{RedactedLogRelays, Redactor, child_output};
use crate::registry::Registry;
use crate::service::process::{
    ExecSubstitution, SlotEndpoints, StartedService, platform_start_identity, process_group,
    resolve_exec_cwd, terminate_process_group, wait_for_child_exit,
};
use crate::service::registry::{
    TaskProcessRecord, TaskTerminalStatus, ensure_service_instance_probe_ready, mark_task_finished,
    record_task_canceling, record_task_started,
};
use crate::state::HostPlacement;
use nixfied_model::ServiceId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub task_id: String,
    /// Evidence identity: the flattened step path of the node that ran this
    /// leaf (`<root>.<step>...` for composite selections, the task id for a
    /// direct leaf run). Logs, summaries, and the registry process row key by
    /// it, so the same leaf referenced twice leaves distinct evidence.
    pub step_path: String,
    pub process_key: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub canceled: bool,
    pub success: bool,
    pub duration_ms: u64,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub summary_path: PathBuf,
}

#[derive(Debug)]
pub enum CompletedEvidence {
    Captured(TaskRun),
    Replayable { task: TaskRun, ticket: ReplayTicket },
}

impl CompletedEvidence {
    pub fn task_run(&self) -> &TaskRun {
        match self {
            Self::Captured(task) | Self::Replayable { task, .. } => task,
        }
    }

    pub fn into_task_and_replay(self) -> (TaskRun, Option<ReplayTicket>) {
        match self {
            Self::Captured(task) => (task, None),
            Self::Replayable { task, ticket } => (task, Some(ticket)),
        }
    }
}

#[derive(Debug)]
pub enum TaskExecution {
    Succeeded(CompletedEvidence),
    Failed {
        error: RuntimeError,
        evidence: CompletedEvidence,
    },
}

#[derive(Debug)]
pub enum TaskExecutionError {
    BeforeTerminal(Box<RuntimeError>),
    AfterTerminal {
        error: Box<RuntimeError>,
        evidence: Box<CompletedEvidence>,
    },
}

impl TaskExecutionError {
    fn before(error: RuntimeError) -> Self {
        Self::BeforeTerminal(Box::new(error))
    }

    fn after(error: RuntimeError, evidence: CompletedEvidence) -> Self {
        Self::AfterTerminal {
            error: Box::new(error),
            evidence: Box::new(evidence),
        }
    }

    pub fn error(&self) -> &RuntimeError {
        match self {
            Self::BeforeTerminal(error) | Self::AfterTerminal { error, .. } => error,
        }
    }

    pub fn into_error_and_evidence(self) -> (RuntimeError, Option<CompletedEvidence>) {
        match self {
            Self::BeforeTerminal(error) => (*error, None),
            Self::AfterTerminal { error, evidence } => (*error, Some(*evidence)),
        }
    }
}

/// Typed evidence retained when a service prepare task fails. The service
/// lifecycle owns the reservation failure, while the run driver owns the
/// completed task evidence for summaries and public error details.
#[derive(Debug)]
pub struct PrepareTaskError {
    error: Box<RuntimeError>,
    task_runs: Vec<TaskRun>,
}

impl PrepareTaskError {
    pub fn new(error: RuntimeError, task_runs: Vec<TaskRun>) -> Self {
        Self {
            error: Box::new(error),
            task_runs,
        }
    }

    pub fn into_parts(self) -> (RuntimeError, Vec<TaskRun>) {
        (*self.error, self.task_runs)
    }
}

/// The run-level context a task executes in, independent of any service: the run
/// identity, the codebase root it runs in, and the slot state root. A task may
/// depend on zero services (e.g. a lint/test task) or many (e.g. an e2e test) — a
/// service dependency only adds `${port}`/`${host}` substitution, never the run
/// context.
#[derive(Debug, Clone, Copy)]
pub struct RunContext<'a> {
    pub run_id: &'a str,
    pub computed_model_hash: &'a str,
    pub source_root: &'a Path,
    pub state_root: &'a Path,
    pub secrets: &'a ResolvedSecrets,
    pub redactor: &'a Redactor,
}

impl<'a> RunContext<'a> {
    /// The run context as carried by an already-started service (every service in
    /// a run shares it).
    pub fn from_service(service: &'a StartedService) -> Self {
        Self {
            run_id: &service.run_id,
            computed_model_hash: &service.computed_model_hash,
            source_root: &service.source_root,
            state_root: &service.state_root,
            secrets: &service.secrets,
            redactor: &service.redactor,
        }
    }
}

pub fn run_dependent_task(
    placement: &HostPlacement,
    registry: &mut Registry,
    run_context: RunContext<'_>,
    dependencies: &[&StartedService],
    task: &ExecTask,
) -> Result<TaskExecution, TaskExecutionError> {
    // A directly selected leaf's step path is the task id.
    run_dependent_task_cancellable(
        placement,
        registry,
        run_context,
        dependencies,
        task.task_id.as_str(),
        task,
        &CancellationToken::new(),
        EvidenceMode::CaptureOnly,
    )
}

/// Run a bounded task gated on the readiness of every service it declares in
/// `dependsOnServicesReady` (which may be empty). The run-level context comes from
/// `run_context`; the first dependency, if any, is the primary that provides
/// `${port}`/`${host}` substitution.
#[allow(clippy::too_many_arguments)]
pub fn run_dependent_task_cancellable(
    placement: &HostPlacement,
    registry: &mut Registry,
    run_context: RunContext<'_>,
    dependencies: &[&StartedService],
    node_id: &str,
    task: &ExecTask,
    cancellation: &CancellationToken,
    evidence: EvidenceMode,
) -> Result<TaskExecution, TaskExecutionError> {
    run_dependent_task_with_evidence(
        placement,
        registry,
        run_context,
        dependencies,
        node_id,
        task,
        cancellation,
        evidence,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_dependent_task_with_evidence(
    placement: &HostPlacement,
    registry: &mut Registry,
    run_context: RunContext<'_>,
    dependencies: &[&StartedService],
    node_id: &str,
    task: &ExecTask,
    cancellation: &CancellationToken,
    evidence: EvidenceMode,
) -> Result<TaskExecution, TaskExecutionError> {
    cancellation.check().map_err(TaskExecutionError::before)?;
    let task_id = task.task_id.as_str();
    ensure_task_dependencies(registry, task, dependencies).map_err(TaskExecutionError::before)?;
    // The first dependency is the primary, providing bare ${port}/${host};
    // every declared dependency is addressable by name via ${port:<serviceId>}
    // and ${host:<serviceId>}. A task with no services runs in the run context
    // alone.
    let endpoint = dependencies
        .first()
        .and_then(|service| service.selected_endpoint());
    // Endpoint-less dependencies are alive while the task runs but contribute
    // nothing addressable; lowering already rejected placeholders toward them.
    let named: SlotEndpoints = dependencies
        .iter()
        .filter_map(|service| {
            Some((
                ServiceId::new(service.service_name()),
                service.selected_endpoint().cloned()?,
            ))
        })
        .collect();
    // A task binds no endpoints of its own, so the `${port:<name>}` namespace is
    // its declared dependencies only.
    let own_endpoints = std::collections::BTreeMap::new();
    let substitution = ExecSubstitution {
        own_primary: endpoint,
        own_endpoints: &own_endpoints,
        named: &named,
        state_root: run_context.state_root,
        secrets: run_context.secrets,
    };
    let exec = &task.exec;
    // Key logs by step path, not task id: a composite may run the same leaf in
    // more than one step, and task-id-keyed paths would overwrite each other's
    // logs.
    let stdout_path = placement
        .logs_dir
        .join(format!("task.{node_id}.stdout.log"));
    let stderr_path = placement
        .logs_dir
        .join(format!("task.{node_id}.stderr.log"));
    let args = substitution
        .args(&exec.args)
        .map_err(TaskExecutionError::before)?;
    let env = substitution
        .env(&exec.env)
        .map_err(TaskExecutionError::before)?;
    let env = exec.env_with_path(env);
    let command_cwd =
        resolve_exec_cwd(run_context.source_root, &exec.cwd).map_err(TaskExecutionError::before)?;
    let command_json = serde_json::to_string(&TaskCommandRecord {
        task_id,
        executable: exec.executable.as_str(),
        args: &args,
        cwd: command_cwd.as_path(),
        stdout_path: stdout_path.as_path(),
        stderr_path: stderr_path.as_path(),
    })
    .map_err(|error| {
        TaskExecutionError::before(RuntimeError::new(
            ErrorCode::LifecycleFailed,
            error.to_string(),
        ))
    })?;
    cancellation.check().map_err(TaskExecutionError::before)?;
    let started = Instant::now();
    let mut child = spawn_task(
        exec,
        &args,
        &env,
        &command_cwd,
        &stdout_path,
        &stderr_path,
        run_context.redactor,
    )
    .map_err(TaskExecutionError::before)?;
    let pid = child.child.id();
    let pgid = match process_group(pid) {
        Ok(Some(pgid)) => pgid,
        Ok(None) => {
            return Err(TaskExecutionError::before(cleanup_unrecorded_task(
                child,
                RuntimeError::new(ErrorCode::ProcEscape, "task process disappeared"),
            )));
        }
        Err(error) => {
            return Err(TaskExecutionError::before(cleanup_unrecorded_task(
                child, error,
            )));
        }
    };
    let process_key = format!("process-{}-task-{node_id}-{pid}-{pgid}", run_context.run_id);
    let start_identity = process_start_identity(pid, pgid, platform_start_identity(pid).as_deref());
    if let Err(error) = record_task_started(
        registry,
        &TaskProcessRecord {
            run_id: run_context.run_id,
            process_key: &process_key,
            pid,
            pgid,
            start_identity: &start_identity,
            command_json: &command_json,
            computed_model_hash: run_context.computed_model_hash,
        },
    ) {
        let termination_error = terminate_process_group(pgid, 1000).err();
        let wait_error = match wait_for_child_exit(&mut child.child, 1000) {
            Ok(true) => None,
            Ok(false) => match child.child.kill() {
                Ok(()) if matches!(wait_for_child_exit(&mut child.child, 1000), Ok(true)) => None,
                Ok(()) => Some(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    "task child did not exit after containment or direct kill",
                )),
                Err(kill_error) => Some(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    format!("task child did not exit after containment: {kill_error}"),
                )),
            },
            Err(error) => Some(error),
        };
        let error = termination_error
            .into_iter()
            .chain(wait_error)
            .fold(error, |error, cleanup| error.with_cause(cleanup));
        return Err(TaskExecutionError::before(error));
    }
    let outcome = wait_for_task(
        registry,
        &mut child.child,
        pgid,
        exec.timeout.as_millis() as u64,
        cancellation,
        TaskCancellationContext {
            run_id: run_context.run_id,
            task_id,
            process_key: &process_key,
            computed_model_hash: run_context.computed_model_hash,
        },
    )
    .map_err(TaskExecutionError::before)?;
    child.logs.join().map_err(TaskExecutionError::before)?;
    let duration_ms = elapsed_ms(started);
    let canceled = outcome.canceled;
    let timed_out = outcome.timed_out;
    let success = !canceled
        && !timed_out
        && outcome
            .exit_code
            .map(|code| task.success_codes.contains(&code))
            .unwrap_or(false);
    let exit_code = outcome.exit_code;
    let failure_message = if timed_out {
        format!(
            "task {task_id} timed out after {}ms",
            exec.timeout.as_millis()
        )
    } else {
        format!(
            "task {task_id} exited with code {}",
            exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )
    };
    let run = TaskRun {
        task_id: task_id.to_string(),
        step_path: node_id.to_string(),
        process_key: process_key.clone(),
        exit_code,
        timed_out,
        canceled,
        success,
        duration_ms,
        stdout_path,
        stderr_path,
        // Key the summary by step path like the logs: a run's nodes share
        // the run dir, and a single run-level summary.json would be
        // overwritten by each node, losing per-node evidence.
        summary_path: placement
            .summary_path
            .with_file_name(format!("summary.{node_id}.json")),
    };
    let evidence = match evidence {
        EvidenceMode::CaptureOnly => CompletedEvidence::Captured(run),
        EvidenceMode::ReplaySelected => CompletedEvidence::Replayable {
            ticket: ReplayTicket::open(&run.stdout_path, &run.stderr_path),
            task: run,
        },
    };
    if let Err(error) = write_summary(evidence.task_run(), run_context.redactor) {
        return Err(TaskExecutionError::after(error, evidence));
    }
    let mut payload_value = match serde_json::to_value(evidence.task_run()) {
        Ok(value) => value,
        Err(error) => {
            return Err(TaskExecutionError::after(
                RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string()),
                evidence,
            ));
        }
    };
    run_context.redactor.redact_value(&mut payload_value);
    let payload_json = match serde_json::to_string(&payload_value) {
        Ok(value) => value,
        Err(error) => {
            return Err(TaskExecutionError::after(
                RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string()),
                evidence,
            ));
        }
    };
    if let Err(error) = mark_task_finished(
        registry,
        run_context.run_id,
        &process_key,
        run_context.computed_model_hash,
        task_terminal_status(success, timed_out, canceled),
        &payload_json,
    ) {
        return Err(TaskExecutionError::after(error, evidence));
    }
    // The run's evidence (exit code, log and summary paths) already exists;
    // carry it on the error so the failure surface links to it instead of
    // discarding it. A timeout is an execution failure, not an operator
    // cancellation — only a canceled run reports CANCELED.
    if success {
        Ok(TaskExecution::Succeeded(evidence))
    } else if canceled {
        Ok(TaskExecution::Failed {
            error: RuntimeError::new(ErrorCode::Canceled, canceled_error().message),
            evidence,
        })
    } else {
        Ok(TaskExecution::Failed {
            error: RuntimeError::new(ErrorCode::TaskFailed, failure_message),
            evidence,
        })
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn task_terminal_status(success: bool, timed_out: bool, canceled: bool) -> TaskTerminalStatus {
    if success {
        TaskTerminalStatus::Succeeded
    } else if canceled {
        TaskTerminalStatus::Canceled
    } else if timed_out {
        TaskTerminalStatus::TimedOut
    } else {
        TaskTerminalStatus::Failed
    }
}

/// Verify every service the task declares as a dependency is among the started
/// services and in a task-ready state. A task may depend on more than one service.
fn ensure_task_dependencies(
    registry: &Registry,
    task: &ExecTask,
    dependencies: &[&StartedService],
) -> RuntimeResult<()> {
    for service_name in &task.requires {
        let service = dependencies
            .iter()
            .find(|service| service.service_name() == service_name.as_str())
            .ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::DependencyUnavailable,
                    format!("task dependency {service_name} was not among the started services"),
                )
            })?;
        ensure_service_instance_probe_ready(
            registry,
            service_name.as_str(),
            &service.service_instance_id,
        )?;
    }
    Ok(())
}

fn spawn_task(
    exec: &ResolvedInvocation,
    args: &[String],
    env: &std::collections::BTreeMap<String, String>,
    command_cwd: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
    redactor: &Redactor,
) -> RuntimeResult<SpawnedTask> {
    let output = child_output(stdout_path, stderr_path, redactor)?;
    let mut command = Command::new(&exec.executable);
    // Hermetic child environment: declared env + runtime-owned PATH only.
    command
        .env_clear()
        .args(args)
        .current_dir(command_cwd)
        .envs(env)
        .stdin(crate::service::process::stdin_for(exec.stdin))
        .stdout(output.stdout)
        .stderr(output.stderr);
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let child = command.spawn().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to spawn task process: {error}"),
        )
    })?;
    Ok(SpawnedTask {
        child,
        logs: output.relays,
    })
}

struct SpawnedTask {
    child: Child,
    logs: RedactedLogRelays,
}

fn cleanup_unrecorded_task(mut task: SpawnedTask, mut error: RuntimeError) -> RuntimeError {
    match task.child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => {
            if let Err(kill_error) = task.child.kill() {
                error = error.with_cause(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    format!("failed to kill unrecorded task process: {kill_error}"),
                ));
            } else if !matches!(wait_for_child_exit(&mut task.child, 1000), Ok(true)) {
                error = error.with_cause(RuntimeError::new(
                    ErrorCode::ProcEscape,
                    "unrecorded task process did not exit after kill",
                ));
            }
        }
        Err(wait_error) => {
            error = error.with_cause(RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect unrecorded task process: {wait_error}"),
            ));
        }
    }
    if let Err(relay_error) = task.logs.join() {
        error = error.with_cause(relay_error);
    }
    error
}

fn wait_for_task(
    registry: &mut Registry,
    child: &mut Child,
    pgid: i32,
    timeout_ms: u64,
    cancellation: &CancellationToken,
    context: TaskCancellationContext<'_>,
) -> RuntimeResult<TaskOutcome> {
    let timeout = Duration::from_millis(timeout_ms);
    let deadline = Instant::now() + timeout;
    loop {
        if cancellation.is_canceled() {
            record_task_cancellation_intent(registry, &context, pgid, "run canceled")?;
            terminate_process_group(pgid, 1000)?;
            require_child_exit(child)?;
            return Ok(TaskOutcome {
                exit_code: None,
                timed_out: false,
                canceled: true,
            });
        }
        if let Some(status) = child.try_wait().map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect task process: {error}"),
            )
        })? {
            // The direct child has exited, but a bounded task may have spawned
            // children into its own process group. Reconcile the owned group so a
            // task that daemonizes and exits 0 cannot leave processes behind,
            // matching the containment services enforce.
            terminate_process_group(pgid, 1000)?;
            return Ok(TaskOutcome {
                exit_code: status.code(),
                timed_out: false,
                canceled: false,
            });
        }
        if Instant::now() >= deadline {
            record_task_cancellation_intent(registry, &context, pgid, "task timeout")?;
            terminate_process_group(pgid, 1000)?;
            require_child_exit(child)?;
            return Ok(TaskOutcome {
                exit_code: None,
                timed_out: true,
                canceled: false,
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn require_child_exit(child: &mut Child) -> RuntimeResult<()> {
    match wait_for_child_exit(child, 1000)? {
        true => Ok(()),
        false => Err(RuntimeError::new(
            ErrorCode::ProcEscape,
            "task child did not exit after cancellation or timeout containment",
        )),
    }
}

#[derive(Debug, Clone, Copy)]
struct TaskCancellationContext<'a> {
    run_id: &'a str,
    task_id: &'a str,
    process_key: &'a str,
    computed_model_hash: &'a str,
}

fn record_task_cancellation_intent(
    registry: &mut Registry,
    context: &TaskCancellationContext<'_>,
    pgid: i32,
    reason: &str,
) -> RuntimeResult<()> {
    let payload = serde_json::json!({
        "taskId": context.task_id,
        "pgid": pgid,
        "reason": reason,
    })
    .to_string();
    record_task_canceling(
        registry,
        context.run_id,
        context.process_key,
        context.computed_model_hash,
        &payload,
    )
}

struct TaskOutcome {
    exit_code: Option<i32>,
    timed_out: bool,
    canceled: bool,
}

fn process_start_identity(pid: u32, pgid: i32, platform_start: Option<&str>) -> String {
    let observed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    serde_json::json!({
        "pid": pid,
        "pgid": pgid,
        "platformStart": platform_start,
        "observedAtNanos": observed_at,
    })
    .to_string()
}

fn write_summary(run: &TaskRun, redactor: &Redactor) -> RuntimeResult<()> {
    let mut summary = serde_json::to_value(run)
        .map_err(|error| RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string()))?;
    redactor.redact_value(&mut summary);
    let summary = serde_json::to_vec_pretty(&summary)
        .map_err(|error| RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string()))?;
    std::fs::write(&run.summary_path, summary).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!(
                "failed to write task summary {}: {error}",
                run.summary_path.display()
            ),
        )
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskCommandRecord<'a> {
    task_id: &'a str,
    executable: &'a str,
    args: &'a [String],
    cwd: &'a Path,
    stdout_path: &'a Path,
    stderr_path: &'a Path,
}
