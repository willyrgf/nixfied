use std::net::SocketAddr;
use std::time::Instant;

use super::socket::{Connection, TcpSocket};
use std::path::Path;

use nixfied_manifest::LoopbackHost;

use crate::cancellation::{CancellationToken, canceled_error};
use crate::error::RuntimeResult;
use crate::execution::ProbePolicy;
use crate::redaction::Redactor;
use crate::service::process::{
    CapturedExec, CapturedExecOutcome, CapturedExecTransition, RenderedInvocation,
    get_process_group, platform_start_identity, resolve_exec_cwd, spawn_gated_captured_exec,
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
    checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
) -> RuntimeResult<ProbeAttempt> {
    let socket_addr = SocketAddr::new(host.ip(), port);
    let failed = |error: std::io::Error| {
        ProbeAttempt::Failed(format!(
            "probe {} did not connect to {socket_addr}: {error}",
            probe.label
        ))
    };
    cancellation.check()?;
    checkpoint()?;
    let started = Instant::now();
    let socket = match TcpSocket::new(host.ip()) {
        Ok(socket) => socket,
        Err(error) => return Ok(failed(error)),
    };
    let mut state = socket.connect(host.ip(), port);
    loop {
        // Never accept a connection or failure without a fresh observation.
        cancellation.check()?;
        checkpoint()?;
        let pending = match state {
            Ok(Connection::Connected) => return Ok(ProbeAttempt::Succeeded),
            Err(error) => return Ok(failed(error)),
            Ok(Connection::Pending(pending)) => pending,
        };
        let Some(remaining) = probe
            .timeout
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
        else {
            return Ok(failed(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "connection attempt timed out",
            )));
        };
        state = pending.poll(remaining);
    }
}

pub(crate) struct ExecProbe<'a> {
    pub command: &'a RenderedInvocation,
    pub source_root: &'a Path,
    pub logs_dir: &'a Path,
    pub redactor: &'a Redactor,
    pub registry: &'a mut crate::registry::Registry,
    pub launcher: &'a Path,
    pub run_id: &'a str,
    pub service_name: &'a str,
    pub manifest_hash: &'a str,
    pub occurrence: u64,
}

/// Execute one invocation probe attempt: run the probe's bound exec
/// (args/env already substituted at service start) in its own process group
/// with the probe's per-attempt deadline; exit 0 is success. Each attempt's
/// output is retained in exclusive occurrence files, preserving earlier attempts.
pub(crate) fn exec_probe_attempt(
    probe: &ProbePolicy,
    invocation: ExecProbe<'_>,
    cancellation: &CancellationToken,
    checkpoint: &mut dyn FnMut() -> RuntimeResult<()>,
) -> RuntimeResult<ProbeAttempt> {
    let ExecProbe {
        command,
        source_root,
        logs_dir,
        redactor,
        registry,
        launcher,
        run_id,
        service_name,
        manifest_hash,
        occurrence,
    } = invocation;
    let command_cwd = resolve_exec_cwd(source_root, &command.cwd)?;
    let stdout_path = logs_dir.join(format!(
        "lifecycle.{service_name}.{}.probe.{occurrence}.stdout.log",
        probe.label
    ));
    let stderr_path = logs_dir.join(format!(
        "lifecycle.{service_name}.{}.probe.{occurrence}.stderr.log",
        probe.label
    ));
    cancellation.check()?;
    checkpoint()?;
    let pending = spawn_gated_captured_exec(
        &CapturedExec {
            authority: registry.authority(),
            executable: &command.executable,
            args: &command.args,
            env: &command.env,
            cwd: &command_cwd,
            stdin: command.stdin,
            timeout: Some(probe.timeout),
            stdout_path: &stdout_path,
            stderr_path: &stderr_path,
            redactor,
            log_file_mode: crate::redaction::LogFileMode::New,
            label: &format!("lifecycle operation {}", probe.label),
        },
        launcher,
    )?;
    use crate::registry::session::ExecutionOutcome;
    use crate::service::registry::{
        InvocationIdentity, InvocationOwner, InvocationProcessRecord, TaskTerminalStatus,
        mark_invocation_finished, record_invocation_observed, record_invocation_started,
        record_probe_canceling,
    };
    let pid = pending.pid();
    let pgid = pid as i32;
    let process_key = format!("process-{run_id}-probe-{service_name}-{occurrence}-{pid}");
    let identity = InvocationIdentity {
        run_id,
        process_key: &process_key,
        manifest_hash,
        owner: InvocationOwner::Probe(service_name),
    };
    let command_json = serde_json::json!({
        "label": probe.label, "serviceId": service_name, "executable": command.executable,
        "args": command.args, "cwd": command_cwd, "stdoutPath": stdout_path, "stderrPath": stderr_path,
    }).to_string();
    let child = pending.register_and_release(
        |_| {
            if get_process_group(pid)? != pgid {
                return Err(crate::RuntimeError::new(
                    crate::ErrorCode::ProcEscape,
                    "probe launcher process group changed",
                ));
            }
            let start = platform_start_identity(pid).ok_or_else(|| {
                crate::RuntimeError::new(
                    crate::ErrorCode::ProcEscape,
                    "probe launcher start identity is unavailable",
                )
            })?;
            let start_identity =
                crate::service::StoredProcessIdentity::encode(pid, pgid, Some(&start), None);
            record_invocation_started(
                registry,
                &InvocationProcessRecord {
                    run_id,
                    process_key: &process_key,
                    pid,
                    pgid,
                    start_identity: &start_identity,
                    command_json: &command_json,
                    computed_manifest_hash: manifest_hash,
                },
                identity.owner,
            )
        },
        || {
            cancellation.check()?;
            checkpoint()
        },
    )?;
    let terminal = |outcome: &CapturedExecOutcome| match outcome {
        CapturedExecOutcome::Exited(status) if status.success() => (
            ExecutionOutcome::Succeeded,
            status.code(),
            TaskTerminalStatus::Succeeded,
        ),
        CapturedExecOutcome::Exited(status) => (
            ExecutionOutcome::Failed,
            status.code(),
            TaskTerminalStatus::Failed,
        ),
        CapturedExecOutcome::Canceled => (
            ExecutionOutcome::Canceled,
            None,
            TaskTerminalStatus::Canceled,
        ),
        CapturedExecOutcome::TimedOut => {
            (ExecutionOutcome::Failed, None, TaskTerminalStatus::TimedOut)
        }
    };
    let outcome = child
        .complete(cancellation, checkpoint, |transition| match transition {
            CapturedExecTransition::Observed(outcome) => {
                let (outcome, code, _) = terminal(outcome);
                record_invocation_observed(registry, identity, outcome, code)
            }
            CapturedExecTransition::Terminating(_) => record_probe_canceling(registry, identity),
        })
        .map_err(|failure| {
            let error = *failure.error;
            if !failure.settled {
                return error;
            }
            // A settled probe interrupted by its session keeps no obligation.
            let status = failure
                .outcome
                .as_ref()
                .map_or(TaskTerminalStatus::Canceled, |outcome| terminal(outcome).2);
            match mark_invocation_finished(registry, identity, status, "{}") {
                Ok(()) => error,
                Err(settlement) => error.with_cause(settlement),
            }
        })?;
    mark_invocation_finished(registry, identity, terminal(&outcome).2, "{}")?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener};
    use std::time::Duration;

    fn policy(timeout: Duration) -> ProbePolicy {
        ProbePolicy {
            label: "tcp-test".into(),
            timeout,
            retry_interval: Duration::from_millis(1),
            max_attempts: 1.try_into().unwrap(),
        }
    }

    #[test]
    fn tcp_probe_connects_once_and_refuses_a_closed_port() {
        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ] {
            let listener = TcpListener::bind(SocketAddr::new(ip, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            listener.set_nonblocking(true).unwrap();
            let host = LoopbackHost::parse(&ip.to_string()).unwrap();
            assert!(matches!(
                tcp_probe_attempt(
                    &policy(Duration::from_secs(1)),
                    host,
                    port,
                    &CancellationToken::new(),
                    &mut || Ok(())
                )
                .unwrap(),
                ProbeAttempt::Succeeded
            ));
            drop(listener.accept().unwrap());
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
            drop(listener);
            assert!(matches!(
                tcp_probe_attempt(
                    &policy(Duration::from_secs(1)),
                    host,
                    port,
                    &CancellationToken::new(),
                    &mut || Ok(())
                )
                .unwrap(),
                ProbeAttempt::Failed(_)
            ));
        }
    }

    // Linux's filled accept queue leaves a loopback connect pending. Keep every
    // accepted connection open and never accept: no timing-dependent remote host
    // or packet-filter changes are needed to exercise a real pending attempt.
    #[cfg(target_os = "linux")]
    fn saturated_listener() -> (TcpSocket, Vec<std::net::TcpStream>, u16) {
        use std::os::fd::AsRawFd;
        let ip = Ipv4Addr::LOCALHOST.into();
        let listener = TcpSocket::new(ip).unwrap();
        listener.bind(ip, 0).unwrap();
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
        let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        let mut length = std::mem::size_of_val(&address) as libc::socklen_t;
        assert_eq!(
            unsafe {
                libc::getsockname(listener.as_raw_fd(), (&raw mut address).cast(), &mut length)
            },
            0
        );
        let port = u16::from_be(address.sin_port);
        let mut clients = Vec::new();
        for _ in 0..8 {
            match std::net::TcpStream::connect_timeout(
                &SocketAddr::new(ip, port),
                Duration::from_millis(50),
            ) {
                Ok(client) => clients.push(client),
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                    return (listener, clients, port);
                }
                Err(error) => panic!("failed to fill listener queue: {error}"),
            }
        }
        panic!("listener queue did not saturate");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pending_tcp_attempt_observes_cancellation_failure_and_its_deadline() {
        use crate::error::{ErrorCode, RuntimeError};
        let (_listener, _clients, port) = saturated_listener();
        let host = LoopbackHost::parse("127.0.0.1").unwrap();
        for cancel in [true, false] {
            let token = CancellationToken::new();
            let mut observations = 0;
            let started = Instant::now();
            let error = tcp_probe_attempt(
                &policy(Duration::from_secs(30)),
                host,
                port,
                &token,
                &mut || {
                    observations += 1;
                    if observations < 3 {
                        return Ok(());
                    }
                    if cancel {
                        token.cancel();
                        token.check()
                    } else {
                        Err(RuntimeError::new(
                            ErrorCode::DependencyUnavailable,
                            "fixture service exited",
                        ))
                    }
                },
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                if cancel {
                    ErrorCode::Canceled
                } else {
                    ErrorCode::DependencyUnavailable
                }
            );
            assert!(started.elapsed() < Duration::from_secs(2));
        }
        let started = Instant::now();
        assert!(matches!(
            tcp_probe_attempt(
                &policy(Duration::from_millis(80)),
                host,
                port,
                &CancellationToken::new(),
                &mut || Ok(())
            )
            .unwrap(),
            ProbeAttempt::Failed(_)
        ));
        assert!(started.elapsed() >= Duration::from_millis(80));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
