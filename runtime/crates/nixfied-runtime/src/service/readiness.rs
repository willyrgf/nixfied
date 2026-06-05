use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use nixfied_model::{EndpointSpec, ProbeSpec, ProbeTarget, ServiceSpec};

use crate::cancellation::{CancellationToken, sleep_cancellable};
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

pub fn wait_for_readiness_probe(
    service: &ServiceSpec,
    selected_endpoint: &EndpointSpec,
    selected_port: u16,
) -> RuntimeResult<()> {
    wait_for_readiness_probe_cancellable(
        service,
        selected_endpoint,
        selected_port,
        &CancellationToken::new(),
    )
}

pub fn wait_for_readiness_probe_cancellable(
    service: &ServiceSpec,
    selected_endpoint: &EndpointSpec,
    selected_port: u16,
    cancellation: &CancellationToken,
) -> RuntimeResult<()> {
    let probe = service
        .probes
        .iter()
        .find(|probe| probe.probe_id == service.readiness_probe)
        .ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!("readiness probe {} is missing", service.readiness_probe),
            )
        })?;
    wait_for_probe(probe, selected_endpoint, selected_port, cancellation)
}

fn wait_for_probe(
    probe: &ProbeSpec,
    selected_endpoint: &EndpointSpec,
    selected_port: u16,
    cancellation: &CancellationToken,
) -> RuntimeResult<()> {
    match &probe.target {
        ProbeTarget::TcpConnect { endpoint_id } => {
            if endpoint_id != &selected_endpoint.endpoint_id {
                return Err(RuntimeError::new(
                    ErrorCode::ModelAdmission,
                    format!(
                        "probe endpoint {endpoint_id} does not match selected endpoint {}",
                        selected_endpoint.endpoint_id
                    ),
                ));
            }
            wait_for_tcp(probe, selected_endpoint, selected_port, cancellation)
        }
        ProbeTarget::HttpGet { .. } => Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            "M0 service readiness only supports tcp-connect probes",
        )),
    }
}

fn wait_for_tcp(
    probe: &ProbeSpec,
    selected_endpoint: &EndpointSpec,
    selected_port: u16,
    cancellation: &CancellationToken,
) -> RuntimeResult<()> {
    let address = format!("{}:{selected_port}", selected_endpoint.host);
    let socket_addr = address.parse::<SocketAddr>().map_err(|error| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("invalid readiness address {address}: {error}"),
        )
    })?;
    let timeout = Duration::from_millis(probe.timeout_ms);
    let retry_interval = Duration::from_millis(probe.retry_interval_ms);
    let attempts = probe.max_attempts.max(1);
    let mut last_error = None;
    for _ in 0..attempts {
        cancellation.check()?;
        match TcpStream::connect_timeout(&socket_addr, timeout) {
            Ok(_) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                sleep_cancellable(retry_interval, cancellation)?;
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
