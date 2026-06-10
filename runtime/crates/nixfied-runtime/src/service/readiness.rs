use std::net::{SocketAddr, TcpStream};

use crate::cancellation::{CancellationToken, sleep_cancellable};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::TcpProbe;

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
            probe.probe_id,
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "no attempts made".to_string())
        ),
    ))
}
