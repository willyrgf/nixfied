use std::collections::{BTreeMap, BTreeSet};

use crate::constants::{MODEL_VERSION, RUNTIME_ABI, TOOLCHAIN_ID};
use crate::error::ValidationError;
use crate::types::*;

const M0_SURFACES: &[&str] = &[
    "model",
    "schema",
    "docs",
    "capabilities",
    "check",
    "run",
    "ps",
    "down",
    "clean",
];

pub trait ValidateM0 {
    fn validate_m0(&self) -> Result<(), ValidationError>;
}

impl ValidateM0 for Model {
    fn validate_m0(&self) -> Result<(), ValidationError> {
        validate_exact_identities(self)?;
        validate_required_strings(self)?;
        validate_m0_shape(self)?;
        validate_m0_discoverability(self)?;
        validate_m0_runtime_constraints(self)?;
        validate_no_host_absolute_placement(&self.placement)?;
        validate_slot_placements(self)?;
        validate_m0_exec_and_closure(self)?;
        validate_m0_service_and_task(self)?;
        validate_references(self)?;
        Ok(())
    }
}

fn validate_exact_identities(model: &Model) -> Result<(), ValidationError> {
    if model.model_version != MODEL_VERSION {
        return Err(ValidationError::ModelVersion {
            expected: MODEL_VERSION,
            actual: model.model_version,
        });
    }
    if model.toolchain_id != TOOLCHAIN_ID {
        return Err(ValidationError::ToolchainId {
            expected: TOOLCHAIN_ID,
            actual: model.toolchain_id.clone(),
        });
    }
    if model.runtime_abi != RUNTIME_ABI {
        return Err(ValidationError::RuntimeAbi {
            expected: RUNTIME_ABI,
            actual: model.runtime_abi.clone(),
        });
    }
    Ok(())
}

fn validate_required_strings(model: &Model) -> Result<(), ValidationError> {
    require_non_empty("generator.name", &model.generator.name)?;
    require_non_empty("generator.version", &model.generator.version)?;
    require_non_empty("generator.emitter", &model.generator.emitter)?;
    require_non_empty("project.projectId", &model.project.project_id)?;
    require_non_empty("project.name", &model.project.name)?;
    require_non_empty("target.system", &model.target.system)?;
    require_non_empty("target.os", &model.target.os)?;
    require_non_empty("target.arch", &model.target.arch)?;
    require_non_empty("target.closureSystem", &model.target.closure_system)?;
    require_non_empty("state.markerIdentity", &model.state.marker_identity)?;
    require_non_empty("state.stateEpoch", &model.state.state_epoch)?;
    Ok(())
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        Err(ValidationError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn validate_m0_shape(model: &Model) -> Result<(), ValidationError> {
    if !model.secrets.is_empty() {
        return Err(ValidationError::MustBeEmpty { field: "secrets" });
    }
    if !model.workflows.is_empty() {
        return Err(ValidationError::MustBeEmpty { field: "workflows" });
    }
    if model.codebases.len() != 1 {
        return Err(ValidationError::ExpectedOne { field: "codebases" });
    }
    let codebase = &model.codebases[0];
    expect_string("codebases[0].codebaseId", "main", &codebase.codebase_id)?;
    expect_source_mode(
        "codebases[0].sourceMode",
        &SourceMode::LiveWorkspace,
        &codebase.source_mode,
    )?;

    if model.environments.len() != 1 {
        return Err(ValidationError::ExpectedOne {
            field: "environments",
        });
    }
    if !model.environments.contains_key("dev") {
        return Err(ValidationError::UnsupportedValue {
            field: "environments",
            expected: "dev",
            actual: format!("{:?}", model.environments.keys().collect::<Vec<_>>()),
        });
    }
    let env = &model.environments["dev"];
    expect_string("environments.dev.environmentId", "dev", &env.environment_id)?;

    validate_slot_policy(&model.slot_policy)?;

    if model.services.len() != 1 || !model.services.contains_key("synthetic") {
        return Err(ValidationError::UnsupportedValue {
            field: "services",
            expected: "synthetic",
            actual: format!("{:?}", model.services.keys().collect::<Vec<_>>()),
        });
    }
    if model.tasks.len() != 1 || !model.tasks.contains_key("smoke") {
        return Err(ValidationError::UnsupportedValue {
            field: "tasks",
            expected: "smoke",
            actual: format!("{:?}", model.tasks.keys().collect::<Vec<_>>()),
        });
    }

    let service = &model.services["synthetic"];
    expect_string(
        "services.synthetic.serviceId",
        "synthetic",
        &service.service_id,
    )?;
    if !service.foreground {
        return Err(ValidationError::UnsupportedValue {
            field: "services.synthetic.foreground",
            expected: "true",
            actual: "false".to_string(),
        });
    }
    if service.lifetime != ServiceLifetime::RunScoped {
        return Err(ValidationError::UnsupportedValue {
            field: "services.synthetic.lifetime",
            expected: "run-scoped",
            actual: format!("{:?}", service.lifetime),
        });
    }

    let task = &model.tasks["smoke"];
    expect_string("tasks.smoke.taskId", "smoke", &task.task_id)?;
    Ok(())
}

fn validate_m0_discoverability(model: &Model) -> Result<(), ValidationError> {
    expect_vec(
        "capabilities.environments",
        &["dev"],
        &model.capabilities.environments,
    )?;
    expect_vec(
        "capabilities.services",
        &["synthetic"],
        &model.capabilities.services,
    )?;
    expect_vec("capabilities.tasks", &["smoke"], &model.capabilities.tasks)?;
    let slots = expected_slots(&model.slot_policy)?;
    if model.capabilities.slots != slots {
        return Err(ValidationError::UnsupportedValue {
            field: "capabilities.slots",
            expected: "slotPolicy range",
            actual: format!("{:?}", model.capabilities.slots),
        });
    }
    if !model.capabilities.workflows.is_empty() {
        return Err(ValidationError::MustBeEmpty {
            field: "capabilities.workflows",
        });
    }
    expect_vec(
        "capabilities.surfaces",
        M0_SURFACES,
        &model.capabilities.surfaces,
    )?;
    let surface_names = model
        .surfaces
        .iter()
        .map(|surface| surface.name.clone())
        .collect::<Vec<_>>();
    if model.capabilities.surfaces != surface_names {
        return Err(ValidationError::UnsupportedValue {
            field: "capabilities.surfaces",
            expected: "model.surfaces names",
            actual: format!("{:?}", model.capabilities.surfaces),
        });
    }
    expect_vec("surfaces", M0_SURFACES, &surface_names)?;
    for surface in &model.surfaces {
        if !surface.aliases.is_empty() {
            return Err(ValidationError::MustBeEmpty {
                field: "surfaces.aliases",
            });
        }
        expect_vec(
            "surfaces.exitClasses",
            &["ok", "error"],
            &surface.exit_classes,
        )?;
        if surface.evaluation_permission != EvaluationPermission::Never {
            return Err(ValidationError::UnsupportedValue {
                field: "surfaces.evaluationPermission",
                expected: "never",
                actual: format!("{:?}", surface.evaluation_permission),
            });
        }
        if surface.maturity != SurfaceMaturity::M0 {
            return Err(ValidationError::UnsupportedValue {
                field: "surfaces.maturity",
                expected: "m0",
                actual: format!("{:?}", surface.maturity),
            });
        }
    }
    Ok(())
}

fn validate_m0_runtime_constraints(model: &Model) -> Result<(), ValidationError> {
    expect_vec(
        "runtimeConstraints.allowedEnvironments",
        &["dev"],
        &model.runtime_constraints.allowed_environments,
    )?;
    if model.runtime_constraints.slot_min != model.slot_policy.min
        || model.runtime_constraints.slot_default != model.slot_policy.default
        || model.runtime_constraints.slot_max != model.slot_policy.max
    {
        return Err(ValidationError::UnsupportedValue {
            field: "runtimeConstraints.slot",
            expected: "slotPolicy min/default/max",
            actual: format!(
                "slotMin={}, slotDefault={}, slotMax={}",
                model.runtime_constraints.slot_min,
                model.runtime_constraints.slot_default,
                model.runtime_constraints.slot_max
            ),
        });
    }
    if model.runtime_constraints.allow_port_override {
        return Err(ValidationError::UnsupportedValue {
            field: "runtimeConstraints.allowPortOverride",
            expected: "false",
            actual: "true".to_string(),
        });
    }
    if model.runtime_constraints.collision_policy != CollisionPolicy::Fail {
        return Err(ValidationError::UnsupportedValue {
            field: "runtimeConstraints.collisionPolicy",
            expected: "fail",
            actual: format!("{:?}", model.runtime_constraints.collision_policy),
        });
    }
    let caps = &model.target.required_runtime_capabilities;
    if !(caps.process_group && caps.tcp_port_ownership && caps.sqlite_wal) {
        return Err(ValidationError::UnsupportedValue {
            field: "target.requiredRuntimeCapabilities",
            expected: "processGroup=true, tcpPortOwnership=true, sqliteWal=true",
            actual: format!(
                "processGroup={}, tcpPortOwnership={}, sqliteWal={}",
                caps.process_group, caps.tcp_port_ownership, caps.sqlite_wal
            ),
        });
    }
    Ok(())
}

fn validate_slot_policy(slot_policy: &SlotPolicy) -> Result<(), ValidationError> {
    if slot_policy.max < slot_policy.min {
        return Err(ValidationError::UnsupportedValue {
            field: "slotPolicy",
            expected: "max >= min",
            actual: format!(
                "min={}, default={}, max={}",
                slot_policy.min, slot_policy.default, slot_policy.max
            ),
        });
    }
    if slot_policy.default < slot_policy.min || slot_policy.default > slot_policy.max {
        return Err(ValidationError::UnsupportedValue {
            field: "slotPolicy.default",
            expected: "within slotPolicy range",
            actual: slot_policy.default.to_string(),
        });
    }
    Ok(())
}

fn expected_slots(slot_policy: &SlotPolicy) -> Result<Vec<u32>, ValidationError> {
    validate_slot_policy(slot_policy)?;
    Ok((slot_policy.min..=slot_policy.max).collect())
}

fn validate_slot_placements(model: &Model) -> Result<(), ValidationError> {
    validate_candidate_port_window("placement.candidatePorts", &model.placement.candidate_ports)?;
    let slots = expected_slots(&model.slot_policy)?;
    let expected_keys = slots.iter().map(u32::to_string).collect::<BTreeSet<_>>();
    let actual_keys = model
        .placement
        .slot_placements
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if actual_keys != expected_keys {
        return Err(ValidationError::UnsupportedValue {
            field: "placement.slotPlacements",
            expected: "exact slotPolicy range",
            actual: format!("{:?}", actual_keys),
        });
    }

    let mut windows = Vec::new();
    for slot in slots {
        let key = slot.to_string();
        let placement = model.placement.slot_placements.get(&key).ok_or_else(|| {
            ValidationError::UnsupportedValue {
                field: "placement.slotPlacements",
                expected: "slotPolicy range",
                actual: format!("missing slot {slot}"),
            }
        })?;
        if placement.slot != slot {
            return Err(ValidationError::UnsupportedValue {
                field: "placement.slotPlacements.slot",
                expected: "map key slot",
                actual: placement.slot.to_string(),
            });
        }
        for (field, expected, actual) in [
            (
                "placement.slotPlacements.stateRootTemplate",
                model.placement.state_root_template.as_str(),
                placement.state_root_template.as_str(),
            ),
            (
                "placement.slotPlacements.registryDir",
                model.placement.registry_dir.as_str(),
                placement.registry_dir.as_str(),
            ),
            (
                "placement.slotPlacements.runDirTemplate",
                model.placement.run_dir_template.as_str(),
                placement.run_dir_template.as_str(),
            ),
            (
                "placement.slotPlacements.logsDirTemplate",
                model.placement.logs_dir_template.as_str(),
                placement.logs_dir_template.as_str(),
            ),
            (
                "placement.slotPlacements.artifactsDirTemplate",
                model.placement.artifacts_dir_template.as_str(),
                placement.artifacts_dir_template.as_str(),
            ),
        ] {
            if actual != expected {
                return Err(ValidationError::UnsupportedValue {
                    field,
                    expected: "common placement template",
                    actual: actual.to_string(),
                });
            }
        }
        validate_candidate_port_window(
            "placement.slotPlacements.candidatePorts",
            &placement.candidate_ports,
        )?;
        windows.push((
            placement.candidate_ports.start,
            placement.candidate_ports.end,
        ));
    }

    let default_key = model.slot_policy.default.to_string();
    let default = model
        .placement
        .slot_placements
        .get(&default_key)
        .ok_or_else(|| ValidationError::UnsupportedValue {
            field: "placement.slotPlacements",
            expected: "default slot placement",
            actual: format!("missing slot {}", model.slot_policy.default),
        })?;
    if model.placement.candidate_ports != default.candidate_ports {
        return Err(ValidationError::UnsupportedValue {
            field: "placement.candidatePorts",
            expected: "default slot candidatePorts",
            actual: format!("{:?}", model.placement.candidate_ports),
        });
    }

    windows.sort_unstable_by_key(|(start, _)| *start);
    for pair in windows.windows(2) {
        let (_, previous_end) = pair[0];
        let (next_start, _) = pair[1];
        if next_start <= previous_end {
            return Err(ValidationError::UnsupportedValue {
                field: "placement.slotPlacements.candidatePorts",
                expected: "non-overlapping windows",
                actual: format!("{windows:?}"),
            });
        }
    }
    Ok(())
}

fn validate_candidate_port_window(
    field: &'static str,
    window: &CandidatePortWindow,
) -> Result<(), ValidationError> {
    if window.start == 0 || window.start > window.end {
        return Err(ValidationError::UnsupportedValue {
            field,
            expected: "ports in 1..65535 with start <= end",
            actual: format!("start={}, end={}", window.start, window.end),
        });
    }
    Ok(())
}

fn validate_m0_exec_and_closure(model: &Model) -> Result<(), ValidationError> {
    expect_len("closures", 1, model.closures.len())?;
    let closure = &model.closures[0];
    expect_string("closures[0].closureId", "m0-helper", &closure.closure_id)?;
    if closure.kind != ClosureKind::Executable {
        return Err(ValidationError::UnsupportedValue {
            field: "closures[0].kind",
            expected: "executable",
            actual: format!("{:?}", closure.kind),
        });
    }
    if !closure.requires_executable {
        return Err(ValidationError::UnsupportedValue {
            field: "closures[0].requiresExecutable",
            expected: "true",
            actual: "false".to_string(),
        });
    }
    expect_vec(
        "closures[0].operationBindings",
        &[
            "service.synthetic.start",
            "service.synthetic.stop",
            "task.smoke.run",
        ],
        &closure.operation_bindings,
    )?;

    expect_len("execs", 1, model.execs.len())?;
    let exec = model
        .execs
        .get("m0-helper")
        .ok_or_else(|| ValidationError::UnsupportedValue {
            field: "execs",
            expected: "m0-helper",
            actual: format!("{:?}", model.execs.keys().collect::<Vec<_>>()),
        })?;
    expect_string("execs.m0-helper.execId", "m0-helper", &exec.exec_id)?;
    expect_string("execs.m0-helper.closureId", "m0-helper", &exec.closure_id)?;
    expect_string("execs.m0-helper.codebaseId", "main", &exec.codebase_id)?;
    if exec.stdin != StdinPolicy::Null {
        return Err(ValidationError::UnsupportedValue {
            field: "execs.m0-helper.stdin",
            expected: "null",
            actual: format!("{:?}", exec.stdin),
        });
    }
    if exec.output_capture != OutputCapture::StdoutStderr {
        return Err(ValidationError::UnsupportedValue {
            field: "execs.m0-helper.outputCapture",
            expected: "stdout-stderr",
            actual: format!("{:?}", exec.output_capture),
        });
    }
    if exec.cancellation_mode != CancellationMode::KillProcessGroup {
        return Err(ValidationError::UnsupportedValue {
            field: "execs.m0-helper.cancellationMode",
            expected: "kill-process-group",
            actual: format!("{:?}", exec.cancellation_mode),
        });
    }
    Ok(())
}

fn validate_m0_service_and_task(model: &Model) -> Result<(), ValidationError> {
    let service = &model.services["synthetic"];
    expect_len("services.synthetic.endpoints", 1, service.endpoints.len())?;
    expect_len("services.synthetic.probes", 1, service.probes.len())?;
    expect_len("services.synthetic.lifecycle", 3, service.lifecycle.len())?;
    expect_string(
        "services.synthetic.readinessProbe",
        "synthetic-tcp",
        &service.readiness_probe,
    )?;
    if service.containment != ContainmentRequirement::ProcessGroup {
        return Err(ValidationError::UnsupportedValue {
            field: "services.synthetic.containment",
            expected: "process-group",
            actual: format!("{:?}", service.containment),
        });
    }
    validate_m0_endpoint(model, &service.endpoints[0])?;
    validate_m0_probe(&service.probes[0])?;
    validate_m0_lifecycle(&service.lifecycle)?;

    let task = &model.tasks["smoke"];
    expect_string(
        "tasks.smoke.operationId",
        "task.smoke.run",
        &task.operation_id,
    )?;
    expect_string("tasks.smoke.execId", "m0-helper", &task.exec_id)?;
    expect_vec(
        "tasks.smoke.args",
        &["task", "--host", "127.0.0.1", "--port", "${port}"],
        &task.args,
    )?;
    expect_vec(
        "tasks.smoke.dependsOnServicesReady",
        &["synthetic"],
        &task.depends_on_services_ready,
    )?;
    if task.exit_policy.success_codes != [0] {
        return Err(ValidationError::UnsupportedValue {
            field: "tasks.smoke.exitPolicy.successCodes",
            expected: "[0]",
            actual: format!("{:?}", task.exit_policy.success_codes),
        });
    }
    if task.output_capture != OutputCapture::StdoutStderr {
        return Err(ValidationError::UnsupportedValue {
            field: "tasks.smoke.outputCapture",
            expected: "stdout-stderr",
            actual: format!("{:?}", task.output_capture),
        });
    }
    Ok(())
}

fn validate_m0_endpoint(model: &Model, endpoint: &EndpointSpec) -> Result<(), ValidationError> {
    expect_string(
        "services.synthetic.endpoints[0].endpointId",
        "synthetic-tcp",
        &endpoint.endpoint_id,
    )?;
    if endpoint.protocol != EndpointProtocol::Tcp {
        return Err(ValidationError::UnsupportedValue {
            field: "services.synthetic.endpoints[0].protocol",
            expected: "tcp",
            actual: format!("{:?}", endpoint.protocol),
        });
    }
    expect_string(
        "services.synthetic.endpoints[0].host",
        "127.0.0.1",
        &endpoint.host,
    )?;
    match &endpoint.port {
        PortPolicy::CandidateWindow { start, end }
            if *start == model.placement.candidate_ports.start
                && *end == model.placement.candidate_ports.end => {}
        _ => {
            return Err(ValidationError::UnsupportedValue {
                field: "services.synthetic.endpoints[0].port",
                expected: "placement candidate window",
                actual: format!("{:?}", endpoint.port),
            });
        }
    }
    if endpoint.ownership_verification != OwnershipVerification::Required {
        return Err(ValidationError::UnsupportedValue {
            field: "services.synthetic.endpoints[0].ownershipVerification",
            expected: "required",
            actual: format!("{:?}", endpoint.ownership_verification),
        });
    }
    if endpoint.socket_activation != SocketActivation::Disabled {
        return Err(ValidationError::UnsupportedValue {
            field: "services.synthetic.endpoints[0].socketActivation",
            expected: "disabled",
            actual: format!("{:?}", endpoint.socket_activation),
        });
    }
    Ok(())
}

fn validate_m0_probe(probe: &ProbeSpec) -> Result<(), ValidationError> {
    expect_string(
        "services.synthetic.probes[0].probeId",
        "synthetic-tcp",
        &probe.probe_id,
    )?;
    match &probe.target {
        ProbeTarget::TcpConnect { endpoint_id } if endpoint_id == "synthetic-tcp" => {}
        _ => {
            return Err(ValidationError::UnsupportedValue {
                field: "services.synthetic.probes[0].target",
                expected: "tcp-connect synthetic-tcp",
                actual: format!("{:?}", probe.target),
            });
        }
    }
    if probe.max_attempts == 0 || probe.timeout_ms == 0 || probe.retry_interval_ms == 0 {
        return Err(ValidationError::UnsupportedValue {
            field: "services.synthetic.probes[0].timing",
            expected: "positive timeoutMs, retryIntervalMs, maxAttempts",
            actual: format!(
                "timeoutMs={}, retryIntervalMs={}, maxAttempts={}",
                probe.timeout_ms, probe.retry_interval_ms, probe.max_attempts
            ),
        });
    }
    Ok(())
}

fn validate_m0_lifecycle(lifecycle: &[LifecycleOpSpec]) -> Result<(), ValidationError> {
    let by_id = lifecycle
        .iter()
        .map(|op| (op.operation_id.as_str(), op))
        .collect::<BTreeMap<_, _>>();
    validate_lifecycle_op(
        &by_id,
        "service.synthetic.start",
        LifecycleOpClass::Start,
        Some("m0-helper"),
        &["service", "--host", "127.0.0.1", "--port", "${port}"],
        None,
    )?;
    validate_lifecycle_op(
        &by_id,
        "service.synthetic.ready",
        LifecycleOpClass::Ready,
        None,
        &[],
        Some("synthetic-tcp"),
    )?;
    validate_lifecycle_op(
        &by_id,
        "service.synthetic.stop",
        LifecycleOpClass::Stop,
        Some("m0-helper"),
        &["stop"],
        None,
    )?;
    Ok(())
}

fn validate_lifecycle_op(
    by_id: &BTreeMap<&str, &LifecycleOpSpec>,
    operation_id: &'static str,
    class: LifecycleOpClass,
    exec_id: Option<&'static str>,
    exec_args: &[&'static str],
    probe_id: Option<&'static str>,
) -> Result<(), ValidationError> {
    let op = by_id
        .get(operation_id)
        .ok_or_else(|| ValidationError::UndeclaredReference {
            reference_kind: "lifecycle.operationId",
            id: operation_id.to_string(),
        })?;
    if op.class != class {
        return Err(ValidationError::UnsupportedValue {
            field: "lifecycle.class",
            expected: operation_id,
            actual: format!("{:?}", op.class),
        });
    }
    expect_option("lifecycle.execId", exec_id, op.exec_id.as_deref())?;
    expect_vec("lifecycle.execArgs", exec_args, &op.exec_args)?;
    expect_option("lifecycle.probeId", probe_id, op.probe_id.as_deref())?;
    Ok(())
}

fn expect_string(
    field: &'static str,
    expected: &'static str,
    actual: &str,
) -> Result<(), ValidationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedValue {
            field,
            expected,
            actual: actual.to_string(),
        })
    }
}

fn expect_option(
    field: &'static str,
    expected: Option<&'static str>,
    actual: Option<&str>,
) -> Result<(), ValidationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedValue {
            field,
            expected: expected.unwrap_or("null"),
            actual: actual.unwrap_or("null").to_string(),
        })
    }
}

fn expect_len(field: &'static str, expected: usize, actual: usize) -> Result<(), ValidationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ValidationError::ExpectedLen {
            field,
            expected,
            actual,
        })
    }
}

fn expect_vec<T>(
    field: &'static str,
    expected: &[&'static str],
    actual: &[T],
) -> Result<(), ValidationError>
where
    T: AsRef<str> + std::fmt::Debug,
{
    let actual_strings = actual.iter().map(AsRef::as_ref).collect::<Vec<_>>();
    if actual_strings.as_slice() == expected {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedValue {
            field,
            expected: "exact M0 values",
            actual: format!("{actual:?}"),
        })
    }
}

fn expect_source_mode(
    field: &'static str,
    expected: &SourceMode,
    actual: &SourceMode,
) -> Result<(), ValidationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedValue {
            field,
            expected: "live-workspace",
            actual: format!("{actual:?}"),
        })
    }
}

fn validate_no_host_absolute_placement(placement: &Placement) -> Result<(), ValidationError> {
    for (field, value) in [
        (
            "placement.stateRootTemplate",
            &placement.state_root_template,
        ),
        ("placement.registryDir", &placement.registry_dir),
        ("placement.runDirTemplate", &placement.run_dir_template),
        ("placement.logsDirTemplate", &placement.logs_dir_template),
        (
            "placement.artifactsDirTemplate",
            &placement.artifacts_dir_template,
        ),
    ] {
        if value.starts_with('/') {
            return Err(ValidationError::HostAbsolutePath {
                field,
                value: value.clone(),
            });
        }
    }
    for slot_placement in placement.slot_placements.values() {
        for (field, value) in [
            (
                "placement.slotPlacements.stateRootTemplate",
                &slot_placement.state_root_template,
            ),
            (
                "placement.slotPlacements.registryDir",
                &slot_placement.registry_dir,
            ),
            (
                "placement.slotPlacements.runDirTemplate",
                &slot_placement.run_dir_template,
            ),
            (
                "placement.slotPlacements.logsDirTemplate",
                &slot_placement.logs_dir_template,
            ),
            (
                "placement.slotPlacements.artifactsDirTemplate",
                &slot_placement.artifacts_dir_template,
            ),
        ] {
            if value.starts_with('/') {
                return Err(ValidationError::HostAbsolutePath {
                    field,
                    value: value.clone(),
                });
            }
        }
    }
    Ok(())
}

fn validate_references(model: &Model) -> Result<(), ValidationError> {
    let codebase_ids = model
        .codebases
        .iter()
        .map(|codebase| codebase.codebase_id.as_str())
        .collect::<BTreeSet<_>>();
    let closure_ids = model
        .closures
        .iter()
        .map(|closure| closure.closure_id.as_str())
        .collect::<BTreeSet<_>>();
    let exec_ids = model
        .execs
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let service_ids = model
        .services
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let task_ids = model
        .tasks
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut declared_operations = BTreeSet::new();
    let mut probe_ids = BTreeSet::new();
    let mut endpoint_ids = BTreeSet::new();

    for exec in model.execs.values() {
        if !closure_ids.contains(exec.closure_id.as_str()) {
            return Err(ValidationError::UndeclaredReference {
                reference_kind: "exec.closureId",
                id: exec.closure_id.clone(),
            });
        }
        if !codebase_ids.contains(exec.codebase_id.as_str()) {
            return Err(ValidationError::UndeclaredReference {
                reference_kind: "exec.codebaseId",
                id: exec.codebase_id.clone(),
            });
        }
    }

    for closure in &model.closures {
        if closure.target_system != model.target.closure_system {
            return Err(ValidationError::ClosureTargetMismatch {
                closure_id: closure.closure_id.clone(),
                target_system: closure.target_system.clone(),
                closure_system: model.target.closure_system.clone(),
            });
        }
    }

    for service in model.services.values() {
        for endpoint in &service.endpoints {
            endpoint_ids.insert(endpoint.endpoint_id.as_str());
        }
        for probe in &service.probes {
            probe_ids.insert(probe.probe_id.as_str());
        }
        for lifecycle in &service.lifecycle {
            declared_operations.insert(lifecycle.operation_id.as_str());
            if let Some(exec_id) = &lifecycle.exec_id
                && !exec_ids.contains(exec_id.as_str())
            {
                return Err(ValidationError::UndeclaredReference {
                    reference_kind: "lifecycle.execId",
                    id: exec_id.clone(),
                });
            }
            if let Some(probe_id) = &lifecycle.probe_id
                && !probe_ids.contains(probe_id.as_str())
            {
                return Err(ValidationError::UndeclaredReference {
                    reference_kind: "lifecycle.probeId",
                    id: probe_id.clone(),
                });
            }
        }
        if !probe_ids.contains(service.readiness_probe.as_str()) {
            return Err(ValidationError::UndeclaredReference {
                reference_kind: "service.readinessProbe",
                id: service.readiness_probe.clone(),
            });
        }
    }

    for task in model.tasks.values() {
        declared_operations.insert(task.operation_id.as_str());
        if !exec_ids.contains(task.exec_id.as_str()) {
            return Err(ValidationError::UndeclaredReference {
                reference_kind: "task.execId",
                id: task.exec_id.clone(),
            });
        }
        for service_id in &task.depends_on_services_ready {
            if !service_ids.contains(service_id.as_str()) {
                return Err(ValidationError::UndeclaredReference {
                    reference_kind: "task.dependsOnServicesReady",
                    id: service_id.clone(),
                });
            }
        }
    }

    for env in model.environments.values() {
        for service_id in &env.services {
            if !service_ids.contains(service_id.as_str()) {
                return Err(ValidationError::UndeclaredReference {
                    reference_kind: "environment.services",
                    id: service_id.clone(),
                });
            }
        }
        for task_id in &env.tasks {
            if !task_ids.contains(task_id.as_str()) {
                return Err(ValidationError::UndeclaredReference {
                    reference_kind: "environment.tasks",
                    id: task_id.clone(),
                });
            }
        }
    }

    for service in model.services.values() {
        for endpoint in &service.endpoints {
            match &endpoint.port {
                PortPolicy::Fixed { .. } | PortPolicy::CandidateWindow { .. } => {}
            }
        }
        for probe in &service.probes {
            match &probe.target {
                ProbeTarget::TcpConnect { endpoint_id }
                | ProbeTarget::HttpGet { endpoint_id, .. } => {
                    if !endpoint_ids.contains(endpoint_id.as_str()) {
                        return Err(ValidationError::UndeclaredReference {
                            reference_kind: "probe.endpointId",
                            id: endpoint_id.clone(),
                        });
                    }
                }
            }
        }
        for lifecycle in &service.lifecycle {
            if let Some(probe_id) = &lifecycle.probe_id {
                let probe = find_probe(service, probe_id)?;
                match &probe.target {
                    ProbeTarget::TcpConnect { endpoint_id }
                    | ProbeTarget::HttpGet { endpoint_id, .. } => {
                        if !endpoint_ids.contains(endpoint_id.as_str()) {
                            return Err(ValidationError::UndeclaredReference {
                                reference_kind: "probe.endpointId",
                                id: endpoint_id.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    for closure in &model.closures {
        for binding in &closure.operation_bindings {
            if !declared_operations.contains(binding.as_str()) {
                return Err(ValidationError::UnknownOperationBinding {
                    binding: binding.clone(),
                });
            }
        }
    }

    Ok(())
}

fn find_probe<'a>(
    service: &'a ServiceSpec,
    probe_id: &str,
) -> Result<&'a ProbeSpec, ValidationError> {
    service
        .probes
        .iter()
        .find(|probe| probe.probe_id == probe_id)
        .ok_or_else(|| ValidationError::UndeclaredReference {
            reference_kind: "probe.probeId",
            id: probe_id.to_string(),
        })
}
