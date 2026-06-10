use std::collections::{BTreeMap, BTreeSet};

use crate::constants::{MODEL_VERSION, RUNTIME_ABI, TOOLCHAIN_ID};
use crate::error::ValidationError;
use crate::types::*;

/// Framework-owned public surfaces. These are not user-declarable; they are the
/// generated view/runtime command set every model exposes.
const REQUIRED_SURFACES: &[&str] = &[
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

/// Every service declares the full generic lifecycle operation contract: exactly
/// one operation per class, in this canonical order of classes.
const LIFECYCLE_CLASSES: &[LifecycleOpClass] = &[
    LifecycleOpClass::Prepare,
    LifecycleOpClass::Start,
    LifecycleOpClass::Ready,
    LifecycleOpClass::Health,
    LifecycleOpClass::Stop,
    LifecycleOpClass::Clean,
];

pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

impl Validate for Model {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_exact_identities(self)?;
        validate_required_strings(self)?;
        validate_deferred_features(self)?;
        validate_target_capabilities(self)?;
        validate_codebases(self)?;
        validate_environments(self)?;
        validate_slot_policy(&self.slot_policy)?;
        validate_runtime_constraints(self)?;
        validate_surfaces(self)?;
        validate_capabilities(self)?;
        validate_no_host_absolute_placement(&self.placement)?;
        validate_slot_placements(self)?;
        validate_execs(self)?;
        validate_closures(self)?;
        validate_services(self)?;
        validate_tasks(self)?;
        validate_workflows(self)?;
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

/// Features intentionally deferred beyond the current capability line.
fn validate_deferred_features(model: &Model) -> Result<(), ValidationError> {
    if !model.secrets.is_empty() {
        return Err(ValidationError::MustBeEmpty { field: "secrets" });
    }
    Ok(())
}

/// Workflows are bounded acyclic graphs of task nodes over declared services.
fn validate_workflows(model: &Model) -> Result<(), ValidationError> {
    let task_ids = model
        .tasks
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let service_ids = model
        .services
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for (id, workflow) in &model.workflows {
        if &workflow.workflow_id != id {
            return Err(ValidationError::UnsupportedValue {
                field: "workflows.workflowId",
                expected: "map key",
                actual: format!("key={id}, workflowId={}", workflow.workflow_id),
            });
        }
        for service in &workflow.services_required {
            if !service_ids.contains(service.as_str()) {
                return Err(ValidationError::UndeclaredReference {
                    reference_kind: "workflow.servicesRequired",
                    id: service.clone(),
                });
            }
        }
        if workflow.nodes.is_empty() {
            return Err(ValidationError::UnsupportedValue {
                field: "workflows.nodes",
                expected: "at least one node",
                actual: "[]".to_string(),
            });
        }
        let mut node_ids = BTreeSet::new();
        for node in &workflow.nodes {
            require_non_empty("workflows.nodes.nodeId", &node.node_id)?;
            if !node_ids.insert(node.node_id.as_str()) {
                return Err(ValidationError::UnsupportedValue {
                    field: "workflows.nodes.nodeId",
                    expected: "unique node ids",
                    actual: node.node_id.clone(),
                });
            }
            if !task_ids.contains(node.task_id.as_str()) {
                return Err(ValidationError::UndeclaredReference {
                    reference_kind: "workflow.node.taskId",
                    id: node.task_id.clone(),
                });
            }
        }
        for node in &workflow.nodes {
            for dependency in &node.depends_on {
                if !node_ids.contains(dependency.as_str()) {
                    return Err(ValidationError::UndeclaredReference {
                        reference_kind: "workflow.node.dependsOn",
                        id: dependency.clone(),
                    });
                }
            }
        }
        // The run plan starts only the workflow's servicesRequired, so every
        // service a node task depends on must be declared there; otherwise the
        // workflow would fail at runtime with a missing dependency instead of
        // being rejected at admission.
        let required = workflow
            .services_required
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for node in &workflow.nodes {
            let Some(task) = model.tasks.get(&node.task_id) else {
                continue;
            };
            for service in &task.depends_on_services_ready {
                if !required.contains(service.as_str()) {
                    return Err(ValidationError::UndeclaredReference {
                        reference_kind: "workflow.node.task.dependsOnServicesReady",
                        id: service.clone(),
                    });
                }
            }
        }
        validate_workflow_acyclic(id, &workflow.nodes)?;
    }
    Ok(())
}

/// Reject cycles via Kahn-style topological reduction.
fn validate_workflow_acyclic(
    workflow_id: &str,
    nodes: &[WorkflowNode],
) -> Result<(), ValidationError> {
    let mut remaining = nodes
        .iter()
        .map(|node| {
            (
                node.node_id.as_str(),
                node.depends_on
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        if ready.is_empty() {
            return Err(ValidationError::UnsupportedValue {
                field: "workflows.nodes.dependsOn",
                expected: "acyclic dependency graph",
                actual: workflow_id.to_string(),
            });
        }
        for id in ready {
            remaining.remove(id);
            for deps in remaining.values_mut() {
                deps.remove(id);
            }
        }
    }
    Ok(())
}

fn validate_target_capabilities(model: &Model) -> Result<(), ValidationError> {
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

/// A single live-workspace codebase named `main`. Multi-codebase source identity
/// is a later milestone; the runtime admission layer still resolves one root.
fn validate_codebases(model: &Model) -> Result<(), ValidationError> {
    if model.codebases.len() != 1 {
        return Err(ValidationError::ExpectedOne { field: "codebases" });
    }
    let codebase = &model.codebases[0];
    expect_string("codebases[0].codebaseId", "main", &codebase.codebase_id)?;
    require_non_empty("codebases[0].logicalRoot", &codebase.logical_root)?;
    if codebase.source_mode != SourceMode::LiveWorkspace {
        return Err(ValidationError::UnsupportedValue {
            field: "codebases[0].sourceMode",
            expected: "live-workspace",
            actual: format!("{:?}", codebase.source_mode),
        });
    }
    Ok(())
}

/// A single `dev` environment whose service/task references resolve (resolution
/// is enforced in `validate_references`). Multi-environment orchestration is a
/// later milestone.
fn validate_environments(model: &Model) -> Result<(), ValidationError> {
    if model.environments.len() != 1 {
        return Err(ValidationError::ExpectedOne {
            field: "environments",
        });
    }
    let env = model
        .environments
        .get("dev")
        .ok_or_else(|| ValidationError::UnsupportedValue {
            field: "environments",
            expected: "dev",
            actual: format!("{:?}", model.environments.keys().collect::<Vec<_>>()),
        })?;
    expect_string("environments.dev.environmentId", "dev", &env.environment_id)?;
    Ok(())
}

fn validate_runtime_constraints(model: &Model) -> Result<(), ValidationError> {
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
    Ok(())
}

fn validate_surfaces(model: &Model) -> Result<(), ValidationError> {
    let surface_names = model
        .surfaces
        .iter()
        .map(|surface| surface.name.clone())
        .collect::<Vec<_>>();
    expect_vec("surfaces", REQUIRED_SURFACES, &surface_names)?;
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
        if surface.maturity != SurfaceMaturity::Stable {
            return Err(ValidationError::UnsupportedValue {
                field: "surfaces.maturity",
                expected: "stable",
                actual: format!("{:?}", surface.maturity),
            });
        }
    }
    Ok(())
}

/// The `capabilities` section is a generated projection of the model and must
/// mirror it exactly (SINGLE-MODEL-1).
fn validate_capabilities(model: &Model) -> Result<(), ValidationError> {
    expect_vec(
        "capabilities.environments",
        &["dev"],
        &model.capabilities.environments,
    )?;
    let service_keys = model.services.keys().cloned().collect::<Vec<_>>();
    expect_string_vec(
        "capabilities.services",
        &service_keys,
        &model.capabilities.services,
    )?;
    let task_keys = model.tasks.keys().cloned().collect::<Vec<_>>();
    expect_string_vec("capabilities.tasks", &task_keys, &model.capabilities.tasks)?;
    let workflow_keys = model.workflows.keys().cloned().collect::<Vec<_>>();
    expect_string_vec(
        "capabilities.workflows",
        &workflow_keys,
        &model.capabilities.workflows,
    )?;
    let slots = expected_slots(&model.slot_policy)?;
    if model.capabilities.slots != slots {
        return Err(ValidationError::UnsupportedValue {
            field: "capabilities.slots",
            expected: "slotPolicy range",
            actual: format!("{:?}", model.capabilities.slots),
        });
    }
    let surface_names = model
        .surfaces
        .iter()
        .map(|surface| surface.name.clone())
        .collect::<Vec<_>>();
    expect_string_vec(
        "capabilities.surfaces",
        &surface_names,
        &model.capabilities.surfaces,
    )?;
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

/// At least one reusable exec; each binds a declared closure/codebase (resolved
/// in `validate_references`) and carries a positive timeout.
fn validate_execs(model: &Model) -> Result<(), ValidationError> {
    if model.execs.is_empty() {
        return Err(ValidationError::UnsupportedValue {
            field: "execs",
            expected: "at least one exec",
            actual: "{}".to_string(),
        });
    }
    for (id, exec) in &model.execs {
        if &exec.exec_id != id {
            return Err(ValidationError::UnsupportedValue {
                field: "execs.execId",
                expected: "map key",
                actual: format!("key={id}, execId={}", exec.exec_id),
            });
        }
        require_non_empty("execs.executable", &exec.executable)?;
        require_non_empty("execs.cwd", &exec.cwd)?;
        if exec.timeout_ms == 0 {
            return Err(ValidationError::UnsupportedValue {
                field: "execs.timeoutMs",
                expected: "positive timeout",
                actual: "0".to_string(),
            });
        }
    }
    Ok(())
}

/// At least one realised closure; each closure id is unique and non-empty.
/// Target compatibility and operation-binding resolution are checked in
/// `validate_references`.
fn validate_closures(model: &Model) -> Result<(), ValidationError> {
    if model.closures.is_empty() {
        return Err(ValidationError::UnsupportedValue {
            field: "closures",
            expected: "at least one closure",
            actual: "[]".to_string(),
        });
    }
    let mut seen = BTreeSet::new();
    for closure in &model.closures {
        require_non_empty("closures.closureId", &closure.closure_id)?;
        if !seen.insert(closure.closure_id.as_str()) {
            return Err(ValidationError::UnsupportedValue {
                field: "closures.closureId",
                expected: "unique closure ids",
                actual: closure.closure_id.clone(),
            });
        }
        require_non_empty("closures.executable", &closure.executable)?;
        require_non_empty("closures.storePath", &closure.store_path)?;
    }
    Ok(())
}

/// At least one service, each declaring the full generic lifecycle contract.
fn validate_services(model: &Model) -> Result<(), ValidationError> {
    if model.services.is_empty() {
        return Err(ValidationError::UnsupportedValue {
            field: "services",
            expected: "at least one service",
            actual: "{}".to_string(),
        });
    }
    for (id, service) in &model.services {
        if &service.service_id != id {
            return Err(ValidationError::UnsupportedValue {
                field: "services.serviceId",
                expected: "map key",
                actual: format!("key={id}, serviceId={}", service.service_id),
            });
        }
        if !service.foreground {
            return Err(ValidationError::UnsupportedValue {
                field: "services.foreground",
                expected: "true",
                actual: "false".to_string(),
            });
        }
        if service.lifetime != ServiceLifetime::RunScoped {
            return Err(ValidationError::UnsupportedValue {
                field: "services.lifetime",
                expected: "run-scoped",
                actual: format!("{:?}", service.lifetime),
            });
        }
        match service.containment {
            ContainmentRequirement::ProcessGroup | ContainmentRequirement::ProcessTree => {}
        }
        if service.health_policy != HealthPolicy::Explicit {
            return Err(ValidationError::UnsupportedValue {
                field: "services.healthPolicy",
                expected: "explicit",
                actual: format!("{:?}", service.health_policy),
            });
        }
        if service.endpoints.is_empty() {
            return Err(ValidationError::UnsupportedValue {
                field: "services.endpoints",
                expected: "at least one endpoint",
                actual: "[]".to_string(),
            });
        }
        if service.probes.is_empty() {
            return Err(ValidationError::UnsupportedValue {
                field: "services.probes",
                expected: "at least one probe",
                actual: "[]".to_string(),
            });
        }
        require_non_empty("services.readinessProbe", &service.readiness_probe)?;
        for endpoint in &service.endpoints {
            validate_endpoint(model, endpoint)?;
        }
        for probe in &service.probes {
            validate_probe(probe)?;
        }
        validate_stop_policy(&service.stop_policy)?;
        validate_service_lifecycle(service)?;
    }
    Ok(())
}

/// Stop is a signal-based runtime primitive, so the declared signal must be one
/// the runtime can send and the graceful budget must be positive.
const STOP_SIGNALS: &[&str] = &["TERM", "INT", "QUIT", "HUP"];

fn validate_stop_policy(stop_policy: &StopPolicy) -> Result<(), ValidationError> {
    if !STOP_SIGNALS.contains(&stop_policy.signal.as_str()) {
        return Err(ValidationError::UnsupportedValue {
            field: "services.stopPolicy.signal",
            expected: "one of TERM, INT, QUIT, HUP",
            actual: stop_policy.signal.clone(),
        });
    }
    if stop_policy.timeout_ms == 0 {
        return Err(ValidationError::UnsupportedValue {
            field: "services.stopPolicy.timeoutMs",
            expected: "positive timeout",
            actual: "0".to_string(),
        });
    }
    Ok(())
}

fn validate_endpoint(model: &Model, endpoint: &EndpointSpec) -> Result<(), ValidationError> {
    require_non_empty("services.endpoints.endpointId", &endpoint.endpoint_id)?;
    if endpoint.protocol != EndpointProtocol::Tcp {
        return Err(ValidationError::UnsupportedValue {
            field: "services.endpoints.protocol",
            expected: "tcp",
            actual: format!("{:?}", endpoint.protocol),
        });
    }
    require_non_empty("services.endpoints.host", &endpoint.host)?;
    match &endpoint.port {
        PortPolicy::Fixed { port } => {
            if *port == 0 {
                return Err(ValidationError::UnsupportedValue {
                    field: "services.endpoints.port",
                    expected: "non-zero fixed port",
                    actual: "0".to_string(),
                });
            }
        }
        PortPolicy::CandidateWindow { start, end } => {
            if *start != model.placement.candidate_ports.start
                || *end != model.placement.candidate_ports.end
            {
                return Err(ValidationError::UnsupportedValue {
                    field: "services.endpoints.port",
                    expected: "placement candidate window",
                    actual: format!("start={start}, end={end}"),
                });
            }
        }
    }
    if endpoint.ownership_verification != OwnershipVerification::Required {
        return Err(ValidationError::UnsupportedValue {
            field: "services.endpoints.ownershipVerification",
            expected: "required",
            actual: format!("{:?}", endpoint.ownership_verification),
        });
    }
    if endpoint.socket_activation != SocketActivation::Disabled {
        return Err(ValidationError::UnsupportedValue {
            field: "services.endpoints.socketActivation",
            expected: "disabled",
            actual: format!("{:?}", endpoint.socket_activation),
        });
    }
    Ok(())
}

fn validate_probe(probe: &ProbeSpec) -> Result<(), ValidationError> {
    require_non_empty("services.probes.probeId", &probe.probe_id)?;
    // The current runtime ABI implements tcp-connect probes only. Reject http-get
    // at admission so an advertised primitive cannot compile into a model that
    // always fails its readiness/health probe at runtime.
    if let ProbeTarget::HttpGet { .. } = probe.target {
        return Err(ValidationError::UnsupportedValue {
            field: "services.probes.target.kind",
            expected: "tcp-connect",
            actual: "http-get".to_string(),
        });
    }
    if probe.max_attempts == 0 || probe.timeout_ms == 0 || probe.retry_interval_ms == 0 {
        return Err(ValidationError::UnsupportedValue {
            field: "services.probes.timing",
            expected: "positive timeoutMs, retryIntervalMs, maxAttempts",
            actual: format!(
                "timeoutMs={}, retryIntervalMs={}, maxAttempts={}",
                probe.timeout_ms, probe.retry_interval_ms, probe.max_attempts
            ),
        });
    }
    Ok(())
}

/// Each service declares exactly one lifecycle operation per class with unique
/// operation ids, and each class binds the generic primitive its semantics
/// require:
/// - prepare: optional exec, no probe (e.g. data-dir init);
/// - start:   required exec, no probe;
/// - ready:   the readiness probe, no exec;
/// - health:  a probe, no exec;
/// - stop:    required exec, no probe;
/// - clean:   neither exec nor probe (marker-gated runtime cleanup).
fn validate_service_lifecycle(service: &ServiceSpec) -> Result<(), ValidationError> {
    let mut seen_ids = BTreeSet::new();
    for op in &service.lifecycle {
        require_non_empty("lifecycle.operationId", &op.operation_id)?;
        if !seen_ids.insert(op.operation_id.as_str()) {
            return Err(ValidationError::UnsupportedValue {
                field: "lifecycle.operationId",
                expected: "unique operation IDs",
                actual: op.operation_id.clone(),
            });
        }
        require_non_empty("lifecycle.terminal.success", &op.terminal.success)?;
        require_non_empty("lifecycle.terminal.failure", &op.terminal.failure)?;
    }

    let mut by_class: BTreeMap<&str, &LifecycleOpSpec> = BTreeMap::new();
    for op in &service.lifecycle {
        let key = class_name(&op.class);
        if by_class.insert(key, op).is_some() {
            return Err(ValidationError::UnsupportedValue {
                field: "lifecycle.class",
                expected: "exactly one operation per class",
                actual: key.to_string(),
            });
        }
    }
    for class in LIFECYCLE_CLASSES {
        if !by_class.contains_key(class_name(class)) {
            return Err(ValidationError::UnsupportedValue {
                field: "lifecycle.class",
                expected: "full generic lifecycle class set",
                actual: format!("missing {}", class_name(class)),
            });
        }
    }

    let prepare = by_class[class_name(&LifecycleOpClass::Prepare)];
    require_no_probe("lifecycle.prepare.probeId", prepare)?;

    let start = by_class[class_name(&LifecycleOpClass::Start)];
    require_exec("lifecycle.start.execId", start)?;
    require_no_probe("lifecycle.start.probeId", start)?;

    let ready = by_class[class_name(&LifecycleOpClass::Ready)];
    require_no_exec("lifecycle.ready.execId", ready)?;
    match ready.probe_id.as_deref() {
        Some(probe_id) if probe_id == service.readiness_probe => {}
        other => {
            return Err(ValidationError::UnsupportedValue {
                field: "lifecycle.ready.probeId",
                expected: "the service readinessProbe",
                actual: other.unwrap_or("null").to_string(),
            });
        }
    }

    let health = by_class[class_name(&LifecycleOpClass::Health)];
    require_no_exec("lifecycle.health.execId", health)?;
    if health.probe_id.is_none() {
        return Err(ValidationError::UnsupportedValue {
            field: "lifecycle.health.probeId",
            expected: "a declared probe",
            actual: "null".to_string(),
        });
    }

    let stop = by_class[class_name(&LifecycleOpClass::Stop)];
    require_exec("lifecycle.stop.execId", stop)?;
    require_no_probe("lifecycle.stop.probeId", stop)?;

    let clean = by_class[class_name(&LifecycleOpClass::Clean)];
    require_no_exec("lifecycle.clean.execId", clean)?;
    require_no_probe("lifecycle.clean.probeId", clean)?;

    Ok(())
}

fn require_exec(field: &'static str, op: &LifecycleOpSpec) -> Result<(), ValidationError> {
    if op.exec_id.is_none() {
        return Err(ValidationError::UnsupportedValue {
            field,
            expected: "a bound exec",
            actual: "null".to_string(),
        });
    }
    Ok(())
}

fn require_no_exec(field: &'static str, op: &LifecycleOpSpec) -> Result<(), ValidationError> {
    if let Some(exec_id) = &op.exec_id {
        return Err(ValidationError::UnsupportedValue {
            field,
            expected: "null",
            actual: exec_id.clone(),
        });
    }
    Ok(())
}

fn require_no_probe(field: &'static str, op: &LifecycleOpSpec) -> Result<(), ValidationError> {
    if let Some(probe_id) = &op.probe_id {
        return Err(ValidationError::UnsupportedValue {
            field,
            expected: "null",
            actual: probe_id.clone(),
        });
    }
    Ok(())
}

fn validate_tasks(model: &Model) -> Result<(), ValidationError> {
    for (id, task) in &model.tasks {
        if &task.task_id != id {
            return Err(ValidationError::UnsupportedValue {
                field: "tasks.taskId",
                expected: "map key",
                actual: format!("key={id}, taskId={}", task.task_id),
            });
        }
        require_non_empty("tasks.operationId", &task.operation_id)?;
        require_non_empty("tasks.execId", &task.exec_id)?;
        if task.exit_policy.success_codes.is_empty() {
            return Err(ValidationError::UnsupportedValue {
                field: "tasks.exitPolicy.successCodes",
                expected: "at least one success code",
                actual: "[]".to_string(),
            });
        }
    }
    Ok(())
}

fn class_name(class: &LifecycleOpClass) -> &'static str {
    match class {
        LifecycleOpClass::Prepare => "prepare",
        LifecycleOpClass::Start => "start",
        LifecycleOpClass::Ready => "ready",
        LifecycleOpClass::Health => "health",
        LifecycleOpClass::Stop => "stop",
        LifecycleOpClass::Clean => "clean",
    }
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
            expected: "exact values",
            actual: format!("{actual:?}"),
        })
    }
}

fn expect_string_vec(
    field: &'static str,
    expected: &[String],
    actual: &[String],
) -> Result<(), ValidationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ValidationError::UnsupportedValue {
            field,
            expected: "model-derived projection",
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
        // Endpoint and probe ids are service-local: a service may reference only
        // the probes/endpoints it declares, matching how the runtime resolves
        // them within the ServiceSpec. Fail closed - a cross-service reference is
        // rejected at admission, never surfaced at execution.
        let mut endpoint_ids = BTreeSet::new();
        for endpoint in &service.endpoints {
            endpoint_ids.insert(endpoint.endpoint_id.as_str());
        }
        let mut probe_ids = BTreeSet::new();
        for probe in &service.probes {
            probe_ids.insert(probe.probe_id.as_str());
        }
        for lifecycle in &service.lifecycle {
            if !declared_operations.insert(lifecycle.operation_id.as_str()) {
                return Err(ValidationError::UnsupportedValue {
                    field: "lifecycle.operationId",
                    expected: "globally unique operation IDs",
                    actual: lifecycle.operation_id.clone(),
                });
            }
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
    }

    for task in model.tasks.values() {
        if !declared_operations.insert(task.operation_id.as_str()) {
            return Err(ValidationError::UnsupportedValue {
                field: "task.operationId",
                expected: "globally unique operation IDs",
                actual: task.operation_id.clone(),
            });
        }
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
