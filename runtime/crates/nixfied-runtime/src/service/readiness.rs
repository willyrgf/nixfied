use std::net::{SocketAddr, TcpStream};
use std::path::Path;

use nixfied_manifest::LoopbackHost;

use crate::cancellation::{CancellationToken, canceled_error};
use crate::error::RuntimeResult;
use crate::execution::ProbePolicy;
use crate::redaction::Redactor;
use crate::service::process::{
    CapturedExec, CapturedExecOutcome, RenderedInvocation, resolve_exec_cwd, run_captured_exec,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeAttempt {
    Succeeded,
    Failed(String),
}

/// Execute one tcp-connect probe attempt. Retry budgeting and endpoint
/// ownership observation live together in `process.rs`.
pub(crate) fn tcp_probe_attempt(
    probe: &ProbePolicy,
    host: LoopbackHost,
    port: u16,
    cancellation: &CancellationToken,
) -> RuntimeResult<ProbeAttempt> {
    let socket_addr = SocketAddr::new(host.ip(), port);
    cancellation.check()?;
    Ok(
        match TcpStream::connect_timeout(&socket_addr, probe.timeout) {
            Ok(_) => ProbeAttempt::Succeeded,
            Err(error) => ProbeAttempt::Failed(format!(
                "probe {} did not connect to {socket_addr}: {error}",
                probe.label
            )),
        },
    )
}

/// Execute one invocation probe attempt: run the probe's bound exec
/// (args/env already substituted at service start) in its own process group
/// with the probe's per-attempt deadline; exit 0 is success. Each attempt's
/// output overwrites `lifecycle.<label>.probe.{stdout,stderr}.log`, so the last
/// attempt's evidence — the one an operator debugs — survives.
pub(crate) fn exec_probe_attempt(
    probe: &ProbePolicy,
    command: &RenderedInvocation,
    source_root: &Path,
    logs_dir: &Path,
    redactor: &Redactor,
    cancellation: &CancellationToken,
    checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
) -> RuntimeResult<ProbeAttempt> {
    let command_cwd = resolve_exec_cwd(source_root, &command.cwd)?;
    let stdout_path = logs_dir.join(format!("lifecycle.{}.probe.stdout.log", probe.label));
    let stderr_path = logs_dir.join(format!("lifecycle.{}.probe.stderr.log", probe.label));
    cancellation.check()?;
    checkpoint()?;
    let outcome = run_captured_exec(
        &CapturedExec {
            executable: &command.executable,
            args: &command.args,
            env: &command.env,
            cwd: &command_cwd,
            stdin: command.stdin,
            timeout: Some(probe.timeout),
            stdout_path: &stdout_path,
            stderr_path: &stderr_path,
            redactor,
            log_file_mode: crate::redaction::LogFileMode::Replace,
            label: &format!("lifecycle operation {}", probe.label),
        },
        cancellation,
        checkpoint,
    )?;
    Ok(match outcome {
        CapturedExecOutcome::Canceled => return Err(canceled_error()),
        CapturedExecOutcome::Exited(status) if status.success() => ProbeAttempt::Succeeded,
        CapturedExecOutcome::Exited(status) => ProbeAttempt::Failed(format!(
            "probe {} exited with code {} (probe logs: {}, {})",
            probe.label,
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            stdout_path.display(),
            stderr_path.display(),
        )),
        CapturedExecOutcome::TimedOut => ProbeAttempt::Failed(format!(
            "probe {} timed out after {}ms (probe logs: {}, {})",
            probe.label,
            probe.timeout.as_millis(),
            stdout_path.display(),
            stderr_path.display(),
        )),
    })
}
