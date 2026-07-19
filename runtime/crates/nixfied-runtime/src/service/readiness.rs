use std::net::{SocketAddr, TcpStream};
use std::path::Path;

use crate::cancellation::CancellationToken;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{ExecProbe, TcpProbe};
use crate::redaction::Redactor;
use crate::service::process::{
    BoundedExec, BoundedExecOutcome, resolve_exec_cwd, run_bounded_exec,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeAttempt {
    Succeeded,
    Failed(String),
}

/// Execute one tcp-connect probe attempt. Retry budgeting and endpoint
/// ownership observation live together in `process.rs`.
pub(crate) fn tcp_probe_attempt(
    probe: &TcpProbe,
    host: &str,
    port: u16,
    cancellation: &CancellationToken,
) -> RuntimeResult<ProbeAttempt> {
    // Build the address from the parsed IP, not string concatenation: a bare
    // IPv6 literal such as `::1` concatenated with `:port` is unparseable.
    let ip = host.parse::<std::net::IpAddr>().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("invalid readiness host {host}: {error}"),
        )
    })?;
    let socket_addr = SocketAddr::new(ip, port);
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
    probe: &ExecProbe,
    source_root: &Path,
    logs_dir: &Path,
    redactor: &Redactor,
    cancellation: &CancellationToken,
) -> RuntimeResult<ProbeAttempt> {
    let command_cwd = resolve_exec_cwd(source_root, &probe.exec.cwd)?;
    let env = probe.exec.env_with_path(probe.exec.env.clone());
    let stdout_path = logs_dir.join(format!("lifecycle.{}.probe.stdout.log", probe.label));
    let stderr_path = logs_dir.join(format!("lifecycle.{}.probe.stderr.log", probe.label));
    cancellation.check()?;
    let outcome = run_bounded_exec(
        &BoundedExec {
            executable: &probe.exec.executable,
            args: &probe.exec.args,
            env: &env,
            cwd: &command_cwd,
            stdin: probe.exec.stdin,
            timeout: probe.timeout,
            stdout_path: &stdout_path,
            stderr_path: &stderr_path,
            redactor,
            label: &probe.label,
        },
        cancellation,
    )?;
    Ok(match outcome {
        BoundedExecOutcome::Exited(status) if status.success() => ProbeAttempt::Succeeded,
        BoundedExecOutcome::Exited(status) => ProbeAttempt::Failed(format!(
            "probe {} exited with code {} (probe logs: {}, {})",
            probe.label,
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            stdout_path.display(),
            stderr_path.display(),
        )),
        BoundedExecOutcome::TimedOut => ProbeAttempt::Failed(format!(
            "probe {} timed out after {}ms (probe logs: {}, {})",
            probe.label,
            probe.timeout.as_millis(),
            stdout_path.display(),
            stderr_path.display(),
        )),
    })
}
