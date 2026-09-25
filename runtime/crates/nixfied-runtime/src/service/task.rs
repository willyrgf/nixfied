use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use crate::admission::RunAdmission;
use crate::cancellation::{CancellationToken, canceled_error};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::ExecTask;
use crate::output::{EvidenceSource, SourcePresentation};
use crate::redaction::CaptureOutcome;
use crate::redaction::{LogFileMode, Redactor};
use crate::registry::Registry;
use crate::service::process::{
    CapturedExec, CapturedExecOutcome, ExecSubstitution, Invocation, InvocationFailure,
    ReadyService, SlotEndpoints, TerminationReason, check_services_live, resolve_exec_cwd,
    spawn_gated_captured_exec,
};
use crate::service::registry::{
    InvocationOwner, TaskTerminalStatus, ensure_service_instance_probe_ready,
    mark_invocation_finished,
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
pub enum TaskExecution {
    Succeeded(TaskRun),
    Failed {
        error: RuntimeError,
        evidence: TaskRun,
    },
}

#[derive(Debug)]
pub enum TaskExecutionError {
    BeforeTerminal(Box<RuntimeError>),
    ObservedWithoutEvidence {
        error: Box<RuntimeError>,
        outcome: crate::registry::session::ExecutionOutcome,
    },
    AfterTerminal {
        error: Box<RuntimeError>,
        evidence: Box<TaskRun>,
    },
}

impl TaskExecutionError {
    fn before(error: RuntimeError) -> Self {
        Self::BeforeTerminal(Box::new(error))
    }

    fn after(error: RuntimeError, evidence: TaskRun) -> Self {
        Self::AfterTerminal {
            error: Box::new(error),
            evidence: Box::new(evidence),
        }
    }

    pub fn error(&self) -> &RuntimeError {
        match self {
            Self::BeforeTerminal(error)
            | Self::AfterTerminal { error, .. }
            | Self::ObservedWithoutEvidence { error, .. } => error,
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
    launcher: &'a Path,
    admission: &'a RunAdmission,
    run_id: &'a str,
    state_root: &'a Path,
    redactor: &'a Redactor,
}

impl<'a> RunContext<'a> {
    pub fn new(
        launcher: &'a Path,
        admission: &'a RunAdmission,
        run_id: &'a str,
        state_root: &'a Path,
        redactor: &'a Redactor,
    ) -> Self {
        Self {
            launcher,
            admission,
            run_id,
            state_root,
            redactor,
        }
    }
}

/// Run a task gated on the readiness of every service it requires (which may
/// be none) among the session's `started` services, all of which it observes.
/// The run-level context comes from `run_context`; the first required service,
/// if any, is the primary that provides `${port}`/`${host}` substitution.
#[allow(clippy::too_many_arguments)]
pub fn run_dependent_task_cancellable(
    placement: &HostPlacement,
    registry: &mut Registry,
    run_context: RunContext<'_>,
    started: &[&ReadyService],
    node_id: &str,
    occurrence: u64,
    task: &ExecTask,
    cancellation: &CancellationToken,
    presentation: SourcePresentation,
) -> Result<TaskExecution, TaskExecutionError> {
    cancellation.check().map_err(TaskExecutionError::before)?;
    let task_id = task.task_id.as_str();
    let declared_dependencies =
        required_services(registry, task, started).map_err(TaskExecutionError::before)?;
    check_services_live(started.iter().copied()).map_err(TaskExecutionError::before)?;
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
    let evidence = EvidenceSource::in_logs(
        &placement.logs_dir,
        node_id,
        presentation,
        &format!("task.{occurrence}"),
    );
    let stdout_path = evidence.stdout.clone();
    let stderr_path = evidence.stderr.clone();
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
    let manifest_hash = run_context.admission.common().computed_manifest_hash();
    let invocation = Invocation {
        owner: InvocationOwner::Task,
        run_id: run_context.run_id,
        manifest_hash,
        source: &evidence,
        command_json: &command_json,
        terminal: &|outcome| task_terminal(task, outcome),
        canceling: &|pgid, reason| {
            serde_json::json!({
                "taskId": task_id,
                "pgid": pgid,
                "reason": match reason {
                    TerminationReason::Canceled => "run canceled",
                    TerminationReason::TimedOut => "task timeout",
                    TerminationReason::ObservationFailed => "session observation failed",
                },
            })
            .to_string()
        },
    };
    let began = Instant::now();
    let pending = spawn_gated_captured_exec(
        &CapturedExec {
            authority: registry.authority(),
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
        },
        run_context.launcher,
    )
    .map_err(TaskExecutionError::before)?;
    let (process_key, outcome) = invocation
        .run(
            registry,
            pending,
            // The gate's own process group has the leader's pid as its id.
            |pid| format!("process-{}-task-{node_id}-{pid}-{pid}", run_context.run_id),
            cancellation,
            &mut || check_services_live(started.iter().copied()),
        )
        .map_err(|failure| task_failure(task, failure))?;
    let duration_ms = elapsed_ms(began);
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
    let evidence = run;
    if let Err(error) = write_summary(&evidence, run_context.redactor) {
        return Err(TaskExecutionError::after(error, evidence));
    }
    let mut payload_value = match serde_json::to_value(&evidence) {
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
    if let Err(error) = mark_invocation_finished(
        registry,
        invocation.identity(&process_key),
        terminal_status,
        &payload_json,
        Some(CaptureOutcome::Complete),
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

/// A failed task invocation. An outcome observed before the failure keeps
/// its execution result and outcome error as evidence.
fn task_failure(task: &ExecTask, failure: InvocationFailure) -> TaskExecutionError {
    let InvocationFailure { error, outcome } = failure;
    let Some(outcome) = outcome else {
        return TaskExecutionError::BeforeTerminal(error);
    };
    let mut error = *error;
    let (_, terminal) = task_terminal(task, &outcome);
    if let Some(outcome_error) = task_outcome_error(task, &outcome, terminal) {
        error = error.with_cause(outcome_error);
    }
    TaskExecutionError::ObservedWithoutEvidence {
        error: Box::new(error),
        outcome: terminal.execution_outcome(),
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

/// Resolve every service the task requires among the started services, each
/// in a task-ready state.
fn required_services<'s>(
    registry: &Registry,
    task: &ExecTask,
    started: &[&'s ReadyService],
) -> RuntimeResult<Vec<&'s ReadyService>> {
    task.requires
        .iter()
        .map(|name| {
            let service = started
                .iter()
                .copied()
                .find(|service| service.service_name() == name.as_str())
                .ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::DependencyUnavailable,
                        format!(
                            "task {} requires service {name}, which is not among the started services",
                            task.task_id
                        ),
                    )
                })?;
            ensure_service_instance_probe_ready(
                registry,
                name.as_str(),
                &service.info().service_instance_id,
            )?;
            Ok(service)
        })
        .collect()
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
        let root = crate::test_support::TestDir::new("task-cancel");
        let mut registry = Registry::open_or_create(
            crate::state::ownership::fixture_guard(
                &root,
                &RegistryIdentity::default_slot("test", "abi", "toolchain"),
            ),
            &RegistryIdentity::default_slot("test", "abi", "toolchain"),
        )
        .unwrap();
        registry
            .connection()
            .execute("DROP TABLE events", [])
            .unwrap();
        let child = crate::service::process::spawn_gated_captured_exec(
            &CapturedExec {
                authority: registry.authority(),
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
            },
            &crate::launch::test_launcher(),
        )
        .unwrap()
        .register_and_release(|_| Ok(()), || Ok(()))
        .unwrap_or_else(|_| panic!("the captured child should be released"));
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
                    crate::service::registry::record_invocation_canceling(
                        &mut registry,
                        crate::service::registry::InvocationIdentity {
                            run_id: "test",
                            process_key: "process",
                            manifest_hash: "hash",
                            owner: InvocationOwner::Task,
                        },
                        "{}",
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
        assert_eq!(failure.error.code, ErrorCode::RegistryCorrupt);
        assert!(
            exited,
            "failed intent recording must not bypass owned child containment"
        );
    }
}
