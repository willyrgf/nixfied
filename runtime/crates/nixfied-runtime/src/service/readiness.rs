use std::net::{SocketAddr, TcpStream};
use std::path::Path;

use crate::cancellation::{CancellationToken, sleep_cancellable};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{ExecProbe, TcpProbe};
use crate::service::process::{
    BoundedExec, BoundedExecOutcome, resolve_exec_cwd, run_bounded_exec,
};

/// Wait for a tcp-connect probe to succeed against the service's bound endpoint.
/// The probe is already resolved (single endpoint, tcp only) by the lowering, so
/// there is no probe lookup or endpoint matching to do here.
pub fn wait_for_tcp_probe(
    probe: &TcpProbe,
    host: &str,
    port: u16,
    cancellation: &CancellationToken,
) -> RuntimeResult<()> {
    let address = format!("{host}:{port}");
    let socket_addr = address.parse::<SocketAddr>().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("invalid readiness address {address}: {error}"),
        )
    })?;
    let attempts = probe.max_attempts.max(1);
    let mut last_error = None;
    for _ in 0..attempts {
        cancellation.check()?;
        match TcpStream::connect_timeout(&socket_addr, probe.timeout) {
            Ok(_) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                sleep_cancellable(probe.retry_interval, cancellation)?;
            }
        }
    }
    Err(RuntimeError::new(
        ErrorCode::ReadinessTimeout,
        format!(
            "readiness probe {} did not connect to {socket_addr}: {}",
            probe.label,
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "no attempts made".to_string())
        ),
    ))
}

/// Wait for an exec probe to succeed: per attempt, run the probe's bound exec
/// (args/env already substituted at service start) in its own process group
/// with the probe's per-attempt deadline; exit 0 is success. Each attempt's
/// output overwrites `lifecycle.<label>.probe.{stdout,stderr}.log`, so the last
/// attempt's evidence — the one an operator debugs — survives.
pub fn wait_for_exec_probe(
    probe: &ExecProbe,
    source_root: &Path,
    logs_dir: &Path,
    cancellation: &CancellationToken,
) -> RuntimeResult<()> {
    let command_cwd = resolve_exec_cwd(source_root, &probe.exec.cwd)?;
    let stdout_path = logs_dir.join(format!("lifecycle.{}.probe.stdout.log", probe.label));
    let stderr_path = logs_dir.join(format!("lifecycle.{}.probe.stderr.log", probe.label));
    let attempts = probe.max_attempts.max(1);
    let mut last_failure = None;
    for _ in 0..attempts {
        cancellation.check()?;
        let outcome = run_bounded_exec(
            &BoundedExec {
                executable: &probe.exec.executable,
                args: &probe.exec.args,
                env: &probe.exec.env,
                cwd: &command_cwd,
                stdin: probe.exec.stdin,
                timeout: probe.timeout,
                stdout_path: &stdout_path,
                stderr_path: &stderr_path,
                label: &probe.label,
            },
            cancellation,
        )?;
        match outcome {
            BoundedExecOutcome::Exited(status) if status.success() => return Ok(()),
            BoundedExecOutcome::Exited(status) => {
                last_failure = Some(format!(
                    "exited with code {}",
                    status
                        .code()
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ));
            }
            BoundedExecOutcome::TimedOut => {
                last_failure = Some(format!("timed out after {}ms", probe.timeout.as_millis()));
            }
        }
        sleep_cancellable(probe.retry_interval, cancellation)?;
    }
    Err(RuntimeError::new(
        ErrorCode::ReadinessTimeout,
        format!(
            "readiness probe {} did not succeed: last attempt {} (probe logs: {}, {})",
            probe.label,
            last_failure.unwrap_or_else(|| "no attempts made".to_string()),
            stdout_path.display(),
            stderr_path.display(),
        ),
    ))
}
