use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nixfied_model::{ExecSpec, Model, TaskSpec};
use serde::Serialize;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::registry::Registry;
use crate::service::process::{
    SelectedEndpoint, StartedService, platform_start_identity, process_group, resolve_exec_cwd,
    signal_process_group, wait_for_child_exit,
};
use crate::service::registry::{
    TaskProcessRecord, ensure_service_instance_probe_ready, mark_task_finished, record_task_started,
};
use crate::state::HostPlacement;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub task_id: String,
    pub process_key: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub success: bool,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub summary_path: PathBuf,
}

pub fn run_dependent_task(
    model: &Model,
    placement: &HostPlacement,
    registry: &mut Registry,
    service: &StartedService,
    task_id: &str,
) -> RuntimeResult<TaskRun> {
    let task = model.tasks.get(task_id).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("task {task_id} is missing"),
        )
    })?;
    ensure_task_dependencies(registry, task, service)?;
    let exec = model.execs.get(&task.exec_id).ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("task exec {} is missing", task.exec_id),
        )
    })?;
    let stdout_path = placement
        .logs_dir
        .join(format!("task.{task_id}.stdout.log"));
    let stderr_path = placement
        .logs_dir
        .join(format!("task.{task_id}.stderr.log"));
    let args = task_args(exec, task, &service.selected_endpoint);
    let command_cwd = resolve_exec_cwd(&service.source_root, &exec.cwd)?;
    let command_json = serde_json::to_string(&TaskCommandRecord {
        task_id,
        executable: exec.executable.as_str(),
        args: &args,
        cwd: command_cwd.as_path(),
        stdout_path: stdout_path.as_path(),
        stderr_path: stderr_path.as_path(),
    })
    .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    let mut child = spawn_task(exec, &args, &command_cwd, &stdout_path, &stderr_path)?;
    let pid = child.id();
    let pgid = process_group(pid)?
        .ok_or_else(|| RuntimeError::new(ErrorCode::ProcEscape, "task process disappeared"))?;
    let process_key = format!("process-{}-task-{task_id}-{pid}-{pgid}", service.run_id);
    let start_identity = process_start_identity(pid, pgid, platform_start_identity(pid).as_deref());
    if let Err(error) = record_task_started(
        registry,
        &TaskProcessRecord {
            run_id: &service.run_id,
            process_key: &process_key,
            pid,
            pgid,
            start_identity: &start_identity,
            command_json: &command_json,
            computed_model_hash: &service.computed_model_hash,
        },
    ) {
        let _ = signal_process_group(pgid, libc::SIGKILL);
        let _ = child.wait();
        return Err(error);
    }
    let outcome = wait_for_task(&mut child, pgid, exec.timeout_ms)?;
    let success = !outcome.timed_out
        && outcome
            .exit_code
            .map(|code| task.exit_policy.success_codes.contains(&code))
            .unwrap_or(false);
    let exit_code = outcome.exit_code;
    let timed_out = outcome.timed_out;
    let failure_message = if timed_out {
        format!("task {task_id} timed out after {}ms", exec.timeout_ms)
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
        &service.run_id,
        &process_key,
        &service.computed_model_hash,
        success,
        &payload_json,
    )?;
    if success {
        Ok(run)
    } else {
        Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            failure_message,
        ))
    }
}

fn ensure_task_dependencies(
    registry: &Registry,
    task: &TaskSpec,
    service: &StartedService,
) -> RuntimeResult<()> {
    for service_name in &task.depends_on_services_ready {
        if service_name != &service.service_name {
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("M0 task dependency {service_name} is not the started synthetic service"),
            ));
        }
        ensure_service_instance_probe_ready(registry, service_name, &service.service_instance_id)?;
    }
    Ok(())
}

fn spawn_task(
    exec: &ExecSpec,
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

fn wait_for_task(child: &mut Child, pgid: i32, timeout_ms: u64) -> RuntimeResult<TaskOutcome> {
    let timeout = Duration::from_millis(timeout_ms);
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            RuntimeError::new(
                ErrorCode::ProcEscape,
                format!("failed to inspect task process: {error}"),
            )
        })? {
            return Ok(TaskOutcome {
                exit_code: status.code(),
                timed_out: false,
            });
        }
        if Instant::now() >= deadline {
            let _ = signal_process_group(pgid, libc::SIGKILL);
            let _ = wait_for_child_exit(child, 1000);
            return Ok(TaskOutcome {
                exit_code: None,
                timed_out: true,
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
}

struct TaskOutcome {
    exit_code: Option<i32>,
    timed_out: bool,
}

fn task_args(exec: &ExecSpec, task: &TaskSpec, endpoint: &SelectedEndpoint) -> Vec<String> {
    exec.args
        .iter()
        .chain(task.args.iter())
        .map(|arg| {
            arg.replace("${port}", &endpoint.port.to_string())
                .replace("${host}", &endpoint.host)
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
