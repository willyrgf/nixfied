use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::cancellation::{CancellationToken, canceled_error};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{ExecTask, ResolvedExec};
use crate::registry::Registry;
use crate::service::process::{
    SelectedEndpoint, StartedService, platform_start_identity, process_group, resolve_exec_cwd,
    substitute_arg, terminate_process_group, wait_for_child_exit,
};
use crate::service::registry::{
    TaskProcessRecord, TaskTerminalStatus, ensure_service_instance_probe_ready, mark_task_finished,
    record_task_canceling, record_task_started,
};
use crate::state::HostPlacement;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub task_id: String,
    pub process_key: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub canceled: bool,
    pub success: bool,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub summary_path: PathBuf,
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
        }
    }
}

pub fn run_dependent_task(
    placement: &HostPlacement,
    registry: &mut Registry,
    run_context: RunContext<'_>,
    dependencies: &[&StartedService],
    task: &ExecTask,
) -> RuntimeResult<TaskRun> {
    // Outside a workflow the node id is the task id.
    run_dependent_task_cancellable(
        placement,
        registry,
        run_context,
        dependencies,
        &task.task_id,
        task,
        &CancellationToken::new(),
    )
}

/// Run a bounded task gated on the readiness of every service it declares in
/// `dependsOnServicesReady` (which may be empty). The run-level context comes from
/// `run_context`; the first dependency, if any, is the primary that provides
/// `${port}`/`${host}` substitution.
pub fn run_dependent_task_cancellable(
    placement: &HostPlacement,
    registry: &mut Registry,
    run_context: RunContext<'_>,
    dependencies: &[&StartedService],
    node_id: &str,
    task: &ExecTask,
    cancellation: &CancellationToken,
) -> RuntimeResult<TaskRun> {
    cancellation.check()?;
    let task_id = task.task_id.as_str();
    ensure_task_dependencies(registry, task, dependencies)?;
    // A service dependency provides only the endpoint to substitute; a task with
    // no services runs in the run context alone.
    let endpoint = dependencies.first().map(|service| &service.selected_endpoint);
    let exec = &task.exec;
    // Key logs by node id, not task id: a workflow may run the same task in more
    // than one node, and task-id-keyed paths would overwrite each other's logs.
    let stdout_path = placement
        .logs_dir
        .join(format!("task.{node_id}.stdout.log"));
    let stderr_path = placement
        .logs_dir
        .join(format!("task.{node_id}.stderr.log"));
    let args = task_args(exec, endpoint, run_context.state_root);
    let command_cwd = resolve_exec_cwd(run_context.source_root, &exec.cwd)?;
    let command_json = serde_json::to_string(&TaskCommandRecord {
        task_id,
        executable: exec.executable.as_str(),
        args: &args,
        cwd: command_cwd.as_path(),
        stdout_path: stdout_path.as_path(),
        stderr_path: stderr_path.as_path(),
    })
    .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    cancellation.check()?;
    let mut child = spawn_task(exec, &args, &command_cwd, &stdout_path, &stderr_path)?;
    let pid = child.id();
    let pgid = process_group(pid)?
        .ok_or_else(|| RuntimeError::new(ErrorCode::ProcEscape, "task process disappeared"))?;
    let process_key = format!("process-{}-task-{task_id}-{pid}-{pgid}", run_context.run_id);
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
        let _ = terminate_process_group(pgid, 1000);
        let _ = child.wait();
        return Err(error);
    }
    let outcome = wait_for_task(
        registry,
        &mut child,
        pgid,
        exec.timeout.as_millis() as u64,
        cancellation,
        TaskCancellationContext {
            run_id: run_context.run_id,
            task_id,
            process_key: &process_key,
            computed_model_hash: run_context.computed_model_hash,
        },
    )?;
    let canceled = outcome.timed_out || outcome.canceled;
    let success = !canceled
        && outcome
            .exit_code
            .map(|code| task.success_codes.contains(&code))
            .unwrap_or(false);
    let exit_code = outcome.exit_code;
    let timed_out = outcome.timed_out;
    let failure_message = if timed_out {
        format!("task {task_id} timed out after {}ms", exec.timeout.as_millis())
    } else if outcome.canceled {
        "run was canceled".to_string()
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
        process_key: process_key.clone(),
        exit_code,
        timed_out,
        canceled,
        success,
        stdout_path,
        stderr_path,
        summary_path: placement.summary_path.clone(),
    };
    write_summary(&run)?;
    let payload_json = serde_json::to_string(&run)
        .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    mark_task_finished(
        registry,
        run_context.run_id,
        &process_key,
        run_context.computed_model_hash,
        task_terminal_status(success, canceled),
        &payload_json,
    )?;
    if success {
        Ok(run)
    } else if canceled {
        let message = if timed_out {
            failure_message
        } else {
            canceled_error().message
        };
        Err(RuntimeError::new(ErrorCode::Canceled, message))
    } else {
        Err(RuntimeError::new(ErrorCode::TaskFailed, failure_message))
    }
}

fn task_terminal_status(success: bool, canceled: bool) -> TaskTerminalStatus {
    if success {
        TaskTerminalStatus::Succeeded
    } else if canceled {
        TaskTerminalStatus::Canceled
    } else {
        TaskTerminalStatus::Failed
    }
}

/// Verify every service the task declares as a dependency is among the started
/// services and is probe-ready. A task may depend on more than one service.
fn ensure_task_dependencies(
    registry: &Registry,
    task: &ExecTask,
    dependencies: &[&StartedService],
) -> RuntimeResult<()> {
    for service_name in &task.depends_on_services_ready {
        let service = dependencies
            .iter()
            .find(|service| service.service_name() == service_name)
            .ok_or_else(|| {
                RuntimeError::new(
                    ErrorCode::DependencyUnavailable,
                    format!("task dependency {service_name} was not among the started services"),
                )
            })?;
        ensure_service_instance_probe_ready(registry, service_name, &service.service_instance_id)?;
    }
    Ok(())
}

fn spawn_task(
    exec: &ResolvedExec,
    args: &[String],
    command_cwd: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
) -> RuntimeResult<Child> {
    let mut command = Command::new(&exec.executable);
    command
        .args(args)
        .current_dir(command_cwd)
        .envs(&exec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::from(create_log_file(stdout_path)?))
        .stderr(Stdio::from(create_log_file(stderr_path)?));
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    command.spawn().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ProcEscape,
            format!("failed to spawn task process: {error}"),
        )
    })
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
            let _ = wait_for_child_exit(child, 1000);
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
            let _ = terminate_process_group(pgid, 1000);
            return Ok(TaskOutcome {
                exit_code: status.code(),
                timed_out: false,
                canceled: false,
            });
        }
        if Instant::now() >= deadline {
            record_task_cancellation_intent(registry, &context, pgid, "task timeout")?;
            terminate_process_group(pgid, 1000)?;
            let _ = wait_for_child_exit(child, 1000);
            return Ok(TaskOutcome {
                exit_code: None,
                timed_out: true,
                canceled: false,
            });
        }
        thread::sleep(Duration::from_millis(10));
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

fn task_args(
    exec: &ResolvedExec,
    endpoint: Option<&SelectedEndpoint>,
    state_root: &Path,
) -> Vec<String> {
    // The resolved exec already combines the base and task args; substitute the
    // runtime placeholders here. `${stateDir}` is always available from the run;
    // `${port}`/`${host}` only when the task depends on a service (admission
    // rejects a service-less task that references them).
    exec.args
        .iter()
        .map(|arg| match endpoint {
            Some(endpoint) => {
                substitute_arg(arg, endpoint.port, state_root).replace("${host}", &endpoint.host)
            }
            None => arg.replace("${stateDir}", &state_root.to_string_lossy()),
        })
        .collect()
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

fn write_summary(run: &TaskRun) -> RuntimeResult<()> {
    let summary = serde_json::to_vec_pretty(run)
        .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
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

fn create_log_file(path: &Path) -> RuntimeResult<File> {
    File::create(path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("failed to create log file {}: {error}", path.display()),
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
