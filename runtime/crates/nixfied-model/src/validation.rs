use std::collections::{BTreeMap, BTreeSet};

use crate::constants::{MODEL_VERSION, RUNTIME_ABI, TOOLCHAIN_ID};
use crate::error::ValidationError;
use crate::types::*;


pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

impl Validate for Model {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_exact_identities(self)?;
        validate_required_strings(self)?;
        validate_codebases(self)?;
        validate_environments(self)?;
        validate_slot_policy(&self.slot_policy)?;
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
                actual: "{}".to_string(),
            });
        }
        // Node ids are unique by construction (map keys). Resolve task references.
        for node in workflow.nodes.values() {
            if !task_ids.contains(node.task_id.as_str()) {
                return Err(ValidationError::UndeclaredReference {
                    reference_kind: "workflow.node.taskId",
                    id: node.task_id.clone(),
                });
            }
        }
        for node in workflow.nodes.values() {
            for dependency in &node.depends_on {
                if !workflow.nodes.contains_key(dependency) {
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
        for node in workflow.nodes.values() {
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
    nodes: &BTreeMap<String, WorkflowNode>,
) -> Result<(), ValidationError> {
    let mut remaining = nodes
        .iter()
        .map(|(node_id, node)| {
            (
                node_id.as_str(),
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
    if !model.environments.contains_key("dev") {
        return Err(ValidationError::UnsupportedValue {
            field: "environments",
            expected: "dev",
            actual: format!("{:?}", model.environments.keys().collect::<Vec<_>>()),
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
    for exec in model.execs.values() {
        require_non_empty("execs.executable", &exec.executable)?;
        require_non_empty("execs.cwd", &exec.cwd)?;
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
            actual: "{}".to_string(),
        });
    }
    // Closure ids are unique by construction (map keys).
    for closure in model.closures.values() {
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
    for service in model.services.values() {
        match service.containment {
            ContainmentRequirement::ProcessGroup | ContainmentRequirement::ProcessTree => {}
        }
        require_non_empty("services.endpoint.endpointId", &service.endpoint.endpoint_id)?;
        validate_service_lifecycle(service)?;
    }
    Ok(())
}

/// The lifecycle's per-class shape and the endpoint/probe wiring are now
/// guaranteed by the types (single endpoint, inline probe timings, loopback
/// host), so only non-empty value checks on ids/terminals remain.
fn validate_service_lifecycle(service: &ServiceSpec) -> Result<(), ValidationError> {
    for (operation_id, terminal) in lifecycle_ops(&service.lifecycle) {
        require_non_empty("lifecycle.operationId", operation_id)?;
        require_non_empty("lifecycle.terminal.success", &terminal.success)?;
        require_non_empty("lifecycle.terminal.failure", &terminal.failure)?;
    }
    Ok(())
}

/// The (operationId, terminal) pair of each lifecycle op, in canonical order.
fn lifecycle_ops(lifecycle: &Lifecycle) -> [(&str, &TerminalSemantics); 6] {
    [
        (&lifecycle.prepare.operation_id, &lifecycle.prepare.terminal),
        (&lifecycle.start.operation_id, &lifecycle.start.terminal),
        (&lifecycle.ready.operation_id, &lifecycle.ready.terminal),
        (&lifecycle.health.operation_id, &lifecycle.health.terminal),
        (&lifecycle.stop.operation_id, &lifecycle.stop.terminal),
        (&lifecycle.clean.operation_id, &lifecycle.clean.terminal),
    ]
}

fn validate_tasks(model: &Model) -> Result<(), ValidationError> {
    for task in model.tasks.values() {
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
        .keys()
        .map(String::as_str)
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

    for (closure_id, closure) in &model.closures {
        if closure.target_system != model.target.closure_system {
            return Err(ValidationError::ClosureTargetMismatch {
                closure_id: closure_id.clone(),
                target_system: closure.target_system.clone(),
                closure_system: model.target.closure_system.clone(),
            });
        }
    }

    for service in model.services.values() {
        let lifecycle = &service.lifecycle;
        for (operation_id, _) in lifecycle_ops(lifecycle) {
            if !declared_operations.insert(operation_id) {
                return Err(ValidationError::UnsupportedValue {
                    field: "lifecycle.operationId",
                    expected: "globally unique operation IDs",
                    actual: operation_id.to_string(),
                });
            }
        }
        for exec_id in [
            lifecycle.prepare.exec_id.as_deref(),
            Some(lifecycle.start.exec_id.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            if !exec_ids.contains(exec_id) {
                return Err(ValidationError::UndeclaredReference {
                    reference_kind: "lifecycle.execId",
                    id: exec_id.to_string(),
                });
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
        let env_services = env
            .services
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
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
            // A task the environment runs can only depend on services the
            // environment starts; otherwise the run fails mid-flight on a missing
            // dependency. Mirrors the workflow servicesRequired check.
            if let Some(task) = model.tasks.get(task_id) {
                for service in &task.depends_on_services_ready {
                    if !env_services.contains(service.as_str()) {
                        return Err(ValidationError::UndeclaredReference {
                            reference_kind: "environment.task.dependsOnServicesReady",
                            id: service.clone(),
                        });
                    }
                }
            }
        }
    }

    for closure in model.closures.values() {
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
