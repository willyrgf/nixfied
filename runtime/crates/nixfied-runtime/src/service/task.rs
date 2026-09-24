use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use crate::admission::RunAdmission;
use crate::cancellation::{CancellationToken, canceled_error};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::ExecTask;
use crate::output::{EvidenceMode, ReplayTicket};
use crate::redaction::{LogFileMode, Redactor};
use crate::registry::Registry;
use crate::service::process::{
    CapturedExec, CapturedExecOutcome, ExecSubstitution, SlotEndpoints, StartedService,
    TerminationReason, platform_start_identity, resolve_exec_cwd, spawn_captured_exec,
};
use crate::service::registry::{
    TaskProcessRecord, TaskTerminalStatus, ensure_service_instance_probe_ready, mark_task_finished,
    record_task_canceling, record_task_started,
};
use crate::state::HostPlacement;
use nixfied_manifest::ServiceId;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub task_id: String,
    pub step_path: String,
    pub process_key: String,
    #[serde(default)]
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
}

/// The run-level context a task executes in, independent of any service: the run
/// identity, the codebase root it runs in, and the slot state root. A task may
/// depend on zero services (e.g. a lint/test task) or many (e.g. an e2e test) — a
/// service dependency only adds `${port}`/`${host}` substitution, never the run
/// context.
#[derive(Debug, Clone, Copy)]
pub struct RunContext<'a> {
    admission: &'a RunAdmission,
    run_id: &'a str,
    state_root: &'a Path,
    redactor: &'a Redactor,
}

impl<'a> RunContext<'a> {
    pub fn new(
        admission: &'a RunAdmission,
        run_id: &'a str,
        state_root: &'a Path,
        redactor: &'a Redactor,
    ) -> Self {
        Self {
            admission,
            run_id,
            state_root,
            redactor,
        }
    }
}

/// Run a task gated on the readiness of every service it declares in
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
    occurrence: u64,
    task: &ExecTask,
    cancellation: &CancellationToken,
    evidence: EvidenceMode,
) -> Result<TaskExecution, TaskExecutionError> {
    cancellation.check().map_err(TaskExecutionError::before)?;
    let task_id = task.task_id.as_str();
    ensure_task_dependencies(registry, task, dependencies).map_err(TaskExecutionError::before)?;
    check_services_live(dependencies).map_err(TaskExecutionError::before)?;
    let declared_dependencies: Vec<_> = task
        .requires
        .iter()
        .map(|name| {
            dependencies
                .iter()
                .copied()
                .find(|service| service.service_name() == name.as_str())
                .expect("declared dependency checked above")
        })
        .collect();
    // The first dependency is the primary, providing bare ${port}/${host};
    // every declared dependency is addressable by name via ${port:<serviceId>}
    // and ${host:<serviceId>}. A task with no services runs in the run context
    // alone.
    let endpoint = declared_dependencies
        .first()
        .and_then(|service| service.selected_endpoint());
    // Endpoint-less dependencies are alive while the task runs but contribute
    // nothing addressable; lowering already rejected placeholders toward them.
    let named: SlotEndpoints = declared_dependencies
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
        secrets: run_context.admission.secrets(),
    };
    let exec = &task.exec;
    let stdout_path = placement
        .logs_dir
        .join(format!("task.{occurrence}.stdout.log"));
    let stderr_path = placement
        .logs_dir
        .join(format!("task.{occurrence}.stderr.log"));
    let args = substitution
        .args(&exec.args)
        .map_err(TaskExecutionError::before)?;
    let env = substitution
        .env(&exec.env)
        .map_err(TaskExecutionError::before)?;
    let env = exec.env_with_path(env);
    let command_cwd = resolve_exec_cwd(&run_context.admission.source().observed_root, &exec.cwd)
        .map_err(TaskExecutionError::before)?;
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
    let child = spawn_captured_exec(&CapturedExec {
        executable: &exec.executable,
        args: &args,
        env: &env,
        cwd: &command_cwd,
        stdin: exec.stdin,
        timeout: task.timeout,
        stdout_path: &stdout_path,
        stderr_path: &stderr_path,
        redactor: run_context.redactor,
        log_file_mode: LogFileMode::New,
        label: "task process",
    })
    .map_err(TaskExecutionError::before)?;
    let pid = child.pid();
    // process_group(0) establishes the owned group before the child execs.
    let pgid = pid as i32;
    let process_key = format!("process-{}-task-{node_id}-{pid}-{pgid}", run_context.run_id);
    let start_identity = super::StoredProcessIdentity::encode(
        pid,
        pgid,
        platform_start_identity(pid).as_deref(),
        None,
    );
    if let Err(error) = record_task_started(
        registry,
        &TaskProcessRecord {
            run_id: run_context.run_id,
            process_key: &process_key,
            pid,
            pgid,
            start_identity: &start_identity,
            command_json: &command_json,
            computed_manifest_hash: run_context.admission.common().computed_manifest_hash(),
        },
    ) {
        return Err(TaskExecutionError::before(child.abort(error)));
    }
    let outcome = child
        .complete(
            cancellation,
            || check_services_live(dependencies),
            |reason| {
                record_task_cancellation_intent(
                    registry,
                    &TaskCancellationContext {
                        run_id: run_context.run_id,
                        task_id,
                        process_key: &process_key,
                        computed_manifest_hash: run_context
                            .admission
                            .common()
                            .computed_manifest_hash(),
                    },
                    pgid,
                    match reason {
                        TerminationReason::Canceled => "run canceled",
                        TerminationReason::TimedOut => "task timeout",
                        TerminationReason::ObservationFailed => "session observation failed",
                    },
                )
            },
        )
        .map_err(|failure| {
            let mut error = *failure.error;
            if let Some(outcome) = failure.outcome {
                let (_, terminal) = task_terminal(task, &outcome);
                if let Some(outcome_error) = task_outcome_error(task, &outcome, terminal) {
                    error = error.with_cause(outcome_error);
                }
            }
            TaskExecutionError::before(error)
        })?;
    let duration_ms = elapsed_ms(started);
    let (exit_code, terminal_status) = task_terminal(task, &outcome);
    let success = terminal_status == TaskTerminalStatus::Succeeded;
    let timed_out = terminal_status == TaskTerminalStatus::TimedOut;
    let canceled = terminal_status == TaskTerminalStatus::Canceled;
    let outcome_error = task_outcome_error(task, &outcome, terminal_status);
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
        // Repeated prepares may share a step path; each attempt owns its summary.
        summary_path: placement
            .summary_path
            .with_file_name(format!("summary.{occurrence}.json")),
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
        run_context.admission.common().computed_manifest_hash(),
        terminal_status,
        &payload_json,
    ) {
        return Err(TaskExecutionError::after(error, evidence));
    }
    // The run's evidence (exit code, log and summary paths) already exists;
    // carry it on the error so the failure surface links to it instead of
    // discarding it. A timeout is an execution failure, not an operator
    // cancellation — only a canceled run reports CANCELED.
    match outcome_error {
        None => Ok(TaskExecution::Succeeded(evidence)),
        Some(error) => Ok(TaskExecution::Failed { error, evidence }),
    }
}

fn task_terminal(
    task: &ExecTask,
    outcome: &CapturedExecOutcome,
) -> (Option<i32>, TaskTerminalStatus) {
    match outcome {
        CapturedExecOutcome::Exited(status) => {
            let code = status.code();
            let terminal = if code.is_some_and(|code| task.success_codes.contains(&code)) {
                TaskTerminalStatus::Succeeded
            } else {
                TaskTerminalStatus::Failed
            };
            (code, terminal)
        }
        CapturedExecOutcome::TimedOut => (None, TaskTerminalStatus::TimedOut),
        CapturedExecOutcome::Canceled => (None, TaskTerminalStatus::Canceled),
    }
}

fn task_outcome_error(
    task: &ExecTask,
    outcome: &CapturedExecOutcome,
    terminal: TaskTerminalStatus,
) -> Option<RuntimeError> {
    match terminal {
        TaskTerminalStatus::Succeeded => None,
        TaskTerminalStatus::Canceled => Some(canceled_error()),
        TaskTerminalStatus::TimedOut => Some(RuntimeError::new(
            ErrorCode::TaskFailed,
            format!(
                "task {} timed out after {}ms",
                task.task_id,
                task.timeout
                    .expect("timeout outcome requires a deadline")
                    .as_millis()
            ),
        )),
        TaskTerminalStatus::Failed => {
            let code = match outcome {
                CapturedExecOutcome::Exited(status) => status.code(),
                _ => None,
            };
            Some(RuntimeError::new(
                ErrorCode::TaskFailed,
                format!(
                    "task {} exited with code {}",
                    task.task_id,
                    code.map(|code| code.to_string())
                        .unwrap_or_else(|| "unknown".into())
                ),
            ))
        }
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn check_services_live(services: &[&StartedService]) -> RuntimeResult<()> {
    for service in services {
        service.check_liveness()?;
    }
    Ok(())
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
            &service.info().service_instance_id,
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct TaskCancellationContext<'a> {
    run_id: &'a str,
    task_id: &'a str,
    process_key: &'a str,
    computed_manifest_hash: &'a str,
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
        context.computed_manifest_hash,
        &payload,
    )
}

fn write_summary(run: &TaskRun, redactor: &Redactor) -> RuntimeResult<()> {
    let mut summary = serde_json::to_value(run)
        .map_err(|error| RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string()))?;
    redactor.redact_value(&mut summary);
    let summary = serde_json::to_vec_pretty(&summary)
        .map_err(|error| RuntimeError::new(ErrorCode::LifecycleFailed, error.to_string()))?;
    std::fs::File::create_new(&run.summary_path)
        .and_then(|mut file| file.write_all(&summary))
        .map_err(|error| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::RegistryIdentity;
    use std::time::Duration;

    #[test]
    fn cancellation_recording_failure_still_contains_and_reaps_child() {
        let root = std::env::temp_dir().join(format!(
            "nixfied-task-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut registry = Registry::open_or_create(
            root.join("registry.sqlite3"),
            &RegistryIdentity::default_slot("test", "abi", "toolchain"),
        )
        .unwrap();
        registry
            .connection()
            .execute("DROP TABLE events", [])
            .unwrap();
        let child = spawn_captured_exec(&CapturedExec {
            executable: &std::env::var("NIXFIED_TEST_SLEEP").unwrap(),
            args: &["30".into()],
            env: &std::collections::BTreeMap::new(),
            cwd: &root,
            stdin: nixfied_manifest::StdinPolicy::Null,
            timeout: Some(Duration::from_secs(5)),
            stdout_path: &root.join("stdout"),
            stderr_path: &root.join("stderr"),
            redactor: &Redactor::empty(),
            log_file_mode: LogFileMode::New,
            label: "task process",
        })
        .unwrap();
        let pgid = child.pid() as i32;
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let failure = child
            .complete(
                &cancellation,
                || Ok(()),
                |_| {
                    assert_eq!(
                        unsafe { libc::kill(pgid, 0) },
                        0,
                        "intent precedes termination"
                    );
                    record_task_cancellation_intent(
                        &mut registry,
                        &TaskCancellationContext {
                            run_id: "test",
                            task_id: "task",
                            process_key: "process",
                            computed_manifest_hash: "hash",
                        },
                        pgid,
                        "task canceled",
                    )
                },
            )
            .err()
            .unwrap();
        let exited = unsafe { libc::kill(pgid, 0) } == -1;
        assert!(matches!(
            failure.outcome,
            Some(CapturedExecOutcome::Canceled)
        ));
        drop(registry);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(failure.error.code, ErrorCode::RegistryCorrupt);
        assert!(
            exited,
            "failed intent recording must not bypass owned child containment"
        );
    }
}
